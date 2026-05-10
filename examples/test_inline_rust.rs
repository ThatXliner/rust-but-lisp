fn use_inline_rust() -> i32 {
    let x: i32 = 42; x * 2
}

fn inline_let() -> () {
    let message = "from raw Rust";
    println!("{}", message)
}

fn main() -> () {
    println!("result: {}", use_inline_rust());
    inline_let()
}