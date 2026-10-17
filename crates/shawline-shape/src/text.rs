//! Text runs: words, numbers, «literals» and ※ comments.

use shawline_grid::repertoire::is_word_char;
use shawline_grid::{Diagnostic, Grid, Pos};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextKind {
    Word,
    Number,
    Literal,
    Comment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    pub pos: Pos,
    /// Cells covered, including delimiters.
    pub width: usize,
    pub kind: TextKind,
    /// The text without delimiters.
    pub content: String,
    /// The innermost box whose interior holds this text.
    pub owner: Option<usize>,
    /// The box whose top border holds this text as its title.
    pub title_of: Option<usize>,
    /// The box one of whose dividers holds this text as a guard.
    pub divider_of: Option<usize>,
}

pub(crate) fn find(grid: &Grid, diagnostics: &mut Vec<Diagnostic>) -> Vec<Text> {
    let mut texts = Vec::new();
    for r in 0..grid.height() {
        let row = grid.row(r);
        let mut c = 0;
        while c < row.len() {
            let start = c;
            let (kind, content, end) = match row[c] {
                '※' => {
                    let end = row.iter().rposition(|&ch| ch != ' ').map_or(c + 1, |i| i + 1);
                    let content: String = row[c + 1..end].iter().collect();
                    (TextKind::Comment, content.trim().to_owned(), end)
                }
                '«' => {
                    let close = row[c + 1..].iter().position(|&ch| ch == '»');
                    if close.is_none() {
                        diagnostics.push(Diagnostic::new(Pos::new(r, c), "S002", "unclosed «"));
                    }
                    let inner_end = close.map_or(row.len(), |n| c + 1 + n);
                    let content = row[c + 1..inner_end].iter().collect();
                    (TextKind::Literal, content, (inner_end + 1).min(row.len()))
                }
                '»' => {
                    diagnostics.push(Diagnostic::new(Pos::new(r, c), "S003", "» without «"));
                    c += 1;
                    continue;
                }
                ch if is_word_char(ch) => {
                    let n = row[c..].iter().take_while(|&&ch| is_word_char(ch)).count();
                    let word: String = row[c..c + n].iter().collect();
                    let digits = word.chars().filter(char::is_ascii_digit).count();
                    let kind = if digits == n {
                        TextKind::Number
                    } else {
                        if digits > 0 {
                            let message = "digits and letters in one word";
                            diagnostics.push(Diagnostic::new(Pos::new(r, c), "S004", message));
                        }
                        TextKind::Word
                    };
                    (kind, word, c + n)
                }
                _ => {
                    c += 1;
                    continue;
                }
            };
            texts.push(Text {
                pos: Pos::new(r, start),
                width: end - start,
                kind,
                content,
                owner: None,
                title_of: None,
                divider_of: None,
            });
            c = end;
        }
    }
    texts
}
