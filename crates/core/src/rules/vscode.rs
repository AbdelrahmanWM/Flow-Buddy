//! VS Code pack: the editor's own features, beyond Vim inside it.
//!
//! The companion reports UI events by name only (a file opened, the
//! terminal took focus); the daemon adds what it saw just before: a click,
//! or the shortcut.

use crate::context::{ContextKind, VimMode};
use crate::rule::{Finding, Rule, RuleEvent, Shape};
use crate::vim::Step;

const VSCODE: &[ContextKind] = &[ContextKind::VsCodeVim];

/// What happened in VS Code, and how the user got there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiAction {
    pub what: UiWhat,
    pub how: How,
    pub at_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiWhat {
    FileOpened,
    TerminalFocused,
    TabSwitched,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    /// The rule's shortcut was pressed just before.
    Shortcut,
    /// A click came just before, and no shortcut.
    Click,
    /// Neither: something else did it (go to definition, an extension).
    Other,
}

/// `count` clicks for `what` within `window_ms` make a habit.
struct ClickHabit {
    id: &'static str,
    what: UiWhat,
    count: u32,
    window_ms: u64,
    clicks: Vec<u64>,
    title: &'static str,
    text: &'static str,
    shape: &'static str,
    description: &'static str,
}

impl Rule for ClickHabit {
    fn id(&self) -> &'static str {
        self.id
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn title(&self) -> &'static str {
        self.title
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VSCODE
    }

    fn threshold(&self) -> Option<u32> {
        Some(self.count)
    }

    fn set_threshold(&mut self, n: u32) {
        self.count = n.max(1);
    }

    fn needs_editor(&self) -> bool {
        true
    }

    fn on_step(&mut self, _step: &Step, _out: &mut Vec<RuleEvent>) {}

    fn on_ui(&mut self, ev: &UiAction, out: &mut Vec<RuleEvent>) {
        if ev.what != self.what {
            return;
        }
        match ev.how {
            How::Shortcut => {
                self.clicks.clear();
                out.push(RuleEvent::Adoption {
                    rule_id: self.id,
                    at_ms: ev.at_ms,
                });
            }
            How::Click => {
                self.clicks
                    .retain(|t| ev.at_ms.saturating_sub(*t) <= self.window_ms);
                self.clicks.push(ev.at_ms);
                if self.clicks.len() as u32 >= self.count {
                    self.clicks.clear();
                    out.push(RuleEvent::Finding(Finding {
                        rule_id: self.id,
                        title: self.title,
                        text: self.text.into(),
                        shape: Shape::repeat(self.shape, self.count),
                        mode: VimMode::Normal,
                        at_ms: ev.at_ms,
                    }));
                }
            }
            How::Other => {}
        }
    }

    // Habits over minutes: kept across context changes.
    fn reset(&mut self) {}
}

pub fn quick_open() -> Box<dyn Rule> {
    Box::new(ClickHabit {
        id: "vscode.quick-open",
        what: UiWhat::FileOpened,
        count: 3,
        window_ms: 10 * 60_000,
        clicks: Vec::new(),
        title: "Open files by name",
        text: "You clicked your way to files 3 times. `Ctrl-P` opens one by typing a few letters of its name.",
        shape: "files opened by clicking",
        description: "Opening files by clicking through the explorer instead of Ctrl-P",
    })
}

pub fn terminal_toggle() -> Box<dyn Rule> {
    Box::new(ClickHabit {
        id: "vscode.terminal-toggle",
        what: UiWhat::TerminalFocused,
        count: 3,
        window_ms: 10 * 60_000,
        clicks: Vec::new(),
        title: "Toggle the terminal from the keyboard",
        text: "You clicked into the terminal 3 times. `` Ctrl-` `` jumps to the terminal and back.",
        shape: "clicks into the terminal",
        description: "Clicking into VS Code's terminal instead of Ctrl-`",
    })
}

pub fn tab_switch() -> Box<dyn Rule> {
    Box::new(ClickHabit {
        id: "vscode.tab-switch",
        what: UiWhat::TabSwitched,
        count: 5,
        window_ms: 10 * 60_000,
        clicks: Vec::new(),
        title: "Switch tabs from the keyboard",
        text: "You clicked between tabs 5 times. `gt` and `gT` go to the next and previous tab. `Ctrl-P` opens one by name.",
        shape: "tabs switched by clicking",
        description: "Clicking between editor tabs instead of gt or Ctrl-P",
    })
}

pub fn all() -> Vec<Box<dyn Rule>> {
    vec![quick_open(), terminal_toggle(), tab_switch()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(rule: &mut dyn Rule, actions: &[(UiWhat, How, u64)]) -> Vec<RuleEvent> {
        let mut out = Vec::new();
        for &(what, how, at_ms) in actions {
            rule.on_ui(&UiAction { what, how, at_ms }, &mut out);
        }
        out
    }

    #[test]
    fn three_clicks_make_a_habit() {
        use How::*;
        use UiWhat::FileOpened as F;
        let ev = run(
            &mut *quick_open(),
            &[(F, Click, 0), (F, Click, 60_000), (F, Click, 120_000)],
        );
        assert_eq!(ev.len(), 1);
        // Spread out, or opened another way: not a habit.
        assert!(
            run(
                &mut *quick_open(),
                &[(F, Click, 0), (F, Click, 700_000), (F, Click, 1_400_000)]
            )
            .is_empty()
        );
        assert!(
            run(
                &mut *quick_open(),
                &[(F, Other, 0), (F, Other, 1), (F, Other, 2)]
            )
            .is_empty()
        );
        assert_eq!(
            run(&mut *quick_open(), &[(F, Shortcut, 5)]),
            [RuleEvent::Adoption {
                rule_id: "vscode.quick-open",
                at_ms: 5
            }]
        );
        // The terminal rule ignores files.
        assert!(
            run(
                &mut *terminal_toggle(),
                &[(F, Click, 0), (F, Click, 1), (F, Click, 2)]
            )
            .is_empty()
        );
    }

    #[test]
    fn every_command_is_explained() {
        let mut out = Vec::new();
        for mut r in all() {
            for what in [UiWhat::FileOpened, UiWhat::TerminalFocused] {
                for t in 0..3 {
                    r.on_ui(
                        &UiAction {
                            what,
                            how: How::Click,
                            at_ms: t,
                        },
                        &mut out,
                    );
                }
            }
        }
        assert_eq!(out.len(), 2);
        for ev in &out {
            if let RuleEvent::Finding(f) = ev {
                for span in crate::glossary::code_spans(&f.text) {
                    assert!(
                        !crate::glossary::explain_keys(span, false).is_empty(),
                        "`{span}`"
                    );
                }
            }
        }
    }
}
