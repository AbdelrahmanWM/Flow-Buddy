//! Edit rule pack: changes that take more keys than they need.
//!
//! These work from keys alone. Suggestions name commands from the static
//! tables below, never from what was typed: `dfx` is described as
//! `df<char>`, and inserted text only ever counts as a number.

use crate::context::{ContextKind, VimMode};
use crate::glossary;
use crate::key::Key;
use crate::rule::{Finding, Rule, RuleEvent, Shape};
use crate::rules::repeat::{RepeatRule, RepeatSpec};
use crate::rules::vim::{command, insert_key};
use crate::vim::{Command, InsertKey, Step, VimEvent};

const VIM: &[ContextKind] = &[ContextKind::TerminalVim, ContextKind::VsCodeVim];

/// Two commands this close together count as one gesture.
const GESTURE_MS: u64 = 1500;

/// Motions and text objects that can follow an operator, by their keys.
const MOTIONS: &[&str] = &[
    "w", "e", "b", "W", "E", "B", "ge", "$", "0", "^", "j", "k", "h", "l", "}", "{", "G", "gg",
    "iw", "aw", "iW", "aW", "is", "as", "ip", "ap", "i(", "a(", "ib", "ab", "i)", "a)", "i[", "a[",
    "i]", "a]", "i{", "a{", "iB", "aB", "i}", "a}", "i<", "a<", "i>", "a>", "it", "at", "i\"",
    "a\"", "i'", "a'", "i`", "a`",
];

/// `keys` as a static motion name: `w`, `iw`, `f<char>`.
fn motion_name(keys: &[Key]) -> Option<&'static str> {
    let first = keys.first()?.plain_char()?;
    if keys.len() == 2 && matches!(first, 'f' | 't' | 'F' | 'T') {
        return Some(match first {
            'f' => "f<char>",
            't' => "t<char>",
            'F' => "F<char>",
            _ => "T<char>",
        });
    }
    let spelled: Option<String> = keys.iter().map(|k| k.plain_char()).collect();
    let spelled = spelled?;
    MOTIONS.iter().copied().find(|m| *m == spelled)
}

/// An operator command's operator and motion name: `dw` → (`d`, `w`).
fn split_operator(c: &Command) -> Option<(char, &'static str)> {
    let (op, rest) = c.keys.split_first()?;
    let op = op.plain_char()?;
    if !matches!(op, 'd' | 'c' | 'y' | '>' | '<' | '=') {
        return None;
    }
    // A count between operator and motion (d3w) isn't a plain motion.
    if rest.first()?.plain_char()?.is_ascii_digit() && rest.len() > 1 {
        return None;
    }
    Some((op, motion_name(rest)?))
}

fn finding(
    id: &'static str,
    title: &'static str,
    text: String,
    shape: Shape,
    step: &Step,
) -> RuleEvent {
    RuleEvent::Finding(Finding {
        rule_id: id,
        title,
        text,
        shape,
        mode: VimMode::Normal,
        at_ms: step.at_ms,
    })
}

fn adoption(id: &'static str, step: &Step) -> RuleEvent {
    RuleEvent::Adoption {
        rule_id: id,
        at_ms: step.at_ms,
    }
}

// Delete then insert ------------------------------------------------------

#[derive(Clone, Copy)]
enum Deleted {
    /// `x`, with its count.
    Chars(Option<u32>),
    /// `d` + motion other than `$`.
    Motion(&'static str),
    /// `d$` or `D`: cursor ends on the last character.
    ToEnd,
    Line,
}

/// `xi` → `s`, `dwi` → `cw`, `Da` → `C`, `ddO` → `cc`.
#[derive(Default)]
pub struct DeleteThenInsert {
    last: Option<(Deleted, u64)>,
}

impl DeleteThenInsert {
    const ID: &'static str = "vim.delete-then-insert";
}

impl Rule for DeleteThenInsert {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Deleting and then entering Insert mode where one change command does both"
    }

