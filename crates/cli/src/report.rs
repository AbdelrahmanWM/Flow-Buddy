//! `flowbuddy summary` and `flowbuddy memory show`: read the database
//! directly, so they work even when the daemon isn't running.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use flowbuddy_core::engine::Engine;
use flowbuddy_core::learning::{self, Stage};
use flowbuddy_store::{Store, days};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Period {
    Today,
    Week,
}

impl Period {
    fn days(self) -> u64 {
        match self {
            Period::Today => 1,
            Period::Week => 7,
        }
    }
}

pub fn describe(rule_id: &str) -> String {
    Engine::standard()
        .rules()
        .find(|r| r.id() == rule_id)
        .map_or_else(|| rule_id.to_string(), |r| r.description().to_string())
}

fn hm(minutes: u64) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m:02}m"),
    }
}

#[derive(Default, Clone, Copy)]
struct Totals {
    hits: u64,
    adoptions: u64,
}

fn totals(store: &Store, range: &(String, String)) -> anyhow::Result<BTreeMap<String, Totals>> {
    let mut out: BTreeMap<String, Totals> = BTreeMap::new();
    for r in store.daily(&range.0, &range.1)? {
        let t = out.entry(r.rule_id).or_default();
        t.hits += r.hits;
        t.adoptions += r.adoptions;
    }
    Ok(out)
}

/// Change in hits per active hour, as an arrow and a percentage.
fn trend(now: f64, before: f64) -> String {
    if before == 0.0 {
        return if now == 0.0 {
            "–".into()
        } else {
            "new".into()
        };
    }
    let change = (now - before) / before * 100.0;
    match change.round() as i64 {
        c if c <= -5 => format!("↓ {}%", -c),
        c if c >= 5 => format!("↑ {c}%"),
        _ => "≈ same".into(),
    }
}

pub fn summary(store: &Store, now_ms: u64, period: Period) -> anyhow::Result<String> {
    let n = period.days();
    let current = days::range(now_ms, n, 0);
    let previous = days::range(now_ms, n, n);
    let (label, versus) = match period {
        Period::Today => (
            format!("today ({})", days::local_date(now_ms).format("%a %-d %b")),
            "yesterday",
        ),
        Period::Week => ("the last 7 days".to_string(), "the 7 days before"),
    };

    let active = store.active_minutes(&current.0, &current.1)?;
    let active_prev: u64 = store
        .active_minutes(&previous.0, &previous.1)?
        .values()
        .sum();
    let active_total: u64 = active.values().sum();
    let now_t = totals(store, &current)?;
    let prev_t = totals(store, &previous)?;

    let mut out = String::new();
    writeln!(out, "Flow Buddy: {label}, compared with {versus}\n")?;
    if active_total == 0 && now_t.is_empty() {
        writeln!(out, "  Nothing recorded yet for this period.")?;
        return Ok(out);
    }
    let per_ctx: Vec<String> = active
        .iter()
        .filter(|(_, m)| **m > 0)
        .map(|(c, m)| format!("{} {}", place_name(c), hm(*m)))
        .collect();
    writeln!(
        out,
        "  Active  {}  ({})\n",
        hm(active_total),
        per_ctx.join(", ")
    )?;

    let mut rows: Vec<(&String, &Totals)> = now_t.iter().collect();
    rows.sort_by(|a, b| b.1.hits.cmp(&a.1.hits).then(a.0.cmp(b.0)));
    if rows.is_empty() {
        writeln!(out, "  No habits to work on came up.")?;
    } else {
        writeln!(
            out,
            "  {:<46} {:>5}  {:>11}  Trend",
            "Habit", "Times", "Better way"
        )?;
    }
    let per_hour = |hits: u64, minutes: u64| hits as f64 * 60.0 / minutes.max(1) as f64;
    for (rule, t) in rows {
        let ratio = t.adoptions * 100 / (t.hits + t.adoptions).max(1);
        let before = prev_t.get(rule).copied().unwrap_or_default();
        let tr = trend(
            per_hour(t.hits, active_total),
            per_hour(before.hits, active_prev),
        );
        // A long name gets a line of its own, rather than being cut.
        let name = describe(rule);
        let name = if name.chars().count() > 46 {
            writeln!(out, "  {name}")?;
            String::new()
        } else {
            name
        };
        writeln!(
            out,
            "  {name:<46} {:>5}  {:>5} ({:>2}%)  {tr}",
            t.hits, t.adoptions, ratio
        )?;
    }

    let mut by_stage: BTreeMap<u8, Vec<String>> = BTreeMap::new();
    for p in store.patterns()? {
        let order = match p.stage {
            Stage::Regressed => 0,
            Stage::Improving => 1,
            Stage::Mastered => 2,
            _ => continue,
        };
        by_stage.entry(order).or_default().push(p.rule_id);
    }
    for (order, mut rules) in by_stage {
        rules.sort();
        rules.dedup();
        let heading = ["Slipping back", "Getting better", "Mastered"][usize::from(order)];
        writeln!(out, "\n  {heading}")?;
        for rule in rules {
            writeln!(out, "    {}", describe(&rule))?;
        }
    }
    if let Some(line) = typing_line(store, &current, &previous, versus)? {
        writeln!(out, "\n  {line}")?;
    }
    if !now_t.is_empty() {
        writeln!(
            out,
            "\n  ↓ fewer per active hour is better. `flowbuddy memory show` for history."
        )?;
    }
    Ok(out)
}

