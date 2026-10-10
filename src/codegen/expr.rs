use cranelift::codegen::ir::Value;
use cranelift::codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift::codegen::ir::types::I64;
use cranelift::frontend::FunctionBuilder;
use cranelift::prelude::InstBuilder;
use cranelift_module::{FuncId, Module};
use std::collections::HashMap;

use crate::codegen::context::CodegenCtx;
use crate::codegen::ty::{
    alloc_closure, i64_to_f64, make_bool, make_closure_pair, make_num, make_unit,
};
use crate::ir::core::PrimOp;
use crate::ir::lifted::{Expr, ExprKind};
use crate::resolve::LocalId;

fn call_runtime_ii_i(
    ctx: &mut CodegenCtx,
    bcx: &mut FunctionBuilder,
    func_id: FuncId,
    a: Value,
    b: Value,
) -> Value {
    let fref = ctx.module.declare_func_in_func(func_id, bcx.func);
    let call = bcx.ins().call(fref, &[a, b]);
    bcx.inst_results(call)[0]
}

fn call_runtime_i_i(
    ctx: &mut CodegenCtx,
    bcx: &mut FunctionBuilder,
    func_id: FuncId,
    a: Value,
) -> Value {
    let fref = ctx.module.declare_func_in_func(func_id, bcx.func);
    let call = bcx.ins().call(fref, &[a]);
    bcx.inst_results(call)[0]
}

