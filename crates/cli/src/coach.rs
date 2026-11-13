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

use flowbuddy_core::config::{AiConfig, AiProvider};
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

/// What the data says about the habits, for any prompt: the overview,
/// facts across habits, and each habit, most worth working on first.
pub fn facts(export: &Export) -> String {
    let engine = Engine::standard();
    let mut out = String::new();
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
    out
}

/// The prompt for `export`, answering `question` if there is one.
/// The model's answer, for a terminal: Markdown it wrote anyway made
/// readable. Headings and **bold** become bold, `commands` stand out,
/// bullets are •, and code blocks are indented. `color`: a terminal that
/// shows it; otherwise only the marks go.
pub fn for_terminal(answer: &str, color: bool) -> String {
    let (bold, code, off) = if color {
        ("\x1b[1m", "\x1b[36m", "\x1b[0m")
    } else {
        ("", "", "")
    };
    let inline = |line: &str| -> String {
        let mut out = String::new();
        let mut rest = line;
        while !rest.is_empty() {
            if let Some(r) = rest.strip_prefix("**")
                && let Some(end) = r.find("**")
            {
                let _ = write!(out, "{bold}{}{off}", &r[..end]);
                rest = &r[end + 2..];
            } else if let Some(r) = rest.strip_prefix('`')
                && let Some(end) = r.find('`')
            {
                let _ = write!(out, "{code}{}{off}", &r[..end]);
                rest = &r[end + 1..];
            } else {
                let c = rest.chars().next().unwrap_or_default();
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
        out
    };
    let mut out = String::new();
    let mut in_block = false;
    for line in answer.trim().lines() {
        let t = line.trim_start();
        if t.starts_with("```") {
            in_block = !in_block;
            continue;
        }
        if in_block {
            let _ = writeln!(out, "    {code}{line}{off}");
            continue;
        }
        let indent = &line[..line.len() - t.len()];
        if let Some(h) = t.strip_prefix('#') {
            let h = h.trim_start_matches('#').trim();
            let _ = writeln!(
                out,
                "{bold}{}{off}",
                inline(h).replace(off, &format!("{off}{bold}"))
            );
        } else if let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            let _ = writeln!(out, "{indent}• {}", inline(item));
        } else if t.chars().all(|c| matches!(c, '-' | '*' | '_')) && t.len() >= 3 {
            out.push('\n');
        } else {
            let _ = writeln!(out, "{}", inline(line));
        }
    }
    out
}

pub fn prompt(export: &Export, question: Option<&str>) -> String {
    prompt_with(export, question, &[])
}

/// The prompt, with recent tips and the code around them, redacted.
pub fn prompt_with(
    export: &Export,
    question: Option<&str>,
    snippets: &[flowbuddy_core::ipc::TipSnippet],
) -> String {
    let mut out = String::new();
    out.push_str(
        "You are a friendly, practical Vim coach. Below is what Flow Buddy, a local tool, \
         noticed about how this person uses Vim (in VS Code, the terminal and bash vi-mode). \
         It only has counts and the shape of each habit, never their text.\n\n\
         Rules:\n\
         - Use only this data. Don't invent numbers or habits.\n\
         - Be concise: at most 5 short points, each starting with \"- \".\n\
         - It's shown in a terminal: plain text, no headings, bold, tables or code blocks.\n\
         - Name exact commands in backticks, like `Ctrl-d` or `ci(`, and say in a few words what each does.\n\
         - Prefer what's practical to type over what's shortest: nobody counts 40 lines for `40j`.\n\
         - Praise real progress (improving or mastered habits) briefly.\n\n\
         Examples are written as shapes: `j×12` is j pressed 12 times in a row; \
         `keys for }×7` is 7 keys spent on a move that `}` makes in one.\n\n",
    );
    out.push_str(&facts(export));
    out.push('\n');
    if !snippets.is_empty() {
        out.push_str(
            "Recent tips, each with the code it was about (secrets redacted). Use them \
             only to make the advice concrete; don't review the code.\n\n",
        );
        for s in snippets {
            let code = flowbuddy_core::text::redact(&s.snippet.lines.join("\n"));
            let _ = writeln!(
                out,
                "- Tip ({}): {}\n```{}\n{code}\n```",
                s.rule_id,
                flowbuddy_core::glossary::plain(&s.tip),
                s.snippet.filetype.as_deref().unwrap_or("")
            );
        }
        out.push('\n');
    }
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
    run_in(command, prompt, timeout_s, out, None)
}

fn run_in(
    command: &[String],
    prompt: &str,
    timeout_s: u32,
    out: &mut dyn Write,
    dir: Option<&std::path::Path>,
) -> Result<(), RunError> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| RunError::Failed("[ai] command is empty".into()))?;
    let mut cmd = Command::new(program);
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
    let mut child = cmd
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

