# rlisp

Rust semantics with LISP syntax. A transparent s-expression frontend that compiles directly to Rust — no runtime, no GC, just `(s-expr → .rs → binary)`.

```lisp
(struct Point
  (x f64)
  (y f64))

(impl Point
  (fn distance ((&self) (other &Point)) f64
    (let dx (- (. self x) (. other x)))
    (let dy (- (. self y) (. other y)))
    ((. dx powf 2.0) + (. dy powf 2.0)) sqrt))

(fn main () ()
  (let p1 (new Point (x 0.0) (y 0.0)))
  (let p2 (new Point (x 3.0) (y 4.0)))
  (println! "Distance: {}" (. p1 distance (& p2))))
```

Everything Rust has — ownership, borrowing, lifetimes, generics, traits, pattern matching — expressed as s-expressions. No semantic gap. `rustc` does type checking, borrow checking, and optimization. rlisp just handles the syntax.

## Install

```bash
git clone https://github.com/ThatXliner/rlisp.git
cd rlisp
cargo install --path .
```

## Usage

```bash
rlisp compile file.lisp   # transpile to file.rs
rlisp build file.lisp     # transpile and compile with rustc
rlisp run file.lisp       # transpile, compile, and run
```

## Syntax map

| LISP | Rust |
|------|------|
| `(fn add ((x i32) (y i32)) i32 (+ x y))` | `fn add(x: i32, y: i32) -> i32 { (x + y) }` |
| `(let x i32 42)` | `let x: i32 = 42;` |
| `(struct Point (x f64) (y f64))` | `struct Point { x: f64, y: f64 }` |
| `(enum Option (T) (Some T) None)` | `enum Option<T> { Some(T), None }` |
| `(match val ((Some x) (handle x)) (None ()))` | `match val { Some(x) => { handle(x) }, None => { } }` |
| `(if (> x 0) (println! "yes") (println! "no"))` | `if (x > 0) { println!("yes") } else { println!("no") }` |
| `(impl Point (fn new (...) ...))` | `impl Point { fn new(...) ... }` |
| `(trait Display (fn fmt (...) Result))` | `trait Display { fn fmt(...) -> Result; }` |
| `(new Point (x 1.0) (y 2.0))` | `Point { x: 1.0, y: 2.0 }` |
| `(. obj field)` | `obj.field` |
| `(. obj method arg)` | `obj.method(arg)` |
| `([] arr 0)` | `arr[0]` |
| `(foo! args)` | `foo!(args)` |
| `(println! "{}" x)` | `println!("{}", x)` |

Binary operators (`+`, `-`, `*`, `/`, `==`, `!=`, `<`, `>`, `&&`, etc.) emit infix: `(+ a b)` → `(a + b)`.

## Why

Mostly for fun — an exploration of what Rust looks like when you strip away the syntax and keep the semantics. But there are practical angles too:

- **Macros become trivial.** In LISP, a macro is just a function that takes s-expressions and returns s-expressions, executed at compile time. No token streaming, no `proc_macro` ceremony. This is the killer feature LISP brings to Rust.
- **Structural editing.** s-expressions are trivial to manipulate with editor tooling — slurp, barf, transpose, wrap. Every operation is balanced by construction.
- **Homogeneous syntax.** No distinction between expressions, statements, types, and patterns. Everything is an s-expression. `match` arms and function signatures use the same syntax you already know.

## License

MIT
