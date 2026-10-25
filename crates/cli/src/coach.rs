//! `flowbuddy coach`: ask a local model about your habits.
//!
//! The model sees the prompt built here and nothing else: the mistake
//! memory export (counts, learning stages and abstract shapes such as
//! `j×12`), never keys, text or file names. `--prompt` prints it so you can
//! check. The model is whatever local program `[ai] command` names; Flow
//! Buddy only writes to its stdin and reads its stdout.

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use flowbuddy_core::engine::Engine;
use flowbuddy_store::Export;

/// Patterns in the prompt, most relevant first; small models do better
/// with less.
const MAX_PATTERNS: usize = 8;

fn times(n: u64) -> String {
    match n {
        1 => "once".into(),
        n => format!("{n} times"),
    }
}

/// Families of habits that share one underlying cause.
const FAMILIES: &[(&str, &[&str])] = &[
    (
        "stepping one line or character at a time instead of a count or a bigger motion",
        &[
            "vim.repeat-jk",
            "vim.repeat-hl",
            "vim.repeat-dd",
            "vim.repeat-indent",
            "vim.repeat-join",
            "vim.repeat-paste",
            "motion.count",
            "motion.paragraph",
            "motion.page",
            "motion.word",
            "motion.find",
        ],
    ),
    (
        "reaching for the arrow keys",
        &[
            "vim.arrows-normal",
            "vim.arrows-insert",
            "bash.left-streak",
            "bash.up-history",
        ],
    ),
    (
        "deleting a character at a time",
        &[
            "vim.repeat-x",
            "vim.backspace-streak",
            "bash.backspace-clear",
            "vim.delete-then-insert",
        ],
    ),
];

/// Facts across habits, worked out here so the model only has to phrase
/// them: shared causes, a habit that differs between contexts, and the
/// week's overall balance.
pub fn insights(export: &Export) -> Vec<String> {
    use flowbuddy_core::learning::Stage;
    let mut out = Vec::new();
    for (cause, rules) in FAMILIES {
        let active: Vec<_> = export
            .patterns
            .iter()
            .filter(|p| rules.contains(&p.rule.as_str()) && p.last_7_days.hits > 0)
            .collect();
        let mut distinct: Vec<&str> = active.iter().map(|p| p.rule.as_str()).collect();
        distinct.sort();
        distinct.dedup();
        let hits: u64 = active.iter().map(|p| p.last_7_days.hits).sum();
        if distinct.len() >= 2 && hits >= 5 {
            out.push(format!(
                "{} habits come from one cause, {cause}: {hits} times this week in all.",
                distinct.len()
            ));
        }
    }
    let good = |s: Stage| matches!(s, Stage::Improving | Stage::Mastered);
    for (i, a) in export.patterns.iter().enumerate() {
        for b in &export.patterns[i + 1..] {
            if a.rule == b.rule && a.context != b.context && good(a.stage) != good(b.stage) {
                let (better, worse) = if good(a.stage) { (a, b) } else { (b, a) };
                out.push(format!(
                    "{}: {} in {}, but {} in {}.",
                    better.description,
                    better.stage.as_str(),
                    better.context,
                    worse.stage.as_str(),
                    worse.context
                ));
            }
        }
    }
    let hits: u64 = export.patterns.iter().map(|p| p.last_7_days.hits).sum();
    let adoptions: u64 = export
        .patterns
        .iter()
        .map(|p| p.last_7_days.adoptions)
        .sum();
    if hits + adoptions > 0 {
        out.push(format!(
            "This week: the better way {}, the old way {}.",
            times(adoptions),
            times(hits)
        ));
    }
    out
}