    fn title(&self) -> &'static str {
        "Change instead"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        if step.event.is_none() {
            return;
        }
        let Some(c) = command(step).filter(|_| step.before != VimMode::Insert) else {
            self.last = None;
            return;
        };
        let last = self.last.take();
        if step.before.is_visual() {
            return;
        }
        if c.head().is_some_and(|h| matches!(h, 'c' | 's' | 'S' | 'C')) {
            out.push(adoption(Self::ID, step));
            return;
        }
        let deleted = if c.is("x") {
            Some(Deleted::Chars(c.count))
        } else if c.is("D") || c.is("d$") {
            Some(Deleted::ToEnd)
        } else if c.is("dd") {
            Some(Deleted::Line)
        } else {
            split_operator(c)
                .filter(|(op, _)| *op == 'd')
                .map(|(_, m)| Deleted::Motion(m))
        };
        if let Some(d) = deleted {
            self.last = Some((d, step.at_ms));
            return;
        }
        let Some((d, at)) = last.filter(|(_, at)| step.at_ms.saturating_sub(*at) <= GESTURE_MS)
        else {
            return;
        };
        if c.count.is_some() {
            return;
        }
        let (shape, text) = match (d, c.single().and_then(|k| k.plain_char())) {
            (Deleted::Chars(None), Some('i')) => (
                "x then i",
                "`x` then `i` is `s`: substitute the character.".to_string(),
            ),
            (Deleted::Chars(Some(n)), Some('i')) => (
                "x then i",
                format!("`{n}x` then `i` is `{n}s`: substitute {n} characters."),
            ),
            (Deleted::Motion(m), Some('i')) => (
                "d{motion} then i",
                format!(
                    "{} then `i` is {}: change deletes and starts Insert.",
                    glossary::mark(&format!("d{m}")),
                    glossary::mark(&format!("c{m}"))
                ),
            ),
            (Deleted::ToEnd, Some('a')) => (
                "D then a",
                "`D` then `a` is `C`: change to the end of the line.".to_string(),
            ),
            (Deleted::Line, Some('O')) => (
                "dd then O",
                "`dd` then `O` is `cc` (or `S`): change the whole line.".to_string(),
            ),
            _ => return,
        };
        let _ = at;
        out.push(finding(
            Self::ID,
            "Change instead",
            text,
            Shape::fixed(shape),
            step,
        ));
    }

    fn reset(&mut self) {
        self.last = None;
    }
}

// Shorthands ---------------------------------------------------------------

/// `d$` → `D`, `$a` → `A`, `ko` → `O` and friends.
#[derive(Default)]
pub struct Shorthand {
    /// The previous single-key command, if it can start a pair.
    prev: Option<(char, u64)>,
}

impl Shorthand {
    const ID: &'static str = "vim.shorthand";
}

impl Rule for Shorthand {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Spelling out a command that has a one-key form (d$ for D, $a for A)"
    }

    fn title(&self) -> &'static str {
        "One key does it"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        if step.event.is_none() {
            return;
        }
        let prev = self.prev.take();
        let Some(c) =
            command(step).filter(|_| matches!(step.before, VimMode::Normal | VimMode::OpPending))
        else {
            return;
        };
        if c.count.is_some() {
            return;
        }
        let long_forms: [(&'static str, &str); 4] =
            [("d$", "D"), ("c$", "C"), ("dl", "x"), ("dh", "X")];
        if let Some((long, short)) = long_forms.into_iter().find(|(long, _)| c.is(long)) {
            out.push(finding(
                Self::ID,
                "One key does it",
                format!("`{long}` is `{short}`."),
                Shape::fixed(long),
                step,
            ));
            return;
        }
        if ["D", "C", "A", "I"].iter().any(|s| c.is(s)) {
            out.push(adoption(Self::ID, step));
        }
        let this = c.single().and_then(|k| k.plain_char());
        let pair = prev
            .filter(|(_, at)| step.at_ms.saturating_sub(*at) <= GESTURE_MS)
            .zip(this)
            .and_then(|((p, _), t)| match (p, t) {
                ('$', 'a') => Some((
                    "$ then a",
                    "`$` then `a` is `A`: append at the end of the line.",
                )),
                ('^', 'i') => Some((
                    "^ then i",
                    "`^` then `i` is `I`: insert at the first non-blank.",
                )),
                ('k', 'o') => Some(("k then o", "`k` then `o` is `O`: open a line above.")),
                ('j', 'O') => Some(("j then O", "`j` then `O` is `o`: open a line below.")),
                _ => None,
            });
        if let Some((shape, text)) = pair {
            out.push(finding(
                Self::ID,
                "One key does it",
                text.into(),
                Shape::fixed(shape),
                step,
            ));
            return;
        }
        if let Some(t) = this.filter(|t| matches!(t, '$' | '^' | 'k' | 'j')) {
            self.prev = Some((t, step.at_ms));
        }
    }

    fn reset(&mut self) {
        self.prev = None;
    }
}

