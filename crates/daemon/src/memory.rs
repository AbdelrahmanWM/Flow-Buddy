//! The daemon's side of the mistake memory: record what happened, keep
//! learning stages up to date, prune old episodes.

use std::collections::{HashMap, HashSet};

use flowbuddy_core::config::Config;
use flowbuddy_core::context::ContextKind;
use flowbuddy_core::curriculum;
use flowbuddy_core::ipc::NudgeAction;
use flowbuddy_core::learning::{self, LearningConfig, Stage};
use flowbuddy_core::rule::Finding;
use flowbuddy_core::tipfilter::{self, Label, Model, TipFeatures};
use flowbuddy_store::{Event, Store, days};

const HOUR_MS: u64 = 3_600_000;

pub use flowbuddy_store::days::day;

/// What [`Memory::progress`] reads from the database.
#[derive(Clone)]
struct ProgressCache {
    /// The day it was read: "recent" moves on with the days.
    day: String,
    /// What you said you know or don't (`flowbuddy learn --know`).
    overrides: HashMap<String, bool>,
    /// Per environment (`None`: all of them), read when first wanted.
    uses: HashMap<Option<ContextKind>, HashMap<String, flowbuddy_store::ItemUse>>,
    /// Mastery per skill and the fitted half-life model, likewise.
    skills: HashMap<Option<ContextKind>, Skills>,
    items: HashMap<String, flowbuddy_store::LearnRow>,
}

type Skills = (
    HashMap<String, flowbuddy_core::mastery::Skill>,
    flowbuddy_core::mastery::Model,
);

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
    /// Curriculum levels taken as known (`[learn] level`).
    known_levels: u8,
    /// Uses and daily-tip rows per curriculum item, read once and kept up
    /// to date here, so knowing what you know needn't read the database.
    progress_cache: std::cell::RefCell<Option<ProgressCache>>,
    retention_days: u32,
    /// The habit worth working on now (`flowbuddy next`), updated hourly.
    focus: Option<String>,
    /// Mastered patterns due a spaced check-in, updated hourly.
    check_ins: HashSet<PatternKey>,
    /// Tips still waiting for an outcome: rule → (row, shown at).
    open_tips: HashMap<&'static str, (i64, u64)>,
    /// Tips shown today, for the filter's features: (day, count).
    tips_today: (String, u32),
    /// The learned tip filter, retrained hourly; `None` until it has
    /// enough outcomes.
    filter: Option<Model>,
    /// Per rule: tips shown so far, and skipped since the last one shown.
    shown: HashMap<String, u32>,
    skipped: HashMap<&'static str, u32>,
    /// Tips the filter skipped today: (day, count).
    skipped_today: (String, u32),
}

