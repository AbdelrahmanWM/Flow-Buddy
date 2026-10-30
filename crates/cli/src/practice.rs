//! `flowbuddy practice`: a short drill on your weakest habit, in Vim. The
//! drill is written to a file, Vim runs it (the plugin's
//! `flowbuddy#practice#run`), and how it went is kept in
//! `~/.local/state/flowbuddy/practice.json`: your scores, never any text.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use flowbuddy_core::practice::{self, Drill, Family, RoundResult, Session};

/// Sessions kept in the history.
const KEEP: usize = 200;

/// Which drill, and why: the most pressing habit that has one, unless
/// that was the last drill; then the one not done for the longest, so
/// running it again gives something different.
pub fn pick(
    patterns: &[flowbuddy_store::PatternExport],
    sessions: &[Session],
) -> (Family, Option<String>) {
    let habit = patterns
        .iter()
        .filter(|p| p.stage != flowbuddy_core::learning::Stage::Mastered)
        .find_map(|p| practice::family_for(&p.rule).map(|f| (f, p.description.clone())));
    let Some(last) = sessions.last().map(|s| s.family) else {
        return habit.map_or((Family::Vertical, None), |(f, why)| (f, Some(why)));
    };
    if let Some((f, why)) = habit.filter(|(f, _)| *f != last) {
        return (f, Some(why));
    }
    let done = |f: Family| sessions.iter().rev().position(|s| s.family == f);
    let other = Family::ALL
        .into_iter()
        .filter(|f| *f != last)
        // Never done first, then the longest ago.
        .min_by_key(|f| std::cmp::Reverse(done(*f).unwrap_or(usize::MAX)))
        .unwrap_or(last);
    (
        other,
        Some(format!(
            "something different from last time ({})",
            last.title().to_lowercase()
        )),
    )
}

/// The other drills, by name, for the end of a report.
pub fn others(family: Family) -> String {
    let names: Vec<String> = Family::ALL
        .into_iter()
        .filter(|f| *f != family)
        .map(|f| format!("{} ({})", f.name(), f.title().to_lowercase()))
        .collect();
    format!(
        "Other drills: {}. Run `flowbuddy practice <name>`, or `:FlowBuddyPractice <name>` in Vim; with no name, the next one is picked for you.",
        names.join(", ")
    )
}

pub fn history_file() -> PathBuf {
    flowbuddy_core::paths::state_file().with_file_name("practice.json")
}

pub fn load(path: &Path) -> Vec<Session> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, sessions: &[Session]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let from = sessions.len().saturating_sub(KEEP);
    let text = serde_json::to_string_pretty(&sessions[from..]).map_err(std::io::Error::other)?;
    std::fs::write(path, text)
}

/// What the plugin writes when the drill ends.
#[derive(serde::Deserialize)]
pub struct Outcome {
    pub rounds: Vec<RoundResult>,
    #[serde(default)]
    pub stopped: bool,
}

/// The command that runs `drill` in `editor`, writing to `out`.
pub fn editor_command(editor: &str, drill: &Path, out: &Path) -> Vec<String> {
    let quote = |p: &Path| p.display().to_string().replace('\'', "''");
    vec![
        editor.to_string(),
        "-c".into(),
        format!(
            "call flowbuddy#practice#run('{}', '{}')",
            quote(drill),
            quote(out)
        ),
    ]
}

/// The report after a drill, against the last one of the same kind.
pub fn report(session: &Session, before: Option<&Session>, stopped: bool, now: u64) -> String {
    let mut out = String::new();
    let rounds = session.rounds.len();
    let _ = writeln!(
        out,
        "{}: {} round{} in {:.0} s, {} in the fewest keys.",
        session.family.title(),
        rounds,
        if rounds == 1 { "" } else { "s" },
        session.seconds(),
        session.perfect()
    );
    let keys: u32 = session.rounds.iter().map(|r| r.keys).sum();
    let par: u32 = session.rounds.iter().map(|r| r.par).sum();
    let _ = writeln!(
        out,
        "You used {keys} keys; the fewest was {par}: {:.0}%.",
        session.efficiency() * 100.0
    );
    if let Some(b) = before {
        let diff = (session.efficiency() - b.efficiency()) * 100.0;
        let change = if diff.abs() < 1.0 {
            "about the same".to_string()
        } else if diff > 0.0 {
            format!("{diff:.0} points better")
        } else {
            format!("{:.0} points lower", -diff)
        };
        let _ = writeln!(
            out,
            "Last time ({}): {:.0}% in {:.0} s. This time: {change}.",
            ago(b.at, now),
            b.efficiency() * 100.0,
            b.seconds()
        );
    }
    let missed: Vec<&RoundResult> = session.rounds.iter().filter(|r| r.keys > r.par).collect();
    if !missed.is_empty() {
        let _ = writeln!(out, "\nWhere fewer keys would do:");
        for r in missed {
            let _ = writeln!(out, "  {} keys; {} does it in {}", r.keys, r.best, r.par);
        }
    }
    if stopped {
        let _ = writeln!(out, "\nStopped early; the rounds done are kept.");
    }
    let _ = writeln!(out, "\n{}", others(session.family));
    out
}

