use bumpalo::Bump;
use crate::ast::core::{Program, Item, Expression, ExprKind, Identifier};
use crate::diagnostics::ParserError;
use ariadne::{Report, ReportKind, Label};

pub struct Desugarer<'a> {
    pub arena: &'a Bump,
}

impl<'a> Desugarer<'a> {
    pub fn new(arena: &'a Bump) -> Self {
        Self { arena }
    }

    pub fn desugar(&self, prog: &Program<'a>) -> Result<Program<'a>, ParserError<'a>> {
        let mut items = Vec::new();
        for item in &prog.items {
            items.push(self.desugar_item(item)?);
        }
        Ok(Program { items })
    }

    fn desugar_item(&self, item: &Item<'a>) -> Result<Item<'a>, ParserError<'a>> {
        match item {
            Item::Expr(expr) => Ok(Item::Expr(self.desugar_expr(expr)?)),
            Item::Print { type_of, expr } => Ok(Item::Print {
                type_of: *type_of,
                expr: self.desugar_expr(expr)?,
            }),
            Item::Binding { name, value } => Ok(Item::Binding {
                name,
                value: self.desugar_expr(value)?,
            }),
            Item::FnDefinition { name, params, body } => Ok(Item::FnDefinition {
                name,
                params,
                body: self.desugar_expr(body)?,
            }),
            Item::SugarBinding { target, value } => {
                let desugared_value = self.desugar_expr(value)?;
                
                let mut current = *target;
                let mut params = Vec::new();
                
                while let ExprKind::App { func, arg } = &current.kind {
                    if let ExprKind::Ident(name) = arg.kind {
                        params.push(Identifier { name, span: arg.span.clone() });
                    } else {
                        return Err(ParserError(Report::build(ReportKind::Error, arg.span.clone())
                            .with_message("function parameter must be an identifier")
                            .with_label(Label::new(arg.span.clone()).with_message("not an identifier"))
                            .finish()));
                    }
                    current = func;
                }
                
                if let ExprKind::Ident(name) = current.kind {
                    if params.is_empty() {
                        Ok(Item::Binding {
                            name,
                            value: desugared_value,
                        })
                    } else {
                        params.reverse();
                        let params_slice = self.arena.alloc_slice_fill_iter(params.into_iter());
                        Ok(Item::FnDefinition {
                            name,
                            params: params_slice,
                            body: desugared_value,
                        })
                    }
                } else {
                    Err(ParserError(Report::build(ReportKind::Error, target.span.clone())
                        .with_message("invalid binding target")
                        .with_label(Label::new(target.span.clone()).with_message("expected identifier or function application"))
                        .finish()))
                }
            }
        }
    }

    fn desugar_expr(&self, expr: &Expression<'a>) -> Result<&'a Expression<'a>, ParserError<'a>> {
        let kind = match &expr.kind {
            ExprKind::Unit => ExprKind::Unit,
            ExprKind::Ident(name) => ExprKind::Ident(name),
            ExprKind::Num(n) => ExprKind::Num(*n),
            ExprKind::Bool(b) => ExprKind::Bool(*b),
            ExprKind::Unary { op, expr: inner } => ExprKind::Unary {
                op: op.clone(),
                expr: self.desugar_expr(inner)?,
            },
            ExprKind::BinaryOp { left, op, right } => ExprKind::BinaryOp {
                left: self.desugar_expr(left)?,
                op: op.clone(),
                right: self.desugar_expr(right)?,
            },
            ExprKind::If { cond, then_branch, else_branch } => ExprKind::If {
                cond: self.desugar_expr(cond)?,
                then_branch: self.desugar_expr(then_branch)?,
                else_branch: self.desugar_expr(else_branch)?,
            },
            ExprKind::Fn { arg, body } => ExprKind::Fn {
                arg: Identifier { name: arg.name, span: arg.span.clone() },
                body: self.desugar_expr(body)?,
            },
            ExprKind::App { func, arg } => ExprKind::App {
                func: self.desugar_expr(func)?,
                arg: self.desugar_expr(arg)?,
            },
        };
        Ok(self.arena.alloc(Expression { kind, span: expr.span.clone() }))
    }
}
