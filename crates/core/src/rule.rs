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
    /// When the keys it's about began; `at_ms` is when they ended. Tips
    /// whose spans overlap are about the same keys.
    pub start_ms: u64,
}

/// Keys this close before or after a tip's span still belong to the same
/// gesture (a pause to think between `xxxx` and `i`).
pub const GESTURE_SLACK_MS: u64 = 1500;
/// A runner-up joins the winning tip only if both fit in this.
const MAX_JOINED: usize = 280;

impl Finding {
    /// Whether this and `other` are about the same keys: their spans
    /// overlap, give or take [`GESTURE_SLACK_MS`].
    pub fn overlaps(&self, other: &Finding) -> bool {
        let (a0, a1) = (self.start_ms.min(self.at_ms), self.at_ms);
        let (b0, b1) = (other.start_ms.min(other.at_ms), other.at_ms);
        a0 <= b1 + GESTURE_SLACK_MS && b0 <= a1 + GESTURE_SLACK_MS
    }

    /// Take `other` in as another way to do it, if both still read well
    /// together, and cover both spans.
    pub fn absorb(&mut self, other: &Finding) {
        if self.text.len() + other.text.len() < MAX_JOINED && !self.text.contains(" Also: ") {
            self.text = format!("{} Also: {}", self.text, other.text);
        }
        self.start_ms = self.start_ms.min(other.start_ms);
        self.at_ms = self.at_ms.max(other.at_ms);
    }
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

    /// What you already use, so advice stays one step ahead of it.
    fn set_known(&mut self, _known: &crate::curriculum::Known) {}

    /// Forget in-progress state (context changed, mode uncertain).
    fn reset(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(rule_id: &'static str, start_ms: u64, at_ms: u64) -> Finding {
        Finding {
            rule_id,
            title: "t",
            text: format!("{rule_id} tip."),
            shape: Shape::fixed("x"),
            mode: VimMode::Normal,
            at_ms,
            start_ms,
        }
    }

    #[test]
    fn spans_tell_gestures_apart() {
        // A long j-streak and a tip at its end: the same keys.
        assert!(f("a", 0, 9_000).overlaps(&f("b", 8_500, 8_600)));
        // Ten seconds later, other keys.
        assert!(!f("a", 0, 1_000).overlaps(&f("b", 11_000, 11_500)));
        let mut best = f("a", 1_000, 2_000);
        best.absorb(&f("b", 500, 1_800));
        assert_eq!(best.text, "a tip. Also: b tip.");
        assert_eq!((best.start_ms, best.at_ms), (500, 2_000));
        best.absorb(&f("c", 600, 700));
        assert!(!best.text.contains("c tip"), "one alternative is enough");
    }
}
