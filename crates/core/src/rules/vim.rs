//! Vim rule pack: Vim in a terminal and vscodevim.

use crate::context::{ContextKind, VimMode};
use crate::key::Key;
use crate::rule::{Finding, Rule, RuleEvent, Shape};
use crate::rules::repeat::{RepeatRule, RepeatSpec};
use crate::vim::{Command, InsertKey, Step, VimEvent};

const VIM: &[ContextKind] = &[ContextKind::TerminalVim, ContextKind::VsCodeVim];
/// Rules about editing one line, which also apply to Claude Code's prompt.
const LINE: &[ContextKind] = &[
    ContextKind::TerminalVim,
    ContextKind::VsCodeVim,
    ContextKind::ClaudeVim,
];
/// Line editing in a vi-mode shell's command line too; the shells' own
/// rules cover arrows and Backspace there.
pub(crate) const LINE_AND_SHELL: &[ContextKind] = &[
    ContextKind::TerminalVim,
    ContextKind::VsCodeVim,
    ContextKind::ClaudeVim,
    ContextKind::BashVi,
];

/// The command, if this step completed a Normal/Visual-mode command.
/// Operator commands such as `dw` complete from OpPending.
pub(crate) fn command(step: &Step) -> Option<&Command> {
    let from_normal = matches!(step.before, VimMode::Normal | VimMode::OpPending);
    match &step.event {
        Some(VimEvent::Command(c)) if from_normal || step.before.is_visual() => Some(c),
        _ => None,
    }
}

pub(crate) fn insert_key(step: &Step) -> Option<InsertKey> {
    match step.event {
        Some(VimEvent::Insert(k)) if step.before == VimMode::Insert => Some(k),
        _ => None,
    }
}

/// An uncounted single-key command that is one of `chars`.
pub(crate) fn uncounted<'a>(step: &'a Step, chars: &[char]) -> Option<&'a Command> {
    command(step).filter(|c| {
        c.count.is_none()
            && c.single()
                .and_then(|k| k.plain_char())
                .is_some_and(|ch| chars.contains(&ch))
    })
}

fn token(c: char) -> &'static str {
    match c {
        'j' => "j",
        'k' => "k",
        'h' => "h",
        'l' => "l",
        _ => "?",
    }
}

pub fn repeat_jk() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.repeat-jk",
        description: "Pressing j or k many times instead of a count or a bigger motion",
        title: "Try a count",
        contexts: VIM,
        threshold: 5,
        unit: |s| uncounted(s, &['j', 'k']).and_then(|c| c.head()).map(token),
        adopted: |s| {
            command(s).is_some_and(|c| {
                (c.count.is_some() && matches!(c.head(), Some('j' | 'k')))
                    || matches!(c.head(), Some('}' | '{' | 'G' | 'H' | 'M' | 'L'))
                    || c.is("gg")
                    || c.single()
                        .is_some_and(|k| ['d', 'u', 'f', 'b'].iter().any(|&ch| k.is_ctrl(ch)))
            })
        },
        message: |unit, n| {
            let (down, up) = if unit == "j" {
                ("}", "Ctrl-d")
            } else {
                ("{", "Ctrl-u")
            };
            // A big count is right but useless: nobody counts 40 lines.
            if n <= 9 {
                format!("You pressed {unit} {n} times. Try `{n}{unit}`, `{down}` or `{up}`.")
            } else {
                format!(
                    "You pressed {unit} {n} times. For long moves, `{up}` jumps half a page and `{down}` to the next blank line; or search with `/`."
                )
            }
        },
    })
    .replaced_by_editor()
    .when_held(|unit, secs| {
        let (page, para) = if unit == "j" { ("Ctrl-d", "}") } else { ("Ctrl-u", "{") };
        format!("You held {unit} for {secs:.1} s. `{page}` moves half a page per press, `{para}` jumps to the next blank line.")
    })
}

