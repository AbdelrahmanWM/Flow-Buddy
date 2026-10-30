//! `flowbuddy practice`: short drills on one habit, the way keybr drills
//! your weakest key. Each round is a small buffer, where the cursor starts,
//! what to do, and the fewest keys that do it; the Vim plugin runs it and
//! counts the keys you used. The text is made up here: none of yours.

use serde::{Deserialize, Serialize};

/// What a drill practises.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Family {
    /// Up and down: counts, `}` and `{`.
    Vertical,
    /// Along a line: `w` with a count, `f`.
    Horizontal,
    /// Deleting: `dw`, `d2w`, `dt,`, `D`.
    Delete,
    /// Inside brackets and quotes: `di(`, `di"`.
    Inside,
}

impl Family {
    pub const ALL: [Family; 4] = [
        Family::Vertical,
        Family::Horizontal,
        Family::Delete,
        Family::Inside,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Family::Vertical => "vertical",
            Family::Horizontal => "horizontal",
            Family::Delete => "delete",
            Family::Inside => "inside",
        }
    }

    pub fn parse(s: &str) -> Option<Family> {
        Family::ALL.into_iter().find(|f| f.name() == s)
    }

    pub fn title(self) -> &'static str {
        match self {
            Family::Vertical => "Moving up and down",
            Family::Horizontal => "Moving along a line",
            Family::Delete => "Deleting by words",
            Family::Inside => "Inside brackets and quotes",
        }
    }

    /// The commands it practises, as a hint before the first round.
    pub fn about(self) -> &'static str {
        match self {
            Family::Vertical => {
                "Reach the marked line in as few keys as you can: a count (5j, 12k, the line numbers show how far) or } and { to jump to the blank line between paragraphs."
            }
            Family::Horizontal => {
                "Reach the marked spot: w with a count (3w), or f and the letter (fd)."
            }
            Family::Delete => {
                "Delete the marked text: dw, a count (d2w), up to a character (dt,), or to the end of the line (D)."
            }
            Family::Inside => {
                "Delete what's inside the brackets or quotes, from wherever the cursor is: di( di\" di{ di[."
            }
        }
    }
}

/// The drill for a habit (a rule id), if there is one.
pub fn family_for(rule: &str) -> Option<Family> {
    Some(match rule {
        "vim.repeat-jk" | "vim.arrows-normal" | "vim.scroll-run" | "vim.overshoot"
        | "motion.count" | "motion.paragraph" | "motion.page" | "motion.screen"
        | "motion.scroll" | "motion.line-number" => Family::Vertical,
        "vim.repeat-hl" | "vim.find-repeat" | "vim.find-back" | "motion.word" | "motion.find"
        | "motion.line-ends" => Family::Horizontal,
        "vim.repeat-x"
        | "vim.operator-run"
        | "vim.repeat-dd"
        | "vim.backspace-streak"
        | "motion.operator" => Family::Delete,
        "vim.visual-operator" | "motion.text-object" | "motion.bracket" => Family::Inside,
        _ => return None,
    })
}

/// When a round is done.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Goal {
    /// The cursor on this line (1-based), any column.
    Line(usize),
    /// The cursor here: line and column, 1-based (bytes; the text is ASCII).
    Cursor(usize, usize),
    /// The buffer reads exactly this.
    Text(Vec<String>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Round {
    pub lines: Vec<String>,
    /// Line and column, 1-based.
    pub cursor: (usize, usize),
    /// What to do, in a few words.
    pub ask: String,
    pub goal: Goal,
    /// What to highlight: line, column and length (0 for the whole line).
    pub mark: (usize, usize, usize),
    /// The fewest keys, and one way to do it in that many.
    pub par: u32,
    pub best: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Drill {
    pub version: u32,
    pub family: Family,
    pub title: String,
    pub about: String,
    /// Show relative line numbers, so counts can be read off.
    pub relative_numbers: bool,
    pub rounds: Vec<Round>,
}

const WORDS: [&str; 32] = [
    "alpha", "bright", "cedar", "delta", "ember", "fable", "grove", "harbor", "iris", "jolly",
    "kernel", "lemon", "maple", "noble", "orbit", "pepper", "quartz", "river", "salt", "tiger",
    "umber", "velvet", "willow", "xenon", "yonder", "zephyr", "amber", "basil", "copper", "dune",
    "echo", "fern",
];

/// A small deterministic generator: the same seed, the same drill.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// In `lo..=hi`.
    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + (self.next() % (hi - lo + 1) as u64) as usize
    }

    fn word(&mut self) -> &'static str {
        WORDS[self.range(0, WORDS.len() - 1)]
    }

    fn words(&mut self, n: usize) -> Vec<&'static str> {
        (0..n).map(|_| self.word()).collect()
    }
}