/// Opt-in typing accuracy for the summary, if there's any.
fn typing_line(
    store: &Store,
    current: &(String, String),
    previous: &(String, String),
    versus: &str,
) -> anyhow::Result<Option<String>> {
    let (typed, corrections) = store.typing(&current.0, &current.1)?;
    if typed == 0 {
        return Ok(None);
    }
    let rate = |t: u64, c: u64| c as f64 * 100.0 / t.max(1) as f64;
    let mut line = format!(
        "Typing  {:.1}% of characters corrected ({corrections} of {typed})",
        rate(typed, corrections)
    );
    let (before_t, before_c) = store.typing(&previous.0, &previous.1)?;
    if before_t > 0 {
        let _ = write!(line, ", {:.1}% {versus}", rate(before_t, before_c));
    }
    let pairs = store.typo_pairs(&current.0, &current.1, 3)?;
    if !pairs.is_empty() {
        let list: Vec<String> = pairs.iter().map(|(p, n)| format!("{p} {n}×")).collect();
        let _ = write!(line, ". Most corrected after: {}", list.join(", "));
    }
    line.push('.');
    Ok(Some(line))
}

fn date(unix_s: u64) -> String {
    days::local_date(unix_s * 1000)
        .format("%-d %b %Y")
        .to_string()
}

pub fn memory_show(store: &Store, now_ms: u64) -> anyhow::Result<String> {
    let week = days::range(now_ms, 7, 0);
    let mut out = String::new();
    let patterns = store.patterns()?;
    if patterns.is_empty() {
        writeln!(out, "The mistake memory is empty.")?;
        return Ok(out);
    }
    for p in patterns {
        let w = store.window(&p.rule_id, &p.context, &week.0, &week.1)?;
        let shapes: Vec<String> = store
            .recent_episodes(&p.rule_id, &p.context, 5)?
            .into_iter()
            .map(|e| e.shape)
            .collect();
        writeln!(out, "{} ({})", p.rule_id, p.context)?;
        writeln!(out, "  {}", describe(&p.rule_id))?;
        writeln!(
            out,
            "  stage      {} since {}",
            p.stage.as_str(),
            date(p.stage_since)
        )?;
        writeln!(
            out,
            "  seen       {} → {}",
            date(p.first_seen),
            date(p.last_seen)
        )?;
        writeln!(
            out,
            "  total      {}, the better way {}",
            times(p.occurrences),
            times(p.adoptions)
        )?;
        writeln!(
            out,
            "  last week  {}, the better way {}",
            times(w.hits),
            times(w.adoptions)
        )?;
        if let Some(e) = w.estimate() {
            let pct = |x: f64| (x * 100.0).round() as u32;
            writeln!(
                out,
                "  progress   the better way {}% of the time (likely {}–{}%)",
                pct(e.ratio),
                pct(e.low),
                pct(e.high)
            )?;
        }
        writeln!(
            out,
            "  nudges     {} shown, {} dismissed, {} not helpful",
            p.nudges, p.dismissed, p.false_positives
        )?;
        if !shapes.is_empty() {
            writeln!(out, "  recently   {}", shapes.join(", "))?;
        }
        writeln!(out)?;
    }
    Ok(out)
}

fn times(n: u64) -> String {
    match n {
        1 => "once".into(),
        n => format!("{n} times"),
    }
}

/// The habit `flowbuddy next` would pick, by id.
pub fn focus(
    store: &Store,
    now_ms: u64,
    skip: &BTreeSet<String>,
) -> anyhow::Result<Option<String>> {
    Ok(ranked(store, now_ms, skip)?
        .into_iter()
        .next()
        .map(|(_, p, _)| p.rule_id))
}

type Ranked = Vec<(
    f64,
    flowbuddy_store::Pattern,
    flowbuddy_core::learning::Window,
)>;

fn ranked(store: &Store, now_ms: u64, skip: &BTreeSet<String>) -> anyhow::Result<Ranked> {
    let week = days::range(now_ms, 7, 0);
    let mut ranked = Vec::new();
    for p in store.patterns()? {
        if skip.contains(&p.rule_id) {
            continue;
        }
        let w = store.window(&p.rule_id, &p.context, &week.0, &week.1)?;
        let score = learning::priority(p.stage, w.hits, p.false_positives);
        if score > 0.0 {
            ranked.push((score, p, w));
        }
    }
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    Ok(ranked)
}

