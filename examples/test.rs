struct Point {
    x: f64,
    y: f64
}

enum MyOption<T> {
    MySome(T),
    MyNone
}

trait Greet {
    fn greet(&self) -> String;
}

impl Point {
    fn new(x: f64, y: f64) -> Point {
        Point { x: x, y: y }
    }

    fn distance(&self, other: &Point) -> f64 {
        let dx = (self.x - other.x);
        let dy = (self.y - other.y);
        dx.powf(2.0)
    }
}

struct Borrow<'a> {
    x: &'a str
}

fn first<T>(list: &[T]) -> MyOption<&T> {
    if (list.len() != 0) { MyOption::MySome(&(list[0])) } else { MyOption::MyNone }
}

fn main() -> () {
    let p1 = Point { x: 1.0, y: 2.0 };
    let p2 = Point { x: 4.0, y: 6.0 };
    let d = p1.distance(&(p2));
    println!("Distance: {}", d);
    let opt = MyOption::MySome(42);
    match opt {
        MyOption::MySome(val) => { println!("Got: {}", val) },
        MyOption::MyNone => { println!("Nothing") }
    };
    if (d > 3.0) { println!("Far apart") } else { println!("Close together") }
}