/// The prompt for the weekly note: the usual facts plus `summary`, the
/// fixed note, asking for something short enough for a notification.
pub fn weekly_prompt(export: &Export, summary: &str) -> String {
    let mut p = prompt(export, None);
    let ask = "Tell them the one or two habits most worth working on now, and exactly how.\n";
    if let Some(stripped) = p.strip_suffix(ask) {
        p = stripped.to_string();
    }
    let _ = write!(
        p,
        "This week's numbers: {summary}\n\n\
         Write their weekly progress note: two or three short, warm sentences for a desktop \
         notification. Mention one real improvement if there is one, and the one habit to work \
         on next with the exact command. Plain text only: no lists, no headings, no markdown.\n"
    );
    p
}

/// Tidy a model's answer for a notification: plain, short.
pub fn notification_text(answer: &str) -> Option<String> {
    let text = answer.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = text
        .trim_matches(|c: char| c == '"' || c.is_whitespace())
        .to_string();
    if text.is_empty() {
        return None;
    }
    const MAX: usize = 400;
    Some(if text.chars().count() > MAX {
        let cut: String = text.chars().take(MAX - 1).collect();
        format!("{}…", cut.trim_end())
    } else {
        text
    })
}

/// Ask the AI you chose in `[ai]` and copy its answer to `out`. `None` if
/// none is set up.
pub fn ask(ai: &AiConfig, prompt: &str, out: &mut dyn Write) -> Option<Result<(), RunError>> {
    let timeout = ai.timeout_seconds;
    let provider = match ai.provider {
        AiProvider::None if ai.command.is_some() => AiProvider::Command,
        p => p,
    };
    Some(match provider {
        AiProvider::None => return None,
        AiProvider::Command => {
            let command = ai.command.clone().unwrap_or_default();
            run(&command, prompt, timeout, out)
        }
        AiProvider::ClaudeCode => {
            // `claude -p` reads the prompt from stdin and answers with your
            // Claude subscription. Run it in an empty directory so it has
            // nothing to look at but the prompt.
            let mut command = vec!["claude".to_string(), "-p".into()];
            if let Some(model) = &ai.model {
                command.extend(["--model".into(), model.clone()]);
            }
            // Ours alone: Claude Code reads settings (and hooks) from the
            // folder it runs in, so it mustn't be one another user can
            // write to, like /tmp.
            flowbuddy_core::paths::scratch_dir("coach")
                .map_err(|e| RunError::Failed(format!("no private folder to run in: {e}")))
                .and_then(|dir| {
                    let result = run_in(&command, prompt, timeout, out, Some(&dir));
                    let _ = std::fs::remove_dir_all(&dir);
                    result
                })
                .map_err(|e| match (e, &ai.model) {
                    (RunError::Failed(why), Some(model)) => RunError::Failed(format!(
                        "{why}. If Claude Code said the model is wrong, [ai] model is \"{model}\": try `flowbuddy config set ai.model opus`"
                    )),
                    (e, _) => e,
                })
        }
        AiProvider::Anthropic => api_key(ai, "ANTHROPIC_API_KEY").and_then(|key| {
            let url = ai
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com".into());
            let model = ai.model.clone().unwrap_or_else(|| ANTHROPIC_MODEL.into());
            anthropic(&url, &key, &model, prompt, timeout).map(|t| {
                let _ = out.write_all(t.as_bytes());
            })
        }),
        AiProvider::Openai => api_key(ai, "OPENAI_API_KEY").and_then(|key| {
            let url = ai
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".into());
            let Some(model) = ai.model.clone() else {
                return Err(RunError::Failed(
                    "set [ai] model for the openai provider".into(),
                ));
            };
            openai(&url, &key, &model, prompt, timeout).map(|t| {
                let _ = out.write_all(t.as_bytes());
            })
        }),
    })
}