// Visual mode for one operator ---------------------------------------------

struct Selection {
    linewise: bool,
    /// One kind of motion, and how many of it.
    motion: Option<(&'static str, u32)>,
    object: Option<&'static str>,
    keys: u32,
    simple: bool,
}

/// `viwd` → `diw`, `Vjjd` → `3dd`, `veed` → `d2e`.
#[derive(Default)]
pub struct VisualOperator {
    sel: Option<Selection>,
}

impl VisualOperator {
    const ID: &'static str = "vim.visual-operator";
}

fn op_name(c: &Command) -> Option<char> {
    match c.single()?.plain_char()? {
        'x' => Some('d'),
        op @ ('d' | 'c' | 'y' | '>' | '<' | '=') => Some(op),
        _ => None,
    }
}

fn op_str(op: char) -> &'static str {
    match op {
        'd' => "d",
        'c' => "c",
        'y' => "y",
        '>' => ">",
        '<' => "<",
        _ => "=",
    }
}

impl Rule for VisualOperator {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Selecting in Visual mode just to run one operator on a motion or text object"
    }

    fn title(&self) -> &'static str {
        "Skip Visual mode"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        if step.event.is_none() {
            return;
        }
        let Some(c) = command(step) else {
            self.sel = None;
            return;
        };
        if !step.before.is_visual() {
            self.sel = None;
            if c.is("v") || c.is("V") {
                self.sel = Some(Selection {
                    linewise: c.is("V"),
                    motion: None,
                    object: None,
                    keys: 1,
                    simple: true,
                });
            } else if split_operator(c)
                .is_some_and(|(_, m)| m.len() == 2 && m.starts_with(['i', 'a']))
            {
                out.push(adoption(Self::ID, step));
            }
            return;
        }
        let Some(sel) = self.sel.as_mut() else { return };
        if let Some(op) = op_name(c) {
            let sel = self.sel.take().unwrap();
            if !sel.simple {
                return;
            }
            let used = sel.keys + 1;
            let op_s = op_str(op);
            let (better, cost) = match (sel.linewise, sel.object, sel.motion) {
                (false, Some(obj), None) => (format!("{op_s}{obj}"), 1 + obj.len() as u32),
                (true, None, Some(("j" | "k", n))) => {
                    let lines = n + 1;
                    (format!("{lines}{op_s}{op_s}"), digits(lines) + 2)
                }
                (false, None, Some((m @ ("e" | "E" | "$"), n))) => {
                    if n == 1 {
                        (format!("{op_s}{m}"), 2)
                    } else {
                        (format!("{op_s}{n}{m}"), 2 + digits(n))
                    }
                }
                _ => return,
            };
            if used > cost {
                let text = format!(
                    "That selection took {used} keys. {} does it in {cost}, without Visual mode.",
                    glossary::mark(&better)
                );
                out.push(finding(
                    Self::ID,
                    "Skip Visual mode",
                    text,
                    Shape::repeat("visual then operator", used),
                    step,
                ));
            }
            return;
        }
        let keys = c.keys.len() as u32 + c.count.map_or(0, digits);
        sel.keys += keys;
        match motion_name(&c.keys) {
            Some(obj) if obj.len() == 2 && obj.starts_with(['i', 'a']) => {
                sel.simple &= sel.object.is_none() && sel.motion.is_none();
                sel.object = Some(obj);
            }
            Some(m) => {
                let n = c.count.unwrap_or(1);
                match sel.motion {
                    None if sel.object.is_none() => sel.motion = Some((m, n)),
                    Some((prev, total)) if prev == m => sel.motion = Some((m, total + n)),
                    _ => sel.simple = false,
                }
            }
            None => sel.simple = false,
        }
    }

    fn reset(&mut self) {
        self.sel = None;
    }
}

