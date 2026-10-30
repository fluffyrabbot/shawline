//! The `.repair` dump (PLAN P10, wave 3): how each repair `repairs` offers
//! ends, one line per repair in the order offered.
//!
//! - `ROW:COL CODE EDIT ok DIGEST`, or `ROW:COL CODE EDIT refused KIND AT`,
//!   for a repair with an edit, where `DIGEST` is the result's hash as in
//!   `.studio`;
//! - `ROW:COL CODE`, for a repair whose follow-up is all of it;
//! - either ends `then FOLLOWUP ROW:COL` when the repair has a follow-up.
//!
//! Labels are wording, so they are never recorded.

use std::fmt::Write;

use shawline_doc::Doc;
use shawline_edit::{Followup, apply, repairs};

use crate::studio::digest;

/// The `.repair` dump of `src`.
pub fn dump(src: &str) -> String {
    let doc = Doc::new(src);
    let mut out = String::new();
    for repair in repairs(&doc) {
        write!(out, "{} {}", repair.at, repair.code).unwrap();
        if let Some(edit) = &repair.edit {
            match apply(&doc, edit) {
                Ok(edited) => write!(out, " {edit} ok {}", digest(edited.doc.text())).unwrap(),
                Err(refusal) => write!(out, " {edit} refused {} {}", refusal.kind, refusal.at).unwrap(),
            }
        }
        match repair.then {
            Followup::None => {}
            Followup::NameField(at) | Followup::DrawWire(at) => {
                write!(out, " then {} {at}", repair.then.name()).unwrap();
            }
        }
        out.push('\n');
    }
    out
}