/// The Anthropic API's default model for Flow Buddy.
const ANTHROPIC_MODEL: &str = "claude-opus-5-5";

/// The API key: from `api_key_command` if set, else the environment.
fn api_key(ai: &AiConfig, default_env: &str) -> Result<String, RunError> {
    if let Some(cmd) = ai.api_key_command.as_ref().filter(|c| !c.is_empty()) {
        let out = Command::new(&cmd[0])
            .args(&cmd[1..])
            .stderr(Stdio::inherit())
            .output()
            .map_err(|e| RunError::Failed(format!("api_key_command: {e}")))?;
        let key = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !out.status.success() || key.is_empty() {
            return Err(RunError::Failed("api_key_command printed no key".into()));
        }
        return Ok(key);
    }
    let var = ai.api_key_env.as_deref().unwrap_or(default_env);
    std::env::var(var)
        .ok()
        .filter(|k| !k.is_empty())
        .ok_or_else(|| RunError::Failed(format!("no API key: set {var}, or [ai] api_key_command")))
}

fn agent(timeout_s: u32) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(u64::from(timeout_s))))
        .http_status_as_error(false)
        // The key goes to the server you named and nowhere it points on.
        .max_redirects(0)
        .build()
        .into()
}

/// A key is only sent encrypted, or to this machine.
fn safe_for_a_key(base_url: &str) -> Result<(), RunError> {
    if base_url.starts_with("https://") || flowbuddy_core::text::is_local_url(base_url) {
        return Ok(());
    }
    Err(RunError::Failed(format!(
        "[ai] base_url is {base_url}: your API key would be sent unencrypted. Use https://, or a server on this machine (http://localhost…)"
    )))
}

/// One Messages API call. Refusals and errors come back as errors; the key
/// never appears in them.
pub fn anthropic(
    base_url: &str,
    key: &str,
    model: &str,
    prompt: &str,
    timeout_s: u32,
) -> Result<String, RunError> {
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 16000,
        "output_config": {"effort": "medium"},
        // If a safety classifier declines, let the API retry on a model
        // that can answer, instead of failing.
        "fallbacks": "default",
        "messages": [{"role": "user", "content": prompt}],
    });
    safe_for_a_key(base_url)?;
    let mut resp = agent(timeout_s)
        .post(&format!("{}/v1/messages", base_url.trim_end_matches('/')))
        .header("x-api-key", key)
        .header("anthropic-version", "2026-10-21")
        .header("anthropic-beta", "server-side-fallback-2026-07-01")
        .header("content-type", "application/json")
        .send_json(&body)
        .map_err(|e| RunError::Failed(format!("Anthropic API: {e}")))?;
    let status = resp.status().as_u16();
    let v: serde_json::Value = resp
        .body_mut()
        .read_json()
        .map_err(|e| RunError::Failed(format!("Anthropic API: unreadable answer ({e})")))?;
    if status != 200 {
        let msg = v["error"]["message"].as_str().unwrap_or("no details");
        return Err(RunError::Failed(format!("Anthropic API {status}: {msg}")));
    }
    if v["stop_reason"] == "refusal" {
        return Err(RunError::Failed("the model declined to answer".into()));
    }
    let text: String = v["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect();
    Ok(text)
}