/// Keys in a count: `12j` is three.
fn count_keys(n: usize) -> u32 {
    if n == 1 {
        1
    } else {
        n.to_string().len() as u32 + 1
    }
}

/// A drill of `rounds` rounds.
pub fn drill(family: Family, rounds: usize, seed: u64) -> Drill {
    let mut rng = Rng::new(seed);
    let rounds = (0..rounds)
        .map(|_| match family {
            Family::Vertical => vertical(&mut rng),
            Family::Horizontal => horizontal(&mut rng),
            Family::Delete => delete(&mut rng),
            Family::Inside => inside(&mut rng),
        })
        .collect();
    Drill {
        version: 1,
        family,
        title: family.title().into(),
        about: family.about().into(),
        relative_numbers: family == Family::Vertical,
        rounds,
    }
}

/// Paragraphs of a few lines with blank lines between; move to a line.
fn vertical(rng: &mut Rng) -> Round {
    let mut lines = Vec::new();
    while lines.len() < 30 {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        for _ in 0..rng.range(3, 6) {
            lines.push(
                {
                    let n = rng.range(3, 6);
                    rng.words(n)
                }
                .join(" "),
            );
        }
    }
    let text_lines: Vec<usize> = (1..=lines.len())
        .filter(|&l| !lines[l - 1].is_empty())
        .collect();
    let start = text_lines[rng.range(0, text_lines.len() - 1)];
    let blank = |l: usize| lines[l - 1].is_empty();
    // The blank lines `}` and `{` would reach from the start.
    let below = (start + 1..=lines.len()).find(|&l| blank(l));
    let above = (1..start).rev().find(|&l| blank(l));
    let paragraph_jump = rng.range(0, 2) == 0;
    let target = match (paragraph_jump, below, above) {
        (true, Some(b), _) => b,
        (true, None, Some(a)) => a,
        _ => loop {
            let d = rng.range(3, 12);
            let down = rng.range(0, 1) == 0;
            let t = if down {
                start + d
            } else {
                start.saturating_sub(d)
            };
            if (1..=lines.len()).contains(&t) && !blank(t) {
                break t;
            }
        },
    };
    let (par, best) = if Some(target) == below {
        (1, "}".to_string())
    } else if Some(target) == above {
        (1, "{".to_string())
    } else {
        let d = start.abs_diff(target);
        let key = if target > start { 'j' } else { 'k' };
        let n = if d == 1 { String::new() } else { d.to_string() };
        (count_keys(d), format!("{n}{key}"))
    };
    Round {
        lines,
        cursor: (start, 1),
        ask: "Go to the marked line".into(),
        goal: Goal::Line(target),
        mark: (target, 1, 0),
        par,
        best,
    }
}

/// A line of words; move from the start to the start of one of them.
fn horizontal(rng: &mut Rng) -> Round {
    let words = rng.words(9);
    let line = words.join(" ");
    let k = rng.range(2, 8);
    let col = words[..k].iter().map(|w| w.len() + 1).sum::<usize>() + 1;
    let first = words[k].as_bytes()[0];
    // `f` reaches it in two if its letter doesn't come earlier.
    let by_find = !line.as_bytes()[1..col - 1].contains(&first);
    let (par, best) = if by_find {
        (2, format!("f{}", first as char))
    } else {
        let n = if k == 1 { String::new() } else { k.to_string() };
        (count_keys(k), format!("{n}w"))
    };
    Round {
        lines: vec![line],
        cursor: (1, 1),
        ask: "Go to the start of the marked word".into(),
        goal: Goal::Cursor(1, col),
        mark: (1, col, words[k].len()),
        par,
        best,
    }
}

