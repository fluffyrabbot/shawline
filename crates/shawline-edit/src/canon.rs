//! The sheet as the caller gave it, versus the canonical sheet edits draw on.
//!
//! Edits draw on canonical text, so each stroke has one meaning and the
//! result needs no tidying. The caller's positions, though, name cells of
//! the text it shows, and formatting moves things: leading and doubled
//! blank lines go, text is respaced along its row, a wire's dip is
//! straightened. Read after formatting, a position can name the wrong cell,
//! or a different thing in the right one. So every target is resolved on
//! the given text and carried over by identity:
//!
//! - boxes, each box's ports and the texts keep their number and order,
//!   because the formatter's safety net compares them index by index;
//! - a wire keeps what it joins;
//! - a cell inside a box keeps its place relative to the box, because no
//!   row inside a box is dropped or inserted and no box moves sideways.

use shawline_doc::Doc;
use shawline_grid::{Grid, Pos};
use shawline_shape::Diagram;

use crate::model::{Model, Term};
use crate::{Refusal, RefusalKind};

/// A sheet read for editing: as given, and in canonical form. Both are
/// borrowed from the given [`Doc`], which keeps its canonical revision
/// too, so every edit and preview of one revision shares one reading.
pub(crate) struct Sheets<'a> {
    given: (&'a Grid, &'a Diagram),
    /// `None` when the given text is already canonical.
    canon: Option<(&'a Grid, &'a Diagram)>,
    /// The canonical revision.
    doc: &'a Doc,
}

impl<'a> Sheets<'a> {
    /// Reads `doc` and its canonical revision. A refusal about the whole
    /// sheet points at `at`.
    pub(crate) fn read(doc: &'a Doc, at: Pos) -> Result<Self, Refusal> {
        let grid = doc.grid().map_err(|errors| {
            let why = "the sheet has a character Shawline can't read; fix it first";
            Refusal::new(errors.first().map_or(at, |e| e.at), RefusalKind::Unreadable, why)
        })?;
        let diagram = doc.diagram().expect("a sheet on the grid is recognized");
        let untidy = || {
            let why = "the sheet can't be tidied for editing; fix the drawing in the text pane first";
            Refusal::new(at, RefusalKind::Unreadable, why)
        };
        let canonical = doc.canonical().ok_or_else(untidy)?;
        if std::ptr::eq(canonical, doc) {
            return Ok(Self { given: (grid, diagram), canon: None, doc });
        }
        let canon_grid = canonical.grid().map_err(|_| untidy())?;
        let canon = canonical.diagram().expect("a sheet on the grid is recognized");
        // What the identities rest on. The formatter keeps all of it.
        let size = |b: &shawline_shape::BoxShape| {
            (b.rect.bottom - b.rect.top, b.rect.right - b.rect.left, b.ports.len())
        };
        let kept = diagram.texts.len() == canon.texts.len()
            && diagram.boxes.len() == canon.boxes.len()
            && diagram.boxes.iter().zip(&canon.boxes).all(|(a, b)| size(a) == size(b));
        if !kept {
            return Err(untidy());
        }
        Ok(Self { given: (grid, diagram), canon: Some((canon_grid, canon)), doc: canonical })
    }

    /// The sheet as given: what the caller's positions name.
    pub(crate) fn given(&self) -> (&'a Grid, &'a Diagram) {
        self.given
    }

    /// The canonical sheet: what edits draw on.
    pub(crate) fn canon(&self) -> (&'a Grid, &'a Diagram) {
        self.canon.unwrap_or(self.given)
    }

    /// Whether the given text isn't canonical, so edits draw on its
    /// formatted text.
    pub(crate) fn tidied(&self) -> bool {
        self.canon.is_some()
    }

