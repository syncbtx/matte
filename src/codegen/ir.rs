#[derive(Debug, Clone)]
pub enum IrExpr<'a> {
    Num(f64),
    Bool(bool),
    Local(&'a str),
}