/// Delete words, up to a comma, or to the end of the line.
fn delete(rng: &mut Rng) -> Round {
    match rng.range(0, 2) {
        0 => {
            let words = rng.words(8);
            let i = rng.range(1, 4);
            let n = rng.range(1, 3);
            let col = words[..i].iter().map(|w| w.len() + 1).sum::<usize>() + 1;
            let len = words[i..i + n].iter().map(|w| w.len() + 1).sum::<usize>();
            let line = words.join(" ");
            let after = format!("{}{}", &line[..col - 1], &line[col - 1 + len..]);
            let (par, best) = if n == 1 {
                (2, "dw".to_string())
            } else {
                (3, format!("d{n}w"))
            };
            Round {
                lines: vec![line],
                cursor: (1, col),
                ask: "Delete the marked words".into(),
                goal: Goal::Text(vec![after]),
                mark: (1, col, len - 1),
                par,
                best,
            }
        }
        1 => {
            let head = rng.words(2).join(" ");
            let clause = {
                let n = rng.range(2, 4);
                rng.words(n)
            }
            .join(" ");
            let tail = rng.words(3).join(" ");
            let line = format!("{head} {clause}, {tail}");
            let col = head.len() + 2;
            let after = format!("{head} , {tail}");
            Round {
                lines: vec![line],
                cursor: (1, col),
                ask: "Delete the marked words, up to the comma".into(),
                goal: Goal::Text(vec![after]),
                mark: (1, col, clause.len()),
                par: 3,
                best: "dt,".into(),
            }
        }
        _ => {
            let words = rng.words(8);
            let i = rng.range(3, 6);
            let col = words[..i].iter().map(|w| w.len() + 1).sum::<usize>() + 1;
            let line = words.join(" ");
            let after = line[..col - 1].to_string();
            Round {
                mark: (1, col, line.len() - col + 1),
                lines: vec![line],
                cursor: (1, col),
                ask: "Delete to the end of the line".into(),
                goal: Goal::Text(vec![after]),
                par: 1,
                best: "D".into(),
            }
        }
    }
}

/// Brackets or quotes with words inside; delete the inside from anywhere
/// in it.
fn inside(rng: &mut Rng) -> Round {
    let (open, close, cmd) = match rng.range(0, 3) {
        0 => ('(', ')', "di("),
        1 => ('"', '"', "di\""),
        2 => ('{', '}', "di{"),
        _ => ('[', ']', "di["),
    };
    let name = rng.word();
    let inner = {
        let n = rng.range(2, 4);
        rng.words(n)
    }
    .join(", ");
    let inner = if open == '{' {
        format!(" {inner} ")
    } else {
        inner
    };
    let (lead, after) = match open {
        '"' => (format!("{name} = "), ";"),
        '{' => (format!("{name} = "), ""),
        _ => (name.to_string(), ";"),
    };
    let line = format!("{lead}{open}{inner}{close}{after}");
    let start = lead.len() + 2;
    // Somewhere inside, not on the first character.
    let col = rng.range(start + 1, start + inner.len() - 2);
    Round {
        lines: vec![line],
        cursor: (1, col),
        ask: "Delete everything inside the marked brackets or quotes".into(),
        goal: Goal::Text(vec![format!("{lead}{open}{close}{after}")]),
        mark: (1, start, inner.len()),
        par: 3,
        best: cmd.into(),
    }
}

/// How one round went, as the plugin reports it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundResult {
    pub keys: u32,
    pub par: u32,
    pub ms: u64,
    pub best: String,
}

/// A finished (or stopped) drill.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// Unix seconds.
    pub at: u64,
    pub family: Family,
    pub rounds: Vec<RoundResult>,
}

impl Session {
    /// The fewest keys over the keys used, at most 1: 1 is perfect.
    pub fn efficiency(&self) -> f64 {
        let keys: u32 = self.rounds.iter().map(|r| r.keys.max(r.par)).sum();
        let par: u32 = self.rounds.iter().map(|r| r.par).sum();
        if keys == 0 {
            return 0.0;
        }
        f64::from(par) / f64::from(keys)
    }

    pub fn seconds(&self) -> f64 {
        self.rounds.iter().map(|r| r.ms).sum::<u64>() as f64 / 1000.0
    }