    /// The canonical revision.
    pub(crate) fn doc(&self) -> &'a Doc {
        self.doc
    }

    /// The top-left corner of box `b` in the canonical sheet.
    pub(crate) fn corner(&self, b: usize) -> Pos {
        corner(self.canon().1, b)
    }

    /// The canonical cell naming what given cell `p` names, when that is a
    /// box port or a cell of a text: the same port, or the same cell of the
    /// same text. `None` for any other cell.
    pub(crate) fn entity(&self, p: Pos) -> Option<Pos> {
        let (given, canon) = (self.given().1, self.canon().1);
        if let Some((b, k)) = port_index(given, p) {
            return Some(canon.boxes[b].ports[k].pos);
        }
        let t = given.texts.iter().position(|t| {
            t.pos.row == p.row && (t.pos.col..t.pos.col + t.width).contains(&p.col)
        })?;
        let to = canon.texts[t].pos;
        Some(Pos::new(to.row, to.col + (p.col - given.texts[t].pos.col)))
    }

    /// Given cell `p` carried over by its place in the innermost box around
    /// it. `None` outside every box, where nothing holds a place.
    pub(crate) fn place(&self, p: Pos) -> Option<Pos> {
        let given = self.given().1;
        let b = (0..given.boxes.len())
            .filter(|&b| given.boxes[b].rect.contains(p))
            .min_by_key(|&b| given.boxes[b].rect.area())?;
        rebase(p, corner(given, b), self.corner(b))
    }

    /// Canonical cell `p`, which lies in box `b`, as a cell of the given
    /// text: [`Sheets::place`] the other way.
    pub(crate) fn back(&self, b: usize, p: Pos) -> Option<Pos> {
        rebase(p, self.corner(b), corner(self.given().1, b))
    }

    /// Canonical cell `p` as a cell of the given text, by its place in the
    /// innermost box around it. `None` outside every box.
    pub(crate) fn shown(&self, p: Pos) -> Option<Pos> {
        let canon = self.canon().1;
        let b = (0..canon.boxes.len())
            .filter(|&b| canon.boxes[b].rect.contains(p))
            .min_by_key(|&b| canon.boxes[b].rect.area())?;
        self.back(b, p)
    }

    /// The canonical index of wire `w` of the given sheet, which passes
    /// through given cell `at`. `given` and `canon` model the two sheets.
    ///
    /// A wire is found by what it joins. Ports and texts carry over
    /// exactly; a loose end doesn't, so two wires joining only loose ends
    /// can look alike, and then the one through `at`'s place is taken.
    pub(crate) fn wire(
        &self,
        given: &Model,
        canon: &Model,
        w: usize,
        at: Pos,
    ) -> Result<usize, &'static str> {
        if self.canon.is_none() {
            return Ok(w);
        }
        let want = shape(given, w, |p| self.entity(p));
        let alike: Vec<usize> =
            (0..canon.terms.len()).filter(|&v| shape(canon, v, Some) == want).collect();
        if let [v] = alike[..] {
            return Ok(v);
        }
        let cell = self.place(at);
        let here: Vec<usize> = alike
            .into_iter()
            .filter(|&v| cell.is_some_and(|c| canon.diagram.wires[v].cells.contains(&c)))
            .collect();
        match here[..] {
            [v] => Ok(v),
            _ => Err(LOOK_ALIKE),
        }
    }
}

/// What wire `w` joins, with ports and texts named through `lift` and
/// loose ends by direction only, plus its tee and crossing counts.
fn shape(model: &Model, w: usize, lift: impl Fn(Pos) -> Option<Pos>) -> (Vec<Term>, usize, usize) {
    let none = Pos::new(usize::MAX, usize::MAX);
    let mut terms: Vec<Term> = model.terms[w]
        .iter()
        .map(|&t| match t {
            Term::Port(p, d) => Term::Port(lift(p).unwrap_or(none), d),
            Term::Text(p, d) => Term::Text(lift(p).unwrap_or(none), d),
            Term::Arrow(_) => Term::Arrow(none),
            Term::Loose(_, d, blocked) => Term::Loose(none, d, blocked),
        })
        .collect();
    terms.sort();
    let wire = &model.diagram.wires[w];
    (terms, wire.tees.len(), wire.crossings.len())
}

/// The box and index within it of the port at `p`.
fn port_index(d: &Diagram, p: Pos) -> Option<(usize, usize)> {
    d.boxes
        .iter()
        .enumerate()
        .find_map(|(b, shape)| shape.ports.iter().position(|q| q.pos == p).map(|k| (b, k)))
}

fn corner(d: &Diagram, b: usize) -> Pos {
    let r = d.boxes[b].rect;
    Pos::new(r.top, r.left)
}

/// `p`, kept at the same offset from `to` as it is from `from`.
fn rebase(p: Pos, from: Pos, to: Pos) -> Option<Pos> {
    Some(Pos::new(
        p.row.checked_add(to.row)?.checked_sub(from.row)?,
        p.col.checked_add(to.col)?.checked_sub(from.col)?,
    ))
}

/// Why a wire on untidy text can't be named: formatting tells it apart.
pub(crate) const LOOK_ALIKE: &str = "format the sheet first: this wire looks like another one until then";

/// Why an edit's drawing can't be read back.
pub(crate) const STRAY: &str = "it would write a stray character";

/// The revision an edit returns for the text it drew, formatted afresh:
/// `drawn` itself when formatting changes nothing, otherwise the formatted
/// text, which must format to itself. `Err` says what's wrong.
///
/// Never an input's canonical revision: that one is seeded as canonical
/// without being formatted again ([`Doc::canonical`]), so it proves
/// nothing about a result.
pub(crate) fn tidy(drawn: Doc) -> Result<Doc, &'static str> {
    let formatted = drawn.formatted().ok_or(STRAY)?;
    if formatted == drawn.text() {
        return Ok(drawn);
    }
    let tidied = Doc::new(formatted.to_owned());
    match tidied.formatted() {
        Some(again) if again == tidied.text() => Ok(tidied),
        Some(_) => Err("formatting it again would change it"),
        None => Err(STRAY),
    }
}
