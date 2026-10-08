use logos::Span;

#[derive(Debug, Clone)]
pub struct Program<'a> {
    pub items: Vec<Item<'a>>,
}

#[derive(Debug, Clone)]
pub enum Item<'a> {
    Expr(&'a Expression<'a>),
    Binding {
        name: &'a str,
        value: &'a Expression<'a>,
    },
    FnDefinition {
        name: &'a str,
        params: &'a [Identifier<'a>],
        body: &'a Expression<'a>,
    },
    Print {
        type_of: bool,
        expr: &'a Expression<'a>,
    },
    SugarBinding {
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
pub enum UnaryOp {
    Neg(Span),
    Fact(Span),
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
    Fn {
        arg: Identifier<'a>,
        body: &'a Expression<'a>,
    },
    App {
        func: &'a Expression<'a>,
        arg: &'a Expression<'a>,
    },
}