    /// Rounds done in the fewest keys.
    pub fn perfect(&self) -> usize {
        self.rounds.iter().filter(|r| r.keys <= r.par).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_makes_the_same_drill() {
        for f in Family::ALL {
            assert_eq!(drill(f, 6, 7), drill(f, 6, 7));
            assert_ne!(drill(f, 6, 7).rounds, drill(f, 6, 8).rounds);
        }
    }

    #[test]
    fn every_round_is_doable_as_said() {
        for seed in 0..300 {
            for f in Family::ALL {
                for r in drill(f, 4, seed).rounds {
                    check(&r);
                }
            }
        }
    }

    /// Do `best` by hand and see it reach the goal in `par` keys.
    fn check(r: &Round) {
        let (line, col) = r.cursor;
        assert!(line >= 1 && line <= r.lines.len(), "{r:?}");
        assert!(col >= 1 && col <= r.lines[line - 1].len().max(1), "{r:?}");
        assert!(r.par >= 1);
        let b = r.best.as_str();
        match &r.goal {
            Goal::Line(t) => {
                let reached = match b {
                    "}" => (line + 1..=r.lines.len()).find(|&l| r.lines[l - 1].is_empty()),
                    "{" => (1..line).rev().find(|&l| r.lines[l - 1].is_empty()),
                    _ => {
                        let (n, key) = b.split_at(b.len() - 1);
                        let n: usize = if n.is_empty() { 1 } else { n.parse().unwrap() };
                        Some(if key == "j" { line + n } else { line - n })
                    }
                };
                assert_eq!(reached, Some(*t), "{r:?}");
                assert_eq!(r.par as usize, b.len(), "{r:?}");
            }
            Goal::Cursor(l, c) => {
                let text = &r.lines[l - 1];
                let reached = if let Some(ch) = b.strip_prefix('f') {
                    text[col..].find(ch).map(|i| col + i + 1)
                } else {
                    let n: usize = b.trim_end_matches('w').parse().unwrap_or(1);
                    let starts: Vec<usize> = std::iter::once(1)
                        .chain(text.match_indices(' ').map(|(i, _)| i + 2))
                        .collect();
                    starts.iter().position(|&s| s == col).map(|i| starts[i + n])
                };
                assert_eq!(reached, Some(*c), "{r:?}");
                assert_eq!(r.par as usize, b.len(), "{r:?}");
            }
            Goal::Text(after) => {
                let text = &r.lines[0];
                let done = match b {
                    "D" => text[..col - 1].to_string(),
                    "dt," => {
                        let comma = text[col - 1..].find(',').unwrap() + col - 1;
                        format!("{}{}", &text[..col - 1], &text[comma..])
                    }
                    _ if b.starts_with("di") => {
                        let (open, close) = match &b[2..] {
                            "(" => ('(', ')'),
                            "{" => ('{', '}'),
                            "[" => ('[', ']'),
                            _ => ('"', '"'),
                        };
                        let o = text[..col].rfind(open).unwrap();
                        let c = text[col..].find(close).unwrap() + col;
                        format!("{}{}", &text[..=o], &text[c..])
                    }
                    _ => {
                        let n: usize = b[1..b.len() - 1].parse().unwrap_or(1);
                        let mut end = col - 1;
                        for _ in 0..n {
                            end += text[end..].find(' ').unwrap() + 1;
                        }
                        format!("{}{}", &text[..col - 1], &text[end..])
                    }
                };
                assert_eq!(&vec![done], after, "{r:?}");
                assert_eq!(r.par as usize, b.len(), "{r:?}");
            }
        }
        let (ml, mc, _) = r.mark;
        assert!(ml >= 1 && ml <= r.lines.len() && mc >= 1, "{r:?}");
    }

    #[test]
    fn habits_have_drills() {
        assert_eq!(family_for("vim.repeat-jk"), Some(Family::Vertical));
        assert_eq!(family_for("vim.repeat-x"), Some(Family::Delete));
        assert_eq!(family_for("motion.text-object"), Some(Family::Inside));
        assert_eq!(family_for("vim.w-then-q"), None);
        for f in Family::ALL {
            assert_eq!(Family::parse(f.name()), Some(f));
        }
    }

    #[test]
    fn efficiency() {
        let r = |keys, par| RoundResult {
            keys,
            par,
            ms: 1000,
            best: String::new(),
        };
        let s = Session {
            at: 0,
            family: Family::Vertical,
            rounds: vec![r(2, 2), r(6, 2)],
        };
        assert!((s.efficiency() - 0.5).abs() < 1e-9);
        assert_eq!(s.perfect(), 1);
        assert!((s.seconds() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn drills_are_json() {
        let d = drill(Family::Inside, 2, 1);
        let back: Drill = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(back, d);
    }
}