/// `flowbuddy next`: the one habit worth working on now, by
/// [`learning::priority`], skipping rules in `skip` (disabled or muted).
pub fn next(store: &Store, now_ms: u64, skip: &BTreeSet<String>) -> anyhow::Result<String> {
    let engine = Engine::standard();
    let ranked = ranked(store, now_ms, skip)?;
    let mut out = String::new();
    let Some((_, top, w)) = ranked.first() else {
        out.push_str("Nothing to work on: no habits showed up this week.\n");
        return Ok(out);
    };
    let rule = engine.rules().find(|r| r.id() == top.rule_id);
    writeln!(out, "Work on: {} ({})", describe(&top.rule_id), top.context)?;
    if let Some(title) = rule.map(|r| r.title()).filter(|t| !t.is_empty()) {
        writeln!(out, "  {title}.")?;
    }
    writeln!(
        out,
        "  This week: {}, the better way {}. Stage: {}.",
        times(w.hits),
        times(w.adoptions),
        top.stage.as_str()
    )?;
    if let Some((_, second, w2)) = ranked.get(1) {
        writeln!(
            out,
            "  After that: {} ({} this week).",
            describe(&second.rule_id),
            times(w2.hits)
        )?;
    }
    Ok(out)
}

/// A difference of two shares, with its 95% interval (normal
/// approximation): `a` of `n` against `b` of `m`.
pub fn effect(a: u64, n: u64, b: u64, m: u64) -> (f64, f64) {
    let (p, q) = (a as f64 / n.max(1) as f64, b as f64 / m.max(1) as f64);
    let se = (p * (1.0 - p) / n.max(1) as f64 + q * (1.0 - q) / m.max(1) as f64).sqrt();
    (p - q, 1.96 * se)
}

/// `flowbuddy learn --report`: whether tips help (against the ones held
/// back), how well you know each skill, and how challenges went.
pub fn learn_report(store: &Store, now_ms: u64) -> anyhow::Result<String> {
    use flowbuddy_store::Outcome;
    let mut out = String::new();
    // Rule: (shown, adopted after), (held, adopted anyway).
    let mut by: BTreeMap<String, [(u64, u64); 2]> = BTreeMap::new();
    for t in store.tips(0)?.iter().filter(|t| t.outcome.is_some()) {
        let row = &mut by.entry(t.rule_id.clone()).or_default()[usize::from(t.held)];
        row.0 += 1;
        row.1 += u64::from(t.outcome == Some(Outcome::Adopted));
    }
    writeln!(
        out,
        "What tips do: how often you did it the better way soon after a tip, against tips held back at random."
    )?;
    let compared: Vec<_> = by
        .iter()
        .filter(|(_, r)| r[0].0 > 0 && r[1].0 > 0)
        .collect();
    if compared.is_empty() {
        writeln!(
            out,
            "  Nothing to compare yet: a tenth of tips are held back ([learn] holdout)."
        )?;
    }
    for (rule, [(n, a), (m, b)]) in compared {
        let (d, ci) = effect(*a, *n, *b, *m);
        let verdict = if d - ci > 0.0 {
            "helps"
        } else if d + ci < 0.0 {
            "seems to hurt"
        } else {
            "not clear yet"
        };
        writeln!(
            out,
            "  {rule:<24} shown {a}/{n}, held {b}/{m}: {:+.0} points (± {:.0}), {verdict}",
            d * 100.0,
            ci * 100.0
        )?;
    }

    let today = days::day(now_ms);
    let (skills, model) = store.skills(&today, None)?;
    writeln!(
        out,
        "\nYour memory: a skill used on one day is half-forgotten after {:.0} days; after nine days of use, {:.0} days.",
        model.half_life(1.0, 0.0),
        model.half_life(9.0, 0.0)
    )?;
    let mut rows: Vec<_> = skills.iter().collect();
    rows.sort_by(|a, b| b.1.mastery.total_cmp(&a.1.mastery).then(a.0.cmp(b.0)));
    if !rows.is_empty() {
        writeln!(
            out,
            "\n{:<18} {:>8} {:>7} {:>10}",
            "skill", "mastery", "recall", "half-life"
        )?;
    }
    for (id, s) in rows {
        writeln!(
            out,
            "{id:<18} {:>7.0}% {:>6.0}% {:>8.0} d",
            s.mastery * 100.0,
            s.recall(&model) * 100.0,
            model.half_life(s.right, s.wrong)
        )?;
    }

    let challenges = store.challenge_outcomes()?;
    if !challenges.is_empty() {
        let (done, missed, expired) = challenges
            .values()
            .fold((0, 0, 0), |t, c| (t.0 + c.0, t.1 + c.1, t.2 + c.2));
        writeln!(
            out,
            "\nChallenges: {done} done, {missed} missed, {expired} left to expire."
        )?;
    }
    Ok(out)
}

