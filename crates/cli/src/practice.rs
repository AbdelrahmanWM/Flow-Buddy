//! `flowbuddy practice`: a short drill on your weakest habit, in Vim. The
//! drill is written to a file, Vim runs it (the plugin's
//! `flowbuddy#practice#run`), and how it went is kept in
//! `~/.local/state/flowbuddy/practice.json`: your scores, never any text.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use flowbuddy_core::practice::{self, Drill, Family, RoundResult, Session};

/// Sessions kept in the history.
const KEEP: usize = 200;

/// Which drill, and why: the most pressing habit that has one.
pub fn pick(patterns: &[flowbuddy_store::PatternExport]) -> (Family, Option<String>) {
    patterns
        .iter()
        .filter(|p| p.stage != flowbuddy_core::learning::Stage::Mastered)
        .find_map(|p| practice::family_for(&p.rule).map(|f| (f, Some(p.description.clone()))))
        .unwrap_or((Family::Vertical, None))
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
