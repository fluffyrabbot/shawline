//! What the derived impls in `bytecode.rs` can't give a million-element list
//! or a value whose cells are shared: a drop, a structural equality and a
//! printed length that never recurse on the host stack, plus the count of
//! live cells and text letters behind the heap limit (SPEC §10).

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::mem;
use std::rc::Rc;

use crate::bytecode::Value;

impl Drop for Value {
    fn drop(&mut self) {
        match self {
            Value::Pair(cell) | Value::List(Some(cell)) if Rc::strong_count(cell) == 1 => {
                heap::freed(1);
            }
            Value::Text(t) if Rc::strong_count(t) == 1 => heap::freed(t.chars().count()),
            _ => {}
        }
        // The default drop recurses once per list cell; instead, detach the
        // children of every uniquely owned cell and drop them from a worklist.
        let mut pending = Vec::new();
        detach(self, &mut pending);
        while let Some(mut v) = pending.pop() {
            detach(&mut v, &mut pending);
        }
    }
}

/// Moves the compound children out of a cell `v` owns uniquely, so dropping
/// the cell itself goes no deeper. A shared cell only loses a reference.
fn detach(v: &mut Value, pending: &mut Vec<Value>) {
    let (Value::Pair(cell) | Value::List(Some(cell))) = v else {
        return;
    };
    if let Some((a, b)) = Rc::get_mut(cell) {
        for child in [a, b] {
            if matches!(child, Value::Pair(_) | Value::List(Some(_))) {
                pending.push(mem::replace(child, Value::Int(0)));
            }
        }
    }
}

/// The number of pairs, list cells and text letters alive during a run.
///
/// A run starts the count at zero; the VM counts each cell it makes and each
/// letter of each text it makes, and every cell or text freed while the run
/// lasts counts down. Only the run's own values can be freed during it: the
/// module's constants outlive it, and nothing from an earlier run can be
/// dropped until it returns. Outside a run nothing counts, so values that
/// outlive one never skew the next.
pub(crate) mod heap {
    use super::*;

    thread_local! {
        static LIVE: Cell<Option<usize>> = const { Cell::new(None) };
    }

    /// Counts until dropped.
    pub(crate) struct Run(());

    pub(crate) fn start() -> Run {
        LIVE.set(Some(0));
        Run(())
    }

    impl Drop for Run {
        fn drop(&mut self) {
            LIVE.set(None);
        }
    }

    pub(crate) fn live() -> usize {
        LIVE.get().unwrap_or(0)
    }

    /// A new cell, counted.
    pub(crate) fn cell(a: Value, b: Value) -> Rc<(Value, Value)> {
        made(1);
        Rc::new((a, b))
    }

    /// A new text of `letters` letters, each counted.
    pub(crate) fn text(t: &str, letters: usize) -> Rc<str> {
        made(letters);
        t.into()
    }

    fn made(count: usize) {
        if let Some(n) = LIVE.get() {
            LIVE.set(Some(n + count));
        }
    }

    pub(super) fn freed(count: usize) {
        if let Some(n) = LIVE.get() {
            LIVE.set(Some(n.saturating_sub(count)));
        }
    }
}

/// The address of a cell, as a memo key.
fn addr(cell: &Rc<(Value, Value)>) -> *const (Value, Value) {
    Rc::as_ptr(cell)
}

/// Structural equality, the meaning of `𐑦` and of switch guards.
///
/// Prefer this to `==`, whose derived implementation recurses once per list
/// cell and so overflows the host stack on long lists. Pairs of shared cells
/// are compared once, so values that share structure stay polynomial.
pub fn equal(a: &Value, b: &Value) -> bool {
    // Switch guards mostly compare scalars; skip the worklist allocation.
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => return x == y,
        (Value::Text(x), Value::Text(y)) => return x == y,
        _ => {}
    }
    // A cell with one reference is reached at most once per visit of its
    // parent, so only pairs involving a shared cell can recur.
    let mut seen = HashSet::new();
    let mut pending = vec![(a, b)];
    while let Some(pair) = pending.pop() {
        match pair {
            (Value::Int(x), Value::Int(y)) if x == y => {}
            (Value::Text(x), Value::Text(y)) if x == y => {}
            (Value::List(None), Value::List(None)) => {}
            (Value::Pair(x), Value::Pair(y)) | (Value::List(Some(x)), Value::List(Some(y))) => {
                if Rc::ptr_eq(x, y) {
                    continue;
                }
                let shared = Rc::strong_count(x) > 1 || Rc::strong_count(y) > 1;
                if shared && !seen.insert((addr(x), addr(y))) {
                    continue;
                }
                pending.push((&x.1, &y.1));
                pending.push((&x.0, &y.0));
            }
            _ => return false,
        }
    }
    true
}

