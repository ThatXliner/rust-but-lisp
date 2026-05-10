use crate::ast::Expr;
use std::collections::HashMap;

#[derive(Debug, Clone)]
struct Macro {
    params: Vec<String>,
    has_rest: bool,
    rest_param: Option<String>,
    body: Expr,
}

/// Expands macros in a list of top-level expressions.
/// `defmacro` forms are collected and removed. Macro calls are expanded in place.
pub fn expand(exprs: &[Expr]) -> Vec<Expr> {
    let mut macros: HashMap<String, Macro> = HashMap::new();
    let mut result: Vec<Expr> = Vec::new();

    for expr in exprs {
        match try_parse_defmacro(expr) {
            Some((name, m)) => {
                macros.insert(name, m);
            }
            None => {
                result.push(expand_expr(expr, &macros));
            }
        }
    }

    // Keep expanding until no more macros apply
    loop {
        let mut changed = false;
        let mut new_result: Vec<Expr> = Vec::new();
        for expr in &result {
            let expanded = expand_expr(expr, &macros);
            if &expanded != expr {
                changed = true;
            }
            new_result.push(expanded);
        }
        result = new_result;
        if !changed {
            break;
        }
    }

    result
}

fn try_parse_defmacro(expr: &Expr) -> Option<(String, Macro)> {
    match expr {
        Expr::List(items, _) if items.len() >= 4 => {
            match &items[0] {
                Expr::Symbol(s) if s == "defmacro" => {
                    let name = match &items[1] {
                        Expr::Symbol(n) => n.clone(),
                        _ => return None,
                    };
                    let params = match &items[2] {
                        Expr::List(p, _) => parse_macro_params(p),
                        _ => return None,
                    };
                    let body = items[3].clone();
                    let (param_names, has_rest, rest_param) = params;
                    Some((
                        name,
                        Macro {
                            params: param_names,
                            has_rest,
                            rest_param,
                            body,
                        },
                    ))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Parse macro parameter list. Supports `&rest` for variadic args.
/// (a b &rest c) → ([a, b], true, Some("c"))
/// (a b) → ([a, b], false, None)
fn parse_macro_params(params: &[Expr]) -> (Vec<String>, bool, Option<String>) {
    let mut names = Vec::new();
    let mut has_rest = false;
    let mut rest_param = None;

    for (i, p) in params.iter().enumerate() {
        match p {
            Expr::Symbol(s) if s == "&rest" => {
                has_rest = true;
                // Next param is the rest collector
                if i + 1 < params.len()
                    && let Expr::Symbol(name) = &params[i + 1] {
                        rest_param = Some(name.clone());
                    }
                break;
            }
            Expr::Symbol(s) => {
                names.push(s.clone());
            }
            _ => {}
        }
    }

    (names, has_rest, rest_param)
}

fn expand_expr(expr: &Expr, macros: &HashMap<String, Macro>) -> Expr {
    match expr {
        Expr::List(items, span) if !items.is_empty() => {
            // Check if the head is a macro
            if let Expr::Symbol(head) = &items[0]
                && let Some(m) = macros.get(head) {
                    let args = &items[1..];
                    return expand_macro_call(m, args, macros);
                }
            // Otherwise, recursively expand sub-expressions
            Expr::List(items.iter().map(|e| expand_expr(e, macros)).collect(), *span)
        }
        _ => expr.clone(),
    }
}

fn expand_macro_call(m: &Macro, args: &[Expr], macros: &HashMap<String, Macro>) -> Expr {
    // Build bindings
    let mut bindings: HashMap<String, Expr> = HashMap::new();

    for (i, param) in m.params.iter().enumerate() {
        if i < args.len() {
            bindings.insert(param.clone(), args[i].clone());
        }
    }

    if m.has_rest
        && let Some(ref rest_name) = m.rest_param {
            let rest_start = m.params.len();
            let rest_args: Vec<Expr> = if rest_start < args.len() {
                args[rest_start..].to_vec()
            } else {
                Vec::new()
            };
            bindings.insert(rest_name.clone(), Expr::List(rest_args, None));
        }

    // Expand the template body
    let expanded = expand_template(&m.body, &bindings);
    // Re-expand to handle nested macro calls in the result
    expand_expr(&expanded, macros)
}

/// Walk the template body, replacing (unquote name) with bound values
/// and splicing (unquote-splicing name) into surrounding lists.
fn expand_template(expr: &Expr, bindings: &HashMap<String, Expr>) -> Expr {
    match expr {
        Expr::Symbol(_) | Expr::Number(_) | Expr::StringLit(_) => expr.clone(),

        Expr::List(items, _) if items.is_empty() => Expr::List(Vec::new(), None),

        Expr::List(items, span) => {
            // Check for (unquote name)
            if items.len() == 2
                && let Expr::Symbol(head) = &items[0] {
                    if head == "unquote" {
                        if let Expr::Symbol(name) = &items[1]
                            && let Some(val) = bindings.get(name) {
                                return val.clone();
                            }
                        // If name not found, return as-is
                        return expr.clone();
                    }
                    if head == "unquote-splicing" {
                        if let Expr::Symbol(name) = &items[1]
                            && let Some(val) = bindings.get(name) {
                                return val.clone();
                            }
                        return expr.clone();
                    }
                    if head == "quasiquote" {
                        return expand_template(&items[1], bindings);
                    }
                }

            // Expand the list, handling splicing
            let mut expanded: Vec<Expr> = Vec::new();
            let mut has_splice = false;
            for item in items {
                let is_splice = match item {
                    Expr::List(inner, _) if inner.len() == 2 => {
                        matches!(&inner[0], Expr::Symbol(s) if s == "unquote-splicing")
                    }
                    _ => false,
                };

                if is_splice {
                    if let Expr::List(inner, _) = item
                        && let Expr::Symbol(name) = &inner[1]
                            && let Some(val) = bindings.get(name) {
                                if let Expr::List(elems, _) = val {
                                    expanded.extend(elems.clone());
                                } else {
                                    expanded.push(val.clone());
                                }
                                has_splice = true;
                                continue;
                            }
                    expanded.push(item.clone());
                } else {
                    expanded.push(expand_template(item, bindings));
                }
            }
            let new_span = if has_splice { None } else { *span };
            Expr::List(expanded, new_span)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    #[test]
    fn test_simple_macro() {
        let src = "
        (defmacro when (condition &rest body)
          (quasiquote (if (unquote condition) (do (unquote-splicing body)))))

        (when (> x 0)
          (println! \"positive\")
          (println! \"still positive\"))
        ";
        let exprs = parser::parse(src).unwrap();
        let expanded = expand(&exprs);

        // The defmacro should be removed, leaving only the expanded call
        assert_eq!(expanded.len(), 1);

        // The expanded form should be an if
        if let Expr::List(items, _) = &expanded[0] {
            assert_eq!(items[0], Expr::Symbol("if".into()));
            assert_eq!(
                items[1],
                Expr::List(vec![
                    Expr::Symbol(">".into()),
                    Expr::Symbol("x".into()),
                    Expr::Number("0".into()),
                ], None)
            );
        } else {
            panic!("Expected list");
        }
    }

    #[test]
    fn test_fixed_arity_macro() {
        let src = "
        (defmacro double (x)
          (quasiquote (+ (unquote x) (unquote x))))

        (double 21)
        ";
        let exprs = parser::parse(src).unwrap();
        let expanded = expand(&exprs);
        assert_eq!(expanded.len(), 1);
        assert_eq!(
            expanded[0],
            Expr::List(vec![
                Expr::Symbol("+".into()),
                Expr::Number("21".into()),
                Expr::Number("21".into()),
            ], None)
        );
    }
}
