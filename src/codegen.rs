use crate::ast::Expr;

/// Compile a list of top-level s-expressions into Rust source code.
pub fn compile(exprs: &[Expr]) -> String {
    let mut out = String::new();
    for (i, expr) in exprs.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        out.push_str(&compile_top_level(expr));
    }
    out
}

/// Compile a top-level expression (items like fn, struct, enum, etc.).
fn compile_top_level(expr: &Expr) -> String {
    match expr {
        Expr::List(items) => {
            if items.is_empty() {
                return String::new();
            }
            let head = &items[0];
            match head {
                Expr::Symbol(s) if s == "fn" => compile_fn_def(&items[1..]),
                Expr::Symbol(s) if s == "struct" => compile_struct(&items[1..]),
                Expr::Symbol(s) if s == "enum" => compile_enum(&items[1..]),
                Expr::Symbol(s) if s == "trait" => compile_trait(&items[1..]),
                Expr::Symbol(s) if s == "impl" => compile_impl_block(&items[1..]),
                _ => format!("{};", compile_expr(expr)),
            }
        }
        _ => format!("{};", compile_expr(expr)),
    }
}

/// Compile a single expression.
fn compile_expr(expr: &Expr) -> String {
    match expr {
        Expr::Symbol(s) => s.clone(),
        Expr::Number(n) => n.replace('_', ""),
        Expr::StringLit(s) => s.clone(),
        Expr::List(items) => compile_list(items),
    }
}

/// Compile a list in expression position.
fn compile_list(items: &[Expr]) -> String {
    if items.is_empty() {
        return "()".to_string();
    }

    let head = &items[0];

    // Check for special forms
    if let Expr::Symbol(s) = head {
        match s.as_str() {
            "do" => return compile_do(&items[1..]),
            "let" => return compile_let(&items[1..]),
            "if" => return compile_if(&items[1..]),
            "match" => return compile_match(&items[1..]),
            "." => return compile_dot(&items[1..]),
            "new" => return compile_struct_new(&items[1..]),
            "[]" => return compile_index(&items[1..]),
            _ => {}
        }

        // Macro call: name ends with '!'
        if s.ends_with('!') {
            return compile_macro_call(s, &items[1..]);
        }
    }

    // Function call (including operators like +, -, etc.)
    compile_fn_call(items)
}

/// Compile a function definition.
fn compile_fn_def(args: &[Expr]) -> String {
    if args.len() < 3 {
        return format!("/* malformed fn: {:?} */", args);
    }

    let (name, rest) = (&args[0], &args[1..]);

    let mut i = 0;

    // Parse optional generics
    let generics = if i < rest.len() {
        try_parse_generics(&rest[i]).map(|g| {
            i += 1;
            g
        }).unwrap_or_default()
    } else {
        String::new()
    };

    // Parse parameter list: ((x i32) (y &str))
    let params = if i < rest.len() {
        compile_params(&rest[i])
    } else {
        String::new()
    };
    i += 1;

    // Parse return type: i32, (Option &T), etc.
    let ret_type = if i < rest.len() {
        compile_type_expr(&rest[i])
    } else {
        "()".to_string()
    };
    i += 1;

    // Parse body (remaining expressions)
    let body = compile_body(&rest[i..]);

    if body.trim().is_empty() {
        format!(
            "fn {}{}({}) -> {};",
            compile_expr(name),
            generics,
            params,
            ret_type,
        )
    } else {
        format!(
            "fn {}{}({}) -> {} {{\n{}\n}}",
            compile_expr(name),
            generics,
            params,
            ret_type,
            indent(&body)
        )
    }
}

/// Compile let binding.
fn compile_let(args: &[Expr]) -> String {
    if args.is_empty() {
        return "let _ = ();".to_string();
    }

    let mut i = 0;
    let mut mutable = false;

    // Check for `mut`
    if let Expr::Symbol(s) = &args[0] {
        if s == "mut" {
            mutable = true;
            i += 1;
        }
    }

    if i >= args.len() {
        return "let _ = ();".to_string();
    }

    let name = &args[i];
    i += 1;

    // Determine if there's a type annotation
    let (type_ann, value_start) = if i + 1 < args.len() {
        // If there's more than one expr remaining, the next one might be a type
        // Heuristic: treat the next expr as a type (it will be)
        (Some(compile_type_expr(&args[i])), i + 1)
    } else {
        (None, i)
    };

    let value = compile_body(&args[value_start..]);

    let mut_str = if mutable { "mut " } else { "" };
    if let Some(t) = type_ann {
        format!("let {}{}: {} = {}", mut_str, compile_expr(name), t, value)
    } else {
        format!("let {}{} = {}", mut_str, compile_expr(name), value)
    }
}