/// The length in characters of `v` as `Display` prints it, or `None` past
/// `limit`. Shared cells are measured once, and no string is built, so a
/// value of a few cells that would print astronomically large is cheap.
pub(crate) fn printed_len(v: &Value, limit: usize) -> Option<usize> {
    if let Value::Text(t) = v {
        return Some(t.chars().count()).filter(|&n| n <= limit);
    }
    enum Step<'a> {
        Enter(&'a Value),
        /// A pair's two lengths are on the stack.
        Pair(&'a Rc<(Value, Value)>),
        /// Measures a list cell: its item and every later one, with the spaces
        /// between them but not the brackets.
        Cell(&'a Rc<(Value, Value)>),
        /// The cell's item length is on the stack, then its rest's, if any.
        CellDone(&'a Rc<(Value, Value)>),
        /// Adds a list's brackets to its first cell's length.
        Brackets,
    }
    // Pairs and list cells are keyed apart, though the VM never makes one
    // cell serve as both. A cell with one reference is measured at most once
    // per measurement of its parent, so only shared cells are memoized.
    let mut memo: HashMap<(*const (Value, Value), bool), usize> = HashMap::new();
    let mut lens: Vec<usize> = Vec::new();
    let mut steps = vec![Step::Enter(v)];
    let pop = |lens: &mut Vec<usize>| lens.pop().expect("a measured part");
    while let Some(step) = steps.pop() {
        // Every length measured is a part of the whole, so any one past the
        // limit settles it.
        let n = match step {
            Step::Enter(Value::Int(n)) => n.to_string().len(),
            Step::Enter(Value::Text(t)) => t.chars().count() + 2,
            Step::Enter(Value::List(None)) => 2,
            Step::Enter(Value::List(Some(c))) => {
                steps.push(Step::Brackets);
                steps.push(Step::Cell(c));
                continue;
            }
            Step::Enter(Value::Pair(c)) => match memo.get(&(addr(c), false)) {
                Some(&n) => n,
                None => {
                    steps.push(Step::Pair(c));
                    steps.push(Step::Enter(&c.1));
                    steps.push(Step::Enter(&c.0));
                    continue;
                }
            },
            Step::Pair(c) => {
                let b = pop(&mut lens);
                let a = pop(&mut lens);
                let n = a + b + 3;
                if Rc::strong_count(c) > 1 {
                    memo.insert((addr(c), false), n);
                }
                n
            }
            Step::Cell(c) => match memo.get(&(addr(c), true)) {
                Some(&n) => n,
                None => {
                    steps.push(Step::CellDone(c));
                    if let Value::List(Some(next)) = &c.1 {
                        steps.push(Step::Cell(next));
                    }
                    steps.push(Step::Enter(&c.0));
                    continue;
                }
            },
            Step::CellDone(c) => {
                let rest = match &c.1 {
                    Value::List(Some(_)) => pop(&mut lens) + 1,
                    _ => 0,
                };
                let n = pop(&mut lens) + rest;
                if Rc::strong_count(c) > 1 {
                    memo.insert((addr(c), true), n);
                }
                n
            }
            Step::Brackets => pop(&mut lens) + 2,
        };
        if n > limit {
            return None;
        }
        lens.push(n);
    }
    lens.pop()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(a: Value, b: Value) -> Value {
        Value::Pair(Rc::new((a, b)))
    }

    fn list(items: impl DoubleEndedIterator<Item = Value>) -> Value {
        items.rev().fold(Value::List(None), |tail, head| Value::List(Some(Rc::new((head, tail)))))
    }

    /// `x` paired with itself `n` times: n cells, 2^n leaves.
    fn dag(n: u32) -> Value {
        (0..n).fold(Value::Int(0), |x, _| pair(x.clone(), x))
    }

    #[test]
    fn deep_values_print_and_drop_without_recursing() {
        let deep = (0..1_000_000).fold(Value::Int(0), |acc, _| pair(acc, Value::Int(7)));
        let shown = deep.to_string();
        assert_eq!(shown.len(), 1 + 4 * 1_000_000);
        assert!(shown.trim_start_matches('(').starts_with("0 7) 7) 7)"));
        assert_eq!(printed_len(&deep, usize::MAX), Some(shown.len()));
        let long = list((1..=1_000_000).map(Value::Int));
        let shown = long.to_string();
        assert!(shown.starts_with("[1 2 3 ") && shown.ends_with(" 999999 1000000]"));
        assert_eq!(printed_len(&long, usize::MAX), Some(shown.len()));
    }

    #[test]
    fn printed_len_matches_display() {
        let shared = list([Value::Int(-12), Value::Text("𐑣𐑦".into())].into_iter());
        let values = [
            Value::Text("𐑣𐑦 𐑞𐑺".into()),
            Value::List(None),
            pair(shared.clone(), list([shared.clone(), Value::List(None)].into_iter())),
            pair(Value::Text("".into()), dag(5)),
        ];
        for v in values {
            assert_eq!(printed_len(&v, usize::MAX), Some(v.to_string().chars().count()), "{v}");
        }
        assert_eq!(printed_len(&dag(4), 15), None);
    }

    #[test]
    fn shared_structure_stays_cheap() {
        // 2^60 leaves each: exponential without memoizing on cells.
        assert!(equal(&dag(60), &dag(60)));
        assert!(!equal(&dag(60), &dag(59)));
        assert_eq!(printed_len(&dag(60), crate::MAX_OUTPUT), None);
    }
}
