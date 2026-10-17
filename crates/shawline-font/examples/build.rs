//! Writes Shawline Mono to the path given on the command line:
//! `cargo run -p shawline-font --example build -- ShawlineMono.ttf`

use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = std::env::args_os().nth(1) else {
        eprintln!("usage: build <out.ttf>");
        return ExitCode::FAILURE;
    };
    match std::fs::write(&path, shawline_font::build()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cannot write {}: {e}", path.to_string_lossy());
            ExitCode::FAILURE
        }
    }
}