/// Compile struct definition.
fn compile_struct(args: &[Expr]) -> String {
    if args.is_empty() {
        return "struct _ {}".to_string();
    }

    let (name, rest) = (&args[0], &args[1..]);

    // Parse optional generics
    let generics = if !rest.is_empty() {
        try_parse_generics(&rest[0]).map(|g| g).unwrap_or_default()
    } else {
        String::new()
    };
    let rest = if generics.is_empty() { rest } else { &rest[1..] };

    let fields: Vec<String> = rest.iter().map(|f| compile_struct_field(f)).collect();
    let fields_str = fields.join(",\n    ");

    format!(
        "struct {}{} {{\n    {}\n}}",
        compile_expr(name),
        generics,
        fields_str
    )
}

/// Compile a struct field: (name type...) → name: type
fn compile_struct_field(expr: &Expr) -> String {
    match expr {
        Expr::List(items) if items.len() >= 2 => {
            let name = compile_expr(&items[0]);
            let type_parts: Vec<String> = items[1..].iter().map(|e| compile_expr(e)).collect();
            format!("{}: {}", name, type_parts.join(" "))
        }
        Expr::Symbol(name) => format!("{}: ()", name),
        _ => format!("_: ()"),
    }
}

/// Compile enum definition.
fn compile_enum(args: &[Expr]) -> String {
    if args.is_empty() {
        return "enum _ {}".to_string();
    }

    let (name, rest) = (&args[0], &args[1..]);

    // Parse optional generics
    let generics = if !rest.is_empty() {
        try_parse_generics(&rest[0]).map(|g| g).unwrap_or_default()
    } else {
        String::new()
    };
    let rest = if generics.is_empty() { rest } else { &rest[1..] };

    let variants: Vec<String> = rest.iter().map(|v| compile_enum_variant(v)).collect();
    let variants_str = variants.join(",\n    ");

    format!(
        "enum {}{} {{\n    {}\n}}",
        compile_expr(name),
        generics,
        variants_str
    )
}

/// Compile an enum variant: (Name T1 T2) → Name(T1, T2), or Name → Name
fn compile_enum_variant(expr: &Expr) -> String {
    match expr {
        Expr::List(items) if !items.is_empty() => {
            let name = compile_expr(&items[0]);
            let fields: Vec<String> = items[1..].iter().map(|e| compile_expr(e)).collect();
            if fields.is_empty() {
                name
            } else {
                format!("{}({})", name, fields.join(", "))
            }
        }
        Expr::Symbol(name) => name.clone(),
        _ => format!("_ /* {:?} */", expr),
    }
}

/// Compile match expression.
fn compile_match(args: &[Expr]) -> String {
    if args.is_empty() {
        return "match _ {}".to_string();
    }

    let value = compile_expr(&args[0]);
    let arms: Vec<String> = args[1..]
        .iter()
        .map(|arm| compile_match_arm(arm))
        .collect();
    let arms_str = arms.join(",\n");

    format!("match {} {{\n{}\n}}", value, indent(&arms_str))
}

/// Compile a match arm: ((pattern) body...) → pattern => { body... }
fn compile_match_arm(expr: &Expr) -> String {
    match expr {
        Expr::List(items) if items.len() >= 2 => {
            let pattern = compile_pattern(&items[0]);
            let body = compile_body(&items[1..]);
            format!("{} => {{ {} }}", pattern, body)
        }
        Expr::List(items) if items.len() == 1 => {
            let pattern = compile_pattern(&items[0]);
            format!("{} => {{}}", pattern)
        }
        Expr::Symbol(s) => format!("{} => {{}}", s),
        _ => format!("_ => {{}}"),
    }
}

/// Compile a pattern: (Some x) → Some(x), _ → _, etc.
fn compile_pattern(expr: &Expr) -> String {
    match expr {
        Expr::Symbol(s) if s == "_" => "_".to_string(),
        Expr::Symbol(s) => s.clone(),
        Expr::List(items) if items.is_empty() => "()".to_string(),
        Expr::List(items) => {
            let head = compile_expr(&items[0]);
            let rest: Vec<String> = items[1..].iter().map(|e| compile_pattern(e)).collect();
            if rest.is_empty() {
                head
            } else {
                format!("{}({})", head, rest.join(", "))
            }
        }
        _ => "_".to_string(),
    }
}

/// Compile if expression.
fn compile_if(args: &[Expr]) -> String {
    if args.is_empty() {
        return "if true {}".to_string();
    }

    let cond = compile_expr(&args[0]);
    let then_body = if args.len() >= 2 {
        compile_expr(&args[1])
    } else {
        "()".to_string()
    };

    let then_block = if then_body.starts_with('{') {
        then_body
    } else {
        format!("{{ {} }}", then_body)
    };

    if args.len() >= 3 {
        let else_body = compile_expr(&args[2]);
        let else_block = if else_body.starts_with('{') {
            else_body
        } else {
            format!("{{ {} }}", else_body)
        };
        format!("if {} {} else {}", cond, then_block, else_block)
    } else {
        format!("if {} {}", cond, then_block)
    }
}

