//! Vim rule pack: Vim in a terminal and vscodevim.

use crate::context::{ContextKind, VimMode};
use crate::key::Key;
use crate::rule::{Finding, Rule, RuleEvent, Shape};
use crate::rules::repeat::{RepeatRule, RepeatSpec};
use crate::vim::{Command, InsertKey, Step, VimEvent};

const VIM: &[ContextKind] = &[ContextKind::TerminalVim, ContextKind::VsCodeVim];

/// The command, if this step completed a Normal/Visual-mode command.
/// Operator commands such as `dw` complete from OpPending.
fn command(step: &Step) -> Option<&Command> {
    let from_normal = matches!(step.before, VimMode::Normal | VimMode::OpPending);
    match &step.event {
        Some(VimEvent::Command(c)) if from_normal || step.before.is_visual() => Some(c),
        _ => None,
    }
}

fn insert_key(step: &Step) -> Option<InsertKey> {
    match step.event {
        Some(VimEvent::Insert(k)) if step.before == VimMode::Insert => Some(k),
        _ => None,
    }
}

/// An uncounted single-key command that is one of `chars`.
fn uncounted<'a>(step: &'a Step, chars: &[char]) -> Option<&'a Command> {
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
            format!("You pressed {unit} {n} times. Try {n}{unit}, {down} or {up}.")
        },
    })
}

pub fn repeat_hl() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.repeat-hl",
        description: "Pressing h or l many times instead of a word or find motion",
        title: "Move by word",
        contexts: VIM,
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
                ("w or e", "f<char>")
            } else {
                ("b", "F<char>")
            };
            format!("You pressed {unit} {n} times. Try {word}, or jump with {find}.")
        },
    })
}

pub fn repeat_x() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.repeat-x",
        description: "Deleting character by character with x",
        title: "Delete by word",
        contexts: VIM,
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
        message: |_, n| format!("You pressed x {n} times. Try dw, de or D."),
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
        message: |_, n| format!("{n} arrow keys in Normal mode. Use h j k l, your hands stay put."),
    })
}

pub fn arrows_insert() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.arrows-insert",
        description: "Moving around with arrow keys while in Insert mode",
        title: "Leave Insert to move",
        contexts: VIM,
        threshold: 4,
        unit: |s| matches!(insert_key(s), Some(InsertKey::Arrow(_))).then_some("arrow"),
        adopted: |s| matches!(insert_key(s), Some(InsertKey::CtrlO)),
        message: |_, n| {
            format!(
                "{n} arrow keys in Insert mode. Press Esc, move with a motion, then i/a/A to continue."
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
        message: |_, n| format!("You pressed dd {n} times. Try {n}dd, d{}j or dap.", n - 1),
    })
}

pub fn backspace_insert() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.backspace-streak",
        description: "Holding Backspace in Insert mode to delete words",
        title: "Delete by word",
        contexts: VIM,
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
            format!("{n} Backspaces in a row. Try Ctrl-w to delete a word, or Esc then ciw.")
        },
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
                    text: ":w then :q can be one command: :wq, :x or ZZ.".into(),
                    shape: Shape::fixed(":w then :q"),
                    mode: VimMode::Cmdline,
                    at_ms: step.at_ms,
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

pub fn all() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(repeat_jk()),
        Box::new(repeat_hl()),
        Box::new(repeat_x()),
        Box::new(arrows_normal()),
        Box::new(arrows_insert()),
        Box::new(repeat_dd()),
        Box::new(backspace_insert()),
        Box::new(WriteThenQuit::default()),
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
}
