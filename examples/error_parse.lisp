; Example with a parse error: missing closing paren
(struct Point
  (x f64)
  (y f64)

; Missing ) above — this will trigger an ariadne parse error

(fn main () ()
  (println! "Hello"))