/// Compile `impl` block.
fn compile_impl_block(args: &[Expr]) -> String {
    if args.is_empty() {
        return "impl _ {}".to_string();
    }

    // Check for `impl Trait for Type`
    let (trait_name, type_name, method_start): (Option<String>, String, usize) =
        if args.len() >= 3 {
            if let Expr::Symbol(s) = &args[1] {
                if s == "for" {
                    (
                        Some(compile_expr(&args[0])),
                        compile_expr(&args[2]),
                        3,
                    )
                } else {
                    (None, compile_expr(&args[0]), 1)
                }
            } else {
                (None, compile_expr(&args[0]), 1)
            }
        } else if args.len() >= 1 {
            (None, compile_expr(&args[0]), 1)
        } else {
            return "impl _ {}".to_string();
        };

    let methods: Vec<String> = args[method_start..]
        .iter()
        .map(|m| compile_top_level(m))
        .collect();
    let methods_str = methods.join("\n\n");

    let impl_header = if let Some(t) = trait_name {
        format!("impl {} for {}", t, type_name)
    } else {
        format!("impl {}", type_name)
    };

    format!("{} {{\n{}\n}}", impl_header, indent(&methods_str))
}

/// Compile trait definition.
fn compile_trait(args: &[Expr]) -> String {
    if args.is_empty() {
        return "trait _ {}".to_string();
    }

    let name = compile_expr(&args[0]);
    let methods: Vec<String> = args[1..]
        .iter()
        .map(|m| compile_top_level(m))
        .collect();

    let methods_str = methods.join("\n\n");
    // For trait method signatures (no body), add semicolons
    // A fn def in a trait has no body and ends with ;
    // We need to detect this... For now, we compile fn bodies and all

    format!("trait {} {{\n{}\n}}", name, indent(&methods_str))
}

/// Compile dot access: (. expr field) or (. expr method args...)
fn compile_dot(args: &[Expr]) -> String {
    if args.is_empty() {
        return ".".to_string();
    }

    let obj = compile_expr(&args[0]);

    if args.len() == 2 {
        // Field access: (. obj field)
        let field = compile_expr(&args[1]);
        format!("{}.{}", obj, field)
    } else {
        // Method call: (. obj method args...)
        let method = compile_expr(&args[1]);
        let call_args: Vec<String> = args[2..].iter().map(|a| compile_expr(a)).collect();
        format!("{}.{}({})", obj, method, call_args.join(", "))
    }
}

/// Compile a macro call: (foo! args...) → foo!(args...)
fn compile_macro_call(name: &str, args: &[Expr]) -> String {
    let args_str: Vec<String> = args.iter().map(|a| compile_expr(a)).collect();
    format!("{}({})", name, args_str.join(", "))
}

/// Compile a function call: (func arg1 arg2) → func(arg1, arg2)
/// For known binary operators with 2 args, emits infix form: (+ a b) → (a + b)
fn compile_fn_call(items: &[Expr]) -> String {
    let head = compile_expr(&items[0]);
    let args: Vec<String> = items[1..].iter().map(|a| compile_expr(a)).collect();

    if is_binary_op(&head) && args.len() == 2 {
        return format!("({} {} {})", args[0], head, args[1]);
    }

    format!("{}({})", head, args.join(", "))
}

fn is_binary_op(s: &str) -> bool {
    matches!(
        s,
        "+" | "-" | "*" | "/" | "%"
            | "==" | "!=" | "<" | ">" | "<=" | ">="
            | "&&" | "||"
            | "&" | "|" | "^" | "<<" | ">>"
            | "=" | "+=" | "-=" | "*=" | "/="
    )
}

fn is_uppercase_type(s: &str) -> bool {
    s.chars().next().map_or(false, |c| c.is_ascii_uppercase())
}

/// Compile a sequence of expressions as a block body.
/// All but the last get semicolons.
fn compile_body(exprs: &[Expr]) -> String {
    if exprs.is_empty() {
        return String::new();
    }

    let last_idx = exprs.len() - 1;
    let mut parts: Vec<String> = Vec::new();
    for (i, expr) in exprs.iter().enumerate() {
        let compiled = compile_expr(expr);
        if i == last_idx {
            parts.push(compiled);
        } else {
            parts.push(format!("{};", compiled));
        }
    }
    parts.join("\n")
}

