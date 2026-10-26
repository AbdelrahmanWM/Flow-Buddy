//! Shell pack: habits in the commands themselves, from the hook's command
//! shapes (see `CommandShape`). The hook works the shape out from the
//! command line inside the shell; only the shape reaches these rules.

use crate::context::{ContextKind, VimMode};
use crate::ipc::CommandShape;
use crate::rule::{Finding, Rule, RuleEvent, Shape};
use crate::vim::Step;

const SHELL: &[ContextKind] = &[ContextKind::Shell];

/// A command the hook reported, with its shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellEvent {
    pub shape: CommandShape,
    pub at_ms: u64,
}

fn finding(
    id: &'static str,
    title: &'static str,
    text: String,
    shape: Shape,
    at_ms: u64,
) -> RuleEvent {
    RuleEvent::Finding(Finding {
        rule_id: id,
        title,
        text,
        shape,
        // Typed at the prompt, where Insert-mode meanings apply.
        mode: VimMode::Insert,
        at_ms,
    })
}

/// `cd ..` again and again.
#[derive(Default)]
pub struct CdUp {
    run: u32,
    last_ms: u64,
}

impl CdUp {
    const ID: &'static str = "shell.cd-up";
    /// Commands further apart than this aren't one climb.
    const GAP_MS: u64 = 30_000;
}

impl Rule for CdUp {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Climbing up folders with cd .. one at a time"
    }

    fn title(&self) -> &'static str {
        "Go up in one go"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        SHELL
    }

    fn threshold(&self) -> Option<u32> {
        Some(2)
    }

    fn on_step(&mut self, _step: &Step, _out: &mut Vec<RuleEvent>) {}

    fn on_shell(&mut self, ev: &ShellEvent, out: &mut Vec<RuleEvent>) {
        match ev.shape {
            CommandShape::CdUp => {
                if ev.at_ms.saturating_sub(self.last_ms) > Self::GAP_MS {
                    self.run = 0;
                }
                self.run += 1;
                self.last_ms = ev.at_ms;
                if self.run == 2 {
                    out.push(finding(
                        Self::ID,
                        "Go up in one go",
                        "`cd ..` twice in a row. `cd ../..` goes up two at once, and `cd -` takes you back to where you were.".into(),
                        Shape::repeat("cd ..", 2),
                        ev.at_ms,
                    ));
                }
            }
            CommandShape::CdUpMany => {
                self.run = 0;
                out.push(RuleEvent::Adoption {
                    rule_id: Self::ID,
                    at_ms: ev.at_ms,
                });
            }
            _ => self.run = 0,
        }
    }

    fn reset(&mut self) {}
}

/// Typing the previous command's long last argument again.
#[derive(Default)]
pub struct ReusedArg;

impl Rule for ReusedArg {
    fn id(&self) -> &'static str {
        "shell.reused-arg"
    }

    fn description(&self) -> &'static str {
        "Typing the previous command's long last argument again"
    }

    fn title(&self) -> &'static str {
        "Reuse the last argument"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        SHELL
    }

    fn on_step(&mut self, _step: &Step, _out: &mut Vec<RuleEvent>) {}

    fn on_shell(&mut self, ev: &ShellEvent, out: &mut Vec<RuleEvent>) {
        if ev.shape == CommandShape::ReusedArg {
            out.push(finding(
                self.id(),
                "Reuse the last argument",
                "You typed the last argument again. `Alt-.` inserts the previous command's last argument, and `!$` stands for it.".into(),
                Shape::fixed("same long argument again"),
                ev.at_ms,
            ));
        }
    }

    fn reset(&mut self) {}
}

/// Retyping the previous command with one word changed.
#[derive(Default)]
pub struct OneWordChanged;

impl Rule for OneWordChanged {
    fn id(&self) -> &'static str {
        "shell.one-word-changed"
    }

    fn description(&self) -> &'static str {
        "Retyping the previous command to change one word"
    }

    fn title(&self) -> &'static str {
        "Edit the last command"
    }

    fn contexts(&self) -> &'static [ContextKind] {
        SHELL
    }

    fn on_step(&mut self, _step: &Step, _out: &mut Vec<RuleEvent>) {}

    fn on_shell(&mut self, ev: &ShellEvent, out: &mut Vec<RuleEvent>) {
        if ev.shape == CommandShape::OneWordChanged {
            out.push(finding(
                self.id(),
                "Edit the last command",
                "The same command with one word changed. `^old^new` reruns it with the word swapped, or press `Up` and edit it.".into(),
                Shape::fixed("same command, one word changed"),
                ev.at_ms,
            ));
        }
    }

    fn reset(&mut self) {}
}

pub fn all() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(CdUp::default()),
        Box::new(ReusedArg),
        Box::new(OneWordChanged),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(rule: &mut dyn Rule, shapes: &[(CommandShape, u64)]) -> Vec<RuleEvent> {
        let mut out = Vec::new();
        for &(shape, at_ms) in shapes {
            rule.on_shell(&ShellEvent { shape, at_ms }, &mut out);
        }
        out
    }

    #[test]
    fn cd_up_twice() {
        use CommandShape::{CdUp as Up, CdUpMany as UpMany};
        let ev = feed(&mut CdUp::default(), &[(Up, 0), (Up, 5_000), (Up, 9_000)]);
        assert_eq!(ev.len(), 1, "one nudge per climb");
        // Far apart isn't one climb.
        assert!(feed(&mut CdUp::default(), &[(Up, 0), (Up, 60_000)]).is_empty());
        assert_eq!(
            feed(&mut CdUp::default(), &[(UpMany, 0)]),
            [RuleEvent::Adoption {
                rule_id: "shell.cd-up",
                at_ms: 0
            }]
        );
    }

    #[test]
    fn reused_arguments_and_one_word() {
        assert_eq!(
            feed(&mut ReusedArg, &[(CommandShape::ReusedArg, 1)]).len(),
            1
        );
        assert!(feed(&mut ReusedArg, &[(CommandShape::CdUp, 1)]).is_empty());
        assert_eq!(
            feed(&mut OneWordChanged, &[(CommandShape::OneWordChanged, 1)]).len(),
            1
        );
    }

    #[test]
    fn every_command_is_explained() {
        let mut out = Vec::new();
        for mut r in all() {
            for shape in [
                CommandShape::CdUp,
                CommandShape::CdUp,
                CommandShape::ReusedArg,
                CommandShape::OneWordChanged,
            ] {
                r.on_shell(&ShellEvent { shape, at_ms: 0 }, &mut out);
            }
        }
        for ev in &out {
            if let RuleEvent::Finding(f) = ev {
                for span in crate::glossary::code_spans(&f.text) {
                    assert!(
                        !crate::glossary::explain_keys(span, true).is_empty(),
                        "`{span}`"
                    );
                }
            }
        }
    }
}
