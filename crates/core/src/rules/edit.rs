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
use crate::rules::vim::{LINE_AND_SHELL, command, insert_key};
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
        start_ms: step.at_ms,
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
    /// `x` then `i`: characters typed so far. One, then Esc, is `r`.
    replacing: Option<u32>,
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
        LINE_AND_SHELL
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        if step.event.is_none() {
            return;
        }
        if let Some(typed) = self.replacing.as_mut() {
            match (insert_key(step), &step.event) {
                (Some(InsertKey::Text), _) => {
                    *typed += 1;
                    return;
                }
                (_, Some(VimEvent::Escape)) => {
                    let (shape, text) = if *typed == 1 {
                        (
                            "x then i, one letter",
                            "`x`, `i`, one letter and `Esc` is `r<char>`: replace the character and stay in Normal mode (`~` if it's only the case).",
                        )
                    } else {
                        ("x then i", "`x` then `i` is `s`: substitute the character.")
                    };
                    self.replacing = None;
                    out.push(finding(
                        Self::ID,
                        "Change instead",
                        text.into(),
                        Shape::fixed(shape),
                        step,
                    ));
                    return;
                }
                _ => self.replacing = None,
            }
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
            // `xxx` then `i` is `3s`.
            let before = match last {
                Some((Deleted::Chars(n), at)) if step.at_ms.saturating_sub(at) <= GESTURE_MS => {
                    n.unwrap_or(1)
                }
                _ => 0,
            };
            let n = before + c.count.unwrap_or(1);
            Some(Deleted::Chars((n > 1).then_some(n)))
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
            (Deleted::Chars(None), Some('i')) => {
                // Said at Esc, once it's clear whether one letter replaced it.
                self.replacing = Some(0);
                return;
            }
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
        self.replacing = None;
    }
}

// Shorthands ---------------------------------------------------------------

/// `d$` → `D`, `$a` → `A`, `ko` → `O` and friends.
#[derive(Default)]
pub struct Shorthand {
    /// The previous single-key command, if it can start a pair.
    prev: Option<(char, u64)>,
    /// `A` was just pressed: Enter right away is `o`.
    appended: Option<u64>,
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
        LINE_AND_SHELL
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        if step.event.is_none() {
            return;
        }
        if matches!(&step.event, Some(VimEvent::Cmdline { kind: ':', text: Some(t) }) if t.trim() == "q!")
        {
            out.push(finding(
                Self::ID,
                "One key does it",
                "`:q!` is `ZQ`: quit without saving.".into(),
                Shape::fixed(":q!"),
                step,
            ));
            return;
        }
        if let Some(at) = self.appended.take()
            && insert_key(step) == Some(InsertKey::Enter)
            && step.at_ms.saturating_sub(at) <= GESTURE_MS
        {
            out.push(finding(
                Self::ID,
                "One key does it",
                "`A` then Enter is `o`: open a line below.".into(),
                Shape::fixed("A then Enter"),
                step,
            ));
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
        if ["D", "C", "A", "I", "o", "O", "gI", "ZQ"]
            .iter()
            .any(|s| c.is(s))
        {
            out.push(adoption(Self::ID, step));
        }
        if c.is("A") {
            self.appended = Some(step.at_ms);
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
                ('0', 'i') => Some((
                    "0 then i",
                    "`0` then `i` is `gI`: insert at the very start of the line (`I` starts at the first non-blank).",
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
        if let Some(t) = this.filter(|t| matches!(t, '$' | '^' | '0' | 'k' | 'j')) {
            self.prev = Some((t, step.at_ms));
        }
    }

    fn reset(&mut self) {
        self.prev = None;
        self.appended = None;
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
        register: None,
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

pub fn undo_run() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.undo-run",
        description: "Pressing u (or Ctrl-r) many times to go back through changes",
        title: "Undo further at once",
        contexts: VIM,
        threshold: 6,
        unit: |s| {
            let c = command(s).filter(|c| c.count.is_none() && !s.before.is_visual())?;
            if c.is("u") {
                Some("u")
            } else if c.single().is_some_and(|k| k.is_ctrl('r')) {
                Some("Ctrl-r")
            } else {
                None
            }
        },
        adopted: |s| {
            command(s).is_some_and(|c| {
                (c.count.is_some() && (c.is("u") || c.single().is_some_and(|k| k.is_ctrl('r'))))
                    || c.is("U")
                    || c.is("g-")
                    || c.is("g+")
            }) || is_cmdline(s, &["earlier", "later", "undo"])
        },
        message: |unit, n| {
            if unit == "u" {
                format!(
                    "`u` {n} times. `{n}u` undoes that many at once. `:earlier 1m` goes back to how the file was a minute ago."
                )
            } else {
                format!(
                    "`Ctrl-r` {n} times. `{n}Ctrl-r` redoes that many. `:later 1m` redoes a minute's worth."
                )
            }
        },
    })
}

/// A `:` command starting with one of `names`.
fn is_cmdline(step: &Step, names: &[&str]) -> bool {
    match &step.event {
        Some(VimEvent::Cmdline {
            kind: ':',
            text: Some(t),
        }) => {
            let t = t.trim_start().trim_start_matches('%');
            names.iter().any(|n| t.starts_with(n))
        }
        _ => false,
    }
}

/// `n.n.n.` → `cgn` then `...`, or `:%s` for every match.
#[derive(Default)]
pub struct ChangeMatches {
    /// Saw a search hop and waits for `.`.
    hopped: bool,
    /// (hop, `.`) pairs in a row, and when the last step was.
    pairs: u32,
    last_ms: u64,
}

impl ChangeMatches {
    const ID: &'static str = "vim.change-matches";
    const PAIRS: u32 = 3;
    /// Longer than a gesture: you look at each match before changing it.
    const GAP_MS: u64 = 8000;
}

impl Rule for ChangeMatches {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Jumping to each search match and repeating the change by hand (n then .)"
    }

    fn title(&self) -> &'static str {
        "Change the next match"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn threshold(&self) -> Option<u32> {
        Some(Self::PAIRS)
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        if is_cmdline(step, &["s/", "s#", "substitute"]) {
            out.push(adoption(Self::ID, step));
        }
        let Some(c) = command(step).filter(|_| !step.before.is_visual()) else {
            if step.event.is_some() && !matches!(step.event, Some(VimEvent::Escape)) {
                self.reset();
            }
            return;
        };
        if step.at_ms.saturating_sub(self.last_ms) > Self::GAP_MS {
            self.reset();
        }
        self.last_ms = step.at_ms;
        if c.is("cgn") || c.is("dgn") {
            out.push(adoption(Self::ID, step));
            self.reset();
            return;
        }
        let hop = c.count.is_none() && ["n", "N", "*", "#"].iter().any(|h| c.is(h));
        if hop {
            self.hopped = true;
            return;
        }
        if c.is(".") && self.hopped {
            self.hopped = false;
            self.pairs += 1;
            if self.pairs == Self::PAIRS {
                let n = self.pairs;
                out.push(finding(
                    Self::ID,
                    "Change the next match",
                    format!(
                        "`n` then `.` {n} times. Make the change with `cgn` instead: then `.` alone jumps to the next match and changes it. For every match at once, `:%s/old/new/g`."
                    ),
                    Shape::repeat("n then .", n),
                    step,
                ));
            }
            return;
        }
        self.reset();
    }

    fn reset(&mut self) {
        self.hopped = false;
        self.pairs = 0;
    }
}

/// `Ifoo<Esc>j` line after line → `Ctrl-v`, `j`…, `I`, type once.
#[derive(Default)]
pub struct VisualBlock {
    /// The line's edit so far: `I` or `A`, then characters typed.
    edit: Option<(char, u32)>,
    /// The last finished edit, and how many lines in a row had it.
    last: Option<(char, u32)>,
    times: u32,
}

impl VisualBlock {
    const ID: &'static str = "vim.visual-block";
    const LINES: u32 = 3;
}

impl Rule for VisualBlock {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Adding the same text at the start or end of line after line, where a block selection does all at once"
    }

