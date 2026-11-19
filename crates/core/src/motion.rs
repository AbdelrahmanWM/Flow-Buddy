//! Motion coaching: compare a movement with the cheapest motion that would
//! have landed in the same place.
//!
//! An editor plugin reports a [`Motion`] with facts about where it ended
//! (see [`crate::editor`]); the daemon adds how many keys you pressed. From
//! the facts, [`cheapest`] builds the shortest exact plan, such as `}}w`,
//! and the motion rules nudge when it would have saved enough keys.

use crate::context::{ContextKind, VimMode};
use crate::curriculum::Known;
use crate::editor::{Motion, MotionFacts, OperatorReport, ScreenPos};
use crate::key::Sym;
use crate::rule::{Finding, Rule, RuleEvent, Shape};
use crate::vim::{Command, Step};

/// An operator plus the keys the user pressed for it.
#[derive(Clone, Debug, PartialEq)]
pub struct OperatorEvent {
    pub report: OperatorReport,
    /// The operator's motion went backwards (`db`, `dF`): the cursor was
    /// just after the range.
    pub backward: bool,
    /// Keys for the operator and anything that moved into place first.
    pub keys_used: u32,
    /// Keys for the operator command alone (`d3w`: 3).
    pub op_keys: u32,
}

/// A movement plus the keys the user pressed to make it.
#[derive(Clone, Debug, PartialEq)]
pub struct MotionEvent {
    pub motion: Motion,
    pub keys_used: u32,
    /// The mouse made this move: a click, and no keys.
    pub clicked: bool,
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
    LastChange,
    LineNumber,
    Section,
    Unmatched,
    Sentence,
    ScreenLine,
    SearchTo,
}

