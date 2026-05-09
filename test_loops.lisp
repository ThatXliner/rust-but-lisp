(fn countdown ((n i32)) ()
  (let mut x n)
  (while (> x 0)
    (println! "{}" x)
    (-= x 1)))

(fn first_loop () ()
  (loop (println! "once")
    (break)))

(fn sum_to ((n i32)) i32
  (let mut total 0)
  (let mut i 0)
  (while (< i n)
    (+= total i)
    (+= i 1))
  total)

(fn main () ()
  (countdown 3)
  (let s (sum_to 5))
  (println! "sum: {}" s))