    fn title(&self) -> &'static str {
        "Edit a block of lines"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn threshold(&self) -> Option<u32> {
        Some(Self::LINES)
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        if step.event.is_none() {
            return;
        }
        if step.before == VimMode::Insert {
            match (&mut self.edit, insert_key(step), &step.event) {
                (Some((_, n)), Some(InsertKey::Text), _) => *n += 1,
                (_, _, Some(VimEvent::Escape)) => {}
                _ => {
                    self.edit = None;
                    self.last = None;
                }
            }
            return;
        }
        let Some(c) = command(step) else {
            if !matches!(step.event, Some(VimEvent::Escape)) {
                self.reset();
            }
            return;
        };
        if c.single().is_some_and(|k| k.is_ctrl('v')) {
            out.push(adoption(Self::ID, step));
            self.reset();
            return;
        }
        if c.count.is_none() && (c.is("I") || c.is("A")) {
            self.edit = Some((c.head().unwrap_or('I'), 0));
            return;
        }
        if !(c.count.is_none() && (c.is("j") || c.is("+"))) {
            self.reset();
            return;
        }
        let Some(edit) = self.edit.take().filter(|(_, n)| *n > 0) else {
            self.last = None;
            self.times = 0;
            return;
        };
        if self.last == Some(edit) {
            self.times += 1;
        } else {
            self.last = Some(edit);
            self.times = 1;
        }
        if self.times == Self::LINES {
            let n = self.times;
            let how = if edit.0 == 'I' { "`I`" } else { "`$` then `A`" };
            out.push(finding(
                Self::ID,
                "Edit a block of lines",
                format!(
                    "The same text added to {n} lines one at a time. Select them with `Ctrl-v` and `j`, press {how}, type it once and `Esc`: it goes on every line."
                ),
                Shape::repeat("same text per line", n),
                step,
            ));
        }
    }

    fn reset(&mut self) {
        self.edit = None;
        self.last = None;
        self.times = 0;
    }
}

/// Yank, delete something else, paste the deleted text by mistake, undo:
/// `"0p` pastes the yank.
#[derive(Default)]
pub struct Registers {
    /// 0 nothing, 1 yanked, 2 then deleted, 3 then pasted.
    state: u8,
    at_ms: u64,
}

impl Registers {
    const ID: &'static str = "vim.registers";
    /// Yank to paste: you go somewhere and delete in between.
    const WINDOW_MS: u64 = 60_000;
}

