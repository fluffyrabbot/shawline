//! `shawline`: the command-line front end.

use std::process::ExitCode;
use std::{env, fs};

use shawline_cli::Theme;
use shawline_grid::Diagnostic;

const USAGE: &str = "usage: shawline <command> [--dark] <file.shaw>

commands:
  check     report diagnostics
  run       check, then run ·𐑥𐑱𐑯 and print its outputs
  fmt       print the canonical form
  render    print an SVG drawing (--dark for the dark theme)
  diagram   print the recognized diagram
  ast       print the syntax tree
";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let dark = args.iter().any(|a| a == "--dark");
    let rest: Vec<&String> = args.iter().filter(|a| *a != "--dark").collect();
    let [command, path] = rest.as_slice() else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    if !["check", "run", "fmt", "render", "diagram", "ast"].contains(&command.as_str()) {
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
    let report_all = |diagnostics: &[Diagnostic]| {
        for d in diagnostics {
            report(path, &src, d);
        }
        ExitCode::FAILURE
    };

    match command.as_str() {
        "fmt" => match shawline_cli::format(&src) {
            Ok(text) => print!("{text}"),
            Err(errors) => return report_all(&errors),
        },
        "render" => {
            let theme = if dark { Theme::DARK } else { Theme::LIGHT };
            match shawline_cli::render(&src, theme) {
                Ok(svg) => print!("{svg}"),
                Err(errors) => return report_all(&errors),
            }
        }
        _ => {
            let checked = match shawline_cli::check(&src) {
                Ok(checked) => checked,
                Err(errors) => return report_all(&errors),
            };
            match command.as_str() {
                "diagram" => print!("{}", checked.diagram),
                "ast" => print!("{}", checked.sheet),
                _ => {}
            }
            report_all(&checked.diagnostics);
            if checked.has_errors() {
                return ExitCode::FAILURE;
            }
            if command.as_str() == "run" {
                let program = checked.program.as_ref().expect("no errors, so lowered");
                match shawline_cli::run(program) {
                    Ok(values) => values.iter().for_each(|v| println!("{v}")),
                    Err(e) => {
                        // SPEC §9: zero-based position, nothing on stdout.
                        eprintln!("error {} {}: {}", e.at, e.code, e.message);
                        return ExitCode::FAILURE;
                    }
                }
            }
        }
    }
    ExitCode::SUCCESS
}

/// Prints `d` with its source line and a caret. Every repertoire character is
/// one cell wide, so the caret lines up in Shawline Mono.
fn report(path: &str, src: &str, d: &Diagnostic) {
    eprintln!("{path}:{}:{}: {} {}", d.at.row + 1, d.at.col + 1, d.code, d.message);
    if let Some(line) = src.lines().nth(d.at.row) {
        eprintln!("  {line}\n  {}^", " ".repeat(d.at.col));
    }
}
