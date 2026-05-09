(fn use_inline_rust () i32
  (rust "let x: i32 = 42; x * 2"))

(fn inline_let () ()
  (rust "let message = \"from raw Rust\";")
  (rust "println!(\"{}\", message);"))

(fn main () ()
  (println! "result: {}" (use_inline_rust))
  (inline_let))
