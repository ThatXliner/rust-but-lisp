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
| `(loop (println! "tick"))` | `loop { println!("tick") }` |
| `(while (> x 0) (-= x 1))` | `while x > 0 { x -= 1 }` |
| `(for x in 0..10 (println! "{}" x))` | `for x in 0..10 { println!("{}", x) }` |
| `(lambda (x y) (+ x y))` | `\|x, y\| { x + y }` |
| `(pub fn foo () i32 42)` | `pub fn foo() -> i32 { 42 }` |
| `(pub (crate) mod m (fn f () () ()))` | `pub(crate) mod m { fn f() {} }` |
| `(use std::collections::HashMap)` | `use std::collections::HashMap;` |
| `(const MAX usize 1024)` | `const MAX: usize = 1024;` |
| `(rust "let x: i32 = 42; x")` | `let x: i32 = 42; x` |

Binary operators (`+`, `-`, `*`, `/`, `==`, `!=`, `<`, `>`, `&&`, etc.) emit infix: `(+ a b)` → `(a + b)`.

## Macros

rlisp macros are compile-time s-expression transformers — no `proc_macro` crate, no token streaming, no syn/quote. A macro is just a function from s-expressions to s-expressions.

Macro bodies use three special forms borrowed from LISP:

| Form | Meaning |
|------|---------|
| `(quasiquote template)` | "Quote this template, but allow unquotes inside" — like a tagged template literal |
| `(unquote name)` | "Insert the value of `name` here" — a hole in the template |
| `(unquote-splicing name)` | "Splice the list `name` into the surrounding list" — for inserting multiple forms |

Think of `quasiquote` as "return this exact s-expression, except for the `unquote` holes." Without it, you'd have to manually construct every parenthesis with `list` and `cons`.

```lisp
;; Define a when macro: (when condition body...)
(defmacro when (condition &rest body)
  (quasiquote (if (unquote condition) (do (unquote-splicing body)))))

;; Macro expansion:
;;   (when (> x 10) (print "big") (print "huge"))
;; → (if (> x 10) (do (print "big") (print "huge")))
;; → if x > 10 { print("big"); print("huge") }

(defmacro double (x)
  (quasiquote (+ (unquote x) (unquote x))))

;; (double 21) → (+ 21 21) → 21 + 21

(fn main () ()
  (let x 21)
  (println! "Double: {}" (double x))
  (when (> x 10)
    (println! "x is greater than 10")
    (println! "this too")))
```

`&rest` captures all remaining arguments into a list, and `unquote-splicing` flattens that list into the surrounding form. This is how variadic macros work.

## Loops

```lisp
(while (> x 0)
  (println! "{}" x)
  (-= x 1))

(loop (println! "tick"))

(for x in 0..10
  (println! "{}" x))

;; for with destructuring
(for (i val) in (. v iter) enumerate
  (println! "{}: {}" i val))
```

## Closures

```lisp
;; untyped
(let add (lambda (x y) (+ x y)))

;; typed with return type
(let mul (lambda ((x i32) (y i32)) i32 (* x y)))

;; move closure
(let s "hello")
(let greet (lambda move () (println! "{}" s)))
```

## Modules, visibility, and imports

```lisp
(pub fn public_api () i32 42)
(pub (crate) fn internal () i32 0)       ;; pub(crate)
(pub (super) fn parent_visible () i32 1)  ;; pub(super)

(pub struct Config
  (pub host String)                        ;; public field
  (port u16))                              ;; private field

(pub mod utils                            ;; inline module
  (pub fn helper () i32 1)
  (fn private () i32 0))

(mod external_lib)                        ;; external module decl

(use std::collections::HashMap)
(use std::io::{self,Write,Read})
(use std::fmt::Display as Fmt)
```

## Inline Rust

Drop into raw Rust with `(rust "...")` for anything rlisp doesn't express natively.
The string is emitted verbatim into the generated `.rs` file (with LISP escape sequences unescaped):

```lisp
(fn raw_example () i32
  (rust "let x: i32 = 42; x * 2"))

(fn main () ()
  (rust "let message: &str = \"from raw Rust\";")
  (println! "{}" (rust "message")))
```

## Why

Mostly for fun — an exploration of what Rust looks like when you strip away the syntax and keep the semantics. But there are practical angles too:

- **Macros become trivial.** In LISP, a macro is just a function that takes s-expressions and returns s-expressions, executed at compile time. No token streaming, no `proc_macro` ceremony. This is the killer feature LISP brings to Rust.
- **Structural editing.** s-expressions are trivial to manipulate with editor tooling — slurp, barf, transpose, wrap. Every operation is balanced by construction.
- **Homogeneous syntax.** No distinction between expressions, statements, types, and patterns. Everything is an s-expression. `match` arms and function signatures use the same syntax you already know.

## License

MIT
