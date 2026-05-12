use crate::ast::{Expr, Span};
use std::cell::RefCell;
use std::collections::HashMap;

/// Convert a lisp kebab-case identifier to a valid Rust identifier.
/// Hyphens become `__` (double underscore). Preserves operator symbols.
fn sanitize_ident(s: &str) -> String {
    // Never mangle binary operators — they are used verbatim in output.
    if is_binary_op(s) {
        return s.to_string();
    }
    s.replace('-', "__")
}

/// Wrap generic parameters in angle brackets for declaration sites.
fn wrap_generics(s: &str) -> String {
    if s.is_empty() {
        String::new()
    } else {
        format!("<{}>", s)
    }
}

#[derive(Debug, Clone)]
pub struct Warning {
    pub message: String,
    pub span: Option<Span>,
}

thread_local! {
    static WARNINGS: RefCell<Vec<Warning>> = const { RefCell::new(Vec::new()) };
    static CURRENT_SPAN: RefCell<Option<Span>> = const { RefCell::new(None) };
    static SEEN_IDENTS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

fn warn(msg: impl Into<String>) {
    let span = CURRENT_SPAN.with(|s| *s.borrow());
    WARNINGS.with(|w| w.borrow_mut().push(Warning {
        message: msg.into(),
        span,
    }));
}

/// Register a definition identifier and check for collisions.
/// Two different Lisp identifiers that produce the same Rust name (e.g.
/// `foo-bar` and `foo__bar` both → `foo__bar`) will trigger a warning.
fn register_ident(expr: &Expr) -> String {
    let (original, result) = match expr {
        Expr::Symbol(s) => {
            let sanitized = sanitize_ident(s);
            (s.clone(), sanitized)
        }
        _ => {
            let compiled = compile_expr(expr);
            (compiled.clone(), compiled)
        }
    };
    SEEN_IDENTS.with(|seen| {
        let mut seen = seen.borrow_mut();
        if let Some(prev) = seen.insert(result.clone(), original.clone()) {
            if prev != original {
                warn(format!(
                    "Identifier collision: '{}' and '{}' both map to '{}' in Rust",
                    prev, original, result
                ));
            }
        }
    });
    result
}

struct SpanGuard {
    prev: Option<Span>,
}

impl SpanGuard {
    fn enter(span: Option<Span>) -> Self {
        let prev = CURRENT_SPAN.with(|s| s.replace(span));
        Self { prev }
    }
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        CURRENT_SPAN.with(|s| {
            s.replace(self.prev);
        });
    }
}

/// Compile a list of top-level s-expressions into Rust source code.
/// Returns the generated Rust code and any compile warnings.
pub fn compile(exprs: &[Expr]) -> (String, Vec<Warning>) {
    WARNINGS.with(|w| w.borrow_mut().clear());
    SEEN_IDENTS.with(|s| s.borrow_mut().clear());
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
        Expr::List(items, span) => {
            let _guard = SpanGuard::enter(*span);
            if items.is_empty() {
                return String::new();
            }
            let (vis, offset) = try_parse_visibility(items);
            let items = &items[offset..];
            if items.is_empty() {
                return String::new();
            }
            let head = &items[0];
            // Attributes appear right after the head symbol:
            // (fn (derive must_use) important ...) or (struct (derive Debug) Point ...)
            let (attrs, attr_offset) = try_parse_attributes(&items[1..]);
            let body = if attr_offset > 0 {
                &items[1 + attr_offset..]
            } else {
                &items[1..]
            };
            match head {
                Expr::Symbol(s) if s == "fn" => compile_fn_def(&attrs, body, &vis),
                Expr::Symbol(s) if s == "struct" => compile_struct(&attrs, body, &vis),
                Expr::Symbol(s) if s == "enum" => compile_enum(&attrs, body, &vis),
                Expr::Symbol(s) if s == "trait" => compile_trait(body, &vis),
                Expr::Symbol(s) if s == "impl" => compile_impl_block(body),
                Expr::Symbol(s) if s == "mod" => compile_mod(body, &vis),
                Expr::Symbol(s) if s == "use" => compile_use(body, &vis),
                Expr::Symbol(s) if s == "const" => compile_const(body, &vis),
                Expr::Symbol(s) if s == "static" => compile_static(body, &vis),
                Expr::Symbol(s) if s == "type" => compile_type_alias(body, &vis),
                _ => format!("{};", compile_expr(expr)),
            }
        }
        _ => format!("{};", compile_expr(expr)),
    }
}

