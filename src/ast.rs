#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

#[derive(Debug, Clone)]
pub enum Expr {
    Symbol(String),
    Number(String),
    StringLit(String),
    List(Vec<Expr>, Option<Span>),
}

impl PartialEq for Expr {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Expr::Symbol(a), Expr::Symbol(b)) => a == b,
            (Expr::Number(a), Expr::Number(b)) => a == b,
            (Expr::StringLit(a), Expr::StringLit(b)) => a == b,
            (Expr::List(a, _), Expr::List(b, _)) => a == b,
            _ => false,
        }
    }
}