pub fn repeat_hl() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.repeat-hl",
        description: "Pressing h or l many times instead of a word or find motion",
        title: "Move by word",
        contexts: LINE_AND_SHELL,
        threshold: 5,
        unit: |s| uncounted(s, &['h', 'l']).and_then(|c| c.head()).map(token),
        adopted: |s| {
            command(s).is_some_and(|c| {
                matches!(
                    c.head(),
                    Some(
                        'w' | 'b' | 'e' | 'W' | 'B' | 'E' | 'f' | 't' | 'F' | 'T' | '0' | '^' | '$'
                    )
                ) || (c.count.is_some() && matches!(c.head(), Some('h' | 'l')))
            })
        },
        message: |unit, n| {
            let (word, find) = if unit == "l" {
                ("`w` or `e`", "`f<char>`")
            } else {
                ("`b`", "`F<char>`")
            };
            format!("You pressed {unit} {n} times. Try {word}, or jump with {find}.")
        },
    })
    .replaced_by_editor()
    .when_held(|unit, secs| {
        let (word, find) = if unit == "l" { ("w", "f<char>") } else { ("b", "F<char>") };
        format!("You held {unit} for {secs:.1} s. `{word}` moves a word per press; `{find}` jumps straight to a character.")
    })
}

pub fn repeat_x() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.repeat-x",
        description: "Deleting character by character with x",
        title: "Delete by word",
        contexts: LINE_AND_SHELL,
        threshold: 4,
        unit: |s| uncounted(s, &['x']).map(|_| "x"),
        adopted: |s| {
            command(s).is_some_and(|c| {
                matches!(c.head(), Some('D' | 'C'))
                    || (c.count.is_some() && c.is("x"))
                    || (matches!(c.head(), Some('d' | 'c'))
                        && c.keys.get(1).and_then(Key::plain_char).is_some_and(|m| {
                            matches!(m, 'w' | 'e' | 'W' | 'E' | '$' | 't' | 'f' | 'i' | 'a')
                        }))
            })
        },
        message: |_, n| format!("You pressed x {n} times. Try `dw`, `de` or `D`."),
    })
    .when_held(|_, secs| {
        format!("You held x for {secs:.1} s. `dw` deletes the word, `D` the rest of the line.")
    })
}

fn is_arrow_command(step: &Step) -> bool {
    command(step).is_some_and(|c| c.single().is_some_and(|k| k.is_arrow()))
}

pub fn arrows_normal() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.arrows-normal",
        description: "Using arrow keys to move in Normal or Visual mode",
        title: "Stay on the home row",
        contexts: VIM,
        threshold: 3,
        unit: |s| is_arrow_command(s).then_some("arrow"),
        adopted: |s| {
            uncounted(s, &['h', 'j', 'k', 'l']).is_some()
                || command(s).is_some_and(|c| {
                    c.count.is_some() && matches!(c.head(), Some('h' | 'j' | 'k' | 'l'))
                })
        },
        message: |_, n| {
            format!("{n} arrow keys in Normal mode. Use `h` `j` `k` `l`, your hands stay put.")
        },
    })
}

pub fn arrows_insert() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.arrows-insert",
        description: "Moving around with arrow keys while in Insert mode",
        title: "Leave Insert to move",
        contexts: LINE,
        threshold: 4,
        unit: |s| matches!(insert_key(s), Some(InsertKey::Arrow(_))).then_some("arrow"),
        adopted: |s| matches!(insert_key(s), Some(InsertKey::CtrlO)),
        message: |_, n| {
            format!(
                "{n} arrow keys in Insert mode. Press `Esc`, move with a motion, then `i`, `a` or `A` to continue."
            )
        },
    })
}

