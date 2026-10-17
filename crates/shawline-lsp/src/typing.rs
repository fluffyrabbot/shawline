//! Transliteration as you type (SPEC §13), driven by
//! `textDocument/onTypeFormatting`.
//!
//! The client has already inserted the typed ASCII key when it asks; the
//! answer replaces that key, plus any still-pending letters before it, with
//! Shavian.

use shawline_word::{SEPARATOR, TABLE, Transliterator};

/// Every character that can start or continue a key, plus the separator.
/// The namer dot's key `=` is in [`TABLE`] already.
pub fn trigger_characters() -> Vec<String> {
    let mut chars: Vec<char> = TABLE.iter().flat_map(|(key, _)| key.chars()).collect();
    chars.push(SEPARATOR);
    chars.sort_unstable();
    chars.dedup();
    chars.into_iter().map(String::from).collect()
}

/// Replace character indices `start..end` of line `row` with `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replace {
    pub row: usize,
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// One document's transliteration state.
///
/// The transliterator's pending letters are only valid while they sit right
/// before the cursor. So the state remembers where its last edit left the
/// cursor, and resets whenever the next key arrives anywhere else.
#[derive(Debug, Default)]
pub struct Typing {
    translit: Transliterator,
    /// `(row, col)` where the next key must be typed to continue.
    expect: Option<(usize, usize)>,
    /// The previous key was `/`, which makes a following `v` a letter even
    /// at the start of a word (SPEC §13).
    after_separator: bool,
}

impl Typing {
    pub fn reset(&mut self) {
        self.translit.reset();
        self.expect = None;
        self.after_separator = false;
    }

    /// Handles `key`, just typed on raw line `row` (`line`), leaving the
    /// cursor at character index `cursor`.
    pub fn on_type(&mut self, line: &str, row: usize, cursor: usize, key: char) -> Option<Replace> {
        let chars: Vec<char> = line.chars().collect();
        let typed = cursor
            .checked_sub(1)
            .filter(|&i| chars.get(i) == Some(&key));
        let Some(typed) = typed else {
            self.reset();
            return None;
        };
        if self.expect != Some((row, typed)) {
            self.translit.reset();
            self.after_separator = false;
        }
        if key == 'v' && !self.after_separator && !extends_word(&chars[..typed]) {
            // Outside a word run `v` is the `▼` fallback, and it breaks
            // any pending letter.
            self.reset();
            return None;
        }
        let edit = self.translit.push(key);
        self.after_separator = key == SEPARATOR;
        let Some(start) = typed.checked_sub(edit.delete) else {
            self.reset();
            return None;
        };
        let end = typed + 1;
        let text = edit.insert;
        self.expect = Some((row, start + text.chars().count()));
        if chars[start..end].iter().copied().eq(text.chars()) {
            return None;
        }
        Some(Replace {
            row,
            start,
            end,
            text,
        })
    }
}

/// True if the character before the typed key belongs to a word run.
fn extends_word(before: &[char]) -> bool {
    before
        .last()
        .is_some_and(|&c| c == '·' || shawline_word::class(c).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Types `keys` at the end of `line` the way an editor would, applying
    /// each returned edit, and returns the final line.
    fn type_into(line: &str, keys: &str) -> String {
        let mut typing = Typing::default();
        let mut buf: Vec<char> = line.chars().collect();
        let mut cursor = buf.len();
        for key in keys.chars() {
            buf.insert(cursor, key);
            cursor += 1;
            let text: String = buf.iter().collect();
            if let Some(r) = typing.on_type(&text, 0, cursor, key) {
                buf.splice(r.start..r.end, r.text.chars());
                cursor = r.start + r.text.chars().count();
            }
        }
        buf.into_iter().collect()
    }

    #[test]
    fn sequences() {
        assert_eq!(type_into("", "th"), "𐑔");
        assert_eq!(type_into("", "t/h"), "𐑑𐑣");
        assert_eq!(type_into("│ ", "=dubul"), "│ ·𐑛𐑳𐑚𐑳𐑤");
        assert_eq!(type_into("", "air"), "𐑺");
        assert_eq!(type_into("𐑐", "v"), "𐑐𐑝");
        assert_eq!(type_into("", "lvl"), "𐑤𐑝𐑤");
    }

    #[test]
    fn v_outside_a_word_stays_an_arrowhead() {
        assert_eq!(type_into("──", "v"), "──v");
        assert_eq!(type_into("", "v"), "v");
        assert_eq!(type_into("𐑐 ", "v"), "𐑐 v");
    }

    #[test]
    fn separator_makes_v_a_letter_at_word_start() {
        assert_eq!(type_into("", "/v"), "𐑝");
        assert_eq!(type_into("𐑐 ", "/vow"), "𐑐 𐑝𐑬");
    }

    #[test]
    fn moving_the_cursor_resets() {
        let mut typing = Typing::default();
        let r = typing.on_type("t", 0, 1, 't').unwrap();
        assert_eq!(r.text, "𐑑");
        // `h` typed somewhere else must not merge with the pending `t`.
        let r = typing.on_type("𐑑 h", 0, 3, 'h').unwrap();
        assert_eq!((r.start, r.end, r.text.as_str()), (2, 3, "𐑣"));
        // Same column, different row: also a reset.
        let r = typing.on_type("h", 1, 1, 'h').unwrap();
        assert_eq!((r.start, r.text.as_str()), (0, "𐑣"));
    }

    #[test]
    fn mismatched_key_is_ignored() {
        let mut typing = Typing::default();
        assert_eq!(typing.on_type("x", 0, 1, 't'), None);
    }

    #[test]
    fn triggers_cover_keys_and_separator() {
        let t = trigger_characters();
        for c in ["t", "h", "@", "=", "/", "v"] {
            assert!(t.contains(&c.to_string()), "{c}");
        }
        assert!(!t.contains(&"x".to_string()));
    }
}