/// `flowbuddy learn --audit`: where your history disagrees with the
/// tree's order, to change it by.
pub fn audit(store: &Store, now_ms: u64) -> anyhow::Result<String> {
    use flowbuddy_core::curriculum::{CURRICULUM, requires};
    use flowbuddy_store::days::day;
    let first = store.item_first_used()?;
    let today = day(now_ms);
    let weeks_ago = day(now_ms.saturating_sub(21 * 86_400_000));
    let mut out = String::new();
    // Picked up before what the tree says it builds on.
    let early: Vec<String> = CURRICULUM
        .iter()
        .filter_map(|i| {
            let used = first.get(i.id)?;
            let before: Vec<&str> = requires(i.id)
                .iter()
                .filter(|r| first.get(**r).is_none_or(|d| d > used))
                .copied()
                .collect();
            (!before.is_empty()).then(|| format!("{} before {}", i.id, before.join(" and ")))
        })
        .collect();
    // Ready for weeks (everything it builds on used three weeks ago), unused.
    let stale: Vec<&str> = CURRICULUM
        .iter()
        .filter(|i| {
            let reqs = requires(i.id);
            !first.contains_key(i.id)
                && !reqs.is_empty()
                && reqs
                    .iter()
                    .all(|r| first.get(*r).is_some_and(|d| *d <= weeks_ago))
        })
        .map(|i| i.id)
        .collect();
    // Shown as the daily tip a week or more ago, never taken up.
    let week = (now_ms / 1000).saturating_sub(7 * 86_400);
    let ignored: Vec<String> = store
        .learn_items()?
        .into_iter()
        .filter(|r| r.learned_at.is_none() && r.last_shown <= week)
        .map(|r| r.item_id)
        .collect();
    let section = |out: &mut String, title: &str, items: &[String]| {
        out.push_str(&format!("{title} ({}):\n", items.len()));
        if items.is_empty() {
            out.push_str("  none\n");
        }
        for i in items {
            out.push_str(&format!("  {i}\n"));
        }
        out.push('\n');
    };
    out.push_str(&format!(
        "Your history against the tree's order, up to {today}.\n\n"
    ));
    section(
        &mut out,
        "Used before what they build on: maybe they don't need it",
        &early,
    );
    section(
        &mut out,
        "Ready for three weeks and not used: maybe too far ahead, or not useful to you",
        &stale.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    );
    section(&mut out, "Daily tips never taken up", &ignored);
    // How each link with evidence stands.
    let links: Vec<String> = store
        .links()?
        .into_iter()
        .filter(|l| l.for_ + l.against > 0)
        .map(|l| {
            let verdict = if l.holds() { "kept" } else { "dropped" };
            format!(
                "{} builds on {}: {} for, {} against, {:.0}% likely, {verdict}",
                l.item,
                l.on,
                l.for_,
                l.against,
                l.strength() * 100.0
            )
        })
        .collect();
    section(
        &mut out,
        "Links, from the order you learned things in each place (dropped ones no longer hold lessons back)",
        &links,
    );
    Ok(out)
}

fn place_name(ctx: &str) -> &str {
    match ctx {
        "vscode-vim" => "VS Code",
        "terminal-vim" => "Vim",
        "claude-vim" => "Claude Code",
        "bash-vi" => "the shell",
        other => other,
    }
}

/// `flowbuddy learn`: the latest "something new", progress through the
/// curriculum, and what's next.
/// What's recorded about your use of the curriculum, for the skill tree.
struct Recorded {
    uses: std::collections::HashMap<String, flowbuddy_store::ItemUse>,
    skills: std::collections::HashMap<String, flowbuddy_core::mastery::Skill>,
    model: flowbuddy_core::mastery::Model,
    rows: BTreeMap<String, flowbuddy_store::LearnRow>,
}

impl Recorded {
    fn read(store: &Store, now_ms: u64) -> anyhow::Result<Recorded> {
        let today = flowbuddy_store::days::day(now_ms);
        let recent_from = flowbuddy_store::days::day(
            now_ms.saturating_sub(flowbuddy_core::curriculum::RECENT_DAYS * 86_400_000),
        );
        let (skills, model) = store.skills(&today, None)?;
        Ok(Recorded {
            uses: store.item_uses(&today, &recent_from, None)?,
            skills,
            model,
            rows: store
                .learn_items()?
                .into_iter()
                .map(|r| (r.item_id.clone(), r))
                .collect(),
        })
    }

    fn progress(&self, id: &str) -> flowbuddy_core::curriculum::Progress {
        let row = self.rows.get(id);
        let u = self.uses.get(id).copied().unwrap_or_default();
        let skill = self.skills.get(id).map(|s| s.percents(&self.model));
        let half_life = self.skills.get(id).map(|s| s.half_life_days(&self.model));
        flowbuddy_core::curriculum::Progress {
            uses: u.total,
            recent_uses: u.recent,
            days_idle: self.uses.contains_key(id).then_some(u.days_idle),
            times_shown: row.map_or(0, |r| r.times_shown),
            last_shown: row.map(|r| r.last_shown),
            learned: row.is_some_and(|r| r.learned_at.is_some()),
            mastery: skill.map(|s| s.0),
            recall: skill.map(|s| s.1),
            half_life,
        }
    }