fn digits(n: u32) -> u32 {
    n.checked_ilog10().map_or(1, |d| d + 1)
}

// Dot repeat ---------------------------------------------------------------

/// What makes two changes "the same": the command's keys and how many
/// characters were typed after it. Kept in memory only, until reset.
#[derive(Clone, PartialEq)]
struct Change {
    keys: Vec<Key>,
    typed: u32,
}

/// The same change typed out again where `.` would repeat it.
#[derive(Default)]
pub struct DotRepeat {
    /// A change still in Insert mode.
    open: Option<Change>,
    last: Option<Change>,
    /// How many times in a row `last` was made.
    times: u32,
}

impl DotRepeat {
    const ID: &'static str = "vim.dot-repeat";

    /// Changes this rule looks at; x, dd, J and p have their own rules.
    fn is_change(c: &Command) -> bool {
        let Some(head) = c.head() else { return false };
        match head {
            'c' | 'd' => !c.is("dd") && c.keys.len() >= 2,
            's' | 'S' | 'C' | 'D' | 'r' | '~' => true,
            _ => false,
        }
    }

    fn finish(&mut self, change: Change, step: &Step, out: &mut Vec<RuleEvent>) {
        let cost = change.keys.len() as u32
            + change.typed
            + u32::from(change.typed > 0 || self.enters_insert(&change));
        if self.last.as_ref() == Some(&change) {
            self.times += 1;
            if self.times == 2 && cost >= 3 {
                let text = format!(
                    "That change again took {cost} keys. `.` repeats the last change in one."
                );
                out.push(finding(
                    Self::ID,
                    "Repeat it with .",
                    text,
                    Shape::repeat("same change", 2),
                    step,
                ));
            }
        } else {
            self.last = Some(change);
            self.times = 1;
        }
    }

    fn enters_insert(&self, change: &Change) -> bool {
        change
            .keys
            .first()
            .and_then(|k| k.plain_char())
            .is_some_and(|c| matches!(c, 'c' | 's' | 'S' | 'C'))
    }
}

impl Rule for DotRepeat {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Typing the same change again where . repeats it"
    }

    fn title(&self) -> &'static str {
        "Repeat it with ."
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        if step.before == VimMode::Insert {
            match (&mut self.open, insert_key(step), &step.event) {
                (Some(ch), Some(InsertKey::Text), _) => ch.typed += 1,
                (Some(_), _, Some(VimEvent::Escape)) => {
                    let ch = self.open.take().unwrap();
                    self.finish(ch, step, out);
                }
                // Anything else in Insert (arrows, Ctrl keys) makes it a
                // different kind of edit.
                (Some(_), _, Some(_)) => {
                    self.open = None;
                    self.last = None;
                }
                _ => {}
            }
            return;
        }
        let Some(c) = command(step) else {
            if step.event.is_some() && !matches!(step.event, Some(VimEvent::Escape)) {
                self.last = None;
            }
            return;
        };
        if step.before.is_visual() {
            self.last = None;
            return;
        }
        if c.is(".") {
            out.push(adoption(Self::ID, step));
            return;
        }
        if Self::is_change(c) {
            let change = Change {
                keys: c.keys.clone(),
                typed: 0,
            };
            if step.after == VimMode::Insert {
                self.open = Some(change);
            } else {
                self.finish(change, step, out);
            }
            return;
        }
        if !crate::motion::is_motion(c) {
            self.last = None;
        }
    }

    fn reset(&mut self) {
        self.open = None;
        self.last = None;
        self.times = 0;
    }
}

// Counts for >>, J and p ---------------------------------------------------