impl Category {
    pub const ALL: [Category; 19] = [
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
        Category::LastChange,
        Category::LineNumber,
        Category::Section,
        Category::Unmatched,
        Category::Sentence,
        Category::ScreenLine,
        Category::SearchTo,
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
            Category::LastChange => "motion.last-change",
            Category::LineNumber => "motion.line-number",
            Category::Section => "motion.section",
            Category::Unmatched => "motion.unmatched",
            Category::Sentence => "motion.sentence",
            Category::ScreenLine => "motion.screen-line",
            Category::SearchTo => "motion.search-to",
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
            Category::LastChange => "Jump to the last change",
            Category::LineNumber => "Jump to the line number",
            Category::Section => "Jump by section",
            Category::Unmatched => "Jump to the enclosing bracket",
            Category::Sentence => "Move by sentence",
            Category::ScreenLine => "Move by screen line",
            Category::SearchTo => "Search for it",
        }
    }

    /// The skill-tree item a plan of this kind exercises; `keys` tells
    /// `w` from `e` from `W`, and `f` from `t`.
    fn skill(self, keys: &str) -> &'static str {
        match self {
            Category::Count => "count",
            Category::Paragraph => "paragraph",
            Category::Screen if keys == "M" => "screen-middle",
            Category::Screen => "screen",
            Category::File => "file-ends",
            Category::Word if keys.contains(['W', 'B']) => "bigword",
            Category::Word if keys.contains('e') => "word-end",
            Category::Word => "word",
            Category::Find if keys.contains(['t', 'T']) => "till",
            Category::Find => "find",
            Category::LineEnds if keys.ends_with('0') => "line-start",
            Category::LineEnds if keys.ends_with('^') => "first-nonblank",
            Category::LineEnds => "line-ends",
            Category::Bracket => "percent",
            Category::Star if keys.contains('#') => "star-back",
            Category::Star => "star",
            Category::JumpBack => "jump-back",
            Category::Page => "half-page",
            Category::Search => "search-next",
            Category::LastChange => "change-list",
            Category::LineNumber => "goto-line",
            Category::Section => "section",
            Category::Unmatched => "unmatched",
            Category::Sentence => "sentence-motion",
            Category::ScreenLine => "screen-lines",
            Category::SearchTo => "search",
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
            Category::LastChange => "Moving back to where you last changed something without g;",
            Category::LineNumber => "Moving far to a line whose number is on screen without 42G",
            Category::Section => "Moving line by line to the next function or block where ]] lands",
            Category::Unmatched => "Moving to the bracket around the cursor without [( or ])",
            Category::Sentence => "Moving word by word through prose where ( or ) lands",
            Category::ScreenLine => "Moving through a wrapped line without gj or gk",
            Category::SearchTo => "Moving far to text you can see without searching for it",
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
            Category::LastChange => "keys for g;",
            Category::LineNumber => "keys for 42G",
            Category::Section => "keys for ]]",
            Category::Unmatched => "keys for [(",
            Category::Sentence => "keys for )",
            Category::ScreenLine => "keys for gj",
            Category::SearchTo => "keys for /",
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

impl Plan {
    pub fn skill(&self) -> &'static str {
        self.category.skill(&self.keys)
    }
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
    // Blank lines then a few more: `}}` then `2j`, or one too far and back.
    if let Some(bounds) = &f.paragraph_lines {
        let (para, back) = if down { ("}", "k") } else { ("{", "j") };
        let best = bounds
            .iter()
            .take(GLANCE as usize)
            .enumerate()
            .filter_map(|(i, &d)| {
                let k = i as u32 + 1;
                let (key, r) = if d <= n { (fwd, n - d) } else { (back, d - n) };
                (r > 0).then(|| {
                    (
                        k,
                        repeated(Category::Count, key, 1, r, readable_lines(r, f)),
                    )
                })
            })
            .min_by_key(|(k, rest)| k + rest.cost);
        if let Some((k, rest)) = best {
            let paras = if k == 1 {
                para.to_string()
            } else {
                format!("{para}×{k}")
            };
            parts.push(Part {
                category: Category::Paragraph,
                keys: format!("{paras} then {}", rest.keys),
                cost: k + rest.cost,
            });
        }
    }
    if let Some(k) = f.section_hops {
        parts.push(repeated(
            Category::Section,
            if down { "]]" } else { "[[" },
            2,
            k,
            k <= GLANCE,
        ));
    }
    if let Some(k) = f.screen_line_hops {
        parts.push(repeated(
            Category::ScreenLine,
            if down { "gj" } else { "gk" },
            2,
            k,
            k <= GLANCE,
        ));
    }
    // The number is right there in the margin: type it.
    if let Some(d) = f.line_number_digits.filter(|_| n > GLANCE) {
        parts.push(Part {
            category: Category::LineNumber,
            keys: "<line>G".into(),
            cost: d + 1,
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

/// How much effort typing `keys` takes, in half-keys: each key is 2,
/// and a shifted key (`}`, `$`, `G`), a `Ctrl` chord, or a count digit
/// (counted by eye) is 1 more. `cost` is the plan's key count.
pub fn effort(keys: &str, cost: u32) -> u32 {
    let mut extra = 0;
    for word in keys.split_whitespace() {
        if matches!(word, "then" | "or" | "and") {
            continue;
        }
        let word = word.trim_end_matches(',');
        let (cmd, times) = match word.split_once('×') {
            Some((c, n)) => (c, n.parse().unwrap_or(1)),
            None => (word, 1),
        };
        let chords = cmd.matches("Ctrl-").count() as u32;
        let rest = cmd
            .replace("Ctrl-", "")
            .replace("<char>", "")
            .replace("<text>", "");
        let line = u32::from(rest.contains("<line>")) * 2;
        let rest = rest.replace("<line>", "");
        let shifted = rest
            .chars()
            .filter(|c| c.is_ascii_uppercase() || "{}$%^~#*:()?_+<>|\"&!@".contains(*c))
            .count() as u32;
        let digits = rest
            .chars()
            .filter(|c| c.is_ascii_digit() && *c != '0')
            .count() as u32;
        extra += (chords + shifted + digits + line) * times;
    }
    2 * cost + extra
}

/// Less effort wins. Counting characters or lines by eye is hard, so on a
/// tie any other motion wins over a count; otherwise the earlier one.
fn better(a_effort: u32, a: Category, b_effort: u32, b: Category) -> bool {
    (b_effort, b == Category::Count) < (a_effort, a == Category::Count)
}

fn cheapest_part(parts: Vec<Part>) -> Option<Part> {
    parts.into_iter().reduce(|a, b| {
        if better(
            effort(&a.keys, a.cost),
            a.category,
            effort(&b.keys, b.cost),
            b.category,
        ) {
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
    plans_for(m, &Known::everything())
}

/// Every practical exact plan for `m` made only of what `known` is ready
/// for, best first.
pub fn plans_for(m: &Motion, known: &Known) -> Vec<Plan> {
    let ready = |p: &Part| known.ready(p.category.skill(&p.keys));
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
    if f.unmatched_paren {
        plans.push(Plan {
            category: Category::Unmatched,
            keys: if back { "[(" } else { "])" }.into(),
            cost: 2,
        });
    }
    if let Some(k) = f.sentence_hops {
        let p = repeated(
            Category::Sentence,
            if back { "(" } else { ")" },
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
    // Type a few letters of what's there: `/`, them, Enter.
    if let Some(c) = f.search_chars {
        plans.push(Plan {
            category: Category::SearchTo,
            keys: if back { "?<text>" } else { "/<text>" }.into(),
            cost: c + 2,
        });
    }
    if f.last_change && m.lines != 0 {
        plans.push(Plan {
            category: Category::LastChange,
            keys: "g;".into(),
            cost: 2,
        });
    }

    // A vertical part, then a part within the end line.
    let none = || Part {
        category: Category::Count,
        keys: String::new(),
        cost: 0,
    };
    let verticals = if m.lines != 0 {
        let mut v = vertical(m.lines, f);
        v.retain(ready);
        v
    } else {
        vec![none()]
    };
    // Vertical motions keep the column, so no horizontal part is needed
    // when it didn't change.
    let h = if m.cols != 0 {
        cheapest_part(
            horizontal(m.lines, m.cols, f)
                .into_iter()
                .filter(ready)
                .collect(),
        )
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
            let joiner =
                if (v.keys.contains(' ') || v.keys.contains("<line>")) && !h.keys.is_empty() {
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
    plans.retain(|p| p.cost > 0 && known.ready(p.skill()));
    // Without punctuation, a blank line is where a sentence ends too: `)`
    // only lands there because `}` does, so don't offer it.
    let paragraph = plans
        .iter()
        .filter(|p| p.category == Category::Paragraph)
        .map(|p| effort(&p.keys, p.cost))
        .min();
    if let Some(e) = paragraph {
        plans.retain(|p| p.category != Category::Sentence || effort(&p.keys, p.cost) < e);
    }
    plans.sort_by(|a, b| {
        let (ea, eb) = (effort(&a.keys, a.cost), effort(&b.keys, b.cost));
        if better(ea, a.category, eb, b.category) {
            std::cmp::Ordering::Greater
        } else if better(eb, b.category, ea, a.category) {
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

/// A plan's keys with each command marked: `Ctrl-d`×10 then `7k`.
pub fn marked(keys: &str) -> String {
    keys.split(' ')
        .map(|word| {
            if matches!(word, "then" | "") {
                return word.to_string();
            }
            let (word, comma) = match word.strip_suffix(',') {
                Some(w) => (w, ","),
                None => (word, ""),
            };
            match word.split_once('×') {
                Some((cmd, times)) => format!("{}×{times}{comma}", crate::glossary::mark(cmd)),
                None => format!("{}{comma}", crate::glossary::mark(word)),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// One rule per [`Category`]: nudges when the cheapest plan is of its kind
/// and saves at least `min_saving` keys.
pub struct MotionRule {
    category: Category,
    min_saving: u32,
    known: Known,
}

impl MotionRule {
    /// Keys a plan must save to be worth a tip: the threshold, or a quarter
    /// of a long move (a key or two off forty isn't worth breaking flow).
    fn saving(&self, used: u32) -> u32 {
        self.min_saving.max(used / 4)
    }

    pub fn new(category: Category) -> Self {
        MotionRule {
            category,
            min_saving: 3,
            known: Known::everything(),
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
        // Slow and steady: reading through, not trying to get somewhere.
        let m = &ev.motion;
        if m.moves >= 3
            && m.end_ms.saturating_sub(m.start_ms) / u64::from(m.moves - 1)
                >= crate::rules::repeat::READING_MS
        {
            return;
        }
        // Only what you're ready for: a plan a step too far gives way to
        // the next best.
        let all = plans_for(&ev.motion, &self.known);
        let Some(plan) = all.first() else {
            return;
        };
        // A second way of a different kind, if it's also worth it.
        let alt = all.iter().find(|p| {
            p.category != plan.category && ev.keys_used >= p.cost + self.saving(ev.keys_used)
        });
        if plan.category != self.category {
            return;
        }
        let at_ms = ev.motion.end_ms;
        if ev.keys_used >= plan.cost + self.saving(ev.keys_used) {
            let used = ev.keys_used;
            out.push(RuleEvent::Finding(Finding {
                rule_id: self.category.id(),
                title: self.category.title(),
                text: match alt {
                    Some(alt) => format!(
                        "That took {used} keys. {} gets there in {}, or {} in {}.",
                        marked(&plan.keys),
                        plan.cost,
                        marked(&alt.keys),
                        alt.cost
                    ),
                    None => format!(
                        "That took {used} keys. {} gets there in {}.",
                        marked(&plan.keys),
                        plan.cost
                    ),
                },
                shape: Shape::repeat(self.category.shape_token(), used),
                mode: ev.motion.mode.vim_mode().unwrap_or(VimMode::Normal),
                at_ms,
                start_ms: ev.motion.start_ms,
            }));
        } else if ev.keys_used <= plan.cost && plan.cost >= 2 {
            // As short as it gets for a move that needed more than one key.
            out.push(RuleEvent::Adoption {
                rule_id: self.category.id(),
                at_ms,
            });
        }
    }

    fn set_known(&mut self, known: &Known) {
        self.known = known.clone();
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
                "That took {used} keys to scroll {v} lines and come back. `{key}` scrolls a line without moving the cursor; `zt`, `zz` and `zb` put this line at the top, middle or bottom."
            ),
            shape: Shape::repeat("keys to scroll", used),
            mode: m.mode.vim_mode().unwrap_or(VimMode::Normal),
            at_ms: m.end_ms,
            start_ms: m.start_ms,
        }));
    }

    fn reset(&mut self) {
        self.last = None;
    }
}

/// Clicking to move in Vim: usually a sign you didn't know the keys, so
/// they're named, worked out from where the click landed.
#[derive(Default)]
pub struct ClickRule {
    known: Known,
}

impl ClickRule {
    pub const ID: &'static str = "motion.mouse";
    /// Past this, the keys aren't much of an answer to a click.
    const MAX_COST: u32 = 8;
}

impl Rule for ClickRule {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Clicking to move the cursor in Vim"
    }

    fn title(&self) -> &'static str {
        "The keys for that click"
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
        if !ev.clicked || m.mode != crate::editor::EditorMode::Normal {
            return;
        }
        let all = plans_for(m, &self.known);
        let Some(plan) = all.first().filter(|p| p.cost <= Self::MAX_COST) else {
            return;
        };
        let alt = all
            .iter()
            .find(|p| p.category != plan.category && p.cost <= plan.cost + 2);
        let text = match alt {
            Some(alt) => format!(
                "You clicked to get there. From the keyboard: {} ({}), or {} ({}).",
                marked(&plan.keys),
                keys(plan.cost),
                marked(&alt.keys),
                alt.cost
            ),
            None => format!(
                "You clicked to get there. From the keyboard: {} ({}).",
                marked(&plan.keys),
                keys(plan.cost)
            ),
        };
        out.push(RuleEvent::Finding(Finding {
            rule_id: Self::ID,
            title: self.title(),
            text,
            shape: Shape::fixed("a click to move"),
            mode: VimMode::Normal,
            at_ms: m.end_ms,
            start_ms: m.start_ms,
        }));
    }

    fn set_known(&mut self, known: &Known) {
        self.known = known.clone();
    }

    fn reset(&mut self) {}
}

/// "1 key", "3 keys".
fn keys(n: u32) -> String {
    if n == 1 {
        "1 key".into()
    } else {
        format!("{n} keys")
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
                "Back to this line for the {} time, {used} keys this trip. Mark it with `ma` once, then `'a` jumps here in 2 (`` `a `` for the exact spot).",
                ordinal(visits + 1)
            ),
            shape: Shape::repeat("keys back to a line", used),
            mode: m.mode.vim_mode().unwrap_or(VimMode::Normal),
            at_ms: m.end_ms,
            start_ms: m.start_ms,
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
pub struct TextObjectRule {
    known: Known,
}

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
        let Some(object) = ev
            .report
            .objects
            .iter()
            .find(|o| self.known.ready(crate::curriculum::object_skill(o.keys())))
        else {
            return;
        };
        let cost = 1 + object.keys().len() as u32;
        let used = ev.keys_used;
        if used < cost + Self::MIN_SAVING {
            return;
        }
        let better = crate::glossary::mark(&format!("{}{}", ev.report.op.key(), object.keys()));
        out.push(RuleEvent::Finding(Finding {
            rule_id: Self::ID,
            title: "Use a text object",
            text: format!(
                "That took {used} keys. {better} does it from anywhere inside, in {cost}."
            ),
            shape: Shape::repeat("keys for a text object", used),
            mode: VimMode::Normal,
            at_ms: ev.report.at_ms,
            start_ms: ev.report.at_ms,
        }));
    }

    fn set_known(&mut self, known: &Known) {
        self.known = known.clone();
    }

    fn reset(&mut self) {}
}

/// An operator whose motion took more keys than one that covers the same
/// range exactly: `d4l` where `de` does it. Needs `[text] shapes`.
#[derive(Default)]
pub struct OperatorMotionRule {
    known: Known,
}

impl OperatorMotionRule {
    const ID: &'static str = "motion.operator";
    const MIN_SAVING: u32 = 2;
}

/// Ways to cover an operator's range, as (keys, cost, skill), best first.
pub fn range_plans(
    op: crate::editor::Operator,
    range: &crate::editor::OperatorRange,
    backward: bool,
) -> Vec<(String, u32, &'static str)> {
    use crate::editor::Operator;
    let o = op.key();
    let f = &range.facts;
    // `motion` takes `cost` keys (`t<char>`: 2), plus a count if n > 1.
    let counted = |n: u32, motion: &str, cost: u32| {
        if n == 1 {
            (format!("{o}{motion}"), 1 + cost)
        } else {
            (format!("{o}{n}{motion}"), 1 + digits(n) + cost)
        }
    };
    let mut out = Vec::new();
    if range.linewise {
        let n = range.lines.max(0) as u32 + 1;
        if f.paragraph_hops == Some(1) {
            out.push((format!("{o}}}"), 2, "paragraph"));
        }
        if f.file_end {
            out.push((format!("{o}G"), 2, "file-ends"));
        }
        if n <= GLANCE {
            let (k, c) = if n == 1 {
                (format!("{o}{o}"), 2)
            } else {
                (format!("{n}{o}{o}"), digits(n) + 2)
            };
            out.push((k, c, "count"));
        }
        out.sort_by_key(|(_, c, _)| *c);
        return out;
    }
    // From just after the range, going back: only if that's where you were.
    if backward {
        if let Some(b) = &range.back {
            let mut add = |n: Option<u32>, motion: &str, cost: u32, skill: &'static str| {
                if let Some(n) = n.filter(|n| *n <= GLANCE) {
                    let (k, c) = counted(n, motion, cost);
                    out.push((k, c, skill));
                }
            };
            add(b.words, "b", 1, "word");
            add(b.bigwords, "B", 1, "bigword");
            add(b.find_hops, "F<char>", 2, "find");
            add(b.till_hops, "T<char>", 2, "till");
            if b.line_start {
                out.push((format!("{o}0"), 2, "line-start"));
            }
            if b.first_nonblank {
                out.push((format!("{o}^"), 2, "first-nonblank"));
            }
        }
        out.sort_by_key(|(_, c, _)| *c);
        return out;
    }
    if range.bracket {
        out.push((format!("{o}%"), 2, "percent"));
    }
    if f.line_end {
        match op {
            Operator::Delete => out.push(("D".to_string(), 1, "d-to-end")),
            Operator::Change => out.push(("C".to_string(), 1, "d-to-end")),
            Operator::Yank => out.push((format!("{o}$"), 2, "line-ends")),
        }
    }
    // `cw` works like `ce` (to the end of the word, not the next one), so
    // a change's `w` comes from the word ends.
    if op == Operator::Change {
        if let Some(n) = f.word_end_hops.filter(|n| *n <= GLANCE) {
            let (k, c) = counted(n, "w", 1);
            out.push((k, c, "change-word"));
        }
        if let Some(n) = range.bigword_ends.filter(|n| *n <= GLANCE) {
            let (k, c) = counted(n, "W", 1);
            out.push((k, c, "bigword"));
        }
    } else {
        if let Some(n) = range.words.filter(|n| *n <= GLANCE) {
            let (k, c) = counted(n, "w", 1);
            out.push((k, c, "word"));
        }
        if let Some(n) = f.word_end_hops.filter(|n| *n <= GLANCE) {
            let (k, c) = counted(n, "e", 1);
            out.push((k, c, "word-end"));
        }
        if let Some(n) = range.bigwords.filter(|n| *n <= GLANCE) {
            let (k, c) = counted(n, "W", 1);
            out.push((k, c, "bigword"));
        }
        if let Some(n) = range.bigword_ends.filter(|n| *n <= GLANCE) {
            let (k, c) = counted(n, "E", 1);
            out.push((k, c, "bigword"));
        }
    }
    if let Some(n) = f.till_hops.filter(|n| *n <= GLANCE) {
        let (k, c) = counted(n, "t<char>", 2);
        out.push((k, c, "till"));
    }
    if let Some(n) = f.find_hops.filter(|n| *n <= GLANCE) {
        let (k, c) = counted(n, "f<char>", 2);
        out.push((k, c, "find"));
    }
    // Cheapest first; on a tie the earlier (simpler) one.
    out.sort_by_key(|(_, c, _)| *c);
    out
}

impl Rule for OperatorMotionRule {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "An operator with a longer motion than one that covers the same text (d4l for de)"
    }

    fn title(&self) -> &'static str {
        "A shorter motion"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn needs_editor(&self) -> bool {
        true
    }

    fn on_step(&mut self, _step: &Step, _out: &mut Vec<RuleEvent>) {}

    fn on_operator(&mut self, ev: &OperatorEvent, out: &mut Vec<RuleEvent>) {
        let Some(range) = &ev.report.range else {
            return;
        };
        let used = ev.op_keys;
        let Some((keys, cost, _)) = range_plans(ev.report.op, range, ev.backward)
            .into_iter()
            .find(|(_, _, skill)| self.known.ready(skill))
        else {
            return;
        };
        if used == 0 || used < cost + Self::MIN_SAVING {
            return;
        }
        out.push(RuleEvent::Finding(Finding {
            rule_id: Self::ID,
            title: "A shorter motion",
            text: format!(
                "That took {used} keys. {} covers the same text in {cost}.",
                crate::glossary::mark(&keys)
            ),
            shape: Shape::repeat("keys for an operator's motion", used),
            mode: VimMode::Normal,
            at_ms: ev.report.at_ms,
            start_ms: ev.report.at_ms,
        }));
    }

    fn set_known(&mut self, known: &Known) {
        self.known = known.clone();
    }

    fn reset(&mut self) {}
}

/// The best ways to make a movement or an edit, for `flowbuddy explain
/// --last`: (keys, cost), best first, only ones you're ready for.
pub fn best_for_motion(ev: &MotionEvent, known: &Known) -> Vec<(String, u32)> {
    plans_for(&ev.motion, known)
        .into_iter()
        .map(|p| (p.keys, p.cost))
        .collect()
}

/// For an edit: a text object from anywhere inside it (against all the
/// keys used), and the operator's motions from where it started.
pub fn best_for_edit(ev: &OperatorEvent, known: &Known) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = ev
        .report
        .objects
        .iter()
        .filter(|o| known.ready(crate::curriculum::object_skill(o.keys())))
        .map(|o| {
            (
                format!("{}{}", ev.report.op.key(), o.keys()),
                1 + o.keys().len() as u32,
            )
        })
        .take(1)
        .collect();
    if let Some(range) = &ev.report.range {
        out.extend(
            range_plans(ev.report.op, range, ev.backward)
                .into_iter()
                .filter(|(_, _, skill)| known.ready(skill))
                .map(|(k, c, _)| (k, c)),
        );
    }
    out.sort_by_key(|(_, c)| *c);
    out
}

pub fn all() -> Vec<Box<dyn Rule>> {
    let mut rules: Vec<Box<dyn Rule>> = Category::ALL
        .into_iter()
        .map(|c| Box::new(MotionRule::new(c)) as Box<dyn Rule>)
        .collect();
    rules.push(Box::new(ScrollRule::default()));
    rules.push(Box::new(ClickRule::default()));
    rules.push(Box::new(MarkRule));
    rules.push(Box::new(TextObjectRule::default()));
    rules.push(Box::new(OperatorMotionRule::default()));
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
    fn a_blank_line_is_a_paragraph_not_a_sentence() {
        // Lines without full stops: `)` stops at the blank line, as `}` does.
        let f = MotionFacts {
            paragraph_hops: Some(1),
            sentence_hops: Some(1),
            ..Default::default()
        };
        let all = plans(&motion(7, 0, f));
        assert_eq!(all[0].keys, "}");
        assert!(
            all.iter().all(|p| p.category != Category::Sentence),
            "{all:?}"
        );
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
    fn every_plan_is_explained() {
        let facts = MotionFacts {
            paragraph_hops: Some(3),
            screen: Some(ScreenPos::Top),
            window_lines: Some(40),
            file_start: true,
            word_hops: Some(12),
            word_end_hops: Some(2),
            bigword_hops: Some(1),
            find_hops: Some(12),
            till_hops: Some(2),
            line_start: true,
            first_nonblank: true,
            line_end: true,
            bracket_match: true,
            star_hops: Some(2),
            jump_back: true,
            search_hops: Some(3),
            ..Default::default()
        };
        let mut checked = 0;
        for (lines, cols) in [
            (193, 0),
            (-193, 0),
            (-30, 0),
            (5, 3),
            (0, 14),
            (0, -14),
            (-2, -5),
        ] {
            for p in plans(&motion(lines, cols, facts.clone())) {
                for span in crate::glossary::code_spans(&marked(&p.keys)) {
                    checked += 1;
                    assert!(
                        !crate::glossary::explain_keys(span, false).is_empty(),
                        "`{span}` from {:?} has no explanation",
                        p.keys
                    );
                }
            }
        }
        assert!(checked > 40, "{checked}");
        assert_eq!(marked("Ctrl-d×10 then 7k"), "`Ctrl-d`×10 then `7k`");
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
            clicked: false,
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
            crate::glossary::plain(&finding.text),
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
            clicked: false,
        }
    }

    #[test]
    fn text_objects_instead_of_positioning() {
        use crate::editor::{Operator, TextObject};
        let ev = |keys_used, objects: Vec<TextObject>| OperatorEvent {
            backward: false,
            op_keys: 2,
            report: OperatorReport {
                op: Operator::Delete,
                at_ms: 9,
                objects,
                range: None,
            },
            keys_used,
        };
        let mut out = Vec::new();
        TextObjectRule::default().on_operator(
            &ev(6, vec![TextObject::InnerParen, TextObject::AParen]),
            &mut out,
        );
        let RuleEvent::Finding(f) = &out[0] else {
            panic!("{out:?}")
        };
        assert_eq!(
            crate::glossary::plain(&f.text),
            "That took 6 keys. di( does it from anywhere inside, in 3."
        );
        // Already short, or no text object matched.
        let mut out = Vec::new();
        TextObjectRule::default().on_operator(&ev(3, vec![TextObject::InnerParen]), &mut out);
        TextObjectRule::default().on_operator(&ev(9, vec![]), &mut out);
        assert!(out.is_empty());

        // Not `di(` to someone who doesn't use `ciw` yet; `diw` is fine
        // once `cw` is known.
        let mut r = TextObjectRule::default();
        r.set_known(&Known::of(["change-word"]));
        let mut out = Vec::new();
        r.on_operator(&ev(6, vec![TextObject::InnerParen]), &mut out);
        assert!(out.is_empty());
        r.on_operator(
            &ev(6, vec![TextObject::InnerParen, TextObject::InnerWord]),
            &mut out,
        );
        let RuleEvent::Finding(f) = &out[0] else {
            panic!("{out:?}")
        };
        assert!(f.text.contains("`diw`"), "{}", f.text);
    }

    #[test]
    fn plans_wait_for_what_they_build_on() {
        // `t` is one step past `f`: with `f` unknown, `t` gives way.
        let facts = MotionFacts {
            till_hops: Some(1),
            word_hops: Some(12),
            ..Default::default()
        };
        let ev = MotionEvent {
            motion: motion(0, 20, facts),
            keys_used: 20,
            clicked: false,
        };
        let fire = |known: Known| {
            let mut out = Vec::new();
            for mut r in all() {
                r.set_known(&known);
                r.on_motion(&ev, &mut out);
            }
            crate::rules::testing::findings(&out)
                .iter()
                .map(|f| (f.rule_id, f.text.clone()))
                .collect::<Vec<_>>()
        };
        let f = fire(Known::everything());
        assert_eq!(f.len(), 1);
        assert!(f[0].1.contains("`t<char>`"), "{f:?}");
        let f = fire(Known::of([]));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].0, "motion.word");
        assert!(!f[0].1.contains("`t"), "{f:?}");
        assert!(fire(Known::of(["find"]))[0].1.contains("`t<char>`"));
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
            crate::glossary::plain(&f.text),
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
            crate::glossary::plain(&f.text),
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

    #[test]
    fn back_to_the_last_change() {
        let f = MotionFacts {
            last_change: true,
            ..Default::default()
        };
        assert_eq!(
            plan(-40, 0, f.clone()),
            (Category::LastChange, "g;".into(), 2)
        );
        // On the same line it's just a move along it.
        assert_ne!(plan(0, 5, f).0, Category::LastChange);
    }

    #[test]
    fn shorter_motions_for_an_operator() {
        use crate::editor::{Operator, OperatorRange};
        let ev = |op, op_keys, range| OperatorEvent {
            backward: false,
            report: OperatorReport {
                op,
                at_ms: 9,
                objects: vec![],
                range: Some(Box::new(range)),
            },
            keys_used: op_keys,
            op_keys,
        };
        // d6l over a word: de.
        let word = OperatorRange {
            cols: 5,
            facts: MotionFacts {
                word_end_hops: Some(1),
                ..Default::default()
            },
            words: None,
            ..Default::default()
        };
        let mut out = Vec::new();
        OperatorMotionRule::default().on_operator(&ev(Operator::Delete, 3, word.clone()), &mut out);
        assert!(out.is_empty(), "d6l is 3 keys, de is 2: not enough");
        OperatorMotionRule::default().on_operator(&ev(Operator::Delete, 5, word), &mut out);
        let f = crate::rules::testing::findings(&out);
        assert_eq!(
            crate::glossary::plain(&f[0].text),
            "That took 5 keys. de covers the same text in 2."
        );
        // To the end of the line: D, or C for a change.
        let to_end = OperatorRange {
            cols: 20,
            facts: MotionFacts {
                line_end: true,
                till_hops: Some(3),
                ..Default::default()
            },
            words: Some(4),
            ..Default::default()
        };
        let plans = range_plans(Operator::Change, &to_end, false);
        assert_eq!(plans[0].0, "C");
        assert!(
            plans.iter().all(|p| !p.0.contains('w')),
            "cw is ce: {plans:?}"
        );
        let keys: Vec<_> = range_plans(Operator::Delete, &to_end, false)
            .into_iter()
            .map(|p| p.0)
            .collect();
        assert_eq!(keys, ["D", "d4w", "d3t<char>"]);
        // Without range facts ([text] shapes off), nothing.
        let mut out = Vec::new();
        let mut plain = ev(Operator::Delete, 9, OperatorRange::default());
        plain.report.range = None;
        OperatorMotionRule::default().on_operator(&plain, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn line_numbers_on_screen() {
        let f = MotionFacts {
            line_number_digits: Some(3),
            window_lines: Some(40),
            ..Default::default()
        };
        // 120 lines down to line 245: `245G` beats paging.
        assert_eq!(
            plan(120, 0, f.clone()),
            (Category::LineNumber, "<line>G".into(), 4)
        );
        // Close by, a count or a few presses do.
        assert_ne!(plan(5, 0, f).0, Category::LineNumber);
    }

    #[test]
    fn whole_lines_for_an_operator() {
        use crate::editor::{Operator, OperatorRange};
        let range = OperatorRange {
            linewise: true,
            lines: 10,
            facts: MotionFacts {
                paragraph_hops: Some(1),
                ..Default::default()
            },
            ..Default::default()
        };
        // 11 lines up to a blank line: `d}`, not counting them.
        let keys: Vec<_> = range_plans(Operator::Delete, &range, false)
            .into_iter()
            .map(|p| p.0)
            .collect();
        assert_eq!(keys, ["d}"]);
        let three = OperatorRange {
            linewise: true,
            lines: 2,
            ..Default::default()
        };
        assert_eq!(
            range_plans(Operator::Yank, &three, false)[0],
            ("3yy".to_string(), 3, "count")
        );
    }

    #[test]
    fn backward_bracket_and_bigword_plans() {
        use crate::editor::{BackRange, Operator, OperatorRange};
        let keys = |op, range: &OperatorRange, back| -> Vec<String> {
            range_plans(op, range, back)
                .into_iter()
                .map(|p| p.0)
                .collect()
        };
        let back = OperatorRange {
            back: Some(BackRange {
                words: Some(2),
                find_hops: Some(1),
                line_start: true,
                ..Default::default()
            }),
            words: Some(1),
            ..Default::default()
        };
        // Cursor after the range: only backward motions make sense.
        assert_eq!(
            keys(Operator::Delete, &back, true),
            ["d0", "d2b", "dF<char>"]
        );
        assert_eq!(keys(Operator::Delete, &back, false), ["dw"]);
        let bracket = OperatorRange {
            bracket: true,
            bigwords: Some(1),
            bigword_ends: Some(1),
            facts: MotionFacts {
                word_end_hops: Some(3),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            keys(Operator::Delete, &bracket, false),
            ["d%", "dW", "dE", "d3e"]
        );
        // A change: `cw` from the word ends, as Vim does it.
        assert_eq!(keys(Operator::Change, &bracket, false), ["c%", "cW", "c3w"]);
    }

    #[test]
    fn wider_search_for_the_cheapest_way() {
        // 26 lines down; blank lines at 10 and 24: `}}` then `2j` (4 keys).
        let f = MotionFacts {
            paragraph_lines: Some(vec![10, 24, 40]),
            ..Default::default()
        };
        assert_eq!(
            plan(26, 0, f),
            (Category::Paragraph, "}×2 then jj".into(), 4)
        );
        // One past the target and back.
        let f = MotionFacts {
            paragraph_lines: Some(vec![13]),
            ..Default::default()
        };
        assert_eq!(plan(12, 0, f), (Category::Paragraph, "} then k".into(), 2));
        // Far away and visible: three letters of it.
        let f = MotionFacts {
            search_chars: Some(3),
            ..Default::default()
        };
        assert_eq!(plan(40, 12, f), (Category::SearchTo, "/<text>".into(), 5));
        let f = MotionFacts {
            section_hops: Some(1),
            ..Default::default()
        };
        assert_eq!(plan(30, 0, f), (Category::Section, "]]".into(), 2));
        let f = MotionFacts {
            unmatched_paren: true,
            ..Default::default()
        };
        assert_eq!(plan(0, -14, f), (Category::Unmatched, "[(".into(), 2));
    }

    #[test]
    fn reading_through_gets_no_tip() {
        let mut ev = MotionEvent {
            motion: Motion {
                start_ms: 0,
                end_ms: 5_600,
                moves: 8,
                mode: EditorMode::Normal,
                lines: 8,
                cols: 0,
                facts: MotionFacts::default(),
            },
            keys_used: 8,
            clicked: false,
        };
        let tips = |ev: &MotionEvent| {
            let mut out = Vec::new();
            MotionRule::new(Category::Count).on_motion(ev, &mut out);
            crate::rules::testing::findings(&out).len()
        };
        assert_eq!(tips(&ev), 0, "a line every 0.8 s: reading");
        ev.motion.end_ms = 800;
        assert_eq!(tips(&ev), 1);
    }

    #[test]
    fn effort_weighs_shift_chords_and_counting() {
        assert_eq!(effort("w", 1), 2);
        assert_eq!(effort("}", 1), 3, "shifted");
        assert_eq!(effort("Ctrl-d", 1), 3, "a chord");
        assert_eq!(effort("7j", 2), 5, "a digit to count");
        assert_eq!(effort("Ctrl-d×3 then 4j", 5), 10 + 3 + 1);
        assert_eq!(effort("f<char>", 2), 4);
        // Two plain keys beat two shifted ones at the same key count.
        assert!(effort("ww", 2) < effort("}}", 2));
    }

    #[test]
    fn a_long_move_must_save_more() {
        let r = MotionRule::new(Category::Count);
        assert_eq!(r.saving(8), 3);
        assert_eq!(r.saving(40), 10);
    }
}
