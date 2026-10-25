//! The daemon's side of the mistake memory: record what happened, keep
//! learning stages up to date, prune old episodes.

use std::collections::{HashMap, HashSet};

use flowbuddy_core::config::Config;
use flowbuddy_core::context::ContextKind;
use flowbuddy_core::ipc::NudgeAction;
use flowbuddy_core::learning::{self, LearningConfig, Stage};
use flowbuddy_core::rule::Finding;
use flowbuddy_store::{Event, Store, days};

const HOUR_MS: u64 = 3_600_000;

pub use flowbuddy_store::days::day;

/// A pattern: (rule id, context).
type PatternKey = (String, String);

pub struct Memory {
    store: Option<Store>,
    stages: HashMap<(String, String), Stage>,
    /// Context of each rule's latest finding, for nudges delivered later.
    last_ctx: HashMap<&'static str, ContextKind>,
    last_active_minute: Option<(u64, ContextKind)>,
    last_maintenance_ms: Option<u64>,
    learning: LearningConfig,
    retention_days: u32,
    /// The habit worth working on now (`flowbuddy next`), updated hourly.
    focus: Option<String>,
    /// Mastered patterns due a spaced check-in, updated hourly.
    check_ins: HashSet<PatternKey>,
}

impl Memory {
    pub fn new(store: Option<Store>, config: &Config) -> Self {
        let stages = store
            .as_ref()
            .and_then(|s| s.stages().ok())
            .unwrap_or_default();
        Memory {
            store,
            stages,
            last_ctx: HashMap::new(),
            last_active_minute: None,
            last_maintenance_ms: None,
            learning: config.learning,
            retention_days: config.memory.episode_retention_days,
            focus: None,
            check_ins: HashSet::new(),
        }
    }

    /// "Not helpful" reports for a rule in the last 14 days, all contexts.
    pub fn recent_false_positives(&self, rule_id: &str, now_ms: u64) -> u64 {
        let Some(store) = &self.store else { return 0 };
        let (from, to) = days::range(now_ms, 14, 0);
        store
            .daily(&from, &to)
            .unwrap_or_default()
            .iter()
            .filter(|r| r.rule_id == rule_id)
            .map(|r| r.false_positives)
            .sum()
    }

    pub fn check_in_due(&self, rule_id: &str, ctx: ContextKind) -> bool {
        self.check_ins
            .contains(&(rule_id.to_string(), ctx.as_str().to_string()))
    }

    pub fn is_focus(&self, rule_id: &str) -> bool {
        self.focus.as_deref() == Some(rule_id)
    }

    pub fn set_config(&mut self, config: &Config) {
        self.learning = config.learning;
        self.retention_days = config.memory.episode_retention_days;
    }

    #[cfg(test)]
    pub fn store(&self) -> Option<&Store> {
        self.store.as_ref()
    }

    fn record(&self, ctx: ContextKind, rule_id: &str, event: Event<'_>, now_ms: u64) {
        let Some(store) = &self.store else { return };
        if let Err(e) = store.record(&day(now_ms), ctx.as_str(), rule_id, event, now_ms / 1000) {
            tracing::warn!(error = %e, "could not record to the memory");
        }
    }

    pub fn stage(&self, rule_id: &str, ctx: ContextKind) -> Stage {
        self.stages
            .get(&(rule_id.to_string(), ctx.as_str().to_string()))
            .copied()
            .unwrap_or_default()
    }

    pub fn hit(&mut self, f: &Finding, ctx: ContextKind, now_ms: u64) {
        self.last_ctx.insert(f.rule_id, ctx);
        let shape = f.shape.to_string();
        let event = Event::Hit {
            mode: f.mode.as_str(),
            shape: &shape,
        };
        self.record(ctx, f.rule_id, event, now_ms);
    }

    pub fn adoption(&mut self, rule_id: &'static str, ctx: ContextKind, now_ms: u64) {
        self.record(ctx, rule_id, Event::Adoption, now_ms);
    }

    pub fn finding_context(&self, rule_id: &str) -> Option<ContextKind> {
        self.last_ctx.get(rule_id).copied()
    }

    pub fn nudged(&mut self, rule_id: &'static str, now_ms: u64) {
        let Some(ctx) = self.finding_context(rule_id) else {
            return;
        };
        self.record(ctx, rule_id, Event::Nudge, now_ms);
        let key = (rule_id.to_string(), ctx.as_str().to_string());
        self.check_ins.remove(&key);
        let stage = self.stages.entry(key).or_default();
        if *stage == Stage::New {
            *stage = Stage::Nudged;
            if let Some(store) = &self.store {
                let _ = store.set_stage(rule_id, ctx.as_str(), Stage::Nudged, now_ms / 1000);
            }
        }
    }

    pub fn response(&mut self, rule_id: &'static str, action: NudgeAction, now_ms: u64) {
        let Some(ctx) = self.finding_context(rule_id) else {
            return;
        };
        let event = match action {
            NudgeAction::Dismiss => Event::Dismissed,
            NudgeAction::FalsePositive => Event::FalsePositive,
            NudgeAction::Seen | NudgeAction::MuteRule => return,
        };
        self.record(ctx, rule_id, event, now_ms);
    }