fn uncounted_is(step: &Step, spellings: &[&'static str]) -> Option<&'static str> {
    let c = command(step).filter(|c| c.count.is_none() && !step.before.is_visual())?;
    spellings.iter().copied().find(|s| c.is(s))
}

fn counted_is(step: &Step, spellings: &[&str]) -> bool {
    command(step).is_some_and(|c| c.count.is_some() && spellings.iter().any(|s| c.is(s)))
}

pub fn repeat_indent() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.repeat-indent",
        description: "Indenting line by line with >> or << instead of a count",
        title: "Indent with a count",
        contexts: VIM,
        threshold: 3,
        unit: |s| uncounted_is(s, &[">>", "<<"]),
        adopted: |s| {
            counted_is(s, &[">>", "<<"])
                || command(s).is_some_and(|c| {
                    c.keys.len() > 2 && c.head().is_some_and(|h| h == '>' || h == '<')
                })
        },
        message: |unit, n| {
            format!(
                "`{unit}` {n} times. Try `{n}{unit}`, or `{}` with a motion such as `>ip`.",
                &unit[..1]
            )
        },
    })
}

pub fn repeat_join() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.repeat-join",
        description: "Joining lines one J at a time instead of a count",
        title: "Join with a count",
        contexts: VIM,
        threshold: 3,
        unit: |s| uncounted_is(s, &["J"]),
        adopted: |s| counted_is(s, &["J"]),
        message: |_, n| format!("`J` {n} times. `{}J` joins those lines at once.", n + 1),
    })
}

pub fn repeat_paste() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.repeat-paste",
        description: "Pasting the same thing again and again instead of a count",
        title: "Paste with a count",
        contexts: VIM,
        threshold: 3,
        unit: |s| uncounted_is(s, &["p", "P"]),
        adopted: |s| counted_is(s, &["p", "P"]),
        message: |unit, n| format!("`{unit}` {n} times. `{n}{unit}` pastes it {n} times."),
    })
}

// Macros -------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum Piece {
    Command(Vec<Key>),
    /// Characters typed in Insert mode; only the number.
    Typed(u32),
}

/// The same edit done line after line where a macro would replay it.
#[derive(Default)]
pub struct Macro {
    unit: Vec<Piece>,
    /// Insert mode used keys other than text: not a replayable unit.
    broken: bool,
    last: Option<Vec<Piece>>,
    times: u32,
}

impl Macro {
    const ID: &'static str = "vim.macro";
    /// Repeats of the same unit before suggesting a macro.
    const TIMES: u32 = 3;

    /// Moves that go on to the next place to edit.
    fn is_next(c: &Command) -> bool {
        c.count.is_none() && ["j", "+", "n", "N", "*", "#", ";"].iter().any(|s| c.is(s))
    }

    fn keys(unit: &[Piece]) -> u32 {
        unit.iter()
            .map(|p| match p {
                Piece::Command(k) => k.len() as u32,
                // The text, plus Esc to leave Insert mode.
                Piece::Typed(n) => n + 1,
            })
            .sum()
    }

    fn close(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        let unit = std::mem::take(&mut self.unit);
        let broken = std::mem::take(&mut self.broken);
        let edits = unit
            .iter()
            .filter(|p| match p {
                Piece::Command(k) => !k.is_empty() && !is_motion_keys(k),
                Piece::Typed(_) => true,
            })
            .count();
        if broken || edits == 0 || Self::keys(&unit) < 4 {
            self.last = None;
            return;
        }
        if self.last.as_ref() == Some(&unit) {
            self.times += 1;
            if self.times == Self::TIMES {
                let n = self.times;
                let keys = Self::keys(&unit);
                out.push(finding(
                    Self::ID,
                    "Record a macro",
                    format!(
                        "The same {keys}-key edit {n} times in a row. Record it once with `qq` … `q`, then replay it with `@q` and `@@`."
                    ),
                    Shape::repeat("same edit per line", n),
                    step,
                ));
            }
        } else {
            self.last = Some(unit);
            self.times = 1;
        }
    }
}

fn is_motion_keys(keys: &[Key]) -> bool {
    crate::motion::is_motion(&Command {
        count: None,
        keys: keys.to_vec(),
    })
}

