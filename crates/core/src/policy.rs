//! Coaching modes and the decision of when to nudge.

use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};

use crate::config::{CoachingConfig, TimeRange, parse_range};
use crate::learning::Stage;
use crate::rule::Finding;

/// How involved Flow Buddy is. See docs/architecture.md#coaching-modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoachingMode {
    /// Capture paused.
    Off,
    /// Count only; summaries on demand and once a day.
    Passive,
    /// A short batch of tips every few minutes.
    #[default]
    Balanced,
    /// Real-time nudges, rate-limited.
    Active,
}

impl CoachingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            CoachingMode::Off => "off",
            CoachingMode::Passive => "passive",
            CoachingMode::Balanced => "balanced",
            CoachingMode::Active => "active",
        }
    }
}

impl std::str::FromStr for CoachingMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "off" => Ok(CoachingMode::Off),
            "passive" => Ok(CoachingMode::Passive),
            "balanced" => Ok(CoachingMode::Balanced),
            "active" => Ok(CoachingMode::Active),
            _ => Err(format!(
                "unknown mode {s:?} (expected off, passive, balanced or active)"
            )),
        }
    }
}

const MINUTE_MS: u64 = 60_000;
const HOUR_MS: u64 = 60 * MINUTE_MS;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PolicyConfig {
    pub cooldown_ms: u64,
    pub hourly_cap: usize,
    pub interval_ms: u64,
    pub batch_size: usize,
    pub quiet: Option<TimeRange>,
}

impl From<&CoachingConfig> for PolicyConfig {
    fn from(c: &CoachingConfig) -> Self {
        PolicyConfig {
            cooldown_ms: u64::from(c.cooldown_minutes) * MINUTE_MS,
            hourly_cap: c.hourly_cap as usize,
            interval_ms: u64::from(c.balanced_interval_minutes.max(1)) * MINUTE_MS,
            batch_size: c.batch_size.max(1) as usize,
            quiet: c.quiet_hours.as_deref().and_then(parse_range),
        }
    }
}

/// Everything outside the policy that decides whether nudging is allowed.
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    pub mode: CoachingMode,
    pub paused: bool,
    /// Local minutes since midnight, for quiet hours.
    pub local_minute: u32,
}

/// Facts about the rule behind a finding.
#[derive(Clone, Copy, Debug)]
pub struct RuleFacts {
    pub muted: bool,
    pub stage: Stage,
    /// The habit picked to work on now; balanced batches favour it.
    pub focus: bool,
    /// A mastered habit is due a spaced check-in, so it may be nudged once.
    pub check_in: bool,
}

struct Queued {
    finding: Finding,
    count: u32,
    stage: Stage,
    focus: bool,
}

/// Decides whether, when and which findings become nudges.
pub struct Coach {
    cfg: PolicyConfig,
    last_per_rule: HashMap<&'static str, u64>,
    recent: VecDeque<u64>,
    queue: HashMap<&'static str, Queued>,
    last_batch_ms: Option<u64>,
    /// Balanced mode shows its first tip at once, so you see it works
    /// (a new install would otherwise wait half an hour); then batches.
    first_shown: bool,
}

impl Coach {
    pub fn new(cfg: PolicyConfig) -> Self {
        Coach {
            cfg,
            last_per_rule: HashMap::new(),
            recent: VecDeque::new(),
            queue: HashMap::new(),
            last_batch_ms: None,
            first_shown: false,
        }
    }

    pub fn set_config(&mut self, cfg: PolicyConfig) {
        self.cfg = cfg;
    }

    fn quiet(&self, gate: &Gate) -> bool {
        self.cfg
            .quiet
            .is_some_and(|q| q.contains(gate.local_minute))
    }

    /// Whether any nudge may be shown right now.
    fn open(&self, gate: &Gate) -> bool {
        matches!(gate.mode, CoachingMode::Balanced | CoachingMode::Active)
            && !gate.paused
            && !self.quiet(gate)
    }