pub fn translate(
    ctx: &mut CodegenCtx,
    bcx: &mut FunctionBuilder,
    locals: &HashMap<LocalId, (Value, Value)>,
    expr: &Expr,
) -> (Value, Value) {
    match &expr.kind {
        ExprKind::Unit => make_unit(bcx),
        ExprKind::Num(n) => {
            let f_val = bcx.ins().f64const(*n);
            make_num(bcx, f_val)
        }
        ExprKind::Bool(b) => {
            let val = bcx
                .ins()
                .iconst(cranelift::codegen::ir::types::I8, if *b { 1 } else { 0 });
            make_bool(bcx, val)
        }
        ExprKind::Local(lid) => *locals.get(lid).expect("local not found"),
        ExprKind::Global(gid) => {
            let data_id = ctx.global_slots[gid];
            let gv = ctx.module.declare_data_in_func(data_id, bcx.func);
            let addr = bcx.ins().symbol_value(I64, gv);
            let tag = bcx
                .ins()
                .load(I64, cranelift::codegen::ir::MemFlagsData::new(), addr, 0);
            let payload = bcx
                .ins()
                .load(I64, cranelift::codegen::ir::MemFlagsData::new(), addr, 8);
            (tag, payload)
        }
        ExprKind::Prim { op, args } => {
            let arg_vals: Vec<_> = args
                .iter()
                .map(|a| translate(ctx, bcx, locals, a))
                .collect();
            match op {
                PrimOp::Add | PrimOp::Sub | PrimOp::Mul | PrimOp::Div => {
                    let a_f64 = i64_to_f64(bcx, arg_vals[0].1);
                    let b_f64 = i64_to_f64(bcx, arg_vals[1].1);
                    let res = match op {
                        PrimOp::Add => bcx.ins().fadd(a_f64, b_f64),
                        PrimOp::Sub => bcx.ins().fsub(a_f64, b_f64),
                        PrimOp::Mul => bcx.ins().fmul(a_f64, b_f64),
                        PrimOp::Div => bcx.ins().fdiv(a_f64, b_f64),
                        _ => unreachable!(),
                    };
                    make_num(bcx, res)
                }
                PrimOp::Pow => {
                    let res =
                        call_runtime_ii_i(ctx, bcx, ctx.matte_pow_id, arg_vals[0].1, arg_vals[1].1);
                    let tag = bcx.ins().iconst(I64, crate::codegen::ty::TAG_NUM);
                    (tag, res)
                }
                PrimOp::Mod => {
                    let res = call_runtime_ii_i(
                        ctx,
                        bcx,
                        ctx.matte_fmod_id,
                        arg_vals[0].1,
                        arg_vals[1].1,
                    );
                    let tag = bcx.ins().iconst(I64, crate::codegen::ty::TAG_NUM);
                    (tag, res)
                }
                PrimOp::Neg => {
                    let a_f64 = i64_to_f64(bcx, arg_vals[0].1);
                    let res = bcx.ins().fneg(a_f64);
                    make_num(bcx, res)
                }
                PrimOp::Fact => {
                    let res = call_runtime_i_i(ctx, bcx, ctx.matte_fact_id, arg_vals[0].1);
                    let tag = bcx.ins().iconst(I64, crate::codegen::ty::TAG_NUM);
                    (tag, res)
                }
                PrimOp::Lt | PrimOp::Gt | PrimOp::Le | PrimOp::Ge => {
                    let a_f64 = i64_to_f64(bcx, arg_vals[0].1);
                    let b_f64 = i64_to_f64(bcx, arg_vals[1].1);
                    let cc = match op {
                        PrimOp::Lt => FloatCC::LessThan,
                        PrimOp::Gt => FloatCC::GreaterThan,
                        PrimOp::Le => FloatCC::LessThanOrEqual,
                        PrimOp::Ge => FloatCC::GreaterThanOrEqual,
                        _ => unreachable!(),
                    };
                    let cmp = bcx.ins().fcmp(cc, a_f64, b_f64);
                    make_bool(bcx, cmp)
                }
                PrimOp::Eq | PrimOp::Ne => {
                    if let crate::checker::Type::Num = args[0].ty {
                        let a_f64 = i64_to_f64(bcx, arg_vals[0].1);
                        let b_f64 = i64_to_f64(bcx, arg_vals[1].1);
                        let cc = match op {
                            PrimOp::Eq => FloatCC::Equal,
                            PrimOp::Ne => FloatCC::NotEqual,
                            _ => unreachable!(),
                        };
                        let cmp = bcx.ins().fcmp(cc, a_f64, b_f64);
                        make_bool(bcx, cmp)
                    } else {
                        let a_i64 = arg_vals[0].1;
                        let b_i64 = arg_vals[1].1;
                        let cc = match op {
                            PrimOp::Eq => IntCC::Equal,
                            PrimOp::Ne => IntCC::NotEqual,
                            _ => unreachable!(),
                        };
                        let cmp = bcx.ins().icmp(cc, a_i64, b_i64);
                        make_bool(bcx, cmp)
                    }
                }
            }
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let then_b = bcx.create_block();
            let else_b = bcx.create_block();
            let merge_b = bcx.create_block();
            bcx.append_block_param(merge_b, I64);
            bcx.append_block_param(merge_b, I64);

            let (_, cond_payload) = translate(ctx, bcx, locals, cond);
            let c = bcx.ins().icmp_imm_u(IntCC::NotEqual, cond_payload, 0);
            bcx.ins().brif(c, then_b, &[], else_b, &[]);

            bcx.switch_to_block(then_b);
            bcx.seal_block(then_b);
            let (tt, tp) = translate(ctx, bcx, locals, then_branch);
            bcx.ins().jump(merge_b, &[tt, tp].map(|v| v.into()));

            bcx.switch_to_block(else_b);
            bcx.seal_block(else_b);
            let (et, ep) = translate(ctx, bcx, locals, else_branch);
            bcx.ins().jump(merge_b, &[et, ep].map(|v| v.into()));

            bcx.switch_to_block(merge_b);
            bcx.seal_block(merge_b);
            let tag = bcx.block_params(merge_b)[0];
            let payload = bcx.block_params(merge_b)[1];
            (tag, payload)
        }
        ExprKind::CallDirect { func, args } => {
            let nc = ctx.func_entries[func.0].n_captures;
            let np = ctx.func_entries[func.0].n_params;
            let arg_vals: Vec<_> = args
                .iter()
                .map(|a| translate(ctx, bcx, locals, a))
                .collect();

            let last_k = np - 1;
            let env_caps: Vec<_> = arg_vals[0..nc + last_k].to_vec();
            let last_stage_id = ctx.func_entries[func.0].stages[last_k];
            let func_ref_addr = ctx.module.declare_func_in_func(last_stage_id, bcx.func);
            let fn_ptr = bcx.ins().func_addr(I64, func_ref_addr);
            let closure_ptr =
                alloc_closure(&mut ctx.module, bcx, ctx.matte_alloc_id, fn_ptr, &env_caps);

            let (lt, lp) = arg_vals[nc + last_k];
            let func_ref_call = ctx.module.declare_func_in_func(last_stage_id, bcx.func);
            let call = bcx.ins().call(func_ref_call, &[closure_ptr, lt, lp]);
            let res = bcx.inst_results(call);
            (res[0], res[1])
        }
        ExprKind::MakeClosure { func, args } => {
            let entry = &ctx.func_entries[func.0];
            let nc = entry.n_captures;
            let n_applied = args.len().saturating_sub(nc);
            let stage_k = n_applied;
            let stage_id = entry.stages[stage_k];
            let func_ref = ctx.module.declare_func_in_func(stage_id, bcx.func);
            let fn_ptr = bcx.ins().func_addr(I64, func_ref);
            let cap_vals: Vec<_> = args
                .iter()
                .map(|a| translate(ctx, bcx, locals, a))
                .collect();
            let ptr = alloc_closure(&mut ctx.module, bcx, ctx.matte_alloc_id, fn_ptr, &cap_vals);
            make_closure_pair(bcx, ptr)
        }
        ExprKind::Apply { callee, args } => {
            let (mut cur_tag, mut cur_payload) = translate(ctx, bcx, locals, callee);
            let sig_ref = bcx.import_signature(ctx.matte_fn_sig.clone());
            for arg in args.iter() {
                let (at, ap) = translate(ctx, bcx, locals, arg);
                let fn_ptr = bcx.ins().load(
                    I64,
                    cranelift::codegen::ir::MemFlagsData::new(),
                    cur_payload,
                    0,
                );
                let call = bcx
                    .ins()
                    .call_indirect(sig_ref, fn_ptr, &[cur_payload, at, ap]);
                let res = bcx.inst_results(call);
                cur_tag = res[0];
                cur_payload = res[1];
            }
            (cur_tag, cur_payload)
        }
    }
}
