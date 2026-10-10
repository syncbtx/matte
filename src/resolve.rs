use std::collections::HashMap;

use bumpalo::Bump;
use logos::Span;

use crate::ast::core::{ExprKind, Expression, Item, Program};
use crate::diagnostics::ParserError;

/// Stable identity for a top-level binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlobalId(pub usize);

/// Stable identity for a function parameter. Every parameter declaration gets
/// its own ID, including shadowed parameters with the same spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalId(pub usize);

/// Stable identity for an expression in the resolved tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolvedName {
    Global(GlobalId),
    Local(LocalId),
    /// Recovery-only node. A successful `resolve` never returns a tree
    /// containing this variant because any resolution error returns `Err`.
    Error,
}

#[derive(Debug, Clone)]
pub struct GlobalSymbol<'a> {
    pub id: GlobalId,
    pub name: &'a str,
    pub name_span: Span,
    pub declaration_item: usize,
}

/// The resolved tree lives in the bump arena: lists are `&'a [T]` and child
/// expressions are `&'a ResolvedExpression<'a>`.
#[derive(Debug, Clone)]
pub struct ResolvedProgram<'a> {
    pub items: &'a [ResolvedItem<'a>],
    /// Indexed by `GlobalId`.
    pub globals: &'a [GlobalSymbol<'a>],
    pub local_count: usize,
}

#[derive(Debug, Clone)]
pub enum ResolvedItem<'a> {
    Expr(ResolvedExpression<'a>),
    Binding {
        id: GlobalId,
        name: &'a str,
        name_span: Span,
        value: ResolvedExpression<'a>,
    },
    Print {
        type_of: bool,
        expr: ResolvedExpression<'a>,
    },
}

#[derive(Debug, Clone)]
pub struct ResolvedExpression<'a> {
    pub id: ExprId,
    pub kind: ResolvedExprKind<'a>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ResolvedExprKind<'a> {
    Unit,
    Ident(ResolvedName),
    Num(f64),
    Bool(bool),
    Unary {
        op: crate::ast::core::UnaryOp,
        expr: &'a ResolvedExpression<'a>,
    },
    Binary {
        left: &'a ResolvedExpression<'a>,
        op: crate::ast::core::BinaryOp,
        right: &'a ResolvedExpression<'a>,
    },
    If {
        cond: &'a ResolvedExpression<'a>,
        then_branch: &'a ResolvedExpression<'a>,
        else_branch: &'a ResolvedExpression<'a>,
    },
    Fn {
        arg: LocalId,
        arg_name: &'a str,
        arg_span: Span,
        body: &'a ResolvedExpression<'a>,
    },
    App {
        func: &'a ResolvedExpression<'a>,
        arg: &'a ResolvedExpression<'a>,
    },
}

/// Resolves names in a desugared AST.
///
/// Language rules implemented here:
/// - Local parameters shadow top-level names.
/// - Top-level references must refer to an earlier binding, with one
///   exception: a function body may refer to a later top-level binding whose
///   value is a lambda. This allows forward references and (mutual)
///   recursion between functions.
/// - A non-lambda global can therefore never be read before it is
///   initialized, so top-level initialization order is source order.
/// - A binding is not considered defined while its own value is being
///   resolved.
pub struct NameResolver<'a> {
    arena: &'a Bump,
    globals_by_name: HashMap<&'a str, GlobalId>,
    globals: Vec<GlobalSymbol<'a>>,
    globals_defined: Vec<bool>,
    global_is_fn: Vec<bool>,
    locals: Vec<(&'a str, LocalId)>,
    function_depth: usize,
    next_local: usize,
    next_expr: usize,
    errors: Vec<ParserError<'a>>,
}

impl<'a> NameResolver<'a> {
    pub fn new(arena: &'a Bump) -> Self {
        Self {
            arena,
            globals_by_name: HashMap::new(),
            globals: Vec::new(),
            globals_defined: Vec::new(),
            global_is_fn: Vec::new(),
            locals: Vec::new(),
            function_depth: 0,
            next_local: 0,
            next_expr: 0,
            errors: Vec::new(),
        }
    }