pub fn repeat_dd() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.repeat-dd",
        description: "Deleting line by line with repeated dd",
        title: "Count your deletes",
        contexts: VIM,
        threshold: 3,
        unit: |s| {
            command(s)
                .filter(|c| c.count.is_none() && c.is("dd"))
                .map(|_| "dd")
        },
        adopted: |s| {
            command(s).is_some_and(|c| {
                (c.count.is_some() && c.is("dd"))
                    || c.is("dap")
                    || c.is("dip")
                    || c.is("dG")
                    || c.is("dgg")
                    || c.is("d}")
                    || c.is("d{")
                    || (c.head() == Some('d')
                        && c.keys.len() >= 3
                        && c.keys[1].plain_char().is_some_and(|d| d.is_ascii_digit()))
            })
        },
        message: |_, n| {
            format!(
                "You pressed dd {n} times. Try `{n}dd`, `d{}j` or `dap`.",
                n - 1
            )
        },
    })
}

pub fn backspace_insert() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.backspace-streak",
        description: "Holding Backspace in Insert mode to delete words",
        title: "Delete by word",
        contexts: LINE,
        threshold: 6,
        unit: |s| matches!(insert_key(s), Some(InsertKey::Backspace)).then_some("BS"),
        adopted: |s| {
            matches!(insert_key(s), Some(InsertKey::CtrlW | InsertKey::CtrlU))
                || command(s).is_some_and(|c| {
                    ["db", "dB", "diw", "daw", "ciw", "caw", "cb", "cB"]
                        .iter()
                        .any(|x| c.is(x))
                })
        },
        message: |_, n| {
            format!("{n} Backspaces in a row. Try `Ctrl-w` to delete a word, or `Esc` then `ciw`.")
        },
    })
    .when_held(|_, secs| {
        format!("You held Backspace for {secs:.1} s. `Ctrl-w` deletes a word at a time, `Ctrl-u` back to the line start.")
    })
}

/// `:w` immediately followed by `:q`.
#[derive(Default)]
pub struct WriteThenQuit {
    wrote_at: Option<u64>,
}

impl WriteThenQuit {
    const ID: &'static str = "vim.w-then-q";
}

fn cmdline_text(step: &Step) -> Option<&str> {
    match &step.event {
        Some(VimEvent::Cmdline {
            kind: ':',
            text: Some(t),
        }) => Some(t.trim()),
        _ => None,
    }
}

impl Rule for WriteThenQuit {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Saving with :w and then quitting with :q"
    }

    fn title(&self) -> &'static str {
        "Save and quit at once"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        let zz = command(step).is_some_and(|c| c.is("ZZ"));
        match cmdline_text(step) {
            Some("wq" | "x" | "wqa" | "xa" | "wq!" | "x!") => {}
            Some("w") => {
                self.wrote_at = Some(step.at_ms);
                return;
            }
            Some("q" | "q!") if self.wrote_at.is_some() => {
                self.wrote_at = None;
                out.push(RuleEvent::Finding(Finding {
                    rule_id: Self::ID,
                    title: "Save and quit at once",
                    text: "`:w` then `:q` can be one command: `:wq`, `:x` or `ZZ`.".into(),
                    shape: Shape::fixed(":w then :q"),
                    mode: VimMode::Cmdline,
                    at_ms: step.at_ms,
                    start_ms: step.at_ms,
                }));
                return;
            }
            _ if !zz => {
                // Escape and half-typed commands keep the :w pending;
                // anything else (more editing) clears it.
                let neutral = matches!(step.event, None | Some(VimEvent::Escape))
                    || matches!(step.event, Some(VimEvent::Command(ref c)) if c.single() == Some(Key::ch(':')));
                if !neutral {
                    self.wrote_at = None;
                }
                return;
            }
            _ => {}
        }
        self.wrote_at = None;
        out.push(RuleEvent::Adoption {
            rule_id: Self::ID,
            at_ms: step.at_ms,
        });
    }

    fn reset(&mut self) {
        self.wrote_at = None;
    }
}

