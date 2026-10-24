//! Rule packs.

pub mod bash;
pub mod repeat;
pub mod vim;

use crate::rule::Rule;

/// Every built-in rule.
pub fn all() -> Vec<Box<dyn Rule>> {
    let mut rules = vim::all();
    rules.extend(bash::all());
    rules
}

#[cfg(test)]
pub(crate) mod testing {
    use crate::context::VimMode;
    use crate::rule::{Finding, Rule, RuleEvent};
    use crate::vim::notation::strokes;
    use crate::vim::{Flavor, Tracker};

    /// Run one rule over keys typed 100 ms apart, starting in `mode`.
    pub fn run(
        mut rule: Box<dyn Rule>,
        flavor: Flavor,
        mode: VimMode,
        keys: &str,
    ) -> Vec<RuleEvent> {
        let mut tracker = Tracker::new(flavor);
        tracker.reset_known(mode);
        let mut out = Vec::new();
        let mut last = 0;
        for stroke in strokes(keys, 0) {
            let step = tracker.feed(stroke);
            if step.confident {
                rule.on_step(&step, &mut out);
            }
            last = stroke.at_ms;
        }
        rule.tick(last + 10_000, &mut out);
        out
    }

    pub fn findings(events: &[RuleEvent]) -> Vec<&Finding> {
        events
            .iter()
            .filter_map(|e| match e {
                RuleEvent::Finding(f) => Some(f),
                _ => None,
            })
            .collect()
    }

    pub fn adoptions(events: &[RuleEvent]) -> usize {
        events
            .iter()
            .filter(|e| matches!(e, RuleEvent::Adoption { .. }))
            .count()
    }

    #[test]
    fn rule_ids_are_unique() {
        let mut ids: Vec<_> = super::all().iter().map(|r| r.id()).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }
}
