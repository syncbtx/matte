use logos::Span;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::lexer::TokenKind;

#[derive(Debug, Clone)]
pub struct Program<'a> {
    pub items: Vec<Item<'a>>,
}

#[derive(Debug, Clone)]
pub enum Item<'a> {
    Expr(&'a Expression<'a>), // 2 + 3.;
    Binding {
        // x :: add 2 3;
        name: Identifier<'a>,
        value: &'a Expression<'a>,
    },
    Print {
        // print typeof add;
        type_of: bool,
        expr: &'a Expression<'a>,
    },
    FnBinding {
        // max x y :: if x > y then x else y;
        // max :: fn x -> fn y -> if x > y then x else y;
        target: &'a Expression<'a>,
        value: &'a Expression<'a>,
    },
}

#[derive(Debug, Clone)]
pub struct Expression<'a> {
    pub kind: ExprKind<'a>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Identifier<'a> {
    pub name: &'a str,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind<'a> {
    Unit,
    Ident(&'a str),
    Num(f64),
    Bool(bool),
    Unary {
        op: UnaryOp,
        expr: &'a Expression<'a>,
    },
    BinaryOp {
        left: &'a Expression<'a>,
        op: BinaryOp,
        right: &'a Expression<'a>,
    },
    If {
        cond: &'a Expression<'a>,
        then_branch: &'a Expression<'a>,
        else_branch: &'a Expression<'a>,
    },
    /// `fn x -> body`. exactly one parameter; the parser lowers
    /// `fn x y -> e` into `Fn(x, Fn(y, e))`.
    Fn {
        arg: Identifier<'a>,
        body: &'a Expression<'a>,
    },

    /// `f a`. exactly one argument;
    /// `f a b` is `App(App(f, a), b)`.
    App {
        func: &'a Expression<'a>,
        arg: &'a Expression<'a>,
    },
}

#[derive(Debug, Clone)]
pub enum UnaryOp {
    Neg(Span),
    Fact(Span),
}

impl UnaryOp {
    pub fn from_token_kind(kind: TokenKind<'_>, span: Span) -> Option<UnaryOp> {
        match kind {
            TokenKind::Minus => Some(UnaryOp::Neg(span)),
            TokenKind::Bang => Some(UnaryOp::Fact(span)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum BinaryOp {
    Add(Span),
    Sub(Span),
    Mul(Span),
    Div(Span),
    Mod(Span),
    Pow(Span),
    Eq(Span),
    Ne(Span),
    Lt(Span),
    Gt(Span),
    Le(Span),
    Ge(Span),
}

impl BinaryOp {
    pub fn from_token_kind(kind: TokenKind<'_>, span: Span) -> Option<BinaryOp> {
        match kind {
            TokenKind::Plus => Some(BinaryOp::Add(span)),
            TokenKind::Minus => Some(BinaryOp::Sub(span)),
            TokenKind::Star => Some(BinaryOp::Mul(span)),
            TokenKind::Slash => Some(BinaryOp::Div(span)),
            TokenKind::Percent => Some(BinaryOp::Mod(span)),
            TokenKind::Caret => Some(BinaryOp::Pow(span)),
            TokenKind::Eq => Some(BinaryOp::Eq(span)),
            TokenKind::Ne => Some(BinaryOp::Ne(span)),
            TokenKind::Lt => Some(BinaryOp::Lt(span)),
            TokenKind::Gt => Some(BinaryOp::Gt(span)),
            TokenKind::Le => Some(BinaryOp::Le(span)),
            TokenKind::Ge => Some(BinaryOp::Ge(span)),
            _ => None,
        }
    }
}
