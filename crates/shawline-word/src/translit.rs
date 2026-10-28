//! Phonemic ASCII transliteration. The table and the longest-match rule are
//! documented at the crate root.

/// Key sequence → letter, for all 48 Shavian letters and the namer dot. The
/// mapping is one-to-one. Rendered as a table in the crate docs.
pub const TABLE: &[(&str, char)] = &[
    ("p", '𐑐'),   // peep
    ("t", '𐑑'),   // tot
    ("k", '𐑒'),   // kick
    ("f", '𐑓'),   // fee
    ("th", '𐑔'),  // thigh
    ("s", '𐑕'),   // so
    ("sh", '𐑖'),  // sure
    ("ch", '𐑗'),  // church
    ("y", '𐑘'),   // yea
    ("ng", '𐑙'),  // hung
    ("b", '𐑚'),   // bib
    ("d", '𐑛'),   // dead
    ("g", '𐑜'),   // gag
    ("v", '𐑝'),   // vow
    ("dh", '𐑞'),  // they
    ("z", '𐑟'),   // zoo
    ("zh", '𐑠'),  // measure
    ("j", '𐑡'),   // judge
    ("w", '𐑢'),   // woe
    ("h", '𐑣'),   // ha-ha
    ("l", '𐑤'),   // loll
    ("m", '𐑥'),   // mime
    ("i", '𐑦'),   // if
    ("e", '𐑧'),   // egg
    ("a", '𐑨'),   // ash
    ("@", '𐑩'),   // ado
    ("o", '𐑪'),   // on
    ("uu", '𐑫'),  // wool
    ("ow", '𐑬'),  // out
    ("ah", '𐑭'),  // ah
    ("r", '𐑮'),   // roar
    ("n", '𐑯'),   // nun
    ("ee", '𐑰'),  // eat
    ("ay", '𐑱'),  // age
    ("ie", '𐑲'),  // ice
    ("u", '𐑳'),   // up
    ("oh", '𐑴'),  // oak
    ("oo", '𐑵'),  // ooze
    ("oy", '𐑶'),  // oil
    ("aw", '𐑷'),  // awe
    ("ar", '𐑸'),  // are
    ("or", '𐑹'),  // or
    ("air", '𐑺'), // air
    ("ur", '𐑻'),  // err
    ("@r", '𐑼'),  // array
    ("eer", '𐑽'), // ear
    ("i@", '𐑾'),  // ian
    ("ew", '𐑿'),  // yew
    ("=", '·'),   // namer dot
];

/// Each letter's traditional name, a word it sounds in (SPEC §13): 𐑐 is
/// "peep". The key panel shows it beside the keys.
const LETTER_NAMES: &[(char, &str)] = &[
    ('𐑐', "peep"), ('𐑑', "tot"), ('𐑒', "kick"), ('𐑓', "fee"), ('𐑔', "thigh"), ('𐑕', "so"),
    ('𐑖', "sure"), ('𐑗', "church"), ('𐑘', "yea"), ('𐑙', "hung"), ('𐑚', "bib"), ('𐑛', "dead"),
    ('𐑜', "gag"), ('𐑝', "vow"), ('𐑞', "they"), ('𐑟', "zoo"), ('𐑠', "measure"), ('𐑡', "judge"),
    ('𐑢', "woe"), ('𐑣', "ha-ha"), ('𐑤', "loll"), ('𐑥', "mime"), ('𐑦', "if"), ('𐑧', "egg"),
    ('𐑨', "ash"), ('𐑩', "ado"), ('𐑪', "on"), ('𐑫', "wool"), ('𐑬', "out"), ('𐑭', "ah"),
    ('𐑮', "roar"), ('𐑯', "nun"), ('𐑰', "eat"), ('𐑱', "age"), ('𐑲', "ice"), ('𐑳', "up"),
    ('𐑴', "oak"), ('𐑵', "ooze"), ('𐑶', "oil"), ('𐑷', "awe"), ('𐑸', "are"), ('𐑹', "or"),
    ('𐑺', "air"), ('𐑻', "err"), ('𐑼', "array"), ('𐑽', "ear"), ('𐑾', "ian"), ('𐑿', "yew"),
    ('·', "namer dot"),
];

/// `letter`'s traditional name (SPEC §13), or `None` for a character that
/// isn't in [`TABLE`].
pub fn letter_name(letter: char) -> Option<&'static str> {
    LETTER_NAMES.iter().find(|&&(c, _)| c == letter).map(|&(_, name)| name)
}

/// Typed between keys to stop them combining (`t/h` is 𐑑𐑣). It produces
/// no output.
pub const SEPARATOR: char = '/';