impl Rule for Registers {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Pasting the text you just deleted instead of the text you yanked, then undoing"
    }

    fn title(&self) -> &'static str {
        "Paste the yank"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        let Some(c) = command(step) else { return };
        if step.at_ms.saturating_sub(self.at_ms) > Self::WINDOW_MS {
            self.state = 0;
        }
        let head = c.head();
        let paste = matches!(head, Some('p' | 'P')) && c.keys.len() == 1;
        if paste
            && c.register
                .is_some_and(|r| r == '0' || r.is_ascii_lowercase())
        {
            out.push(adoption(Self::ID, step));
            self.state = 0;
            return;
        }
        // Into a named register or the black hole: nothing to clobber.
        let unnamed = c.register.is_none();
        let yank = head == Some('y');
        let delete = matches!(head, Some('d' | 'c' | 'x' | 'X' | 's' | 'D' | 'C'));
        self.state = match (self.state, yank, delete, paste, c.is("u")) {
            (_, true, _, _, _) if unnamed => 1,
            (1, _, true, _, _) if unnamed => 2,
            (2, _, _, true, _) => 3,
            (3, _, _, _, true) => {
                out.push(finding(
                    Self::ID,
                    "Paste the yank",
                    "The delete replaced what you yanked, so `p` pasted the deleted text. `\"0p` always pastes the last yank.".into(),
                    Shape::fixed("yank, delete, paste, undo"),
                    step,
                ));
                0
            }
            (s @ (1 | 2), _, _, _, _) if crate::motion::is_motion(c) => s,
            _ => 0,
        };
        self.at_ms = step.at_ms;
    }

    fn reset(&mut self) {
        self.state = 0;
    }
}

pub fn case_run() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.case-run",
        description: "Switching case one character at a time with ~",
        title: "Change case by word",
        contexts: LINE_AND_SHELL,
        threshold: 4,
        unit: |s| uncounted_is(s, &["~"]),
        adopted: |s| {
            counted_is(s, &["~"])
                || command(s).is_some_and(|c| {
                    c.keys.len() > 2
                        && c.head() == Some('g')
                        && c.keys[1]
                            .plain_char()
                            .is_some_and(|k| matches!(k, '~' | 'u' | 'U'))
                })
        },
        message: |_, n| {
            format!(
                "`~` {n} times. `g~iw` switches the whole word's case; `gUiw` makes it upper case, `guiw` lower."
            )
        },
    })
}

/// Replacing digits one by one where `Ctrl-a` and `Ctrl-x` count.
#[derive(Default)]
pub struct Increment {
    times: Vec<u64>,
}

impl Increment {
    const ID: &'static str = "vim.increment";
    const TIMES: usize = 3;
    const WINDOW_MS: u64 = 10 * 60_000;
}

impl Rule for Increment {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Changing numbers by retyping digits where Ctrl-a and Ctrl-x add and subtract"
    }

    fn title(&self) -> &'static str {
        "Count with Ctrl-a"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        let Some(c) = command(step) else { return };
        if c.single().is_some_and(|k| k.is_ctrl('a') || k.is_ctrl('x')) {
            out.push(adoption(Self::ID, step));
            self.times.clear();
            return;
        }
        // `r` and a digit: only whether it's a digit is looked at.
        let digit = c.keys.len() == 2
            && c.head() == Some('r')
            && c.keys[1].plain_char().is_some_and(|d| d.is_ascii_digit());
        if !digit {
            return;
        }
        self.times
            .retain(|t| step.at_ms.saturating_sub(*t) <= Self::WINDOW_MS);
        self.times.push(step.at_ms);
        if self.times.len() == Self::TIMES {
            self.times.clear();
            out.push(finding(
                Self::ID,
                "Count with Ctrl-a",
                "Changing numbers digit by digit? `Ctrl-a` adds one to the number under or after the cursor, `5Ctrl-a` adds five, `Ctrl-x` subtracts.".into(),
                Shape::fixed("r then a digit"),
                step,
            ));
        }
    }

    fn reset(&mut self) {}
}

/// Leaving Insert mode for one command and coming straight back.
#[derive(Default)]
pub struct InsertKeys {
    /// Escaped from Insert at this time; then the command run, if one.
    escaped: Option<u64>,
    command: Option<&'static str>,
    /// Earlier round trips: when, and for a paste or a move.
    seen: Vec<(u64, &'static str)>,
}

impl InsertKeys {
    const ID: &'static str = "vim.insert-keys";
    /// It saves a key each time: worth a tip once it's a habit.
    const TIMES: usize = 3;
    const WINDOW_MS: u64 = 5 * 60_000;
}

impl Rule for InsertKeys {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Leaving Insert mode for one command or a paste and going straight back"
    }

    fn title(&self) -> &'static str {
        "Stay in Insert mode"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        if step.event.is_none() {
            return;
        }
        if step.before == VimMode::Insert {
            if matches!(insert_key(step), Some(InsertKey::CtrlO | InsertKey::CtrlR)) {
                out.push(adoption(Self::ID, step));
            }
            self.escaped = matches!(step.event, Some(VimEvent::Escape)).then_some(step.at_ms);
            self.command = None;
            return;
        }
        let Some(at) = self.escaped else { return };
        let Some(c) = command(step).filter(|_| step.at_ms.saturating_sub(at) <= GESTURE_MS) else {
            self.reset();
            return;
        };
        match self.command {
            None if c.count.is_none() && (c.is("p") || c.is("P")) => self.command = Some("p"),
            None if crate::motion::is_motion(c) && c.keys.len() <= 2 => {
                self.command = Some("motion")
            }
            Some(what) if c.is("a") || c.is("i") => {
                let now = step.at_ms;
                self.seen
                    .retain(|(t, w)| *w == what && now.saturating_sub(*t) <= Self::WINDOW_MS);
                self.seen.push((now, what));
                self.reset();
                if self.seen.len() < Self::TIMES {
                    return;
                }
                self.seen.clear();
                let text = if what == "p" {
                    "`Esc`, `p`, then back to typing: `Ctrl-r\"` pastes without leaving Insert mode."
                } else {
                    "`Esc`, one move, then back to typing: `Ctrl-o` runs one Normal-mode command and returns to Insert mode."
                };
                // Insert-mode meanings for Ctrl-o and Ctrl-r.
                out.push(RuleEvent::Finding(Finding {
                    rule_id: Self::ID,
                    title: "Stay in Insert mode",
                    text: text.into(),
                    shape: Shape::fixed(if what == "p" {
                        "Esc p a"
                    } else {
                        "Esc motion a"
                    }),
                    mode: VimMode::Insert,
                    at_ms: step.at_ms,
                    start_ms: step.at_ms,
                }));
            }
            _ => self.reset(),
        }
    }

    fn reset(&mut self) {
        self.escaped = None;
        self.command = None;
    }
}

