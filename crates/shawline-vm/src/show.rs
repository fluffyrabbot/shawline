//! A value's printed form, cut short.

use crate::bytecode::Value;

/// See [`crate::show`].
///
/// Walks the same worklist as `Display`, but stops once one character more
/// than fits has been written. Every step writes at least one character, so
/// the walk is bounded by `max_chars` whatever the value's size or sharing,
/// and no memo is needed.
pub(crate) fn show(value: &Value, max_chars: usize) -> String {
    enum Task<'a> {
        Value(&'a Value),
        Str(&'static str),
        /// The rest of a list after an item: more items, or the `]`.
        Rest(&'a Value),
    }
    let mut out = Out { text: String::new(), len: 0, room: max_chars.saturating_add(1) };
    if let Value::Text(t) = value {
        out.push(t);
    } else {
        let mut tasks = vec![Task::Value(value)];
        while let Some(task) = tasks.pop() {
            if out.len >= out.room {
                break;
            }
            match task {
                Task::Str(s) => out.push(s),
                Task::Value(Value::Int(n)) => out.push(&n.to_string()),
                Task::Value(Value::Text(t)) => {
                    out.push("«");
                    out.push(t);
                    out.push("»");
                }
                Task::Value(Value::Pair(p)) => {
                    out.push("(");
                    tasks.extend([Task::Str(")"), Task::Value(&p.1), Task::Str(" "), Task::Value(&p.0)]);
                }
                Task::Value(Value::List(None)) => out.push("[]"),
                Task::Value(Value::List(Some(c))) => {
                    out.push("[");
                    tasks.extend([Task::Rest(&c.1), Task::Value(&c.0)]);
                }
                Task::Rest(Value::List(Some(c))) => {
                    out.push(" ");
                    tasks.extend([Task::Rest(&c.1), Task::Value(&c.0)]);
                }
                Task::Rest(_) => out.push("]"),
            }
        }
    }
    if out.len <= max_chars {
        return out.text;
    }
    let Some(keep) = max_chars.checked_sub(1) else {
        return String::new();
    };
    let cut = out.text.char_indices().nth(keep).map_or(out.text.len(), |(i, _)| i);
    out.text.truncate(cut);
    out.text.push('…');
    out.text
}

/// Text that takes at most `room` characters and drops the rest.
struct Out {
    text: String,
    /// Characters in `text`.
    len: usize,
    room: usize,
}

impl Out {
    fn push(&mut self, s: &str) {
        for ch in s.chars() {
            if self.len >= self.room {
                return;
            }
            self.text.push(ch);
            self.len += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::show;
    use crate::bytecode::Value;

    fn pair(a: Value, b: Value) -> Value {
        Value::Pair(Rc::new((a, b)))
    }

    #[test]
    fn short_values_print_whole() {
        let list = Value::List(Some(Rc::new((Value::Text("𐑣𐑦".into()), Value::List(None)))));
        let v = pair(Value::Int(-3), list);
        assert_eq!(show(&v, 100), "(-3 [«𐑣𐑦»])");
        assert_eq!(show(&v, 12), "(-3 [«𐑣𐑦»])");
        assert_eq!(show(&Value::Text("𐑣𐑦𐑤𐑴".into()), 4), "𐑣𐑦𐑤𐑴");
    }

    #[test]
    fn long_values_are_cut_with_an_ellipsis() {
        let list = Value::List(Some(Rc::new((Value::Text("𐑣𐑦".into()), Value::List(None)))));
        let v = pair(Value::Int(-3), list);
        assert_eq!(show(&v, 11), "(-3 [«𐑣𐑦»])");
        assert_eq!(show(&v, 10), "(-3 [«𐑣𐑦»…");
        assert_eq!(show(&v, 8), "(-3 [«𐑣…");
        assert_eq!(show(&v, 1), "…");
        assert_eq!(show(&v, 0), "");
        assert_eq!(show(&Value::Text("𐑣𐑦𐑤𐑴".into()), 3), "𐑣𐑦…");
        assert_eq!(show(&Value::Int(123456), 4), "123…");
    }

    #[test]
    fn a_vast_shared_value_is_cheap() {
        // 2^40 leaves in 40 cells.
        let dag = (0..40).fold(Value::Int(0), |x, _| pair(x.clone(), x));
        let shown = show(&dag, 30);
        assert_eq!(shown.chars().count(), 30);
        assert!(shown.starts_with("((((((") && shown.ends_with('…'), "{shown}");
        let long = (0..1_000_000).fold(Value::List(None), |t, n| Value::List(Some(Rc::new((Value::Int(n), t)))));
        assert_eq!(show(&long, 10), "[999999 9…");
    }
}
