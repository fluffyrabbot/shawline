//! Code actions: quick fixes from `shawline_edit::repairs` (PLAN P10,
//! "Repairs"), and the edit that turns one text into another.
//!
//! The language server can't open a name field or draw a wire, so a repair
//! is offered only for its `edit`, and its `then` is ignored.

use lsp_types::{Position, Range, TextEdit};
use shawline_doc::Doc;
use shawline_edit::{Repair, apply};
use shawline_grid::Diagnostic;

use crate::position::{self, Encoding};

/// A repair that can be offered as a quick fix.
#[derive(Debug)]
pub struct Fix<'a> {
    /// The diagnostic it fixes, one of the sheet's.
    pub diagnostic: &'a Diagnostic,
    /// The repair's label.
    pub title: String,
    /// The sheet's text once the repair's edit is applied.
    pub text: String,
    /// It is the only fix offered for its diagnostic.
    pub preferred: bool,
}

/// The fixes `repairs` offer for those of `doc`'s diagnostics that `wanted`
/// picks, in the order of `repairs`. A repair is left out when it has no
/// edit, when its edit is refused (`repairs` offers none such, so this is
/// a guard), or when it names no diagnostic of `doc`'s by cell and code.
///
/// Repairs come in diagnostic order, at most one each, so a repair is for
/// the first diagnostic of its cell and code that no earlier repair took.
/// Both ends of a one-cell wire are then two `Y003`s with a fix each.
pub fn fixes<'a>(
    doc: &'a Doc,
    repairs: &[Repair],
    wanted: impl Fn(&Diagnostic) -> bool,
) -> Vec<Fix<'a>> {
    let diagnostics = doc.diagnostics();
    let mut taken = vec![false; diagnostics.len()];
    let mut found: Vec<(usize, Fix<'a>)> = Vec::new();
    for repair in repairs {
        let fixes =
            |i: &usize| diagnostics[*i].at == repair.at && diagnostics[*i].code == repair.code;
        let mut matching = (0..diagnostics.len()).filter(fixes);
        let Some(first) = matching.clone().next() else {
            continue;
        };
        let index = matching.find(|&i| !taken[i]).unwrap_or(first);
        taken[index] = true;
        let Some(edit) = &repair.edit else { continue };
        if !wanted(&diagnostics[index]) {
            continue;
        }
        let Ok(edited) = apply(doc, edit) else {
            continue;
        };
        let fix = Fix {
            diagnostic: &diagnostics[index],
            title: repair.label.clone(),
            text: edited.doc.text().to_owned(),
            preferred: false,
        };
        found.push((index, fix));
    }
    let shared = |index: usize| found.iter().filter(|(i, _)| *i == index).count();
    let preferred: Vec<bool> = found.iter().map(|(index, _)| shared(*index) == 1).collect();
    found
        .into_iter()
        .zip(preferred)
        .map(|((_, fix), preferred)| Fix { preferred, ..fix })
        .collect()
}

