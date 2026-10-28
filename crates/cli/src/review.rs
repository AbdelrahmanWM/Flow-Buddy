//! `flowbuddy review`: your AI looks at how the tips went and proposes
//! tuning (thresholds, mutes), which you accept one by one.
//!
//! The model's answer is untrusted text: only proposals that name a real
//! rule and stay within bounds survive, and nothing changes until you
//! apply it.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use flowbuddy_core::engine::Engine;
use flowbuddy_store::{Outcome, TipRow};
use serde::{Deserialize, Serialize};

/// One suggested change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Proposal {
    Threshold {
        rule: String,
        value: u32,
        reason: String,
    },
    Mute {
        rule: String,
        reason: String,
    },
    Unmute {
        rule: String,
        reason: String,
    },
}

impl Proposal {
    pub fn rule(&self) -> &str {
        match self {
            Proposal::Threshold { rule, .. }
            | Proposal::Mute { rule, .. }
            | Proposal::Unmute { rule, .. } => rule,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Proposal::Threshold {
                rule,
                value,
                reason,
            } => {
                format!("set {rule}'s threshold to {value}: {reason}")
            }
            Proposal::Mute { rule, reason } => format!("mute {rule}: {reason}"),
            Proposal::Unmute { rule, reason } => format!("unmute {rule}: {reason}"),
        }
    }
}

/// How the tips of one rule went.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TipStats {
    pub shown: u32,
    pub adopted: u32,
    pub dismissed: u32,
    pub not_helpful: u32,
    pub ignored: u32,
}

pub fn tip_stats(rows: &[TipRow]) -> BTreeMap<String, TipStats> {
    let mut out: BTreeMap<String, TipStats> = BTreeMap::new();
    for r in rows.iter().filter(|r| !r.held) {
        let s = out.entry(r.rule_id.clone()).or_default();
        s.shown += 1;
        match r.outcome {
            Some(Outcome::Adopted) => s.adopted += 1,
            Some(Outcome::Dismissed) => s.dismissed += 1,
            Some(Outcome::NotHelpful) => s.not_helpful += 1,
            Some(Outcome::Ignored) | None => s.ignored += 1,
        }
    }
    out
}

/// The current setting of a rule, for the prompt.
pub struct RuleSetting {
    pub threshold: Option<u32>,
    pub default: Option<u32>,
    pub muted: bool,
}

/// The review prompt: the coaching prompt's facts, how each rule's tips
/// went, the current settings, and the answer format.
pub fn prompt(
    coach_facts: &str,
    stats: &BTreeMap<String, TipStats>,
    settings: &BTreeMap<String, RuleSetting>,
) -> String {
    let mut p = String::new();
    p.push_str(
        "You are tuning Flow Buddy, a local Vim coach, for one person. Below are their habits, \
         how its tips went over the last 30 days, and each rule's settings. Propose at most 5 \
         changes that would make the tips more useful to them:\n\
         - raise a threshold where tips are mostly dismissed, marked not helpful or ignored \
         (the rule fires too easily); lower it where tips are adopted and the habit is common;\n\
         - mute a rule whose tips keep being marked not helpful;\n\
         - unmute a muted rule only if its habit is clearly frequent again.\n\
         Make no change without evidence in the numbers. Fewer, better proposals beat many.\n\n",
    );
    p.push_str("Habits:\n");
    p.push_str(coach_facts);
    p.push_str("\nTips over the last 30 days, per rule (shown: adopted / dismissed / not helpful / ignored):\n");
    for (rule, s) in stats {
        let _ = writeln!(
            p,
            "- {rule}: {}: {} / {} / {} / {}",
            s.shown, s.adopted, s.dismissed, s.not_helpful, s.ignored
        );
    }
    p.push_str("\nSettings, per rule:\n");
    for (rule, s) in settings {
        let threshold = match (s.threshold, s.default) {
            (Some(t), Some(d)) => format!("threshold {t} (default {d})"),
            _ => "no threshold".into(),
        };
        let muted = if s.muted { ", muted" } else { "" };
        let _ = writeln!(p, "- {rule}: {threshold}{muted}");
    }
    p.push_str(
        "\nAnswer with only a JSON object, no other text:\n\
         {\"proposals\": [\n\
         \x20 {\"kind\": \"threshold\", \"rule\": \"<rule id>\", \"value\": <number>, \"reason\": \"<one sentence>\"},\n\
         \x20 {\"kind\": \"mute\", \"rule\": \"<rule id>\", \"reason\": \"<one sentence>\"},\n\
         \x20 {\"kind\": \"unmute\", \"rule\": \"<rule id>\", \"reason\": \"<one sentence>\"}\n\
         ]}\n\
         Use only rule ids from the lists above. An empty list is a fine answer.\n",
    );
    p
}

