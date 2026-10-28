//! Readings and keys, for the gloss layer (SPEC §13, Reading). A reading
//! says how a word sounds; its keys are what types it. The signatures are a
//! contract (PLAN P9, Readings as respellings).

use std::ops::Range;

use crate::translit::{SEPARATOR, TABLE, munch};

/// Written between two letters of a reading whose spellings would otherwise
/// read as a third letter's (`𐑑𐑣` reads `t-h`).
pub const READING_SEPARATOR: char = '-';

/// The letters whose reading differs from their keys: the schwa and the
/// two letters spelled with it, and the namer dot, which has no sound.
const READS: &[(char, &str)] = &[('𐑩', "ə"), ('𐑼', "ər"), ('𐑾', "iə"), ('·', "")];

/// How `letter` reads: its keys, except for the letters in [`READS`].
fn spelling(letter: char) -> Option<&'static str> {
    READS
        .iter()
        .find(|&&(c, _)| c == letter)
        .map(|&(_, reads)| reads)
        .or_else(|| TABLE.iter().find(|&&(_, c)| c == letter).map(|&(key, _)| key))
}

/// One longest-match step over the letters' readings: the byte length of the
/// longest reading `s` starts with, or of its first character.
fn read_one(s: &str) -> (usize, char) {
    TABLE
        .iter()
        .filter_map(|&(_, c)| spelling(c).filter(|r| !r.is_empty()).map(|r| (r, c)))
        .filter(|(r, _)| s.starts_with(r))
        .max_by_key(|(r, _)| r.len())
        .map(|(r, c)| (r.len(), c))
        .unwrap_or_else(|| {
            let c = s.chars().next().expect("read_one on empty input");
            (c.len_utf8(), c)
        })
}

/// How `word` sounds, in the letters of English pronunciation respellings:
/// `·𐑛𐑳𐑚𐑩𐑤` reads `dubəl`. It is the keys with three changes: 𐑩 reads `ə`,
/// the namer dot reads nothing, and a `-` stands between two letters only
/// where their spellings would otherwise read as a third (`𐑑𐑣` reads `t-h`).
/// Reading the result back by longest match gives `word` without its namer
/// dots. Characters with no key (digits) pass through.
pub fn reading(word: &str) -> String {
    spell(word, READING_SEPARATOR, |c| spelling(c).map(str::to_owned), read_one, false)
}

/// The keys that type `word`, so that `transliterate(&keys(w)) == w`:
/// `·𐑥𐑱𐑯` is typed `=mayn`. A `/` goes between two keys only where the
/// longest match would otherwise join them (`𐑑𐑣` is typed `t/h`), and a word
/// that starts with 𐑝 is typed `/v`. Characters with no key (digits) pass
/// through.
pub fn keys(word: &str) -> String {
    let key = |c: char| TABLE.iter().find(|&&(_, letter)| letter == c).map(|&(key, _)| key.to_owned());
    spell(word, SEPARATOR, key, munch, true)
}

/// Spells `word` letter by letter with `spell_letter`, putting `separator`
/// before a spelling wherever `matched` (a longest-match step) would read it
/// together with an earlier one. With `lead_v`, a word starting with 𐑝 also
/// starts with the separator.
fn spell(
    word: &str,
    separator: char,
    spell_letter: impl Fn(char) -> Option<String>,
    matched: fn(&str) -> (usize, char),
    lead_v: bool,
) -> String {
    let longest = TABLE.iter().map(|(key, _)| key.len()).max().unwrap_or(1);
    let mut out = String::new();
    // Where each spelling written so far sits in `out`.
    let mut written: Vec<Range<usize>> = Vec::new();
    for (i, c) in word.chars().enumerate() {
        let part = spell_letter(c).unwrap_or_else(|| c.to_string());
        // A letter with no sound (the namer dot, in a reading) adds nothing.
        if part.is_empty() {
            continue;
        }
        // The new spelling joins an earlier one if the longest match from that
        // one's start would now read past its end. Spellings are at least a
        // byte long, so only the last `longest - 1` reach this far. Separating
        // here, as late as possible, is where a typist would, and it also
        // separates the fewest spellings.
        let joins = written
            .iter()
            .rev()
            .take(longest - 1)
            .any(|r| matched(&format!("{}{part}", &out[r.start..])).0 > r.len());
        // Outside a word run `v` is the `▼` fallback (SPEC §13).
        if joins || (lead_v && i == 0 && c == '𐑝') {
            out.push(separator);
        }
        let start = out.len();
        out.push_str(&part);
        written.push(start..out.len());
    }
    out
}

