//! `shawline`: the command-line front end.

use std::process::ExitCode;
use std::{env, fs};

use shawline_grid::{Diagnostic, Grid};

const USAGE: &str = "usage: shawline <command> <file.shaw>

commands:
  check     report diagnostics
  diagram   print the recognized diagram
";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let [command, path] = args.as_slice() else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    if !matches!(command.as_str(), "check" | "diagram") {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    }
    let src = match fs::read_to_string(path) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("shawline: {path}: {e}");
            return ExitCode::from(2);
        }
    };
    let grid = match Grid::parse(&src) {
        Ok(grid) => grid,
        Err(errors) => {
            for e in errors {
                report(path, &src, &e.into());
            }
            return ExitCode::FAILURE;
        }
    };

    let diagram = shawline_shape::recognize(&grid);
    if command == "diagram" {
        print!("{diagram}");
    }
    for d in &diagram.diagnostics {
        report(path, &src, d);
    }
    if diagram.diagnostics.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// Prints `d` with its source line and a caret. Every repertoire character is
/// one cell wide, so the caret lines up in Shawline Mono.
fn report(path: &str, src: &str, d: &Diagnostic) {
    eprintln!("{path}:{}:{}: {} {}", d.at.row + 1, d.at.col + 1, d.code, d.message);
    if let Some(line) = src.lines().nth(d.at.row) {
        eprintln!("  {line}\n  {}^", " ".repeat(d.at.col));
    }
}
