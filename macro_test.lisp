; Define a when macro
(defmacro when (condition &rest body)
  (quasiquote (if (unquote condition) (do (unquote-splicing body)))))

; Define a simple doubling macro
(defmacro double (x)
  (quasiquote (+ (unquote x) (unquote x))))

(fn main () ()
  (let x 21)
  (println! "Double: {}" (double x))

  (when (> x 10)
    (println! "x is greater than 10")
    (println! "this too")))
