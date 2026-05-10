mod ast;
mod codegen;
mod r#macro;
mod parser;

use std::env;
use std::fs;
use std::path::Path;
use std::process;

use ariadne::{Color, Label, Report, ReportKind, Source};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: rlisp <command> <file.lisp>");
        eprintln!("  compile  — transpile to .rs file");
        eprintln!("  build    — transpile and compile with rustc");
        eprintln!("  run      — transpile, compile, and run");
        process::exit(1);
    }

    let command = &args[1];
    let input_path = &args[2];

    let source = fs::read_to_string(input_path).unwrap_or_else(|e| {
        eprintln!("Error reading {}: {}", input_path, e);
        process::exit(1);
    });

    let ast = parser::parse(&source).unwrap_or_else(|e| {
        let (start, end) = e.span;
        let range = if start == end && start < source.len() {
            let s = source.len().saturating_sub(1);
            s..source.len()
        } else if start == end {
            start..start
        } else {
            start..end
        };
        let file = input_path.as_str();
        Report::build(ReportKind::Error, (file, range.clone()))
            .with_message("Parse error")
            .with_label(
                Label::new((file, range))
                    .with_message(&e.message)
                    .with_color(Color::Red),
            )
            .finish()
            .eprint((file, Source::from(&source)))
            .unwrap();
        process::exit(1);
    });

    let expanded = r#macro::expand(&ast);
    let (rust_code, warnings) = codegen::compile(&expanded);

    let file = input_path.as_str();
    for w in &warnings {
        match &w.span {
            Some(span) => {
                Report::build(ReportKind::Warning, (file, span.start..span.end))
                    .with_message("Warning")
                    .with_label(
                        Label::new((file, span.start..span.end))
                            .with_message(&w.message)
                            .with_color(Color::Yellow),
                    )
                    .finish()
                    .eprint((file, Source::from(&source)))
                    .ok();
            }
            None => {
                eprintln!("warning: {}", w.message);
            }
        }
    }

    let input_path = Path::new(input_path);
    let output_path = input_path.with_extension("rs");

    fs::write(&output_path, &rust_code).unwrap_or_else(|e| {
        eprintln!("Error writing {}: {}", output_path.display(), e);
        process::exit(1);
    });

    match command.as_str() {
        "compile" => {
            println!("Wrote {}", output_path.display());
        }
        "build" => {
            println!("Compiling...");
            let status = process::Command::new("rustc")
                .arg(&output_path)
                .status()
                .unwrap_or_else(|e| {
                    eprintln!("Error running rustc: {}", e);
                    process::exit(1);
                });
            if !status.success() {
                process::exit(status.code().unwrap_or(1));
            }
            println!("Build successful");
        }
        "run" => {
            let exe_path = input_path.with_extension("");
            println!("Compiling...");
            let status = process::Command::new("rustc")
                .arg(&output_path)
                .arg("-o")
                .arg(&exe_path)
                .status()
                .unwrap_or_else(|e| {
                    eprintln!("Error running rustc: {}", e);
                    process::exit(1);
                });
            if !status.success() {
                process::exit(status.code().unwrap_or(1));
            }
            println!("Running...");
            let status = process::Command::new(&exe_path)
                .status()
                .unwrap_or_else(|e| {
                    eprintln!("Error running {}: {}", exe_path.display(), e);
                    process::exit(1);
                });
            process::exit(status.code().unwrap_or(0));
        }
        _ => {
            eprintln!("Unknown command: {}", command);
            eprintln!("Commands: compile, build, run");
            process::exit(1);
        }
    }
}