/// A tip is adopted if the better way comes within this long.
const TIP_WINDOW_MS: u64 = 30 * 60_000;
/// Tip outcomes older than this are deleted.
const TIP_RETENTION_S: u64 = 90 * 86_400;

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
            known_levels: config.learn.level.known_levels(),
            progress_cache: std::cell::RefCell::new(None),
            retention_days: config.memory.episode_retention_days,
            focus: None,
            check_ins: HashSet::new(),
            open_tips: HashMap::new(),
            tips_today: (String::new(), 0),
            filter: None,
            shown: HashMap::new(),
            skipped: HashMap::new(),
            skipped_today: (String::new(), 0),
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
        self.known_levels = config.learn.level.known_levels();
    }

    /// A tip as it was shown, for `flowbuddy tips` and the menu.
    pub fn add_history(&self, tip: &flowbuddy_store::HistoryTip) {
        if let Some(store) = &self.store {
            let _ = store.add_history(tip);
        }
    }

    pub fn history(&self, limit: usize) -> Vec<flowbuddy_store::HistoryTip> {
        self.store
            .as_ref()
            .and_then(|s| s.history("", limit).ok())
            .unwrap_or_default()
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
        self.close_tip(rule_id, flowbuddy_store::Outcome::Adopted);
    }

    /// A tip was shown: open a row for what comes of it.
    pub fn tip_shown(&mut self, rule_id: &'static str, mode: &str, local_hour: u32, now_ms: u64) {
        self.open_tip(rule_id, mode, local_hour, now_ms, false);
    }

    /// A tip was held back at random: what comes of it anyway is the
    /// comparison.
    pub fn tip_held(&mut self, rule_id: &'static str, mode: &str, local_hour: u32, now_ms: u64) {
        self.open_tip(rule_id, mode, local_hour, now_ms, true);
    }

    fn open_tip(
        &mut self,
        rule_id: &'static str,
        mode: &str,
        local_hour: u32,
        now_ms: u64,
        held: bool,
    ) {
        let Some(store) = &self.store else { return };
        let Some(ctx) = self.finding_context(rule_id) else {
            return;
        };
        let today = day(now_ms);
        if self.tips_today.0 != today {
            self.tips_today = (today, 0);
        }
        if !held {
            *self.shown.entry(rule_id.to_string()).or_default() += 1;
            self.skipped.remove(rule_id);
        }
        let stage = self.stage(rule_id, ctx);
        let tip = flowbuddy_store::TipShown {
            shown_at: now_ms / 1000,
            rule_id,
            context: ctx.as_str(),
            stage: stage.as_str(),
            hour: local_hour,
            mode,
            prior_today: self.tips_today.1,
            held,
        };
        if !held {
            self.tips_today.1 += 1;
        }
        match store.tip_shown(&tip) {
            Ok(id) => {
                // A newer tip for the same rule replaces the open one.
                if let Some((old, _)) = self.open_tips.insert(rule_id, (id, now_ms)) {
                    let _ = store.close_tip(old, flowbuddy_store::Outcome::Ignored);
                }
            }
            Err(e) => tracing::warn!(error = %e, "could not record a tip"),
        }
    }

    /// Whether the learned filter lets this tip through.
    pub fn filter_allows(
        &mut self,
        rule_id: &'static str,
        mode: &str,
        local_hour: u32,
        now_ms: u64,
    ) -> bool {
        let Some(ctx) = self.finding_context(rule_id) else {
            return true;
        };
        let today = day(now_ms);
        let prior_today = if self.tips_today.0 == today {
            self.tips_today.1
        } else {
            0
        };
        let stage = self.stage(rule_id, ctx);
        let f = TipFeatures {
            rule_id,
            context: ctx.as_str(),
            stage: stage.as_str(),
            hour: local_hour,
            mode,
            prior_today,
        };
        let shown = self.shown.get(rule_id).copied().unwrap_or(0);
        let skipped = self.skipped.get(rule_id).copied().unwrap_or(0);
        let unhelpful = if self.filter.is_none() {
            self.unhelpful_in_a_row(rule_id, now_ms)
        } else {
            0
        };
        if tipfilter::should_show(self.filter.as_ref(), &f, shown, skipped, unhelpful) {
            return true;
        }
        *self.skipped.entry(rule_id).or_default() += 1;
        if self.skipped_today.0 != today {
            self.skipped_today = (today, 0);
        }
        self.skipped_today.1 += 1;
        tracing::debug!(rule = rule_id, "tip skipped by the learned filter");
        false
    }

    /// How many of this rule's latest tips (last 30 days) were dismissed,
    /// marked not helpful or ignored, counting back from the newest that
    /// has an outcome.
    pub fn unhelpful_in_a_row(&self, rule_id: &str, now_ms: u64) -> u32 {
        use flowbuddy_store::Outcome;
        let Some(store) = &self.store else { return 0 };
        let since = (now_ms / 1000).saturating_sub(30 * 86_400);
        let rows = store.tips(since).unwrap_or_default();
        rows.iter()
            .rev()
            .filter(|r| r.rule_id == rule_id && !r.held)
            .filter_map(|r| r.outcome)
            .take_while(|o| {
                matches!(
                    o,
                    Outcome::Dismissed | Outcome::NotHelpful | Outcome::Ignored
                )
            })
            .count() as u32
    }

    /// What the filter is doing, for `flowbuddy status`.
    pub fn filter_status(&self, now_ms: u64) -> String {
        match &self.filter {
            None => format!(
                "learning ({} tips so far)",
                self.shown.values().sum::<u32>()
            ),
            Some(m) => {
                let skipped = if self.skipped_today.0 == day(now_ms) {
                    self.skipped_today.1
                } else {
                    0
                };
                format!(
                    "on, learned from {} tips; {skipped} skipped today",
                    m.trained_on
                )
            }
        }
    }

    /// Retrain the filter from the outcomes kept.
    fn retrain(&mut self, now_ms: u64) {
        let Some(store) = &self.store else { return };
        let Ok(mut rows) = store.tips((now_ms / 1000).saturating_sub(TIP_RETENTION_S)) else {
            return;
        };
        // Held tips weren't seen: nothing to learn about showing them.
        rows.retain(|r| !r.held);
        use flowbuddy_store::Outcome;
        self.shown.clear();
        for r in &rows {
            *self.shown.entry(r.rule_id.clone()).or_default() += 1;
        }
        let labelled: Vec<(TipFeatures<'_>, Label)> = rows
            .iter()
            .filter_map(|r| {
                let label = match r.outcome? {
                    Outcome::Adopted => Label {
                        useful: true,
                        weight: 1.0,
                    },
                    Outcome::Dismissed | Outcome::NotHelpful => Label {
                        useful: false,
                        weight: 1.0,
                    },
                    // Not looking isn't the same as not liking it.
                    Outcome::Ignored => Label {
                        useful: false,
                        weight: 0.3,
                    },
                };
                Some((
                    TipFeatures {
                        rule_id: &r.rule_id,
                        context: &r.context,
                        stage: &r.stage,
                        hour: r.hour,
                        mode: &r.mode,
                        prior_today: r.prior_today,
                    },
                    label,
                ))
            })
            .collect();
        self.filter = Model::train(&labelled);
    }

    fn close_tip(&mut self, rule_id: &str, outcome: flowbuddy_store::Outcome) {
        if let (Some((id, _)), Some(store)) = (self.open_tips.remove(rule_id), &self.store) {
            let _ = store.close_tip(id, outcome);
        }
    }

    /// Tips with nothing after them for a while were ignored.
    pub fn expire_tips(&mut self, now_ms: u64) {
        let stale: Vec<&'static str> = self
            .open_tips
            .iter()
            .filter(|(_, (_, at))| now_ms.saturating_sub(*at) > TIP_WINDOW_MS)
            .map(|(rule, _)| *rule)
            .collect();
        for rule in stale {
            self.close_tip(rule, flowbuddy_store::Outcome::Ignored);
        }
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
        use flowbuddy_store::Outcome;
        match action {
            NudgeAction::Dismiss => self.close_tip(rule_id, Outcome::Dismissed),
            NudgeAction::FalsePositive | NudgeAction::MuteRule => {
                self.close_tip(rule_id, Outcome::NotHelpful)
            }
            NudgeAction::Seen => {}
        }
        let event = match action {
            NudgeAction::Dismiss => Event::Dismissed,
            NudgeAction::FalsePositive => Event::FalsePositive,
            NudgeAction::Seen | NudgeAction::MuteRule => return,
        };
        self.record(ctx, rule_id, event, now_ms);
    }

    /// Write curriculum uses, and mark shown items you've started using as
    /// learned.
    pub fn command_uses(&self, usage: HashMap<(ContextKind, &'static str), u64>, now_ms: u64) {
        let Some(store) = &self.store else { return };
        if usage.is_empty() {
            return;
        }
        let today = day(now_ms);
        for ((ctx, id), n) in &usage {
            let _ = store.add_command_use(&today, ctx.as_str(), id, *n);
        }
        if let Some(c) = self.progress_cache.borrow_mut().as_mut() {
            for ((ctx, id), n) in &usage {
                for (scope, uses) in c.uses.iter_mut() {
                    if scope.is_none_or(|s| s == *ctx) {
                        let u = uses.entry(id.to_string()).or_default();
                        u.total += n;
                        u.recent += n;
                        u.days_idle = 0;
                    }
                }
            }
        }
        let used: std::collections::HashSet<&str> = usage.keys().map(|(_, id)| *id).collect();
        for row in store.learn_items().unwrap_or_default() {
            if row.learned_at.is_some() || !used.contains(row.item_id.as_str()) {
                continue;
            }
            let since = day(row.first_shown * 1000);
            let uses = store
                .command_uses(&since)
                .ok()
                .and_then(|u| u.get(&row.item_id).copied())
                .unwrap_or(0);
            if uses >= curriculum::KNOWN_USES {
                let _ = store.learn_learned(&row.item_id, now_ms / 1000);
                self.progress_cache.replace(None);
            }
        }
    }

    /// Uses and daily-tip rows per curriculum item, in `ctx` (with uses
    /// from before environments were told apart), or everywhere.
    fn progress(
        &self,
        ctx: Option<ContextKind>,
        now_ms: u64,
    ) -> Option<impl Fn(&str) -> curriculum::Progress + use<>> {
        let store = self.store.as_ref()?;
        let mut cache = self.progress_cache.borrow_mut();
        let today = day(now_ms);
        let recent_from = day(now_ms.saturating_sub(curriculum::RECENT_DAYS * 86_400_000));
        if cache.as_ref().is_none_or(|c| c.day != today) {
            *cache = Some(ProgressCache {
                day: today.clone(),
                overrides: store.known_overrides().unwrap_or_default(),
                uses: HashMap::new(),
                skills: HashMap::new(),
                items: store
                    .learn_items()
                    .ok()?
                    .into_iter()
                    .map(|r| (r.item_id.clone(), r))
                    .collect(),
            });
        }
        let c = cache.as_mut()?;
        let uses = match c.uses.entry(ctx) {
            std::collections::hash_map::Entry::Occupied(e) => e.get().clone(),
            std::collections::hash_map::Entry::Vacant(e) => e
                .insert(
                    store
                        .item_uses(&today, &recent_from, ctx.map(|c| c.as_str()))
                        .ok()?,
                )
                .clone(),
        };
        let (skills, model) = match c.skills.entry(ctx) {
            std::collections::hash_map::Entry::Occupied(e) => e.get().clone(),
            std::collections::hash_map::Entry::Vacant(e) => e
                .insert(store.skills(&today, ctx.map(|c| c.as_str())).ok()?)
                .clone(),
        };
        let items = c.items.clone();
        Some(move |id: &str| {
            let row = items.get(id);
            let u = uses.get(id).copied().unwrap_or_default();
            let skill = skills.get(id).map(|s| s.percents(&model));
            curriculum::Progress {
                uses: u.total,
                recent_uses: u.recent,
                days_idle: uses.contains_key(id).then_some(u.days_idle),
                times_shown: row.map_or(0, |r| r.times_shown),
                last_shown: row.map(|r| r.last_shown),
                learned: row.is_some_and(|r| r.learned_at.is_some()),
                mastery: skill.map(|s| s.0),
                recall: skill.map(|s| s.1),
            }
        })
    }

    /// What you already use, for the skill tree; everything when there's
    /// no memory to tell.
    /// Say items are known, not known, or back to what use shows.
    pub fn set_known(&self, items: &[String], known: Option<bool>) {
        let Some(store) = &self.store else { return };
        for item in items {
            let _ = store.set_known_override(item, known);
        }
        self.progress_cache.replace(None);
    }

    pub fn known(&self, ctx: Option<ContextKind>, now_ms: u64) -> curriculum::Known {
        let Some(progress) = self.progress(ctx, now_ms) else {
            return curriculum::Known::everything();
        };
        let mastered = self
            .stages
            .iter()
            .filter(|(_, s)| **s == Stage::Mastered)
            .map(|((rule, _), _)| rule.as_str());
        let overrides = self
            .progress_cache
            .borrow()
            .as_ref()
            .map(|c| c.overrides.clone())
            .unwrap_or_default();
        curriculum::Known::from_progress(&progress, mastered)
            .with_levels(self.known_levels)
            .with_overrides(&overrides)
    }

    /// The day's "something new", or a review, if anything's left.
    pub fn pick_something_new(&self, ctx: ContextKind, now_ms: u64) -> Option<curriculum::Pick> {
        let progress = self.progress(Some(ctx), now_ms)?;
        // This week's habits, most hits first.
        let (from, to) = flowbuddy_store::days::range(now_ms, 7, 0);
        let mut hits: HashMap<String, u64> = HashMap::new();
        for row in self.store.as_ref()?.daily(&from, &to).unwrap_or_default() {
            *hits.entry(row.rule_id).or_default() += row.hits;
        }
        // Rules you tripped, not ones you only did the better way for.
        let mut habits: Vec<(String, u64)> = hits.into_iter().filter(|(_, n)| *n > 0).collect();
        habits.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        curriculum::choose(
            &progress,
            &self.known(Some(ctx), now_ms),
            now_ms / 1000,
            ctx,
            &habits,
        )
    }

    /// A challenge ended: `done`, `missed` or `expired`.
    pub fn challenge_ended(&self, item_id: &str, set_at: u64, outcome: &str, now_ms: u64) {
        if let Some(store) = &self.store {
            let _ = store.challenge_ended(item_id, set_at, &day(now_ms), outcome);
            self.progress_cache.replace(None);
        }
    }

    pub fn learn_shown(&self, item_id: &str, now_ms: u64) {
        if let Some(store) = &self.store {
            let _ = store.learn_shown(item_id, now_ms / 1000);
            self.progress_cache.replace(None);
        }
    }

    /// Write typing accuracy counts (opt-in).
    pub fn typing(&self, counts: crate::typing::Counts, now_ms: u64) {
        let Some(store) = &self.store else { return };
        let today = day(now_ms);
        let (contexts, pairs) = counts;
        for (ctx, typed, corrections) in contexts {
            let _ = store.add_typing(&today, ctx.as_str(), typed, corrections);
        }
        for (pair, n) in pairs {
            let _ = store.add_typo_pair(&today, &pair, n);
        }
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
            let _ = store.prune_tips((now_ms / 1000).saturating_sub(TIP_RETENTION_S));
            let cutoff = (now_ms / 1000).saturating_sub(u64::from(self.retention_days) * 86_400);
            match store.prune_episodes(cutoff) {
                Ok(0) => {}
                Ok(n) => tracing::info!(episodes = n, "pruned old episodes"),
                Err(e) => tracing::warn!(error = %e, "could not prune episodes"),
            }
        }
        self.refresh_stages();
        self.retrain(now_ms);
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

#[cfg(test)]
mod tests {
    use super::*;
    use flowbuddy_store::{Outcome, TipShown};

    #[test]
    fn backs_off_a_rule_nobody_heeds_before_the_filter_learns() {
        let store = Store::open_in_memory().unwrap();
        let tip = |at| TipShown {
            shown_at: at,
            rule_id: "vim.repeat-jk",
            context: "terminal-vim",
            stage: "nudged",
            hour: 10,
            mode: "active",
            prior_today: 0,
            held: false,
        };
        for (i, outcome) in [
            Outcome::Adopted,
            Outcome::Dismissed,
            Outcome::Ignored,
            Outcome::NotHelpful,
        ]
        .into_iter()
        .enumerate()
        {
            let id = store.tip_shown(&tip(1_000 + i as u64)).unwrap();
            store.close_tip(id, outcome).unwrap();
        }
        let mut m = Memory::new(Some(store), &Config::default());
        assert_eq!(m.unhelpful_in_a_row("vim.repeat-jk", 2_000_000), 3);
        assert_eq!(m.unhelpful_in_a_row("vim.repeat-x", 2_000_000), 0);
        let f = Finding {
            rule_id: "vim.repeat-jk",
            title: "t",
            text: "x".into(),
            shape: flowbuddy_core::rule::Shape::fixed("x"),
            mode: flowbuddy_core::context::VimMode::Normal,
            at_ms: 0,
            start_ms: 0,
        };
        m.hit(&f, ContextKind::TerminalVim, 2_000_000);
        assert!(!m.filter_allows("vim.repeat-jk", "active", 10, 2_000_000));
    }
}
