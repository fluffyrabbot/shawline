//! One revision of a sheet and everything read from it (PLAN P8).
//!
//! Lead-owned. A [`Doc`] holds one text and never changes. Each stage of the
//! pipeline is computed the first time something asks for it and then kept,
//! so every consumer of one revision (the CLI, the corpus, the language
//! server, the playground and the edits) shares a single reading, and
//! nothing is computed that nobody asked for. Every consumer is
//! single-threaded, so the stages sit in [`OnceCell`]s.
//!
//! ```text
//! text ─► grid ─► diagram ─► sheet ─► diagnostics, program ─► module ─► run
//!                    └─────► formatted, fixed ─► canonical
//! ```

use std::cell::OnceCell;
use std::fmt;

use shawline_grid::{Diagnostic, Grid, Pos};
use shawline_ir::SheetTrace;
use shawline_ir::ir::Program;
use shawline_shape::Diagram;
use shawline_syntax::ast::Sheet;
use shawline_vm::bytecode::{Module, Value};

/// The step budget for live badges ([`Doc::trace`]): generous for any sheet
/// a person draws, and small enough that a constant-memory infinite loop
/// stops in about a second.
pub const TRACE_STEPS: u64 = 5_000_000;

/// One revision of a sheet: its text, and each stage read from it on first
/// use.
pub struct Doc {
    text: String,
    grid: OnceCell<Result<Grid, Vec<Diagnostic>>>,
    diagram: OnceCell<Diagram>,
    /// The syntax tree and syntax diagnostics.
    syntax: OnceCell<(Sheet, Vec<Diagnostic>)>,
    checked: OnceCell<Checked>,
    module: OnceCell<Option<Module>>,
    formatted: OnceCell<Option<String>>,
    fixed: OnceCell<Option<String>>,
    canonical: OnceCell<Canonical>,
}

/// Every static stage's verdict.
struct Checked {
    /// Sorted by position, then code.
    diagnostics: Vec<Diagnostic>,
    /// `None` whenever there is an error.
    program: Option<Program>,
}

enum Canonical {
    /// The text has grid errors, so it has no canonical form.
    Unreadable,
    Itself,
    Other(Box<Doc>),
}

impl Doc {
    /// A revision holding `text`. Nothing is read yet.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            grid: OnceCell::new(),
            diagram: OnceCell::new(),
            syntax: OnceCell::new(),
            checked: OnceCell::new(),
            module: OnceCell::new(),
            formatted: OnceCell::new(),
            fixed: OnceCell::new(),
            canonical: OnceCell::new(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The grid of cells, or the grid errors that stop every later stage.
    pub fn grid(&self) -> Result<&Grid, &[Diagnostic]> {
        let parsed = self.grid.get_or_init(|| {
            Grid::parse(&self.text)
                .map_err(|errors| errors.into_iter().map(Diagnostic::from).collect())
        });
        match parsed {
            Ok(grid) => Ok(grid),
            Err(errors) => Err(errors),
        }
    }

    /// The recognized drawing. `None` when the text has grid errors.
    pub fn diagram(&self) -> Option<&Diagram> {
        let grid = self.grid().ok()?;
        Some(self.diagram.get_or_init(|| shawline_shape::recognize(grid)))
    }

    /// The syntax tree. `None` when the text has grid errors; a sheet with
    /// shape or syntax errors still has one.
    pub fn sheet(&self) -> Option<&Sheet> {
        self.syntax().map(|(sheet, _)| sheet)
    }

    fn syntax(&self) -> Option<&(Sheet, Vec<Diagnostic>)> {
        let (grid, diagram) = (self.grid().ok()?, self.diagram()?);
        Some(self.syntax.get_or_init(|| shawline_syntax::parse(grid, diagram)))
    }

    /// Every static diagnostic, lints included, sorted by position then
    /// code. A text with grid errors has only those.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self.checked() {
            Some(checked) => &checked.diagnostics,
            None => self.grid().err().unwrap_or_default(),
        }
    }

    /// Whether any diagnostic is an error: then there is no program.
    pub fn has_errors(&self) -> bool {
        self.diagnostics().iter().any(|d| !d.is_warning())
    }

    /// The lowered program, present exactly when there are no errors.
    pub fn program(&self) -> Option<&Program> {
        self.checked()?.program.as_ref()
    }

    /// The program's bytecode, compiled once.
    pub fn module(&self) -> Option<&Module> {
        let program = self.program()?;
        self.module.get_or_init(|| Some(shawline_ir::compile(program))).as_ref()
    }

    /// Runs `·𐑥𐑱𐑯`: its outputs, or the runtime error that stopped it.
    /// `None` when there is no program. Each call runs it again.
    pub fn run(&self) -> Option<Result<Vec<Value>, Diagnostic>> {
        self.module().map(shawline_vm::run)
    }

    /// The value on every wire within `steps` VM steps, for Studio's live
    /// badges (PLAN P7). `None` when there is no program.
    pub fn trace(&self, steps: u64) -> Option<SheetTrace> {
        Some(shawline_ir::trace(self.sheet()?, self.program()?, steps))
    }

    /// The canonical form of the text (SPEC §11). `None` when the text has
    /// grid errors.
    pub fn formatted(&self) -> Option<&str> {
        let (grid, diagram) = (self.grid().ok()?, self.diagram()?);
        self.formatted.get_or_init(|| Some(shawline_fmt::format(grid, diagram))).as_deref()
    }

    /// The canonical form with every slipped border cell moved home (SPEC
    /// §3, §11 rule 6). `None` when the text has grid errors.
    pub fn fixed(&self) -> Option<&str> {
        let (grid, diagram) = (self.grid().ok()?, self.diagram()?);
        self.fixed.get_or_init(|| Some(shawline_fmt::fix(grid, diagram))).as_deref()
    }

    /// The canonical revision: this one when its text is already canonical,
    /// otherwise a revision of [`Doc::formatted`]. `None` when the text has
    /// grid errors.
    pub fn canonical(&self) -> Option<&Doc> {
        let canonical = self.canonical.get_or_init(|| match self.formatted() {
            None => Canonical::Unreadable,
            Some(text) if text == self.text => Canonical::Itself,
            Some(text) => {
                // The formatter is idempotent (`fmt_properties`, the jitter
                // corpus), so the canonical revision is its own canonical
                // form and is never formatted again.
                let doc = Doc::new(text);
                let _ = doc.formatted.set(Some(doc.text.clone()));
                let _ = doc.canonical.set(Canonical::Itself);
                Canonical::Other(Box::new(doc))
            }
        });
        match canonical {
            Canonical::Unreadable => None,
            Canonical::Itself => Some(self),
            Canonical::Other(doc) => Some(doc),
        }
    }

    /// The checker only sees sheets free of shape and syntax errors (SPEC
    /// §14), so a broken drawing doesn't also produce phantom name or arity
    /// errors.
    fn checked(&self) -> Option<&Checked> {
        let (grid, diagram) = (self.grid().ok()?, self.diagram()?);
        let (sheet, syntax) = self.syntax()?;
        Some(self.checked.get_or_init(|| {
            let mut diagnostics = diagram.diagnostics.clone();
            diagnostics.extend(syntax.iter().cloned());
            let mut program = None;
            if !diagnostics.iter().any(|d| !d.is_warning()) {
                let (lowered, checks) = shawline_ir::lower(sheet);
                diagnostics.extend(checks);
                program = lowered;
                // A clean sheet always lowers. If it ever doesn't, say so
                // plainly rather than leave callers holding neither a
                // program nor an error.
                if program.is_none() && !diagnostics.iter().any(|d| !d.is_warning()) {
                    let message = "internal error: this sheet passed every check but couldn't be compiled; please report it";
                    diagnostics.push(Diagnostic::new(Pos::new(0, 0), "I001", message));
                }
            }
            diagnostics.extend(shawline_fmt::lint(grid, diagram));
            diagnostics.sort_by(|a, b| (a.at, a.code).cmp(&(b.at, b.code)));
            if diagnostics.iter().any(|d| !d.is_warning()) {
                program = None;
            }
            Checked { diagnostics, program }
        }))
    }
}

