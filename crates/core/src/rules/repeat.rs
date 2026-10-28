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
    first_ms: u64,
    last_ms: u64,
    mode: VimMode,
    /// Of `n`, how many came from holding the key (auto-repeat).
    repeats: u32,
    /// Already nudged while it was going on (#114).
    reported: bool,
}

/// A streak this many times its threshold is nudged while it goes on.
pub const ONGOING_FACTOR: u32 = 3;

pub struct RepeatRule {
    spec: RepeatSpec,
    streak: Option<Streak>,
    /// The better-way step just seen and when. Repeating it (holding `j`)
    /// counts as one adoption, not hundreds.
    adopting: Option<(Same, u64)>,
    replaced_by_editor: bool,
    /// Tip text for a held key: unit and seconds held.
    held: Option<fn(&'static str, f64) -> String>,
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
            held: None,
        }
    }

    /// What to say when the streak came mostly from holding the key down.
    pub fn when_held(mut self, message: fn(&'static str, f64) -> String) -> Self {
        self.held = Some(message);
        self
    }

    fn finding(&self, s: &Streak) -> RuleEvent {
        let secs = s.last_ms.saturating_sub(s.first_ms) as f64 / 1000.0;
        let text = match self.held {
            Some(held) if s.repeats * 2 > s.n && s.repeats >= 5 => held(s.unit, secs),
            _ => (self.spec.message)(s.unit, s.n),
        };
        RuleEvent::Finding(Finding {
            rule_id: self.spec.id,
            title: self.spec.title,
            text,
            shape: Shape::repeat(s.unit, s.n),
            mode: s.mode,
            at_ms: s.last_ms,
            start_ms: s.first_ms,
        })
    }

    /// Step aside when an editor reports motions (see [`Rule::replaced_by_editor`]).
    pub fn replaced_by_editor(mut self) -> Self {
        self.replaced_by_editor = true;
        self
    }

    fn end(&mut self, out: &mut Vec<RuleEvent>) {
        let Some(s) = self.streak.take() else { return };
        if s.n >= self.spec.threshold && !s.reported {
            out.push(self.finding(&s));
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

    fn title(&self) -> &'static str {
        self.spec.title
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
                s.repeats += u32::from(step.repeat);
            }
            _ => {
                self.end(out);
                self.streak = Some(Streak {
                    unit,
                    n: 1,
                    first_ms: step.at_ms,
                    last_ms: step.at_ms,
                    mode: step.before,
                    repeats: u32::from(step.repeat),
                    reported: false,
                });
            }
        }
        // Far past the threshold: say so now rather than after it ends.
        if let Some(s) = &self.streak
            && !s.reported
            && s.n >= self.spec.threshold * ONGOING_FACTOR
        {
            let finding = self.finding(s);
            out.push(finding);
            if let Some(s) = &mut self.streak {
                s.reported = true;
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

    use crate::key::KeyStroke;
    use crate::rule::{Rule, RuleEvent};
    use crate::vim::Tracker;

    fn findings(ev: &[RuleEvent]) -> Vec<String> {
        ev.iter()
            .filter_map(|e| match e {
                RuleEvent::Finding(f) => Some(f.text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn long_streaks_are_nudged_while_they_go_on() {
        // repeat-x nudges at 4; at 12 it says so straight away, once.
        let keys = "x".repeat(20);
        let ev = run(
            Box::new(vim::repeat_x()),
            Flavor::Vim,
            VimMode::Normal,
            &keys,
        );
        let f = findings(&ev);
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(f[0].contains("12 times"), "{}", f[0]);
    }

    #[test]
    fn holding_a_key_is_one_gesture() {
        // j held down: one press, then auto-repeat every 33 ms for 2 s.
        let mut t = Tracker::new(Flavor::Vim);
        t.reset_known(VimMode::Normal);
        let mut rule = vim::repeat_jk();
        let mut out = Vec::new();
        let j = crate::key::Key::ch('j');
        for i in 0..60 {
            let step = t.feed(KeyStroke {
                key: j,
                repeat: i > 0,
                at_ms: 1000 + i * 33,
            });
            rule.on_step(&step, &mut out);
        }
        let f = findings(&out);
        assert!(f[0].starts_with("You held j for"), "{f:?}");
        assert!(f[0].contains("`Ctrl-d`"), "{f:?}");
    }

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