    /// What you know, as the daemon judges it.
    fn tree(
        &self,
        store: &Store,
        known_levels: u8,
    ) -> anyhow::Result<flowbuddy_core::curriculum::Known> {
        let stages = store.stages()?;
        let mastered = stages
            .iter()
            .filter(|(_, s)| **s == flowbuddy_core::learning::Stage::Mastered)
            .map(|((rule, _), _)| rule.as_str());
        let overrides = store.known_overrides()?;
        let unlinked = flowbuddy_core::links::dropped(&store.links()?);
        Ok(
            flowbuddy_core::curriculum::Known::from_progress(&|id| self.progress(id), mastered)
                .with_levels(known_levels)
                .with_overrides(&overrides)
                .with_links(&unlinked),
        )
    }
}

/// `flowbuddy learn --json`: every item with where you are with it.
pub fn learn_json(
    store: &Store,
    now_ms: u64,
    known_levels: u8,
) -> anyhow::Result<serde_json::Value> {
    use flowbuddy_core::curriculum::{self, CURRICULUM};
    let rec = Recorded::read(store, now_ms)?;
    let tree = rec.tree(store, known_levels)?;
    let items: Vec<serde_json::Value> = CURRICULUM
        .iter()
        .map(|i| {
            let p = rec.progress(i.id);
            let (known, ready) = (tree.knows(i.id), tree.ready(i.id));
            let state = if known {
                "known"
            } else if ready {
                "ready"
            } else {
                "later"
            };
            serde_json::json!({
                "id": i.id,
                "level": i.level,
                "keys": flowbuddy_core::glossary::plain(i.keys),
                "what": i.what,
                "state": state,
                "tile": tile(&p, known, ready),
                "uses": p.uses,
                "days_idle": p.days_idle,
                "half_life_days": p.half_life,
                "mastery": p.mastery,
                "recall": p.recall,
            })
        })
        .collect();
    let latest = rec
        .rows
        .values()
        .max_by_key(|r| r.last_shown)
        .and_then(|r| curriculum::find(&r.item_id).map(|i| (i, r.learned_at.is_some())))
        .map(|(i, learned)| serde_json::json!({"id": i.id, "text": flowbuddy_core::glossary::plain(&curriculum::tip_text(i)), "example": curriculum::demo(i.id).map(flowbuddy_core::glossary::plain), "learned": learned}));
    Ok(serde_json::json!({ "version": 1, "latest": latest, "items": items }))
}

/// A skill on the map: `known`, `slipping` (known, but you may not recall
/// it now), `learning` (used, not known yet), `ready` or `later`.
pub fn tile(p: &flowbuddy_core::curriculum::Progress, known: bool, ready: bool) -> &'static str {
    match (known, p.recall) {
        (true, Some(r)) if r < 50 => "slipping",
        (true, _) => "known",
        _ if p.uses > 0 => "learning",
        _ if ready => "ready",
        _ => "later",
    }
}

/// The level names, as everywhere else.
pub fn level_name(level: u8) -> &'static str {
    match level {
        1 => "Basics",
        2 => "Bigger moves",
        3 => "Power tools",
        _ => "Advanced",
    }
}

/// `learn --map`: each skill as a tile, coloured by where you are with it,
/// a row of tiles per level, wrapped at `width`. Without colour, a mark
/// before the keys says the same.
pub fn skill_map(json: &serde_json::Value, color: bool, width: usize) -> String {
    let items = json["items"].as_array().cloned().unwrap_or_default();
    let mut out = String::new();
    let paint = |tile: &str, keys: &str| -> (String, usize) {
        let shown = format!(" {keys} ");
        let len = shown.chars().count();
        if color {
            let code = match tile {
                "known" => "30;42",
                "slipping" => "30;43",
                "learning" => "30;46",
                "ready" => "1;7",
                _ => "2",
            };
            (format!("\x1b[{code}m{shown}\x1b[0m"), len)
        } else {
            let mark = match tile {
                "known" => '+',
                "slipping" => '!',
                "learning" => '~',
                "ready" => '>',
                _ => '.',
            };
            (format!("{mark}{}", shown.trim_end()), len)
        }
    };
    for level in 1..=4u64 {
        let tiles: Vec<&serde_json::Value> = items
            .iter()
            .filter(|i| i["level"].as_u64() == Some(level))
            .collect();
        if tiles.is_empty() {
            continue;
        }
        let known = tiles.iter().filter(|i| i["tile"] == "known").count();
        let _ = writeln!(
            out,
            "{} ({known} of {})",
            level_name(level as u8),
            tiles.len()
        );
        // Colour sets tiles apart; marks need more room.
        let gap = if color { " " } else { "   " };
        let mut line = String::from("  ");
        let mut used = 2;
        for t in tiles {
            let (s, len) = paint(
                t["tile"].as_str().unwrap_or("later"),
                t["keys"].as_str().unwrap_or("?"),
            );
            if used + len + gap.len() > width && used > 2 {
                let _ = writeln!(out, "{}", line.trim_end());
                line = String::from("  ");
                used = 2;
            }
            line.push_str(&s);
            line.push_str(gap);
            used += len + gap.len();
        }
        let _ = writeln!(out, "{}\n", line.trim_end());
    }
    let legend = if color {
        format!(
            "{} you use it  {} slipping: not used lately  {} learning  {} ready to learn  {} later",
            paint("known", "w").0,
            paint("slipping", "w").0,
            paint("learning", "w").0,
            paint("ready", "w").0,
            paint("later", "w").0
        )
    } else {
        "+ you use it  ! slipping: not used lately  ~ learning  > ready to learn  . later".into()
    };
    let _ = writeln!(out, "{legend}");
    out
}