/// "Tue 14:02", or "today 14:02".
fn ago(at: u64, now: u64) -> String {
    use chrono::{Local, TimeZone};
    let (Some(t), Some(n)) = (
        Local.timestamp_opt(at as i64, 0).single(),
        Local.timestamp_opt(now as i64, 0).single(),
    ) else {
        return String::new();
    };
    if t.date_naive() == n.date_naive() {
        t.format("today %H:%M").to_string()
    } else if n - t < chrono::Duration::days(6) {
        t.format("%a %H:%M").to_string()
    } else {
        t.format("%-d %b").to_string()
    }
}

/// `--history`, and the practice part of `learn --report`: each kind of
/// drill, how many times, and the first score against the latest.
pub fn history(sessions: &[Session]) -> String {
    let mut out = String::new();
    for f in Family::ALL {
        let mine: Vec<&Session> = sessions.iter().filter(|s| s.family == f).collect();
        let (Some(first), Some(last)) = (mine.first(), mine.last()) else {
            continue;
        };
        if mine.len() == 1 {
            let _ = writeln!(
                out,
                "  {}: once, {:.0}%",
                f.title(),
                first.efficiency() * 100.0
            );
        } else {
            let _ = writeln!(
                out,
                "  {}: {} times, from {:.0}% to {:.0}% ({:.0} s to {:.0} s)",
                f.title(),
                mine.len(),
                first.efficiency() * 100.0,
                last.efficiency() * 100.0,
                first.seconds(),
                last.seconds()
            );
        }
    }
    out
}

/// A seed that changes each time, so drills differ.
pub fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
}

pub fn write_drill(dir: &Path, drill: &Drill) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("drill.json");
    std::fs::write(
        &path,
        serde_json::to_string(drill).map_err(std::io::Error::other)?,
    )?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(at: u64, rounds: &[(u32, u32)]) -> Session {
        Session {
            at,
            family: Family::Vertical,
            rounds: rounds
                .iter()
                .map(|&(keys, par)| RoundResult {
                    keys,
                    par,
                    ms: 5000,
                    best: "5j".into(),
                })
                .collect(),
        }
    }

    #[test]
    fn the_report_compares_with_last_time() {
        let before = session(1_000, &[(4, 2), (4, 2)]);
        let now = session(2_000, &[(2, 2), (3, 2)]);
        let text = report(&now, Some(&before), false, 2_000);
        assert!(
            text.starts_with("Moving up and down: 2 rounds in 10 s, 1 in the fewest keys.\n"),
            "{text}"
        );
        assert!(
            text.contains("You used 5 keys; the fewest was 4: 80%."),
            "{text}"
        );
        assert!(
            text.contains(": 50% in 10 s. This time: 30 points better."),
            "{text}"
        );
        assert!(text.contains("  3 keys; 5j does it in 2"), "{text}");
        assert!(!text.contains("Stopped"));
    }

    #[test]
    fn running_it_again_gives_another_drill() {
        let s = |f| Session {
            family: f,
            ..session(1, &[(2, 2)])
        };
        // No habit to go on, nothing done: up and down.
        assert_eq!(pick(&[], &[]).0, Family::Vertical);
        // Just done: the drill never done, then the one done longest ago.
        let (f, why) = pick(&[], &[s(Family::Vertical)]);
        assert_eq!(f, Family::Horizontal);
        assert_eq!(
            why.as_deref(),
            Some("something different from last time (moving up and down)")
        );
        let done = [
            s(Family::Delete),
            s(Family::Horizontal),
            s(Family::Inside),
            s(Family::Vertical),
        ];
        assert_eq!(pick(&[], &done).0, Family::Delete);
        // With no habit, round them all.
        assert_eq!(pick(&[], &[s(Family::Inside)]).0, Family::Vertical);
        assert_eq!(
            pick(&[], &[s(Family::Vertical), s(Family::Horizontal)]).0,
            Family::Delete
        );
    }

    #[test]
    fn history_by_drill() {
        let s = [session(1, &[(4, 2)]), session(2, &[(2, 2)])];
        assert_eq!(
            history(&s),
            "  Moving up and down: 2 times, from 50% to 100% (5 s to 5 s)\n"
        );
    }

    #[test]
    fn the_history_file_keeps_the_latest() {
        let dir = std::env::temp_dir().join(format!("fb-practice-{}", std::process::id()));
        let path = dir.join("practice.json");
        let many: Vec<Session> = (0..KEEP as u64 + 5)
            .map(|i| session(i, &[(2, 2)]))
            .collect();
        save(&path, &many).unwrap();
        let back = load(&path);
        assert_eq!(back.len(), KEEP);
        assert_eq!(back.last().unwrap().at, KEEP as u64 + 4);
        std::fs::remove_dir_all(dir).unwrap();
        assert!(load(&path).is_empty());
    }

    #[test]
    fn vim_is_told_where_the_drill_is() {
        let c = editor_command(
            "vim",
            Path::new("/tmp/a'b/drill.json"),
            Path::new("/tmp/out.json"),
        );
        assert_eq!(
            c,
            [
                "vim",
                "-c",
                "call flowbuddy#practice#run('/tmp/a''b/drill.json', '/tmp/out.json')"
            ]
        );
    }
}