/// One OpenAI-compatible chat completion.
pub fn openai(
    base_url: &str,
    key: &str,
    model: &str,
    prompt: &str,
    timeout_s: u32,
) -> Result<String, RunError> {
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
    });
    safe_for_a_key(base_url)?;
    let mut resp = agent(timeout_s)
        .post(&format!(
            "{}/chat/completions",
            base_url.trim_end_matches('/')
        ))
        .header("authorization", &format!("Bearer {key}"))
        .header("content-type", "application/json")
        .send_json(&body)
        .map_err(|e| RunError::Failed(format!("API: {e}")))?;
    let status = resp.status().as_u16();
    let v: serde_json::Value = resp
        .body_mut()
        .read_json()
        .map_err(|e| RunError::Failed(format!("API: unreadable answer ({e})")))?;
    if status != 200 {
        let msg = v["error"]["message"].as_str().unwrap_or("no details");
        return Err(RunError::Failed(format!("API {status}: {msg}")));
    }
    v["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| RunError::Failed("API: no answer in the response".into()))
}

/// What to tell someone who hasn't set `[ai] command` yet.
pub const SETUP: &str = "\
flowbuddy coach asks an AI of your choice about your habits. It's off until
you pick one in ~/.config/flowbuddy/config.toml:

    [ai]
    provider = \"claude-code\"     # your Claude subscription, via `claude -p`
    # provider = \"anthropic\"     # an API key in ANTHROPIC_API_KEY
    # provider = \"openai\"        # OPENAI_API_KEY, model = \"...\", base_url for others
    # command = [\"ollama\", \"run\", \"llama3.2:3b\"]   # a local model