/// The same operator and motion again and again: `dw dw dw` → `3dw`.
#[derive(Default)]
pub struct OperatorRun {
    /// The operator and motion, how many in a row, and when the last was.
    run: Option<(char, &'static str, u32, u64)>,
}

impl OperatorRun {
    const ID: &'static str = "vim.operator-run";
    const TIMES: u32 = 3;
    /// Longer than a key streak: each one is a small decision.
    const GAP_MS: u64 = 3000;

    fn flush(&mut self, out: &mut Vec<RuleEvent>) {
        let Some((op, motion, n, at_ms)) = self.run.take() else {
            return;
        };
        if n < Self::TIMES {
            return;
        }
        let once = glossary::mark(&format!("{op}{motion}"));
        let counted = glossary::mark(&format!("{n}{op}{motion}"));
        out.push(RuleEvent::Finding(Finding {
            rule_id: Self::ID,
            title: "Use a count",
            text: format!("{once} {n} times. {counted} does it in one go."),
            shape: Shape::repeat("same operator", n),
            mode: VimMode::Normal,
            at_ms,
            start_ms: at_ms,
        }));
    }
}

impl Rule for OperatorRun {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "The same operator and motion again and again (dw dw dw) where a count does it"
    }

    fn title(&self) -> &'static str {
        "Use a count"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        LINE_AND_SHELL
    }

    fn threshold(&self) -> Option<u32> {
        Some(Self::TIMES)
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        if step.event.is_none() {
            return;
        }
        let Some(c) = command(step).filter(|_| !step.before.is_visual()) else {
            if !matches!(step.event, Some(VimEvent::Escape)) {
                self.flush(out);
            }
            return;
        };
        // dd and >> have their own rules; c starts typing in between.
        let this = split_operator(c).filter(|(op, _)| matches!(op, 'd' | 'y'));
        if c.count.is_some() && this.is_some() {
            out.push(adoption(Self::ID, step));
        }
        match (this.filter(|_| c.count.is_none()), self.run) {
            (Some((op, m)), Some((rop, rm, n, last)))
                if op == rop && m == rm && step.at_ms.saturating_sub(last) <= Self::GAP_MS =>
            {
                self.run = Some((op, m, n + 1, step.at_ms));
            }
            (next, _) => {
                self.flush(out);
                self.run = next.map(|(op, m)| (op, m, 1, step.at_ms));
            }
        }
    }

    fn tick(&mut self, now_ms: u64, out: &mut Vec<RuleEvent>) {
        if self
            .run
            .is_some_and(|(_, _, _, last)| now_ms.saturating_sub(last) > Self::GAP_MS)
        {
            self.flush(out);
        }
    }

    fn reset(&mut self) {
        self.run = None;
    }
}

pub fn scroll_run() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.scroll-run",
        description: "Scrolling a line at a time with Ctrl-e or Ctrl-y, many times",
        title: "Scroll further at once",
        contexts: VIM,
        threshold: 8,
        unit: |s| {
            let c = command(s).filter(|c| c.count.is_none())?;
            let k = c.single()?;
            if k.is_ctrl('e') {
                Some("Ctrl-e")
            } else if k.is_ctrl('y') {
                Some("Ctrl-y")
            } else {
                None
            }
        },
        adopted: |s| {
            command(s).is_some_and(|c| {
                c.single()
                    .is_some_and(|k| ['d', 'u', 'f', 'b'].iter().any(|&ch| k.is_ctrl(ch)))
                    || c.is("zt")
                    || c.is("zz")
                    || c.is("zb")
                    || (c.count.is_some()
                        && c.single().is_some_and(|k| k.is_ctrl('e') || k.is_ctrl('y')))
            })
        },
        message: |unit, n| {
            let page = if unit == "Ctrl-e" { "Ctrl-d" } else { "Ctrl-u" };
            format!(
                "`{unit}` {n} times. `{page}` scrolls half a page at once. `zt`, `zz` and `zb` put the cursor's line at the top, middle or bottom."
            )
        },
    })
    .when_held(|unit, secs| {
        let page = if unit == "Ctrl-e" { "Ctrl-d" } else { "Ctrl-u" };
        format!("You held `{unit}` for {secs:.1} s. `{page}` scrolls half a page at once.")
    })
}