/// What came of the model's answer: the proposals that hold, and why the
/// others were dropped.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    pub proposals: Vec<Proposal>,
    pub dropped: Vec<String>,
}

/// Read proposals out of a model's answer and keep the valid ones.
pub fn parse(answer: &str) -> Result<Parsed, String> {
    // Models sometimes wrap JSON in prose or a code fence: take the
    // outermost object.
    let start = answer.find('{').ok_or("no JSON object in the answer")?;
    let end = answer.rfind('}').ok_or("no JSON object in the answer")?;
    #[derive(Deserialize)]
    struct Answer {
        proposals: Vec<serde_json::Value>,
    }
    let a: Answer = serde_json::from_str(&answer[start..=end])
        .map_err(|e| format!("not the JSON asked for: {e}"))?;
    let engine = Engine::standard();
    let mut out = Parsed::default();
    for raw in a.proposals {
        let p: Proposal = match serde_json::from_value(raw.clone()) {
            Ok(p) => p,
            Err(_) => {
                out.dropped.push(format!("unreadable proposal {raw}"));
                continue;
            }
        };
        if !engine.has_rule(p.rule()) {
            out.dropped.push(format!("unknown rule {}", p.rule()));
            continue;
        }
        if let Proposal::Threshold { rule, value, .. } = &p {
            let Some(default) = engine.default_threshold(rule) else {
                out.dropped.push(format!("{rule} has no threshold"));
                continue;
            };
            let (low, high) = (default.div_ceil(4).max(1), default * 4);
            if !(low..=high).contains(value) {
                out.dropped
                    .push(format!("{rule}: {value} is outside {low}..={high}"));
                continue;
            }
        }
        out.proposals.push(p);
    }
    out.proposals.truncate(5);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_valid_proposals_only() {
        let answer = r#"Here you go:
```json
{"proposals": [
  {"kind": "threshold", "rule": "vim.repeat-jk", "value": 8, "reason": "Mostly dismissed."},
  {"kind": "mute", "rule": "vim.shorthand", "reason": "Marked not helpful 6 times."},
  {"kind": "threshold", "rule": "vim.repeat-jk", "value": 500, "reason": "Too far."},
  {"kind": "mute", "rule": "vim.no-such-rule", "reason": "Invented."},
  {"kind": "delete-everything", "rule": "vim.repeat-x"}
]}
```"#;
        let p = parse(answer).unwrap();
        assert_eq!(
            p.proposals,
            [
                Proposal::Threshold {
                    rule: "vim.repeat-jk".into(),
                    value: 8,
                    reason: "Mostly dismissed.".into()
                },
                Proposal::Mute {
                    rule: "vim.shorthand".into(),
                    reason: "Marked not helpful 6 times.".into()
                },
            ]
        );
        assert_eq!(p.dropped.len(), 3, "{:?}", p.dropped);
        assert!(p.dropped[0].contains("outside"));
    }

    #[test]
    fn rejects_answers_that_are_not_json() {
        assert!(parse("I think you should relax.").is_err());
        assert!(parse("{\"ideas\": []}").is_err());
        assert_eq!(parse("{\"proposals\": []}").unwrap(), Parsed::default());
    }

    #[test]
    fn stats_and_prompt() {
        let row = |rule: &str, outcome| TipRow {
            shown_at: 0,
            rule_id: rule.into(),
            context: "vscode-vim".into(),
            stage: "nudged".into(),
            hour: 10,
            mode: "active".into(),
            prior_today: 0,
            outcome,
            held: false,
        };
        let stats = tip_stats(&[
            row("vim.repeat-x", Some(Outcome::Adopted)),
            row("vim.repeat-x", Some(Outcome::Dismissed)),
            row("vim.repeat-x", None),
        ]);
        let settings = BTreeMap::from([(
            "vim.repeat-x".to_string(),
            RuleSetting {
                threshold: Some(4),
                default: Some(4),
                muted: false,
            },
        )]);
        let p = prompt("- x habit\n", &stats, &settings);
        assert!(p.contains("- vim.repeat-x: 3: 1 / 1 / 0 / 1"), "{p}");
        assert!(p.contains("- vim.repeat-x: threshold 4 (default 4)"), "{p}");
        assert!(p.contains("\"kind\": \"threshold\""));
    }
}