    /// A rule found something. Returns it if it should be shown now
    /// (active mode); in balanced mode it is queued for the next batch.
    pub fn on_finding(
        &mut self,
        finding: &Finding,
        facts: RuleFacts,
        gate: &Gate,
        now_ms: u64,
    ) -> Option<Finding> {
        if facts.muted || (facts.stage == Stage::Mastered && !facts.check_in) {
            return None;
        }
        // A mistake is worth knowing now, while you're stuck: right away in
        // balanced mode too, once per rule per cooldown.
        if finding.rule_id.starts_with("mistake.")
            && matches!(gate.mode, CoachingMode::Active | CoachingMode::Balanced)
            && self.open(gate)
        {
            let cooled = self
                .last_per_rule
                .get(finding.rule_id)
                .is_none_or(|&t| now_ms.saturating_sub(t) >= self.cfg.cooldown_ms);
            if !cooled {
                return None;
            }
            self.last_per_rule.insert(finding.rule_id, now_ms);
            return Some(finding.clone());
        }
        match gate.mode {
            CoachingMode::Active if self.open(gate) => {
                self.recent.retain(|&t| now_ms.saturating_sub(t) < HOUR_MS);
                let cooled = self
                    .last_per_rule
                    .get(finding.rule_id)
                    .is_none_or(|&t| now_ms.saturating_sub(t) >= self.cfg.cooldown_ms);
                if !cooled || self.recent.len() >= self.cfg.hourly_cap {
                    return None;
                }
                self.last_per_rule.insert(finding.rule_id, now_ms);
                self.recent.push_back(now_ms);
                Some(finding.clone())
            }
            CoachingMode::Balanced if !self.first_shown && self.open(gate) => {
                self.first_shown = true;
                // The next batch is a full interval after this one.
                self.last_batch_ms = Some(now_ms);
                self.last_per_rule.insert(finding.rule_id, now_ms);
                Some(finding.clone())
            }
            CoachingMode::Balanced => {
                let q = self.queue.entry(finding.rule_id).or_insert(Queued {
                    finding: finding.clone(),
                    count: 0,
                    stage: facts.stage,
                    focus: facts.focus,
                });
                q.count += 1;
                q.finding = finding.clone();
                q.stage = facts.stage;
                q.focus = facts.focus;
                None
            }
            _ => None,
        }
    }

    /// Balanced mode: every interval, return the most valuable findings.
    pub fn on_tick(&mut self, gate: &Gate, now_ms: u64) -> Vec<Finding> {
        if gate.mode != CoachingMode::Balanced {
            self.queue.clear();
            return Vec::new();
        }
        let last = *self.last_batch_ms.get_or_insert(now_ms);
        if now_ms.saturating_sub(last) < self.cfg.interval_ms {
            return Vec::new();
        }
        if !self.open(gate) {
            // Keep collecting through quiet hours and pauses.
            return Vec::new();
        }
        self.last_batch_ms = Some(now_ms);
        let mut picked: Vec<Queued> = self.queue.drain().map(|(_, q)| q).collect();
        picked.sort_by(|a, b| score(b).total_cmp(&score(a)));
        picked
            .into_iter()
            .filter(|q| score(q) > 0.0)
            .take(self.cfg.batch_size)
            .map(|q| q.finding)
            .collect()
    }
}