/// A search or `:s`, kept as a hash only, to tell if it came again.
fn hash_of(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// Typing the same search or `:s` again, where `n` or `&` repeat it.
#[derive(Default)]
pub struct SearchAgain {
    /// The last search and `:s`, as hashes, and when.
    search: Option<(u64, u64)>,
    substitute: Option<(u64, u64)>,
}

impl SearchAgain {
    const ID: &'static str = "vim.search-again";
    /// Typed again within this long, it's the same hunt.
    const WINDOW_MS: u64 = 120_000;
}

impl Rule for SearchAgain {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Typing the same search or :s again where n or & repeats it"
    }

    fn title(&self) -> &'static str {
        "Repeat it"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        if command(step).is_some_and(|c| c.is("n") || c.is("N") || c.is("&") || c.is("g&")) {
            out.push(adoption(Self::ID, step));
            return;
        }
        let Some(VimEvent::Cmdline {
            kind,
            text: Some(text),
        }) = &step.event
        else {
            return;
        };
        let at = step.at_ms;
        let text = text.trim();
        let (slot, tip, shape) = match kind {
            '/' | '?' if !text.is_empty() => (
                &mut self.search,
                "The same search again. `n` jumps to the next match, `N` back.",
                "same search again",
            ),
            ':' if text.starts_with("s/") || text.starts_with("s#") => (
                &mut self.substitute,
                "The same `:s` again. `&` repeats the last `:s` on this line. `g&` repeats it on every line.",
                "same :s again",
            ),
            _ => return,
        };
        let h = hash_of(text);
        let again =
            slot.is_some_and(|(prev, t)| prev == h && at.saturating_sub(t) <= Self::WINDOW_MS);
        *slot = Some((h, at));
        if again {
            out.push(RuleEvent::Finding(Finding {
                rule_id: Self::ID,
                title: "Repeat it",
                text: tip.into(),
                shape: Shape::fixed(shape),
                mode: VimMode::Normal,
                at_ms: at,
                start_ms: at,
            }));
        }
    }

    fn reset(&mut self) {}
}

// Finding back -------------------------------------------------------------

/// `fx` then `Fx` to go back to the one just passed: `,` does it.
#[derive(Default)]
pub struct FindBack {
    /// The last forward find's target key, and when. Kept in memory only,
    /// for comparing; never put in a tip.
    last: Option<(Key, u64)>,
}

impl FindBack {
    const ID: &'static str = "vim.find-back";
}

impl Rule for FindBack {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Finding a character forward, then finding it again backwards (f then F)"
    }

    fn title(&self) -> &'static str {
        "Back with ,"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        LINE_AND_SHELL
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        let Some(c) = command(step).filter(|_| step.before == VimMode::Normal) else {
            return;
        };
        let last = self.last.take();
        if c.is(",") {
            out.push(adoption(Self::ID, step));
            return;
        }
        let (Some(first), Some(target)) =
            (c.keys.first().and_then(|k| k.plain_char()), c.keys.get(1))
        else {
            return;
        };
        if c.keys.len() != 2 {
            return;
        }
        match first {
            'f' | 't' => self.last = Some((*target, step.at_ms)),
            'F' | 'T'
                if last.is_some_and(|(k, at)| {
                    k == *target && step.at_ms.saturating_sub(at) <= GESTURE_MS * 2
                }) =>
            {
                let back = if first == 'F' { "F<char>" } else { "T<char>" };
                out.push(finding(
                    Self::ID,
                    "Back with ,",
                    format!("`{back}` for the character you just found: `,` goes back to it, and `;` forward again."),
                    Shape::fixed("f then F"),
                    step,
                ));
            }
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.last = None;
    }
}

// Selecting everything --------------------------------------------------------

/// `ggVG` then an operator: `ggdG`, `ggyG`, `gg=G` do it without selecting.
#[derive(Default)]
pub struct SelectAll {
    /// How far into `gg`, `V`, `G` the last commands got.
    progress: u8,
}

impl SelectAll {
    const ID: &'static str = "vim.select-all";
}

impl Rule for SelectAll {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Selecting the whole file to run one operator on it (ggVG then d, y or =)"
    }

    fn title(&self) -> &'static str {
        "No need to select"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        let Some(c) = command(step) else {
            return;
        };
        let progress = std::mem::take(&mut self.progress);
        if c.count.is_none() && c.is("gg") {
            self.progress = 1;
        } else if progress == 1 && c.is("V") {
            self.progress = 2;
        } else if progress == 2 && c.count.is_none() && c.is("G") {
            self.progress = 3;
        } else if progress == 3 && step.before.is_visual() {
            let better = match c.single().and_then(|k| k.plain_char()) {
                Some('d') => "`ggdG`",
                Some('y') => "`ggyG` (or `:%y`)",
                Some('=') => "`gg=G`",
                Some('>') => "`gg>G`",
                Some('<') => "`gg<lt>G`",
                Some('c') => "`ggcG`",
                _ => return,
            };
            out.push(finding(
                Self::ID,
                "No need to select",
                format!(
                    "Selecting the whole file first takes a key more: {better} does it in one go."
                ),
                Shape::fixed("ggVG then an operator"),
                step,
            ));
        } else if progress == 1
            && c.keys.len() == 2
            && c.keys[1].plain_char() == Some('G')
            && "dy=<>c".contains(c.keys[0].plain_char().unwrap_or(' '))
        {
            out.push(adoption(Self::ID, step));
        }
    }

    fn reset(&mut self) {
        self.progress = 0;
    }
}

// Walking through buffers -----------------------------------------------------

/// `:bn` again and again to reach a file: `:b` and part of its name, or
/// `Ctrl-^` for the last one.
#[derive(Default)]
pub struct BufferWalk {
    run: Vec<u64>,
}

impl BufferWalk {
    const ID: &'static str = "vim.buffer-walk";
    const RUN: usize = 3;
}