impl fmt::Debug for Doc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Doc").field(&self.text).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOUBLE: &str = include_str!("../../../corpus/03-double.shaw");
    const ASCII: &str = include_str!("../../../corpus/03-double-ascii.shaw");

    #[test]
    fn a_clean_sheet_runs() {
        let doc = Doc::new(DOUBLE);
        assert!(doc.diagnostics().is_empty(), "{:?}", doc.diagnostics());
        assert!(!doc.has_errors());
        let printed: Vec<String> = doc.run().unwrap().unwrap().iter().map(|v| v.to_string()).collect();
        assert_eq!(printed.join("\n") + "\n", include_str!("../../../corpus/03-double.out"));
        let trace = doc.trace(TRACE_STEPS).unwrap();
        assert!(!trace.stopped && trace.error.is_none() && !trace.wires.is_empty());
    }

    #[test]
    fn a_canonical_text_is_its_own_canonical_form() {
        let doc = Doc::new(DOUBLE);
        assert!(std::ptr::eq(doc.canonical().unwrap(), &doc));
    }

    #[test]
    fn a_sketch_has_a_separate_canonical_revision() {
        let doc = Doc::new(ASCII);
        let canonical = doc.canonical().unwrap();
        assert!(!std::ptr::eq(canonical, &doc));
        assert_eq!(canonical.text(), DOUBLE);
        assert!(std::ptr::eq(canonical.canonical().unwrap(), canonical));
    }

    #[test]
    fn grid_errors_stop_everything() {
        let doc = Doc::new("\t·𐑛\n");
        let codes: Vec<&str> = doc.diagnostics().iter().map(|d| d.code).collect();
        assert_eq!(codes, ["G001"]);
        assert!(doc.has_errors());
        assert!(doc.grid().is_err() && doc.diagram().is_none() && doc.sheet().is_none());
        assert!(doc.formatted().is_none() && doc.canonical().is_none() && doc.run().is_none());
    }

    #[test]
    fn errors_leave_no_program() {
        let doc = Doc::new(include_str!("../../../corpus/99-square-top.shaw"));
        assert!(doc.has_errors());
        assert!(doc.sheet().is_some());
        assert!(doc.program().is_none() && doc.module().is_none() && doc.trace(1).is_none());
    }
}
