use cranelift::codegen::ir::{Value, types};
use cranelift::frontend::FunctionBuilder;
use cranelift::prelude::InstBuilder;
use cranelift_module::{FuncId, Module};
use cranelift_object::ObjectModule;

pub const TAG_NUM: i64 = 0;
pub const TAG_BOOL: i64 = 1;
pub const TAG_CLOSURE: i64 = 2;
pub const TAG_UNIT: i64 = 3;

pub const CLOSURE_FN_PTR_OFF: i32 = 0;
pub const CLOSURE_N_CAPS_OFF: i32 = 8;
pub const CLOSURE_CAPS_BASE: i32 = 16;
pub const CAP_STRIDE: i32 = 16;

pub fn cap_tag_offset(i: usize) -> i32 {
    CLOSURE_CAPS_BASE + (i as i32) * CAP_STRIDE
}

pub fn cap_payload_offset(i: usize) -> i32 {
    CLOSURE_CAPS_BASE + (i as i32) * CAP_STRIDE + 8
}

pub fn make_num(bcx: &mut FunctionBuilder, f64_val: Value) -> (Value, Value) {
    let tag = bcx.ins().iconst(types::I64, TAG_NUM);
    let payload = bcx.ins().bitcast(
        types::I64,
        cranelift::codegen::ir::MemFlagsData::new(),
        f64_val,
    );
    (tag, payload)
}

pub fn make_bool(bcx: &mut FunctionBuilder, i8_val: Value) -> (Value, Value) {
    let tag = bcx.ins().iconst(types::I64, TAG_BOOL);
    let payload = bcx.ins().uextend(types::I64, i8_val);
    (tag, payload)
}

pub fn make_unit(bcx: &mut FunctionBuilder) -> (Value, Value) {
    let tag = bcx.ins().iconst(types::I64, TAG_UNIT);
    let payload = bcx.ins().iconst(types::I64, 0);
    (tag, payload)
}

pub fn make_closure_pair(bcx: &mut FunctionBuilder, ptr: Value) -> (Value, Value) {
    let tag = bcx.ins().iconst(types::I64, TAG_CLOSURE);
    (tag, ptr)
}

pub fn i64_to_f64(bcx: &mut FunctionBuilder, v: Value) -> Value {
    bcx.ins()
        .bitcast(types::F64, cranelift::codegen::ir::MemFlagsData::new(), v)
}

pub fn f64_to_i64(bcx: &mut FunctionBuilder, v: Value) -> Value {
    bcx.ins()
        .bitcast(types::I64, cranelift::codegen::ir::MemFlagsData::new(), v)
}

pub fn load_cap(bcx: &mut FunctionBuilder, env: Value, i: usize) -> (Value, Value) {
    let tag = bcx.ins().load(
        types::I64,
        cranelift::codegen::ir::MemFlagsData::new(),
        env,
        cap_tag_offset(i),
    );
    let payload = bcx.ins().load(
        types::I64,
        cranelift::codegen::ir::MemFlagsData::new(),
        env,
        cap_payload_offset(i),
    );
    (tag, payload)
}

pub fn store_cap(bcx: &mut FunctionBuilder, ptr: Value, i: usize, tag: Value, payload: Value) {
    bcx.ins().store(
        cranelift::codegen::ir::MemFlagsData::new(),
        tag,
        ptr,
        cap_tag_offset(i),
    );
    bcx.ins().store(
        cranelift::codegen::ir::MemFlagsData::new(),
        payload,
        ptr,
        cap_payload_offset(i),
    );
}

pub fn alloc_closure(
    module: &mut ObjectModule,
    bcx: &mut FunctionBuilder,
    matte_alloc_id: FuncId,
    fn_ptr: Value,
    captures: &[(Value, Value)],
) -> Value {
    let n_caps_val = bcx.ins().iconst(types::I64, captures.len() as i64);
    let alloc_ref = module.declare_func_in_func(matte_alloc_id, bcx.func);
    let call = bcx.ins().call(alloc_ref, &[fn_ptr, n_caps_val]);
    let ptr = bcx.inst_results(call)[0];

    for (i, &(tag, payload)) in captures.iter().enumerate() {
        store_cap(bcx, ptr, i, tag, payload);
    }

    ptr
}
