//! Core IR: a typed tree that still has lambdas.
//!
//! Built from `ResolvedProgram` + `Checked`. Every node carries its resolved
//! type, directly nested `fn`s are collapsed into one multi-parameter `Lam`,
//! and application spines are flattened (`f a b` is one `App`). Nodes live in
//! the compiler's bump arena (`&'a Expr<'a>` / `&'a [T]`). Identity of
//! variables is the resolver's `LocalId` / `GlobalId`, so nothing is matched
//! by name.

use bumpalo::Bump;

use crate::ast::core::{BinaryOp, UnaryOp};
use crate::checker::{Checked, Type};
use crate::resolve::{
    GlobalId, LocalId, ResolvedExprKind, ResolvedExpression, ResolvedItem, ResolvedName,
    ResolvedProgram,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Neg,
    Fact,
    Lt,
    Gt,
    Le,
    Ge,
    /// Operand type (`Num` or `Bool`) is `args[0].ty`.
    Eq,
    Ne,
}

#[derive(Debug, Clone)]
pub struct Expr<'a> {
    pub kind: ExprKind<'a>,
    /// Fully resolved; never `Type::Var`.
    pub ty: Type<'a>,
}

#[derive(Debug, Clone)]
pub enum ExprKind<'a> {
    Unit,
    Num(f64),
    Bool(bool),
    Local(LocalId),
    Global(GlobalId),
    Prim {
        op: PrimOp,
        args: &'a [Expr<'a>],
    },
    If {
        cond: &'a Expr<'a>,
        then_branch: &'a Expr<'a>,
        else_branch: &'a Expr<'a>,
    },
    /// One or more parameters; `ty` of the whole node is `p1 -> p2 -> ... -> ret`.
    Lam {
        params: &'a [(LocalId, Type<'a>)],
        body: &'a Expr<'a>,
    },
    /// `args` is never empty. `ty` is the type after applying all of them.
    App {
        func: &'a Expr<'a>,
        args: &'a [Expr<'a>],
    },
}

#[derive(Debug, Clone)]
pub enum Item<'a> {
    Global {
        id: GlobalId,
        name: &'a str,
        value: Expr<'a>,
    },
    Eval(Expr<'a>),
    Print(Expr<'a>),
    /// `print typeof e;` is fully known at compile time.
    PrintType(&'a str),
}

#[derive(Debug, Clone)]
pub struct Program<'a> {
    /// Source order. Top-level initialization order is exactly this order.
    pub items: &'a [Item<'a>],
    /// Indexed by `GlobalId`.
    pub global_names: &'a [&'a str],
}

pub fn lower<'a>(
    arena: &'a Bump,
    program: &ResolvedProgram<'a>,
    checked: &Checked<'a>,
) -> Program<'a> {
    let lw = Lowerer { arena, checked };
    let items = arena.alloc_slice_fill_iter(program.items.iter().map(|item| match item {
        ResolvedItem::Binding {
            id, name, value, ..
        } => Item::Global {
            id: *id,
            name: *name,
            value: lw.expr(value),
        },
        ResolvedItem::Expr(e) => Item::Eval(lw.expr(e)),
        ResolvedItem::Print {
            type_of: true,
            expr,
        } => Item::PrintType(arena.alloc_str(&lw.ty(expr).pretty())),
        ResolvedItem::Print {
            type_of: false,
            expr,
        } => Item::Print(lw.expr(expr)),
    }));

    Program {
        items,
        global_names: arena.alloc_slice_fill_iter(program.globals.iter().map(|g| g.name)),
    }
}

struct Lowerer<'c, 'a> {
    arena: &'a Bump,
    checked: &'c Checked<'a>,
}

impl<'c, 'a> Lowerer<'c, 'a> {
    fn ty(&self, e: &ResolvedExpression<'a>) -> Type<'a> {
        *self
            .checked
            .node_types
            .get(&e.id)
            .expect("checker records a type for every expression")
    }

    fn node(&self, e: Expr<'a>) -> &'a Expr<'a> {
        self.arena.alloc(e)
    }

    fn slice<T>(&self, items: Vec<T>) -> &'a [T] {
        self.arena.alloc_slice_fill_iter(items)
    }

