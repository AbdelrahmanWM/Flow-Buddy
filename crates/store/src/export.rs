//! The mistake memory as a compact, versioned JSON document.
//!
//! This is what `flowbuddy memory export` prints and what a future AI
//! layer will receive. It contains aggregates and abstract shapes only.
//! The format is documented in docs/memory.md; bump [`EXPORT_VERSION`]
//! for any incompatible change.

use flowbuddy_core::learning::Stage;
use serde::{Deserialize, Serialize};

use crate::{Result, Store};

pub const EXPORT_VERSION: u32 = 1;
/// Keep the document small enough to hand to a model as context.
const MAX_PATTERNS: usize = 20;
const MAX_SHAPES: usize = 5;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Export {
    pub format: String,
    pub version: u32,
    /// Unix seconds.
    pub generated_at: u64,
    pub summary: Summary,
    /// Most relevant first: what to coach next comes first.
    pub patterns: Vec<PatternExport>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub patterns: usize,
    pub mastered: usize,
    pub improving: usize,
    pub regressed: usize,
    /// Unix seconds of the first thing ever recorded.
    pub tracking_since: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Counts {
    pub hits: u64,
    pub adoptions: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatternExport {
    pub rule: String,
    pub context: String,
    pub description: String,
    pub stage: Stage,
    /// Unix seconds.
    pub stage_since: u64,
    pub first_seen: u64,
    pub last_seen: u64,
    pub total: Counts,
    pub last_7_days: Counts,
    pub nudges: u64,
    pub dismissed: u64,
    pub false_positives: u64,
    /// Most recent shapes, e.g. `["j×7", "j×12"]`.
    pub recent_shapes: Vec<String>,
}

/// Coach-next ordering: regressed, then nudged/new, improving, mastered.
fn stage_rank(stage: Stage) -> u8 {
    match stage {
        Stage::Regressed => 0,
        Stage::Nudged => 1,
        Stage::New => 2,
        Stage::Improving => 3,
        Stage::Mastered => 4,
    }
}

impl Export {
    /// Build the export. `describe` maps a rule id to its description;
    /// `week` is the (from, to) day range for `last_7_days`.
    pub fn build(
        store: &Store,
        describe: &dyn Fn(&str) -> Option<String>,
        week: (&str, &str),
        now_s: u64,
    ) -> Result<Export> {
        let mut patterns = Vec::new();
        for p in store.patterns()? {
            let w = store.window(&p.rule_id, &p.context, week.0, week.1)?;
            let recent_shapes = store
                .recent_episodes(&p.rule_id, &p.context, MAX_SHAPES)?
                .into_iter()
                .map(|e| e.shape)
                .collect();
            patterns.push(PatternExport {
                description: describe(&p.rule_id).unwrap_or_default(),
                rule: p.rule_id,
                context: p.context,
                stage: p.stage,
                stage_since: p.stage_since,
                first_seen: p.first_seen,
                last_seen: p.last_seen,
                total: Counts {
                    hits: p.occurrences,
                    adoptions: p.adoptions,
                },
                last_7_days: Counts {
                    hits: w.hits,
                    adoptions: w.adoptions,
                },
                nudges: p.nudges,
                dismissed: p.dismissed,
                false_positives: p.false_positives,
                recent_shapes,
            });
        }
        let count = |s: Stage| patterns.iter().filter(|p| p.stage == s).count();
        let summary = Summary {
            patterns: patterns.len(),
            mastered: count(Stage::Mastered),
            improving: count(Stage::Improving),
            regressed: count(Stage::Regressed),
            tracking_since: store.first_seen()?,
        };
        patterns.sort_by(|a, b| {
            stage_rank(a.stage)
                .cmp(&stage_rank(b.stage))
                .then(b.last_7_days.hits.cmp(&a.last_7_days.hits))
                .then(b.total.hits.cmp(&a.total.hits))
                .then(a.rule.cmp(&b.rule))
        });
        patterns.truncate(MAX_PATTERNS);
        Ok(Export {
            format: "flowbuddy.memory".into(),
            version: EXPORT_VERSION,
            generated_at: now_s,
            summary,
            patterns,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Event;

    fn sample() -> Store {
        let s = Store::open_in_memory().unwrap();
        let hit = |shape| Event::Hit {
            mode: "normal",
            shape,
        };
        s.record(
            "2026-10-01",
            "vscode-vim",
            "vim.repeat-jk",
            hit("j×9"),
            1_000_000,
        )
        .unwrap();
        s.record(
            "2026-10-23",
            "vscode-vim",
            "vim.repeat-jk",
            hit("j×7"),
            1_003_600,
        )
        .unwrap();
        s.record(
            "2026-10-23",
            "vscode-vim",
            "vim.repeat-jk",
            Event::Adoption,
            1_003_700,
        )
        .unwrap();
        s.record(
            "2026-10-23",
            "vscode-vim",
            "vim.repeat-jk",
            Event::Nudge,
            1_003_600,
        )
        .unwrap();
        s.record(
            "2026-10-20",
            "terminal-vim",
            "vim.w-then-q",
            hit(":w then :q"),
            1_002_000,
        )
        .unwrap();
        s.set_stage("vim.w-then-q", "terminal-vim", Stage::Mastered, 1_004_000)
            .unwrap();
        s
    }

    fn describe(id: &str) -> Option<String> {
        Some(format!("about {id}"))
    }

    /// Golden file: the documented format must not change by accident.
    #[test]
    fn golden_export() {
        let e = Export::build(
            &sample(),
            &describe,
            ("2026-10-18", "2026-10-24"),
            1_005_000,
        )
        .unwrap();
        let json = serde_json::to_string_pretty(&e).unwrap();
        let expected = include_str!("../tests/golden_export.json");
        assert_eq!(json.trim(), expected.trim(), "\n{json}");
    }

    #[test]
    fn ordering_puts_what_to_coach_first() {
        let e = Export::build(&sample(), &describe, ("2026-10-18", "2026-10-24"), 0).unwrap();
        let rules: Vec<_> = e.patterns.iter().map(|p| p.rule.as_str()).collect();
        assert_eq!(rules, ["vim.repeat-jk", "vim.w-then-q"]);
        assert_eq!(e.summary.mastered, 1);
    }

    #[test]
    fn export_round_trips() {
        let e = Export::build(&sample(), &describe, ("2026-10-18", "2026-10-24"), 0).unwrap();
        let back: Export = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(back, e);
    }
}
