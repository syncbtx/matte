// use cranelift::{
//     codegen::ir::{InstBuilder, Value},
//     frontend::FunctionBuilder,
// };
//
// use crate::ast::core::{BinaryOp, ExprKind, Expression, Item, Program};
//
// pub struct CodeGenerator {}
//
// impl CodeGenerator {
//     pub fn new() -> Self {
//         Self
//     }
//
//     pub fn run(builder: &mut FunctionBuilder, program: &Program) -> Value {
//         program.items.iter().for_each(|item| match item {
//             Item::Expr(expr) => {}
//             Item::Binding { name, value } => {}
//             Item::Print { type_of, expr } => {}
//             Item::FnBinding { .. } => unreachable!(),
//         });
//     }
//
//     pub fn translate_expr<'a>(
//         &self,
//         builder: &mut FunctionBuilder,
//         expr: &'a Expression<'a>,
//     ) -> Value {
//         match expr.kind {
//             ExprKind::Num(val) => builder.ins().f64const(val),
//             ExprKind::BinaryOp { left, op, right } => {
//                 let x = self.translate_expr(builder, left);
//                 let y = self.translate_expr(builder, right);
//                 match op {
//                     BinaryOp::Add(_) => builder.ins().fadd(x, y),
//                     BinaryOp::Sub(_) => builder.ins().fsub(x, y),
//                     BinaryOp::Mul(_) => builder.ins().fmul(x, y),
//                     BinaryOp::Div(_) => builder.ins().fdiv(x, y),
//                     BinaryOp::Le(_)  => builder.ins().fcmp(Cond, x, y)
//                 }
//             }
//             _ => unreachable!(),
//         }
//     }
// }
