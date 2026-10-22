//! `shawline`: the command-line front end.

use std::process::ExitCode;
use std::{env, fs};

use shawline_doc::Doc;
use shawline_grid::Diagnostic;
use shawline_render::{Options, Theme};

const USAGE: &str = "usage: shawline <command> [flags] <file.shaw>

commands:
  check     report diagnostics
  run       check, then run ·𐑥𐑱𐑯 and print its outputs
  fmt       print the canonical form (--fix: also move slipped border cells
            home; --write: rewrite the file in place)
  render    print an SVG drawing with each word's reading in a strip under
            its row (--dark: the dark theme)
  diagram   print the recognized diagram
  ast       print the syntax tree
  asm       print the compiled bytecode as .shawasm

Messages follow each Shavian word with its reading, as in 𐑥𐑱𐑯 (mayn).
--plain turns the readings off, in messages and in the drawing.
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
    let allowed: &[&str] = match command.as_str() {
        "fmt" => &["--write", "--fix", "--plain"],
        "render" => &["--dark", "--plain"],
        _ => &["--plain"],
    };
    let known = COMMANDS.contains(&command.as_str()) && flags.iter().all(|f| allowed.contains(&f.as_str()));
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
    let plain = flag("--plain");
    let message = |m: &str| if plain { m.to_owned() } else { shawline_word::annotate(m) };
    let report_all = |diagnostics: &[Diagnostic]| {
        for d in diagnostics {
            report(path, &src, d, &message(&d.message));
        }
        ExitCode::FAILURE
    };

    let doc = Doc::new(src.as_str());
    let grid = match doc.grid() {
        Ok(grid) => grid,
        Err(errors) => return report_all(errors),
    };

    match command.as_str() {
        "fmt" => {
            // On the grid, formatting always succeeds.
            let text = if flag("--fix") { doc.fixed() } else { doc.formatted() }.unwrap_or_default();
            if !flag("--write") {
                print!("{text}");
            } else if text != src
                && let Err(e) = replace(path, text)
            {
                eprintln!("shawline: {path}: {e}");
                return ExitCode::FAILURE;
            }
        }
        "render" => {
            let theme = if flag("--dark") { Theme::DARK } else { Theme::LIGHT };
            print!("{}", shawline_render::render_with(grid, Options { theme, gloss: !plain }));
        }
        _ => {
            match command.as_str() {
                "diagram" => print!("{}", doc.diagram().expect("on the grid")),
                "ast" => print!("{}", doc.sheet().expect("on the grid")),
                _ => {}
            }
            report_all(doc.diagnostics());
            // A sheet without errors always has a module.
            let Some(module) = doc.module() else {
                return ExitCode::FAILURE;
            };
            if command.as_str() == "asm" {
                print!("{}", shawline_vm::disassemble(module));
            }
            if command.as_str() == "run" {
                match shawline_vm::run(module) {
                    Ok(values) => values.iter().for_each(|v| println!("{v}")),
                    Err(e) => {
                        // SPEC §9: zero-based position, nothing on stdout.
                        eprintln!("error {} {}: {}", e.at, e.code, message(&e.message));
                        return ExitCode::FAILURE;
                    }
                }
            }
        }
    }
    ExitCode::SUCCESS
}

/// Writes `text` over `path` without ever leaving it half-written: the new
/// text goes to a sibling file first, which then replaces the original.
fn replace(path: &str, text: &str) -> std::io::Result<()> {
    let target = std::path::Path::new(path);
    let name = target.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    let temp = target.with_file_name(format!(".{name}.shawline-tmp"));
    fs::write(&temp, text)?;
    fs::rename(&temp, target).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}

/// Prints `d` with `message`, its source line and a caret. Every repertoire
/// character is one cell wide, so the caret lines up in Shawline Mono.
fn report(path: &str, src: &str, d: &Diagnostic, message: &str) {
    eprintln!("{path}:{}:{}: {} {message}", d.at.row + 1, d.at.col + 1, d.code);
    if let Some(line) = src.lines().nth(d.at.row) {
        eprintln!("  {line}\n  {}^", " ".repeat(d.at.col));
    }
}
