//! Shavian lexicon: letter classes (tall, deep, short, compound), mirror
//! pairs, letter names, and the ASCII phonemic transliteration used for
//! typing Shawline (SPEC §4, §13).
//!
//! # Transliteration
//!
//! Each Shavian letter, and the namer dot, has one ASCII key sequence. The
//! keys follow the sounds, spelled the way English pronunciation respellings
//! spell them (Wikipedia's respelling key: `ee` as in *see*, `ay` as in *day*,
//! `oh`, `oo`, `ow`, `oy`, `aw`, `ar`, `or`, `air`, `ur`, `eer`, `ew`), with
//! X-SAMPA's `@` for the schwa. Mirror pairs read as voiceless/voiced key
//! pairs (`p`/`b`, `th`/`dh`, `sh`/`zh`, `ch`/`j`), and the compounds spell
//! their parts (`@r` is 𐑼, `i@` is 𐑾).
//!
//! | Letter | Key | Letter | Key |
//! |---|---|---|---|
//! | 𐑐 peep | `p` | 𐑚 bib | `b` |
//! | 𐑑 tot | `t` | 𐑛 dead | `d` |
//! | 𐑒 kick | `k` | 𐑜 gag | `g` |
//! | 𐑓 fee | `f` | 𐑝 vow | `v` |
//! | 𐑔 thigh | `th` | 𐑞 they | `dh` |
//! | 𐑕 so | `s` | 𐑟 zoo | `z` |
//! | 𐑖 sure | `sh` | 𐑠 measure | `zh` |
//! | 𐑗 church | `ch` | 𐑡 judge | `j` |
//! | 𐑘 yea | `y` | 𐑢 woe | `w` |
//! | 𐑙 hung | `ng` | 𐑣 ha-ha | `h` |
//! | 𐑤 loll | `l` | 𐑮 roar | `r` |
//! | 𐑥 mime | `m` | 𐑯 nun | `n` |
//! | 𐑦 if | `i` | 𐑰 eat | `ee` |
//! | 𐑧 egg | `e` | 𐑱 age | `ay` |
//! | 𐑨 ash | `a` | 𐑲 ice | `ie` |
//! | 𐑩 ado | `@` | 𐑳 up | `u` |
//! | 𐑪 on | `o` | 𐑴 oak | `oh` |
//! | 𐑫 wool | `uu` | 𐑵 ooze | `oo` |
//! | 𐑬 out | `ow` | 𐑶 oil | `oy` |
//! | 𐑭 ah | `ah` | 𐑷 awe | `aw` |
//! | 𐑸 are | `ar` | 𐑹 or | `or` |
//! | 𐑺 air | `air` | 𐑻 err | `ur` |
//! | 𐑼 array | `@r` | 𐑽 ear | `eer` |
//! | 𐑾 ian | `i@` | 𐑿 yew | `ew` |
//! | · namer dot | `=` | | |
//!
//! **Longest match wins.** Keys are read left to right, and each letter takes
//! the longest key sequence that fits: `th` is 𐑔, `ar` is 𐑸. To get the
//! shorter reading, type `/` (the [`SEPARATOR`]) between the keys: `t/h` is
//! 𐑑𐑣, `a/r` is 𐑨𐑮. The separator itself produces nothing.
//!
//! Any other character passes through unchanged, so digits, spaces, marks and
//! the ASCII stroke fallbacks can be typed in the same stream. The one
//! exception is `v`, which is 𐑝 here rather than the `▼` fallback; type `▼`
//! for an arrowhead while transliterating. The namer dot is `=` rather than
//! `.` because `.` is a stroke.

mod reading;
mod translit;

pub use reading::{annotate, reading};
pub use translit::{Edit, SEPARATOR, TABLE, Transliterator, transliterate};

/// The four shape classes of the Shavian letters (SPEC §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LetterClass {
    /// Ascending consonants, U+10450–U+10459 (𐑐 peep … 𐑙 hung).
    Tall,
    /// Descending consonants, U+1045A–U+10463 (𐑚 bib … 𐑣 ha-ha).
    Deep,
    /// x-height letters, U+10464–U+10477 (𐑤 loll … 𐑷 awe).
    Short,
    /// Unpaired ligatures, U+10478–U+1047F (𐑸 are … 𐑿 yew).
    Compound,
}

const FIRST: u32 = 0x10450;

/// Letter names in code-point order, U+10450 to U+1047F.
const NAMES: [&str; 48] = [
    "peep", "tot", "kick", "fee", "thigh", "so", "sure", "church", "yea", "hung", //
    "bib", "dead", "gag", "vow", "they", "zoo", "measure", "judge", "woe", "ha-ha", //
    "loll", "mime", "if", "egg", "ash", "ado", "on", "wool", "out", "ah", //
    "roar", "nun", "eat", "age", "ice", "up", "oak", "ooze", "oil", "awe", //
    "are", "or", "air", "err", "array", "ear", "ian", "yew",
];

/// The class of a Shavian letter, or `None` for anything else (including the
/// namer dot).
pub fn class(c: char) -> Option<LetterClass> {
    match c as u32 {
        0x10450..=0x10459 => Some(LetterClass::Tall),
        0x1045A..=0x10463 => Some(LetterClass::Deep),
        0x10464..=0x10477 => Some(LetterClass::Short),
        0x10478..=0x1047F => Some(LetterClass::Compound),
        _ => None,
    }
}