The prompt is your mistake memory: counts and habit shapes, never keys or
text. With a cloud provider it leaves this machine. See it first with:
    flowbuddy coach --prompt
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
    fn weekly_prompt_and_answer() {
        let p = weekly_prompt(&sample(), "The better way 7 times, 6 habits spotted.");
        assert!(
            p.contains("This week's numbers: The better way 7 times"),
            "{p}"
        );
        assert!(p.ends_with("no lists, no headings, no markdown.\n"), "{p}");
        assert!(!p.contains("Tell them the one or two habits"), "{p}");
        assert_eq!(
            notification_text("  \"Great week!\n  Try `}` next.\"  ").as_deref(),
            Some("Great week! Try `}` next.")
        );
        assert_eq!(notification_text("   "), None);
        assert_eq!(
            notification_text(&"x".repeat(500)).unwrap().chars().count(),
            400
        );
    }

    #[test]
    fn snippets_are_redacted_in_the_prompt() {
        let tip = flowbuddy_core::ipc::TipSnippet {
            rule_id: "motion.text-object".into(),
            tip: "That took 7 keys. `diw` does it from anywhere inside, in 3.".into(),
            snippet: flowbuddy_core::ipc::Snippet {
                at_ms: 1,
                filetype: Some("python".into()),
                lines: vec!["api_key = 'sk-live-abcdefghijk'".into(), "call(foo)".into()],
            },
        };
        let p = prompt_with(&sample(), None, &[tip]);
        assert!(
            p.contains("```python\napi_key = '<redacted>'\ncall(foo)\n```"),
            "{p}"
        );
        assert!(p.contains("diw does it from anywhere inside"));
        assert!(!p.contains("abcdefghijk"));
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

    /// A one-shot HTTP server: answers one request with `status` and `body`,
    /// and hands back what it was sent.
    #[test]
    fn a_key_is_never_sent_unencrypted_to_another_machine() {
        for url in ["http://api.example.com", "http://localhost.example.com/v1"] {
            let err = anthropic(url, "sk-secret", "m", "x", 1)
                .unwrap_err()
                .to_string();
            assert!(err.contains("unencrypted"), "{err}");
            assert!(!err.contains("sk-secret"));
            assert!(openai(url, "sk-secret", "m", "x", 1).is_err());
        }
        assert!(safe_for_a_key("https://api.example.com").is_ok());
        assert!(safe_for_a_key("http://127.0.0.1:11434/v1").is_ok());
    }

    fn fake_server(status: u16, body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            let mut len = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
                head.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut body_in = vec![0; len];
            std::io::Read::read_exact(&mut reader, &mut body_in).unwrap();
            let reply = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
            head + &String::from_utf8_lossy(&body_in)
        });
        (url, handle)
    }

    #[test]
    fn anthropic_messages_call() {
        let (url, sent) = fake_server(
            200,
            r#"{"content":[{"type":"text","text":"Try }."}],"stop_reason":"end_turn"}"#,
        );
        let answer = anthropic(&url, "sk-test", "claude-opus-5-5", "hello", 5).unwrap();
        assert_eq!(answer, "Try }.");
        let sent = sent.join().unwrap();
        assert!(sent.starts_with("POST /v1/messages "), "{sent}");
        assert!(sent.contains("x-api-key: sk-test"), "{sent}");
        assert!(sent.contains("anthropic-version: 2026-10-21"), "{sent}");
        let body: serde_json::Value =
            serde_json::from_str(&sent[sent.find('{').unwrap()..]).unwrap();
        assert_eq!(body["model"], "claude-opus-5-5");
        assert_eq!(body["fallbacks"], "default");
        assert_eq!(body["messages"][0]["content"], "hello");
    }

    #[test]
    fn anthropic_errors_and_refusals() {
        let (url, _) = fake_server(
            401,
            r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#,
        );
        let err = anthropic(&url, "sk-secret", "m", "x", 5)
            .unwrap_err()
            .to_string();
        assert!(err.contains("401: invalid x-api-key"), "{err}");
        assert!(!err.contains("sk-secret"), "the key never shows");
        let (url, _) = fake_server(200, r#"{"content":[],"stop_reason":"refusal"}"#);
        assert!(
            anthropic(&url, "k", "m", "x", 5)
                .unwrap_err()
                .to_string()
                .contains("declined")
        );
    }

    #[test]
    fn openai_compatible_call() {
        let (url, sent) = fake_server(
            200,
            r#"{"choices":[{"message":{"role":"assistant","content":"Use ciw."}}]}"#,
        );
        assert_eq!(
            openai(&url, "k", "some-model", "hi", 5).unwrap(),
            "Use ciw."
        );
        let sent = sent.join().unwrap();
        assert!(sent.starts_with("POST /chat/completions "), "{sent}");
        assert!(sent.contains("authorization: Bearer k"), "{sent}");
    }

    #[test]
    fn keys_from_a_command_or_the_environment() {
        let mut ai = AiConfig {
            api_key_command: Some(vec!["echo".into(), "from-keyring".into()]),
            ..Default::default()
        };
        assert_eq!(api_key(&ai, "UNUSED").unwrap(), "from-keyring");
        ai.api_key_command = None;
        ai.api_key_env = Some("FLOWBUDDY_TEST_NO_SUCH_KEY".into());
        assert!(
            api_key(&ai, "X")
                .unwrap_err()
                .to_string()
                .contains("FLOWBUDDY_TEST_NO_SUCH_KEY")
        );
    }

    #[test]
    fn nothing_without_a_provider() {
        let mut out = Vec::new();
        assert!(ask(&AiConfig::default(), "x", &mut out).is_none());
        let ai = AiConfig {
            command: Some(vec!["cat".into()]),
            ..Default::default()
        };
        ask(&ai, "echo me", &mut out).unwrap().unwrap();
        assert_eq!(out, b"echo me");
    }

    #[test]
    fn markdown_reads_well_in_a_terminal() {
        let answer = "## Your habits\n\nYou press **j** a lot.\n\n- Use `5j` to move\n* or `}` for paragraphs\n\n```vim\n:set relativenumber\n```\n---\nDone.\n";
        assert_eq!(
            for_terminal(answer, false),
            "Your habits\n\nYou press j a lot.\n\n• Use 5j to move\n• or } for paragraphs\n\n    :set relativenumber\n\nDone.\n"
        );
        let colored = for_terminal("Try `ci(` and **this**.", true);
        assert_eq!(colored, "Try \x1b[36mci(\x1b[0m and \x1b[1mthis\x1b[0m.\n");
        // A lone backtick or star is left alone.
        assert_eq!(
            for_terminal("a * b and it's `odd", false),
            "a * b and it's `odd\n"
        );
    }
}
