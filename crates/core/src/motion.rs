//! Motion coaching: compare a movement with the cheapest motion that would
//! have landed in the same place.
//!
//! An editor plugin reports a [`Motion`] with facts about where it ended
//! (see [`crate::editor`]); the daemon adds how many keys you pressed. From
//! the facts, [`cheapest`] builds the shortest exact plan, such as `}}w`,
//! and the motion rules nudge when it would have saved enough keys.

use crate::context::{ContextKind, VimMode};
use crate::editor::{Motion, MotionFacts, ScreenPos};
use crate::key::Sym;
use crate::rule::{Finding, Rule, RuleEvent, Shape};
use crate::vim::{Command, Step};

/// A movement plus the keys the user pressed to make it.
#[derive(Clone, Debug, PartialEq)]
pub struct MotionEvent {
    pub motion: Motion,
    pub keys_used: u32,
}

/// The kind of motion a plan is built around; one rule each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Count,
    Paragraph,
    Screen,
    File,
    Word,
    Find,
    LineEnds,
    Bracket,
    Star,
    JumpBack,
}

impl Category {
    pub const ALL: [Category; 10] = [
        Category::Count,
        Category::Paragraph,
        Category::Screen,
        Category::File,
        Category::Word,
        Category::Find,
        Category::LineEnds,
        Category::Bracket,
        Category::Star,
        Category::JumpBack,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Category::Count => "motion.count",
            Category::Paragraph => "motion.paragraph",
            Category::Screen => "motion.screen",
            Category::File => "motion.file",
            Category::Word => "motion.word",
            Category::Find => "motion.find",
            Category::LineEnds => "motion.line-ends",
            Category::Bracket => "motion.bracket",
            Category::Star => "motion.star",
            Category::JumpBack => "motion.jump-back",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Category::Count => "Use a count",
            Category::Paragraph => "Jump by paragraph",
            Category::Screen => "Jump within the screen",
            Category::File => "Jump to the start or end",
            Category::Word => "Move by word",
            Category::Find => "Find the character",
            Category::LineEnds => "Jump to the line's start or end",
            Category::Bracket => "Jump to the matching bracket",
            Category::Star => "Search the word under the cursor",
            Category::JumpBack => "Jump back",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Category::Count => {
                "Moving line by line or character by character where a count gets there"
            }
            Category::Paragraph => "Moving line by line where { or } lands on the same line",
            Category::Screen => {
                "Moving to the top, middle or bottom of the window without H, M or L"
            }
            Category::File => "Moving to the first or last line without gg or G",
            Category::Word => "Moving character by character where w, b, e or W land",
            Category::Find => "Moving along a line where f, F, t or T land",
            Category::LineEnds => "Moving to the start or end of a line without 0, ^ or $",
            Category::Bracket => "Moving to a matching bracket without %",
            Category::Star => "Moving to the next use of a word without * or #",
            Category::JumpBack => "Walking back to where you were without Ctrl-o",
        }
    }

    /// Token for the mistake memory's shape, e.g. `keys for }×12`.
    fn shape_token(self) -> &'static str {
        match self {
            Category::Count => "keys for a count",
            Category::Paragraph => "keys for }",
            Category::Screen => "keys for H/M/L",
            Category::File => "keys for gg/G",
            Category::Word => "keys for w/b/e",
            Category::Find => "keys for f/t",
            Category::LineEnds => "keys for 0/^/$",
            Category::Bracket => "keys for %",
            Category::Star => "keys for */#",
            Category::JumpBack => "keys for Ctrl-o",
        }
    }
}

/// The shortest exact way to make a movement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub category: Category,
    /// What to type, e.g. `}}w` or `3f<char>`. Static pieces and numbers.
    pub keys: String,
    pub cost: u32,
}

struct Part {
    category: Category,
    keys: String,
    cost: u32,
}

fn digits(n: u32) -> u32 {
    n.checked_ilog10().map_or(1, |d| d + 1)
}

/// `key` pressed `n` times, or with a count if that's shorter.
fn repeated(category: Category, key: &str, key_cost: u32, n: u32) -> Part {
    let plain = key_cost * n;
    let counted = digits(n) + key_cost;
    if n == 1 || plain <= counted {
        Part {
            category,
            keys: key.repeat(n as usize),
            cost: plain,
        }
    } else {
        Part {
            category,
            keys: format!("{n}{key}"),
            cost: counted,
        }
    }
}

/// `f<char>`, `f<char>;`, or with a count from three on (`3f<char>`).
fn find(category: Category, key: &str, n: u32) -> Part {
    let (keys, cost) = match n {
        1 => (format!("{key}<char>"), 2),
        2 => (format!("{key}<char>;"), 3),
        _ => (format!("{n}{key}<char>"), digits(n) + 2),
    };
    Part {
        category,
        keys,
        cost,
    }
}

