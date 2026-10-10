use std::collections::HashMap;
use cranelift::codegen::ir::types::I64;
use cranelift::codegen::ir::Value;
use cranelift::frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift::prelude::InstBuilder;
use cranelift_module::Module;

use crate::codegen::context::CodegenCtx;
use crate::codegen::expr;
use crate::codegen::ty::{alloc_closure, load_cap, make_closure_pair};
use crate::ir::lifted;
use crate::resolve::LocalId;

pub fn emit_all_funcs(ctx: &mut CodegenCtx, program: &lifted::Program) {
    for func in program.funcs {
        emit_func(ctx, func);
    }
}

pub fn emit_func(ctx: &mut CodegenCtx, func: &lifted::Func) {
    let n_params = func.params.len();
    for k in 0..n_params {
        let cranelift_func_id = ctx.func_entries[func.id.0].stages[k];
        let mut cg_ctx = ctx.module.make_context();
        cg_ctx.func.signature = ctx.matte_fn_sig.clone();
        let mut fb_ctx = FunctionBuilderContext::new();
        {
            let mut bcx = FunctionBuilder::new(&mut cg_ctx.func, &mut fb_ctx);
            let entry = bcx.create_block();
            bcx.append_block_param(entry, I64); // env_ptr
            bcx.append_block_param(entry, I64); // arg_tag
            bcx.append_block_param(entry, I64); // arg_payload
            bcx.switch_to_block(entry);
            bcx.seal_block(entry);
            
            let env_ptr = bcx.block_params(entry)[0];
            let arg_tag = bcx.block_params(entry)[1];
            let arg_payload = bcx.block_params(entry)[2];
            
            let c = func.captures.len();
            let mut locals: HashMap<LocalId, (Value, Value)> = HashMap::new();
            
            // Load original captures from env[0..C]
            for (i, (local_id, _)) in func.captures.iter().enumerate() {
                let (t, p) = load_cap(&mut bcx, env_ptr, i);
                locals.insert(*local_id, (t, p));
            }
            // Load previously-applied params from env[C..C+k]
            for j in 0..k {
                let (local_id, _) = func.params[j];
                let (t, p) = load_cap(&mut bcx, env_ptr, c + j);
                locals.insert(local_id, (t, p));
            }
            // Bind current arg to params[k]
            let (local_id_k, _) = func.params[k];
            locals.insert(local_id_k, (arg_tag, arg_payload));
            
            if k < n_params - 1 {
                let next_stage_id = ctx.func_entries[func.id.0].stages[k + 1];
                let func_ref = ctx.module.declare_func_in_func(next_stage_id, bcx.func);
                let fn_ptr = bcx.ins().func_addr(I64, func_ref);
                
                let mut caps: Vec<(Value, Value)> = Vec::new();
                for i in 0..c {
                    caps.push(locals[&func.captures[i].0]);
                }
                for j in 0..k {
                    caps.push(locals[&func.params[j].0]);
                }
                caps.push((arg_tag, arg_payload));
                
                let ptr = alloc_closure(&mut ctx.module, &mut bcx, ctx.matte_alloc_id, fn_ptr, &caps);
                let (tag, payload) = make_closure_pair(&mut bcx, ptr);
                bcx.ins().return_(&[tag, payload]);
            } else {
                let (ret_tag, ret_payload) = expr::translate(ctx, &mut bcx, &locals, &func.body);
                bcx.ins().return_(&[ret_tag, ret_payload]);
            }
            bcx.finalize(ctx.module.isa().frontend_config());
        }
        ctx.module.define_function(cranelift_func_id, &mut cg_ctx).unwrap();
        ctx.module.clear_context(&mut cg_ctx);
    }
}
