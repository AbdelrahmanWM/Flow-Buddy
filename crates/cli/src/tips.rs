//! LLM-written tips (`[ai] tips`): once a day, your AI writes one sentence
//! for each habit you've had lately, from your numbers. The daemon adds it
//! to that habit's tips; it never runs the model itself.
//!
//! Every sentence is checked before it's kept: short, plain, for a real
//! rule, and every command in it known to the glossary.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use flowbuddy_core::engine::Engine;
use flowbuddy_core::glossary;
use serde::{Deserialize, Serialize};

/// What the daemon reads: `tips.json` next to `state.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TipCache {
    /// Unix seconds.
    pub generated_at: u64,
    pub tips: BTreeMap<String, String>,
}

pub const MAX_LEN: usize = 220;

pub fn prompt(facts: &str, rules: &[(String, String)]) -> String {
    let mut p = String::new();
    p.push_str(
        "You write short coaching tips for Flow Buddy, a local Vim coach, for one person. Below \
         are their habits. For each rule listed at the end, write one sentence (at most 200 \
         characters) that speaks to their numbers and names the better command. Put every \
         command in backticks, like `Ctrl-d` or `ci(`, and use only real Vim commands. Plain \
         text: no markdown, no lists, no quotes around the sentence.\n\nHabits:\n",
    );
    p.push_str(facts);
    p.push_str("\nRules to write for:\n");
    for (id, description) in rules {
        let _ = writeln!(p, "- {id}: {description}");
    }
    p.push_str("\nAnswer with only a JSON object, no other text:\n{\"tips\": {\"<rule id>\": \"<sentence>\"}}\n");
    p
}

/// Keep the sentences that pass the checks; say why the others didn't.
pub fn parse(answer: &str, now_s: u64) -> Result<(TipCache, Vec<String>), String> {
    let start = answer.find('{').ok_or("no JSON object in the answer")?;
    let end = answer.rfind('}').ok_or("no JSON object in the answer")?;
    #[derive(Deserialize)]
    struct Answer {
        tips: BTreeMap<String, String>,
    }
    let a: Answer = serde_json::from_str(&answer[start..=end])
        .map_err(|e| format!("not the JSON asked for: {e}"))?;
    let engine = Engine::standard();
    let mut cache = TipCache {
        generated_at: now_s,
        tips: BTreeMap::new(),
    };
    let mut dropped = Vec::new();
    for (rule, text) in a.tips {
        let text = text.trim().to_string();
        let spans = glossary::code_spans(&text);
        let why = if !engine.has_rule(&rule) {
            Some("unknown rule")
        } else if text.is_empty() || text.chars().count() > MAX_LEN {
            Some("empty or too long")
        } else if text.contains('\n') || text.contains("**") || text.starts_with('-') {
            Some("not plain text")
        } else if spans.is_empty() {
            Some("names no command")
        } else if spans
            .iter()
            .any(|s| glossary::explain_keys(s, false).is_empty())
        {
            Some("names a command Flow Buddy doesn't know")
        } else {
            None
        };
        match why {
            Some(why) => dropped.push(format!("{rule}: {why}")),
            None => {
                cache.tips.insert(rule, text);
            }
        }
    }
    Ok((cache, dropped))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_good_sentences_only() {
        let answer = r#"{"tips": {
            "vim.repeat-jk": "You stepped down 40 lines with j this week: `Ctrl-d` covers half a page per press.",
            "vim.repeat-x": "Try `zq`, the magic delete.",
            "vim.repeat-hl": "Use word motions.",
            "vim.nope": "`w` is great.",
            "vim.backspace-streak": "**Bold** advice with `Ctrl-w`."
        }}"#;
        let (cache, dropped) = parse(answer, 7).unwrap();
        assert_eq!(cache.generated_at, 7);
        assert_eq!(cache.tips.keys().collect::<Vec<_>>(), ["vim.repeat-jk"]);
        assert_eq!(dropped.len(), 4, "{dropped:?}");
        assert!(dropped.iter().any(|d| d.contains("doesn't know")));
    }

    #[test]
    fn prompt_lists_the_rules() {
        let p = prompt(
            "- habit\n",
            &[("vim.repeat-jk".into(), "Pressing j".into())],
        );
        assert!(p.contains("- vim.repeat-jk: Pressing j"));
        assert!(p.ends_with("{\"tips\": {\"<rule id>\": \"<sentence>\"}}\n"));
    }
}
