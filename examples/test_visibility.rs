pub fn public_func() -> i32 {
    42
}

pub struct Point {
    x: f64,
    y: f64
}

pub struct Config {
    pub host: String,
    port: u16
}

pub enum Status {
    Ok,
    Err
}

pub trait Display {
    fn show(&self) -> String;
}

pub mod utils {
    pub(crate) fn helper() -> i32 {
        1
    }

    pub(super) fn parent_helper() -> i32 {
        0
    }

    fn private_help() -> i32 {
        2
    }
}

use std::collections::HashMap;

use std::io::{self,Write,Read};

use std::fmt::Display as Fmt;

fn main() -> () {
    println!("Hello")
}