fn vertical(lines: i64, f: &MotionFacts) -> Vec<Part> {
    let n = lines.unsigned_abs().min(u64::from(u32::MAX)) as u32;
    let down = lines > 0;
    let mut parts = vec![repeated(
        Category::Count,
        if down { "j" } else { "k" },
        1,
        n,
    )];
    if let Some(k) = f.paragraph_hops {
        parts.push(repeated(
            Category::Paragraph,
            if down { "}" } else { "{" },
            1,
            k,
        ));
    }
    if let Some(pos) = f.screen {
        let key = match pos {
            ScreenPos::Top => "H",
            ScreenPos::Middle => "M",
            ScreenPos::Bottom => "L",
        };
        parts.push(Part {
            category: Category::Screen,
            keys: key.into(),
            cost: 1,
        });
    }
    if f.file_start && !down {
        parts.push(Part {
            category: Category::File,
            keys: "gg".into(),
            cost: 2,
        });
    }
    if f.file_end && down {
        parts.push(Part {
            category: Category::File,
            keys: "G".into(),
            cost: 1,
        });
    }
    parts
}

fn horizontal(lines: i64, cols: i64, f: &MotionFacts) -> Vec<Part> {
    let right = cols > 0;
    let mut parts = Vec::new();
    if lines == 0 {
        let n = cols.unsigned_abs().min(u64::from(u32::MAX)) as u32;
        parts.push(repeated(
            Category::Count,
            if right { "l" } else { "h" },
            1,
            n,
        ));
    }
    let hop = |parts: &mut Vec<Part>, hops: Option<u32>, fwd: &str, back: &str, cost: u32| {
        if let Some(k) = hops {
            parts.push(repeated(
                Category::Word,
                if right { fwd } else { back },
                cost,
                k,
            ));
        }
    };
    hop(&mut parts, f.word_hops, "w", "b", 1);
    hop(&mut parts, f.word_end_hops, "e", "ge", 1);
    hop(&mut parts, f.bigword_hops, "W", "B", 1);
    if let Some(k) = f.find_hops {
        parts.push(find(Category::Find, if right { "f" } else { "F" }, k));
    }
    if let Some(k) = f.till_hops {
        parts.push(find(Category::Find, if right { "t" } else { "T" }, k));
    }
    for (yes, key) in [
        (f.line_start, "0"),
        (f.first_nonblank, "^"),
        (f.line_end, "$"),
    ] {
        if yes {
            parts.push(Part {
                category: Category::LineEnds,
                keys: key.into(),
                cost: 1,
            });
        }
    }
    parts
}

/// Counting characters or lines by eye is hard, so on a tie any other
/// motion wins over a count; otherwise the earlier one.
fn better(a_cost: u32, a: Category, b_cost: u32, b: Category) -> bool {
    (b_cost, b == Category::Count) < (a_cost, a == Category::Count)
}

fn cheapest_part(parts: Vec<Part>) -> Option<Part> {
    parts.into_iter().reduce(|a, b| {
        if better(a.cost, a.category, b.cost, b.category) {
            b
        } else {
            a
        }
    })
}

/// The shortest exact plan for `m`, if the facts allow one.
pub fn cheapest(m: &Motion) -> Option<Plan> {
    let f = &m.facts;
    let mut plans = Vec::new();

    // Whole-position motions.
    let back = m.lines < 0 || (m.lines == 0 && m.cols < 0);
    if f.bracket_match {
        plans.push(Plan {
            category: Category::Bracket,
            keys: "%".into(),
            cost: 1,
        });
    }
    if let Some(k) = f.star_hops {
        let p = repeated(Category::Star, if back { "#" } else { "*" }, 1, k);
        plans.push(Plan {
            category: p.category,
            keys: p.keys,
            cost: p.cost,
        });
    }
    if f.jump_back {
        plans.push(Plan {
            category: Category::JumpBack,
            keys: "Ctrl-o".into(),
            cost: 1,
        });
    }

    // A vertical part, then a part within the end line.
    let v = if m.lines != 0 {
        cheapest_part(vertical(m.lines, f))
    } else {
        Some(Part {
            category: Category::Count,
            keys: String::new(),
            cost: 0,
        })
    };
    // Vertical motions keep the column, so no horizontal part is needed
    // when it didn't change.
    let h = if m.cols != 0 {
        cheapest_part(horizontal(m.lines, m.cols, f))
    } else {
        Some(Part {
            category: Category::Count,
            keys: String::new(),
            cost: 0,
        })
    };
    if let (Some(v), Some(h)) = (v, h) {
        // Name the plan after its most specific part.
        let category = if v.category != Category::Count || m.lines != 0 && h.cost == 0 {
            v.category
        } else {
            h.category
        };
        plans.push(Plan {
            category,
            keys: v.keys + &h.keys,
            cost: v.cost + h.cost,
        });
    }
    plans.into_iter().filter(|p| p.cost > 0).reduce(|a, b| {
        if better(a.cost, a.category, b.cost, b.category) {
            b
        } else {
            a
        }
    })
}