/// How much a queued finding deserves a tip: how often it happened,
/// weighted by where the habit stands, doubled for the focus habit.
fn score(q: &Queued) -> f64 {
    let focus = if q.focus { 2.0 } else { 1.0 };
    f64::from(q.count) * q.stage.nudge_weight() * focus
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::VimMode;
    use crate::rule::Shape;

    fn cfg() -> PolicyConfig {
        PolicyConfig::from(&CoachingConfig::default())
    }

    fn finding(rule_id: &'static str) -> Finding {
        Finding {
            rule_id,
            title: "t",
            text: "x".into(),
            shape: Shape::repeat("j", 6),
            mode: VimMode::Normal,
            at_ms: 0,
            start_ms: 0,
        }
    }

    fn gate(mode: CoachingMode) -> Gate {
        Gate {
            mode,
            paused: false,
            local_minute: 12 * 60,
        }
    }

    const NEW: RuleFacts = RuleFacts {
        muted: false,
        stage: Stage::New,
        focus: false,
        check_in: false,
    };

    #[test]
    fn off_and_passive_never_nudge() {
        for mode in [CoachingMode::Off, CoachingMode::Passive] {
            let mut c = Coach::new(cfg());
            assert!(c.on_finding(&finding("a"), NEW, &gate(mode), 0).is_none());
            assert!(c.on_tick(&gate(mode), 10 * HOUR_MS).is_empty());
        }
    }

    #[test]
    fn active_nudges_with_cooldown_per_rule() {
        let mut c = Coach::new(cfg());
        let g = gate(CoachingMode::Active);
        assert!(c.on_finding(&finding("a"), NEW, &g, 0).is_some());
        assert!(c.on_finding(&finding("a"), NEW, &g, MINUTE_MS).is_none());
        assert!(c.on_finding(&finding("b"), NEW, &g, MINUTE_MS).is_some());
        assert!(
            c.on_finding(&finding("a"), NEW, &g, 11 * MINUTE_MS)
                .is_some()
        );
    }

    #[test]
    fn mistakes_come_right_away_in_balanced_mode_too() {
        let mut c = batching(cfg());
        let g = gate(CoachingMode::Balanced);
        assert!(
            c.on_finding(&finding("vim.repeat-jk"), NEW, &g, 0)
                .is_none(),
            "batched"
        );
        assert!(
            c.on_finding(&finding("mistake.no-command"), NEW, &g, 0)
                .is_some()
        );
        assert!(
            c.on_finding(&finding("mistake.no-command"), NEW, &g, MINUTE_MS)
                .is_none(),
            "cooling down"
        );
        let quiet = gate(CoachingMode::Passive);
        assert!(
            c.on_finding(&finding("mistake.no-command"), NEW, &quiet, HOUR_MS)
                .is_none()
        );
    }

    #[test]
    fn active_respects_hourly_cap() {
        let mut c = Coach::new(PolicyConfig {
            cooldown_ms: 0,
            hourly_cap: 3,
            ..cfg()
        });
        let g = gate(CoachingMode::Active);
        let shown = (0..10)
            .filter(|i| {
                c.on_finding(&finding("a"), NEW, &g, i * MINUTE_MS)
                    .is_some()
            })
            .count();
        assert_eq!(shown, 3);
        assert!(
            c.on_finding(&finding("a"), NEW, &g, HOUR_MS + MINUTE_MS)
                .is_some()
        );
    }

    #[test]
    fn muted_mastered_paused_and_quiet_are_silent() {
        let mut c = Coach::new(PolicyConfig {
            quiet: parse_range("22:00-08:00"),
            ..cfg()
        });
        let g = gate(CoachingMode::Active);
        let muted = RuleFacts {
            muted: true,
            stage: Stage::New,
            focus: false,
            check_in: false,
        };
        let mastered = RuleFacts {
            muted: false,
            stage: Stage::Mastered,
            focus: false,
            check_in: false,
        };
        assert!(c.on_finding(&finding("a"), muted, &g, 0).is_none());
        assert!(c.on_finding(&finding("a"), mastered, &g, 0).is_none());
        let paused = Gate { paused: true, ..g };
        assert!(c.on_finding(&finding("a"), NEW, &paused, 0).is_none());
        let night = Gate {
            local_minute: 23 * 60,
            ..g
        };
        assert!(c.on_finding(&finding("a"), NEW, &night, 0).is_none());
    }

    /// A coach past its first tip: balanced mode batches from here.
    fn batching(cfg: PolicyConfig) -> Coach {
        let mut c = Coach::new(cfg);
        c.first_shown = true;
        c
    }

    #[test]
    fn balanced_shows_the_first_tip_at_once_then_batches() {
        let mut c = Coach::new(cfg());
        let g = gate(CoachingMode::Balanced);
        assert!(c.on_tick(&g, 0).is_empty());
        let first = c.on_finding(&finding("a"), NEW, &g, MINUTE_MS);
        assert_eq!(first.map(|f| f.rule_id), Some("a"), "you see it works");
        assert!(
            c.on_finding(&finding("b"), NEW, &g, 2 * MINUTE_MS)
                .is_none(),
            "then batched"
        );
        assert!(
            c.on_tick(&g, 30 * MINUTE_MS).is_empty(),
            "a full interval after the first"
        );
        assert_eq!(c.on_tick(&g, 31 * MINUTE_MS).len(), 1);
    }

    #[test]
    fn balanced_batches_top_findings_per_interval() {
        let mut c = batching(cfg());
        let g = gate(CoachingMode::Balanced);
        assert!(c.on_tick(&g, 0).is_empty(), "first tick starts the clock");
        for _ in 0..5 {
            c.on_finding(&finding("often"), NEW, &g, MINUTE_MS);
        }
        c.on_finding(&finding("once"), NEW, &g, MINUTE_MS);
        for _ in 0..3 {
            let improving = RuleFacts {
                muted: false,
                stage: Stage::Improving,
                focus: false,
                check_in: false,
            };
            c.on_finding(&finding("improving"), improving, &g, MINUTE_MS);
        }
        assert!(c.on_tick(&g, 10 * MINUTE_MS).is_empty(), "not yet");
        let batch: Vec<_> = c
            .on_tick(&g, 30 * MINUTE_MS)
            .iter()
            .map(|f| f.rule_id)
            .collect();
        assert_eq!(batch, ["often", "improving"]);
        assert!(
            c.on_tick(&g, 61 * MINUTE_MS).is_empty(),
            "queue was drained"
        );
    }

    #[test]
    fn mastered_habits_only_at_check_ins() {
        let mut c = Coach::new(cfg());
        let g = gate(CoachingMode::Active);
        let mastered = RuleFacts {
            stage: Stage::Mastered,
            ..NEW
        };
        assert!(c.on_finding(&finding("a"), mastered, &g, 0).is_none());
        let due = RuleFacts {
            check_in: true,
            ..mastered
        };
        assert!(c.on_finding(&finding("a"), due, &g, 0).is_some());
    }

    #[test]
    fn balanced_puts_the_focus_habit_first() {
        let mut c = batching(PolicyConfig {
            batch_size: 1,
            ..cfg()
        });
        let g = gate(CoachingMode::Balanced);
        c.on_tick(&g, 0);
        for _ in 0..4 {
            c.on_finding(&finding("other"), NEW, &g, MINUTE_MS);
        }
        let focus = RuleFacts { focus: true, ..NEW };
        for _ in 0..3 {
            c.on_finding(&finding("focus"), focus, &g, MINUTE_MS);
        }
        let batch: Vec<_> = c
            .on_tick(&g, 30 * MINUTE_MS)
            .iter()
            .map(|f| f.rule_id)
            .collect();
        assert_eq!(batch, ["focus"]);
    }

    #[test]
    fn balanced_holds_batch_through_quiet_hours() {
        let mut c = Coach::new(PolicyConfig {
            quiet: parse_range("22:00-08:00"),
            ..cfg()
        });
        let day = gate(CoachingMode::Balanced);
        let night = Gate {
            local_minute: 23 * 60,
            ..day
        };
        c.on_tick(&night, 0);
        c.on_finding(&finding("a"), NEW, &night, MINUTE_MS);
        assert!(c.on_tick(&night, HOUR_MS).is_empty());
        assert_eq!(c.on_tick(&day, 2 * HOUR_MS).len(), 1);
    }
}