impl Rule for Macro {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Doing the same multi-key edit on line after line where a macro replays it"
    }

    fn title(&self) -> &'static str {
        "Record a macro"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        if step.before == VimMode::Insert {
            match (insert_key(step), &step.event) {
                (Some(InsertKey::Text), _) => match self.unit.last_mut() {
                    Some(Piece::Typed(n)) => *n += 1,
                    _ => self.unit.push(Piece::Typed(1)),
                },
                (_, Some(VimEvent::Escape) | None) => {}
                _ => self.broken = true,
            }
            return;
        }
        let Some(c) = command(step) else {
            if !matches!(step.event, None | Some(VimEvent::Escape)) {
                self.broken = true;
            }
            return;
        };
        if c.head().is_some_and(|h| h == 'q' || h == '@') {
            out.push(adoption(Self::ID, step));
            self.reset();
            return;
        }
        let mut keys = c.keys.clone();
        if let Some(n) = c.count {
            // Keep counts as part of the shape: 3x and 2x differ.
            keys.extend(n.to_string().chars().map(Key::ch));
        }
        self.unit.push(Piece::Command(keys));
        if Self::is_next(c) {
            self.close(step, out);
        }
    }

    fn reset(&mut self) {
        self.unit.clear();
        self.broken = false;
        self.last = None;
        self.times = 0;
    }
}