    pub fn resolve(
        mut self,
        program: &Program<'a>,
    ) -> Result<ResolvedProgram<'a>, Vec<ParserError<'a>>> {
        let arena = self.arena;

        // Pass 1: predeclare globals so function bodies can reference later
        // declarations. Keep the first declaration when a name is duplicated.
        let mut skip_item = vec![false; program.items.len()];

        for (item_index, item) in program.items.iter().enumerate() {
            match item {
                Item::Binding { name, value } => {
                    if self.globals_by_name.contains_key(name.name) {
                        self.error(
                            &name.span,
                            format!("`{}` is already defined", name.name),
                            "redefinition",
                        );
                        skip_item[item_index] = true;
                    } else {
                        let id = GlobalId(self.globals.len());
                        self.globals_by_name.insert(name.name, id);
                        self.globals.push(GlobalSymbol {
                            id,
                            name: name.name,
                            name_span: name.span.clone(),
                            declaration_item: item_index,
                        });
                        self.globals_defined.push(false);
                        self.global_is_fn
                            .push(matches!(value.kind, ExprKind::Fn { .. }));
                    }
                }
                Item::FnBinding { target, .. } => {
                    self.error(
                        &target.span,
                        "function binding reached name resolution before desugaring".to_string(),
                        "desugar function bindings before resolving names",
                    );
                    skip_item[item_index] = true;
                }
                Item::Expr(_) | Item::Print { .. } => {}
            }
        }

        // Pass 2: resolve references in source order. The `globals_defined`
        // flags enforce the language's top-level initialization order.
        let mut items = Vec::with_capacity(program.items.len());
        for (item_index, item) in program.items.iter().enumerate() {
            if skip_item[item_index] {
                continue;
            }

            match item {
                Item::Expr(expr) => {
                    items.push(ResolvedItem::Expr(self.resolve_expr(expr)));
                }
                Item::Binding { name, value } => {
                    let id = self.globals_by_name[name.name];
                    let resolved_value = self.resolve_expr(value);
                    items.push(ResolvedItem::Binding {
                        id,
                        name: name.name,
                        name_span: name.span.clone(),
                        value: resolved_value,
                    });
                    self.globals_defined[id.0] = true;
                }
                Item::Print { type_of, expr } => {
                    items.push(ResolvedItem::Print {
                        type_of: *type_of,
                        expr: self.resolve_expr(expr),
                    });
                }
                Item::FnBinding { .. } => unreachable!("handled during predeclaration"),
            }
        }

        if self.errors.is_empty() {
            Ok(ResolvedProgram {
                items: arena.alloc_slice_fill_iter(items),
                globals: arena.alloc_slice_fill_iter(self.globals),
                local_count: self.next_local,
            })
        } else {
            Err(self.errors)
        }
    }

    fn resolve_expr(&mut self, expr: &'a Expression<'a>) -> ResolvedExpression<'a> {
        let arena = self.arena;
        let id = ExprId(self.next_expr);
        self.next_expr += 1;

        let kind = match &expr.kind {
            ExprKind::Unit => ResolvedExprKind::Unit,
            ExprKind::Num(n) => ResolvedExprKind::Num(*n),
            ExprKind::Bool(b) => ResolvedExprKind::Bool(*b),
            ExprKind::Ident(name) => {
                let resolved = if let Some((_, local_id)) = self
                    .locals
                    .iter()
                    .rev()
                    .find(|(local_name, _)| local_name == name)
                {
                    ResolvedName::Local(*local_id)
                } else if let Some(global_id) = self.globals_by_name.get(name).copied() {
                    if self.globals_defined[global_id.0]
                        || (self.function_depth > 0 && self.global_is_fn[global_id.0])
                    {
                        ResolvedName::Global(global_id)
                    } else {
                        self.error(
                            &expr.span,
                            format!("`{name}` is used before its definition"),
                            "only functions may be referred to before their definition",
                        );
                        ResolvedName::Error
                    }
                } else {
                    self.error(&expr.span, format!("unknown name `{name}`"), "not defined");
                    ResolvedName::Error
                };
                ResolvedExprKind::Ident(resolved)
            }
            ExprKind::Unary { op, expr: inner } => ResolvedExprKind::Unary {
                op: op.clone(),
                expr: arena.alloc(self.resolve_expr(inner)),
            },
            ExprKind::Binary { left, op, right } => {
                let left = arena.alloc(self.resolve_expr(left));
                let right = arena.alloc(self.resolve_expr(right));
                ResolvedExprKind::Binary {
                    left,
                    op: op.clone(),
                    right,
                }
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond = arena.alloc(self.resolve_expr(cond));
                let then_branch = arena.alloc(self.resolve_expr(then_branch));
                let else_branch = arena.alloc(self.resolve_expr(else_branch));
                ResolvedExprKind::If {
                    cond,
                    then_branch,
                    else_branch,
                }
            }
            ExprKind::Fn { arg, body } => {
                let local_id = LocalId(self.next_local);
                self.next_local += 1;
                self.locals.push((arg.name, local_id));
                self.function_depth += 1;
                let resolved_body = arena.alloc(self.resolve_expr(body));
                self.function_depth -= 1;
                self.locals.pop();

                ResolvedExprKind::Fn {
                    arg: local_id,
                    arg_name: arg.name,
                    arg_span: arg.span.clone(),
                    body: resolved_body,
                }
            }
            ExprKind::App { func, arg } => {
                let func = arena.alloc(self.resolve_expr(func));
                let arg = arena.alloc(self.resolve_expr(arg));
                ResolvedExprKind::App { func, arg }
            }
        };

        ResolvedExpression {
            id,
            kind,
            span: expr.span.clone(),
        }
    }

    fn error(&mut self, span: &Span, message: String, label: &str) {
        self.errors.push(ParserError(
            ariadne::Report::build(ariadne::ReportKind::Error, span.clone())
                .with_message(message)
                .with_label(ariadne::Label::new(span.clone()).with_message(label))
                .finish(),
        ));
    }
}