    /// Count minutes of typing in each supported context.
    pub fn active(&mut self, ctx: ContextKind, now_ms: u64) {
        let minute = now_ms / 60_000;
        if self.last_active_minute == Some((minute, ctx)) {
            return;
        }
        self.last_active_minute = Some((minute, ctx));
        if let Some(store) = &self.store {
            let _ = store.add_active_minute(&day(now_ms), ctx.as_str());
        }
    }

    /// Hourly: move patterns between learning stages, prune old episodes.
    pub fn maintain(&mut self, now_ms: u64) {
        if self
            .last_maintenance_ms
            .is_some_and(|t| now_ms.saturating_sub(t) < HOUR_MS)
        {
            return;
        }
        self.last_maintenance_ms = Some(now_ms);
        if let Some(store) = &self.store {
            match self.update_stages(store, now_ms) {
                Ok((focus, check_ins)) => {
                    self.focus = focus;
                    self.check_ins = check_ins;
                }
                Err(e) => tracing::warn!(error = %e, "could not update learning stages"),
            }
            let cutoff = (now_ms / 1000).saturating_sub(u64::from(self.retention_days) * 86_400);
            match store.prune_episodes(cutoff) {
                Ok(0) => {}
                Ok(n) => tracing::info!(episodes = n, "pruned old episodes"),
                Err(e) => tracing::warn!(error = %e, "could not prune episodes"),
            }
        }
        self.refresh_stages();
    }

    /// Returns the habit to focus on, scored with the new stages, and the
    /// mastered patterns due a check-in.
    fn update_stages(
        &self,
        store: &Store,
        now_ms: u64,
    ) -> flowbuddy_store::Result<(Option<String>, HashSet<PatternKey>)> {
        let week = days::range(now_ms, 7, 0);
        let fortnight = days::range(now_ms, 14, 0);
        let mut focus: Option<(f64, String)> = None;
        let mut check_ins = HashSet::new();
        for p in store.patterns()? {
            let w7 = store.window(&p.rule_id, &p.context, &week.0, &week.1)?;
            let w14 = store.window(&p.rule_id, &p.context, &fortnight.0, &fortnight.1)?;
            let next = learning::next_stage(p.stage, p.nudges > 0, w7, w14, &self.learning);
            if next != p.stage {
                tracing::info!(rule = %p.rule_id, context = %p.context, from = p.stage.as_str(), to = next.as_str(), "learning stage changed");
                store.set_stage(&p.rule_id, &p.context, next, now_ms / 1000)?;
            }
            if next == Stage::Mastered {
                let since_s = if next == p.stage {
                    p.stage_since
                } else {
                    now_ms / 1000
                };
                let done = store.nudges_since(&p.rule_id, &p.context, &day(since_s * 1000))?;
                if learning::checkin_due(since_s, done, now_ms / 1000) {
                    check_ins.insert((p.rule_id.clone(), p.context.clone()));
                }
            }
            let score = learning::priority(next, w7.hits, p.false_positives);
            if score > 0.0 && focus.as_ref().is_none_or(|(best, _)| score > *best) {
                focus = Some((score, p.rule_id));
            }
        }
        Ok((focus.map(|(_, id)| id), check_ins))
    }

    fn refresh_stages(&mut self) {
        if let Some(s) = self.store.as_ref().and_then(|s| s.stages().ok()) {
            self.stages = s;
        }
    }

    /// One or two lines about today, for the end-of-day notification.
    pub fn today_summary(&self, now_ms: u64, describe: &dyn Fn(&str) -> String) -> Option<String> {
        let store = self.store.as_ref()?;
        let today = day(now_ms);
        let rows = store.daily(&today, &today).ok()?;
        let mut by_rule: HashMap<&str, (u64, u64)> = HashMap::new();
        for r in &rows {
            let e = by_rule.entry(&r.rule_id).or_default();
            e.0 += r.hits;
            e.1 += r.adoptions;
        }
        let hits: u64 = by_rule.values().map(|v| v.0).sum();
        let adoptions: u64 = by_rule.values().map(|v| v.1).sum();
        if hits + adoptions == 0 {
            return None;
        }
        let top = by_rule
            .iter()
            .max_by_key(|(id, v)| (v.0, std::cmp::Reverse(*id)))
            .filter(|(_, v)| v.0 > 0)
            .map(|(id, v)| format!("Most common: {} ({}×).", describe(id), v.0))
            .unwrap_or_default();
        Some(format!(
            "{hits} habits spotted, {adoptions} times you used the better way. {top} Run `flowbuddy summary` for details."
        ))
    }

    /// A few lines about the week for the weekly note.
    pub fn week_summary(&self, now_ms: u64, describe: &dyn Fn(&str) -> String) -> Option<String> {
        flowbuddy_store::week_summary(
            self.store.as_ref()?,
            now_ms,
            describe,
            self.focus.as_deref(),
        )
    }
}
