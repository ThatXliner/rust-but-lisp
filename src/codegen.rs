use crate::ast::Expr;
use std::cell::RefCell;

thread_local! {
    static WARNINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn warn(msg: impl Into<String>) {
    WARNINGS.with(|w| w.borrow_mut().push(msg.into()));
}

/// Compile a list of top-level s-expressions into Rust source code.
/// Returns the generated Rust code and any compile warnings.
pub fn compile(exprs: &[Expr]) -> (String, Vec<String>) {
    WARNINGS.with(|w| w.borrow_mut().clear());
    let mut out = String::new();
    for (i, expr) in exprs.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        out.push_str(&compile_top_level(expr));
    }
    let warnings = WARNINGS.with(|w| w.borrow_mut().drain(..).collect());
    (out, warnings)
}

/// Compile a top-level expression (items like fn, struct, enum, etc.).
fn compile_top_level(expr: &Expr) -> String {
    match expr {
        Expr::List(items) => {
            if items.is_empty() {
                return String::new();
            }
            let (vis, offset) = try_parse_visibility(items);
            let items = &items[offset..];
            if items.is_empty() {
                return String::new();
            }
            let head = &items[0];
            match head {
                Expr::Symbol(s) if s == "fn" => compile_fn_def(&items[1..], &vis),
                Expr::Symbol(s) if s == "struct" => compile_struct(&items[1..], &vis),
                Expr::Symbol(s) if s == "enum" => compile_enum(&items[1..], &vis),
                Expr::Symbol(s) if s == "trait" => compile_trait(&items[1..], &vis),
                Expr::Symbol(s) if s == "impl" => compile_impl_block(&items[1..]),
                Expr::Symbol(s) if s == "mod" => compile_mod(&items[1..], &vis),
                Expr::Symbol(s) if s == "use" => compile_use(&items[1..], &vis),
                Expr::Symbol(s) if s == "const" => compile_const(&items[1..], &vis),
                Expr::Symbol(s) if s == "static" => compile_static(&items[1..], &vis),
                _ => format!("{};", compile_expr(expr)),
            }
        }
        _ => format!("{};", compile_expr(expr)),
    }
}

/// Try to parse a visibility modifier from the front of an item list.
/// Returns (visibility_string, items_consumed).
///
/// Recognizes:
///   pub              → "pub ", 1
///   (pub crate)      → "pub(crate) ", 1
///   (pub super)      → "pub(super) ", 1
///   (pub (in path))  → "pub(in path) ", 1
fn try_parse_visibility(items: &[Expr]) -> (String, usize) {
    if items.is_empty() {
        return (String::new(), 0);
    }
    match &items[0] {
        Expr::Symbol(s) if s == "pub" => {
            // Check for a visibility restriction list: (pub (crate) fn ...), (pub (super) fn ...)
            if items.len() > 1
                && let Expr::List(rest) = &items[1]
                    && !rest.is_empty() {
                        let rest_str: Vec<String> =
                            rest.iter().map(compile_expr).collect();
                        return (format!("pub({}) ", rest_str.join(" ")), 2);
                    }
            ("pub ".to_string(), 1)
        }
        Expr::List(vis_items) if !vis_items.is_empty() => {
            if let Expr::Symbol(head) = &vis_items[0]
                && head == "pub" {
                    let rest: Vec<String> = vis_items[1..]
                        .iter()
                        .map(compile_expr)
                        .collect();
                    if rest.is_empty() {
                        return ("pub ".to_string(), 1);
                    }
                    return (format!("pub({}) ", rest.join(" ")), 1);
                }
            (String::new(), 0)
        }
        _ => (String::new(), 0),
    }
}

