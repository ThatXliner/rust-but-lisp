# rlisp syntax reference

## Full syntax map

| LISP | Rust |
|------|------|
| `(fn add ((x i32) (y i32)) i32 (+ x y))` | `fn add(x: i32, y: i32) -> i32 { (x + y) }` |
| `(let x i32 42)` | `let x: i32 = 42;` |
| `(let x 42)` | `let x = 42;` |
| `(let mut x 42)` | `let mut x = 42;` |
| `(struct Point (x f64) (y f64))` | `struct Point { x: f64, y: f64 }` |
| `(struct Pair f64 f64)` | `struct Pair(f64, f64);` |
| `(struct Unit)` | `struct Unit;` |
| `(enum Option (generic T) (Some T) None)` | `enum Option<T> { Some(T), None }` |
| `(match val ((Some x) (handle x)) (None ()))` | `match val { Some(x) => { handle(x) }, None => { } }` |
| `(if cond (then) (else))` | `if cond { then } else { else }` |
| `(impl Point ((fn new (...) ...)))` | `impl Point { fn new(...) ... }` |
| `(trait Display ((fn fmt (...) Result)))` | `trait Display { fn fmt(...) -> Result; }` |
| `(. obj field)` | `obj.field` |
| `(. obj method arg1 arg2)` | `obj.method(arg1, arg2)` |
| `([] arr index)` | `arr[index]` |
| `(loop (body))` | `loop { body }` |
| `(while cond (body))` | `while cond { body }` |
| `(for x in iter (body))` | `for x in iter { body }` |
| `(new Type (field1 val1) (field2 val2))` | `Type { field1: val1, field2: val2 }` |
| `(lambda (x y) (+ x y))` | `\|x, y\| { x + y }` |
| `(lambda ((x i32) (y i32)) i32 (* x y))` | `\|x: i32, y: i32\| -> i32 { x * y }` |
| `(foo! args)` | `foo!(args)` |
| `(println! "{}" x)` | `println!("{}", x)` |
| `(pub fn foo () i32 42)` | `pub fn foo() -> i32 { 42 }` |
| `(pub (crate) fn foo () i32 42)` | `pub(crate) fn foo() -> i32 { 42 }` |
| `(pub struct Config (pub host String) (port u16))` | `pub struct Config { pub host: String, port: u16 }` |
| `(pub mod utils ((fn helper () i32 1)))` | `pub mod utils { fn helper() -> i32 { 1 } }` |
| `(mod external_lib)` | `mod external_lib;` |
| `(use std::collections::HashMap)` | `use std::collections::HashMap;` |
| `(use std::io::{self,Write,Read})` | `use std::io::{self, Write, Read};` |
| `(const MAX usize 1024)` | `const MAX: usize = 1024;` |
| `(static COUNTER i32 0)` | `static COUNTER: i32 = 0;` |
| `(static mut COUNTER i32 0)` | `static mut COUNTER: i32 = 0;` |
| `(rust "let x: i32 = 42; x")` | `let x: i32 = 42; x` (emitted verbatim) |
| `(:: collect Vec<_>)` | `collect::<Vec<_>>` (turbofish) |
| `(break)` | `break;` |
| `(break val)` | `break val;` |
| `(continue)` | `continue;` |
| `(return expr)` | `return expr;` |
| `(as x i32)` | `x as i32` |
| `(if-let (Some v) x (body) (else))` | `if let Some(v) = x { body } else { else }` |
| `(while-let (Some v) iter (body))` | `while let Some(v) = iter { body }` |
| `(unsafe (body))` | `unsafe { body }` |
| `(generic (T Display) K)` | `<T: Display, K>` |
| `(where (T Display Clone) ('a 'b))` | `where T: Display + Clone, 'a: 'b` |
| `(struct (derive Debug Clone) Point (x i32))` | `#[derive(Debug, Clone)] struct Point { x: i32 }` |
| `(trait Foo Display ((fn bar () ())))` | `trait Foo: Display { fn bar(); }` |
| `(trait Iterator ((type Item) (fn next () ())))` | `trait Iterator { type Item; fn next(); }` |
| `(impl (generic T) (Vec T) ((fn push (...) ...)))` | `impl<T> Vec<T> { fn push(...) ... }` |
| `(type Meters i32)` | `type Meters = i32;` |
| `(type Stack (generic T) (Vec T))` | `type Stack<T> = Vec<T>;` |

## Generics

Generics are introduced with the `generic` command:

```lisp
(enum Option (generic T) (Some T) None)  ;; Option<T>
(struct Wrapper (generic T) T)            ;; Wrapper<T>
(fn first (generic T) ((list &[T])) &T)  ;; fn first<T>(list: &[T]) -> &T
```

### Inline trait bounds

Bounds on generic parameters use a list where the first element is the parameter name and the rest are bounds joined by ` + `:

```lisp
(fn foo (generic (T Display)) ((x T)) () ())         ;; <T: Display>
(fn foo (generic (T Display Clone)) ((x T)) () ())   ;; <T: Display + Clone>
(fn foo (generic (K Display) V) ((k K) (v V)) () ()) ;; <K: Display, V>
```

### Where clauses

