//! `shawline`: the command-line front end.

use std::process::ExitCode;
use std::{env, fs};

use shawline_cli::Theme;
use shawline_grid::Diagnostic;

const USAGE: &str = "usage: shawline <command> [flags] <file.shaw>

commands:
  check     report diagnostics
  run       check, then run ·𐑥𐑱𐑯 and print its outputs
  fmt       print the canonical form (--write: rewrite the file in place)
  render    print an SVG drawing (--dark: the dark theme)
  diagram   print the recognized diagram
  ast       print the syntax tree
  asm       print the compiled bytecode as .shawasm
";

const COMMANDS: [&str; 7] = ["check", "run", "fmt", "render", "diagram", "ast", "asm"];

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let (flags, rest): (Vec<&String>, Vec<&String>) = args.iter().partition(|a| a.starts_with("--"));
    let flag = |name: &str| flags.iter().any(|f| *f == name);
    let [command, path] = rest.as_slice() else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    let known = match command.as_str() {
        "fmt" => flags.iter().all(|f| *f == "--write"),
        "render" => flags.iter().all(|f| *f == "--dark"),
        c => COMMANDS.contains(&c) && flags.is_empty(),
    };
    if !known {
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
            Ok(text) if flag("--write") => {
                if text != src
                    && let Err(e) = fs::write(path, &text)
                {
                    eprintln!("shawline: {path}: {e}");
                    return ExitCode::FAILURE;
                }
            }
            Ok(text) => print!("{text}"),
            Err(errors) => return report_all(&errors),
        },
        "render" => {
            let theme = if flag("--dark") { Theme::DARK } else { Theme::LIGHT };
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
            // `check` guarantees a program whenever there are no errors.
            let Some(program) = checked.program.as_ref() else {
                return ExitCode::FAILURE;
            };
            if command.as_str() == "asm" {
                print!("{}", shawline_cli::asm(program));
            }
            if command.as_str() == "run" {
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
