//! Wording shared by the diagnostics of syntax and of the checker
//! (`shawline-ir`), in the words of SPEC §16.
//!
//! Shavian words are written plainly, never with readings or keys: surfaces
//! add readings themselves (`shawline_word::annotate`). No Shavian word is
//! followed by an apostrophe or a parenthesis, which would run into its
//! reading.

use std::fmt;

/// A port of a definition, a node or a switch, or a switch's merged output,
/// as a message names it: by its order, top to bottom, and the word of what
/// it belongs to. A switch has no word, so its parts are named by arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Port<'a> {
    /// Input `index` of the definition named `definition`, without its
    /// namer dot; `None` for a definition whose title is not a `·`-word.
    DefinitionInput { definition: Option<&'a str>, index: usize },
    /// Output `index` of a definition, named as for an input.
    DefinitionOutput { definition: Option<&'a str>, index: usize },
    /// Input `index` of the node whose word is `word`; `None` for a node box
    /// that doesn't hold exactly one word.
    NodeInput { word: Option<&'a str>, index: usize },
    /// Output `index` of a node, named as for an input.
    NodeOutput { word: Option<&'a str>, index: usize },
    /// The `▼` on a switch's top border.
    Selector,
    /// Left through-port `index` of arm `arm`.
    LeftThroughPort { arm: usize, index: usize },
    /// Right through-port `index` of arm `arm`.
    RightThroughPort { arm: usize, index: usize },
    /// Where the right through-ports at result ordinal `index` join outside
    /// the switch.
    MergedOutput { index: usize },
}

/// A definition as messages name it: by its name, with the namer dot, or
/// `the definition` when its title is not a `·`-word.
pub fn definition(name: Option<&str>) -> String {
    name.map_or("the definition".to_owned(), |n| format!("·{n}"))
}

/// A node as messages name it: by its word, or `the node` when its box
/// doesn't hold exactly one word.
pub fn node(word: Option<&str>) -> String {
    word.unwrap_or("the node").to_owned()
}

impl fmt::Display for Port<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Port::DefinitionInput { definition: d, index } => {
                write!(f, "the {} input of {}", ordinal(index), definition(d))
            }
            Port::DefinitionOutput { definition: d, index } => {
                write!(f, "the {} output of {}", ordinal(index), definition(d))
            }
            Port::NodeInput { word, index } => write!(f, "the {} input of {}", ordinal(index), node(word)),
            Port::NodeOutput { word, index } => write!(f, "the {} output of {}", ordinal(index), node(word)),
            Port::Selector => f.write_str("the switch's selector"),
            Port::LeftThroughPort { arm, index } => {
                write!(f, "the {} left through-port of the {} arm", ordinal(index), ordinal(arm))
            }
            Port::RightThroughPort { arm, index } => {
                write!(f, "the {} right through-port of the {} arm", ordinal(index), ordinal(arm))
            }
            Port::MergedOutput { index } => write!(f, "the switch's {} merged output", ordinal(index)),
        }
    }
}

/// The ordinal word for a zero-based `index`: `first`, `second`, …, `tenth`,
/// then `11th`, `12th`, `21st`, and so on.
pub fn ordinal(index: usize) -> String {
    const WORDS: [&str; 10] =
        ["first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth"];
    if let Some(word) = WORDS.get(index) {
        return (*word).to_owned();
    }
    let n = index + 1;
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// `count` of `noun`, with the plural an `s` makes: `no inputs`, `1 input`,
/// `2 inputs`.
pub fn quantity(count: usize, noun: &str) -> String {
    match count {
        0 => format!("no {noun}s"),
        1 => format!("1 {noun}"),
        n => format!("{n} {noun}s"),
    }
}

/// `items` as an English series: `a`, `a and b`, `a, b and c`.
pub fn series(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// `Y019`: no wire reaches `port`, which is an arrowhead. A through-port is
/// reached from one side only, so the message says which.
pub fn unfed(port: Port<'_>) -> String {
    match port {
        Port::LeftThroughPort { .. } => format!("no wire reaches {port} from outside the switch"),
        Port::RightThroughPort { .. } => format!("no wire reaches {port} from inside the arm"),
        _ => format!("no wire reaches {port}"),
    }
}

/// `Y011`: the arms of a switch have these numbers of results, top to
/// bottom, and they differ.
pub fn unequal_arms(results: &[usize]) -> String {
    let each: Vec<String> = results
        .iter()
        .enumerate()
        .map(|(arm, &count)| {
            let count = if count == 0 { "none".to_owned() } else { count.to_string() };
            match arm {
                0 => format!("the first arm has {count}"),
                _ => format!("the {} has {count}", ordinal(arm)),
            }
        })
        .collect();
    format!("arms with different numbers of results: {}", series(&each))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinals_are_words_to_ten_then_numbers() {
        let got: Vec<String> = [0, 1, 2, 9, 10, 11, 12, 20, 21, 22, 23, 100, 110, 111, 112, 120]
            .into_iter()
            .map(ordinal)
            .collect();
        let want = [
            "first", "second", "third", "tenth", "11th", "12th", "13th", "21st", "22nd", "23rd", "24th",
            "101st", "111th", "112th", "113th", "121st",
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn quantities_and_series() {
        let got = [quantity(0, "input"), quantity(1, "output"), quantity(3, "source")];
        assert_eq!(got, ["no inputs", "1 output", "3 sources"]);
        let items = |n: usize| -> Vec<String> { ["a", "b", "c"][..n].iter().map(|s| s.to_string()).collect() };
        let got: Vec<String> = (0..=3).map(|n| series(&items(n))).collect();
        assert_eq!(got, ["", "a", "a and b", "a, b and c"]);
    }

    #[test]
    fn ports_are_named_by_order_and_owner() {
        let named = [
            Port::DefinitionInput { definition: Some("𐑛𐑳𐑚𐑩𐑤"), index: 0 },
            Port::DefinitionOutput { definition: None, index: 1 },
            Port::NodeInput { word: Some("𐑐"), index: 1 },
            Port::NodeOutput { word: None, index: 0 },
            Port::Selector,
            Port::LeftThroughPort { arm: 1, index: 0 },
            Port::RightThroughPort { arm: 0, index: 2 },
            Port::MergedOutput { index: 0 },
        ]
        .map(|p| p.to_string());
        assert_eq!(
            named,
            [
                "the first input of ·𐑛𐑳𐑚𐑩𐑤",
                "the second output of the definition",
                "the second input of 𐑐",
                "the first output of the node",
                "the switch's selector",
                "the first left through-port of the second arm",
                "the third right through-port of the first arm",
                "the switch's first merged output",
            ]
        );
    }

    #[test]
    fn unfed_through_ports_say_which_side() {
        let got = [
            unfed(Port::NodeInput { word: Some("𐑐"), index: 1 }),
            unfed(Port::LeftThroughPort { arm: 0, index: 0 }),
            unfed(Port::RightThroughPort { arm: 1, index: 0 }),
        ];
        assert_eq!(
            got,
            [
                "no wire reaches the second input of 𐑐",
                "no wire reaches the first left through-port of the first arm from outside the switch",
                "no wire reaches the first right through-port of the second arm from inside the arm",
            ]
        );
    }

    #[test]
    fn unequal_arms_count_each_arm() {
        assert_eq!(
            unequal_arms(&[1, 0]),
            "arms with different numbers of results: the first arm has 1 and the second has none"
        );
        assert_eq!(
            unequal_arms(&[2, 2, 1]),
            "arms with different numbers of results: the first arm has 2, the second has 2 and the third has 1"
        );
    }
}
