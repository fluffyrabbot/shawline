//! Readings: transliteration run backwards, for the gloss layer (SPEC §13,
//! Reading). The signatures are a contract (PLAN P9); the bodies are thin
//! first versions.

use std::ops::Range;

use crate::translit::{SEPARATOR, TABLE, munch};

/// The keys that type `word`, so that `transliterate(&reading(w)) == w`:
/// `·𐑥𐑱𐑯` reads `=mayn`. A `/` goes between two keys only where the longest
/// match would otherwise join them (`𐑑𐑣` reads `t/h`), and a word that starts
/// with 𐑝 reads `/v`. Characters with no key (digits) pass through.
pub fn reading(word: &str) -> String {
    let longest = TABLE.iter().map(|(key, _)| key.len()).max().unwrap_or(1);
    let mut out = String::new();
    // Where each key written so far sits in `out`.
    let mut written: Vec<Range<usize>> = Vec::new();
    for (i, c) in word.chars().enumerate() {
        let key = TABLE
            .iter()
            .find(|&&(_, letter)| letter == c)
            .map_or_else(|| c.to_string(), |(key, _)| (*key).to_owned());
        // The new key joins an earlier one if the longest match from that
        // one's start would now read past its end. Keys are at least a byte
        // long, so only the last `longest - 1` reach this far. Separating
        // here, as late as possible, is where a typist would, and it also
        // separates the fewest keys.
        let joins = written
            .iter()
            .rev()
            .take(longest - 1)
            .any(|r| munch(&format!("{}{key}", &out[r.start..])).0 > r.len());
        // Outside a word run `v` is the `▼` fallback (SPEC §13).
        if joins || (i == 0 && c == '𐑝') {
            out.push(SEPARATOR);
        }
        let start = out.len();
        out.push_str(&key);
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

    /// Checks that `word`'s reading types it back, and that every `/` in the
    /// reading is needed: without it the keys would read differently.
    fn assert_exact(word: &str) {
        let keys = reading(word);
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
            assert_eq!(reading(word), keys, "{word:?}");
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
            assert_exact(&word);
            for &b in &letters {
                word.truncate(a.len_utf8());
                word.push(b);
                assert_exact(&word);
                for &c in &letters {
                    word.truncate(a.len_utf8() + b.len_utf8());
                    word.push(c);
                    assert_exact(&word);
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
        let mut separated = 0;
        for _ in 0..5000 {
            let len = 1 + next() % 10;
            let word: String = (0..len).map(|_| pool[(next() % pool.len() as u64) as usize]).collect();
            assert_exact(&word);
            separated += usize::from(reading(&word).trim_start_matches("/v").contains(SEPARATOR));
        }
        assert!(separated > 250, "only {separated} words needed a /");
    }

    #[test]
    fn annotate_follows_each_word_with_its_reading() {
        for (message, want) in [
            ("unknown name 𐑓𐑵", "unknown name 𐑓𐑵 (foo)"),
            ("𐑓𐑵 takes 2 inputs", "𐑓𐑵 (foo) takes 2 inputs"),
            (
                "·𐑥𐑱𐑯 calls 𐑑𐑣 and 𐑝𐑧𐑮𐑦, then 𐑑𐑣",
                "·𐑥𐑱𐑯 (=mayn) calls 𐑑𐑣 (t/h) and 𐑝𐑧𐑮𐑦 (/veri), then 𐑑𐑣 (t/h)",
            ),
            ("«𐑣𐑧𐑤𐑴 𐑢𐑻𐑤𐑛»", "«𐑣𐑧𐑤𐑴 (heloh) 𐑢𐑻𐑤𐑛 (wurld)»"),
            ("𐑐", "𐑐 (p)"),
            ("𐑓𐑵·𐑚𐑸", "𐑓𐑵·𐑚𐑸 (foo=bar)"),
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
