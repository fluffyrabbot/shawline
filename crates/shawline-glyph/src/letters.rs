//! The 48 Shavian letters.
//!
//! Forms follow Kingsley Read's Shaw alphabet as charted in the Wikipedia
//! article and drawn by Apple Symbols. A letter whose form really is another
//! letter turned or mirrored is derived by that transform, never redrawn.

use crate::{FLIP_H, FLIP_V, ROTATE, Skeleton, ellipse, strokes_only};

/// How a letter's skeleton is obtained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Derivation {
    Drawn,
    /// 180° about the cell centre.
    Rotated(char),
    /// Mirrored left to right.
    FlippedH(char),
    /// Mirrored top to bottom.
    FlippedV(char),
}

const FIRST: u32 = 0x10450;

const NAMES: [&str; 48] = [
    "peep", "tot", "kick", "fee", "thigh", "so", "sure", "church", "yea", "hung", // tall
    "bib", "dead", "gag", "vow", "they", "zoo", "measure", "judge", "woe", "ha-ha", // deep
    "loll", "mime", "if", "egg", "ash", "ado", "on", "wool", "out", "ah", // short
    "roar", "nun", "eat", "age", "ice", "up", "oak", "ooze", "oil", "awe", // short
    "are", "or", "air", "err", "array", "ear", "ian", "yew", // compound
];

fn letter(name: &str) -> char {
    let i = NAMES.iter().position(|n| *n == name).expect("known letter name");
    char::from_u32(FIRST + i as u32).expect("Shavian code point")
}

/// The letter's conventional name ("peep", "bib", …).
pub(crate) fn letter_name(ch: char) -> Option<&'static str> {
    NAMES.get((ch as u32).checked_sub(FIRST)? as usize).copied()
}

/// How `ch` is drawn. Partners (code point +10) are derived where the real
/// partner is a true rotation or reflection; a few non-partners share a form
/// too (𐑧 𐑨 𐑩 𐑪 are one hook in four orientations).
pub(crate) fn derivation(ch: char) -> Derivation {
    use Derivation::*;
    let Some(name) = letter_name(ch) else { return Drawn };
    let (how, base): (fn(char) -> Derivation, &str) = match name {
        "bib" => (Rotated, "peep"),
        "dead" => (Rotated, "tot"),
        "gag" => (Rotated, "kick"),
        "vow" => (Rotated, "fee"),
        "they" => (Rotated, "thigh"),
        // 𐑕 is S-like and turning an S gives an S back; 𐑟 is its mirror image.
        "zoo" => (FlippedV, "so"),
        "measure" => (Rotated, "sure"),
        "judge" => (Rotated, "church"),
        // Likewise a turned `\` is still `\`; 𐑢 is `/`.
        "woe" => (FlippedV, "yea"),
        "ha-ha" => (Rotated, "hung"),
        "roar" => (Rotated, "loll"),
        "nun" => (FlippedH, "mime"),
        "ooze" => (Rotated, "wool"),
        "oil" => (Rotated, "out"),
        "awe" => (FlippedH, "ah"),
        "ash" => (FlippedH, "egg"),
        "ado" => (FlippedV, "egg"),
        "on" => (Rotated, "egg"),
        "ice" => (FlippedH, "age"),
        "err" => (FlippedV, "air"),
        _ => return Drawn,
    };
    how(letter(base))
}

pub(crate) fn skeleton(ch: char) -> Option<Skeleton> {
    let name = letter_name(ch)?;
    Some(match derivation(ch) {
        Derivation::Drawn => drawn(name),
        Derivation::Rotated(b) => skeleton(b)?.transformed(ROTATE),
        Derivation::FlippedH(b) => skeleton(b)?.transformed(FLIP_H),
        Derivation::FlippedV(b) => skeleton(b)?.transformed(FLIP_V),
    })
}

