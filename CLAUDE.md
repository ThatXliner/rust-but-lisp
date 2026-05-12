# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

rlisp — Rust semantics in LISP syntax. A transpiler: s-expressions → `.rs` → binary. Only handles syntax; `rustc` still does type checking, borrow checking, and optimization.

## Commands

```bash
cargo build              # compile the transpiler
cargo test               # run all tests (~70 unit tests across parser, codegen, macros)
cargo install --path .   # install the `rlisp` binary

# After installing:
rlisp compile file.lisp  # transpile to file.rs
rlisp build file.lisp    # transpile + compile with rustc
rlisp run file.lisp      # transpile + compile + run
```

## Architecture

Four-module pipeline in `src/`:

```
source text → parser.rs → ast.rs (Vec<Expr>) → macro.rs → codegen.rs → .rs output
```

- **`ast.rs`** — Single S-expression type: `Expr::Symbol | Number | StringLit | List(Vec<Expr>, Option<Span>)`. `Span` stores byte offsets for diagnostics. No type info, no resolution — purely syntax.
- **`parser.rs`** — Hand-written tokenizer + recursive descent parser. Tokens are LParen/RParen/Symbol/StringLit/Number. No external parser library. `;` starts line comments.
- **`macro.rs`** — Expands `defmacro` forms before codegen. Macros use `quasiquote`/`unquote`/`unquote-splicing`. Supports `&rest` for variadic args. Runs a fixed-point expansion loop (re-expands until no macros apply). `defmacro` forms are stripped from the output.
- **`codegen.rs`** — Walks the AST and emits Rust source strings. Uses `thread_local!` state for warnings and span tracking (`SpanGuard` RAII guard). Returns `(String, Vec<Warning>)`.

## Key design details

- **Kebab → snake**: Hyphens in identifiers become `__` (double underscore). The codegen maintains a global `SEEN_IDENTS` map and warns on collisions (e.g., `foo-bar` and `foo__bar` both → `foo__bar`).
- **Binary operators**: `+`, `-`, `*`, `/`, `==`, `!=`, `<`, `>`, `<=`, `>=`, `&&`, `||`, `&`, `|`, `^`, `<<`, `>>`, `=`, `+=`, `-=`, `*=`, `/=` emit infix when called with exactly 2 args.
- **`compile_body`**: Adds semicolons after all but the last expression in a block. The last expression is the implicit return.
- **Visibility**: Two forms — bare `pub` symbol or `(pub crate)` / `(pub super)` lists. Parsed by `try_parse_visibility` which peels off the visibility prefix and returns the rest.
- **Generics**: Use the `generic` command. E.g., `(generic T)`, `(generic K V)`. Inline bounds: `(generic (T Display))` → `<T: Display>`. Where clauses: `(where (T Clone) ('a 'b))` → `where T: Clone, 'a: 'b`. Supertraits: `(trait Foo Display ...)` → `trait Foo: Display`. Associated types: `(type Item)` in traits. Derive: `(struct (derive Debug Clone) Point ...)`. Type aliases: `(type Foo (generic T) (Option T))` → `type Foo<T> = Option<T>;`.
- **`-` is parsed as a Symbol token** (since `is_number` requires at least one digit). Binary operators including `-` are preserved verbatim by `sanitize_ident` — they are never mangled through kebab→snake conversion.
- **Type expressions**: Lists with an uppercase head implicitly apply generics: `(Option i32)` → `Option<i32>`. Lists with a lowercase/reference head are space-separated: `(& T)` → `& T`, `(dyn Trait)` → `dyn Trait`. The explicit `generic` command still works: `(Option (generic T))` → `Option<T>`.
- **Struct construction**: `(raw_new Type (field val)...)` emits struct literal syntax `Type { field: val, ... }`. For `Type::new()` calls use regular function syntax `(Type::new)`.
- **`else-if`**: Chains conditions in if-expressions: `(if a b (else-if c d e))` → `if a { b } else if c { d } else { e }`.
- **Match guards**: `((pattern) if condition body...)` in match arms → `pattern if condition => { body }`.
- **For-loop patterns**: Use match-pattern syntax. Tuple destructure uses double parens: `(for ((i x)) in iter ...)` → `for (i, x) in iter`. Enum variant patterns use single parens: `(for (Some x) in iter ...)` → `for Some(x) in iter`.
- **`continue`**: Ignores arguments with a warning (Rust's `continue` never takes a value).
- **Edition 2024**: The Cargo.toml uses `edition = "2024"`, which requires a recent Rust toolchain.
- **Only dependency**: `ariadne` (v0.5) for pretty error diagnostics with source spans.
