pub mod ty;
pub mod context;
pub mod func;
pub mod expr;
pub mod main_gen;

use std::path::Path;
use crate::ir::lifted;

pub fn compile(program: &lifted::Program, out_path: &Path) {
    let mut ctx = context::CodegenCtx::new(program);
    func::emit_all_funcs(&mut ctx, program);
    main_gen::emit_main(&mut ctx, program);
    let product = ctx.module.finish();
    let bytes = product.emit().expect("failed to emit object");
    std::fs::write(out_path, bytes).expect("failed to write object file");
}