/// Hand-placed centrelines. Guides: ascender 110, x-height 400, baseline 800
/// (see `metrics`); letters are centred on x = 300.
fn drawn(name: &str) -> Skeleton {
    strokes_only(&match name {
        // Tall.
        "peep" => vec!["M200,220 C200,150 240,110 295,110 C355,110 390,150 390,215 L390,800"],
        "tot" => vec!["M205,275 C285,250 355,200 395,110 L395,800"],
        // A c-bowl hung from a stem: the stem comes down and turns back into the bowl.
        "kick" => vec![
            "M420,110 L420,480 C400,425 355,400 300,400 C220,400 175,480 175,600 \
             C175,720 220,800 300,800 C355,800 400,775 420,725",
        ],
        "fee" => vec!["M390,110 L390,690 C390,760 350,800 290,800 C250,800 215,785 195,760"],
        // A loop whose tail leaves the right side and runs up to the ascender,
        // grazing the loop's own start so it reads closed.
        "thigh" => vec![
            "M310,400 C225,400 170,480 170,600 C170,720 225,800 300,800 \
             C375,800 430,720 430,610 C430,550 415,510 385,470 L180,110",
        ],
        "so" => vec![
            "M420,175 C400,130 360,110 305,110 C240,110 200,150 200,210 \
             C200,290 400,540 400,660 C400,750 355,800 290,800 C245,800 205,785 180,760",
        ],
        "sure" => vec![
            "M420,110 L225,590 C205,640 195,680 195,710 C195,765 235,800 290,800 C345,800 390,770 410,725",
        ],
        "church" => vec![
            "M185,250 L420,110 L225,590 C205,640 195,680 195,710 C195,765 235,800 290,800 \
             C345,800 390,770 410,725",
        ],
        "yea" => vec!["M190,110 L410,800"],
        // Legs cross below a loop; each leg runs straight on through the crossing.
        "hung" => vec![
            "M180,800 L300,470 C348,338 385,260 385,205 C385,148 348,110 300,110 \
             C252,110 215,148 215,205 C215,260 252,338 300,470 L420,800",
        ],
        // Short.
        "loll" => vec!["M420,400 L320,400 C225,400 175,480 175,600 C175,720 225,800 320,800 L420,800"],
        "mime" => vec!["M425,400 C345,400 300,445 300,540 L300,660 C300,755 255,800 175,800"],
        "if" => vec!["M300,400 L300,800"],
        "eat" => vec!["M205,400 L205,640 L395,520 L395,800"],
        "egg" => vec!["M195,400 L195,610 C195,735 270,800 410,800"],
        "age" => vec!["M410,400 L195,400 L195,610 C195,735 270,800 410,800"],
        "up" => vec!["M180,400 L420,400", "M260,800 L260,630 C260,500 300,425 380,400"],
        "wool" => vec!["M180,400 L300,800 L420,400"],
        // A brace: both halves leave the cusp horizontally.
        "out" => vec!["M400,400 C400,520 340,600 190,600 C280,600 310,640 310,700 C310,770 345,800 420,800"],
        "ah" => vec![
            "M425,470 C415,425 370,400 305,400 C235,400 185,435 185,485 C185,535 235,560 300,560 \
             L360,560 C370,640 355,710 330,750 C300,790 255,800 195,800",
        ],
        "oak" => return Skeleton { strokes: vec![ellipse(300.0, 600.0, 125.0, 200.0)], fills: vec![] },
        // Compounds, each a ligature of the letters it sounds like.
        "are" => vec![
            "M290,560 L240,560 C180,560 145,525 145,480 C145,430 190,400 255,400 L320,400 \
             C415,400 465,480 465,600 C465,730 415,800 330,800",
            "M290,560 C290,690 255,790 150,800",
        ],
        "or" => vec![
            "M140,470 C145,425 185,400 240,400 C295,400 330,430 330,475 C330,525 290,555 240,560",
            "M240,560 C240,720 285,800 360,800 C430,800 465,730 465,600 C465,490 445,430 410,410",
        ],
        "air" => vec![
            "M250,800 C190,800 150,775 150,738 C150,700 190,680 240,676 C245,520 290,400 365,400 \
             C430,400 470,470 470,580 C470,720 440,800 380,800",
        ],
        "array" => vec![
            "M160,800 L215,520 C230,445 280,400 340,400 C420,400 465,470 465,600 C465,730 420,800 345,800",
        ],
        "ear" => vec![
            "M125,400 L125,800",
            "M270,800 L298,540 C308,455 345,400 395,400 C450,400 480,470 480,600 C480,730 448,800 395,800",
        ],
        "ian" => vec!["M170,400 L170,800", "M310,800 L328,570 C338,465 370,405 435,400"],
        "yew" => vec!["M150,110 L265,800 L360,400 L455,800"],
        other => unreachable!("no drawing for {other}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::*;
    use shawline_grid::repertoire::SHAVIAN;

    fn span(ch: char) -> (f64, f64) {
        let b = skeleton(ch).unwrap().bounds().unwrap();
        (b.y0, b.y1)
    }

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 0.5 && (a.1 - b.1).abs() < 0.5
    }

    #[test]
    fn letters_occupy_their_height_class() {
        let tall = (ASCENDER, BASELINE);
        let deep = (X_HEIGHT, DESCENDER);
        let short = (X_HEIGHT, BASELINE);
        for ch in SHAVIAN {
            let i = ch as u32 - FIRST;
            let want = match i {
                0..=9 => tall,
                10..=19 => deep,
                47 => tall, // 𐑿 yew, the one tall vowel
                _ => short,
            };
            assert!(close(span(ch), want), "{} spans {:?}, want {want:?}", letter_name(ch).unwrap(), span(ch));
        }
    }

    #[test]
    fn derived_partners_are_their_base_transformed() {
        let peep = skeleton(letter("peep")).unwrap();
        assert_eq!(skeleton(letter("bib")), Some(peep.transformed(ROTATE)));
        let so = skeleton(letter("so")).unwrap();
        assert_eq!(skeleton(letter("zoo")), Some(so.transformed(FLIP_V)));
        let mime = skeleton(letter("mime")).unwrap();
        assert_eq!(skeleton(letter("nun")), Some(mime.transformed(FLIP_H)));
        // Every derivation names a drawn base, so chains never form.
        for ch in SHAVIAN {
            if let Derivation::Rotated(b) | Derivation::FlippedH(b) | Derivation::FlippedV(b) = derivation(ch) {
                assert_eq!(derivation(b), Derivation::Drawn, "{ch:?} derives from a derived letter");
            }
        }
    }

    #[test]
    fn every_tall_deep_partner_is_derived() {
        // Read's rule: the voiced deep letter is its tall partner turned over.
        for i in 0..10 {
            let deep = char::from_u32(FIRST + 10 + i).unwrap();
            let tall = char::from_u32(FIRST + i).unwrap();
            assert!(
                matches!(derivation(deep), Derivation::Rotated(b) | Derivation::FlippedV(b) if b == tall),
                "{} is not derived from {}",
                letter_name(deep).unwrap(),
                letter_name(tall).unwrap()
            );
        }
    }
}
