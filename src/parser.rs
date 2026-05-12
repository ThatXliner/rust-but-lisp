use crate::ast::{Expr, Span};

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
    pub span: (usize, usize),
}

impl ParseError {
    fn new(message: impl Into<String>, start: usize, end: usize) -> Self {
        Self {
            message: message.into(),
            span: (start, end),
        }
    }
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    byte_offset: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    LParen,
    RParen,
    Symbol(String),
    StringLit(String),
    Number(String),
}

fn tokenize(source: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let char_byte: Vec<usize> = source
        .char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(source.len()))
        .collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        let byte_offset = char_byte[i];

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
            tokens.push(Token { kind: TokenKind::LParen, byte_offset });
            i += 1;
            continue;
        }

        if c == ')' {
            tokens.push(Token { kind: TokenKind::RParen, byte_offset });
            i += 1;
            continue;
        }

        // String literals
        if c == '"' {
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
            let end_byte = if i < char_byte.len() {
                char_byte[i]
            } else {
                source.len()
            };
            tokens.push(Token {
                kind: TokenKind::StringLit(source[byte_offset..end_byte].to_string()),
                byte_offset,
            });
            continue;
        }

        // Symbols and numbers
        while i < chars.len()
            && !chars[i].is_whitespace()
            && chars[i] != '('
            && chars[i] != ')'
            && chars[i] != ';'
            && chars[i] != '"'
        {
            i += 1;
        }
        let end_byte = if i < char_byte.len() {
            char_byte[i]
        } else {
            source.len()
        };
        let s = source[byte_offset..end_byte].to_string();

        if is_number(&s) {
            tokens.push(Token { kind: TokenKind::Number(s), byte_offset });
        } else {
            tokens.push(Token { kind: TokenKind::Symbol(s), byte_offset });
        }
    }

    tokens
}

fn is_number(s: &str) -> bool {
    let first = s.chars().next();
    // Must start with a digit or `-`, contain at least one digit, and consist
    // only of digits, `.`, `-`, and `_`.  The digit requirement prevents bare
    // `-` from being parsed as a number so it stays a Symbol (operator).
    matches!(first, Some('0'..='9') | Some('-'))
        && s.chars().any(|c| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == '_')
}

fn parse_expr(tokens: &[Token], pos: &mut usize) -> Result<Expr, ParseError> {
    if *pos >= tokens.len() {
        let end = tokens.last().map_or(0, |t| t.byte_offset);
return Err(ParseError::new("unexpected end of input", end, end));
    }

    match &tokens[*pos].kind {
        TokenKind::LParen => {
            let open_offset = tokens[*pos].byte_offset;
            *pos += 1; // consume '('
            let mut items = Vec::new();
            while *pos < tokens.len() && !matches!(tokens[*pos].kind, TokenKind::RParen) {
                items.push(parse_expr(tokens, pos)?);
            }
            if *pos >= tokens.len() {
                return Err(ParseError::new("unclosed list", open_offset, open_offset + 1));
            }
            let close_offset = tokens[*pos].byte_offset;
            *pos += 1; // consume ')'
            Ok(Expr::List(items, Some(Span::new(open_offset, close_offset + 1))))
        }
        TokenKind::RParen => {
            let offset = tokens[*pos].byte_offset;
Err(ParseError::new("unexpected ')'", offset, offset + 1))
        }
        TokenKind::Symbol(s) => {
            *pos += 1;
            Ok(Expr::Symbol(s.clone()))
        }
        TokenKind::StringLit(s) => {
            *pos += 1;
            Ok(Expr::StringLit(s.clone()))
        }
        TokenKind::Number(n) => {
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
        match &result[0] {
            Expr::List(items, _) => {
                assert_eq!(
                    items.as_slice(),
                    &[
                        Expr::Symbol("a".into()),
                        Expr::Symbol("b".into()),
                        Expr::Symbol("c".into()),
                    ]
                );
            }
            _ => panic!("Expected list"),
        }
    }

    #[test]
    fn test_nested_list() {
        let result = parse("(a (b c) d)").unwrap();
        assert_eq!(result.len(), 1);
        match &result[0] {
            Expr::List(items, _) => {
                assert_eq!(items.len(), 3);
                assert_eq!(items[0], Expr::Symbol("a".into()));
                assert_eq!(items[2], Expr::Symbol("d".into()));
                match &items[1] {
                    Expr::List(inner, _) => {
                        assert_eq!(
                            inner.as_slice(),
                            &[Expr::Symbol("b".into()), Expr::Symbol("c".into()),]
                        );
                    }
                    _ => panic!("Expected nested list"),
                }
            }
            _ => panic!("Expected list"),
        }
    }

    #[test]
    fn test_span_on_list() {
        let src = "(+ 1 2)";
        let result = parse(src).unwrap();
        match &result[0] {
            Expr::List(_, Some(span)) => {
                assert_eq!(span.start, 0);
                assert_eq!(span.end, 7); // ")" at byte 6, +1 = 7
            }
            _ => panic!("Expected list with span"),
        }
    }

    #[test]
    fn test_unexpected_paren_error_offset() {
        let src = "
        )";
        let err = parse(src).unwrap_err();
        assert_eq!(err.message, "unexpected ')'");
        assert!(err.span.0 > 0, "span start should point to the ')'");
    }
}
