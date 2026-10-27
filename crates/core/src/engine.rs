//! Dispatch steps to the rules that apply to the current context.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::context::ContextKind;
use crate::curriculum::Known;
use crate::motion::{MotionEvent, OperatorEvent};
use crate::rule::{Finding, Rule, RuleEvent};
use crate::vim::Step;

/// Per-rule settings from the config file and runtime overrides.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuleSettings {
    pub disabled: HashSet<String>,
    /// Set in the config; wins over everything.
    pub thresholds: BTreeMap<String, u32>,
    /// Added to the default threshold after "not helpful" reports.
    pub raised: BTreeMap<String, u32>,
}

pub struct Engine {
    rules: Vec<Box<dyn Rule>>,
    disabled: HashSet<String>,
    editor_motions: bool,
    /// Each rule's built-in threshold.
    defaults: HashMap<&'static str, u32>,
    known: Known,
    /// Each rule's place in [`crate::rules::PRECEDENCE`], then in the list.
    rank: HashMap<&'static str, usize>,
}

/// Findings this close together in one batch are about the same keys.
const SAME_GESTURE_MS: u64 = 2000;
/// A runner-up joins the winning tip only if both fit in this.
const MAX_JOINED: usize = 280;

impl Engine {
    pub fn new(rules: Vec<Box<dyn Rule>>) -> Self {
        let defaults = rules
            .iter()
            .filter_map(|r| Some((r.id(), r.threshold()?)))
            .collect();
        let precedence = crate::rules::PRECEDENCE;
        let rank = rules
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let p = precedence.iter().position(|id| *id == r.id());
                (r.id(), p.unwrap_or(precedence.len() + i))
            })
            .collect();
        Engine {
            rules,
            disabled: HashSet::new(),
            editor_motions: false,
            defaults,
            known: Known::everything(),
            rank,
        }
    }

    pub fn known(&self) -> &Known {
        &self.known
    }

    /// What you already use: advice you're not ready for is held back.
    pub fn set_known(&mut self, known: Known) {
        for rule in &mut self.rules {
            rule.set_known(&known);
        }
        self.known = known;
    }

    /// One tip per gesture, and only advice you're ready for: drop
    /// findings whose main skill builds on something you don't use yet;
    /// of findings about the same keys, keep the best-ranked and add the
    /// runner-up to it as another way, if both are short enough.
    fn settle(&self, out: &mut Vec<RuleEvent>) {
        let rank = |f: &Finding| self.rank.get(f.rule_id).copied().unwrap_or(usize::MAX);
        out.retain(|e| match e {
            RuleEvent::Finding(f) => self.known.ready_for_rule(f.rule_id),
            _ => true,
        });
        if out
            .iter()
            .filter(|e| matches!(e, RuleEvent::Finding(_)))
            .count()
            < 2
        {
            return;
        }
        let events = std::mem::take(out);
        let mut group: Vec<Finding> = Vec::new();
        let flush = |group: &mut Vec<Finding>, out: &mut Vec<RuleEvent>| {
            group.sort_by_key(rank);
            let mut it = group.drain(..);
            if let Some(mut best) = it.next() {
                if let Some(next) = it.next()
                    && best.text.len() + next.text.len() < MAX_JOINED
                {
                    best.text = format!("{} Also: {}", best.text, next.text);
                }
                out.push(RuleEvent::Finding(best));
            }
        };
        for e in events {
            match e {
                RuleEvent::Finding(f) => {
                    if group
                        .last()
                        .is_some_and(|g| f.at_ms.abs_diff(g.at_ms) > SAME_GESTURE_MS)
                    {
                        flush(&mut group, out);
                    }
                    group.push(f);
                }
                other => out.push(other),
            }
        }
        flush(&mut group, out);
    }

    /// All built-in rules.
    pub fn standard() -> Self {
        Engine::new(crate::rules::all())
    }

    pub fn rules(&self) -> impl Iterator<Item = &dyn Rule> {
        self.rules.iter().map(|r| r.as_ref())
    }

    pub fn has_rule(&self, id: &str) -> bool {
        self.rules.iter().any(|r| r.id() == id)
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        !self.disabled.contains(id)
    }

    pub fn apply(&mut self, settings: &RuleSettings) {
        self.disabled = settings.disabled.clone();
        for rule in &mut self.rules {
            let Some(&default) = self.defaults.get(rule.id()) else {
                continue;
            };
            let n = settings
                .thresholds
                .get(rule.id())
                .copied()
                .unwrap_or_else(|| default + settings.raised.get(rule.id()).copied().unwrap_or(0));
            rule.set_threshold(n);
        }
    }

    pub fn default_threshold(&self, id: &str) -> Option<u32> {
        self.defaults.get(id).copied()
    }

    /// Whether an editor plugin reports motions in the current context:
    /// motion rules run and the key-only rules they replace step aside.
    pub fn set_editor_motions(&mut self, on: bool) {
        if on != self.editor_motions {
            self.editor_motions = on;
            self.reset();
        }
    }

    fn active(&mut self, context: ContextKind) -> impl Iterator<Item = &mut Box<dyn Rule>> {
        let disabled = &self.disabled;
        let editor = self.editor_motions;
        self.rules.iter_mut().filter(move |r| {
            !disabled.contains(r.id())
                && r.contexts().contains(&context)
                && if editor {
                    !r.replaced_by_editor()
                } else {
                    !r.needs_editor()
                }
        })
    }

    /// Something in VS Code's UI.
    pub fn on_ui(&mut self, ev: &crate::rules::vscode::UiAction) -> Vec<RuleEvent> {
        let mut out = Vec::new();
        // These rules need the companion; it's reporting if this arrives.
        let editor = std::mem::replace(&mut self.editor_motions, true);
        for rule in self.active(ContextKind::VsCodeVim) {
            rule.on_ui(ev, &mut out);
        }
        self.editor_motions = editor;
        self.settle(&mut out);
        out
    }

    /// A command the shell hook reported.
    pub fn on_shell(&mut self, ev: &crate::rules::shell::ShellEvent) -> Vec<RuleEvent> {
        let mut out = Vec::new();
        for rule in self.active(ContextKind::Shell) {
            rule.on_shell(ev, &mut out);
        }
        self.settle(&mut out);
        out
    }

    /// An operator reported by an editor plugin.
    pub fn on_operator(&mut self, context: ContextKind, ev: &OperatorEvent) -> Vec<RuleEvent> {
        let mut out = Vec::new();
        for rule in self.active(context) {
            rule.on_operator(ev, &mut out);
        }
        self.settle(&mut out);
        out
    }

    /// A movement reported by an editor plugin.
    pub fn on_motion(&mut self, context: ContextKind, ev: &MotionEvent) -> Vec<RuleEvent> {
        let mut out = Vec::new();
        for rule in self.active(context) {
            rule.on_motion(ev, &mut out);
        }
        self.settle(&mut out);
        out
    }

    /// Feed one step. Unconfident steps reset in-progress streaks instead
    /// of feeding them: we'd rather miss a finding than invent one.
    pub fn on_step(&mut self, context: ContextKind, step: &Step) -> Vec<RuleEvent> {
        let mut out = Vec::new();
        if !step.confident {
            self.reset();
            return out;
        }
        for rule in self.active(context) {
            rule.on_step(step, &mut out);
        }
        self.settle(&mut out);
        out
    }

    /// Called periodically so streaks that ended without another key are
    /// reported.
    pub fn tick(&mut self, context: ContextKind, now_ms: u64) -> Vec<RuleEvent> {
        let mut out = Vec::new();
        for rule in self.active(context) {
            rule.tick(now_ms, &mut out);
        }
        self.settle(&mut out);
        out
    }

    /// The context changed: report finished streaks, then forget state.
    pub fn context_changed(&mut self, old: ContextKind, now_ms: u64) -> Vec<RuleEvent> {
        let out = self.tick(
            old,
            now_ms.saturating_add(crate::rules::repeat::MAX_GAP_MS + 1),
        );
        self.reset();
        out
    }

    pub fn reset(&mut self) {
        for rule in &mut self.rules {
            rule.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::VimMode;
    use crate::vim::notation::strokes;
    use crate::vim::{Flavor, Tracker};

    fn feed(engine: &mut Engine, ctx: ContextKind, keys: &str) -> Vec<RuleEvent> {
        let mut t = Tracker::new(Flavor::Vim);
        t.reset_known(VimMode::Normal);
        let mut out = Vec::new();
        for s in strokes(keys, 0) {
            let step = t.feed(s);
            out.extend(engine.on_step(ctx, &step));
        }
        out.extend(engine.tick(ctx, 1_000_000));
        out
    }

    fn found(events: &[RuleEvent]) -> Vec<&'static str> {
        events
            .iter()
            .filter_map(|e| match e {
                RuleEvent::Finding(f) => Some(f.rule_id),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn dispatches_by_context() {
        let mut e = Engine::standard();
        assert_eq!(
            found(&feed(&mut e, ContextKind::VsCodeVim, "jjjjjj")),
            ["vim.repeat-jk"]
        );
        assert!(found(&feed(&mut e, ContextKind::BashVi, "<Esc>jjjjjj")).is_empty());
    }

    #[test]
    fn disabled_rules_do_not_run() {
        let mut e = Engine::standard();
        e.apply(&RuleSettings {
            disabled: ["vim.repeat-jk".to_string()].into(),
            ..Default::default()
        });
        assert!(found(&feed(&mut e, ContextKind::VsCodeVim, "jjjjjj")).is_empty());
    }

    #[test]
    fn thresholds_apply() {
        let mut e = Engine::standard();
        e.apply(&RuleSettings {
            thresholds: [("vim.repeat-jk".to_string(), 10)].into(),
            ..Default::default()
        });
        assert!(found(&feed(&mut e, ContextKind::VsCodeVim, "jjjjjj")).is_empty());
    }

    #[test]
    fn unconfident_steps_reset_streaks() {
        let mut e = Engine::standard();
        let mut t = Tracker::new(Flavor::Vim);
        t.reset_known(VimMode::Normal);
        let mut out = Vec::new();
        for (i, s) in strokes("jjjjjjj", 0).into_iter().enumerate() {
            if i == 3 {
                t.invalidate();
            }
            let step = t.feed(s);
            out.extend(e.on_step(ContextKind::VsCodeVim, &step));
        }
        out.extend(e.tick(ContextKind::VsCodeVim, 1_000_000));
        assert!(found(&out).is_empty());
    }

    #[test]
    fn context_change_flushes_finished_streaks() {
        let mut e = Engine::standard();
        let mut t = Tracker::new(Flavor::Vim);
        t.reset_known(VimMode::Normal);
        for s in strokes("jjjjjj", 0) {
            let step = t.feed(s);
            e.on_step(ContextKind::TerminalVim, &step);
        }
        let out = e.context_changed(ContextKind::TerminalVim, 600);
        assert_eq!(found(&out), ["vim.repeat-jk"]);
    }

    #[test]
    fn one_tip_per_gesture() {
        // Seven x then i: both "delete by word" and "change instead" fit;
        // the change covers the whole gesture, so it leads.
        let mut e = Engine::standard();
        let out = feed(&mut e, ContextKind::TerminalVim, "xxxxxxxihi<Esc>");
        let tips: Vec<_> = out
            .iter()
            .filter_map(|e| match e {
                RuleEvent::Finding(f) => Some(f),
                _ => None,
            })
            .collect();
        assert_eq!(tips.len(), 1, "{tips:?}");
        assert_eq!(tips[0].rule_id, "vim.delete-then-insert");
        assert!(tips[0].text.contains("`7s`"), "{}", tips[0].text);
        assert!(
            tips[0].text.contains("Also: You pressed x 7 times"),
            "{}",
            tips[0].text
        );
    }

    #[test]
    fn advice_waits_for_what_it_builds_on() {
        // Macros build on `.`: not suggested to someone who doesn't use it.
        let keys = "0cwnew<Esc>j0cwnew<Esc>j0cwnew<Esc>j0cwnew<Esc>j";
        let rules = |known: Known| {
            let mut e = Engine::standard();
            e.set_known(known);
            let out = feed(&mut e, ContextKind::TerminalVim, keys);
            out.iter()
                .filter_map(|e| match e {
                    RuleEvent::Finding(f) => Some((f.rule_id, f.at_ms)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let all = rules(Known::everything());
        assert!(all.iter().any(|(r, _)| *r == "vim.macro"), "{all:?}");
        let beginner = rules(Known::of([]));
        assert!(
            beginner.iter().all(|(r, _)| *r != "vim.macro"),
            "{beginner:?}"
        );
        assert_eq!(rules(Known::of(["dot"])), all);
    }
}