impl Rule for BufferWalk {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Stepping through open files with :bn or :bp to find one"
    }

    fn title(&self) -> &'static str {
        "Straight to the file"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        let Some(VimEvent::Cmdline {
            kind: ':',
            text: Some(t),
        }) = &step.event
        else {
            if command(step)
                .is_some_and(|c| c.single().is_some_and(|k| k.is_ctrl('^') || k.is_ctrl('6')))
            {
                out.push(adoption(Self::ID, step));
            }
            return;
        };
        let t = t.trim();
        let walking = matches!(
            t,
            "bn" | "bnext" | "bp" | "bprevious" | "bprev" | "bN" | "bNext"
        );
        if !walking {
            if t.starts_with("b ") || t.starts_with("buffer ") {
                out.push(adoption(Self::ID, step));
            }
            self.run.clear();
            return;
        }
        self.run
            .retain(|at| step.at_ms.saturating_sub(*at) <= 10_000);
        self.run.push(step.at_ms);
        if self.run.len() == Self::RUN {
            out.push(finding(
                Self::ID,
                "Straight to the file",
                "`:bn` again and again: `:b` and part of the file's name goes straight there (`Tab` completes it), and `Ctrl-^` back to the last file.".into(),
                Shape::fixed(":bn ×3"),
                step,
            ));
        }
    }

    fn reset(&mut self) {
        self.run.clear();
    }
}

// Windows, files and folds ---------------------------------------------------

/// `Ctrl-w` then an arrow key: `Ctrl-w h` and friends stay on the home row.
#[derive(Default)]
pub struct WindowArrows;

impl WindowArrows {
    const ID: &'static str = "vim.window-arrows";
}

impl Rule for WindowArrows {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Moving between windows with Ctrl-w and an arrow key"
    }

    fn title(&self) -> &'static str {
        "Windows from the home row"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        let Some(c) = command(step) else {
            return;
        };
        if c.keys.len() != 2 || !c.keys[0].is_ctrl('w') {
            return;
        }
        if c.keys[1].is_arrow() {
            out.push(finding(
                Self::ID,
                "Windows from the home row",
                "`Ctrl-w` then an arrow: `Ctrl-w h`, `Ctrl-w j`, `Ctrl-w k` and `Ctrl-w l` do the same without reaching for the arrows.".into(),
                Shape::fixed("Ctrl-w then an arrow"),
                step,
            ));
        } else if c.keys[1].plain_char().is_some_and(|k| "hjkl".contains(k)) {
            out.push(adoption(Self::ID, step));
        }
    }

    fn reset(&mut self) {}
}

/// `:e` back and forth between the same two files: `Ctrl-^` does it.
#[derive(Default)]
pub struct FileFlip {
    /// The last files opened with `:e`, as hashes only, and when.
    opened: Vec<(u64, u64)>,
}

impl FileFlip {
    const ID: &'static str = "vim.file-flip";
    const WITHIN_MS: u64 = 10 * 60_000;
}

impl Rule for FileFlip {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Opening the same two files in turn with :e"
    }

    fn title(&self) -> &'static str {
        "Back to the other file"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        if command(step)
            .is_some_and(|c| c.single().is_some_and(|k| k.is_ctrl('^') || k.is_ctrl('6')))
        {
            out.push(adoption(Self::ID, step));
            return;
        }
        let Some(VimEvent::Cmdline {
            kind: ':',
            text: Some(t),
        }) = &step.event
        else {
            return;
        };
        let t = t.trim();
        let Some(file) = t
            .strip_prefix("e ")
            .or_else(|| t.strip_prefix("edit "))
            .map(str::trim)
            .filter(|f| !f.is_empty())
        else {
            return;
        };
        self.opened
            .retain(|(_, at)| step.at_ms.saturating_sub(*at) <= Self::WITHIN_MS);
        self.opened.push((hash_of(file), step.at_ms));
        // A, B, A: back to the file before.
        if let [.., (a, _), (b, _), (c, _)] = self.opened.as_slice()
            && a == c
            && a != b
        {
            out.push(finding(
                Self::ID,
                "Back to the other file",
                "`:e` back to the file you just left: `Ctrl-^` switches between the last two files in one key.".into(),
                Shape::fixed(":e A, :e B, :e A"),
                step,
            ));
            self.opened.clear();
        }
    }

    fn reset(&mut self) {}
}

/// `zc` then `zo` on and off again: `za` does both.
#[derive(Default)]
pub struct FoldFlip {
    /// The run of `zc`/`zo`, alternating, and when the last came.
    run: Vec<(char, u64)>,
}

impl FoldFlip {
    const ID: &'static str = "vim.fold-flip";
    const GAP_MS: u64 = 30_000;
}

impl Rule for FoldFlip {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Closing and opening the same fold in turn with zc and zo"
    }

    fn title(&self) -> &'static str {
        "One key for both"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        let Some(c) = command(step) else {
            return;
        };
        if c.is("za") {
            out.push(adoption(Self::ID, step));
            self.run.clear();
            return;
        }
        let which = if c.is("zc") {
            'c'
        } else if c.is("zo") {
            'o'
        } else {
            // Anything else, a move too: another fold, maybe.
            self.run.clear();
            return;
        };
        let alternates = self
            .run
            .last()
            .is_some_and(|(w, at)| *w != which && step.at_ms.saturating_sub(*at) <= Self::GAP_MS);
        if !alternates {
            self.run.clear();
        }
        self.run.push((which, step.at_ms));
        if self.run.len() == 3 {
            out.push(finding(
                Self::ID,
                "One key for both",
                "`zc` and `zo` in turn on the same fold: `za` opens it when it's closed and closes it when it's open.".into(),
                Shape::fixed("zc, zo, zc"),
                step,
            ));
        }
    }

    fn reset(&mut self) {
        self.run.clear();
    }
}