/// Cycling windows with `Ctrl-w w` or buffers with `:bn` to reach one.
#[derive(Default)]
pub struct Windows {
    /// The last step's kind (`w` window, `b` buffer), how many in a row,
    /// and when.
    run: Option<(char, u32, u64)>,
}

impl Windows {
    const ID: &'static str = "vim.windows";
    const TIMES: u32 = 3;
    const GAP_MS: u64 = 10_000;
}

fn is_ctrl_caret(k: &Key) -> bool {
    k.is_ctrl('^') || k.is_ctrl('6')
}

impl Rule for Windows {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Cycling through windows or buffers one at a time to reach the one you want"
    }

    fn title(&self) -> &'static str {
        "Go straight to the window"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        // vscodevim moves between editor groups with Ctrl-w h/j/k/l.
        VIM
    }

    fn threshold(&self) -> Option<u32> {
        Some(Self::TIMES)
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        if step.event.is_none() {
            return;
        }
        let c = command(step);
        let window = c.filter(|c| c.count.is_none() && c.keys.len() == 2 && c.keys[0].is_ctrl('w'));
        let adopted = window
            .is_some_and(|c| c.keys[1].plain_char().is_some_and(|k| "hjkl".contains(k)))
            || c.and_then(|c| c.single())
                .is_some_and(|k| is_ctrl_caret(&k))
            || cmdline_text(step).is_some_and(|t| t.starts_with("b ") || t.starts_with("buffer "));
        if adopted {
            out.push(RuleEvent::Adoption {
                rule_id: Self::ID,
                at_ms: step.at_ms,
            });
            self.run = None;
            return;
        }
        if cmdline_text(step).is_some_and(|t| matches!(t, "e #" | "e#" | "edit #")) {
            out.push(RuleEvent::Finding(Finding {
                rule_id: Self::ID,
                title: "Go straight to the window",
                text: "`:e #` is `Ctrl-^`: back to the file you had open before.".into(),
                shape: Shape::fixed(":e #"),
                mode: VimMode::Cmdline,
                at_ms: step.at_ms,
                start_ms: step.at_ms,
            }));
            return;
        }
        let kind = if window
            .is_some_and(|c| c.keys[1].plain_char() == Some('w') || c.keys[1].is_ctrl('w'))
        {
            'w'
        } else if cmdline_text(step)
            .is_some_and(|t| matches!(t, "bn" | "bnext" | "bp" | "bprev" | "bprevious" | "bN"))
        {
            'b'
        } else {
            // Typing the `:` of the next `:bn` doesn't break the run.
            if !matches!(step.event, Some(VimEvent::Escape))
                && !c.is_some_and(|c| c.single() == Some(Key::ch(':')))
            {
                self.run = None;
            }
            return;
        };
        let n = match self.run {
            Some((k, n, at)) if k == kind && step.at_ms.saturating_sub(at) <= Self::GAP_MS => n + 1,
            _ => 1,
        };
        self.run = Some((kind, n, step.at_ms));
        if n != Self::TIMES {
            return;
        }
        let (shape, text, mode) = if kind == 'w' {
            (
                "Ctrl-w w again and again",
                format!(
                    "`Ctrl-w w` {n} times to cycle windows. `Ctrl-w h`, `Ctrl-w j`, `Ctrl-w k` and `Ctrl-w l` go straight to the window in that direction."
                ),
                VimMode::Normal,
            )
        } else {
            (
                ":bn again and again",
                format!(
                    "`:bn` {n} times to reach a file. `:b` and part of its name jumps straight to it; `Ctrl-^` switches back to the last one."
                ),
                VimMode::Cmdline,
            )
        };
        out.push(RuleEvent::Finding(Finding {
            rule_id: Self::ID,
            title: "Go straight to the window",
            text,
            shape: Shape::fixed(shape),
            mode,
            at_ms: step.at_ms,
            start_ms: step.at_ms,
        }));
    }

    fn reset(&mut self) {
        self.run = None;
    }
}

