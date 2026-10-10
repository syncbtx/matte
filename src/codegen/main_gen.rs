use cranelift::codegen::ir::types::I64;
use cranelift::codegen::ir::{Signature, Value};
use cranelift::frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift::prelude::InstBuilder;
use cranelift_module::{DataDescription, DataId, Linkage, Module};
use std::collections::HashMap;

use crate::codegen::context::CodegenCtx;
use crate::codegen::expr;
use crate::ir::lifted;
use crate::ir::lifted::Stmt;
use crate::resolve::LocalId;

pub fn emit_main(ctx: &mut CodegenCtx, program: &lifted::Program) {
    let mut str_data_ids: Vec<DataId> = Vec::new();
    for stmt in program.main {
        if let Stmt::PrintType(s) = stmt {
            let full = format!("typeof: {s}\0");
            let name = format!("matte_str_{}", ctx.next_str_id);
            ctx.next_str_id += 1;
            let mut desc = DataDescription::new();
            desc.define(full.into_bytes().into_boxed_slice());
            let data_id = ctx
                .module
                .declare_data(&name, Linkage::Local, false, false)
                .unwrap();
            ctx.module.define_data(data_id, &desc).unwrap();
            str_data_ids.push(data_id);
        }
    }

    let sig = Signature::new(ctx.call_conv);
    let matte_main_id = ctx
        .module
        .declare_function("matte_main", Linkage::Export, &sig)
        .unwrap();
    let mut cg_ctx = ctx.module.make_context();
    cg_ctx.func.signature = sig;
    let mut fb_ctx = FunctionBuilderContext::new();
    {
        let mut bcx = FunctionBuilder::new(&mut cg_ctx.func, &mut fb_ctx);
        let entry = bcx.create_block();
        bcx.switch_to_block(entry);
        bcx.seal_block(entry);

        let locals: HashMap<LocalId, (Value, Value)> = HashMap::new();
        let mut str_idx = 0usize;

        for stmt in program.main {
            match stmt {
                Stmt::SetGlobal(gid, expr) => {
                    let (tag, payload) = expr::translate(ctx, &mut bcx, &locals, expr);
                    let data_id = ctx.global_slots[gid];
                    let gv = ctx.module.declare_data_in_func(data_id, bcx.func);
                    let addr = bcx.ins().symbol_value(I64, gv);
                    bcx.ins()
                        .store(cranelift::codegen::ir::MemFlagsData::new(), tag, addr, 0);
                    bcx.ins().store(
                        cranelift::codegen::ir::MemFlagsData::new(),
                        payload,
                        addr,
                        8,
                    );
                }
                Stmt::Print(expr) => {
                    let (tag, payload) = expr::translate(ctx, &mut bcx, &locals, expr);
                    let fref = ctx
                        .module
                        .declare_func_in_func(ctx.matte_print_id, bcx.func);
                    bcx.ins().call(fref, &[tag, payload]);
                }
                Stmt::PrintType(_) => {
                    let data_id = str_data_ids[str_idx];
                    str_idx += 1;
                    let gv = ctx.module.declare_data_in_func(data_id, bcx.func);
                    let ptr = bcx.ins().symbol_value(I64, gv);
                    let fref = ctx.module.declare_func_in_func(ctx.puts_id, bcx.func);
                    bcx.ins().call(fref, &[ptr]);
                }
                Stmt::Eval(expr) => {
                    expr::translate(ctx, &mut bcx, &locals, expr);
                }
            }
        }
        bcx.ins().return_(&[]);
        bcx.finalize(ctx.module.isa().frontend_config());
    }
    ctx.module
        .define_function(matte_main_id, &mut cg_ctx)
        .unwrap();
    ctx.module.clear_context(&mut cg_ctx);
}
