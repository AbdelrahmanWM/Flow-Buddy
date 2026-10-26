//! The rule interface: what an inefficiency looks like and what doing it
//! better looks like.

use std::fmt;

use crate::context::{ContextKind, VimMode};
use crate::motion::{MotionEvent, OperatorEvent};
use crate::vim::Step;

/// An abstract description of a mistake, e.g. `j×7` or `:w then :q`.
///
/// Built only from `'static` strings and numbers, so typed text can never
/// end up in a shape — and shapes are what the mistake memory stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shape {
    token: &'static str,
    count: Option<u32>,
}

impl Shape {
    /// `token` repeated `n` times, shown as `j×7`.
    pub const fn repeat(token: &'static str, n: u32) -> Shape {
        Shape {
            token,
            count: Some(n),
        }
    }

    pub const fn fixed(text: &'static str) -> Shape {
        Shape {
            token: text,
            count: None,
        }
    }

    pub fn count(&self) -> Option<u32> {
        self.count
    }
}

impl fmt::Display for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.count {
            Some(n) => write!(f, "{}×{n}", self.token),
            None => f.write_str(self.token),
        }
    }
}

/// An inefficiency a rule spotted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub rule_id: &'static str,
    /// Short headline for the nudge, e.g. "Try a count".
    pub title: &'static str,
    /// The suggestion shown to the user. Built from static text and
    /// numbers only.
    pub text: String,
    pub shape: Shape,
    pub mode: VimMode,
    pub at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleEvent {
    Finding(Finding),
    /// The better way was used: this is how improvement is measured.
    Adoption {
        rule_id: &'static str,
        at_ms: u64,
    },
}

pub trait Rule: Send {
    /// Stable id such as `vim.repeat-jk`, used in config and storage.
    fn id(&self) -> &'static str;
    fn description(&self) -> &'static str;
    /// The better way in a few words, e.g. "Try a count".
    fn title(&self) -> &'static str {
        ""
    }
    fn contexts(&self) -> &'static [ContextKind];

    fn threshold(&self) -> Option<u32> {
        None
    }
    fn set_threshold(&mut self, _n: u32) {}

    /// Look at one confident step.
    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>);

    /// Time passed without keys; finish anything that is complete.
    fn tick(&mut self, _now_ms: u64, _out: &mut Vec<RuleEvent>) {}

    /// A movement an editor plugin reported, with the keys pressed for it.
    fn on_motion(&mut self, _ev: &MotionEvent, _out: &mut Vec<RuleEvent>) {}

    /// Something in VS Code's UI, and how the user got there.
    fn on_ui(&mut self, _ev: &crate::rules::vscode::UiAction, _out: &mut Vec<RuleEvent>) {}

    /// A command the shell hook reported, by its shape.
    fn on_shell(&mut self, _ev: &crate::rules::shell::ShellEvent, _out: &mut Vec<RuleEvent>) {}

    /// An operator an editor plugin reported, with the text objects its
    /// range matched and the keys pressed for it.
    fn on_operator(&mut self, _ev: &OperatorEvent, _out: &mut Vec<RuleEvent>) {}

    /// Only works with an editor plugin's motion facts.
    fn needs_editor(&self) -> bool {
        false
    }

    /// A key-only guess that a motion rule does better when an editor
    /// reports motions; it steps aside then so one habit gets one nudge.
    fn replaced_by_editor(&self) -> bool {
        false
    }

    /// Forget in-progress state (context changed, mode uncertain).
    fn reset(&mut self);
}