/// Keys a command took, counting each chord (Ctrl-d) as one.
pub fn command_keys(c: &Command) -> u32 {
    c.keys.len() as u32 + c.count.map_or(0, digits)
}

/// Whether a command only moves the cursor.
pub fn is_motion(c: &Command) -> bool {
    let Some(first) = c.first() else { return false };
    if first.mods.ctrl {
        return matches!(
            first.sym,
            Sym::Char('d' | 'u' | 'f' | 'b' | 'e' | 'y' | 'o' | 'i')
        );
    }
    match first.sym {
        Sym::Char(ch) => {
            "hjklwbeWBE0^$fFtT;,{}()%HMLG*#nN'`_+-|".contains(ch)
                || (ch == 'g' && c.keys.len() == 2)
                || (ch == 'z' && c.keys.len() == 2)
        }
        _ => false,
    }
}

const VIM: &[ContextKind] = &[ContextKind::VsCodeVim, ContextKind::TerminalVim];

/// One rule per [`Category`]: nudges when the cheapest plan is of its kind
/// and saves at least `min_saving` keys.
pub struct MotionRule {
    category: Category,
    min_saving: u32,
}

impl MotionRule {
    pub fn new(category: Category) -> Self {
        MotionRule {
            category,
            min_saving: 3,
        }
    }
}

impl Rule for MotionRule {
    fn id(&self) -> &'static str {
        self.category.id()
    }

    fn description(&self) -> &'static str {
        self.category.description()
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn threshold(&self) -> Option<u32> {
        Some(self.min_saving)
    }

    fn set_threshold(&mut self, n: u32) {
        self.min_saving = n.max(1);
    }

    fn needs_editor(&self) -> bool {
        true
    }

    fn on_step(&mut self, _step: &Step, _out: &mut Vec<RuleEvent>) {}

    fn on_motion(&mut self, ev: &MotionEvent, out: &mut Vec<RuleEvent>) {
        if ev.keys_used == 0 {
            return;
        }
        let Some(plan) = cheapest(&ev.motion) else {
            return;
        };
        if plan.category != self.category {
            return;
        }
        let at_ms = ev.motion.end_ms;
        if ev.keys_used >= plan.cost + self.min_saving {
            let used = ev.keys_used;
            out.push(RuleEvent::Finding(Finding {
                rule_id: self.category.id(),
                title: self.category.title(),
                text: format!(
                    "That took {used} keys. {} gets there in {}.",
                    plan.keys, plan.cost
                ),
                shape: Shape::repeat(self.category.shape_token(), used),
                mode: ev.motion.mode.vim_mode().unwrap_or(VimMode::Normal),
                at_ms,
            }));
        } else if ev.keys_used <= plan.cost && plan.cost >= 2 {
            // As short as it gets for a move that needed more than one key.
            out.push(RuleEvent::Adoption {
                rule_id: self.category.id(),
                at_ms,
            });
        }
    }

    fn reset(&mut self) {}
}