    fn expr(&self, e: &ResolvedExpression<'a>) -> Expr<'a> {
        let ty = self.ty(e);
        let kind = match &e.kind {
            ResolvedExprKind::Unit => ExprKind::Unit,
            ResolvedExprKind::Num(n) => ExprKind::Num(*n),
            ResolvedExprKind::Bool(b) => ExprKind::Bool(*b),
            ResolvedExprKind::Ident(ResolvedName::Global(g)) => ExprKind::Global(*g),
            ResolvedExprKind::Ident(ResolvedName::Local(l)) => ExprKind::Local(*l),
            ResolvedExprKind::Ident(ResolvedName::Error) => {
                unreachable!("successful resolution leaves no error names")
            }
            ResolvedExprKind::Unary { op, expr } => ExprKind::Prim {
                op: unary_op(op),
                args: self.slice(vec![self.expr(expr)]),
            },
            ResolvedExprKind::Binary { left, op, right } => ExprKind::Prim {
                op: binary_op(op),
                args: self.slice(vec![self.expr(left), self.expr(right)]),
            },
            ResolvedExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => ExprKind::If {
                cond: self.node(self.expr(cond)),
                then_branch: self.node(self.expr(then_branch)),
                else_branch: self.node(self.expr(else_branch)),
            },
            ResolvedExprKind::Fn { arg, body, .. } => {
                // Arity collapse: fuse directly nested lambdas only.
                let mut params = vec![(*arg, param_ty(ty))];
                let mut cur: &ResolvedExpression<'a> = &**body;
                while let ResolvedExprKind::Fn { arg, body, .. } = &cur.kind {
                    params.push((*arg, param_ty(self.ty(cur))));
                    cur = &**body;
                }
                ExprKind::Lam {
                    params: self.slice(params),
                    body: self.node(self.expr(cur)),
                }
            }
            ResolvedExprKind::App { .. } => {
                // Flatten the application spine: ((f a) b) c  =>  f [a, b, c].
                let mut args = Vec::new();
                let mut cur = e;
                while let ResolvedExprKind::App { func, arg } = &cur.kind {
                    args.push(self.expr(arg));
                    cur = &**func;
                }
                args.reverse();
                ExprKind::App {
                    func: self.node(self.expr(cur)),
                    args: self.slice(args),
                }
            }
        };
        Expr { kind, ty }
    }
}

fn param_ty<'a>(fn_ty: Type<'a>) -> Type<'a> {
    match fn_ty {
        Type::Fn(param, _) => *param,
        other => unreachable!("lambda must have a function type, got {other:?}"),
    }
}

fn unary_op(op: &UnaryOp) -> PrimOp {
    match op {
        UnaryOp::Neg(_) => PrimOp::Neg,
        UnaryOp::Fact(_) => PrimOp::Fact,
    }
}

fn binary_op(op: &BinaryOp) -> PrimOp {
    match op {
        BinaryOp::Add(_) => PrimOp::Add,
        BinaryOp::Sub(_) => PrimOp::Sub,
        BinaryOp::Mul(_) => PrimOp::Mul,
        BinaryOp::Div(_) => PrimOp::Div,
        BinaryOp::Mod(_) => PrimOp::Mod,
        BinaryOp::Pow(_) => PrimOp::Pow,
        BinaryOp::Lt(_) => PrimOp::Lt,
        BinaryOp::Gt(_) => PrimOp::Gt,
        BinaryOp::Le(_) => PrimOp::Le,
        BinaryOp::Ge(_) => PrimOp::Ge,
        BinaryOp::Eq(_) => PrimOp::Eq,
        BinaryOp::Ne(_) => PrimOp::Ne,
    }
}