/// The one edit that turns `old` into `new`. It replaces the whole lines
/// between those the two texts share at their start and at their end, so
/// the lines around a fix keep their place.
pub fn replacement(old: &str, new: &str, enc: Encoding) -> TextEdit {
    let old_lines: Vec<&str> = old.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = new.split_inclusive('\n').collect();
    let first = old_lines
        .iter()
        .zip(&new_lines)
        .take_while(|(a, b)| a == b)
        .count();
    let last = old_lines[first..]
        .iter()
        .rev()
        .zip(new_lines[first..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    // The start of line `row` of `old`, or its end when there is no such line.
    let start_of = |row: usize| {
        if row < old_lines.len() {
            Position::new(row as u32, 0)
        } else {
            position::end_of(old, enc)
        }
    };
    let range = Range::new(start_of(first), start_of(old_lines.len() - last));
    TextEdit::new(range, new_lines[first..new_lines.len() - last].concat())
}

/// True if two ranges overlap or touch: an empty range at a diagnostic's
/// edge is on it, as the cursor is.
pub fn touches(a: Range, b: Range) -> bool {
    a.start <= b.end && b.start <= a.end
}

#[cfg(test)]
mod tests {
    use super::*;
    use shawline_edit::{Edit, Followup};
    use shawline_grid::Pos;

    fn corpus(name: &str) -> String {
        let path = format!("{}/../../corpus/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    /// `old` with `edit` applied.
    fn applied(old: &str, enc: Encoding, edit: &TextEdit) -> String {
        let offset = |pos: Position| {
            let selected = position::selected(old, enc, Range::new(Position::new(0, 0), pos));
            selected.text.len()
        };
        let (start, end) = (offset(edit.range.start), offset(edit.range.end));
        format!("{}{}{}", &old[..start], edit.new_text, &old[end..])
    }

    fn repair(at: Pos, code: &'static str, label: &str, edit: Option<Edit>) -> Repair {
        Repair {
            at,
            code,
            label: label.into(),
            edit,
            then: Followup::None,
        }
    }

    #[test]
    fn replacement_keeps_common_lines() {
        let enc = Encoding::Utf16;
        let cases = [
            (
                "a\nb\nc\n",
                "a\nB\nc\n",
                Range::new(Position::new(1, 0), Position::new(2, 0)),
                "B\n",
            ),
            (
                "a\nb\n",
                "a\nb\nc\n",
                Range::new(Position::new(2, 0), Position::new(2, 0)),
                "c\n",
            ),
            (
                "a\nb",
                "a\nb\n",
                Range::new(Position::new(1, 0), Position::new(1, 1)),
                "b\n",
            ),
            (
                "a\nb\nc",
                "a\nc",
                Range::new(Position::new(1, 0), Position::new(2, 0)),
                "",
            ),
            (
                "𐑐\n",
                "𐑐\n",
                Range::new(Position::new(1, 0), Position::new(1, 0)),
                "",
            ),
            (
                "",
                "𐑐",
                Range::new(Position::new(0, 0), Position::new(0, 0)),
                "𐑐",
            ),
            (
                "x\r\ny\r\n",
                "x\r\nz\r\n",
                Range::new(Position::new(1, 0), Position::new(2, 0)),
                "z\r\n",
            ),
            (
                "𐑐 𐑑",
                "𐑐 𐑒",
                Range::new(Position::new(0, 0), Position::new(0, 5)),
                "𐑐 𐑒",
            ),
        ];
        for (old, new, range, text) in cases {
            let edit = replacement(old, new, enc);
            assert_eq!(
                (edit.range, edit.new_text.as_str()),
                (range, text),
                "{old:?} → {new:?}"
            );
            assert_eq!(applied(old, enc, &edit), new, "{old:?} → {new:?}");
        }
    }

    #[test]
    fn replacement_of_a_whole_sheet_applies() {
        let old = corpus("03-double-ascii.shaw");
        let new = corpus("03-double-ascii.fmt.shaw");
        for enc in [Encoding::Utf16, Encoding::Utf32] {
            assert_eq!(applied(&old, enc, &replacement(&old, &new, enc)), new);
        }
    }

    #[test]
    fn a_repair_with_an_edit_is_a_preferred_fix() {
        let doc = Doc::new(corpus("static/12-unused-input.shaw"));
        let at = Pos::new(6, 0);
        let repairs = [repair(
            at,
            "W002",
            "Remove the unused input",
            Some(Edit::RemovePort(at)),
        )];
        let fixes = fixes(&doc, &repairs, |_| true);
        let [fix] = fixes.as_slice() else {
            panic!("{fixes:?}")
        };
        assert_eq!((fix.diagnostic.at, fix.diagnostic.code), (at, "W002"));
        assert_eq!(fix.title, "Remove the unused input");
        assert!(fix.preferred);
        let after = Doc::new(fix.text.as_str());
        assert!(
            after.diagnostics().iter().all(|d| d.code != "W002"),
            "{}",
            fix.text
        );
    }

    #[test]
    fn repairs_without_an_applicable_edit_are_left_out() {
        let doc = Doc::new(corpus("static/12-unused-input.shaw"));
        let at = Pos::new(6, 0);
        let repairs = [
            // All follow-up.
            repair(at, "W002", "Fix…: name it", None),
            // Refused: there is no port at 0:0.
            repair(
                at,
                "W002",
                "Remove a port",
                Some(Edit::RemovePort(Pos::new(0, 0))),
            ),
            // No such diagnostic, by cell and then by code.
            repair(Pos::new(6, 1), "W002", "Remove", Some(Edit::RemovePort(at))),
            repair(at, "W001", "Remove", Some(Edit::RemovePort(at))),
        ];
        assert!(fixes(&doc, &repairs, |_| true).is_empty());
        // A diagnostic the caller doesn't want offers nothing.
        let repairs = [repair(
            at,
            "W002",
            "Remove the unused input",
            Some(Edit::RemovePort(at)),
        )];
        assert!(fixes(&doc, &repairs, |d| d.at != at).is_empty());
    }

    #[test]
    fn two_fixes_for_one_diagnostic_are_not_preferred() {
        let doc = Doc::new(corpus("static/12-unused-input.shaw"));
        let at = Pos::new(6, 0);
        let edit = Some(Edit::RemovePort(at));
        let repairs = [
            repair(at, "W002", "One", edit.clone()),
            repair(at, "W002", "Two", edit),
        ];
        let fixes = fixes(&doc, &repairs, |_| true);
        let titles: Vec<(&str, bool)> = fixes
            .iter()
            .map(|f| (f.title.as_str(), f.preferred))
            .collect();
        assert_eq!(titles, [("One", false), ("Two", false)]);
    }

    #[test]
    fn twin_diagnostics_get_a_fix_each() {
        // A one-cell wire at 7:24 has two dangling ends: two `Y003`s there.
        let doc = Doc::new(corpus("fix/03-too-far.shaw"));
        let at = Pos::new(7, 24);
        let fixes = fixes(&doc, &shawline_edit::repairs(&doc), |d| d.at == at);
        let [one, two] = fixes.as_slice() else {
            panic!("{fixes:?}")
        };
        assert_eq!((one.diagnostic.code, two.diagnostic.code), ("Y003", "Y003"));
        assert!(!std::ptr::eq(one.diagnostic, two.diagnostic));
        assert!(one.preferred && two.preferred);
    }

    #[test]
    fn touching_ranges() {
        let r = |a: u32, b: u32| Range::new(Position::new(0, a), Position::new(0, b));
        assert!(touches(r(2, 4), r(3, 3)));
        assert!(touches(r(2, 4), r(2, 2)));
        assert!(touches(r(2, 4), r(4, 4)));
        assert!(touches(r(2, 4), r(0, 9)));
        assert!(!touches(r(2, 4), r(5, 5)));
        assert!(!touches(r(2, 4), r(0, 1)));
    }
}