/// The paired partner of a Shavian letter: tall ↔ deep, and short ↔ short,
/// ten code points apart. Compounds and non-letters have none.
pub fn mirror(c: char) -> Option<char> {
    let cp = c as u32;
    let partner = match cp {
        0x10450..=0x10459 | 0x10464..=0x1046D => cp + 10,
        0x1045A..=0x10463 | 0x1046E..=0x10477 => cp - 10,
        _ => return None,
    };
    char::from_u32(partner)
}

/// The traditional name of a Shavian letter (`peep`, `tot`, … `yew`).
pub fn name(c: char) -> Option<&'static str> {
    class(c)?;
    Some(NAMES[(c as u32 - FIRST) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC §4's table, row by row: tall, deep, short, short.
    const SPEC_PAIRS: [[(char, &str); 4]; 10] = [
        [('𐑐', "peep"), ('𐑚', "bib"), ('𐑤', "loll"), ('𐑮', "roar")],
        [('𐑑', "tot"), ('𐑛', "dead"), ('𐑥', "mime"), ('𐑯', "nun")],
        [('𐑒', "kick"), ('𐑜', "gag"), ('𐑦', "if"), ('𐑰', "eat")],
        [('𐑓', "fee"), ('𐑝', "vow"), ('𐑧', "egg"), ('𐑱', "age")],
        [('𐑔', "thigh"), ('𐑞', "they"), ('𐑨', "ash"), ('𐑲', "ice")],
        [('𐑕', "so"), ('𐑟', "zoo"), ('𐑩', "ado"), ('𐑳', "up")],
        [('𐑖', "sure"), ('𐑠', "measure"), ('𐑪', "on"), ('𐑴', "oak")],
        [
            ('𐑗', "church"),
            ('𐑡', "judge"),
            ('𐑫', "wool"),
            ('𐑵', "ooze"),
        ],
        [('𐑘', "yea"), ('𐑢', "woe"), ('𐑬', "out"), ('𐑶', "oil")],
        [('𐑙', "hung"), ('𐑣', "ha-ha"), ('𐑭', "ah"), ('𐑷', "awe")],
    ];

    const SPEC_COMPOUNDS: [(char, &str); 8] = [
        ('𐑸', "are"),
        ('𐑹', "or"),
        ('𐑺', "air"),
        ('𐑻', "err"),
        ('𐑼', "array"),
        ('𐑽', "ear"),
        ('𐑾', "ian"),
        ('𐑿', "yew"),
    ];

    #[test]
    fn spec_pairs_have_classes_names_and_mirrors() {
        use LetterClass::*;
        for [tall, deep, short_a, short_b] in SPEC_PAIRS {
            for ((c, n), want) in [
                (tall, Tall),
                (deep, Deep),
                (short_a, Short),
                (short_b, Short),
            ] {
                assert_eq!(class(c), Some(want), "{n}");
                assert_eq!(name(c), Some(n));
            }
            assert_eq!(mirror(tall.0), Some(deep.0), "{}", tall.1);
            assert_eq!(mirror(deep.0), Some(tall.0), "{}", deep.1);
            assert_eq!(mirror(short_a.0), Some(short_b.0), "{}", short_a.1);
            assert_eq!(mirror(short_b.0), Some(short_a.0), "{}", short_b.1);
        }
    }

    #[test]
    fn spec_compounds_are_unpaired() {
        for (c, n) in SPEC_COMPOUNDS {
            assert_eq!(class(c), Some(LetterClass::Compound), "{n}");
            assert_eq!(name(c), Some(n));
            assert_eq!(mirror(c), None, "{n}");
        }
    }

    #[test]
    fn class_boundaries() {
        use LetterClass::*;
        let at = |cp| class(char::from_u32(cp).unwrap());
        assert_eq!(at(0x1044F), None);
        assert_eq!(at(0x10450), Some(Tall));
        assert_eq!(at(0x10459), Some(Tall));
        assert_eq!(at(0x1045A), Some(Deep));
        assert_eq!(at(0x10463), Some(Deep));
        assert_eq!(at(0x10464), Some(Short));
        assert_eq!(at(0x10477), Some(Short));
        assert_eq!(at(0x10478), Some(Compound));
        assert_eq!(at(0x1047F), Some(Compound));
        assert_eq!(at(0x10480), None);
        for c in ['·', 'a', '0', '─', ' '] {
            assert_eq!(class(c), None);
            assert_eq!(mirror(c), None);
            assert_eq!(name(c), None);
        }
    }

    #[test]
    fn mirror_is_an_involution_without_fixed_points() {
        for c in (0x10450..=0x1047F).filter_map(char::from_u32) {
            match mirror(c) {
                Some(m) => {
                    assert_ne!(m, c);
                    assert_eq!(mirror(m), Some(c));
                    let short = class(c) == Some(LetterClass::Short);
                    assert_eq!(short, class(m) == Some(LetterClass::Short));
                }
                None => assert_eq!(class(c), Some(LetterClass::Compound)),
            }
        }
    }
}
