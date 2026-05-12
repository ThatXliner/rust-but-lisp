; Struct definition
(struct Point
  (x f64)
  (y f64))

; Enum definition
(enum MyOption (generic T)
  (MySome T)
  MyNone)

; Trait definition
(trait Greet (
  (fn greet ((&self)) String)))

; Impl block
(impl Point (
  (fn new ((x f64) (y f64)) Point
    (raw_new Point (x x) (y y)))
  (fn distance ((&self) (other &Point)) f64
    (let dx (- (. self x) (. other x)))
    (let dy (- (. self y) (. other y)))
    (. dx powf 2.0))))

; Generic struct with lifetime
(struct Borrow (generic 'a)
  (x &'a str))

; Generic function
(fn first (generic T) ((list &[T])) (MyOption (generic &T))
  (if (!= (list.len) 0)
    (MyOption::MySome (& ([] list 0)))
    MyOption::MyNone))

; Main function
(fn main () ()
  (let p1 (raw_new Point (x 1.0) (y 2.0)))
  (let p2 (raw_new Point (x 4.0) (y 6.0)))
  (let d (. p1 distance (& p2)))
  (println! "Distance: {}" d)

  (let opt (MyOption::MySome 42))
  (match opt
    ((MyOption::MySome val) (println! "Got: {}" val))
    (MyOption::MyNone (println! "Nothing")))

  (if (> d 3.0)
    (println! "Far apart")
    (println! "Close together")))
