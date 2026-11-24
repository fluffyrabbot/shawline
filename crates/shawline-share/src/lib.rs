//! Links: a sheet's whole text carried in a URL's fragment (SPEC §17).
//!
//! Lead-owned signatures (PLAN P12). A fragment is `v1.` followed by the
//! base64url encoding (RFC 4648 §5, no `=` padding) of the raw DEFLATE stream
//! (RFC 1951, no zlib or gzip wrapper) of the text's UTF-8 bytes. Nothing is
//! formatted or checked on the way: [`decode`] gives back exactly the bytes
//! [`encode`] was given, byte-order mark, line endings and errors included.
//!
//! Studio reaches this crate through `shawline-wasm`, and the CLI through
//! `shawline link`. The corpus's fixed links (`corpus/links/`, checked by
//! `shawline-corpus/tests/links.rs`) were encoded by another implementation,
//! so they hold this one to the format rather than to its own output.

use std::fmt;

/// The format version this crate writes, and the only one it reads.
pub const VERSION: u32 = 1;

/// The most text a link may carry, in bytes: 1 MiB. [`decode`] stops
/// inflating here rather than after, so a small hostile link can't make it
/// allocate more.
pub const MAX_TEXT: usize = 1 << 20;

/// Why a fragment didn't open. Each message names what is wrong and what
/// the reader can do next; it never mentions a surface, so Studio and the
/// CLI can both show it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkError {
    /// The fragment doesn't start with `v`, one or more ASCII digits and
    /// `.`. Studio takes such a fragment to be an example's name
    /// (`#03-double`), so this is the one error it doesn't show.
    NotALink,
    /// The fragment names a version other than [`VERSION`]. A number too
    /// large for `u32` is `UnknownVersion(u32::MAX)`.
    UnknownVersion(u32),
    /// The payload isn't strict base64url (a character outside the
    /// alphabet, `=` padding, a final group of one character, or nonzero
    /// unused bits), isn't one complete DEFLATE stream, or has bytes after
    /// the stream's final block.
    Damaged,
    /// The stream inflates past [`MAX_TEXT`].
    TooLarge,
    /// The stream inflates to bytes that aren't UTF-8.
    NotText,
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkError::NotALink => {
                write!(f, "This isn’t a link to a sheet: a link’s fragment starts with v{VERSION}.")
            }
            LinkError::UnknownVersion(version) => write!(
                f,
                "This link was made in a newer format (v{version}) than this Shawline reads. \
                 Open it in the latest Studio, or ask for the sheet’s text."
            ),
            LinkError::Damaged => f.write_str(
                "This link is damaged: part of it is missing or changed. \
                 Ask for the link again, or for the sheet’s text.",
            ),
            LinkError::TooLarge => write!(
                f,
                "This link holds more than {} MiB of text, the most a link may carry. \
                 Ask for the sheet’s file instead.",
                MAX_TEXT >> 20
            ),
            LinkError::NotText => f.write_str(
                "This link doesn’t hold text: its bytes aren’t UTF-8. \
                 Ask for the link again, or for the sheet’s text.",
            ),
        }
    }
}

impl std::error::Error for LinkError {}

/// The fragment that carries `text`, without its `#`: `v1.` and then the
/// payload. Any text encodes, however long; only [`decode`] enforces
/// [`MAX_TEXT`], so an over-long link fails where it is opened, with a
/// reason, and a caller that wants to refuse earlier compares the text's
/// length itself.
pub fn encode(text: &str) -> String {
    let _ = text;
    todo!("P12 wave 1, agent S")
}

/// The text `fragment` carries. `fragment` is the URL's fragment without
/// its `#`. `decode(&encode(t)) == Ok(t)` for every text `t` of at most
/// [`MAX_TEXT`] bytes.
pub fn decode(fragment: &str) -> Result<String, LinkError> {
    let _ = fragment;
    todo!("P12 wave 1, agent S")
}