/// The prompt for `export`, answering `question` if there is one.
pub fn prompt(export: &Export, question: Option<&str>) -> String {
    let engine = Engine::standard();
    let mut out = String::new();
    out.push_str(
        "You are a friendly, practical Vim coach. Below is what Flow Buddy, a local tool, \
         noticed about how this person uses Vim (in VS Code, the terminal and bash vi-mode). \
         It only has counts and the shape of each habit, never their text.\n\n\
         Rules:\n\
         - Use only this data. Don't invent numbers or habits.\n\
         - Be concise: at most 5 short bullet points.\n\
         - Name exact commands in backticks, like `Ctrl-d` or `ci(`, and say in a few words what each does.\n\
         - Prefer what's practical to type over what's shortest: nobody counts 40 lines for `40j`.\n\
         - Praise real progress (improving or mastered habits) briefly.\n\n\
         Examples are written as shapes: `j×12` is j pressed 12 times in a row; \
         `keys for }×7` is 7 keys spent on a move that `}` makes in one.\n\n",
    );
    let s = &export.summary;
    let _ = writeln!(
        out,
        "Overview: {} habits tracked; {} mastered, {} improving, {} slipping back.",
        s.patterns, s.mastered, s.improving, s.regressed
    );
    let seen = insights(export);
    if !seen.is_empty() {
        out.push_str("\nAcross habits:\n");
        for line in seen {
            let _ = writeln!(out, "- {line}");
        }
    }
    out.push_str("\nHabits, most worth working on first:\n");
    for p in export.patterns.iter().take(MAX_PATTERNS) {
        let tip = engine
            .rules()
            .find(|r| r.id() == p.rule)
            .map(|r| r.title())
            .filter(|t| !t.is_empty())
            .unwrap_or("");
        let _ = write!(
            out,
            "- {} ({}). Stage: {}. Last 7 days: {}, the better way {}. All time: {}, the better way {}.",
            p.description,
            p.context,
            p.stage.as_str(),
            times(p.last_7_days.hits),
            times(p.last_7_days.adoptions),
            times(p.total.hits),
            times(p.total.adoptions),
        );
        if !tip.is_empty() {
            let _ = write!(out, " Flow Buddy's tip: {tip}.");
        }
        if p.false_positives > 0 {
            let _ = write!(out, " Marked not helpful {}.", times(p.false_positives));
        }
        if !p.recent_shapes.is_empty() {
            let _ = write!(out, " Recent examples: {}.", p.recent_shapes.join(", "));
        }
        out.push('\n');
    }
    if export.patterns.is_empty() {
        out.push_str("- Nothing recorded yet.\n");
    }
    out.push('\n');
    match question {
        Some(q) => {
            let _ = writeln!(out, "Their question: {q}");
        }
        None => out.push_str(
            "Tell them the one or two habits most worth working on now, and exactly how.\n",
        ),
    }
    out
}

#[derive(Debug)]
pub enum RunError {
    NotFound(String),
    Failed(String),
    TimedOut(u32),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::NotFound(cmd) => {
                write!(f, "{cmd}: not found. Is it installed and on your PATH?")
            }
            RunError::Failed(why) => write!(f, "the model command failed: {why}"),
            RunError::TimedOut(s) => write!(f, "no answer after {s}s (see [ai] timeout_seconds)"),
        }
    }
}

/// Run `command`, give it `prompt` on stdin, and copy its answer to `out`
/// as it comes.
pub fn run(
    command: &[String],
    prompt: &str,
    timeout_s: u32,
    out: &mut dyn Write,
) -> Result<(), RunError> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| RunError::Failed("[ai] command is empty".into()))?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => RunError::NotFound(program.clone()),
            _ => RunError::Failed(e.to_string()),
        })?;
    // Write the prompt from a thread so a model that answers while it reads
    // can't deadlock us.
    let mut stdin = child.stdin.take().expect("piped");
    let prompt = prompt.to_string();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(prompt.as_bytes());
    });
    let mut stdout = child.stdout.take().expect("piped");
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let reader = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = stdout.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(u64::from(timeout_s));
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => {
                let _ = out.write_all(&chunk);
                let _ = out.flush();
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RunError::TimedOut(timeout_s));
        }
    }
    let _ = writer.join();
    let _ = reader.join();
    let status = child.wait().map_err(|e| RunError::Failed(e.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(RunError::Failed(format!("exited with {status}")))
    }
}

