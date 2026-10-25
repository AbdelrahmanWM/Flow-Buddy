//! A rule for "the same small thing, many times in a row".
//!
//! Most MVP rules are this shape: `jjjjj`, `xxxx`, Backspace held down.
//! Each is a [`RepeatRule`] configured with plain functions.

use crate::context::{ContextKind, VimMode};
use crate::key::Key;
use crate::rule::{Finding, Rule, RuleEvent, Shape};
use crate::vim::{InsertKey, Step, VimEvent};

/// Presses further apart than this don't form one streak.
pub const MAX_GAP_MS: u64 = 1500;

pub struct RepeatSpec {
    pub id: &'static str,
    pub description: &'static str,
    pub title: &'static str,
    pub contexts: &'static [ContextKind],
    pub threshold: u32,
    /// The streak unit this step is, e.g. `Some("j")`. Steps with no event
    /// (a half-typed command) don't break streaks; other events do.
    pub unit: fn(&Step) -> Option<&'static str>,
    /// Whether this step is the better way.
    pub adopted: fn(&Step) -> bool,
    /// Suggestion text for a streak of `unit` × `n`.
    pub message: fn(&'static str, u32) -> String,
}

struct Streak {
    unit: &'static str,
    n: u32,
    last_ms: u64,
    mode: VimMode,
}

pub struct RepeatRule {
    spec: RepeatSpec,
    streak: Option<Streak>,
    /// The better-way step just seen and when. Repeating it (holding `j`)
    /// counts as one adoption, not hundreds.
    adopting: Option<(Same, u64)>,
    replaced_by_editor: bool,
}

/// What makes two better-way steps "the same" for counting adoptions.
#[derive(Clone, PartialEq)]
enum Same {
    Command(Vec<Key>),
    Insert(std::mem::Discriminant<InsertKey>),
}

impl Same {
    fn of(step: &Step) -> Option<Same> {
        match &step.event {
            Some(VimEvent::Command(c)) => Some(Same::Command(c.keys.clone())),
            Some(VimEvent::Insert(k)) => Some(Same::Insert(std::mem::discriminant(k))),
            _ => None,
        }
    }
}

impl RepeatRule {
    pub fn new(spec: RepeatSpec) -> Self {
        RepeatRule {
            spec,
            streak: None,
            adopting: None,
            replaced_by_editor: false,
        }
    }

    /// Step aside when an editor reports motions (see [`Rule::replaced_by_editor`]).
    pub fn replaced_by_editor(mut self) -> Self {
        self.replaced_by_editor = true;
        self
    }

    fn end(&mut self, out: &mut Vec<RuleEvent>) {
        let Some(s) = self.streak.take() else { return };
        if s.n >= self.spec.threshold {
            out.push(RuleEvent::Finding(Finding {
                rule_id: self.spec.id,
                title: self.spec.title,
                text: (self.spec.message)(s.unit, s.n),
                shape: Shape::repeat(s.unit, s.n),
                mode: s.mode,
                at_ms: s.last_ms,
            }));
        }
    }
}

impl Rule for RepeatRule {
    fn id(&self) -> &'static str {
        self.spec.id
    }

    fn description(&self) -> &'static str {
        self.spec.description
    }

    fn contexts(&self) -> &'static [ContextKind] {
        self.spec.contexts
    }

    fn threshold(&self) -> Option<u32> {
        Some(self.spec.threshold)
    }

    fn set_threshold(&mut self, n: u32) {
        self.spec.threshold = n.max(2);
    }

    fn replaced_by_editor(&self) -> bool {
        self.replaced_by_editor
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        if (self.spec.adopted)(step) {
            let same = Same::of(step);
            let repeated = self.adopting.as_ref().is_some_and(|(prev, t)| {
                same.as_ref() == Some(prev) && step.at_ms.saturating_sub(*t) <= MAX_GAP_MS
            });
            if !repeated {
                out.push(RuleEvent::Adoption {
                    rule_id: self.spec.id,
                    at_ms: step.at_ms,
                });
            }
            self.adopting = same.map(|s| (s, step.at_ms));
        } else if step.event.is_some() {
            self.adopting = None;
        }
        let Some(unit) = (self.spec.unit)(step) else {
            if step.event.is_some() {
                self.end(out);
            }
            return;
        };
        match &mut self.streak {
            Some(s) if s.unit == unit && step.at_ms.saturating_sub(s.last_ms) <= MAX_GAP_MS => {
                s.n += 1;
                s.last_ms = step.at_ms;
            }
            _ => {
                self.end(out);
                self.streak = Some(Streak {
                    unit,
                    n: 1,
                    last_ms: step.at_ms,
                    mode: step.before,
                });
            }
        }
    }

    fn tick(&mut self, now_ms: u64, out: &mut Vec<RuleEvent>) {
        if self
            .streak
            .as_ref()
            .is_some_and(|s| now_ms.saturating_sub(s.last_ms) > MAX_GAP_MS)
        {
            self.end(out);
        }
    }

    fn reset(&mut self) {
        self.streak = None;
        self.adopting = None;
    }
}

#[cfg(test)]
mod tests {
    use crate::context::VimMode;
    use crate::rules::testing::{adoptions, run};
    use crate::rules::{bash, vim};
    use crate::vim::Flavor;

    #[test]
    fn holding_a_better_key_is_one_adoption() {
        let held = "j".repeat(20);
        let ev = run(
            Box::new(vim::arrows_normal()),
            Flavor::Vim,
            VimMode::Normal,
            &held,
        );
        assert_eq!(adoptions(&ev), 1);
    }

    #[test]
    fn different_better_steps_each_count() {
        let ev = run(
            Box::new(vim::arrows_normal()),
            Flavor::Vim,
            VimMode::Normal,
            "jjjwkkk",
        );
        // j-run, k-run; w is not an adoption for arrows but breaks the run.
        assert_eq!(adoptions(&ev), 2);
    }

    #[test]
    fn insert_adoptions_are_counted_per_run() {
        let ev = run(
            Box::new(vim::backspace_insert()),
            Flavor::Vim,
            VimMode::Normal,
            "i<C-w><C-w><C-w><Esc>",
        );
        assert_eq!(adoptions(&ev), 1);
    }

    #[test]
    fn bash_rules_count_runs_too() {
        let ev = run(
            Box::new(bash::backspace_clear()),
            Flavor::Readline,
            VimMode::Insert,
            "<C-u><C-u><C-u>",
        );
        assert_eq!(adoptions(&ev), 1);
    }
}