/// Compile a single expression.
fn compile_expr(expr: &Expr) -> String {
    match expr {
        Expr::Symbol(s) => s.clone(),
        Expr::Number(n) => n.replace('_', ""),
        Expr::StringLit(s) => {
            // Emit a Rust string literal with proper escaping.
            // The stored StringLit includes surrounding quotes; strip them
            // and re-escape for Rust output.
            let inner = &s[1..s.len() - 1]; // strip quotes
            let content = unescape_lisp_string(inner);
            format!("\"{}\"", escape_rust_string(&content))
        }
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
            "loop" => return compile_loop(&items[1..]),
            "while" => return compile_while(&items[1..]),
            "for" => return compile_for(&items[1..]),
            "lambda" => return compile_lambda(&items[1..]),
            "rust" => return compile_rust_block(&items[1..]),
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
fn compile_fn_def(args: &[Expr], vis: &str) -> String {
    if args.len() < 3 {
        warn("fn definition missing name, params, or return type");
        return "/* malformed fn */".to_string();
    }

    let (name, rest) = (&args[0], &args[1..]);

    let mut i = 0;

    // Parse optional generics
    let generics = if i < rest.len() {
        try_parse_generics(&rest[i])
            .inspect(|_| {
                i += 1;
            })
            .unwrap_or_default()
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
            "{}fn {}{}({}) -> {};",
            vis,
            compile_expr(name),
            generics,
            params,
            ret_type,
        )
    } else {
        format!(
            "{}fn {}{}({}) -> {} {{\n{}\n}}",
            vis,
            compile_expr(name),
            generics,
            params,
            ret_type,
            indent(&body)
        )
    }
}

/// Compile a closure expression.
///
/// (lambda (x y) (+ x y))                           => |x, y| { (x + y) }
/// (lambda ((x i32) (y i32)) i32 (+ x y))           => |x: i32, y: i32| -> i32 { (x + y) }
/// (lambda move (x) x)                              => move |x| { x }
/// (lambda move ((x i32) (y i32)) i32 (+ x y))      => move |x: i32, y: i32| -> i32 { (x + y) }
fn compile_lambda(args: &[Expr]) -> String {
    if args.is_empty() {
        return "|| {}".to_string();
    }

    let mut i = 0;

    // Check for `move` keyword
    let move_kw = if matches!(&args[0], Expr::Symbol(s) if s == "move") {
        i += 1;
        "move "
    } else {
        ""
    };

    if i >= args.len() {
        warn("lambda missing params or body");
        return "|| {}".to_string();
    }

    // Parse params: (x y z) or ((x i32) (y i32))
    let typed = match &args[i] {
        Expr::List(params) if !params.is_empty() => {
            matches!(&params[0], Expr::List(_))
        }
        _ => false,
    };

    let params = if typed {
        compile_params(&args[i])
    } else {
        // Untyped: just join symbols
        match &args[i] {
            Expr::List(params) => {
                params.iter().map(compile_expr).collect::<Vec<_>>().join(", ")
            }
            _ => compile_expr(&args[i]),
        }
    };
    i += 1;

    // Parse optional return type (only for typed params)
    let ret_type = if typed && i < args.len() {
        // If there's more than one expr left, the next is the return type
        if i + 1 < args.len() {
            let ret = compile_type_expr(&args[i]);
            i += 1;
            format!(" -> {}", ret)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Parse body
    let body = compile_body(&args[i..]);
    let body_str = if body.is_empty() {
        "{}".to_string()
    } else {
        format!("{{\n{}\n}}", indent(&body))
    };

    format!("{}|{}|{} {}", move_kw, params, ret_type, body_str)
}

/// Compile let binding.
fn compile_let(args: &[Expr]) -> String {
    if args.is_empty() {
        warn("let binding with no name or value");
        return "let _ = ();".to_string();
    }

    let mut i = 0;
    let mut mutable = false;

    // Check for `mut`
    if let Expr::Symbol(s) = &args[0]
        && s == "mut" {
            mutable = true;
            i += 1;
        }

    if i >= args.len() {
        warn("let binding missing name after mut");
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
fn compile_struct(args: &[Expr], vis: &str) -> String {
    if args.is_empty() {
        warn("struct definition missing name");
        return "struct _ {}".to_string();
    }

    let (name, rest) = (&args[0], &args[1..]);

    // Parse optional generics
    let generics = if !rest.is_empty() {
        try_parse_generics(&rest[0]).unwrap_or_default()
    } else {
        String::new()
    };
    let rest = if generics.is_empty() { rest } else { &rest[1..] };

    let fields: Vec<String> = rest.iter().map(compile_struct_field).collect();
    let fields_str = fields.join(",\n    ");

    format!(
        "{}struct {}{} {{\n    {}\n}}",
        vis,
        compile_expr(name),
        generics,
        fields_str
    )
}

/// Compile a struct field: (name type...) → name: type
/// Supports field visibility: (pub x i32) → pub x: i32
fn compile_struct_field(expr: &Expr) -> String {
    match expr {
        Expr::List(items) if items.len() >= 2 => {
            let (vis, offset) = try_parse_visibility(items);
            let items = &items[offset..];
            if items.is_empty() {
                return "_: ()".to_string();
            }
            let name = compile_expr(&items[0]);
            let type_parts: Vec<String> = items[1..].iter().map(compile_expr).collect();
            format!("{}{}: {}", vis, name, type_parts.join(" "))
        }
        Expr::Symbol(name) => format!("{}: ()", name),
        _ => "_: ()".to_string(),
    }
}

/// Compile enum definition.
fn compile_enum(args: &[Expr], vis: &str) -> String {
    if args.is_empty() {
        warn("enum definition missing name");
        return "enum _ {}".to_string();
    }

    let (name, rest) = (&args[0], &args[1..]);

    // Parse optional generics
    let generics = if !rest.is_empty() {
        try_parse_generics(&rest[0]).unwrap_or_default()
    } else {
        String::new()
    };
    let rest = if generics.is_empty() { rest } else { &rest[1..] };

    let variants: Vec<String> = rest.iter().map(compile_enum_variant).collect();
    let variants_str = variants.join(",\n    ");

    format!(
        "{}enum {}{} {{\n    {}\n}}",
        vis,
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
            let fields: Vec<String> = items[1..].iter().map(compile_expr).collect();
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
        .map(compile_match_arm)
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
        _ => "_ => {}".to_string(),
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
            let rest: Vec<String> = items[1..].iter().map(compile_pattern).collect();
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
        warn("if expression with no condition or body");
        return "if true {}".to_string();
    }

    let cond = compile_expr(&args[0]);
    let then_body = if args.len() >= 2 {
        compile_expr(&args[1])
    } else {
        warn("if expression with no then-branch");
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

/// Compile loop expression: (loop body...) → loop { body... }
fn compile_loop(args: &[Expr]) -> String {
    let body = compile_body(args);
    if body.is_empty() {
        "loop {}".to_string()
    } else {
        format!("loop {{\n{}\n}}", indent(&body))
    }
}

/// Compile while expression: (while condition body...) → while condition { body... }
fn compile_while(args: &[Expr]) -> String {
    if args.is_empty() {
        warn("while expression with no condition or body");
        return "while true {}".to_string();
    }

    let cond = compile_expr(&args[0]);
    let body = compile_body(&args[1..]);
    if body.is_empty() {
        warn("while expression with no body — use (loop) for infinite loops");
        format!("while {} {{}}", cond)
    } else {
        format!("while {} {{\n{}\n}}", cond, indent(&body))
    }
}

/// Compile for expression: (for pattern in iterable body...) → for pattern in iterable { body... }
fn compile_for(args: &[Expr]) -> String {
    if args.len() < 3 {
        warn("for expression missing pattern, iterator, or body");
        return format!("/* malformed for: {:?} */", args);
    }

    let pattern = compile_for_pattern(&args[0]);
    // args[1] should be "in"
    let iter = compile_expr(&args[2]);
    let body = compile_body(&args[3..]);
    if body.is_empty() {
        format!("for {} in {} {{}}", pattern, iter)
    } else {
        format!("for {} in {} {{\n{}\n}}", pattern, iter, indent(&body))
    }
}

/// Compile the pattern portion of a for loop.
/// (i x) → (i, x),  (Some(x)) → Some(x),  i → i
fn compile_for_pattern(expr: &Expr) -> String {
    match expr {
        Expr::Symbol(s) => s.clone(),
        Expr::List(items) if items.is_empty() => "()".to_string(),
        Expr::List(items) => {
            // If the head starts with uppercase, treat as enum variant pattern
            if let Expr::Symbol(head) = &items[0]
                && head.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                    return compile_pattern(expr);
                }
            // Otherwise treat as tuple destructure: (i x y) → (i, x, y)
            let parts: Vec<String> = items.iter().map(compile_expr).collect();
            format!("({})", parts.join(", "))
        }
        _ => compile_expr(expr),
    }
}

/// Compile `impl` block.
fn compile_impl_block(args: &[Expr]) -> String {
    if args.is_empty() {
        warn("impl block missing type name");
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
        } else if !args.is_empty() {
            (None, compile_expr(&args[0]), 1)
        } else {
            warn("impl block missing type name");
        return "impl _ {}".to_string();
        };

    let methods: Vec<String> = args[method_start..]
        .iter()
        .map(compile_top_level)
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
fn compile_trait(args: &[Expr], vis: &str) -> String {
    if args.is_empty() {
        warn("trait definition missing name");
        return "trait _ {}".to_string();
    }

    let name = compile_expr(&args[0]);
    let methods: Vec<String> = args[1..]
        .iter()
        .map(compile_top_level)
        .collect();

    let methods_str = methods.join("\n\n");

    format!("{}trait {} {{\n{}\n}}", vis, name, indent(&methods_str))
}

/// Compile a `mod` declaration.
/// (mod my_module body...) → mod my_module { body... }
/// (mod my_module) → mod my_module;
fn compile_mod(args: &[Expr], vis: &str) -> String {
    if args.is_empty() {
        warn("mod declaration missing name");
        return "mod _;".to_string();
    }

    let name = compile_expr(&args[0]);

    if args.len() == 1 {
        format!("{}mod {};", vis, name)
    } else {
        let body_items: Vec<String> = args[1..]
            .iter()
            .map(compile_top_level)
            .collect();
        let body = body_items.join("\n\n");
        format!("{}mod {} {{\n{}\n}}", vis, name, indent(&body))
    }
}

/// Compile a `use` declaration.
/// (use std::collections::HashMap) → use std::collections::HashMap;
/// (use std::collections::HashMap as MyMap) → use std::collections::HashMap as MyMap;
fn compile_use(args: &[Expr], vis: &str) -> String {
    if args.is_empty() {
        warn("use declaration with no path");
        return "/* empty use */;".to_string();
    }

    let path: Vec<String> = args.iter().map(|e| match e {
        Expr::StringLit(s) => {
            // Strip surrounding quotes so raw path content can be embedded
            s[1..s.len()-1].to_string()
        }
        _ => compile_expr(e),
    }).collect();

    format!("{}use {};", vis, path.join(" "))
}

/// Compile a `const` declaration.
/// (const MAX_SIZE usize 1024) → const MAX_SIZE: usize = 1024;
/// (pub const MAX_SIZE usize 1024) → pub const MAX_SIZE: usize = 1024;
fn compile_const(args: &[Expr], vis: &str) -> String {
    if args.len() < 3 {
        warn("const declaration missing name, type, or value");
        return format!("/* malformed const: {:?} */", args);
    }

    let name = compile_expr(&args[0]);
    let ty = compile_type_expr(&args[1]);
    let val = compile_expr(&args[2]);

    format!("{}const {}: {} = {};", vis, name, ty, val)
}

/// Compile a `static` declaration.
/// (static COUNTER i32 0) → static COUNTER: i32 = 0;
/// (static mut COUNTER i32 0) → static mut COUNTER: i32 = 0;
/// (pub static COUNTER i32 0) → pub static COUNTER: i32 = 0;
fn compile_static(args: &[Expr], vis: &str) -> String {
    if args.is_empty() {
        warn("static declaration missing name, type, or value");
        return format!("/* malformed static: {:?} */", args);
    }

    let mut i = 0;
    let mut mutable = false;

    if let Expr::Symbol(s) = &args[0]
        && s == "mut" {
            mutable = true;
            i = 1;
        }

    if i + 2 >= args.len() {
        warn("static declaration missing name, type, or value");
        return format!("/* malformed static: {:?} */", args);
    }

    let name = compile_expr(&args[i]);
    let ty = compile_type_expr(&args[i + 1]);
    let val = compile_expr(&args[i + 2]);

    let mut_str = if mutable { "mut " } else { "" };
    format!("{}static {}{}: {} = {};", vis, mut_str, name, ty, val)
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
        let call_args: Vec<String> = args[2..].iter().map(compile_expr).collect();
        format!("{}.{}({})", obj, method, call_args.join(", "))
    }
}

/// Compile a macro call: (foo! args...) → foo!(args...)
fn compile_macro_call(name: &str, args: &[Expr]) -> String {
    let args_str: Vec<String> = args.iter().map(compile_expr).collect();
    format!("{}({})", name, args_str.join(", "))
}

/// Compile a function call: (func arg1 arg2) → func(arg1, arg2)
/// For known binary operators with 2 args, emits infix form: (+ a b) → (a + b)
fn compile_fn_call(items: &[Expr]) -> String {
    let head = compile_expr(&items[0]);
    let args: Vec<String> = items[1..].iter().map(compile_expr).collect();

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
    s.chars().next().is_some_and(|c| c.is_ascii_uppercase())
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
                            parts[1..].iter().map(compile_expr).collect();
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
        Expr::List(items) => {
            items
                .iter()
                .map(compile_expr)
                .collect::<Vec<_>>()
                .join(" ")
        }
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
                let all_params = items
                    .iter()
                    .all(|e| matches!(e, Expr::Symbol(s) if is_type_param(s)));
                if !all_params {
                    return None;
                }
                0
            };
            if start >= items.len() {
                return Some(String::new());
            }
            let params: Vec<String> = items[start..].iter().map(compile_expr).collect();
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
    s.len() == 1 && s.chars().next().is_some_and(|c| c.is_ascii_uppercase())
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
    let indices: Vec<String> = args[1..].iter().map(compile_expr).collect();
    format!("{}[{}]", expr, indices.join(", "))
}

/// Compile an inline Rust block: (rust "raw_code") → raw_code
/// The string content is emitted verbatim after unescaping. Also accepts bare symbols.
fn compile_rust_block(args: &[Expr]) -> String {
    if args.is_empty() {
        return String::new();
    }

    let code = match &args[0] {
        Expr::StringLit(s) => {
            let inner = &s[1..s.len() - 1]; // strip quotes
            unescape_lisp_string(inner)
        }
        _ => compile_expr(&args[0]),
    };
    // Strip trailing semicolons — compile_body will add its own for non-last expressions
    code.trim_end_matches(';').to_string()
}

/// Escape special characters for Rust string literal output.
/// Converts literal newlines, tabs, etc. to their Rust escape sequences.
fn escape_rust_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out
}

/// Unescape basic LISP string escape sequences: \\\" → ", \\\\ → \\
fn unescape_lisp_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
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
            let rest: Vec<_> = items[1..].iter().map(compile_expr).collect();
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