/// What to tell someone who hasn't set `[ai] command` yet.
pub const SETUP: &str = "\
flowbuddy coach asks a model running on this machine about your habits.
It's off until you name one in ~/.config/flowbuddy/config.toml, for example:

    [ai]
    command = [\"ollama\", \"run\", \"llama3.2:3b\"]

Any program that reads a prompt on stdin and answers on stdout works
(llama.cpp's llama-cli, llamafile, ...). Pick one that runs locally: the
prompt is your mistake memory (counts and habit shapes, never text).
See it first with: flowbuddy coach --prompt
";

#[cfg(test)]
mod tests {
    use super::*;
    use flowbuddy_store::{Event, Store, days};

    fn sample() -> Export {
        let s = Store::open_in_memory().unwrap();
        let now = 1_800_000_000_000u64;
        let day = days::day(now);
        let hit = Event::Hit {
            mode: "normal",
            shape: "j×12",
        };
        for _ in 0..6 {
            s.record(&day, "vscode-vim", "vim.repeat-jk", hit, now / 1000)
                .unwrap();
        }
        s.record(
            &day,
            "vscode-vim",
            "vim.repeat-jk",
            Event::Adoption,
            now / 1000,
        )
        .unwrap();
        let week = days::range(now, 7, 0);
        let describe = |id: &str| Some(crate::report::describe(id));
        Export::build(&s, &describe, (&week.0, &week.1), now / 1000).unwrap()
    }

    #[test]
    fn prompt_has_the_habits_and_no_more() {
        let p = prompt(&sample(), None);
        assert!(p.contains("- Pressing j or k many times instead of a count or a bigger motion (vscode-vim). Stage: new. Last 7 days: 6 times, the better way once."), "{p}");
        assert!(p.contains("Flow Buddy's tip: Try a count."), "{p}");
        assert!(p.contains("Recent examples: j×12"), "{p}");
        assert!(p.ends_with(
            "Tell them the one or two habits most worth working on now, and exactly how.\n"
        ));
    }

    #[test]
    fn insights_across_habits() {
        use flowbuddy_core::learning::Stage;
        let mut e = sample();
        let mut x = e.patterns[0].clone();
        x.rule = "vim.repeat-hl".into();
        x.last_7_days.hits = 4;
        e.patterns.push(x);
        let mut terminal = e.patterns[0].clone();
        terminal.context = "terminal-vim".into();
        terminal.stage = Stage::Improving;
        e.patterns.push(terminal);
        let got = insights(&e);
        assert!(
            got.iter()
                .any(|l| l.starts_with("2 habits come from one cause, stepping one line")),
            "{got:?}"
        );
        assert!(
            got.iter()
                .any(|l| l.ends_with("improving in terminal-vim, but new in vscode-vim.")),
            "{got:?}"
        );
        assert!(
            got.iter()
                .any(|l| l.starts_with("This week: the better way")),
            "{got:?}"
        );
    }

    #[test]
    fn prompt_with_a_question() {
        let p = prompt(&sample(), Some("How do I move faster in big files?"));
        assert!(
            p.ends_with("Their question: How do I move faster in big files?\n"),
            "{p}"
        );
    }

    #[test]
    fn runs_a_command_and_streams_its_answer() {
        // `cat` stands in for a model: it answers with the prompt.
        let mut out = Vec::new();
        run(&["cat".into()], "hello model", 10, &mut out).unwrap();
        assert_eq!(out, b"hello model");
    }

    #[test]
    fn missing_command_and_timeouts() {
        let mut out = Vec::new();
        let err = run(&["no-such-model-binary".into()], "x", 5, &mut out).unwrap_err();
        assert!(matches!(err, RunError::NotFound(_)), "{err}");
        let err = run(&["sleep".into(), "5".into()], "x", 1, &mut out).unwrap_err();
        assert!(matches!(err, RunError::TimedOut(1)), "{err}");
    }
}
