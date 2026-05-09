(const MAX_SIZE usize 1024)
(static COUNTER i32 0)
(static mut GLOBAL_STATE u64 42)

(pub const GREETING &str "hello")
(pub (crate) static APP_NAME &str "rlisp")

(fn main () ()
  (println! "{}" MAX_SIZE)
  (println! "{}" GREETING))