/// Try to parse attributes from the front of an item's argument list.
/// Returns (attribute_string, items_consumed).
///
/// Currently supports `(derive Trait1 Trait2 ...)`.
fn try_parse_attributes(items: &[Expr]) -> (String, usize) {
    if items.is_empty() {
        return (String::new(), 0);
    }
    match &items[0] {
        Expr::List(inner, _) if !inner.is_empty() => {
            match &inner[0] {
                Expr::Symbol(s) if s == "derive" => {
                    let traits: Vec<String> = inner[1..]
                        .iter()
                        .map(compile_expr)
                        .collect();
                    if traits.is_empty() {
                        warn("derive attribute with no traits");
                    }
                    (format!("#[derive({})]\n", traits.join(", ")), 1)
                }
                _ => (String::new(), 0),
            }
        }
        _ => (String::new(), 0),
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
                && let Expr::List(rest, _) = &items[1]
                    && !rest.is_empty() {
                        let rest_str: Vec<String> =
                            rest.iter().map(compile_expr).collect();
                        return (format!("pub({}) ", rest_str.join(" ")), 2);
                    }
            ("pub ".to_string(), 1)
        }
        Expr::List(vis_items, _) if !vis_items.is_empty() => {
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
        Expr::Symbol(s) => sanitize_ident(s),
        Expr::Number(n) => n.replace('_', ""),
        Expr::StringLit(s) => {
            // Emit a Rust string literal with proper escaping.
            // The stored StringLit includes surrounding quotes; strip them
            // and re-escape for Rust output.
            let inner = &s[1..s.len() - 1]; // strip quotes
            let content = unescape_lisp_string(inner);
            format!("\"{}\"", escape_rust_string(&content))
        }
        Expr::List(items, span) => {
            let _guard = SpanGuard::enter(*span);
            compile_list(items)
        }
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
            "raw_new" => return compile_raw_new(&items[1..]),
            "[]" => return compile_index(&items[1..]),
            "::" => return compile_turbofish(&items[1..]),
            "break" => return compile_break(&items[1..]),
            "continue" => return compile_continue(&items[1..]),
            "return" => return compile_return(&items[1..]),
            "as" => return compile_cast(&items[1..]),
            "if-let" => return compile_if_let(&items[1..]),
            "else-if" => return compile_else_if(&items[1..]),
            "while-let" => return compile_while_let(&items[1..]),
            "unsafe" => return compile_unsafe(&items[1..]),
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
/// Return type defaults to `()` when omitted.
/// (fn bar ())          → fn bar();
/// (fn bar () i32 42)   → fn bar() -> i32 { 42 }
fn compile_fn_def(attrs: &str, args: &[Expr], vis: &str) -> String {
    if args.len() < 2 {
        warn("fn definition missing name or params");
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

    // Parse optional where clause
    let where_clause = if i < rest.len() {
        try_parse_where(&rest[i..])
            .inspect(|_| {
                i += 1;
            })
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Parse parameter list: ((x i32) (y &str)) or just ()
    let params = if i < rest.len() {
        compile_params(&rest[i])
    } else {
        String::new()
    };
    i += 1;

    // Parse return type (optional, defaults to ())
    let ret_type = if i < rest.len() {
        let ret = compile_type_expr(&rest[i]);
        i += 1;
        ret
    } else {
        "()".to_string()
    };

    // Parse body (remaining expressions)
    let body = compile_body(&rest[i..]);

    let gen_and_where = join_gen_and_where(&generics, &where_clause);

    if body.trim().is_empty() {
        format!(
            "{}{}fn {}{}({}) -> {};",
            attrs,
            vis,
            register_ident(name),
            gen_and_where,
            params,
            ret_type,
        )
    } else {
        format!(
            "{}{}fn {}{}({}) -> {} {{\n{}\n}}",
            attrs,
            vis,
            register_ident(name),
            gen_and_where,
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
        Expr::List(params, _) if !params.is_empty() => {
            matches!(&params[0], Expr::List(_, _))
        }
        _ => false,
    };

    let params = if typed {
        compile_params(&args[i])
    } else {
        // Untyped: just join symbols
        match &args[i] {
            Expr::List(params, _) => {
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
        return "let _ = ()".to_string();
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
        return "let _ = ()".to_string();
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
        format!("let {}{}: {} = {}", mut_str, register_ident(name), t, value)
    } else {
        format!("let {}{} = {}", mut_str, register_ident(name), value)
    }
}

/// Compile struct definition.
///
/// Detects three forms:
/// - Named fields: (struct Point (x f64) (y f64)) → struct Point { x: f64, y: f64 }
/// - Tuple fields: (struct Point f64 f64) → struct Point(f64, f64);
/// - Unit struct:  (struct Point) → struct Point;
fn compile_struct(attrs: &str, args: &[Expr], vis: &str) -> String {
    if args.is_empty() {
        warn("struct definition missing name");
        return format!("{}struct _ {{}}", attrs);
    }

    let (name, rest) = (&args[0], &args[1..]);

    // Parse optional generics
    let generics = if !rest.is_empty() {
        try_parse_generics(&rest[0]).unwrap_or_default()
    } else {
        String::new()
    };
    let mut rest = if generics.is_empty() { rest } else { &rest[1..] };

    // Parse optional where clause
    let where_clause = if !rest.is_empty() {
        try_parse_where(rest).unwrap_or_default()
    } else {
        String::new()
    };
    if !where_clause.is_empty() {
        rest = &rest[1..];
    }

    let where_part = if where_clause.is_empty() {
        String::new()
    } else {
        format!(" {}", where_clause)
    };

    // Detect struct kind
    let all_symbols = rest.iter().all(|e| matches!(e, Expr::Symbol(_)));
    let all_lists = rest.iter().all(|e| matches!(e, Expr::List(..)));

    let g = wrap_generics(&generics);

    if rest.is_empty() {
        // Unit struct (no fields): (struct Unit) → struct Unit;
        format!("{}{}struct {}{}{};", attrs, vis, register_ident(name), g, where_part)
    } else if all_symbols {
        // Tuple struct: (struct Point f64 f64) → struct Point(f64, f64);
        let fields: Vec<String> = rest.iter().map(compile_expr).collect();
        format!(
            "{}{}struct {}{}({}){};",
            attrs,
            vis,
            register_ident(name),
            g,
            fields.join(", "),
            where_part,
        )
    } else if all_lists {
        // Named-field struct: (struct Point (x f64) (y f64))
        let fields: Vec<String> = rest.iter().map(compile_struct_field).collect();
        let fields_str = fields.join(",\n    ");
        format!(
            "{}{}struct {}{} {{\n    {}\n}}{}",
            attrs,
            vis,
            register_ident(name),
            g,
            fields_str,
            where_part,
        )
    } else {
        // Mixed — warn and treat as named
        warn("struct has mixed named and bare fields — treating as named");
        let fields: Vec<String> = rest.iter().map(|e| match e {
            Expr::Symbol(s) => format!("{}: ()", s),
            _ => compile_struct_field(e),
        }).collect();
        let fields_str = fields.join(",\n    ");
        format!(
            "{}{}struct {}{} {{\n    {}\n}}{}",
            attrs,
            vis,
            register_ident(name),
            g,
            fields_str,
            where_part,
        )
    }
}

/// Compile a struct field: (name type...) → name: type
/// Supports field visibility: (pub x i32) → pub x: i32
fn compile_struct_field(expr: &Expr) -> String {
    match expr {
        Expr::List(items, _) if items.len() >= 2 => {
            let (vis, offset) = try_parse_visibility(items);
            let items = &items[offset..];
            if items.is_empty() {
                return "_: ()".to_string();
            }
            let name = register_ident(&items[0]);
            let type_parts: Vec<String> = items[1..].iter().map(compile_type_expr).collect();
            format!("{}{}: {}", vis, name, type_parts.join(" "))
        }
        Expr::Symbol(_name) => format!("{}: ()", register_ident(expr)),
        _ => "_: ()".to_string(),
    }
}

/// Compile enum definition.
fn compile_enum(attrs: &str, args: &[Expr], vis: &str) -> String {
    if args.is_empty() {
        warn("enum definition missing name");
        return format!("{}enum _ {{}}", attrs);
    }

    let (name, rest) = (&args[0], &args[1..]);

    // Parse optional generics
    let generics = if !rest.is_empty() {
        try_parse_generics(&rest[0]).unwrap_or_default()
    } else {
        String::new()
    };
    let mut rest = if generics.is_empty() { rest } else { &rest[1..] };

    // Parse optional where clause
    let where_clause = if !rest.is_empty() {
        try_parse_where(rest).unwrap_or_default()
    } else {
        String::new()
    };
    if !where_clause.is_empty() {
        rest = &rest[1..];
    }

    let where_part = if where_clause.is_empty() {
        String::new()
    } else {
        format!(" {}", where_clause)
    };

    let variants: Vec<String> = rest.iter()
        .filter(|v| !matches!(v, Expr::List(items, _) if items.is_empty()))
        .map(compile_enum_variant).collect();
    let variants_str = variants.join(",\n    ");

    format!(
        "{}{}enum {}{} {{\n    {}\n}}{}",
        attrs,
        vis,
        register_ident(name),
        wrap_generics(&generics),
        variants_str,
        where_part,
    )
}

/// Compile an enum variant: (Name T1 T2) → Name(T1, T2), or Name → Name
/// Returns None for empty lists or invalid forms.
fn compile_enum_variant(expr: &Expr) -> String {
    match expr {
        Expr::List(items, _) if !items.is_empty() => {
            let name = register_ident(&items[0]);
            let fields: Vec<String> = items[1..].iter().map(compile_type_expr).collect();
            if fields.is_empty() {
                name
            } else {
                format!("{}({})", name, fields.join(", "))
            }
        }
        Expr::Symbol(_) => register_ident(expr),
        _ => compile_expr(expr),
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
/// Supports guards: ((pattern) if condition body...) → pattern if condition => { body... }
fn compile_match_arm(expr: &Expr) -> String {
    match expr {
        Expr::List(items, _) if items.len() >= 2 => {
            let pattern = compile_pattern(&items[0]);
            // Check for guard: items[1] == "if"
            let (guard, body_start) = if items.len() >= 3
                && matches!(&items[1], Expr::Symbol(s) if s == "if")
            {
                (format!(" if {}", compile_expr(&items[2])), 3)
            } else {
                (String::new(), 1)
            };
            let body = compile_body(&items[body_start..]);
            if body.is_empty() {
                format!("{}{} => {{}}", pattern, guard)
            } else {
                format!("{}{} => {{ {} }}", pattern, guard, body)
            }
        }
        Expr::List(items, _) if items.len() == 1 => {
            let pattern = compile_pattern(&items[0]);
            format!("{} => {{}}", pattern)
        }
        Expr::Symbol(s) => format!("{} => {{}}", s),
        _ => "_ => {}".to_string(),
    }
}

/// Compile a pattern for match, if-let, while-let, and for.
///
///   (Some x)         → Some(x)        (enum variant with 1 field)
///   (Some x y)       → Some(x, y)     (enum variant with 2 fields)
///   ((x y))          → (x, y)         (tuple destructure — double parens)
///   x                → x              (binding)
///   _                → _              (wildcard)
fn compile_pattern(expr: &Expr) -> String {
    match expr {
        Expr::Symbol(s) if s == "_" => "_".to_string(),
        Expr::Symbol(s) => sanitize_ident(s),
        Expr::List(items, _) if items.is_empty() => "()".to_string(),
        Expr::List(items, _) => {
            // Tuple destructure: ((x y)) → (x, y)
            if items.len() == 1
                && let Expr::List(inner, _) = &items[0]
                && !inner.is_empty() {
                    let parts: Vec<String> = inner.iter().map(compile_pattern).collect();
                    return format!("({})", parts.join(", "));
                }
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
        let is_else_chain = else_body.starts_with("else");
        let else_block = if else_body.starts_with('{') || is_else_chain {
            else_body
        } else {
            format!("{{ {} }}", else_body)
        };
        // Don't double up on `else` when the body already supplies it (else-if chains)
        if is_else_chain {
            format!("if {} {} {}", cond, then_block, else_block)
        } else {
            format!("if {} {} else {}", cond, then_block, else_block)
        }
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

    let (iter_idx, body_start) = if let Expr::Symbol(s) = &args[1]
        && s == "in" {
            (2, 3)
        } else {
            warn(format!(
                "for expression missing 'in' keyword — found '{}'; treating it as the iterator",
                compile_expr(&args[1])
            ));
            (1, 2)
        };

    let pattern = compile_for_pattern(&args[0]);
    if iter_idx >= args.len() {
        warn("for expression missing iterator after 'in'");
        return format!("for {} in _ {{}}", pattern);
    }
    let iter = compile_expr(&args[iter_idx]);
    let body = compile_body(&args[body_start..]);
    if body.is_empty() {
        format!("for {} in {} {{}}", pattern, iter)
    } else {
        format!("for {} in {} {{\n{}\n}}", pattern, iter, indent(&body))
    }
}

/// Compile the pattern portion of a for loop.
/// Delegates to `compile_pattern`; use double parens for tuple destructure:
/// ((i x)) → (i, x).
fn compile_for_pattern(expr: &Expr) -> String {
    compile_pattern(expr)
}

/// Compile `impl` block.
/// (impl (generic T) (Vec T) ((fn push ...) (fn pop ...)))
///   → impl<T> Vec<T> { fn push() fn pop() }
/// (impl (generic T) (where (T Display)) Display for (MyType T) ((fn fmt ...)))
///   → impl<T> Display for MyType<T> where T: Display { fn fmt() }
fn compile_impl_block(args: &[Expr]) -> String {
    if args.is_empty() {
        warn("impl block missing type name");
        return "impl _ {}".to_string();
    }

    let mut i = 0;

    // Parse optional generics
    let generics = if i < args.len() {
        try_parse_generics(&args[i])
            .inspect(|_| { i += 1; })
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Parse optional where clause
    let where_clause = if i < args.len() {
        try_parse_where(&args[i..])
            .inspect(|_| { i += 1; })
            .unwrap_or_default()
    } else {
        String::new()
    };

    if i >= args.len() {
        warn("impl block missing type name (after generics/where)");
        return "impl _ {}".to_string();
    }

    // Check for `impl Trait for Type`. Type name always uses `compile_type_expr`.
    let (trait_name, type_name, body_start): (Option<String>, String, usize) =
        if i + 2 < args.len()
            && matches!(&args[i + 1], Expr::Symbol(s) if s == "for")
        {
            (
                Some(compile_type_expr(&args[i])),
                compile_type_expr(&args[i + 2]),
                i + 3,
            )
        } else {
            (None, compile_type_expr(&args[i]), i + 1)
        };

    // Parse body: a wrapped list of items
    let methods = if body_start < args.len() {
        if let Expr::List(inner, _) = &args[body_start] {
            inner.iter().map(compile_top_level).collect::<Vec<_>>()
        } else {
            warn("impl body should be a wrapped list");
            vec![]
        }
    } else {
        vec![]
    };
    let methods_str = methods.join("\n\n");

    // Where clause goes after the type: impl<T> Display for MyType<T> where T: Display { ... }
    let where_part = if where_clause.is_empty() {
        String::new()
    } else {
        format!(" {}", where_clause)
    };

    let g = wrap_generics(&generics);
    let impl_header = if let Some(t) = trait_name {
        format!("impl{} {} for {}{}", g, t, type_name, where_part)
    } else {
        format!("impl{} {}{}", g, type_name, where_part)
    };

    format!("{} {{\n{}\n}}", impl_header, indent(&methods_str))
}

/// Compile trait definition.
/// (trait Foo Display (where (Self Sized)) ((fn bar () ()) (type Item)))
///   → trait Foo: Display where Self: Sized { fn bar(); type Item; }
fn compile_trait(args: &[Expr], vis: &str) -> String {
    if args.is_empty() {
        warn("trait definition missing name");
        return "trait _ {}".to_string();
    }

    let name = register_ident(&args[0]);
    let mut i = 1;

    // Parse optional generics
    let generics = if i < args.len() {
        try_parse_generics(&args[i])
            .inspect(|_| { i += 1; })
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Parse optional supertraits
    let supertraits = if i < args.len() {
        try_parse_supertraits(&args[i..])
            .map(|(s, consumed)| { i += consumed; s })
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Parse optional where clause
    let where_clause = if i < args.len() {
        try_parse_where(&args[i..])
            .inspect(|_| { i += 1; })
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Parse body: a wrapped list of items
    let body_items = if i < args.len() {
        if let Expr::List(inner, _) = &args[i] {
            inner.iter().map(|item| {
                if let Expr::List(inner_inner, _) = item {
                    if !inner_inner.is_empty()
                        && matches!(&inner_inner[0], Expr::Symbol(s) if s == "type")
                    {
                        return compile_trait_type_alias(&inner_inner[1..]);
                    }
                }
                compile_top_level(item)
            }).collect::<Vec<_>>()
        } else {
            warn("trait body should be a wrapped list");
            vec![]
        }
    } else {
        vec![]
    };

    let body = body_items.join("\n\n");

    let mut header_extra = String::new();
    if !generics.is_empty() {
        header_extra.push_str(&wrap_generics(&generics));
    }
    if !supertraits.is_empty() {
        header_extra.push_str(&supertraits);
    }
    if !where_clause.is_empty() {
        header_extra.push(' ');
        header_extra.push_str(&where_clause);
    }

    format!("{}trait {}{} {{\n{}\n}}", vis, name, header_extra, indent(&body))
}

/// Compile an associated type declaration inside a trait.
/// (type Item)            → type Item;
/// (type Item Display)    → type Item: Display;
/// (type Item (+ Display Clone)) → type Item: Display + Clone;
fn compile_trait_type_alias(args: &[Expr]) -> String {
    if args.is_empty() {
        warn("associated type missing name");
        return "type _;".to_string();
    }
    let name = register_ident(&args[0]);
    if args.len() >= 2 {
        let bounds: Vec<String> = args[1..].iter().map(compile_expr).collect();
        format!("type {}: {};", name, bounds.join(" + "))
    } else {
        format!("type {};", name)
    }
}

/// Compile a `mod` declaration.
/// (mod my_module)              → mod my_module;         (external, no body)
/// (mod my_module ((fn body)))  → mod my_module { ... }  (inline, wrapped body)
fn compile_mod(args: &[Expr], vis: &str) -> String {
    if args.is_empty() {
        warn("mod declaration missing name");
        return "mod _;".to_string();
    }

    let name = register_ident(&args[0]);

    // Body is an optional wrapped list
    let body_items = if args.len() >= 2 {
        if let Expr::List(inner, _) = &args[1] {
            inner.iter().map(compile_top_level).collect::<Vec<_>>()
        } else {
            warn("mod body should be a wrapped list; treating as external module");
            return format!("{}mod {};", vis, name);
        }
    } else {
        vec![]
    };

    if body_items.is_empty() {
        format!("{}mod {};", vis, name)
    } else {
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

    let name = register_ident(&args[0]);
    let ty = compile_type_expr(&args[1]);
    let vals: Vec<String> = args[2..].iter().map(compile_expr).collect();

    format!("{}const {}: {} = {};", vis, name, ty, vals.join(" "))
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

    let name = register_ident(&args[i]);
    let ty = compile_type_expr(&args[i + 1]);
    let val = compile_expr(&args[i + 2]);

    let mut_str = if mutable { "mut " } else { "" };
    format!("{}static {}{}: {} = {};", vis, mut_str, name, ty, val)
}

/// Compile a type alias.
/// (type Meters i32)                          → type Meters = i32;
/// (type Foo (generic T) (Option T))           → type Foo<T> = Option<T>;
/// (type Foo (generic T) (where (T Clone)) (Option T)) → type Foo<T> = Option<T> where T: Clone;
fn compile_type_alias(args: &[Expr], vis: &str) -> String {
    if args.len() < 2 {
        warn("type alias missing name or definition");
        return "/* malformed type alias */".to_string();
    }

    let name = register_ident(&args[0]);
    let mut i = 1;

    // Parse optional generics
    let generics = if i < args.len() {
        try_parse_generics(&args[i])
            .inspect(|_| { i += 1; })
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Parse optional where clause
    let where_clause = if i < args.len() {
        try_parse_where(&args[i..])
            .inspect(|_| { i += 1; })
            .unwrap_or_default()
    } else {
        String::new()
    };

    if i >= args.len() {
        warn("type alias missing type definition");
        return format!("{}type {} = ();", vis, name);
    }

    let rhs = compile_type_expr(&args[i]);
    let gen_and_where = join_gen_and_where(&generics, &where_clause);

    format!("{}type {}{} = {};", vis, name, gen_and_where, rhs)
}

/// Compile dot access: (. expr field) or (. expr method args...)
fn compile_dot(args: &[Expr]) -> String {
    if args.is_empty() {
        return ".".to_string();
    }

    let obj = compile_expr(&args[0]);

    // Validate that the field/method name is a simple identifier
    if !matches!(&args[1], Expr::Symbol(_)) {
        warn(format!(
            "field or method name must be a simple identifier, found '{}'",
            compile_expr(&args[1])
        ));
    }

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
    // Binary operators are preserved verbatim by sanitize_ident, so the
    // raw symbol text matches what we emit.
    let raw_head = match &items[0] {
        Expr::Symbol(s) => s.as_str(),
        _ => "",
    };
    let is_op = is_binary_op(raw_head);

    let head = compile_expr(&items[0]);
    let args: Vec<String> = items[1..].iter().map(compile_expr).collect();

    if is_op && args.len() == 2 {
        return format!("({} {} {})", args[0], raw_head, args[1]);
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
        Expr::List(items, _) => {
            let params: Vec<String> = items
                .iter()
                .map(|p| match p {
                    Expr::List(parts, _) if parts.is_empty() => String::new(),
                    // Single-element list: just the name (e.g. (&self))
                    Expr::List(parts, _) if parts.len() == 1 => compile_expr(&parts[0]),
                    // Multi-element list: name is first, rest is type
                    Expr::List(parts, _) => {
                        let name = compile_param_name(&parts[0]);
                        let type_parts: Vec<String> =
                            parts[1..].iter().map(compile_type_expr).collect();
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
        Expr::List(items, _) => {
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
/// like generics, None otherwise.
/// Uses the `generic` command — no heuristic (avoids ambiguity with enum variants).
///
/// Syntax:
///   (generic T U V)               → <T, U, V>
///   (generic 'a)                  → <'a>
///   (generic (T Display) K)       → <T: Display, K>
///   (generic (T Display Clone))   → <T: Display + Clone>
fn try_parse_generics(expr: &Expr) -> Option<String> {
    match expr {
        Expr::List(items, _) if !items.is_empty() && matches!(&items[0], Expr::Symbol(s) if s == "generic") => {
            let params: Vec<String> = items[1..].iter().map(|e| match e {
                Expr::List(param_parts, _) if !param_parts.is_empty() => {
                    let name = compile_expr(&param_parts[0]);
                    let bounds: Vec<String> = param_parts[1..]
                        .iter()
                        .map(compile_expr)
                        .collect();
                    if bounds.is_empty() {
                        name
                    } else {
                        format!("{}: {}", name, bounds.join(" + "))
                    }
                }
                Expr::Symbol(s) => s.clone(),
                _ => compile_expr(e),
            }).collect();
            Some(params.join(", "))
        }
        _ => None,
    }
}

/// Try to parse supertrait bounds from the start of items.
/// Returns Some((supertraits_string, items_consumed)) or None.
///
/// An uppercase symbol or a list of bounds joined by `+` after the trait name.
/// (trait Foo Display ...)       → trait Foo: Display { ... }
/// (trait Foo (+ Display Clone) ...) → trait Foo: Display + Clone { ... }
fn try_parse_supertraits(items: &[Expr]) -> Option<(String, usize)> {
    if items.is_empty() {
        return None;
    }
    match &items[0] {
        Expr::Symbol(s) => {
            // Exclude keywords that start other sections
            if matches!(s.as_str(), "where" | "fn" | "type" | "generic") {
                return None;
            }
            Some((format!(": {}", s), 1))
        }
        Expr::List(inner, _) if !inner.is_empty() => {
            if let Expr::Symbol(first) = &inner[0] {
                if first == "where" || first == "fn" || first == "type" || first == "generic" {
                    return None;
                }
            } else {
                // Not a symbol — likely a body item list, not a supertrait
                return None;
            }
            let bounds = compile_expr(&items[0]);
            Some((format!(": {}", bounds), 1))
        }
        _ => None,
    }
}

/// Join generics and where clause with correct spacing.
/// Generics are wrapped in `<>` here so `try_parse_generics` can return raw params.
fn join_gen_and_where(generics: &str, where_clause: &str) -> String {
    let g = wrap_generics(generics);
    match (g.is_empty(), where_clause.is_empty()) {
        (true, true) => String::new(),
        (false, true) => g,
        (true, false) => format!(" {}", where_clause),
        (false, false) => format!("{} {}", g, where_clause),
    }
}

/// Try to parse a `where` clause from the current position.
/// Returns Some(where_clause_string) if found, None otherwise.
///
/// Syntax: (where (T Display Clone) ('a 'b))
/// Output: where T: Display + Clone, 'a: 'b
fn try_parse_where(items: &[Expr]) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    match &items[0] {
        Expr::List(inner, _) if !inner.is_empty() => {
            match &inner[0] {
                Expr::Symbol(s) if s == "where" => {
                    let clauses: Vec<String> = inner[1..]
                        .iter()
                        .map(compile_where_clause)
                        .collect();
                    if clauses.is_empty() {
                        warn("where clause with no bounds");
                        return Some(String::new());
                    }
                    Some(format!("where {}", clauses.join(", ")))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Compile a single where-clause element.
/// (T Display Clone)     → "T: Display + Clone"
/// ('a 'b)               → "'a: 'b"
fn compile_where_clause(expr: &Expr) -> String {
    match expr {
        Expr::List(items, _) if !items.is_empty() => {
            let target = compile_expr(&items[0]);
            let bounds: Vec<String> = items[1..]
                .iter()
                .map(compile_expr)
                .collect();
            if bounds.is_empty() {
                warn("where clause with no bounds");
                target
            } else {
                format!("{}: {}", target, bounds.join(" + "))
            }
        }
        _ => {
            warn("non-list where clause element");
            compile_expr(expr)
        }
    }
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

/// Compile turbofish: (:: expr Type...) → expr::<Type...>
fn compile_turbofish(args: &[Expr]) -> String {
    if args.is_empty() {
        return "".to_string();
    }
    let expr = compile_expr(&args[0]);
    let types: Vec<String> = args[1..].iter().map(compile_expr).collect();
    format!("{}::<{}>", expr, types.join(", "))
}

/// Compile break expression: (break) → break; , (break expr) → break expr;
fn compile_break(args: &[Expr]) -> String {
    if args.is_empty() {
        "break".to_string()
    } else {
        let val = compile_expr(&args[0]);
        format!("break {}", val)
    }
}

/// Compile continue expression: (continue) → continue;
/// Rust `continue` never takes a value — extra arguments are silently ignored
/// (they would be a compile error).
fn compile_continue(args: &[Expr]) -> String {
    if !args.is_empty() {
        warn("continue does not accept a value in Rust — arguments ignored");
    }
    "continue".to_string()
}

/// Compile return expression: (return) → return; , (return expr) → return expr;
fn compile_return(args: &[Expr]) -> String {
    if args.is_empty() {
        "return".to_string()
    } else {
        let val = compile_expr(&args[0]);
        format!("return {}", val)
    }
}

/// Compile type cast: (as expr Type) → expr as Type
fn compile_cast(args: &[Expr]) -> String {
    if args.len() < 2 {
        warn("as expression missing value or type");
        return "() as _".to_string();
    }
    let expr = compile_expr(&args[0]);
    let ty = compile_type_expr(&args[1]);
    format!("{} as {}", expr, ty)
}

/// Compile if-let expression: (if-let pattern expr then) or (if-let pattern expr then else)
fn compile_if_let(args: &[Expr]) -> String {
    if args.len() < 3 {
        warn("if-let expression missing pattern, value, or then-branch");
        return "if let _ = () {}".to_string();
    }
    let pattern = compile_pattern(&args[0]);
    let value = compile_expr(&args[1]);
    let then_body = compile_expr(&args[2]);
    let then_block = if then_body.starts_with('{') {
        then_body
    } else {
        format!("{{ {} }}", then_body)
    };
    if args.len() >= 4 {
        let else_body = compile_expr(&args[3]);
        let else_block = if else_body.starts_with('{') {
            else_body
        } else {
            format!("{{ {} }}", else_body)
        };
        format!("if let {} = {} {} else {}", pattern, value, then_block, else_block)
    } else {
        format!("if let {} = {} {}", pattern, value, then_block)
    }
}

/// Compile else-if expression: (else-if cond then) or (else-if cond then else)
///
/// Used as the else branch of `if`:
///   (if a b (else-if c d))           →  if a { b } else if c { d }
///   (if a b (else-if c d (else-if e f g)))  →  if a { b } else if c { d } else if e { f } else { g }
fn compile_else_if(args: &[Expr]) -> String {
    if args.is_empty() {
        warn("else-if expression with no condition or body");
        return "else if true {}".to_string();
    }

    let cond = compile_expr(&args[0]);
    let then_body = if args.len() >= 2 {
        compile_expr(&args[1])
    } else {
        warn("else-if expression with no then-branch");
        "()".to_string()
    };

    let then_block = if then_body.starts_with('{') {
        then_body
    } else {
        format!("{{ {} }}", then_body)
    };

    if args.len() >= 3 {
        let else_body = compile_expr(&args[2]);
        let is_else_chain = else_body.starts_with("else");
        let else_block = if else_body.starts_with('{') || is_else_chain {
            else_body
        } else {
            format!("{{ {} }}", else_body)
        };
        if is_else_chain {
            format!("else if {} {} {}", cond, then_block, else_block)
        } else {
            format!("else if {} {} else {}", cond, then_block, else_block)
        }
    } else {
        format!("else if {} {}", cond, then_block)
    }
}

/// Compile while-let expression: (while-let pattern expr body...) → while let pattern = expr { body... }
fn compile_while_let(args: &[Expr]) -> String {
    if args.len() < 2 {
        warn("while-let expression missing pattern, value, or body");
        return "while let _ = () {}".to_string();
    }
    let pattern = compile_pattern(&args[0]);
    let value = compile_expr(&args[1]);
    let body = compile_body(&args[2..]);
    if body.is_empty() {
        format!("while let {} = {} {{}}", pattern, value)
    } else {
        format!("while let {} = {} {{\n{}\n}}", pattern, value, indent(&body))
    }
}

/// Compile unsafe block: (unsafe body...) → unsafe { body... }
fn compile_unsafe(args: &[Expr]) -> String {
    let body = compile_body(args);
    if body.is_empty() {
        "unsafe {}".to_string()
    } else {
        format!("unsafe {{\n{}\n}}", indent(&body))
    }
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

/// Compile struct construction: (raw_new Type (field val) (field val)) → Type { field: val, field: val }
fn compile_raw_new(args: &[Expr]) -> String {
    if args.is_empty() {
        return "{}".to_string();
    }
    let type_name = compile_expr(&args[0]);
    let fields: Vec<String> = args[1..]
        .iter()
        .map(|f| match f {
            Expr::List(items, _) if items.len() == 2 => {
                let name = compile_expr(&items[0]);
                let val = compile_expr(&items[1]);
                format!("{}: {}", name, val)
            }
            _ => compile_expr(f),
        })
        .collect();
    format!("{} {{ {} }}", type_name, fields.join(", "))
}

/// Compile a type expression. Types can be single symbols or lists.
///
/// (Option i32)            → Option<i32>   (implicit: uppercase head → generic app)
/// (generic T U)           → T, U          (standalone type params)
/// (Option (generic T U))  → Option<T, U>  (explicit generic type application)
/// (& T)                   → & T           (reference, space-separated)
/// (dyn Trait)             → dyn Trait     (lowercase head → space-separated)
fn compile_type_expr(expr: &Expr) -> String {
    match expr {
        Expr::List(items, _) if !items.is_empty() => {
            // (generic T U) as a standalone type just emits the params
            if matches!(&items[0], Expr::Symbol(s) if s == "generic") {
                let params: Vec<_> = items[1..]
                    .iter()
                    .map(compile_type_expr)
                    .collect();
                return params.join(", ");
            }
            let first = compile_expr(&items[0]);
            if items.len() == 1 {
                return first;
            }
            // (Option (generic T U)) → Option<T, U>  (explicit generic)
            if let Expr::List(generic_items, _) = &items[1]
                && !generic_items.is_empty()
                && matches!(&generic_items[0], Expr::Symbol(s) if s == "generic")
            {
                let params: Vec<_> = generic_items[1..]
                    .iter()
                    .map(compile_type_expr)
                    .collect();
                return format!("{}<{}>", first, params.join(", "));
            }
            // Implicit generic application: uppercase head → Option<i32>
            if first.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                let params: Vec<_> = items[1..]
                    .iter()
                    .map(compile_type_expr)
                    .collect();
                return format!("{}<{}>", first, params.join(", "));
            }
            // Reference, pointer, etc.: (& T) → & T, (*const T) → *const T
            let rest: Vec<_> = items[1..].iter().map(compile_expr).collect();
            format!("{} {}", first, rest.join(" "))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    fn compile_first(src: &str) -> String {
        let exprs = parser::parse(src).unwrap();
        let (out, _warnings) = compile(&exprs);
        out.trim().to_string()
    }

    fn warnings(src: &str) -> Vec<String> {
        let exprs = parser::parse(src).unwrap();
        let (_, w) = compile(&exprs);
        w.into_iter().map(|w| w.message).collect()
    }

    // ——— fn definitions ———

    #[test]
    fn fn_with_no_body_is_semicolon() {
        let out = compile_first("(fn foo () i32)");
        assert_eq!(out, "fn foo() -> i32;");
    }

    #[test]
    fn fn_with_body_is_block() {
        let out = compile_first("(fn foo () i32 42)");
        assert!(out.contains("fn foo() -> i32 {\n    42\n}"));
    }

    #[test]
    fn fn_with_params() {
        let out = compile_first("(fn add ((x i32) (y i32)) i32 (+ x y))");
        assert!(out.contains("fn add(x: i32, y: i32) -> i32"));
    }

    #[test]
    fn fn_with_generics() {
        let out = compile_first("(fn first (generic T) ((list &[T])) (Option (generic &T)) (None))");
        assert!(out.contains("fn first<T>(list: &[T]) -> Option<&T>"));
    }

    #[test]
    fn fn_with_self_param() {
        let out = compile_first("(fn len ((&self)) usize 0)");
        assert!(out.contains("fn len(&self) -> usize"));
    }

    #[test]
    fn malformed_fn_warns() {
        let w = warnings("(fn)");
        assert!(w.iter().any(|m| m.contains("fn definition")));
    }

    // ——— identifier collision detection ———

    #[test]
    fn kebab_collision_between_fn_names() {
        let w = warnings("(fn foo-bar () i32 42) (fn foo__bar () i32 42)");
        assert!(w.iter().any(|m| m.contains("Identifier collision") && m.contains("foo-bar") && m.contains("foo__bar")));
    }

    #[test]
    fn kebab_let_collision() {
        let w = warnings("(fn test () i32 (let my-var 1) (let my__var 2))");
        assert!(w.iter().any(|m| m.contains("Identifier collision") && m.contains("my-var") && m.contains("my__var")));
    }

    #[test]
    fn enum_variant_collision() {
        let w = warnings("(enum E (foo-bar) (foo__bar))");
        assert!(w.iter().any(|m| m.contains("Identifier collision") && m.contains("foo-bar") && m.contains("foo__bar")));
    }

    #[test]
    fn struct_field_collision() {
        let w = warnings("(struct Point (foo-bar i32) (foo__bar i32))");
        assert!(w.iter().any(|m| m.contains("Identifier collision") && m.contains("foo-bar") && m.contains("foo__bar")));
    }

    #[test]
    fn no_collision_for_unique_idents() {
        let w = warnings("(fn foo-bar () i32 42) (fn baz-qux () i32 42)");
        assert!(w.iter().all(|m| !m.contains("Identifier collision")));
    }

    // ——— struct ———

    #[test]
    fn struct_with_fields() {
        let out = compile_first("(struct Point (x f64) (y f64))");
        assert!(out.contains("struct Point {\n    x: f64,\n    y: f64\n}"));
    }

    #[test]
    fn struct_with_generics() {
        let out = compile_first("(struct Pair (generic T) (first T) (second T))");
        assert!(out.contains("struct Pair<T>"));
    }

    #[test]
    fn struct_field_with_visibility() {
        let out = compile_first("(struct Config (pub host String) (port u16))");
        assert!(out.contains("pub host: String"));
        assert!(out.contains("port: u16"));
    }

    #[test]
    fn malformed_struct_warns() {
        let w = warnings("(struct)");
        assert!(w.iter().any(|m| m.contains("struct")));
    }

    #[test]
    fn tuple_struct() {
        let out = compile_first("(struct Point f64 f64)");
        assert!(out.contains("struct Point(f64, f64);"));
    }

    #[test]
    fn tuple_struct_with_generics() {
        let out = compile_first("(struct Wrapper (generic T) T)");
        assert!(out.contains("struct Wrapper<T>(T);"));
    }

    #[test]
    fn unit_struct() {
        let out = compile_first("(struct Unit)");
        assert_eq!(out, "struct Unit;");
    }

    #[test]
    fn unit_struct_with_visibility() {
        let out = compile_first("(pub struct Unit)");
        assert!(out.contains("pub struct Unit;"));
    }

    // ——— enum ———

    #[test]
    fn enum_with_variants() {
        let out = compile_first("(enum Option (generic T) (Some T) None)");
        assert!(out.contains("enum Option<T> {\n    Some(T),\n    None\n}"));
    }

    #[test]
    fn malformed_enum_warns() {
        let w = warnings("(enum)");
        assert!(w.iter().any(|m| m.contains("enum")));
    }

    // ——— trait ———

    #[test]
    fn trait_with_method_sigs() {
        let out = compile_first("(trait Greet ((fn greet ((&self)) String)))");
        assert!(out.contains("trait Greet {\n    fn greet(&self) -> String;\n}"));
    }

    #[test]
    fn malformed_trait_warns() {
        let w = warnings("(trait)");
        assert!(w.iter().any(|m| m.contains("trait")));
    }

    // ——— impl ———

    #[test]
    fn impl_with_methods() {
        let out = compile_first("(impl Point ((fn new ((x f64) (y f64)) Point (raw_new Point (x x) (y y)))))");
        assert!(out.contains("impl Point {"));
        assert!(out.contains("fn new(x: f64, y: f64) -> Point"));
    }

    #[test]
    fn impl_trait_for_type() {
        let out =
            compile_first("(impl Display for Point ((fn fmt ((&self) (f &mut Formatter)) (Result (generic () Error)))))");
        assert!(out.contains("impl Display for Point {"));
        assert!(out.contains("fn fmt(&self, f: &mut Formatter) -> Result<(), Error>;"));
    }

    #[test]
    fn malformed_impl_warns() {
        let w = warnings("(impl)");
        assert!(w.iter().any(|m| m.contains("impl")));
    }

    // ——— let ———

    #[test]
    fn let_is_compiled() {
        // With a trailing expression so the let gets a semicolon
        let out = compile_first("(fn f () i32 (let x 42) x)");
        assert!(out.contains("let x = 42;"));
    }

    #[test]
    fn let_typed() {
        let out = compile_first("(fn f () i32 (let x i32 42) x)");
        assert!(out.contains("let x: i32 = 42;"));
    }

    #[test]
    fn let_mut() {
        let out = compile_first("(fn f () i32 (let mut x 0) x)");
        assert!(out.contains("let mut x = 0;"));
    }

    #[test]
    fn let_with_expression_value() {
        let out = compile_first("(fn f () i32 (let x (+ 1 2)) x)");
        assert!(out.contains("let x = (1 + 2);"));
    }

    #[test]
    fn malformed_let_warns() {
        let w = warnings("(fn f () () (let))");
        assert!(w.iter().any(|m| m.contains("let")));
    }

    // ——— if ———

    #[test]
    fn if_with_else() {
        let out =
            compile_first("(fn f () () (if (> x 0) (println! \"pos\") (println! \"neg\")))");
        assert!(out.contains("if (x > 0) { println!(\"pos\") } else { println!(\"neg\") }"));
    }

    #[test]
    fn if_without_else() {
        let out = compile_first("(fn f () () (if (> x 0) (println! \"pos\")))");
        assert!(out.contains("if (x > 0)"));
        assert!(!out.contains("else"));
    }

    #[test]
    fn malformed_if_warns() {
        let w = warnings("(fn f () () (if))");
        assert!(w.iter().any(|m| m.contains("if")));
    }

    // ——— match ———

    #[test]
    fn match_with_arms_compiles() {
        let out =
            compile_first("(fn f ((opt (Option i32))) () (match opt ((Some x) (print x)) (None ())))");
        assert!(out.contains("match opt {"));
        assert!(out.contains("Some(x) => { print(x) }"));
    }

    #[test]
    fn match_underscore_pattern() {
        let out = compile_first("(fn f ((x i32)) () (match x (_ ()) (0 ())))");
        assert!(out.contains("_ => "));
    }

    // ——— loop / while / for ———

    #[test]
    fn loop_expression() {
        let out = compile_first("(fn f () () (loop (println! \"tick\") (break)))");
        assert!(out.contains("loop {"));
        assert!(out.contains("println!(\"tick\");"));
        assert!(out.contains("break"));
        assert!(!out.contains("break()"));
    }

    #[test]
    fn while_expression() {
        let out = compile_first("(fn f () () (while (> x 0) (-= x 1)))");
        assert!(out.contains("while (x > 0) {"));
        assert!(out.contains("(x -= 1)"));
    }

    #[test]
    fn while_with_no_body_warns() {
        let w = warnings("(fn f () () (while (> x 0)))");
        assert!(w.iter().any(|m| m.contains("no body")));
    }

    #[test]
    fn for_expression() {
        let out =
            compile_first("(fn f ((v &[i32])) () (for x in v (println! \"{}\" x)))");
        assert!(out.contains("for x in v {"));
    }

    #[test]
    fn for_with_tuple_destructure() {
        let out = compile_first(
            "(fn f ((iter (SomeIter))) () (for ((i x)) in iter (println! \"{}\" i)))",
        );
        assert!(out.contains("for (i, x) in iter {"));
    }

    #[test]
    fn empty_loop_is_valid() {
        let out = compile_first("(fn f () () (loop))");
        assert!(out.contains("loop {}"));
    }

    // ——— closures ———

    #[test]
    fn lambda_untyped() {
        let out = compile_first("(fn f () i32 (let add (lambda (x y) (+ x y))) (add 1 2))");
        assert!(out.contains("|x, y| {\n        (x + y)\n    }"));
    }

    #[test]
    fn lambda_typed() {
        let out = compile_first(
            "(fn f () i32 (let mul (lambda ((x i32) (y i32)) i32 (* x y))) (mul 3 4))",
        );
        assert!(out.contains("|x: i32, y: i32| -> i32 {"));
    }

    #[test]
    fn lambda_move() {
        let out = compile_first("(fn f () () (let s \"hi\") (let g (lambda move () s)) (print (g)))");
        assert!(out.contains("move |"));
    }

    #[test]
    fn lambda_no_params() {
        let out = compile_first("(fn f () i32 (let c (lambda () 42)) (c))");
        assert!(out.contains("|| {"));
    }

    // ——— visibility ———

    #[test]
    fn pub_fn() {
        let out = compile_first("(pub fn foo () i32 42)");
        assert!(out.starts_with("pub fn foo"));
    }

    #[test]
    fn pub_crate_fn() {
        let out = compile_first("(pub (crate) fn foo () i32 42)");
        assert!(out.starts_with("pub(crate) fn foo"));
    }

    #[test]
    fn pub_super_fn() {
        let out = compile_first("(pub (super) fn foo () i32 42)");
        assert!(out.starts_with("pub(super) fn foo"));
    }

    #[test]
    fn pub_struct() {
        let out = compile_first("(pub struct Point (x i32))");
        assert!(out.starts_with("pub struct Point"));
    }

    #[test]
    fn pub_enum() {
        let out = compile_first("(pub enum Status Ok Err)");
        assert!(out.starts_with("pub enum Status"));
    }

    #[test]
    fn pub_trait() {
        let out = compile_first("(pub trait Foo (fn bar () ()))");
        assert!(out.starts_with("pub trait Foo"));
    }

    // ——— mod / use ———

    #[test]
    fn mod_declaration() {
        let out = compile_first("(mod mymod)");
        assert_eq!(out, "mod mymod;");
    }

    #[test]
    fn mod_with_body() {
        let out = compile_first("(mod mymod ((fn helper () i32 1)))");
        assert!(out.contains("mod mymod {"));
        assert!(out.contains("fn helper() -> i32 {"));
    }

    #[test]
    fn pub_mod() {
        let out = compile_first("(pub mod mymod ((fn f () () ())))");
        assert!(out.starts_with("pub mod mymod {"));
    }

    #[test]
    fn use_single_path() {
        let out = compile_first("(use std::collections::HashMap)");
        assert_eq!(out, "use std::collections::HashMap;");
    }

    #[test]
    fn use_with_as() {
        let out = compile_first("(use std::fmt::Display as Fmt)");
        assert_eq!(out, "use std::fmt::Display as Fmt;");
    }

    #[test]
    fn use_with_braces() {
        let out = compile_first("(use std::io::{self,Write})");
        assert_eq!(out, "use std::io::{self,Write};");
    }

    #[test]
    fn pub_use() {
        let out = compile_first("(pub use std::collections::HashMap)");
        assert_eq!(out, "pub use std::collections::HashMap;");
    }

    #[test]
    fn malformed_mod_warns() {
        let w = warnings("(mod)");
        assert!(w.iter().any(|m| m.contains("mod")));
    }

    #[test]
    fn malformed_use_warns() {
        let w = warnings("(use)");
        assert!(w.iter().any(|m| m.contains("use")));
    }

    // ——— const / static ———

    #[test]
    fn const_decl() {
        let out = compile_first("(const MAX usize 1024)");
        assert_eq!(out, "const MAX: usize = 1024;");
    }

    #[test]
    fn static_decl() {
        let out = compile_first("(static COUNTER i32 0)");
        assert_eq!(out, "static COUNTER: i32 = 0;");
    }

    #[test]
    fn static_mut() {
        let out = compile_first("(static mut STATE u64 42)");
        assert_eq!(out, "static mut STATE: u64 = 42;");
    }

    #[test]
    fn pub_const() {
        let out = compile_first("(pub const GREETING &str \"hello\")");
        assert_eq!(out, "pub const GREETING: &str = \"hello\";");
    }

    #[test]
    fn malformed_const_warns() {
        let w = warnings("(const)");
        assert!(w.iter().any(|m| m.contains("const")));
    }

    #[test]
    fn malformed_static_warns() {
        let w = warnings("(static)");
        assert!(w.iter().any(|m| m.contains("static")));
    }

    // ——— expressions ———

    #[test]
    fn do_block() {
        let out = compile_first("(fn f () () (do (println! \"a\") (println! \"b\")))");
        assert!(out.contains("println!(\"a\");"));
        assert!(out.contains("println!(\"b\")"));
    }

    #[test]
    fn dot_field_access() {
        let out = compile_first("(fn f () f64 (. p x))");
        assert!(out.contains("p.x"));
    }

    #[test]
    fn dot_method_call() {
        let out = compile_first("(fn f () f64 (. p distance (& other)))");
        assert!(out.contains("p.distance(&(other)"));
    }

    #[test]
    fn index_access() {
        let out = compile_first("(fn f () i32 ([] arr 0))");
        assert!(out.contains("arr[0]"));
    }

    #[test]
    fn struct_construction() {
        let out = compile_first("(fn f () Point (raw_new Point (x 1.0) (y 2.0)))");
        assert!(out.contains("Point { x: 1.0, y: 2.0 }"));
    }

    #[test]
    fn binary_operator_infix() {
        let out = compile_first("(fn f () i32 (+ a b))");
        assert!(out.contains("(a + b)"));
    }

    #[test]
    fn comparison_operator_infix() {
        let out = compile_first("(fn f () bool (> x 0))");
        assert!(out.contains("(x > 0)"));
    }

    #[test]
    fn macro_call() {
        let out = compile_first("(fn f () () (println! \"{}\" x))");
        assert!(out.contains("println!(\"{}\", x)"));
    }

    #[test]
    fn function_call() {
        let out = compile_first("(fn f () i32 (foo a b))");
        assert!(out.contains("foo(a, b)"));
    }

    #[test]
    fn string_literal() {
        let out = compile_first("(fn f () () (println! \"hello world\"))");
        assert!(out.contains("\"hello world\""));
    }

    // ——— inline rust ———

    #[test]
    fn rust_block_string() {
        let out = compile_first("(fn f () i32 (rust \"let x: i32 = 42; x\"))");
        assert!(out.contains("let x: i32 = 42; x"));
    }

    #[test]
    fn rust_block_strips_semicolons() {
        let out =
            compile_first("(fn f () () (rust \"let x = 5;\") (rust \"let y = 6;\"))");
        assert!(!out.contains(";;"));
    }

    // ——— type expressions ———

    #[test]
    fn reference_type_param() {
        let out = compile_first("(fn f ((x &i32)) () ())");
        assert!(out.contains("x: &i32"));
    }

    #[test]
    fn generic_type_param() {
        let out = compile_first("(fn f ((x (Option (generic i32)))) () ())");
        assert!(out.contains("x: Option<i32>"));
    }

    #[test]
    fn reference_lifetime_param() {
        let out = compile_first("(fn f ((s &'a str)) () ())");
        assert!(out.contains("s: &'a str"));
    }

    #[test]
    fn multi_generics_on_fn() {
        let out = compile_first("(fn foo (generic K V) ((k K) (v V)) () ())");
        assert!(out.contains("fn foo<K, V>(k: K, v: V) -> ()"));
    }

    // ——— turbofish, control flow, and other syntax ———

    #[test]
    fn break_expression() {
        let out = compile_first("(fn f () () (loop (println! \"tick\") (break)))");
        assert!(out.contains("break"));
        assert!(!out.contains("break()"));
    }

    #[test]
    fn break_with_value() {
        let out = compile_first("(fn f () i32 (loop (break 42)))");
        assert!(out.contains("break 42"));
    }

    #[test]
    fn continue_expression() {
        let out = compile_first("(fn f () () (for x in 0..10 (if (== x 0) (continue)) (println! \"{}\" x)))");
        assert!(out.contains("continue"));
        assert!(!out.contains("continue()"));
    }

    #[test]
    fn return_expression() {
        let out = compile_first("(fn f () i32 (return 42))");
        assert!(out.contains("return 42"));
    }

    #[test]
    fn return_unit() {
        let out = compile_first("(fn f () () (return))");
        assert!(out.contains("return"));
        assert!(!out.contains("return()"));
    }

    #[test]
    fn turbofish_collect() {
        let out = compile_first("(fn f () Vec<i32> ((:: (. (0..1) collect) Vec<i32>)))");
        assert!(out.contains("collect::<Vec<i32>>"));
    }

    #[test]
    fn turbofish_standalone() {
        let out = compile_first("(fn f () () (let x ((:: Vec::new i32))))");
        assert!(out.contains("Vec::new::<i32>"));
    }

    #[test]
    fn as_cast() {
        let out = compile_first("(fn f ((x f64)) i32 (as x i32))");
        assert!(out.contains("x as i32"));
    }

    #[test]
    fn if_let_simple() {
        let out = compile_first("(fn f ((x (Option i32))) () (if-let (Some v) x (println! \"{}\" v)))");
        assert!(out.contains("if let Some(v) = x"));
    }

    #[test]
    fn if_let_with_else() {
        let out = compile_first("(fn f ((x (Option i32))) () (if-let (Some v) x (println! \"{}\" v) (println! \"none\")))");
        assert!(out.contains("else"));
    }

    #[test]
    fn while_let_simple() {
        let out = compile_first("(fn f ((iter &mut (Iter i32))) () (while-let (Some v) ((. iter next)) (println! \"{}\" v)))");
        assert!(out.contains("while let Some(v) = iter.next()"));
    }

    #[test]
    fn unsafe_block() {
        let out = compile_first("(fn f () () (unsafe (rust \"*ptr\")))");
        assert!(out.contains("unsafe {"));
    }

    #[test]
    fn lifetime_on_fn_def() {
        let out = compile_first("(fn foo (generic 'a) ((x &'a str)) (&'a str) x)");
        assert!(out.contains("fn foo<'a>(x: &'a str) -> &'a str"));
    }

    #[test]
    fn static_lifetime_on_fn() {
        let out = compile_first("(fn foo (generic 'static) ((x &'static str)) (&'static str) x)");
        assert!(out.contains("fn foo<'static>(x: &'static str) -> &'static str"));
    }

    // ——— edge cases ———

    #[test]
    fn clean_code_has_no_warnings() {
        let w = warnings("(fn main () () ())");
        assert!(w.is_empty());
    }

    #[test]
    fn multiple_top_level_items() {
        let out = compile_first("(struct A (x i32))\n(struct B (y f64))");
        assert!(out.contains("struct A {"));
        assert!(out.contains("struct B {"));
    }

    #[test]
    fn multiple_warnings_collected() {
        let w = warnings("(struct)\n(enum)");
        assert!(w.len() >= 2);
    }

    #[test]
    fn empty_input() {
        let (out, _) = compile(&[]);
        assert!(out.is_empty());
    }

    // ——— where clauses ———

    #[test]
    fn fn_with_where_clause() {
        let out = compile_first("(fn foo (generic T) (where (T Display)) ((x T)) String (to_string x))");
        assert!(out.contains("where T: Display"));
    }

    #[test]
    fn fn_with_where_and_lifetime() {
        let out = compile_first("(fn longest (generic 'a) (where ('a 'b)) ((x &'a str)) (&'a str) x)");
        assert!(out.contains("where 'a: 'b"));
    }

    #[test]
    fn struct_with_where_clause() {
        let out = compile_first("(struct Pair (generic T) (where (T Clone)) (first T) (second T))");
        assert!(out.contains("where T: Clone"));
    }

    #[test]
    fn enum_with_where_clause() {
        let out = compile_first("(enum Foo (generic T) (where (T Display)) (Bar T) Baz)");
        assert!(out.contains("where T: Display"));
    }

    #[test]
    fn where_clause_no_fn_body() {
        let out = compile_first("(fn foo (generic T) (where (T Display)) ((x T)) String)");
        assert!(out.contains("where T: Display"));
        assert!(out.ends_with(';'));
    }

    // ——— inline generics bounds ———

    #[test]
    fn generics_with_inline_bounds() {
        let out = compile_first("(fn foo (generic (T Display)) ((x T)) () ())");
        assert!(out.contains("fn foo<T: Display>"));
    }

    #[test]
    fn generics_with_multi_bounds() {
        let out = compile_first("(fn foo (generic (T Display Clone)) () () ())");
        assert!(out.contains("fn foo<T: Display + Clone>"));
    }

    #[test]
    fn generics_mixed_inline_and_bare() {
        let out = compile_first("(fn foo (generic (K Display) V) () () ())");
        assert!(out.contains("fn foo<K: Display, V>"));
    }

    // ——— derive attributes ———

    #[test]
    fn struct_with_derive() {
        let out = compile_first("(struct (derive Debug Clone) Point (x i32) (y i32))");
        assert!(out.contains("#[derive(Debug, Clone)]"));
        assert!(out.contains("struct Point {"));
    }

    #[test]
    fn enum_with_derive() {
        let out = compile_first("(enum (derive Debug PartialEq) Status Ok Err)");
        assert!(out.contains("#[derive(Debug, PartialEq)]"));
    }

    #[test]
    fn fn_with_derive() {
        let out = compile_first("(fn (derive must_use) important () i32 42)");
        assert!(out.contains("#[derive(must_use)]"));
    }

    #[test]
    fn empty_derive_warns() {
        let w = warnings("(struct (derive) Point)");
        assert!(w.iter().any(|m| m.contains("derive")));
    }

    // ——— supertraits ———

    #[test]
    fn trait_with_supertrait() {
        let out = compile_first("(trait Foo Display ((fn fmt () ())))");
        assert!(out.contains("trait Foo: Display {"));
    }

    #[test]
    fn trait_with_multiple_supertraits() {
        let out = compile_first("(trait Foo (+ Display Clone) ((fn bar () ())))");
        assert!(out.contains("trait Foo: (Display + Clone) {"));
    }

    #[test]
    fn trait_with_supertrait_and_where() {
        let out = compile_first("(trait Foo Display (where (Self Sized)) ((fn bar () ())))");
        assert!(out.contains("trait Foo: Display"));
        assert!(out.contains("where Self: Sized"));
    }

    // ——— associated types ———

    #[test]
    fn trait_with_associated_type() {
        let out = compile_first("(trait Iterator ((type Item) (fn next () ())))");
        assert!(out.contains("type Item;"));
    }

    #[test]
    fn trait_with_associated_type_bounds() {
        let out = compile_first("(trait Foo ((type Item Display Clone) (fn get () ())))");
        assert!(out.contains("type Item: Display + Clone;"));
    }

    // ——— generic impl blocks ———

    #[test]
    fn impl_with_generics() {
        let out = compile_first("(impl (generic T) (Vec (generic T)) ((fn push ((&mut self) (value T)) () ())))");
        assert!(out.contains("impl<T> Vec<T> {"));
    }

    #[test]
    fn impl_trait_with_generics_and_where() {
        let out = compile_first("(impl (generic T) (where (T Display)) Display for (MyType (generic T)) ((fn fmt () ())))");
        assert!(out.contains("impl<T> Display for MyType<T> where T: Display {"));
    }

    // ——— type aliases ———

    #[test]
    fn type_alias_simple() {
        let out = compile_first("(type Meters i32)");
        assert_eq!(out, "type Meters = i32;");
    }

    #[test]
    fn type_alias_with_generics() {
        let out = compile_first("(type Foo (generic T) (Option (generic T)))");
        assert_eq!(out, "type Foo<T> = Option<T>;");
    }

    #[test]
    fn type_alias_with_where() {
        let out = compile_first("(type Foo (generic T) (where (T Clone)) (Option (generic T)))");
        assert!(out.contains("type Foo<T> where T: Clone = Option<T>;"));
    }

    #[test]
    fn pub_type_alias() {
        let out = compile_first("(pub type Meters i32)");
        assert_eq!(out, "pub type Meters = i32;");
    }

    #[test]
    fn empty_type_alias_warns() {
        let w = warnings("(type)");
        assert!(w.iter().any(|m| m.contains("type")));
    }

    // ——— combined features ———

    #[test]
    fn fn_with_generics_bounds_and_where() {
        let out = compile_first("(fn foo (generic (T Display)) (where (T Clone)) ((x T)) String (to_string x))");
        assert!(out.contains("fn foo<T: Display> where T: Clone"));
    }

    #[test]
    fn struct_with_derive_and_where() {
        let out = compile_first("(struct (derive Debug) Wrapper (generic T) (where (T Clone)) T)");
        assert!(out.contains("#[derive(Debug)]"));
        assert!(out.contains("struct Wrapper<T>(T) where T: Clone;"));
    }

    // ——— else-if ———

    #[test]
    fn else_if_basic() {
        let out = compile_first("(fn f () () (if (> x 10) (println! \"big\") (else-if (> x 5) (println! \"medium\"))))");
        assert!(out.contains("else if (x > 5) { println!(\"medium\") }"));
    }

    #[test]
    fn else_if_with_else() {
        let out = compile_first("(fn f () () (if (> x 10) (println! \"big\") (else-if (> x 5) (println! \"medium\") (println! \"small\"))))");
        assert!(out.contains("else if (x > 5) { println!(\"medium\") } else { println!(\"small\") }"));
    }

    #[test]
    fn else_if_chained() {
        let out = compile_first("(fn f () () (if (> x 10) (a) (else-if (> x 5) (b) (else-if (> x 0) (c) (d)))))");
        // Should have chained else-if without doubled "else"
        assert!(!out.contains("else else"));
        assert!(out.contains("else if (x > 5)"));
        assert!(out.contains("else if (x > 0)"));
    }

    #[test]
    fn else_if_missing_body_warns() {
        let w = warnings("(fn f () () (if true (x) (else-if)))");
        assert!(w.iter().any(|m| m.contains("else-if")));
    }

    // ——— match guards ———

    #[test]
    fn match_with_guard() {
        let out = compile_first("(fn f ((x (Option i32))) () (match x ((Some v) if (> v 0) (println! \"pos\")) (_ ())))");
        assert!(out.contains("Some(v) if (v > 0) =>"));
    }

    #[test]
    fn match_guard_simple_pattern() {
        let out = compile_first("(fn f ((x i32)) () (match x (_ if (== x 0) (println! \"zero\")) (_ ())))");
        assert!(out.contains("_ if (x == 0) =>"));
    }

    // ——— implicit type generics ———

    #[test]
    fn type_implicit_generic_option() {
        let out = compile_first("(fn f ((x (Option i32))) () ())");
        assert!(out.contains("x: Option<i32>"));
    }

    #[test]
    fn type_implicit_generic_result() {
        let out = compile_first("(fn f () (Result i32 String) (Ok 42))");
        assert!(out.contains("Result<i32, String>"));
    }

    #[test]
    fn type_reference_still_space_separated() {
        let out = compile_first("(fn f ((x &i32)) () ())");
        assert!(out.contains("x: &i32"));
    }

    // ——— continue without value ———

    #[test]
    fn continue_with_value_warns() {
        let w = warnings("(fn f () () (continue 42))");
        assert!(w.iter().any(|m| m.contains("continue")));
    }

    #[test]
    fn continue_without_value() {
        let out = compile_first("(fn f () () (loop (continue)))");
        assert!(out.contains("continue"));
        assert!(!out.contains("continue()"));
    }

    // ——— for loop validation ———

    #[test]
    fn for_without_in_warns() {
        let w = warnings("(fn f () () (for x y (println! \"{}\" x)))");
        assert!(w.iter().any(|m| m.contains("missing 'in'")));
    }

    // ——— tuple destructure in for ———

    #[test]
    fn for_tuple_destructure_double_parens() {
        let out = compile_first("(fn f ((iter (SomeIter))) () (for ((i x)) in iter (println! \"{}\" i)))");
        assert!(out.contains("for (i, x) in iter {"));
    }

    // ——— lowercase enum variant in patterns ———

    #[test]
    fn match_lowercase_variant_pattern() {
        let out = compile_first("(fn f ((r (Result i32 String))) () (match r ((ok val) (print val)) ((err e) (print e))))");
        assert!(out.contains("ok(val)"));
        assert!(out.contains("err(e)"));
    }

    #[test]
    fn for_lowercase_variant_pattern() {
        let out = compile_first("(fn f ((iter (SomeIter))) () (for (some x) in iter (println! \"{}\" x)))");
        assert!(out.contains("for some(x) in iter {"));
    }

    // ——— raw_new ———

    #[test]
    fn raw_new_basic() {
        let out = compile_first("(fn f () Point (raw_new Point (x 1.0) (y 2.0)))");
        assert!(out.contains("Point { x: 1.0, y: 2.0 }"));
    }

    // ——— `-` as symbol (not number) ———

    #[test]
    fn minus_is_symbol_in_binary_op() {
        let out = compile_first("(fn f () i32 (- a b))");
        assert!(out.contains("(a - b)"));
    }

    #[test]
    fn minus_unary() {
        let out = compile_first("(fn f ((x i32)) i32 (- x))");
        assert!(out.contains("-(x)"));
    }
}
