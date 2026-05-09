#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Symbol(String),
    Number(String),
    StringLit(String),
    List(Vec<Expr>),
}