/// `message` with each run of Shavian letters and namer dots followed by its
/// reading in parentheses: `𐑥𐑱𐑯 (mayn)`. Applied where diagnostics are
/// printed, never where they are written.
pub fn annotate(message: &str) -> String {
    let letter = |c: char| crate::class(c).is_some();
    let in_word = |c: char| c == '·' || letter(c);
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(start) = rest.find(in_word) {
        let end = rest[start..].find(|c| !in_word(c)).map_or(rest.len(), |n| start + n);
        let word = &rest[start..end];
        out.push_str(&rest[..end]);
        // A namer dot alone is English punctuation here: "one ·-word".
        if word.chars().any(letter) {
            out.push_str(" (");
            out.push_str(&reading(word));
            out.push(')');
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transliterate;

    /// Checks that `word`'s keys type it back, and that every `/` in them is
    /// needed: without it the keys would read differently.
    fn assert_exact(word: &str) {
        let keys = keys(word);
        assert_eq!(transliterate(&keys), word, "{word:?} reads {keys:?}");
        let leading_v = word.starts_with('𐑝');
        assert_eq!(keys.starts_with("/v"), leading_v, "{word:?} reads {keys:?}");
        for (i, _) in keys.match_indices(SEPARATOR) {
            if i == 0 && leading_v {
                continue;
            }
            let joined = format!("{}{}", &keys[..i], &keys[i + 1..]);
            assert_ne!(
                transliterate(&joined),
                word,
                "{word:?} reads {keys:?}, but the / at {i} is not needed"
            );
        }
    }

    /// Reads `reading` back by longest match over the letters' readings.
    fn read_back(reading: &str) -> String {
        let mut out = String::new();
        let mut rest = reading;
        while let Some(c) = rest.chars().next() {
            if c == READING_SEPARATOR {
                rest = &rest[c.len_utf8()..];
                continue;
            }
            let (len, letter) = read_one(rest);
            out.push(letter);
            rest = &rest[len..];
        }
        out
    }

    /// Checks that `word`'s reading reads back to it, namer dots aside, and
    /// that every `-` in the reading is needed.
    fn assert_reads(word: &str) {
        let read = reading(word);
        let sounded: String = word.chars().filter(|&c| c != '·').collect();
        assert_eq!(read_back(&read), sounded, "{word:?} reads {read:?}");
        for (i, _) in read.match_indices(READING_SEPARATOR) {
            let joined = format!("{}{}", &read[..i], &read[i + 1..]);
            assert_ne!(read_back(&joined), sounded, "{word:?} reads {read:?}, but the - at {i} is not needed");
        }
    }

    /// Checks both spellings of `word`.
    fn assert_both(word: &str) {
        assert_exact(word);
        assert_reads(word);
    }

    /// Letters with how each is spelled.
    type Spelled = Vec<(char, String)>;

    /// SPEC §13's two tables, as `(letter, key)` and `(letter, reads)` pairs
    /// from every `| letter name | `key` |` cell pair of their rows. Each
    /// letter's name must be [`letter_name`]'s.
    fn spec_tables() -> (Spelled, Spelled) {
        let spec = include_str!("../../../docs/SPEC.md");
        let section = &spec[spec.find("## 13. Typing it").unwrap()..spec.find("## 14.").unwrap()];
        let code = |cell: &str| cell.trim().trim_matches('`').to_owned();
        let (mut keyed, mut reads) = (Vec::new(), Vec::new());
        for row in section.lines().filter(|l| l.starts_with("| ") && !l.starts_with("| Letter")) {
            let cells: Vec<&str> = row.trim_matches('|').split('|').collect();
            let letter = |cell: &str| {
                let c = cell.trim().chars().next().unwrap();
                let name = cell.trim()[c.len_utf8()..].trim();
                assert_eq!(crate::letter_name(c), Some(name), "SPEC names {c} {name:?}");
                c
            };
            if cells.len() == 3 {
                let to = if cells[2].trim() == "nothing" { String::new() } else { code(cells[2]) };
                reads.push((letter(cells[0]), to));
                continue;
            }
            for pair in cells.chunks(2).filter(|p| !p[0].trim().is_empty()) {
                keyed.push((letter(pair[0]), code(pair[1])));
            }
        }
        (keyed, reads)
    }

    #[test]
    fn spec_tables_match_the_code() {
        let (keyed, reads) = spec_tables();
        let mut table: Vec<(char, String)> = TABLE.iter().map(|&(k, c)| (c, k.to_owned())).collect();
        let mut keyed = keyed;
        table.sort();
        keyed.sort();
        assert_eq!(keyed, table);
        let mut code: Vec<(char, String)> = READS.iter().map(|&(c, r)| (c, r.to_owned())).collect();
        let mut reads = reads;
        code.sort();
        reads.sort();
        assert_eq!(reads, code);
    }

    #[test]
    fn readings_respell_the_keys() {
        for (word, read) in [
            ("·𐑥𐑱𐑯", "mayn"),
            ("·𐑛𐑳𐑚𐑩𐑤", "dubəl"),
            ("𐑣𐑧𐑤𐑴", "heloh"),
            ("𐑑𐑣", "t-h"),
            ("𐑨𐑘", "a-y"),
            ("𐑩𐑮", "ə-r"),
            ("𐑼", "ər"),
            ("𐑦𐑩", "i-ə"),
            ("𐑾", "iə"),
            ("𐑝", "v"),
            ("𐑝𐑧𐑮𐑦", "veri"),
            ("·𐑝", "v"),
            ("𐑓𐑵2", "foo2"),
            ("", ""),
        ] {
            assert_eq!(reading(word), read, "{word:?}");
            assert_reads(word);
        }
    }

    #[test]
    fn plan_and_spec_examples() {
        for (word, keys) in [
            ("·𐑥𐑱𐑯", "=mayn"),
            ("𐑣𐑧𐑤𐑴", "heloh"),
            ("𐑑𐑣", "t/h"),
            ("𐑨𐑘", "a/y"),
            ("𐑝", "/v"),
            ("𐑝𐑧𐑮𐑦", "/veri"),
            ("·𐑝", "=v"),
            ("𐑦𐑝", "iv"),
            ("·𐑖𐑷", "=shaw"),
            ("𐑰𐑮", "ee/r"),
            ("𐑧𐑧𐑮", "e/er"),
            ("𐑨𐑦𐑮", "ai/r"),
            ("𐑓𐑵2", "foo2"),
            ("", ""),
        ] {
            assert_eq!(super::keys(word), keys, "{word:?}");
            assert_exact(word);
        }
    }

    #[test]
    fn every_word_of_up_to_three_letters_reads_exactly() {
        let letters: Vec<char> = TABLE.iter().map(|&(_, c)| c).collect();
        let mut word = String::new();
        for &a in &letters {
            word.clear();
            word.push(a);
            assert_both(&word);
            for &b in &letters {
                word.truncate(a.len_utf8());
                word.push(b);
                assert_both(&word);
                for &c in &letters {
                    word.truncate(a.len_utf8() + b.len_utf8());
                    word.push(c);
                    assert_both(&word);
                }
            }
        }
    }

    #[test]
    fn seeded_random_words_read_exactly() {
        // SplitMix64, so the words are the same on every run.
        let mut state: u64 = 0x5348_4157_4C49_4E45;
        let mut next = move || {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        // Letters whose keys occur inside longer keys come twice as often,
        // since they are the ones that need separating. Digits pass through.
        let mut pool: Vec<char> = TABLE.iter().map(|&(_, c)| c).collect();
        pool.extend(
            TABLE
                .iter()
                .filter(|(key, _)| TABLE.iter().any(|(k, _)| k.len() > key.len() && k.contains(key)))
                .map(|&(_, c)| c),
        );
        pool.extend(['0', '7']);
        let (mut separated, mut hyphenated) = (0, 0);
        for _ in 0..5000 {
            let len = 1 + next() % 10;
            let word: String = (0..len).map(|_| pool[(next() % pool.len() as u64) as usize]).collect();
            assert_both(&word);
            separated += usize::from(keys(&word).trim_start_matches("/v").contains(SEPARATOR));
            hyphenated += usize::from(reading(&word).contains(READING_SEPARATOR));
        }
        assert!(separated > 250, "only {separated} words needed a /");
        assert!(hyphenated > 250, "only {hyphenated} words needed a -");
    }

    #[test]
    fn annotate_follows_each_word_with_its_reading() {
        for (message, want) in [
            ("unknown name 𐑓𐑵", "unknown name 𐑓𐑵 (foo)"),
            ("𐑓𐑵 takes 2 inputs", "𐑓𐑵 (foo) takes 2 inputs"),
            (
                "·𐑥𐑱𐑯 calls 𐑑𐑣 and 𐑝𐑧𐑮𐑦, then 𐑑𐑣",
                "·𐑥𐑱𐑯 (mayn) calls 𐑑𐑣 (t-h) and 𐑝𐑧𐑮𐑦 (veri), then 𐑑𐑣 (t-h)",
            ),
            ("«𐑣𐑧𐑤𐑴 𐑢𐑻𐑤𐑛»", "«𐑣𐑧𐑤𐑴 (heloh) 𐑢𐑻𐑤𐑛 (wurld)»"),
            ("𐑐", "𐑐 (p)"),
            ("𐑓𐑵·𐑚𐑸", "𐑓𐑵·𐑚𐑸 (foobar)"),
            ("𐑑·𐑣", "𐑑·𐑣 (t-h)"),
            ("𐑛𐑳𐑚𐑩𐑤", "𐑛𐑳𐑚𐑩𐑤 (dubəl)"),
        ] {
            assert_eq!(annotate(message), want, "{message:?}");
        }
    }

    #[test]
    fn annotate_leaves_text_without_shavian_alone() {
        for message in [
            "",
            "digits and letters in one word",
            "a definition's title must be one ·-word",
            "add · 2→1 ··",
            "» without «",
        ] {
            assert_eq!(annotate(message), message);
        }
    }
}
