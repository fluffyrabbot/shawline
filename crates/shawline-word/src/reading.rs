//! Readings: transliteration run backwards, for the gloss layer (SPEC §13,
//! Reading). The signatures are a contract (PLAN P9); the bodies are thin
//! first versions.

use crate::translit::TABLE;

/// The keys that type `word`, so that `transliterate(&reading(w)) == w`:
/// `·𐑥𐑱𐑯` reads `=mayn`. A `/` goes between two keys only where the longest
/// match would otherwise join them (`𐑑𐑣` reads `t/h`), and a word that starts
/// with 𐑝 reads `/v`. Characters with no key (digits) pass through.
pub fn reading(word: &str) -> String {
    word.chars()
        .map(|c| TABLE.iter().find(|&&(_, letter)| letter == c).map_or(c.to_string(), |(key, _)| key.to_string()))
        .collect()
}

/// `message` with each run of Shavian letters and namer dots followed by its
/// reading in parentheses: `𐑥𐑱𐑯 (mayn)`. Applied where diagnostics are
/// printed, never where they are written.
pub fn annotate(message: &str) -> String {
    message.to_owned()
}