`(where ...)` clauses work on fn, struct, enum, trait, impl, and type aliases:

```lisp
(fn foo (generic T) (where (T Display)) ((x T)) String (to_string x))
;; fn foo<T>(x: T) -> String where T: Display { to_string(x) }

(struct Pair (generic T) (where (T Clone)) (first T) (second T))
;; struct Pair<T> { first: T, second: T } where T: Clone

(impl (generic T) (where (T Display)) (Vec T) (
  (fn print_all ((&self)) () ...)))
;; impl<T> Vec<T> where T: Display { fn print_all(&self) { ... } }
```

Where clauses can constrain lifetimes too:

```lisp
(where (T Display Clone) ('a 'b))
;; where T: Display + Clone, 'a: 'b
```

### Derive attributes

`(derive ...)` on fn, struct, and enum items:

```lisp
(struct (derive Debug Clone PartialEq) Point (x i32) (y i32))
;; #[derive(Debug, Clone, PartialEq)]
;; struct Point { x: i32, y: i32 }

(enum (derive Debug) Status Ok Err)
;; #[derive(Debug)]
;; enum Status { Ok, Err }
```

### Supertraits

An uppercase symbol or a list of bounds after the trait name declares supertraits:

```lisp
(trait Foo Display ((fn fmt () ())))
;; trait Foo: Display { fn fmt(); }

(trait Foo (+ Display Clone) ((fn bar () ())))
;; trait Foo: Display + Clone { fn bar(); }
```

### Associated types

Use `(type Name)` or `(type Name Bounds...)` inside trait body:

```lisp
(trait Iterator (generic T) ((type Item) (fn next ((&mut self)) (Option T Self::Item))))
;; trait Iterator<T> { type Item; fn next(&mut self) -> Option<T, Self::Item>; }

(trait Graph ((type Node Display Clone) (fn nodes () ())))
;; trait Graph { type Node: Display + Clone; fn nodes(); }
```

### Type aliases

```lisp
(type Meters i32)                              ;; type Meters = i32;
(type Stack (generic T) (Vec T))               ;; type Stack<T> = Vec<T>;
(type Stack (generic T) (where (T Clone)) (Vec T))  ;; type Stack<T> = Vec<T> where T: Clone;
```

Lifetimes and generics with the `generic` command (same as fn/struct/enum):

```lisp
(struct Borrow (generic 'a)
  (x &'a str))

(fn longest (generic 'a) ((x &'a str) (y &'a str)) (&'a str)
  (if (> (. x len) (. y len)) x y))

(fn foo (generic 'static) ((x &'static str)) (&'static str) x)
```

## Kebab-case identifiers

Hyphens in function names, variables, fields, and enum variants are automatically converted to `__` (double underscore). Collisions (e.g. `foo-bar` and `foo__bar` both → `foo__bar`) emit a compile warning.

```
page-header       →  page__header
my-variable-name  →  my__variable__name
html-content      →  html__content
```

## Binary operators

All common binary operators emit infix. Written as prefix s-expressions, output as their Rust infix forms:

```lisp
(+ a b)      →  (a + b)
(-= x 1)     →  (x -= 1)
(&& cond1 (== x y))  →  (cond1 && (x == y))
(> x 0)      →  (x > 0)
```

Full operator set: `+`, `-`, `*`, `/`, `%`, `==`, `!=`, `<`, `>`, `<=`, `>=`, `&&`, `||`, `&`, `|`, `^`, `<<`, `>>`, `=`, `+=`, `-=`, `*=`, `/=`.

## Type annotations

Parameters use named-tuple syntax: `(name type1 type2...)`:

```lisp
(fn greet ((name &str) (age u32)) String
  (format! "Hello {}, you are {}" name age))

(fn ref-example ((x &i32)) i32       ;; shared reference
  (* x 2))

(fn mut-ref ((x &mut i32)) ()         ;; mutable reference
  (*= x 2))
```

## Struct initialization

`(new Type)` becomes `Type::new()` when the Type is lowercase (function call), or `Type { ... }` with named fields:

```lisp
(new Point (x 1.0) (y 2.0))   →  Point { x: 1.0, y: 2.0 }
(new Vec)                       →  Vec::new()
```

## Pattern matching

`match` arm patterns follow the same s-expression structure:

```lisp
(match val
  ((Some x) (println! "{}" x))
  (None (println! "nothing")))

;; With guards
(match pair
  ((x y) if (> x 0) (println! "positive first"))
  ((x y) (println! "non-positive first")))
```

## Inline Rust

Drop into raw Rust with `(rust "...")`. The string is emitted verbatim:

```lisp
(fn raw_example () i32
  (rust "let x: i32 = 42; x * 2"))
```

## Indexing

```lisp
([] arr 0)       →  arr[0]
([] matrix i j)  →  matrix[i][j]
```

## Unsafe

```lisp
(unsafe
  (rust "let ptr: *const i32 = &42;")
  (rust "*ptr"))
```

## Do blocks

`(do expr1 expr2 ... exprN)` emits a block with semicolons after all but the last expression:

```lisp
(do (side-effect!)
    (another!)
    result-value)   →  { side_effect!(); another!(); result_value }
```
