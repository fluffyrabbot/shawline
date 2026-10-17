//! The pipeline from source text to diagnostics, output, canonical text and
//! SVG, shared by the `shawline` binary and the corpus.

use shawline_grid::{Diagnostic, Grid, Pos};
use shawline_ir::ir::Program;
use shawline_shape::Diagram;
use shawline_syntax::ast::Sheet;
use shawline_vm::bytecode::Value;

pub use shawline_render::Theme;

/// A sheet taken through every static stage.
pub struct Checked {
    pub grid: Grid,
    pub diagram: Diagram,
    pub sheet: Sheet,
    /// `None` whenever there is an error.
    pub program: Option<Program>,
    /// Every static diagnostic, lints included, sorted by position then code.
    pub diagnostics: Vec<Diagnostic>,
}

impl Checked {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| !d.is_warning())
    }
}

/// Runs every static stage. Grid errors stop everything, so they come back
/// as `Err`. The checker only sees sheets free of shape and syntax errors
/// (SPEC §14), so a broken drawing doesn't also produce phantom name or
/// arity errors.
pub fn check(src: &str) -> Result<Checked, Vec<Diagnostic>> {
    let grid = grid(src)?;
    let diagram = shawline_shape::recognize(&grid);
    let mut diagnostics = diagram.diagnostics.clone();
    let (sheet, syntax) = shawline_syntax::parse(&grid, &diagram);
    diagnostics.extend(syntax);
    let mut program = None;
    if !diagnostics.iter().any(|d| !d.is_warning()) {
        let (lowered, checks) = shawline_ir::lower(&sheet);
        diagnostics.extend(checks);
        program = lowered;
        // A clean sheet always lowers. If it ever doesn't, say so plainly
        // rather than leave callers holding neither a program nor an error.
        if program.is_none() && !diagnostics.iter().any(|d| !d.is_warning()) {
            let message = "internal error: this sheet passed every check but couldn't be compiled; please report it";
            diagnostics.push(Diagnostic::new(Pos::new(0, 0), "I001", message));
        }
    }
    diagnostics.extend(shawline_fmt::lint(&grid, &diagram));
    diagnostics.sort_by(|a, b| (a.at, a.code).cmp(&(b.at, b.code)));
    let mut checked = Checked { grid, diagram, sheet, program, diagnostics };
    if checked.has_errors() {
        checked.program = None;
    }
    Ok(checked)
}

/// Compiles and runs a checked program, returning main's outputs.
pub fn run(program: &Program) -> Result<Vec<Value>, Diagnostic> {
    shawline_vm::run(&shawline_ir::compile(program))
}

/// The step budget for [`trace`]: generous for any sheet a person draws, and
/// small enough that a constant-memory infinite loop stops in about a second.
pub const TRACE_STEPS: u64 = 5_000_000;

/// The value on every wire, for Studio's live badges (PLAN P7). A sheet
/// with errors has no trace: its diagnostics come back as `Err`.
pub fn trace(src: &str) -> Result<shawline_ir::SheetTrace, Vec<Diagnostic>> {
    let checked = check(src)?;
    match &checked.program {
        Some(program) => Ok(shawline_ir::trace(&checked.sheet, program, TRACE_STEPS)),
        None => Err(checked.diagnostics.into_iter().filter(|d| !d.is_warning()).collect()),
    }
}

/// The program's bytecode in `.shawasm` form.
pub fn asm(program: &Program) -> String {
    shawline_vm::disassemble(&shawline_ir::compile(program))
}

/// The canonical form of `src` (SPEC §11).
pub fn format(src: &str) -> Result<String, Vec<Diagnostic>> {
    let grid = grid(src)?;
    Ok(shawline_fmt::format(&grid, &shawline_shape::recognize(&grid)))
}

/// The canonical form of `src` with every slipped border cell moved home
/// (SPEC §3, §11 rule 6).
pub fn fix(src: &str) -> Result<String, Vec<Diagnostic>> {
    let grid = grid(src)?;
    Ok(shawline_fmt::fix(&grid, &shawline_shape::recognize(&grid)))
}

/// `src` drawn as a standalone SVG.
pub fn render(src: &str, theme: Theme) -> Result<String, Vec<Diagnostic>> {
    Ok(shawline_render::render(&grid(src)?, theme))
}

fn grid(src: &str) -> Result<Grid, Vec<Diagnostic>> {
    Grid::parse(src).map_err(|errors| errors.into_iter().map(Diagnostic::from).collect())
}