pub fn all() -> Vec<Box<dyn Rule>> {
    Category::ALL
        .into_iter()
        .map(|c| Box::new(MotionRule::new(c)) as Box<dyn Rule>)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::EditorMode;

    fn motion(lines: i64, cols: i64, facts: MotionFacts) -> Motion {
        Motion {
            start_ms: 0,
            end_ms: 1000,
            moves: 1,
            mode: EditorMode::Normal,
            lines,
            cols,
            facts,
        }
    }

    fn plan(lines: i64, cols: i64, facts: MotionFacts) -> (Category, String, u32) {
        let p = cheapest(&motion(lines, cols, facts)).unwrap();
        (p.category, p.keys, p.cost)
    }

    #[test]
    fn counts_for_plain_vertical_moves() {
        assert_eq!(
            plan(12, 0, MotionFacts::default()),
            (Category::Count, "12j".into(), 3)
        );
        assert_eq!(
            plan(-2, 0, MotionFacts::default()),
            (Category::Count, "kk".into(), 2)
        );
    }

    #[test]
    fn paragraphs_beat_counts() {
        let f = MotionFacts {
            paragraph_hops: Some(2),
            ..Default::default()
        };
        assert_eq!(plan(14, 0, f), (Category::Paragraph, "}}".into(), 2));
    }

    #[test]
    fn screen_and_file_positions() {
        let f = MotionFacts {
            screen: Some(ScreenPos::Bottom),
            file_end: true,
            ..Default::default()
        };
        // G and L both cost 1; the first found wins.
        assert_eq!(plan(30, 0, f).2, 1);
        let f = MotionFacts {
            file_start: true,
            ..Default::default()
        };
        assert_eq!(plan(-40, 0, f), (Category::File, "gg".into(), 2));
    }

    #[test]
    fn within_a_line() {
        let f = MotionFacts {
            word_hops: Some(12),
            find_hops: Some(1),
            ..Default::default()
        };
        assert_eq!(plan(0, 14, f), (Category::Find, "f<char>".into(), 2));
        let f = MotionFacts {
            word_hops: Some(2),
            ..Default::default()
        };
        assert_eq!(plan(0, -9, f), (Category::Word, "bb".into(), 2));
        let f = MotionFacts {
            line_end: true,
            ..Default::default()
        };
        assert_eq!(plan(0, 30, f), (Category::LineEnds, "$".into(), 1));
    }

    #[test]
    fn find_with_semicolons_or_a_count() {
        assert_eq!(find(Category::Find, "f", 2).keys, "f<char>;");
        // 3f<char> (3 keys) beats f<char>;; (4).
        assert_eq!(find(Category::Find, "f", 3).keys, "3f<char>");
        assert_eq!(find(Category::Find, "f", 3).cost, 3);
        assert_eq!(find(Category::Find, "f", 12).cost, 4);
    }

    #[test]
    fn vertical_then_horizontal() {
        let f = MotionFacts {
            paragraph_hops: Some(1),
            first_nonblank: true,
            ..Default::default()
        };
        assert_eq!(plan(6, 4, f), (Category::Paragraph, "}^".into(), 2));
        // No fact for the column: no exact plan.
        assert!(cheapest(&motion(6, 4, MotionFacts::default())).is_none());
    }

    #[test]
    fn whole_position_motions() {
        let f = MotionFacts {
            bracket_match: true,
            ..Default::default()
        };
        assert_eq!(plan(20, 3, f), (Category::Bracket, "%".into(), 1));
        let f = MotionFacts {
            star_hops: Some(1),
            ..Default::default()
        };
        assert_eq!(plan(-15, 2, f), (Category::Star, "#".into(), 1));
    }

    fn run(keys_used: u32, lines: i64, cols: i64, facts: MotionFacts) -> Vec<RuleEvent> {
        let ev = MotionEvent {
            motion: motion(lines, cols, facts),
            keys_used,
        };
        let mut out = Vec::new();
        for mut rule in all() {
            rule.on_motion(&ev, &mut out);
        }
        out
    }

    #[test]
    fn nudges_only_when_it_saves_enough() {
        let f = MotionFacts {
            paragraph_hops: Some(1),
            ..Default::default()
        };
        let out = run(7, 7, 0, f.clone());
        assert_eq!(out.len(), 1);
        let RuleEvent::Finding(finding) = &out[0] else {
            panic!("{out:?}")
        };
        assert_eq!(finding.rule_id, "motion.paragraph");
        assert_eq!(finding.text, "That took 7 keys. } gets there in 1.");
        assert_eq!(finding.shape.to_string(), "keys for }×7");
        // Saving only 2 keys isn't worth a nudge.
        assert!(run(3, 3, 0, f).is_empty());
    }

    #[test]
    fn the_short_way_counts_as_adoption() {
        let f = MotionFacts {
            paragraph_hops: Some(2),
            ..Default::default()
        };
        assert_eq!(
            run(2, 14, 0, f),
            vec![RuleEvent::Adoption {
                rule_id: "motion.paragraph",
                at_ms: 1000
            }]
        );
        // A single j is just a single j.
        assert!(run(1, 1, 0, MotionFacts::default()).is_empty());
    }

    #[test]
    fn motion_commands() {
        use crate::vim::notation::strokes;
        use crate::vim::{Flavor, Tracker, VimEvent};
        let mut t = Tracker::new(Flavor::Vim);
        t.reset_known(VimMode::Normal);
        let cmds: Vec<Command> = strokes("5jdwfx<C-d>gg", 0)
            .into_iter()
            .filter_map(|k| match t.feed(k).event {
                Some(VimEvent::Command(c)) => Some(c),
                _ => None,
            })
            .collect();
        let kinds: Vec<(bool, u32)> = cmds
            .iter()
            .map(|c| (is_motion(c), command_keys(c)))
            .collect();
        assert_eq!(
            kinds,
            [(true, 2), (false, 2), (true, 2), (true, 1), (true, 2)]
        );
    }
}