/// Going too far and coming back: `j`×9 then `k`×2, where `7j` lands.
/// Keys alone give the exact numbers, so this works without a plugin.
#[derive(Default)]
pub struct Overshoot {
    /// The run before this one, and this one: (key, presses, first, last).
    prev: Option<(char, u32, u64, u64)>,
    run: Option<(char, u32, u64, u64)>,
}

impl Overshoot {
    const ID: &'static str = "vim.overshoot";
    /// At least this many presses out, for it to be worth a count.
    const OUT: u32 = 4;
    const GAP_MS: u64 = 1500;

    fn opposite(a: char, b: char) -> bool {
        matches!(
            (a, b),
            ('j', 'k') | ('k', 'j') | ('l', 'h') | ('h', 'l') | ('w', 'b') | ('b', 'w')
        )
    }

    /// A run ended: was it the way back from an overshoot?
    fn finish(&mut self, out: &mut Vec<RuleEvent>) {
        let Some(back) = self.run.take() else { return };
        let Some(out_run) = self.prev.replace(back) else {
            return;
        };
        let (k1, n1, start, end1) = out_run;
        let (k2, n2, start2, end2) = back;
        if !Self::opposite(k1, k2)
            || n1 < Self::OUT
            || n2 >= n1
            || n2 > 3
            || start2.saturating_sub(end1) > Self::GAP_MS
        {
            return;
        }
        let net = n1 - n2;
        let better = if net == 1 {
            format!("`{k1}`")
        } else if net <= 9 {
            format!("`{net}{k1}`")
        } else {
            return;
        };
        self.prev = None;
        out.push(RuleEvent::Finding(Finding {
            rule_id: Self::ID,
            title: "Count it out",
            text: format!("`{k1}`×{n1} then `{k2}`×{n2}: {better} lands there in one go."),
            shape: Shape::fixed("too far, then back"),
            mode: VimMode::Normal,
            at_ms: end2,
            start_ms: start,
        }));
    }
}

impl Rule for Overshoot {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Going too far with j, l or w and coming back, where a count lands right away"
    }

    fn title(&self) -> &'static str {
        "Count it out"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        LINE_AND_SHELL
    }

    // An editor's motion facts do this exactly.
    fn replaced_by_editor(&self) -> bool {
        true
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        if step.event.is_none() {
            return;
        }
        let key = uncounted(step, &['j', 'k', 'h', 'l', 'w', 'b']).and_then(|c| c.head());
        let at = step.at_ms;
        match (key, self.run) {
            (Some(k), Some((r, n, first, last)))
                if k == r && at.saturating_sub(last) <= Self::GAP_MS =>
            {
                self.run = Some((r, n + 1, first, at));
            }
            (Some(k), _) => {
                self.finish(out);
                self.run = Some((k, 1, at, at));
            }
            (None, _) => {
                self.finish(out);
                self.prev = None;
            }
        }
    }

    fn tick(&mut self, now_ms: u64, out: &mut Vec<RuleEvent>) {
        if self
            .run
            .is_some_and(|(_, _, _, last)| now_ms.saturating_sub(last) > Self::GAP_MS)
        {
            self.finish(out);
            self.prev = None;
        }
    }

    fn reset(&mut self) {
        self.prev = None;
        self.run = None;
    }
}

pub fn find_repeat() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.find-repeat",
        description: "Pressing ; again and again after f or t",
        title: "Count the find",
        contexts: LINE_AND_SHELL,
        threshold: 3,
        unit: |s| uncounted(s, &[';']).map(|_| ";"),
        adopted: |s| {
            command(s)
                .is_some_and(|c| c.count.is_some() && c.head().is_some_and(|h| "fFtT".contains(h)))
        },
        message: |_, n| {
            if n < 9 {
                format!(
                    "`;` {n} times after `f`. `{}f<char>` gets to that one at once. `/` and a few letters works too.",
                    n + 1
                )
            } else {
                format!(
                    "`;` {n} times after `f`. `/` and a few letters of where you're going gets there at once."
                )
            }
        },
    })
}

