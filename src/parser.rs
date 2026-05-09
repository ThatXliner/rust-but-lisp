use crate::ast::Expr;

pub fn parse(source: &str) -> Result<Vec<Expr>, ParseError> {
    let tokens = tokenize(source);
    let mut pos = 0;
    let mut exprs = Vec::new();

    while pos < tokens.len() {
        exprs.push(parse_expr(&tokens, &mut pos)?);
    }

    Ok(exprs)
}

#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    pub pos: usize,
}

impl ParseError {
    fn new(message: impl Into<String>, pos: usize) -> Self {
        Self {
            message: message.into(),
            pos,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    LParen,
    RParen,
    Symbol(String),
    StringLit(String),
    Number(String),
}

fn tokenize(source: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // Line comments
        if c == ';' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        if c == '(' {
            tokens.push(Token::LParen);
            i += 1;
            continue;
        }

        if c == ')' {
            tokens.push(Token::RParen);
            i += 1;
            continue;
        }

        // String literals
        if c == '"' {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1; // skip escaped char
                }
                i += 1;
            }
            if i < chars.len() {
                i += 1; // closing quote
            }
            tokens.push(Token::StringLit(source[start..i].to_string()));
            continue;
        }

        // Symbols and numbers
        let start = i;
        while i < chars.len()
            && !chars[i].is_whitespace()
            && chars[i] != '('
            && chars[i] != ')'
            && chars[i] != ';'
            && chars[i] != '"'
        {
            i += 1;
        }
        let s = source[start..i].to_string();

        // Determine if it's a number
        if is_number(&s) {
            tokens.push(Token::Number(s));
        } else {
            tokens.push(Token::Symbol(s));
        }
    }

    tokens
}

fn is_number(s: &str) -> bool {
    let first = s.chars().next();
    matches!(first, Some('0'..='9') | Some('-'))
        && s.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == '_')
}

fn parse_expr(tokens: &[Token], pos: &mut usize) -> Result<Expr, ParseError> {
    if *pos >= tokens.len() {
        return Err(ParseError::new("unexpected end of input", *pos));
    }

    match &tokens[*pos] {
        Token::LParen => {
            *pos += 1; // consume '('
            let mut items = Vec::new();
            while *pos < tokens.len() && !matches!(tokens[*pos], Token::RParen) {
                items.push(parse_expr(tokens, pos)?);
            }
            if *pos >= tokens.len() {
                return Err(ParseError::new("unclosed list", *pos));
            }
            *pos += 1; // consume ')'
            Ok(Expr::List(items))
        }
        Token::RParen => Err(ParseError::new("unexpected ')'", *pos)),
        Token::Symbol(s) => {
            *pos += 1;
            Ok(Expr::Symbol(s.clone()))
        }
        Token::StringLit(s) => {
            *pos += 1;
            Ok(Expr::StringLit(s.clone()))
        }
        Token::Number(n) => {
            *pos += 1;
            Ok(Expr::Number(n.clone()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_list() {
        let result = parse("(a b c)").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0],
            Expr::List(vec![
                Expr::Symbol("a".into()),
                Expr::Symbol("b".into()),
                Expr::Symbol("c".into()),
            ])
        );
    }

    #[test]
    fn test_nested_list() {
        let result = parse("(a (b c) d)").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0],
            Expr::List(vec![
                Expr::Symbol("a".into()),
                Expr::List(vec![
                    Expr::Symbol("b".into()),
                    Expr::Symbol("c".into()),
                ]),
                Expr::Symbol("d".into()),
            ])
        );
    }
}
