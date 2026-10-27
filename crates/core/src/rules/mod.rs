//! Rule packs.

pub mod bash;
pub mod edit;
pub mod repeat;
pub mod vim;

use crate::rule::Rule;

/// Every built-in rule.
pub fn all() -> Vec<Box<dyn Rule>> {
    let mut rules = vim::all();
    rules.extend(bash::all());
    rules.extend(edit::all());
    rules.extend(crate::motion::all());
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
    fn every_marked_command_is_explained() {
        use crate::vim::Flavor;
        let sessions = [
            (
                Flavor::Vim,
                VimMode::Normal,
                "jjjjjjj<Esc>kkkkkkkkkkkk<Esc>llllll<Esc>xxxxx<Esc>",
            ),
            (
                Flavor::Vim,
                VimMode::Normal,
                "<Down><Down><Down><Esc>ddddddd<Esc>i<BS><BS><BS><BS><BS><BS><Esc>",
            ),
            (
                Flavor::Vim,
                VimMode::Normal,
                "i<Left><Left><Left><Left><Esc>:w<CR>:q<CR>",
            ),
            (
                Flavor::Vim,
                VimMode::Normal,
                "xihi<Esc>dwihi<Esc>Dahi<Esc>ddOhi<Esc>d$c$hi<Esc>$ahi<Esc>kohi<Esc>",
            ),
            (
                Flavor::Vim,
                VimMode::Normal,
                "viwdVjjdveey>>>>>>JJJpppciwfoo<Esc>wciwbar<Esc>",
            ),
            (
                Flavor::Vim,
                VimMode::Normal,
                "0cwnew<Esc>j0cwnew<Esc>j0cwnew<Esc>j",
            ),
            (
                Flavor::Readline,
                VimMode::Insert,
                "<Up><Up><Up><Up><Up><BS><BS><BS><BS><BS><BS><BS><BS><BS><BS><BS><Left><Left><Left><Left><Left><Left><Left><Left><Left>",
            ),
        ];
        let mut seen = 0;
        for (flavor, mode, keys) in sessions {
            for rule in super::all() {
                for f in findings(&run(rule, flavor, mode, keys)) {
                    for span in crate::glossary::code_spans(&f.text) {
                        seen += 1;
                        let insert = f.mode == VimMode::Insert;
                        assert!(
                            !crate::glossary::explain_keys(span, insert).is_empty(),
                            "{}: `{span}` has no explanation in: {}",
                            f.rule_id,
                            f.text
                        );
                    }
                }
            }
        }
        assert!(seen > 30, "only {seen} commands checked");
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