pub fn all() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(FindBack::default()),
        Box::new(WindowArrows),
        Box::new(FileFlip::default()),
        Box::new(FoldFlip::default()),
        Box::new(SelectAll::default()),
        Box::new(BufferWalk::default()),
        Box::new(Macro::default()),
        Box::new(DeleteThenInsert::default()),
        Box::new(Shorthand::default()),
        Box::new(VisualOperator::default()),
        Box::new(DotRepeat::default()),
        Box::new(repeat_indent()),
        Box::new(repeat_join()),
        Box::new(repeat_paste()),
        Box::new(undo_run()),
        Box::new(ChangeMatches::default()),
        Box::new(VisualBlock::default()),
        Box::new(Registers::default()),
        Box::new(case_run()),
        Box::new(Increment::default()),
        Box::new(InsertKeys::default()),
        Box::new(OperatorRun::default()),
        Box::new(scroll_run()),
        Box::new(SearchAgain::default()),
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
    fn windows_files_and_folds() {
        assert_eq!(findings(&vim(WindowArrows, "<C-w><Left>")).len(), 1);
        assert_eq!(adoptions(&vim(WindowArrows, "<C-w>h")), 1);
        let flip = vim(FileFlip::default(), ":e a.rs<CR>:e b.rs<CR>:e a.rs<CR>");
        assert_eq!(
            texts(&flip),
            [
                ":e back to the file you just left: Ctrl-^ switches between the last two files in one key."
            ]
        );
        assert!(
            findings(&vim(
                FileFlip::default(),
                ":e a.rs<CR>:e b.rs<CR>:e c.rs<CR>"
            ))
            .is_empty()
        );
        assert_eq!(findings(&vim(FoldFlip::default(), "zczozc")).len(), 1);
        assert!(
            findings(&vim(FoldFlip::default(), "zcjzozc")).is_empty(),
            "a move between"
        );
        assert_eq!(adoptions(&vim(FoldFlip::default(), "za")), 1);
    }

    #[test]
    fn find_back_select_all_buffer_walk() {
        assert_eq!(
            texts(&vim(FindBack::default(), "fxFx")),
            ["F<char> for the character you just found: , goes back to it, and ; forward again."]
        );
        assert!(
            texts(&vim(FindBack::default(), "fxFy")).is_empty(),
            "another character"
        );
        assert_eq!(adoptions(&vim(FindBack::default(), "fx;,")), 1);
        assert_eq!(
            texts(&vim(SelectAll::default(), "ggVGy")),
            ["Selecting the whole file first takes a key more: ggyG (or :%y) does it in one go."]
        );
        assert!(texts(&vim(SelectAll::default(), "ggVjy")).is_empty());
        assert_eq!(adoptions(&vim(SelectAll::default(), "ggdG")), 1);
        let walk = vim(BufferWalk::default(), ":bn<CR>:bn<CR>:bn<CR>");
        assert_eq!(findings(&walk).len(), 1);
        assert!(findings(&vim(BufferWalk::default(), ":bn<CR>:bn<CR>")).is_empty());
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

    #[test]
    fn undo_runs() {
        let ev = vim(undo_run(), "uuuuuuu<Esc>");
        assert_eq!(
            texts(&ev),
            [
                "u 7 times. 7u undoes that many at once. :earlier 1m goes back to how the file was a minute ago."
            ]
        );
        assert!(findings(&vim(undo_run(), "uuuu<Esc>")).is_empty());
        assert_eq!(adoptions(&vim(undo_run(), "5u")), 1);
        assert_eq!(adoptions(&vim(undo_run(), ":earlier 2m<CR>")), 1);
    }

    #[test]
    fn changing_match_after_match() {
        let ev = vim(ChangeMatches::default(), "/foo<CR>cwbar<Esc>n.n.n.n.");
        assert_eq!(findings(&ev).len(), 1, "{ev:?}");
        assert_eq!(findings(&ev)[0].rule_id, "vim.change-matches");
        // Twice is fine; something else in between breaks the run.
        assert!(findings(&vim(ChangeMatches::default(), "n.n.")).is_empty());
        assert!(findings(&vim(ChangeMatches::default(), "n.n.x.n.")).is_empty());
        assert_eq!(
            adoptions(&vim(ChangeMatches::default(), "cgnbar<Esc>...")),
            1
        );
        assert_eq!(
            adoptions(&vim(ChangeMatches::default(), ":%s/a/b/g<CR>")),
            1
        );
    }

    #[test]
    fn more_shorthands() {
        assert_eq!(
            texts(&vim(Shorthand::default(), "Ahi<Esc>A<CR>x<Esc>")),
            ["A then Enter is o: open a line below."]
        );
        assert_eq!(
            texts(&vim(Shorthand::default(), "0ihi<Esc>")),
            [
                "0 then i is gI: insert at the very start of the line (I starts at the first non-blank)."
            ]
        );
        // Typing before Enter is a real append.
        assert!(findings(&vim(Shorthand::default(), "Ahi<CR>")).is_empty());
    }

    #[test]
    fn one_letter_replaced_is_r() {
        assert_eq!(
            texts(&vim(DeleteThenInsert::default(), "xiX<Esc>")),
            [
                "x, i, one letter and Esc is r<char>: replace the character and stay in Normal mode (~ if it's only the case)."
            ]
        );
        assert_eq!(
            texts(&vim(DeleteThenInsert::default(), "xihi<Esc>")),
            ["x then i is s: substitute the character."]
        );
    }

    #[test]
    fn same_text_line_after_line() {
        let ev = vim(VisualBlock::default(), "I// <Esc>jI// <Esc>jI// <Esc>j");
        assert_eq!(findings(&ev).len(), 1, "{ev:?}");
        assert!(texts(&ev)[0].contains("Ctrl-v"));
        // Different edits, or not moving down a line, aren't a block.
        assert!(findings(&vim(VisualBlock::default(), "Ia<Esc>jIab<Esc>jIa<Esc>j")).is_empty());
        assert!(findings(&vim(VisualBlock::default(), "Ia<Esc>wIa<Esc>wIa<Esc>w")).is_empty());
        assert_eq!(
            adoptions(&vim(VisualBlock::default(), "<C-v>jjI// <Esc>")),
            1
        );
        // And it beats the macro tip for the same keys.
        let mut e = crate::engine::Engine::standard();
        let mut t = crate::vim::Tracker::new(Flavor::Vim);
        t.reset_known(VimMode::Normal);
        let mut ids = Vec::new();
        for k in crate::vim::notation::strokes("A;<Esc>jA;<Esc>jA;<Esc>j", 0) {
            let step = t.feed(k);
            for ev in e.on_step(ContextKind::TerminalVim, &step) {
                if let RuleEvent::Finding(f) = ev {
                    ids.push(f.rule_id);
                }
            }
        }
        assert_eq!(ids, ["vim.visual-block"]);
    }

    #[test]
    fn pasting_the_delete_by_mistake() {
        let ev = vim(Registers::default(), "yiwwwdiwpu");
        assert_eq!(findings(&ev).len(), 1, "{ev:?}");
        // Pasted what was meant: no undo.
        assert!(findings(&vim(Registers::default(), "yiwwdiwpx")).is_empty());
        // Deleted into the black hole: the yank is still there.
        assert!(findings(&vim(Registers::default(), "yiww\"_diwpu")).is_empty());
        assert_eq!(adoptions(&vim(Registers::default(), "yiwwdiw\"0p")), 1);
    }

    #[test]
    fn case_and_numbers() {
        assert_eq!(findings(&vim(case_run(), "~~~~~<Esc>")).len(), 1);
        assert_eq!(adoptions(&vim(case_run(), "gUiw")), 1);
        let ev = vim(Increment::default(), "r1wr2wr3");
        assert_eq!(findings(&ev).len(), 1);
        assert!(findings(&vim(Increment::default(), "rxryrz")).is_empty());
        assert_eq!(adoptions(&vim(Increment::default(), "5<C-a>")), 1);
    }

    #[test]
    fn leaving_insert_for_one_command() {
        let ev = vim(InsertKeys::default(), "ihi<Esc>pahi<Esc>pahi<Esc>pa");
        assert_eq!(
            texts(&ev),
            ["Esc, p, then back to typing: Ctrl-r\" pastes without leaving Insert mode."]
        );
        assert_eq!(findings(&ev)[0].mode, VimMode::Insert);
        let ev = vim(InsertKeys::default(), "ihi<Esc>wahi<Esc>wahi<Esc>wa");
        assert_eq!(findings(&ev).len(), 1);
        // Twice isn't a habit yet; more than one command is real editing.
        assert!(findings(&vim(InsertKeys::default(), "ihi<Esc>pahi<Esc>pa")).is_empty());
        assert!(
            findings(&vim(
                InsertKeys::default(),
                "ihi<Esc>wwahi<Esc>wwahi<Esc>wwa"
            ))
            .is_empty()
        );
        assert_eq!(adoptions(&vim(InsertKeys::default(), "ihi<C-o>w")), 1);
    }

    #[test]
    fn operator_runs() {
        assert_eq!(
            texts(&vim(OperatorRun::default(), "dwdwdw<Esc>")),
            ["dw 3 times. 3dw does it in one go."]
        );
        assert!(findings(&vim(OperatorRun::default(), "dwdw<Esc>")).is_empty());
        assert!(findings(&vim(OperatorRun::default(), "dwdedw<Esc>")).is_empty());
        // dd has its own rule.
        assert!(findings(&vim(OperatorRun::default(), "dddddd<Esc>")).is_empty());
        assert_eq!(adoptions(&vim(OperatorRun::default(), "3dw")), 1);
    }

    #[test]
    fn scrolling_line_by_line() {
        let ev = vim(
            scroll_run(),
            "<C-e><C-e><C-e><C-e><C-e><C-e><C-e><C-e><C-e><Esc>",
        );
        assert_eq!(findings(&ev).len(), 1);
        assert!(texts(&ev)[0].contains("Ctrl-d scrolls half a page"));
        assert_eq!(adoptions(&vim(scroll_run(), "zt")), 1);
    }

    #[test]
    fn the_same_search_or_substitute_again() {
        let ev = vim(SearchAgain::default(), "/foo<CR>/foo<CR>");
        assert_eq!(
            texts(&ev),
            ["The same search again. n jumps to the next match, N back."]
        );
        assert!(findings(&vim(SearchAgain::default(), "/foo<CR>/bar<CR>")).is_empty());
        let ev = vim(SearchAgain::default(), ":s/a/b/<CR>j:s/a/b/<CR>");
        assert_eq!(findings(&ev).len(), 1);
        assert_eq!(adoptions(&vim(SearchAgain::default(), "/foo<CR>n&")), 2);
        assert_eq!(
            texts(&vim(Shorthand::default(), ":q!<CR>")),
            ["`:q!` is `ZQ`: quit without saving.".replace('`', "")]
        );
    }
}
