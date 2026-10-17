//! What the derived impls in `bytecode.rs` can't give a million-element list:
//! a drop and a structural equality that never recurse on the host stack.

use std::mem;
use std::rc::Rc;

use crate::bytecode::Value;

impl Drop for Value {
    fn drop(&mut self) {
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

/// Structural equality, the meaning of `𐑦` and of switch guards.
///
/// Prefer this to `==`, whose derived implementation recurses once per list
/// cell and so overflows the host stack on long lists.
pub fn equal(a: &Value, b: &Value) -> bool {
    // Switch guards mostly compare scalars; skip the worklist allocation.
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => return x == y,
        (Value::Text(x), Value::Text(y)) => return x == y,
        _ => {}
    }
    let mut pending = vec![(a, b)];
    while let Some(pair) = pending.pop() {
        match pair {
            (Value::Int(x), Value::Int(y)) if x == y => {}
            (Value::Text(x), Value::Text(y)) if x == y => {}
            (Value::List(None), Value::List(None)) => {}
            (Value::Pair(x), Value::Pair(y)) | (Value::List(Some(x)), Value::List(Some(y))) => {
                if !Rc::ptr_eq(x, y) {
                    pending.push((&x.1, &y.1));
                    pending.push((&x.0, &y.0));
                }
            }
            _ => return false,
        }
    }
    true
}