/// What an editor should do after a key press: delete `delete` characters
/// (code points) immediately before the cursor, all of them previously
/// inserted by the same [`Transliterator`], then insert `insert`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Edit {
    /// Characters to delete before the cursor.
    pub delete: usize,
    /// Text to insert at the cursor after deleting.
    pub insert: String,
}

/// Incremental transliterator for editors. After every [`push`], the text it
/// has produced equals [`transliterate`] of every key pushed so far (since
/// the last [`reset`]), so the user always sees the longest-match reading and
/// it is revised as more keys arrive: `t` shows 𐑑, then `h` turns it into 𐑔.
///
/// [`push`]: Transliterator::push
/// [`reset`]: Transliterator::reset
#[derive(Debug, Clone, Default)]
pub struct Transliterator {
    /// Keys whose reading may still change, because together they are a
    /// prefix of some longer table key.
    pending: String,
    /// How `pending` is currently shown: `transliterate(&pending)`.
    shown: String,
}

impl Transliterator {
    /// A transliterator with nothing pending.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one typed key and returns the edit that brings the editor's text
    /// up to date.
    pub fn push(&mut self, key: char) -> Edit {
        if key == SEPARATOR {
            self.reset();
            return Edit::default();
        }
        self.pending.push(key);
        let mut text = String::new();
        // The first pending letter is settled once no table key could still
        // extend the pending keys.
        while !self.pending.is_empty() && !extends_to_key(&self.pending) {
            let (len, out) = munch(&self.pending);
            text.push(out);
            self.pending.drain(..len);
        }
        let rest = transliterate(&self.pending);
        text.push_str(&rest);

        let common = self
            .shown
            .chars()
            .zip(text.chars())
            .take_while(|(a, b)| a == b)
            .count();
        let edit = Edit {
            delete: self.shown.chars().count() - common,
            insert: text.chars().skip(common).collect(),
        };
        self.shown = rest;
        edit
    }

    /// Settles whatever is pending, so the next key starts a new letter. Call
    /// it when the cursor moves away; the text already produced stays as is.
    pub fn reset(&mut self) {
        self.pending.clear();
        self.shown.clear();
    }
}

/// Transliterates a whole string by the longest-match rule: each letter
/// takes the longest table key that fits, [`SEPARATOR`] is dropped, and any
/// other character passes through unchanged.
pub fn transliterate(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(c) = rest.chars().next() {
        if c == SEPARATOR {
            rest = &rest[c.len_utf8()..];
            continue;
        }
        let (len, letter) = munch(rest);
        out.push(letter);
        rest = &rest[len..];
    }
    out
}

/// One longest-match step on non-empty `s`: the byte length consumed and the
/// character produced. A character that starts no key passes through.
pub(crate) fn munch(s: &str) -> (usize, char) {
    TABLE
        .iter()
        .filter(|(key, _)| s.starts_with(key))
        .max_by_key(|(key, _)| key.len())
        .map(|&(key, letter)| (key.len(), letter))
        .unwrap_or_else(|| {
            let c = s.chars().next().expect("munch on empty input");
            (c.len_utf8(), c)
        })
}