/// Compile function parameters: ((x i32) (y &str)) → x: i32, y: &str
fn compile_params(expr: &Expr) -> String {
    match expr {
        Expr::List(items) => {
            let params: Vec<String> = items
                .iter()
                .map(|p| match p {
                    Expr::List(parts) if parts.is_empty() => String::new(),
                    // Single-element list: just the name (e.g. (&self))
                    Expr::List(parts) if parts.len() == 1 => compile_expr(&parts[0]),
                    // Multi-element list: name is first, rest is type
                    Expr::List(parts) => {
                        let name = compile_param_name(&parts[0]);
                        let type_parts: Vec<String> =
                            parts[1..].iter().map(|e| compile_expr(e)).collect();
                        format!("{}: {}", name, type_parts.join(" "))
                    }
                    _ => compile_expr(p),
                })
                .collect();
            params.join(", ")
        }
        _ => compile_expr(expr),
    }
}

/// Compile a param name, joining list elements with spaces (e.g., (&mut f) → "&mut f").
fn compile_param_name(expr: &Expr) -> String {
    match expr {
        Expr::List(items) => items.iter().map(|e| compile_expr(e)).collect::<Vec<_>>().join(" "),
        _ => compile_expr(expr),
    }
}

/// Try to parse generics from an expression. Returns Some(generics_str) if this looks
/// like generics, None otherwise. Supports:
/// - `(< T U V)` — list with `<` marker (always generics)
/// - `(T U)` — list without marker (heuristic: all elements are single uppercase letters or lifetimes)
/// - `<'a>` or `<T>` — bare symbol
fn try_parse_generics(expr: &Expr) -> Option<String> {
    match expr {
        Expr::List(items) if !items.is_empty() => {
            let start = if matches!(&items[0], Expr::Symbol(s) if s == "<") {
                1
            } else {
                // Without `<` marker, use heuristic: all elements must look like type params
                let all_params = items.iter().all(|e| matches!(e, Expr::Symbol(s) if is_type_param(s)));
                if !all_params {
                    return None;
                }
                0
            };
            if start >= items.len() {
                return Some(String::new());
            }
            let params: Vec<String> = items[start..].iter().map(|e| compile_expr(e)).collect();
            Some(format!("<{}>", params.join(", ")))
        }
        Expr::Symbol(s) if s.starts_with('<') && s.ends_with('>') => Some(s.clone()),
        _ => None,
    }
}

/// A type parameter is a single uppercase letter, or a lifetime like 'a.
fn is_type_param(s: &str) -> bool {
    if s.starts_with('\'') {
        return s.len() == 2; // 'a, 'b, etc.
    }
    s.len() == 1 && s.chars().next().map_or(false, |c| c.is_ascii_uppercase())
}

/// Compile do block: (do expr1 expr2) → { expr1; expr2 }
fn compile_do(args: &[Expr]) -> String {
    let body = compile_body(args);
    format!("{{\n{}\n}}", indent(&body))
}

/// Compile index access: ([] expr index) → expr[index]
fn compile_index(args: &[Expr]) -> String {
    if args.is_empty() {
        return "[]".to_string();
    }
    let expr = compile_expr(&args[0]);
    let indices: Vec<String> = args[1..].iter().map(|a| compile_expr(a)).collect();
    format!("{}[{}]", expr, indices.join(", "))
}

/// Compile struct construction: (new Type (field val) (field val)) → Type { field: val, field: val }
fn compile_struct_new(args: &[Expr]) -> String {
    if args.is_empty() {
        return "{}".to_string();
    }
    let type_name = compile_expr(&args[0]);
    let fields: Vec<String> = args[1..]
        .iter()
        .map(|f| match f {
            Expr::List(items) if items.len() == 2 => {
                let name = compile_expr(&items[0]);
                let val = compile_expr(&items[1]);
                format!("{}: {}", name, val)
            }
            _ => compile_expr(f),
        })
        .collect();
    format!("{} {{ {} }}", type_name, fields.join(", "))
}

/// Compile a type expression. Types can be single symbols or lists like (& T).
fn compile_type_expr(expr: &Expr) -> String {
    match expr {
        Expr::List(items) if !items.is_empty() => {
            let first = compile_expr(&items[0]);
            let rest: Vec<_> = items[1..].iter().map(|e| compile_expr(e)).collect();
            if rest.is_empty() {
                first
            } else if is_uppercase_type(&first) {
                // (Option &T) → Option<&T>
                format!("{}<{}>", first, rest.join(", "))
            } else {
                // (& T) → &T, (&'a str) → &'a str
                format!("{} {}", first, rest.join(" "))
            }
        }
        _ => compile_expr(expr),
    }
}

/// Indent each line of a string by 4 spaces.
fn indent(s: &str) -> String {
    s.lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("    {}", line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
