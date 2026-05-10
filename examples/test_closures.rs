fn use_closure() -> i32 {
    let add = |x, y| {
        (x + y)
    };
    add(1, 2)
}

fn typed_closure() -> i32 {
    let mul = |x: i32, y: i32| -> i32 {
        (x * y)
    };
    mul(3, 4)
}

fn move_closure() -> () {
    let s = "hello";
    let greet = move || {
        s
    };
    println!("{}", greet())
}

fn main() -> () {
    println!("add: {}", use_closure());
    println!("mul: {}", typed_closure());
    move_closure()
}