pub fn all() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(Macro::default()),
        Box::new(DeleteThenInsert::default()),
        Box::new(Shorthand::default()),
        Box::new(VisualOperator::default()),
        Box::new(DotRepeat::default()),
        Box::new(repeat_indent()),
        Box::new(repeat_join()),
        Box::new(repeat_paste()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::testing::{adoptions, findings, run};
    use crate::vim::Flavor;

    fn vim(rule: impl Rule + 'static, keys: &str) -> Vec<RuleEvent> {
        run(Box::new(rule), Flavor::Vim, VimMode::Normal, keys)
    }

    fn texts(ev: &[RuleEvent]) -> Vec<String> {
        // Compared without the command markers; see glossary::plain.
        findings(ev)
            .iter()
            .map(|f| glossary::plain(&f.text))
            .collect()
    }

    #[test]
    fn delete_then_insert() {
        let r = || DeleteThenInsert::default();
        assert_eq!(
            texts(&vim(r(), "xihi<Esc>")),
            ["x then i is s: substitute the character."]
        );
        assert_eq!(
            texts(&vim(r(), "3xihi<Esc>")),
            ["3x then i is 3s: substitute 3 characters."]
        );
        assert_eq!(
            texts(&vim(r(), "dwihi<Esc>")),
            ["dw then i is cw: change deletes and starts Insert."]
        );
        assert_eq!(
            texts(&vim(r(), "diwihi<Esc>")),
            ["diw then i is ciw: change deletes and starts Insert."]
        );
        assert_eq!(
            texts(&vim(r(), "Dahi<Esc>")),
            ["D then a is C: change to the end of the line."]
        );
        assert_eq!(
            texts(&vim(r(), "ddOhi<Esc>")),
            ["dd then O is cc (or S): change the whole line."]
        );
        // A find target is a typed character: it's never echoed back.
        assert_eq!(
            texts(&vim(r(), "dfqihi<Esc>")),
            ["df<char> then i is cf<char>: change deletes and starts Insert."]
        );
    }

    #[test]
    fn delete_then_something_else_is_fine() {
        let r = || DeleteThenInsert::default();
        assert!(findings(&vim(r(), "xjihi<Esc>")).is_empty());
        assert!(findings(&vim(r(), "xahi<Esc>")).is_empty());
        assert!(findings(&vim(r(), "ddohi<Esc>")).is_empty());
        assert_eq!(adoptions(&vim(r(), "cwhi<Esc>shi<Esc>")), 2);
    }

    #[test]
    fn shorthands() {
        let r = || Shorthand::default();
        assert_eq!(texts(&vim(r(), "d$")), ["d$ is D."]);
        assert_eq!(texts(&vim(r(), "c$hi<Esc>")), ["c$ is C."]);
        assert_eq!(
            texts(&vim(r(), "$ahi<Esc>")),
            ["$ then a is A: append at the end of the line."]
        );
        assert_eq!(
            texts(&vim(r(), "kohi<Esc>")),
            ["k then o is O: open a line above."]
        );
        assert!(findings(&vim(r(), "$jahi<Esc>")).is_empty());
        assert!(findings(&vim(r(), "3d$")).is_empty());
        assert_eq!(adoptions(&vim(r(), "DAhi<Esc>")), 2);
    }

    #[test]
    fn visual_for_one_operator() {
        let r = || VisualOperator::default();
        assert_eq!(
            texts(&vim(r(), "viwd")),
            ["That selection took 4 keys. diw does it in 3, without Visual mode."]
        );
        assert_eq!(
            texts(&vim(r(), "Vjjd")),
            ["That selection took 4 keys. 3dd does it in 3, without Visual mode."]
        );
        assert_eq!(
            texts(&vim(r(), "veey")),
            ["That selection took 4 keys. y2e does it in 3, without Visual mode."]
        );
        // Mixed motions are a real selection.
        assert!(findings(&vim(r(), "vjwd")).is_empty());
        // So is one that's then used for something else.
        assert!(findings(&vim(r(), "viw<Esc>")).is_empty());
        assert_eq!(adoptions(&vim(r(), "diwci(hi<Esc>")), 2);
    }

    #[test]
    fn dot_repeat() {
        let r = || DotRepeat::default();
        let ev = vim(r(), "ciwfoo<Esc>wciwbar<Esc>");
        assert_eq!(
            texts(&ev),
            ["That change again took 7 keys. . repeats the last change in one."]
        );
        // Different amounts typed: different changes.
        assert!(findings(&vim(r(), "ciwfoo<Esc>wciwab<Esc>")).is_empty());
        // An edit in between breaks it.
        assert!(findings(&vim(r(), "ciwfoo<Esc>xciwfoo<Esc>")).is_empty());
        // Short changes aren't worth it.
        assert!(findings(&vim(r(), "rxjrx")).is_empty());
        assert_eq!(adoptions(&vim(r(), "ciwfoo<Esc>w.")), 1);
    }

    #[test]
    fn counts_for_indent_join_paste() {
        let f = vim(repeat_indent(), ">>>>>><Esc>");
        assert_eq!(
            texts(&f),
            [">> 3 times. Try 3>>, or > with a motion such as >ip."]
        );
        assert_eq!(
            texts(&vim(repeat_join(), "JJJ<Esc>")),
            ["J 3 times. 4J joins those lines at once."]
        );
        assert_eq!(
            texts(&vim(repeat_paste(), "ppp<Esc>")),
            ["p 3 times. 3p pastes it 3 times."]
        );
        assert!(findings(&vim(repeat_join(), "JJ<Esc>")).is_empty());
        assert_eq!(adoptions(&vim(repeat_paste(), "3p")), 1);
    }

    #[test]
    fn macro_for_the_same_edit_on_each_line() {
        let r = || Macro::default();
        let line = "0cwnew<Esc>j";
        let ev = vim(r(), &line.repeat(3));
        assert_eq!(
            texts(&ev),
            [
                "The same 8-key edit 3 times in a row. Record it once with qq … q, then replay it with @q and @@."
            ]
        );
        // Twice isn't a habit yet; a different edit breaks the run.
        assert!(findings(&vim(r(), &line.repeat(2))).is_empty());
        assert!(findings(&vim(r(), "0cwnew<Esc>j0cwab<Esc>j0cwnew<Esc>j")).is_empty());
        // Just moving isn't an edit.
        assert!(findings(&vim(r(), "wwjwwjwwj")).is_empty());
        assert_eq!(adoptions(&vim(r(), "qq0cwnew<Esc>jq@q@@")), 3);
    }

    #[test]
    fn findings_never_contain_typed_text() {
        let typed = "zqxv";
        let keys = format!("dfzi{typed}<Esc>ciw{typed}<Esc>wciw{typed}<Esc>vfzd$a{typed}<Esc>");
        for rule in all() {
            for f in findings(&run(rule, Flavor::Vim, VimMode::Normal, &keys)) {
                assert!(!f.text.contains('z') && !f.text.contains('q'), "{}", f.text);
            }
        }
    }
}
