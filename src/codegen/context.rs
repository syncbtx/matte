use crate::ir::lifted;
use crate::resolve::GlobalId;
use cranelift::codegen::ir::{AbiParam, Signature, types};
use cranelift::codegen::isa::CallConv;
use cranelift::codegen::settings::{self, Configurable};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module, default_libcall_names};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::collections::HashMap;

pub struct FuncEntry {
    pub stages: Vec<FuncId>,
    pub n_captures: usize,
    pub n_params: usize,
}

pub struct CodegenCtx {
    pub module: ObjectModule,
    pub call_conv: CallConv,
    pub matte_fn_sig: Signature,
    pub func_entries: Vec<FuncEntry>,
    pub global_slots: HashMap<GlobalId, DataId>,
    pub matte_alloc_id: FuncId,
    pub matte_print_id: FuncId,
    pub puts_id: FuncId,
    pub matte_pow_id: FuncId,
    pub matte_fmod_id: FuncId,
    pub matte_fact_id: FuncId,
    pub next_str_id: usize,
}

impl CodegenCtx {
    pub fn new(program: &lifted::Program) -> Self {
        let mut flag_builder = settings::builder();
        flag_builder.set("is_pic", "true").unwrap();
        let isa_builder = cranelift::native::builder().unwrap();
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .unwrap();

        let mut module =
            ObjectModule::new(ObjectBuilder::new(isa, "matte", default_libcall_names()).unwrap());
        let call_conv = module.isa().default_call_conv();

        let mut matte_fn_sig = Signature::new(call_conv);
        matte_fn_sig.params.push(AbiParam::new(types::I64)); // env_ptr
        matte_fn_sig.params.push(AbiParam::new(types::I64)); // arg_tag
        matte_fn_sig.params.push(AbiParam::new(types::I64)); // arg_payload
        matte_fn_sig.returns.push(AbiParam::new(types::I64)); // ret_tag
        matte_fn_sig.returns.push(AbiParam::new(types::I64)); // ret_payload

        // Runtime imports
        let mut alloc_sig = Signature::new(call_conv);
        alloc_sig.params.push(AbiParam::new(types::I64));
        alloc_sig.params.push(AbiParam::new(types::I64));
        alloc_sig.returns.push(AbiParam::new(types::I64));
        let matte_alloc_id = module
            .declare_function("matte_alloc", Linkage::Import, &alloc_sig)
            .unwrap();

        let mut print_sig = Signature::new(call_conv);
        print_sig.params.push(AbiParam::new(types::I64));
        print_sig.params.push(AbiParam::new(types::I64));
        let matte_print_id = module
            .declare_function("matte_print", Linkage::Import, &print_sig)
            .unwrap();

        let mut puts_sig = Signature::new(call_conv);
        puts_sig.params.push(AbiParam::new(types::I64));
        puts_sig.returns.push(AbiParam::new(types::I32));
        let puts_id = module
            .declare_function("puts", Linkage::Import, &puts_sig)
            .unwrap();

        let mut math_sig = Signature::new(call_conv);
        math_sig.params.push(AbiParam::new(types::I64));
        math_sig.params.push(AbiParam::new(types::I64));
        math_sig.returns.push(AbiParam::new(types::I64));
        let matte_pow_id = module
            .declare_function("matte_pow", Linkage::Import, &math_sig.clone())
            .unwrap();
        let matte_fmod_id = module
            .declare_function("matte_fmod", Linkage::Import, &math_sig)
            .unwrap();

        let mut fact_sig = Signature::new(call_conv);
        fact_sig.params.push(AbiParam::new(types::I64));
        fact_sig.returns.push(AbiParam::new(types::I64));
        let matte_fact_id = module
            .declare_function("matte_fact", Linkage::Import, &fact_sig)
            .unwrap();

        let mut global_slots = HashMap::new();
        for (gid, _name, _) in program.slots {
            let data_name = format!("matte_global_{}", gid.0);
            let data_id = module
                .declare_data(&data_name, Linkage::Local, true, false)
                .unwrap();
            let mut desc = DataDescription::new();
            desc.define_zeroinit(16);
            module.define_data(data_id, &desc).unwrap();
            global_slots.insert(*gid, data_id);
        }

        let mut max_func_id = 0;
        for func in program.funcs {
            if func.id.0 > max_func_id {
                max_func_id = func.id.0;
            }
        }

        let mut func_entries: Vec<FuncEntry> = Vec::new();
        for _ in 0..=max_func_id {
            func_entries.push(FuncEntry {
                stages: Vec::new(),
                n_captures: 0,
                n_params: 0,
            });
        }

        for func in program.funcs {
            let mut stages = Vec::new();
            let np = func.params.len();
            for k in 0..np {
                let name = if np == 1 {
                    format!("matte_f{}", func.id.0)
                } else {
                    format!("matte_f{}_s{}", func.id.0, k)
                };
                let id = module
                    .declare_function(&name, Linkage::Local, &matte_fn_sig)
                    .unwrap();
                stages.push(id);
            }
            func_entries[func.id.0] = FuncEntry {
                stages,
                n_captures: func.captures.len(),
                n_params: np,
            };
        }

        Self {
            module,
            call_conv,
            matte_fn_sig,
            func_entries,
            global_slots,
            matte_alloc_id,
            matte_print_id,
            puts_id,
            matte_pow_id,
            matte_fmod_id,
            matte_fact_id,
            next_str_id: 0,
        }
    }
}
