//! `flowbuddy summary` and `flowbuddy memory show`: read the database
//! directly, so they work even when the daemon isn't running.

use std::collections::BTreeMap;
use std::fmt::Write;

use flowbuddy_core::engine::Engine;
use flowbuddy_core::learning::Stage;
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
    writeln!(
        out,
        "\n  ↓ fewer per active hour is better. `flowbuddy memory show` for history."
    )?;
    Ok(out)
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
}