pub fn learn(store: &Store, now_ms: u64, known_levels: u8) -> anyhow::Result<String> {
    use flowbuddy_core::curriculum::{self, CURRICULUM, Known, Progress};
    use flowbuddy_core::glossary::plain;
    let today = flowbuddy_store::days::day(now_ms);
    let recent_from =
        flowbuddy_store::days::day(now_ms.saturating_sub(curriculum::RECENT_DAYS * 86_400_000));
    let rec = Recorded::read(store, now_ms)?;
    let rows = &rec.rows;
    let progress = |id: &str| rec.progress(id);
    let tree = rec.tree(store, known_levels)?;
    let known = |id: &str| tree.knows(id);
    let mut out = String::new();
    if let Some(row) = rows.values().max_by_key(|r| r.last_shown)
        && let Some(item) = curriculum::find(&row.item_id)
    {
        let status = if row.learned_at.is_some() {
            "learned"
        } else {
            "not used yet"
        };
        writeln!(
            out,
            "Latest: {} ({status})",
            plain(&curriculum::tip_text(item))
        )?;
        if let Some(demo) = curriculum::demo(item.id) {
            writeln!(out, "  Example: {}", plain(demo))?;
        }
    }
    writeln!(
        out,
        "\nProgress, from basic to advanced (█ you use it, ○ ready to learn, · later):"
    )?;
    for level in 1..=4 {
        let items: Vec<_> = CURRICULUM.iter().filter(|i| i.level == level).collect();
        let done = items.iter().filter(|i| known(i.id)).count();
        let bar: String = items
            .iter()
            .map(|i| {
                if known(i.id) {
                    '█'
                } else if tree.ready(i.id) {
                    '○'
                } else {
                    '·'
                }
            })
            .collect();
        writeln!(out, "  level {level}  {bar}  {done} of {}", items.len())?;
    }
    // Each environment keeps its own progress.
    let mut places = Vec::new();
    for ctx in store.use_contexts()? {
        let here = store.item_uses(&today, &recent_from, Some(&ctx))?;
        let (skills_here, model_here) = store.skills(&today, Some(&ctx))?;
        let progress_here = |id: &str| {
            let u = here.get(id).copied().unwrap_or_default();
            let skill = skills_here.get(id).map(|s| s.percents(&model_here));
            Progress {
                uses: u.total,
                recent_uses: u.recent,
                days_idle: here.contains_key(id).then_some(u.days_idle),
                mastery: skill.map(|s| s.0),
                recall: skill.map(|s| s.1),
                ..progress(id)
            }
        };
        let k = Known::from_progress(&progress_here, std::iter::empty()).with_levels(known_levels);
        let n = CURRICULUM.iter().filter(|i| k.knows(i.id)).count();
        places.push(format!("{} {n}", place_name(&ctx)));
    }
    if places.len() > 1 {
        writeln!(out, "  known by place: {}", places.join(", "))?;
    }
    let ready: Vec<String> = CURRICULUM
        .iter()
        .filter(|i| !known(i.id) && tree.ready(i.id))
        .take(6)
        .map(|i| plain(i.keys))
        .collect();
    if !ready.is_empty() {
        writeln!(out, "\nReady to learn: {}", ready.join("; "))?;
    }
    let waiting: Vec<String> = CURRICULUM
        .iter()
        .filter(|i| !tree.ready(i.id))
        .take(4)
        .map(|i| {
            let first: Vec<String> = curriculum::requires(i.id)
                .iter()
                .filter(|r| !known(r))
                .filter_map(|r| curriculum::find(r))
                .map(|r| plain(r.keys))
                .collect();
            format!("{} after {}", plain(i.keys), first.join(" and "))
        })
        .collect();
    if !waiting.is_empty() {
        writeln!(out, "Later: {}", waiting.join("; "))?;
    }
    let learned: Vec<String> = rows
        .values()
        .filter(|r| r.learned_at.is_some())
        .filter_map(|r| curriculum::find(&r.item_id))
        .map(|i| plain(i.keys))
        .collect();
    if !learned.is_empty() {
        writeln!(out, "\nLearned from daily tips: {}", learned.join("; "))?;
    }
    match curriculum::pick(&progress, &tree, now_ms / 1000) {
        Some(next) => writeln!(out, "\nUp next: {}", plain(&curriculum::tip_text(next)))?,
        None => writeln!(out, "\nNothing new left for now.")?,
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use flowbuddy_store::Event;

    fn at(d: u32, h: u32) -> u64 {
        chrono::Local
            .with_ymd_and_hms(2026, 10, d, h, 0, 0)
            .unwrap()
            .timestamp_millis() as u64
    }

    fn sample() -> Store {
        let s = Store::open_in_memory().unwrap();
        let hit = Event::Hit {
            mode: "normal",
            shape: "j×7",
        };
        for (d, hits, adopts) in [(23, 6, 1), (24, 2, 4)] {
            let day = days::day(at(d, 12));
            for _ in 0..60 {
                s.add_active_minute(&day, "vscode-vim").unwrap();
            }
            for _ in 0..hits {
                s.record(&day, "vscode-vim", "vim.repeat-jk", hit, at(d, 12) / 1000)
                    .unwrap();
            }
            for _ in 0..adopts {
                s.record(
                    &day,
                    "vscode-vim",
                    "vim.repeat-jk",
                    Event::Adoption,
                    at(d, 12) / 1000,
                )
                .unwrap();
            }
        }
        s
    }

    #[test]
    fn summary_today_vs_yesterday() {
        let text = summary(&sample(), at(24, 18), Period::Today).unwrap();
        assert!(text.contains("compared with yesterday"), "{text}");
        assert!(text.contains("Active  1h"), "{text}");
        assert!(text.contains("Pressing j or k"), "{text}");
        assert!(text.contains("↓ 67%"), "6/h → 2/h is 67% fewer:\n{text}");
        assert!(text.contains("4 (66%)"), "{text}");
    }

    #[test]
    fn summary_with_typing_accuracy() {
        let s = sample();
        s.add_typing(&days::day(at(24, 12)), "vscode-vim", 400, 12)
            .unwrap();
        s.add_typing(&days::day(at(23, 12)), "vscode-vim", 200, 10)
            .unwrap();
        s.add_typo_pair(&days::day(at(24, 12)), "er", 4).unwrap();
        let out = summary(&s, at(24, 18), Period::Today).unwrap();
        assert!(
            out.contains("Typing  3.0% of characters corrected (12 of 400), 5.0% yesterday. Most corrected after: er 4×."),
            "{out}"
        );
    }

    #[test]
    fn summary_of_nothing() {
        let s = Store::open_in_memory().unwrap();
        let text = summary(&s, at(24, 18), Period::Week).unwrap();
        assert!(text.contains("Nothing recorded yet"));
    }

    #[test]
    fn memory_show_lists_patterns() {
        let text = memory_show(&sample(), at(24, 18)).unwrap();
        assert!(text.contains("vim.repeat-jk (vscode-vim)"), "{text}");
        assert!(
            text.contains("total      8 times, the better way 5 times"),
            "{text}"
        );
        assert!(text.contains("recently   j×7"), "{text}");
    }

    #[test]
    fn trends() {
        assert_eq!(trend(2.0, 6.0), "↓ 67%");
        assert_eq!(trend(6.0, 2.0), "↑ 200%");
        assert_eq!(trend(2.0, 2.05), "≈ same");
        assert_eq!(trend(1.0, 0.0), "new");
    }

    #[test]
    fn next_picks_the_most_pressing_habit() {
        let s = sample();
        let x = Event::Hit {
            mode: "normal",
            shape: "x×5",
        };
        let day = days::day(at(24, 12));
        s.record(&day, "vscode-vim", "vim.repeat-x", x, at(24, 12) / 1000)
            .unwrap();
        let out = next(&s, at(24, 18), &BTreeSet::new()).unwrap();
        assert!(out.starts_with("Work on: Pressing j or k"), "{out}");
        assert!(out.contains("Try a count."), "{out}");
        assert!(
            out.contains("This week: 8 times, the better way 5 times."),
            "{out}"
        );
        assert!(
            out.contains("After that: Deleting character by character"),
            "{out}"
        );
        // A muted rule is skipped.
        let skip = BTreeSet::from(["vim.repeat-jk".to_string()]);
        assert!(
            next(&s, at(24, 18), &skip)
                .unwrap()
                .starts_with("Work on: Deleting")
        );
    }

    #[test]
    fn next_with_nothing_recorded() {
        let s = Store::open_in_memory().unwrap();
        assert!(
            next(&s, at(24, 18), &BTreeSet::new())
                .unwrap()
                .starts_with("Nothing to work on")
        );
    }

    #[test]
    fn learn_progress() {
        let s = Store::open_in_memory().unwrap();
        let day = days::day(at(24, 12));
        s.add_command_use(&day, "terminal-vim", "word", 9).unwrap();
        s.learn_shown("word-end", at(24, 12) / 1000).unwrap();
        s.learn_learned("word-end", at(24, 13) / 1000).unwrap();
        let audit_text = audit(&s, at(24, 18)).unwrap();
        assert!(
            audit_text.contains("Daily tips never taken up (0)"),
            "{audit_text}"
        );
        let out = learn(&s, at(24, 18), 0).unwrap();
        assert!(
            out.starts_with("Latest: e: jump to the end of the word"),
            "{out}"
        );
        assert!(out.contains("(learned)"), "{out}");
        let level1 = flowbuddy_core::curriculum::CURRICULUM
            .iter()
            .filter(|i| i.level == 1)
            .count();
        let bar = format!("level 1  ██{}  2 of {level1}", "○".repeat(level1 - 2));
        assert!(out.contains(&bar), "{out}");
        assert!(
            out.contains("Later: # after *; n after / and a few letters"),
            "{out}"
        );
        assert!(out.contains("Up next: $: the end of the line"), "{out}");
    }

    #[test]
    fn audit_finds_disagreements() {
        let s = Store::open_in_memory().unwrap();
        // ci( long before ciw; find a month ago and till never since.
        s.add_command_use("2026-09-01", "terminal-vim", "ci-paren", 4)
            .unwrap();
        s.add_command_use("2026-10-20", "terminal-vim", "ciw", 1)
            .unwrap();
        s.add_command_use("2026-09-20", "terminal-vim", "find", 9)
            .unwrap();
        s.learn_shown("marks", at(10, 10) / 1000).unwrap();
        let out = audit(&s, at(24, 18)).unwrap();
        assert!(out.contains("  ci-paren before ciw\n"), "{out}");
        assert!(out.contains("  till\n"), "{out}");
        assert!(
            out.contains("Daily tips never taken up (1):\n  marks\n"),
            "{out}"
        );
    }

    #[test]
    fn the_learning_report_compares_against_held_tips() {
        use flowbuddy_store::{Outcome, TipShown};
        let s = Store::open_in_memory().unwrap();
        for i in 0..40u64 {
            let held = i % 2 == 1;
            let id = s
                .tip_shown(&TipShown {
                    shown_at: i,
                    rule_id: "vim.repeat-jk",
                    context: "vscode-vim",
                    stage: "nudged",
                    hour: 10,
                    mode: "active",
                    prior_today: 0,
                    held,
                })
                .unwrap();
            // Shown: adopted 18 of 20; held: 4 of 20.
            let adopted = if held { i % 10 == 1 } else { i % 20 != 0 };
            let outcome = if adopted {
                Outcome::Adopted
            } else {
                Outcome::Ignored
            };
            s.close_tip(id, outcome).unwrap();
        }
        s.add_command_use(&days::day(at(25, 12)), "vscode-vim", "count", 3)
            .unwrap();
        s.challenge_ended("count", 0, &days::day(at(25, 12)), "done")
            .unwrap();
        let r = learn_report(&s, at(26, 12)).unwrap();
        assert!(r.contains("shown 18/20, held 4/20: +70 points"), "{r}");
        assert!(r.contains("helps"), "{r}");
        assert!(r.contains("count "), "{r}");
        assert!(r.contains("Challenges: 1 done, 0 missed"), "{r}");
        let (d, ci) = effect(5, 10, 5, 10);
        assert!(d == 0.0 && ci > 0.3);
    }

    #[test]
    fn learn_as_json() {
        let s = Store::open_in_memory().unwrap();
        s.add_command_use(&days::day(at(25, 12)), "vscode-vim", "word", 5)
            .unwrap();
        let v = learn_json(&s, at(26, 12), 0).unwrap();
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.len(), flowbuddy_core::curriculum::CURRICULUM.len());
        let word = items.iter().find(|i| i["id"] == "word").unwrap();
        assert_eq!(word["state"], "known");
        assert_eq!(word["uses"], 5);
        assert!(items.iter().any(|i| i["state"] == "ready"));
        assert!(items.iter().any(|i| i["state"] == "later"));
    }

    #[test]
    fn skill_map_tiles() {
        use flowbuddy_core::curriculum::Progress;
        let p = |uses, recall| Progress {
            uses,
            recall,
            ..Default::default()
        };
        assert_eq!(tile(&p(9, Some(80)), true, true), "known");
        assert_eq!(tile(&p(9, Some(20)), true, true), "slipping");
        assert_eq!(tile(&p(2, None), false, true), "learning");
        assert_eq!(tile(&p(0, None), false, true), "ready");
        assert_eq!(tile(&p(0, None), false, false), "later");
        let item = |keys: &str, level: u8, tile: &str| serde_json::json!({"keys": keys, "level": level, "tile": tile});
        let json = serde_json::json!({"items": [
            item("w", 1, "known"), item("b", 1, "slipping"), item("e", 1, "ready"),
            item("}", 2, "later"), item("ciw", 2, "learning"),
        ]});
        assert_eq!(
            skill_map(&json, false, 80),
            "Basics (1 of 3)\n  + w   ! b   > e\n\nBigger moves (0 of 2)\n  . }   ~ ciw\n\n+ you use it  ! slipping: not used lately  ~ learning  > ready to learn  . later\n"
        );
        // Wrapped to the width.
        assert!(skill_map(&json, false, 8).starts_with("Basics (1 of 3)\n  + w\n  ! b\n  > e\n"));
        assert!(skill_map(&json, true, 80).contains("\x1b[30;42m w \x1b[0m"));
    }
}