pub fn all() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(Overshoot::default()),
        Box::new(find_repeat()),
        Box::new(repeat_jk()),
        Box::new(repeat_hl()),
        Box::new(repeat_x()),
        Box::new(arrows_normal()),
        Box::new(arrows_insert()),
        Box::new(repeat_dd()),
        Box::new(backspace_insert()),
        Box::new(WriteThenQuit::default()),
        Box::new(Windows::default()),
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

    #[test]
    fn jk_streak_is_found_with_count_in_message() {
        let ev = vim(repeat_jk(), "jjjjjjj<Esc>");
        let f = findings(&ev);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].shape.to_string(), "j×7");
        assert!(f[0].text.contains("7j"), "{}", f[0].text);
    }

    #[test]
    fn long_jk_streaks_suggest_paging_not_counts() {
        let ev = vim(repeat_jk(), &format!("{}<Esc>", "j".repeat(30)));
        let text = &findings(&ev)[0].text;
        assert!(!text.contains("30j"), "{text}");
        assert!(text.contains("Ctrl-d"), "{text}");
    }

    #[test]
    fn jk_short_or_counted_is_fine() {
        assert!(findings(&vim(repeat_jk(), "jjjj<Esc>")).is_empty());
        let ev = vim(repeat_jk(), "7j}<C-d>gg");
        assert!(findings(&ev).is_empty());
        assert_eq!(adoptions(&ev), 4);
    }

    #[test]
    fn jk_arguments_are_not_motions() {
        // fj and rj consume j as an argument.
        assert!(findings(&vim(repeat_jk(), "fjfjfjfjfjrj<Esc>")).is_empty());
    }

    #[test]
    fn jk_in_insert_mode_is_text() {
        assert!(findings(&vim(repeat_jk(), "ijjjjjjjj<Esc>")).is_empty());
    }

    #[test]
    fn hl_streak() {
        assert_eq!(findings(&vim(repeat_hl(), "llllll<Esc>")).len(), 1);
        assert!(findings(&vim(repeat_hl(), "lll<Esc>")).is_empty());
        assert_eq!(adoptions(&vim(repeat_hl(), "wbefx")), 4);
    }

    #[test]
    fn x_streak() {
        let f = vim(repeat_x(), "xxxxx<Esc>");
        assert_eq!(findings(&f)[0].shape.to_string(), "x×5");
        assert!(findings(&vim(repeat_x(), "xxx<Esc>")).is_empty());
        assert_eq!(adoptions(&vim(repeat_x(), "dwdeDcw")), 4);
    }

    #[test]
    fn arrows_in_normal() {
        assert_eq!(
            findings(&vim(arrows_normal(), "<Down><Down><Down><Esc>")).len(),
            1
        );
        assert!(findings(&vim(arrows_normal(), "<Down><Esc>")).is_empty());
        assert!(findings(&vim(arrows_normal(), "i<Down><Down><Down><Esc>")).is_empty());
    }

    #[test]
    fn arrows_in_insert() {
        assert_eq!(
            findings(&vim(arrows_insert(), "i<Left><Left><Left><Left><Esc>")).len(),
            1
        );
        assert!(findings(&vim(arrows_insert(), "i<Left><Esc>")).is_empty());
        assert!(findings(&vim(arrows_insert(), "<Left><Left><Left><Left><Esc>")).is_empty());
        assert_eq!(adoptions(&vim(arrows_insert(), "i<C-o>")), 1);
    }

    #[test]
    fn dd_streak() {
        let ev = vim(repeat_dd(), "dddddd<Esc>");
        let f = findings(&ev);
        assert_eq!(f[0].shape.to_string(), "dd×3");
        assert!(
            f[0].text.contains("3dd") && f[0].text.contains("d2j"),
            "{}",
            f[0].text
        );
        assert!(findings(&vim(repeat_dd(), "dddd<Esc>")).is_empty());
        assert_eq!(adoptions(&vim(repeat_dd(), "3dddapd3j")), 3);
    }

    #[test]
    fn backspace_in_insert() {
        assert_eq!(
            findings(&vim(backspace_insert(), "i<BS><BS><BS><BS><BS><BS><Esc>")).len(),
            1
        );
        assert!(findings(&vim(backspace_insert(), "i<BS><BS><Esc>")).is_empty());
        assert_eq!(adoptions(&vim(backspace_insert(), "i<C-w><Esc>ciw")), 2);
    }

    #[test]
    fn write_then_quit() {
        let ev = vim(WriteThenQuit::default(), ":w<CR>:q<CR>");
        assert_eq!(findings(&ev)[0].shape.to_string(), ":w then :q");
        assert!(findings(&vim(WriteThenQuit::default(), ":w<CR>ihello<Esc>:q<CR>")).is_empty());
        assert_eq!(
            adoptions(&vim(WriteThenQuit::default(), ":wq<CR>ZZ:x<CR>")),
            3
        );
    }

    #[test]
    fn findings_never_contain_typed_text() {
        let typed = "isecretpassword<Esc>";
        for rule in all() {
            let ev = run(
                rule,
                Flavor::Vim,
                VimMode::Normal,
                &format!("{typed}jjjjjjxxxxx:w<CR>:q<CR>"),
            );
            for f in findings(&ev) {
                assert!(!f.text.contains("secret") && !f.shape.to_string().contains("secret"));
            }
        }
    }

    #[test]
    fn cycling_windows_and_buffers() {
        let ev = vim(Windows::default(), "<C-w>w<C-w>w<C-w>w");
        assert_eq!(findings(&ev).len(), 1, "{ev:?}");
        let ev = vim(Windows::default(), ":bn<CR>:bn<CR>:bn<CR>");
        assert!(findings(&ev)[0].text.contains("`:b`"));
        assert_eq!(findings(&vim(Windows::default(), ":e #<CR>")).len(), 1);
        assert!(findings(&vim(Windows::default(), "<C-w>wj<C-w>wj<C-w>w")).is_empty());
        assert_eq!(adoptions(&vim(Windows::default(), "<C-w>l<C-^>")), 2);
    }

    #[test]
    fn window_commands_take_two_keys() {
        // Ctrl-w w is one command, not Ctrl-w and a word motion.
        let ev = vim(repeat_hl(), "<C-w>l<C-w>l<C-w>l<C-w>l<C-w>l<C-w>l");
        assert!(findings(&ev).is_empty());
    }

    #[test]
    fn overshooting_and_coming_back() {
        let ev = vim(Overshoot::default(), "jjjjjjjjjkk<Esc>");
        let f = findings(&ev);
        assert_eq!(f.len(), 1, "{ev:?}");
        assert_eq!(f[0].text, "`j`×9 then `k`×2: `7j` lands there in one go.");
        // Coming back as far: not an overshoot. A short trip out: fine.
        assert!(findings(&vim(Overshoot::default(), "jjjjjkkkkk<Esc>")).is_empty());
        assert!(findings(&vim(Overshoot::default(), "jjjk<Esc>")).is_empty());
        assert_eq!(
            findings(&vim(Overshoot::default(), "llllllh<Esc>")).len(),
            1
        );
    }

    #[test]
    fn semicolons_after_find() {
        let ev = vim(find_repeat(), "fx;;;;<Esc>");
        assert_eq!(
            findings(&ev)[0].text.split('.').next().unwrap(),
            "`;` 4 times after `f`"
        );
        assert!(findings(&ev)[0].text.contains("`5f<char>`"));
        assert_eq!(adoptions(&vim(find_repeat(), "3fx")), 1);
    }
}