/// Whether `keys` is a prefix of (or equal to) some table key.
fn extends_to_key(keys: &str) -> bool {
    TABLE.iter().any(|(key, _)| key.starts_with(keys))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Simulates an editor: applies each edit to a buffer and checks, after
    /// every key, that the buffer reads as the batch transliteration so far.
    fn type_keys(input: &str) -> String {
        let mut t = Transliterator::new();
        let mut buf: Vec<char> = Vec::new();
        for (i, key) in input.char_indices() {
            let edit = t.push(key);
            assert!(edit.delete <= buf.len(), "{input:?}: deletes past start");
            buf.truncate(buf.len() - edit.delete);
            buf.extend(edit.insert.chars());
            let typed = &input[..i + key.len_utf8()];
            let got: String = buf.iter().collect();
            assert_eq!(got, transliterate(typed), "after typing {typed:?}");
        }
        buf.into_iter().collect()
    }

    #[test]
    fn table_is_one_to_one_and_complete() {
        assert_eq!(TABLE.len(), 49);
        let keys: HashSet<_> = TABLE.iter().map(|(k, _)| *k).collect();
        let letters: HashSet<_> = TABLE.iter().map(|(_, c)| *c).collect();
        assert_eq!(keys.len(), TABLE.len(), "duplicate key");
        assert_eq!(letters.len(), TABLE.len(), "duplicate letter");
        let expected: HashSet<char> = (0x10450..=0x1047F)
            .filter_map(char::from_u32)
            .chain(['·'])
            .collect();
        assert_eq!(letters, expected);
    }

    #[test]
    fn keys_avoid_the_repertoire_except_v() {
        // Strokes, digits and marks must pass through untouched, so keys use
        // only lowercase letters, `@` and `=`; `v` is the one stroke among them.
        for (key, _) in TABLE {
            assert!(!key.is_empty() && key.len() <= 3, "{key:?}");
            for c in key.chars() {
                assert!(c.is_ascii_lowercase() || c == '@' || c == '=', "{key:?}");
                assert_ne!(c, SEPARATOR);
            }
        }
        assert_eq!(
            transliterate("+-.'|:~<>^ 0123456789«»※"),
            "+-.'|:~<>^ 0123456789«»※"
        );
    }

    #[test]
    fn every_key_round_trips() {
        for &(key, letter) in TABLE {
            assert_eq!(transliterate(key), letter.to_string(), "{key:?}");
            assert_eq!(type_keys(key), letter.to_string(), "{key:?}");
        }
        let all_keys = TABLE.iter().map(|(k, _)| *k).collect::<Vec<_>>().join("/");
        let all_letters: String = TABLE.iter().map(|(_, c)| *c).collect();
        assert_eq!(transliterate(&all_keys), all_letters);
        assert_eq!(type_keys(&all_keys), all_letters);
    }

    #[test]
    fn longest_match_wins_and_separator_splits() {
        let cases = [
            ("th", "𐑔"),
            ("t/h", "𐑑𐑣"),
            ("ng", "𐑙"),
            ("n/g", "𐑯𐑜"),
            ("ar", "𐑸"),
            ("a/r", "𐑨𐑮"),
            ("air", "𐑺"),
            ("ai", "𐑨𐑦"),
            ("aie", "𐑨𐑲"),
            ("eer", "𐑽"),
            ("ee/r", "𐑰𐑮"),
            ("very", "𐑝𐑧𐑮𐑘"),
            ("veri", "𐑝𐑧𐑮𐑦"),
            ("=shaw", "·𐑖𐑷"),
            ("p//b", "𐑐𐑚"),
            ("c", "c"),
            ("cat", "c𐑨𐑑"),
        ];
        for (keys, want) in cases {
            assert_eq!(transliterate(keys), want, "{keys:?}");
            assert_eq!(type_keys(keys), want, "{keys:?}");
        }
    }

    #[test]
    fn edits_revise_the_pending_letter() {
        let mut t = Transliterator::new();
        let e = |delete, insert: &str| Edit {
            delete,
            insert: insert.into(),
        };
        assert_eq!(t.push('t'), e(0, "𐑑"));
        assert_eq!(t.push('h'), e(1, "𐑔"));
        assert_eq!(t.push('a'), e(0, "𐑨"));
        assert_eq!(t.push('i'), e(0, "𐑦"));
        assert_eq!(t.push('r'), e(2, "𐑺"));
        assert_eq!(t.push(SEPARATOR), e(0, ""));
        assert_eq!(t.push('c'), e(0, "c"));
        assert_eq!(t.push('h'), e(1, "𐑗"));
        assert_eq!(t.push(' '), e(0, " "));
    }

    #[test]
    fn reset_settles_pending_keys() {
        let mut t = Transliterator::new();
        t.push('t');
        t.reset();
        assert_eq!(t.push('h').delete, 0);
    }

    #[test]
    fn incremental_matches_batch_on_mixed_input() {
        for input in [
            "╭─ =fib ─╮\n│ n ├──▶ 2 «hel@w» ※ ie",
            "ngg@r ch/h sh/h zh i@r ewr uur ooh",
            "+--.'-v aww oyy x q C",
        ] {
            assert_eq!(type_keys(input), transliterate(input), "{input:?}");
        }
    }

    #[test]
    fn crate_docs_table_matches() {
        let docs = include_str!("lib.rs");
        let mut documented = Vec::new();
        for line in docs.lines().filter_map(|l| l.strip_prefix("//! | ")) {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            for pair in cells.chunks(2) {
                let [letter_cell, key_cell] = pair else {
                    continue;
                };
                let (Some(letter), Some(key)) = (
                    letter_cell.chars().next(),
                    key_cell.strip_prefix('`').and_then(|k| k.strip_suffix('`')),
                ) else {
                    continue;
                };
                if let Some(n) = crate::name(letter) {
                    assert!(letter_cell.ends_with(n), "{letter_cell:?}");
                }
                documented.push((key, letter));
            }
        }
        let mut table = TABLE.to_vec();
        table.sort();
        documented.sort();
        assert_eq!(documented, table);
    }
}
