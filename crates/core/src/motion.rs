//! Motion coaching: compare a movement with the cheapest motion that would
//! have landed in the same place.
//!
//! An editor plugin reports a [`Motion`] with facts about where it ended
//! (see [`crate::editor`]); the daemon adds how many keys you pressed. From
//! the facts, [`cheapest`] builds the shortest exact plan, such as `}}w`,
//! and the motion rules nudge when it would have saved enough keys.

use crate::context::{ContextKind, VimMode};
use crate::editor::{Motion, MotionFacts, OperatorReport, ScreenPos};
use crate::key::Sym;
use crate::rule::{Finding, Rule, RuleEvent, Shape};
use crate::vim::{Command, Step};

/// An operator plus the keys the user pressed for it.
#[derive(Clone, Debug, PartialEq)]
pub struct OperatorEvent {
    pub report: OperatorReport,
    pub keys_used: u32,
}

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
    Page,
    Search,
}

impl Category {
    pub const ALL: [Category; 12] = [
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
        Category::Page,
        Category::Search,
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
            Category::Page => "motion.page",
            Category::Search => "motion.search",
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
            Category::Page => "Move by half a page",
            Category::Search => "Repeat the search",
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
            Category::Page => "Moving many lines where Ctrl-d or Ctrl-u land",
            Category::Search => "Moving to the next search match without n or N",
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
            Category::Page => "keys for Ctrl-d/u",
            Category::Search => "keys for n/N",
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

/// Counts up to this are easy to see at a glance.
const GLANCE: u32 = 9;

/// Whether a count of lines can be read off the screen rather than
/// counted: small, or shown by 'relativenumber' for a line in view.
fn readable_lines(n: u32, f: &MotionFacts) -> bool {
    n <= GLANCE || (f.relative_numbers && f.window_lines.is_some_and(|w| n < w))
}

/// `key` pressed `n` times, or with a count if that's shorter and
/// `readable`: nobody types 193j, because nobody counts 193 lines.
fn repeated(category: Category, key: &str, key_cost: u32, n: u32, readable: bool) -> Part {
    let plain = key_cost * n;
    let counted = digits(n) + key_cost;
    if n == 1 || plain <= counted || !readable {
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

/// `f<char>`, `f<char>;`, a readable count (`3f<char>`), or `;` again
/// and again.
fn find(category: Category, key: &str, n: u32) -> Part {
    let (keys, cost) = match n {
        1 => (format!("{key}<char>"), 2),
        2 => (format!("{key}<char>;"), 3),
        _ if n <= GLANCE => (format!("{n}{key}<char>"), digits(n) + 2),
        _ => (format!("{key}<char> then ;×{}", n - 1), n + 1),
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
    let (fwd, back) = if down { ("j", "k") } else { ("k", "j") };
    let mut parts = vec![repeated(Category::Count, fwd, 1, n, readable_lines(n, f))];
    if let Some(k) = f.paragraph_hops {
        parts.push(repeated(
            Category::Paragraph,
            if down { "}" } else { "{" },
            1,
            k,
            k <= GLANCE,
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
    // Ctrl-d and Ctrl-u move by half the window ('scroll'). Page close,
    // then finish with a few lines; the target is on screen by then.
    if let Some(half) = f.window_lines.map(|w| w / 2).filter(|h| *h > 0 && n >= *h) {
        let page = if down { "Ctrl-d" } else { "Ctrl-u" };
        let pages = |k: u32| {
            if k == 1 {
                page.to_string()
            } else {
                format!("{page}×{k}")
            }
        };
        let rest = |key: &str, r: u32| repeated(Category::Count, key, 1, r, readable_lines(r, f));
        let (k, r) = (n / half, n % half);
        let mut options = vec![(k, rest(fwd, r))];
        if r > 0 {
            // One page too far, then back.
            options.push((k + 1, rest(back, half - r)));
        }
        let (k, finish) = options
            .into_iter()
            .min_by_key(|(k, finish)| k + finish.cost)
            .unwrap();
        let keys = if finish.cost == 0 {
            pages(k)
        } else {
            format!("{} then {}", pages(k), finish.keys)
        };
        parts.push(Part {
            category: Category::Page,
            keys,
            cost: k + finish.cost,
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
            n <= GLANCE,
        ));
    }
    let hop = |parts: &mut Vec<Part>, hops: Option<u32>, fwd: &str, back: &str, cost: u32| {
        if let Some(k) = hops {
            parts.push(repeated(
                Category::Word,
                if right { fwd } else { back },
                cost,
                k,
                k <= GLANCE,
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
    plans(m).into_iter().next()
}

/// Every practical exact plan for `m`, best first.
pub fn plans(m: &Motion) -> Vec<Plan> {
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
        let p = repeated(
            Category::Star,
            if back { "#" } else { "*" },
            1,
            k,
            k <= GLANCE,
        );
        plans.push(Plan {
            category: p.category,
            keys: p.keys,
            cost: p.cost,
        });
    }
    if let Some(k) = f.search_hops {
        let p = repeated(Category::Search, "n", 1, k, k <= GLANCE);
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
    let none = || Part {
        category: Category::Count,
        keys: String::new(),
        cost: 0,
    };
    let verticals = if m.lines != 0 {
        vertical(m.lines, f)
    } else {
        vec![none()]
    };
    // Vertical motions keep the column, so no horizontal part is needed
    // when it didn't change.
    let h = if m.cols != 0 {
        cheapest_part(horizontal(m.lines, m.cols, f))
    } else {
        Some(none())
    };
    if let Some(h) = h {
        for v in verticals {
            // Name the plan after its most specific part.
            let category = if v.category != Category::Count || m.lines != 0 && h.cost == 0 {
                v.category
            } else {
                h.category
            };
            let joiner = if v.keys.contains(' ') && !h.keys.is_empty() {
                ", then "
            } else {
                ""
            };
            plans.push(Plan {
                category,
                keys: format!("{}{joiner}{}", v.keys, h.keys),
                cost: v.cost + h.cost,
            });
        }
    }
    plans.retain(|p| p.cost > 0);
    plans.sort_by(|a, b| {
        if better(a.cost, a.category, b.cost, b.category) {
            std::cmp::Ordering::Greater
        } else if better(b.cost, b.category, a.cost, a.category) {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Equal
        }
    });
    plans
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

    fn title(&self) -> &'static str {
        self.category.title()
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
        let all = plans(&ev.motion);
        let Some(plan) = all.first() else {
            return;
        };
        // A second way of a different kind, if it's also worth it.
        let alt = all
            .iter()
            .find(|p| p.category != plan.category && ev.keys_used >= p.cost + self.min_saving);
        if plan.category != self.category {
            return;
        }
        let at_ms = ev.motion.end_ms;
        if ev.keys_used >= plan.cost + self.min_saving {
            let used = ev.keys_used;
            out.push(RuleEvent::Finding(Finding {
                rule_id: self.category.id(),
                title: self.category.title(),
                text: match alt {
                    Some(alt) => format!(
                        "That took {used} keys. {} gets there in {}, or {} in {}.",
                        plan.keys, plan.cost, alt.keys, alt.cost
                    ),
                    None => format!(
                        "That took {used} keys. {} gets there in {}.",
                        plan.keys, plan.cost
                    ),
                },
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

/// Moving the cursor away and back just to scroll the view.
#[derive(Default)]
pub struct ScrollRule {
    /// The previous movement: (lines, scrolled, keys, end).
    last: Option<(i64, i64, u32, u64)>,
}

impl ScrollRule {
    const ID: &'static str = "motion.scroll";
    /// Two movements this close together are one gesture.
    const GESTURE_MS: u64 = 5000;
}

impl Rule for ScrollRule {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Moving the cursor away and back just to scroll the view"
    }

    fn title(&self) -> &'static str {
        "Scroll without moving"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn needs_editor(&self) -> bool {
        true
    }

    fn on_step(&mut self, _step: &Step, _out: &mut Vec<RuleEvent>) {}

    fn on_motion(&mut self, ev: &MotionEvent, out: &mut Vec<RuleEvent>) {
        let m = &ev.motion;
        let scrolled = m.facts.view_scrolled.unwrap_or(0);
        let last = self
            .last
            .replace((m.lines, scrolled, ev.keys_used, m.end_ms));
        let Some((lines, prev_scrolled, prev_keys, end)) = last else {
            return;
        };
        let view = prev_scrolled + scrolled;
        let there_and_back = lines.signum() == -m.lines.signum() && (lines + m.lines).abs() <= 3;
        let used = prev_keys + ev.keys_used;
        if m.start_ms.saturating_sub(end) > Self::GESTURE_MS
            || !there_and_back
            || view.abs() < 3
            || used < 6
        {
            return;
        }
        self.last = None;
        let v = view.unsigned_abs();
        let key = if view > 0 { "Ctrl-e" } else { "Ctrl-y" };
        out.push(RuleEvent::Finding(Finding {
            rule_id: Self::ID,
            title: "Scroll without moving",
            text: format!(
                "That took {used} keys to scroll {v} lines and come back. {key} scrolls a line without moving the cursor; zt, zz and zb put this line at the top, middle or bottom."
            ),
            shape: Shape::repeat("keys to scroll", used),
            mode: m.mode.vim_mode().unwrap_or(VimMode::Normal),
            at_ms: m.end_ms,
        }));
    }

    fn reset(&mut self) {
        self.last = None;
    }
}

/// Walking back to the same line again and again where a mark jumps.
#[derive(Default)]
pub struct MarkRule;

impl MarkRule {
    const ID: &'static str = "motion.mark";
    /// This many earlier visits make it a habit worth a mark.
    const VISITS: u32 = 2;
}

impl Rule for MarkRule {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Moving back to the same line again and again without a mark"
    }

    fn title(&self) -> &'static str {
        "Set a mark"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn needs_editor(&self) -> bool {
        true
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        // Jumping to a mark: ' or ` and a letter.
        if crate::rules::vim::command(step)
            .is_some_and(|c| c.keys.len() == 2 && c.head().is_some_and(|h| h == '\'' || h == '`'))
        {
            out.push(RuleEvent::Adoption {
                rule_id: Self::ID,
                at_ms: step.at_ms,
            });
        }
    }

    fn on_motion(&mut self, ev: &MotionEvent, out: &mut Vec<RuleEvent>) {
        let m = &ev.motion;
        let Some(visits) = m.facts.revisits.filter(|v| *v >= Self::VISITS) else {
            return;
        };
        // A jump that's already short (a mark, '', Ctrl-o) is fine.
        if ev.keys_used < 5 || m.lines.unsigned_abs() < 5 {
            return;
        }
        let used = ev.keys_used;
        out.push(RuleEvent::Finding(Finding {
            rule_id: Self::ID,
            title: "Set a mark",
            text: format!(
                "Back to this line for the {} time, {used} keys this trip. Mark it with ma once, then 'a jumps here in 2 (`a for the exact spot).",
                ordinal(visits + 1)
            ),
            shape: Shape::repeat("keys back to a line", used),
            mode: m.mode.vim_mode().unwrap_or(VimMode::Normal),
            at_ms: m.end_ms,
        }));
    }

    fn reset(&mut self) {}
}

fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// Positioning and then operating where a text object does both.
#[derive(Default)]
pub struct TextObjectRule;

impl TextObjectRule {
    const ID: &'static str = "motion.text-object";
    const MIN_SAVING: u32 = 2;
}

impl Rule for TextObjectRule {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Moving into place and then operating where a text object (iw, i(, ip) does it from anywhere inside"
    }

    fn title(&self) -> &'static str {
        "Use a text object"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn needs_editor(&self) -> bool {
        true
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        // An operator with a text object: d, c or y, then i or a and one key.
        if crate::rules::vim::command(step).is_some_and(|c| {
            c.keys.len() == 3
                && c.head().is_some_and(|h| matches!(h, 'd' | 'c' | 'y'))
                && c.keys[1].plain_char().is_some_and(|k| k == 'i' || k == 'a')
        }) {
            out.push(RuleEvent::Adoption {
                rule_id: Self::ID,
                at_ms: step.at_ms,
            });
        }
    }

    fn on_operator(&mut self, ev: &OperatorEvent, out: &mut Vec<RuleEvent>) {
        let Some(object) = ev.report.objects.first() else {
            return;
        };
        let cost = 1 + object.keys().len() as u32;
        let used = ev.keys_used;
        if used < cost + Self::MIN_SAVING {
            return;
        }
        let better = format!("{}{}", ev.report.op.key(), object.keys());
        out.push(RuleEvent::Finding(Finding {
            rule_id: Self::ID,
            title: "Use a text object",
            text: format!(
                "That took {used} keys. {better} does it from anywhere inside, in {cost}."
            ),
            shape: Shape::repeat("keys for a text object", used),
            mode: VimMode::Normal,
            at_ms: ev.report.at_ms,
        }));
    }

    fn reset(&mut self) {}
}

pub fn all() -> Vec<Box<dyn Rule>> {
    let mut rules: Vec<Box<dyn Rule>> = Category::ALL
        .into_iter()
        .map(|c| Box::new(MotionRule::new(c)) as Box<dyn Rule>)
        .collect();
    rules.push(Box::new(ScrollRule::default()));
    rules.push(Box::new(MarkRule));
    rules.push(Box::new(TextObjectRule));
    rules
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
            plan(7, 0, MotionFacts::default()),
            (Category::Count, "7j".into(), 2)
        );
        assert_eq!(
            plan(-2, 0, MotionFacts::default()),
            (Category::Count, "kk".into(), 2)
        );
    }

    #[test]
    fn counts_only_when_they_can_be_read() {
        // Nobody counts 12 lines by eye...
        let p = plan(12, 0, MotionFacts::default());
        assert_eq!((p.0, p.2), (Category::Count, 12));
        // ...but 'relativenumber' shows it when the line is on screen.
        let f = MotionFacts {
            relative_numbers: true,
            window_lines: Some(40),
            ..Default::default()
        };
        assert_eq!(plan(12, 0, f.clone()), (Category::Count, "12j".into(), 3));
        // Off screen it can't be read either.
        assert_ne!(plan(45, 0, f).1, "45j");
    }

    #[test]
    fn long_moves_page() {
        let f = MotionFacts {
            window_lines: Some(40),
            ..Default::default()
        };
        // 193 = 10 half pages less 7 lines.
        assert_eq!(
            plan(193, 0, f.clone()),
            (Category::Page, "Ctrl-d×10 then 7k".into(), 12)
        );
        assert_eq!(plan(-60, 0, f), (Category::Page, "Ctrl-u×3".into(), 3));
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
        // Nobody counts 12 commas: f then ; until there.
        assert_eq!(find(Category::Find, "f", 12).keys, "f<char> then ;×11");
        assert_eq!(find(Category::Find, "f", 12).cost, 13);
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

    #[test]
    fn half_pages_and_search() {
        let f = MotionFacts {
            window_lines: Some(40),
            ..Default::default()
        };
        assert_eq!(
            plan(40, 0, f.clone()),
            (Category::Page, "Ctrl-d×2".into(), 2)
        );
        assert_eq!(
            plan(-20, 0, f.clone()),
            (Category::Page, "Ctrl-u".into(), 1)
        );
        // Not a multiple of half a page: page, then finish.
        assert_eq!(plan(25, 0, f).1, "Ctrl-d then 5j");
        let f = MotionFacts {
            search_hops: Some(2),
            ..Default::default()
        };
        assert_eq!(plan(30, 5, f), (Category::Search, "nn".into(), 2));
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
        assert_eq!(
            finding.text,
            "That took 7 keys. } gets there in 1, or 7j in 2."
        );
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

    fn ev(keys_used: u32, lines: i64, start_ms: u64, facts: MotionFacts) -> MotionEvent {
        MotionEvent {
            motion: Motion {
                start_ms,
                end_ms: start_ms + 800,
                moves: 1,
                mode: EditorMode::Normal,
                lines,
                cols: 0,
                facts,
            },
            keys_used,
        }
    }

    #[test]
    fn text_objects_instead_of_positioning() {
        use crate::editor::{Operator, TextObject};
        let ev = |keys_used, objects: Vec<TextObject>| OperatorEvent {
            report: OperatorReport {
                op: Operator::Delete,
                at_ms: 9,
                objects,
            },
            keys_used,
        };
        let mut out = Vec::new();
        TextObjectRule.on_operator(
            &ev(6, vec![TextObject::InnerParen, TextObject::AParen]),
            &mut out,
        );
        let RuleEvent::Finding(f) = &out[0] else {
            panic!("{out:?}")
        };
        assert_eq!(
            f.text,
            "That took 6 keys. di( does it from anywhere inside, in 3."
        );
        // Already short, or no text object matched.
        let mut out = Vec::new();
        TextObjectRule.on_operator(&ev(3, vec![TextObject::InnerParen]), &mut out);
        TextObjectRule.on_operator(&ev(9, vec![]), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn scrolling_by_moving_away_and_back() {
        let mut r = ScrollRule::default();
        let mut out = Vec::new();
        let scrolled = |v| MotionFacts {
            view_scrolled: Some(v),
            ..Default::default()
        };
        r.on_motion(&ev(20, 20, 0, scrolled(12)), &mut out);
        r.on_motion(&ev(19, -19, 1500, scrolled(0)), &mut out);
        let RuleEvent::Finding(f) = &out[0] else {
            panic!("{out:?}")
        };
        assert_eq!(
            f.text,
            "That took 39 keys to scroll 12 lines and come back. Ctrl-e scrolls a line without moving the cursor; zt, zz and zb put this line at the top, middle or bottom."
        );
        // Going somewhere else isn't scrolling.
        let mut out = Vec::new();
        let mut r = ScrollRule::default();
        r.on_motion(&ev(20, 20, 0, scrolled(12)), &mut out);
        r.on_motion(&ev(5, 5, 1500, scrolled(5)), &mut out);
        assert!(out.is_empty());
        // Nor is coming back much later.
        r.on_motion(&ev(25, -25, 60_000, scrolled(-5)), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn marks_for_lines_you_keep_coming_back_to() {
        let mut out = Vec::new();
        let revisits = |n| MotionFacts {
            revisits: Some(n),
            ..Default::default()
        };
        MarkRule.on_motion(&ev(14, -14, 0, revisits(1)), &mut out);
        assert!(out.is_empty(), "the second visit isn't a habit yet");
        MarkRule.on_motion(&ev(14, -14, 0, revisits(2)), &mut out);
        let RuleEvent::Finding(f) = &out[0] else {
            panic!("{out:?}")
        };
        assert_eq!(
            f.text,
            "Back to this line for the 3rd time, 14 keys this trip. Mark it with ma once, then 'a jumps here in 2 (`a for the exact spot)."
        );
        // A short trip is fine.
        let mut out = Vec::new();
        MarkRule.on_motion(&ev(2, -30, 0, revisits(4)), &mut out);
        assert!(out.is_empty());
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
