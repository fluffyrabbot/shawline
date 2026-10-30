//! Transliteration as you type (SPEC §13), driven by
//! `textDocument/onTypeFormatting`.
//!
//! The client has already inserted the typed ASCII key when it asks; the
//! answer replaces that key, plus any still-pending letters before it, with
//! Shavian.
//!
//! [`convert`] applies the same rules to keys already in the text, such as
//! pasted ones, for the "Convert keys to Shavian" action.

use shawline_word::{SEPARATOR, TABLE, Transliterator};

/// Every character that can start or continue a key, the capitals of its
/// letters, and the separator. The namer dot's key `=` is in [`TABLE`]
/// already.
pub fn trigger_characters() -> Vec<String> {
    let mut chars: Vec<char> = TABLE.iter().flat_map(|(key, _)| key.chars()).collect();
    let capitals: Vec<char> = chars.iter().map(char::to_ascii_uppercase).collect();
    chars.extend(capitals);
    chars.push(SEPARATOR);
    chars.sort_unstable();
    chars.dedup();
    chars.into_iter().map(String::from).collect()
}

/// The key character `c` types: `c` itself, or for a capital A–Z its
/// lowercase letter, as Studio reads a typed capital. `None` when that is
/// neither part of a key nor the separator.
pub fn key_of(c: char) -> Option<char> {
    let key = c.to_ascii_lowercase();
    (key == SEPARATOR || TABLE.iter().any(|(k, _)| k.contains(key))).then_some(key)
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

    /// Handles `ch`, just typed on raw line `row` (`line`), leaving the
    /// cursor at character index `cursor`. A capital is typed as its
    /// lowercase key.
    pub fn on_type(&mut self, line: &str, row: usize, cursor: usize, ch: char) -> Option<Replace> {
        let chars: Vec<char> = line.chars().collect();
        let typed = cursor.checked_sub(1).filter(|&i| chars.get(i) == Some(&ch));
        let (Some(typed), Some(key)) = (typed, key_of(ch)) else {
            self.reset();
            return None;
        };
        if self.expect != Some((row, typed)) {
            self.translit.reset();
            self.after_separator = false;
        }
        let before = typed.checked_sub(1).map(|i| chars[i]);
        if key == 'v' && !self.after_separator && !extends_word(before) {
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

/// `text` with its keys transliterated as typing them in order there would
/// (SPEC §13): the longest match wins, `/` keeps keys apart and leaves
/// nothing, `=` is the namer dot, and a capital is its lowercase key. `v`
/// is 𐑝 only where it continues a word run or follows `/`; elsewhere it
/// stays the `▼` fallback. `before` is the character just before `text`,
/// which decides whether a leading `v` continues a word. Every other
/// character passes through, and settles the letter before it.
pub fn convert(before: Option<char>, text: &str) -> String {
    let mut out: Vec<char> = Vec::with_capacity(text.len());
    let mut translit = Transliterator::default();
    let mut after_separator = false;
    for c in text.chars() {
        let key = key_of(c).filter(|&key| {
            key != 'v' || after_separator || extends_word(out.last().copied().or(before))
        });
        let Some(key) = key else {
            translit.reset();
            after_separator = false;
            out.push(c);
            continue;
        };
        let edit = translit.push(key);
        after_separator = key == SEPARATOR;
        // The transliterator deletes only what it inserted since its reset.
        out.truncate(out.len() - edit.delete);
        out.extend(edit.insert.chars());
    }
    out.into_iter().collect()
}

/// True if a key typed after `before` continues a word run.
fn extends_word(before: Option<char>) -> bool {
    before.is_some_and(|c| c == '·' || shawline_word::class(c).is_some())
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
        for c in ["t", "h", "@", "=", "/", "v", "T", "H", "V", "C"] {
            assert!(t.contains(&c.to_string()), "{c}");
        }
        for c in ["x", "X", "q", "Q"] {
            assert!(!t.contains(&c.to_string()), "{c}");
        }
    }

    #[test]
    fn a_capital_types_its_lowercase_key() {
        assert_eq!(type_into("", "T"), "𐑑");
        assert_eq!(type_into("", "Th"), "𐑔");
        assert_eq!(type_into("", "tH"), "𐑔");
        assert_eq!(type_into("│ ", "=DUB@L"), "│ ·𐑛𐑳𐑚𐑩𐑤");
        assert_eq!(type_into("", "AIR"), "𐑺");
        // The `v` rule reads the lowercase key, and a capital `V` outside a
        // word is left alone, as `v` is.
        assert_eq!(type_into("𐑐", "V"), "𐑐𐑝");
        assert_eq!(type_into("──", "V"), "──V");
        assert_eq!(type_into("", "/V"), "𐑝");
        // A capital that types no key passes through and settles the letter
        // before it.
        assert_eq!(type_into("", "tX"), "𐑑X");
        assert_eq!(type_into("", "tXh"), "𐑑X𐑣");
    }

    #[test]
    fn key_of_reads_capitals_and_rejects_the_rest() {
        assert_eq!(key_of('T'), Some('t'));
        assert_eq!(key_of('t'), Some('t'));
        assert_eq!(key_of('@'), Some('@'));
        assert_eq!(key_of('/'), Some('/'));
        assert_eq!(key_of('C'), Some('c'));
        for c in ['x', 'X', 'Q', '1', ' ', '-', '𐑐', '·', 'É'] {
            assert_eq!(key_of(c), None, "{c}");
        }
    }

    #[test]
    fn convert_follows_the_typing_rules() {
        assert_eq!(convert(None, "th"), "𐑔");
        assert_eq!(convert(None, "t/h"), "𐑑𐑣");
        assert_eq!(convert(None, "=dub@l"), "·𐑛𐑳𐑚𐑩𐑤");
        assert_eq!(convert(None, "=Mayn"), "·𐑥𐑱𐑯");
        assert_eq!(convert(None, "lvl"), "𐑤𐑝𐑤");
        // `v` starts a word only after `/`, and stays a stroke elsewhere.
        assert_eq!(convert(None, "vow"), "v𐑬");
        assert_eq!(convert(None, "/vow"), "𐑝𐑬");
        assert_eq!(convert(None, "──v"), "──v");
        assert_eq!(convert(Some('𐑐'), "vow"), "𐑝𐑬");
        assert_eq!(convert(Some(' '), "vow"), "v𐑬");
        // Other characters pass through, and each settles the letter before it.
        assert_eq!(convert(None, "t h"), "𐑑 𐑣");
        assert_eq!(convert(None, "1 ──▶ p ├"), "1 ──▶ 𐑐 ├");
        assert_eq!(convert(None, "t\nh\nvow"), "𐑑\n𐑣\nv𐑬");
        assert_eq!(convert(None, "cat"), "c𐑨𐑑");
        assert_eq!(convert(None, "𐑐𐑑"), "𐑐𐑑");
        assert_eq!(convert(None, ""), "");
    }

    #[test]
    fn convert_matches_typing_the_same_keys() {
        for (line, keys) in [
            ("", "th t/h =dub@l lvl vow /vow"),
            ("", "Dubul THE aIr ar/r @r i@ eer ew"),
            ("𐑐", "vow"),
            ("𐑐 ", "vow"),
            ("── ", "v ▶ v1 tv x vx"),
            ("", "ch c ca chv cv ngng n/g"),
            ("·", "vee"),
        ] {
            let typed = type_into(line, keys);
            let converted = format!("{line}{}", convert(line.chars().last(), keys));
            assert_eq!(converted, typed, "{line:?} + {keys:?}");
        }
    }

    #[test]
    fn convert_inverts_keys() {
        let all: String = TABLE.iter().map(|(_, letter)| *letter).collect();
        for word in [
            "·𐑛𐑳𐑚𐑩𐑤",
            "𐑑𐑣",
            "𐑝𐑧𐑮𐑦",
            "·𐑝𐑧𐑮𐑦",
            "·𐑥𐑱𐑯",
            "𐑨𐑮",
            "𐑯𐑦𐑤",
            all.as_str(),
        ] {
            assert_eq!(convert(None, &shawline_word::keys(word)), word, "{word}");
        }
    }
}
