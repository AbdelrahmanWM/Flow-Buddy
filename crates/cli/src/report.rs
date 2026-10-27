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

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n - 1).collect();
        t.push('…');
        t
    }
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
        .map(|(c, m)| format!("{c} {}", hm(*m)))
        .collect();
    writeln!(
        out,
        "  Active  {}  ({})\n",
        hm(active_total),
        per_ctx.join(", ")
    )?;

    let mut rows: Vec<(&String, &Totals)> = now_t.iter().collect();
    rows.sort_by(|a, b| b.1.hits.cmp(&a.1.hits).then(a.0.cmp(b.0)));
    writeln!(
        out,
        "  {:<46} {:>5}  {:>11}  Trend",
        "Habit", "Times", "Better way"
    )?;
    let per_hour = |hits: u64, minutes: u64| hits as f64 * 60.0 / minutes.max(1) as f64;
    for (rule, t) in rows {
        let ratio = t.adoptions * 100 / (t.hits + t.adoptions).max(1);
        let before = prev_t.get(rule).copied().unwrap_or_default();
        let tr = trend(
            per_hour(t.hits, active_total),
            per_hour(before.hits, active_prev),
        );
        writeln!(
            out,
            "  {:<46} {:>5}  {:>5} ({:>2}%)  {tr}",
            truncate(&describe(rule), 46),
            t.hits,
            t.adoptions,
            ratio
        )?;
    }

    let mut by_stage: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for p in store.patterns()? {
        if matches!(
            p.stage,
            Stage::Mastered | Stage::Improving | Stage::Regressed
        ) {
            by_stage
                .entry(p.stage.as_str())
                .or_default()
                .push(p.rule_id);
        }
    }
    if !by_stage.is_empty() {
        writeln!(out)?;
        for (stage, mut rules) in by_stage {
            rules.dedup();
            writeln!(out, "  {stage:<10} {}", rules.join(", "))?;
        }
    }
    if let Some(line) = typing_line(store, &current, &previous, versus)? {
        writeln!(out, "\n  {line}")?;
    }
    writeln!(
        out,
        "\n  ↓ fewer per active hour is better. `flowbuddy memory show` for history."
    )?;
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
            "  total      {} times, better way {} times",
            p.occurrences, p.adoptions
        )?;
        writeln!(
            out,
            "  last week  {} times, better way {} times",
            w.hits, w.adoptions
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

/// `flowbuddy learn`: the latest "something new", progress through the
/// curriculum, and what's next.
pub fn learn(store: &Store, now_ms: u64) -> anyhow::Result<String> {
    use flowbuddy_core::curriculum::{self, CURRICULUM, Known, Progress};
    use flowbuddy_core::glossary::plain;
    let uses = store.command_uses("0000-00-00")?;
    let rows: BTreeMap<String, flowbuddy_store::LearnRow> = store
        .learn_items()?
        .into_iter()
        .map(|r| (r.item_id.clone(), r))
        .collect();
    let progress = |id: &str| {
        let row = rows.get(id);
        Progress {
            uses: uses.get(id).copied().unwrap_or(0),
            times_shown: row.map_or(0, |r| r.times_shown),
            last_shown: row.map(|r| r.last_shown),
            learned: row.is_some_and(|r| r.learned_at.is_some()),
        }
    };
    let stages = store.stages()?;
    let mastered = stages
        .iter()
        .filter(|(_, s)| **s == flowbuddy_core::learning::Stage::Mastered)
        .map(|((rule, _), _)| rule.as_str());
    let tree = Known::from_progress(&progress, mastered);
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
            text.contains("total      8 times, better way 5 times"),
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
        s.add_command_use(&day, "word", 9).unwrap();
        s.learn_shown("word-end", at(24, 12) / 1000).unwrap();
        s.learn_learned("word-end", at(24, 13) / 1000).unwrap();
        let out = learn(&s, at(24, 18)).unwrap();
        assert!(
            out.starts_with("Latest: e: jump to the end of the word"),
            "{out}"
        );
        assert!(out.contains("(learned)"), "{out}");
        assert!(out.contains("level 1  ██○○○○○○○○○○  2 of 12"), "{out}");
        assert!(out.contains("ci( after ciw"), "{out}");
        assert!(out.contains("Up next: 0, ^ and $"), "{out}");
    }
}
