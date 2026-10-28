mod client;
mod coach;
mod config_cmd;
mod doctor;
mod report;
mod review;
mod setup;
mod tips;

use clap::{Parser, Subcommand};
use flowbuddy_core::editor::EditorMode;
use flowbuddy_core::ipc::{ClientKind, ClientMsg, Command, CtxEvent, RuleAction, RuleInfo, Status};
use flowbuddy_core::policy::CoachingMode;

use crate::client::Client;

/// Flow Buddy: improve all the time without stopping to study.
#[derive(Parser)]
#[command(name = "flowbuddy", version = flowbuddy_core::VERSION)]
struct Args {
    #[command(subcommand)]
    command: Cmd,
    /// Print JSON instead of text, for scripts and the settings window
    /// (doctor, learn, learn --report, learn --coverage, summary, rules,
    /// config list). See docs/cli-json.md.
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show whether the daemon is running and what it sees (no key data).
    Status {
        /// Refresh every second; switch windows to see the context change.
        #[arg(long)]
        watch: bool,
    },
    /// Show or set how involved Flow Buddy is: off, passive, balanced, active.
    Mode { mode: Option<CoachingMode> },
    /// Stop watching for a while: 30m, 1h, 2h30m, until-tomorrow.
    Pause {
        #[arg(default_value = "1h")]
        duration: String,
    },
    /// Resume after a pause.
    Resume,
    /// Mark the last nudge as wrong, to help tune the rules.
    ReportFp,
    /// The one habit worth working on now.
    Next,
    /// Your daily "something new" and your progress through Vim's features.
    Learn {
        /// Say you know these (ids from --list), so tips build on them.
        #[arg(long, value_delimiter = ',')]
        know: Vec<String>,
        /// Say you don't know these, so they're taught.
        #[arg(long, value_delimiter = ',')]
        forget: Vec<String>,
        /// Back to judging these from your use.
        #[arg(long, value_delimiter = ',')]
        reset: Vec<String>,
        /// Every item: its id, level and commands.
        #[arg(long)]
        list: bool,
        /// Where your history disagrees with the order things are taught in.
        #[arg(long)]
        audit: bool,
        /// How much of Vim's command index is taught and coached, by mode,
        /// and the everyday commands not covered yet.
        #[arg(long)]
        coverage: bool,
        /// Whether tips help (against ones held back), and how well you
        /// know and recall each skill.
        #[arg(long)]
        report: bool,
    },
    /// Check everything Flow Buddy needs, and say how to fix what's missing.
    Doctor,
    /// A few questions about how you'd like to be coached, saved to the
    /// config file.
    Setup,
    /// Show tips here as they come, with what their commands do: keep it
    /// open in a terminal beside your work. Ctrl-C to stop.
    Watch,
    /// Your recent tips, newest first; with words, only those that match:
    /// `flowbuddy tips undo`.
    Tips {
        search: Vec<String>,
        /// How many to show.
        #[arg(short = 'n', long, default_value_t = 10)]
        limit: usize,
        /// Also say what each command in them does.
        #[arg(long, short)]
        explain: bool,
    },
    /// Ask a local model about your habits (set `[ai] command` first).
    Coach {
        /// A question, e.g. "how do I move faster in big files?"
        question: Vec<String>,
        /// Print exactly what the model would be sent, and stop.
        #[arg(long)]
        prompt: bool,
        /// Send this week's progress note, written by the model if
        /// `[ai] weekly` is on (run by flowbuddy-weekly.service).
        #[arg(long, hide = true)]
        weekly_note: bool,
        /// Have the model write today's personal tips, if `[ai] tips` is on
        /// (run daily by flowbuddy-tips.timer).
        #[arg(long, hide = true)]
        write_tips: bool,
    },
    /// Ask your AI how to tune the tips; apply what you agree with.
    Review {
        /// Apply these proposals from the last review, by number.
        #[arg(long, num_args = 1..)]
        apply: Vec<usize>,
        /// Print what would be sent, and stop.
        #[arg(long)]
        prompt: bool,
    },
    /// Record a session to check rules against (nothing typed is kept).
    Record {
        #[command(subcommand)]
        action: RecordCmd,
    },
    /// Print the ssh config that lets Vim on a remote machine report here.
    SshConfig {
        /// The host, as you `ssh` to it.
        host: String,
    },
    /// What Vim keys do, piece by piece: `flowbuddy explain 'ci"'`.
    Explain {
        /// Keys to explain; with --last, leave out.
        #[arg(required_unless_present = "last", default_value = "")]
        keys: String,
        /// Your last slow move or edit, and the best ways to do it.
        #[arg(long)]
        last: bool,
        /// Insert-mode meanings (Ctrl-w, Ctrl-u, Ctrl-o).
        #[arg(long)]
        insert: bool,
    },
    /// How you did today (or this week) compared with before.
    Summary {
        /// The last 7 days instead of today.
        #[arg(long)]
        week: bool,
    },
    /// Inspect, export or delete the mistake memory.
    Memory {
        #[command(subcommand)]
        action: Option<MemoryCmd>,
    },
    /// Every setting: list them, or get, set or reset one
    /// (`flowbuddy config set learn.daily false`).
    Config {
        #[command(subcommand)]
        action: Option<config_cmd::ConfigCmd>,
    },
    /// List rules, or enable/disable/mute one.
    Rules {
        #[command(subcommand)]
        action: Option<RulesCmd>,
    },
    /// Report shell state to the daemon. Used by shell/flowbuddy.bash.
    #[command(hide = true)]
    Ctx {
        #[command(subcommand)]
        event: CtxCmd,
        #[arg(long, global = true)]
        shell_pid: Option<u32>,
        #[arg(long, global = true)]
        tty: Option<String>,
    },
}

#[derive(Subcommand)]
enum MemoryCmd {
    /// Every remembered pattern with its stage and history (the default).
    Show,
    /// Print the memory as versioned JSON (see docs/memory.md).
    Export,
    /// Delete what is remembered about one rule, or everything.
    Forget {
        /// Rule id, e.g. vim.repeat-jk.
        rule: Option<String>,
        #[arg(long, conflicts_with = "rule")]
        all: bool,
        /// Don't ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Cap the "better way" counts recorded before they were fixed
    /// (they counted every key instead of every run).
    FixAdoptions {
        /// Only days before this one (YYYY-MM-DD), when the fix arrived.
        #[arg(long, default_value = "2026-10-26")]
        before: String,
        #[arg(long, short)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum RecordCmd {
    /// Start recording to ~/.local/state/flowbuddy/recordings/.
    Start,
    /// Stop and print where it was saved.
    Stop,
    /// Whether a recording is going.
    Status,
    /// Replay your recordings (all of them, or these files) with each
    /// rule's threshold lower and higher: how many tips each would give,
    /// and how many of them you marked dismissed or not helpful.
    Tune { files: Vec<std::path::PathBuf> },
    /// Run a recording through the rules and count what they find. With
    /// --expect, compare with the counts you checked by hand.
    Check {
        file: std::path::PathBuf,
        /// A JSON object of rule id to how many findings are right.
        #[arg(long)]
        expect: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
enum RulesCmd {
    /// List rules (the default).
    List,
    /// Turn a rule back on.
    Enable {
        rule: String,
    },
    /// Stop a rule from running at all.
    Disable {
        rule: String,
    },
    /// Keep counting, but never nudge for this rule.
    Mute {
        rule: String,
    },
    Unmute {
        rule: String,
    },
    /// Undo threshold raises made after "not helpful" reports.
    Reset {
        rule: String,
    },
}

#[derive(Subcommand)]
enum CtxCmd {
    /// A command started; pass only its name.
    Fg {
        command: String,
        /// cd-up, reused-arg or one-word-changed, worked out by the hook.
        #[arg(long)]
        shape: Option<String>,
    },
    /// The prompt is back.
    Prompt {
        #[arg(long, action = clap::ArgAction::Set, default_value_t = false)]
        vi_mode: bool,
    },
    /// The vi-mode switched: normal, insert or visual.
    Mode { mode: String },
}

fn main() {
    let args = Args::parse();
    let json = args.json;
    if json && !takes_json(&args.command) {
        eprintln!("flowbuddy: --json works with doctor, learn, summary, rules and config list");
        std::process::exit(2);
    }
    let result = match args.command {
        Cmd::Status { watch } => status(watch),
        Cmd::Rules { action } => rules(action.unwrap_or(RulesCmd::List), json),
        Cmd::Config { action } => config_cmd::run(
            match action {
                Some(config_cmd::ConfigCmd::List { json: j, basic }) => {
                    config_cmd::ConfigCmd::List {
                        json: j || json,
                        basic,
                    }
                }
                Some(other) => other,
                None => config_cmd::ConfigCmd::List { json, basic: !json },
            },
            &flowbuddy_core::paths::config_file(),
        ),
        Cmd::Mode { mode } => set_mode(mode),
        Cmd::Summary { week } => summary(week, json),
        Cmd::Next => next(),
        Cmd::Learn {
            know,
            forget,
            reset,
            list,
            audit,
            coverage,
            report: learn_report,
        } => {
            if learn_report {
                learn_report_cmd(json)
            } else if coverage && json {
                print_json(&coverage_json());
                Ok(())
            } else if coverage {
                print!("{}", learn_coverage());
                Ok(())
            } else if audit {
                learn_audit()
            } else if list {
                for i in flowbuddy_core::curriculum::CURRICULUM {
                    println!(
                        "{:<16} level {}  {}",
                        i.id,
                        i.level,
                        flowbuddy_core::glossary::plain(i.keys)
                    );
                }
                Ok(())
            } else if know.is_empty() && forget.is_empty() && reset.is_empty() {
                learn(json)
            } else {
                Client::connect(ClientKind::Cli).and_then(|mut c| {
                    for (items, known) in [(know, Some(true)), (forget, Some(false)), (reset, None)]
                    {
                        if !items.is_empty() {
                            c.command(Command::KnowItems { items, known })?;
                        }
                    }
                    println!("Noted.");
                    Ok(())
                })
            }
        }
        Cmd::Watch => watch(),
        Cmd::Setup => run_setup(),
        Cmd::Doctor => {
            let status = Client::connect(ClientKind::Cli)
                .and_then(|mut c| c.query(Command::Status))
                .map_err(|e| format!("{e:#}"))
                .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()));
            let checks = doctor::checks(&doctor::Env::gather(status));
            if json {
                print_json(&serde_json::json!({
                    "version": 1,
                    "ok": checks.iter().all(|c| c.ok),
                    "checks": checks
                        .iter()
                        .map(|c| serde_json::json!({"ok": c.ok, "what": c.what, "fix": c.fix}))
                        .collect::<Vec<_>>(),
                }));
            } else {
                print!("{}", doctor::report(&checks));
            }
            if checks.iter().any(|c| !c.ok) {
                std::process::exit(1);
            }
            Ok(())
        }
        Cmd::Tips {
            search,
            limit,
            explain,
        } => tips_history(&search.join(" "), limit, explain),
        Cmd::Record { action } => record(action),
        Cmd::SshConfig { host } => {
            print!("{}", ssh_config(&host, &flowbuddy_core::paths::socket()));
            Ok(())
        }
        Cmd::Review { apply, prompt } => {
            if apply.is_empty() {
                review(prompt)
            } else {
                review_apply(&apply)
            }
        }
        Cmd::Coach {
            weekly_note: true, ..
        } => weekly_note(),
        Cmd::Coach {
            write_tips: true, ..
        } => write_tips(),
        Cmd::Coach {
            question, prompt, ..
        } => coach(&question.join(" "), prompt),
        Cmd::Explain { last: true, .. } => explain_last(),
        Cmd::Explain { keys, insert, .. } => {
            explain(&keys, insert);
            Ok(())
        }
        Cmd::Memory { action } => memory(action.unwrap_or(MemoryCmd::Show)),
        Cmd::Pause { duration } => pause(&duration),
        Cmd::Resume => simple(Command::Resume, "resumed"),
        Cmd::ReportFp => simple(
            Command::ReportFalsePositive,
            "thanks, the last nudge was marked as not helpful",
        ),
        Cmd::Ctx {
            event,
            shell_pid,
            tty,
        } => ctx(event, shell_pid, tty),
    };
    if let Err(e) = result {
        eprintln!("flowbuddy: {e:#}");
        std::process::exit(1);
    }
}

fn record(action: RecordCmd) -> anyhow::Result<()> {
    use flowbuddy_core::ipc::Recording;
    use flowbuddy_core::session;
    let on = match action {
        RecordCmd::Start => Some(true),
        RecordCmd::Stop => Some(false),
        RecordCmd::Status => None,
        RecordCmd::Tune { files } => {
            let files = if files.is_empty() {
                let dir = flowbuddy_core::paths::recordings_dir();
                let mut all: Vec<_> = std::fs::read_dir(&dir)
                    .map(|d| d.flatten().map(|e| e.path()).collect())
                    .unwrap_or_default();
                all.retain(|p: &std::path::PathBuf| p.extension().is_some_and(|x| x == "jsonl"));
                all.sort();
                all
            } else {
                files
            };
            if files.is_empty() {
                println!(
                    "No recordings yet: set [record] keep_days = 7, or run flowbuddy record start."
                );
                return Ok(());
            }
            let sessions: Vec<_> = files
                .iter()
                .filter_map(|f| session::parse(&std::fs::read_to_string(f).ok()?).ok())
                .collect();
            print!("{}", tune_text(&session::tune(&sessions), sessions.len()));
            return Ok(());
        }
        RecordCmd::Check { file, expect } => {
            let text = std::fs::read_to_string(&file)?;
            let entries = session::parse(&text).map_err(anyhow::Error::msg)?;
            let found = session::counts(&session::replay(&entries));
            let expected: std::collections::BTreeMap<String, u32> = match expect {
                Some(path) => serde_json::from_str(&std::fs::read_to_string(path)?)?,
                None => {
                    // Counts only, ready to check and save as the expectation.
                    println!("{}", serde_json::to_string_pretty(&found)?);
                    return Ok(());
                }
            };
            let mut wrong = 0;
            for (rule, s) in session::score(&expected, &found) {
                wrong += s.extra + s.missed;
                println!(
                    "{rule:<28} {} right, {} extra, {} missed",
                    s.hits, s.extra, s.missed
                );
            }
            if wrong > 0 {
                anyhow::bail!("{wrong} findings differ from what was expected");
            }
            return Ok(());
        }
    };
    let mut client = Client::connect(ClientKind::Cli)?;
    let r: Recording = serde_json::from_value(client.query(Command::Record { on })?)?;
    match (on, r.recording, r.path) {
        (Some(true), _, Some(p)) => {
            println!("recording to {p}");
            println!(
                "only what the rules see is kept: commands, Insert-mode key kinds, motion counts."
            );
            println!("stop with: flowbuddy record stop");
        }

        (Some(false), _, Some(p)) => {
            println!("saved {p}\ncheck it with: flowbuddy record check {p}")
        }
        (_, true, Some(p)) => println!("recording to {p}"),
        _ => println!("not recording"),
    }
    Ok(())
}

fn tune_text(
    rows: &[(String, u32, Vec<flowbuddy_core::session::TuneRow>)],
    sessions: usize,
) -> String {
    let mut out = format!(
        "Over {sessions} recording(s): tips each threshold would give, and how many you marked dismissed or not helpful.\n"
    );
    for (rule, now, values) in rows {
        out.push_str(&format!("\n{rule}\n"));
        for r in values {
            let mark = if r.threshold == *now { "  (now)" } else { "" };
            out.push_str(&format!(
                "  {:>4}  {:>4} tips, {:>3} unwanted{mark}\n",
                r.threshold, r.tips, r.unwanted
            ));
        }
        let current = values.iter().find(|r| r.threshold == *now);
        // Most tips unwanted, and a higher threshold drops them: say so.
        if let Some(c) = current.filter(|c| c.unwanted * 2 > c.tips && c.tips >= 3)
            && let Some(better) = values
                .iter()
                .find(|r| r.threshold > *now && r.unwanted * 2 <= r.tips.max(1))
        {
            out.push_str(&format!(
                "  → {} of {} tips were unwanted; try [rules.\"{rule}\"] threshold = {} in config.toml\n",
                c.unwanted, c.tips, better.threshold
            ));
        }
    }
    out
}

fn status(watch: bool) -> anyhow::Result<()> {
    let mut client = match Client::connect(ClientKind::Cli) {
        Ok(c) => c,
        Err(e) => {
            println!("flowbuddyd: not running");
            println!("  {e:#}");
            println!("  start it with: systemctl --user start flowbuddyd");
            std::process::exit(3);
        }
    };
    if !watch {
        let s: Status = serde_json::from_value(client.query(Command::Status)?)?;
        print_status(&s);
        return Ok(());
    }
    loop {
        let s: Status = serde_json::from_value(client.query(Command::Status)?)?;
        // Clear the screen and redraw in place.
        print!("\x1b[H\x1b[2J");
        print_status(&s);
        println!("\n  watching; switch windows to see the context change, Ctrl-C to stop");
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

fn print_status(s: &Status) {
    println!("flowbuddyd {}: running", s.daemon_version);
    let paused = s
        .paused_until
        .map(|t| format!(" (paused until {})", format_time(t)))
        .unwrap_or_default();
    println!("  mode:      {}{paused}", s.mode.as_str());
    println!("  focused:   {}", s.app.as_deref().unwrap_or("nothing"));
    println!("  context:   {}", context_line(s));
    if let Some(guess) = s.mode_guess.and_then(|g| g.describe()) {
        println!("  guessing:  {guess}");
    }
    if let Some(timing) = &s.timing {
        println!("  handling:  {timing}");
    }
    println!("  keyboards: {}", s.keyboards);
    if let Some(filter) = &s.tip_filter {
        println!("  filter:    {filter}");
    }
    if let Some(layout) = &s.layout {
        match &s.layout_note {
            Some(note) => println!("  layout:    {layout} ({note})"),
            None => println!("  layout:    {layout}"),
        }
    }
    if s.keyboards == 0 {
        println!("             no readable keyboard: run scripts/install.sh --with-udev,");
        println!("             or see docs/privacy.md#keyboard-access");
    }
    let clients: Vec<String> = s
        .clients
        .iter()
        .map(|c| {
            serde_json::to_value(c)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    println!(
        "  clients:   {}",
        if clients.is_empty() {
            "none".into()
        } else {
            clients.join(", ")
        }
    );
}

fn context_line(s: &Status) -> String {
    if s.context == "unsupported" {
        "unsupported (not watching)".into()
    } else if s.mode_from_editor {
        format!("{} (mode from the editor)", s.context)
    } else if s.confident {
        s.context.clone()
    } else {
        format!("{} (mode unknown until you press Esc)", s.context)
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Open the database read-only; `None` if nothing was ever recorded.
fn open_db() -> anyhow::Result<Option<flowbuddy_store::Store>> {
    let path = flowbuddy_core::paths::database();
    if !path.exists() {
        return Ok(None);
    }
    let store = flowbuddy_store::Store::open_read_only(&path)?;
    if !store.is_current() {
        anyhow::bail!(
            "the daemon hasn't updated the database yet: restart it after updating \
             (systemctl --user restart flowbuddyd)"
        );
    }
    Ok(Some(store))
}

fn summary(week: bool, json: bool) -> anyhow::Result<()> {
    let Some(store) = open_db()? else {
        nothing_yet(json);
        return Ok(());
    };
    let period = if week {
        report::Period::Week
    } else {
        report::Period::Today
    };
    let text = report::summary(&store, now_ms(), period)?;
    if json {
        print_json(&serde_json::json!({ "version": 1, "text": text }));
    } else {
        print!("{text}");
    }
    Ok(())
}

fn coach(question: &str, show_prompt: bool) -> anyhow::Result<()> {
    let store = match open_db()? {
        Some(s) => s,
        None => flowbuddy_store::Store::open_in_memory()?,
    };
    let now = now_ms();
    let week = flowbuddy_store::days::range(now, 7, 0);
    let describe = |id: &str| Some(report::describe(id));
    let export = flowbuddy_store::Export::build(&store, &describe, (&week.0, &week.1), now / 1000)?;
    let config = flowbuddy_core::config::Config::load(&flowbuddy_core::paths::config_file())
        .map_err(anyhow::Error::msg)?;
    // Code around recent tips, only if [text] allows it for this provider.
    let snippets = if flowbuddy_core::text::snippets_may_go_to(&config.text, &config.ai) {
        Client::connect(ClientKind::Cli)
            .and_then(|mut c| c.query(Command::Snippets))
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    } else {
        if config.text.ai_snippets && !show_prompt {
            eprintln!(
                "flowbuddy: not sending code snippets to a cloud provider; set [text] cloud = true to allow it"
            );
        }
        Vec::new()
    };
    let prompt = coach::prompt_with(
        &export,
        Some(question).filter(|q| !q.trim().is_empty()),
        &snippets,
    );
    if show_prompt {
        print!("{prompt}");
        return Ok(());
    }
    match coach::ask(&config.ai, &prompt, &mut std::io::stdout()) {
        None => print!("{}", coach::SETUP),
        Some(result) => {
            result.map_err(|e| anyhow::anyhow!("{e}"))?;
            println!();
        }
    }
    Ok(())
}

/// The weekly progress note, by the model when `[ai] weekly` is on, with
/// the fixed note as the fallback. Sent as a desktop notification.
fn weekly_note() -> anyhow::Result<()> {
    let Some(store) = open_db()? else {
        return Ok(());
    };
    let now = now_ms();
    let describe = |id: &str| report::describe(id).to_lowercase();
    let config = flowbuddy_core::config::Config::load(&flowbuddy_core::paths::config_file())
        .unwrap_or_default();
    let state = flowbuddy_core::config::State::load(&flowbuddy_core::paths::state_file());
    let mut skip = state.muted();
    skip.extend(flowbuddy_core::config::rule_settings(&config, &state).disabled);
    let focus = report::focus(&store, now, &skip)?;
    let Some(fixed) = flowbuddy_store::week_summary(&store, now, &describe, focus.as_deref())
    else {
        return Ok(());
    };
    let mut text = fixed.clone();
    if config.ai.weekly {
        let week = flowbuddy_store::days::range(now, 7, 0);
        let full = |id: &str| Some(report::describe(id));
        let export = flowbuddy_store::Export::build(&store, &full, (&week.0, &week.1), now / 1000)?;
        let mut answer = Vec::new();
        match coach::ask(
            &config.ai,
            &coach::weekly_prompt(&export, &fixed),
            &mut answer,
        ) {
            Some(Ok(())) => {
                if let Some(t) = coach::notification_text(&String::from_utf8_lossy(&answer)) {
                    text = t;
                }
            }
            Some(Err(e)) => {
                eprintln!("flowbuddy: the model didn't write the note ({e}); sending the usual one")
            }
            None => {}
        }
        // And a review of the tips, ready for `flowbuddy review`.
        match review_prompt(&store, &config, now).and_then(|p| run_review(&config.ai, &p)) {
            Ok(Some(parsed)) if !parsed.proposals.is_empty() => {
                let n = parsed.proposals.len();
                text.push_str(&format!(
                    " Your AI also suggests {n} change{} to your tips: run `flowbuddy review`.",
                    if n == 1 { "" } else { "s" }
                ));
            }
            Ok(_) => {}
            Err(e) => eprintln!("flowbuddy: no review this week ({e})"),
        }
    }
    std::process::Command::new("notify-send")
        .args([
            "--app-name=Flow Buddy",
            "--expire-time=20000",
            "Flow Buddy: your week",
        ])
        .arg(flowbuddy_core::glossary::plain(&text))
        .status()?;
    Ok(())
}

fn explain_last() -> anyhow::Result<()> {
    use flowbuddy_core::glossary::{explain as pieces, plain};
    use flowbuddy_core::ipc::LastSlow;
    let mut client = Client::connect(ClientKind::Cli)?;
    let last: Option<LastSlow> = serde_json::from_value(client.query(Command::LastSlow)?)?;
    let Some(last) = last else {
        println!("No slow move or edit since the daemon started.");
        return Ok(());
    };
    print!("{}", last_slow_text(&last, now_ms()));
    let marked: Vec<String> = last
        .best
        .iter()
        .map(|(k, _)| flowbuddy_core::motion::marked(k))
        .collect();
    for line in pieces(&marked.join(" "), false) {
        println!("  {}", plain(&line));
    }
    Ok(())
}

fn last_slow_text(last: &flowbuddy_core::ipc::LastSlow, now_ms: u64) -> String {
    let ago = now_ms.saturating_sub(last.at_ms) / 1000;
    let when = match ago {
        0..60 => format!("{ago} s ago"),
        60..3600 => format!("{} min ago", ago / 60),
        _ => format!("{} h ago", ago / 3600),
    };
    let mut out = format!(
        "Your last slow {} ({when}) took {} keys.\n",
        last.kind, last.keys_used
    );
    for (i, (keys, cost)) in last.best.iter().enumerate() {
        let lead = if i == 0 { "Best:" } else { "Or:  " };
        out.push_str(&format!("  {lead} {keys}  ({cost} keys)\n"));
    }
    out
}

fn explain(keys: &str, insert: bool) {
    let pieces = flowbuddy_core::glossary::explain_keys(keys, insert);
    if pieces.is_empty() {
        println!("Not sure what {keys} does.");
    }
    let width = pieces
        .iter()
        .map(|(p, _)| p.chars().count())
        .max()
        .unwrap_or(0);
    for (piece, meaning) in pieces {
        println!("{piece:<width$}  {meaning}");
    }
}

/// Where today's model-written tips are kept for the daemon.
fn tips_file() -> std::path::PathBuf {
    flowbuddy_core::paths::state_file().with_file_name("tips.json")
}

fn write_tips() -> anyhow::Result<()> {
    let config = flowbuddy_core::config::Config::load(&flowbuddy_core::paths::config_file())
        .unwrap_or_default();
    if !config.ai.tips {
        return Ok(());
    }
    let Some(store) = open_db()? else {
        return Ok(());
    };
    let now = now_ms();
    let week = flowbuddy_store::days::range(now, 14, 0);
    let describe = |id: &str| Some(report::describe(id));
    let export = flowbuddy_store::Export::build(&store, &describe, (&week.0, &week.1), now / 1000)?;
    let mut rules: Vec<(String, String)> = export
        .patterns
        .iter()
        .filter(|p| p.last_7_days.hits > 0)
        .map(|p| (p.rule.clone(), p.description.clone()))
        .collect();
    rules.dedup_by(|a, b| a.0 == b.0);
    rules.truncate(8);
    if rules.is_empty() {
        return Ok(());
    }
    let prompt = tips::prompt(&coach::facts(&export), &rules);
    let mut answer = Vec::new();
    match coach::ask(&config.ai, &prompt, &mut answer) {
        None => return Ok(()),
        Some(r) => r.map_err(|e| anyhow::anyhow!("{e}"))?,
    }
    let (cache, dropped) =
        tips::parse(&String::from_utf8_lossy(&answer), now / 1000).map_err(anyhow::Error::msg)?;
    for why in dropped {
        eprintln!("flowbuddy: left out a tip ({why})");
    }
    let path = tips_file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&cache)?)?;
    println!("{} personal tips written", cache.tips.len());
    Ok(())
}

/// Where the last review's proposals are kept until applied.
fn review_file() -> std::path::PathBuf {
    flowbuddy_core::paths::state_file().with_file_name("review.json")
}

fn review(show_prompt: bool) -> anyhow::Result<()> {
    let Some(store) = open_db()? else {
        println!("Nothing recorded yet. Is flowbuddyd running? Try `flowbuddy status`.");
        return Ok(());
    };
    let config = flowbuddy_core::config::Config::load(&flowbuddy_core::paths::config_file())
        .unwrap_or_default();
    let prompt = review_prompt(&store, &config, now_ms())?;
    if show_prompt {
        print!("{prompt}");
        return Ok(());
    }
    let Some(parsed) = run_review(&config.ai, &prompt)? else {
        print!("{}", coach::SETUP);
        return Ok(());
    };
    if parsed.proposals.is_empty() {
        println!("No changes proposed: the tips look right as they are.");
    } else {
        println!("Proposed changes:");
        for (i, p) in parsed.proposals.iter().enumerate() {
            println!("  {}. {}", i + 1, p.describe());
        }
        println!("\nApply some with: flowbuddy review --apply 1 2 ...");
    }
    for why in &parsed.dropped {
        println!("  (left out: {why})");
    }
    Ok(())
}

/// The review prompt from the store and the current settings.
fn review_prompt(
    store: &flowbuddy_store::Store,
    config: &flowbuddy_core::config::Config,
    now: u64,
) -> anyhow::Result<String> {
    use std::collections::BTreeMap;
    let week = flowbuddy_store::days::range(now, 7, 0);
    let describe = |id: &str| Some(report::describe(id));
    let export = flowbuddy_store::Export::build(store, &describe, (&week.0, &week.1), now / 1000)?;
    let tips = store.tips((now / 1000).saturating_sub(30 * 86_400))?;
    let stats = review::tip_stats(&tips);
    let state = flowbuddy_core::config::State::load(&flowbuddy_core::paths::state_file());
    let mut engine = flowbuddy_core::engine::Engine::standard();
    engine.apply(&flowbuddy_core::config::rule_settings(config, &state));
    let muted = state.muted();
    let settings: BTreeMap<String, review::RuleSetting> = engine
        .rules()
        .map(|r| {
            (
                r.id().to_string(),
                review::RuleSetting {
                    threshold: r.threshold(),
                    default: engine.default_threshold(r.id()),
                    muted: muted.contains(r.id()),
                },
            )
        })
        .collect();
    Ok(review::prompt(&coach::facts(&export), &stats, &settings))
}

/// Ask the AI for a review and keep its valid proposals for `--apply`.
/// `None` if no AI is set up.
fn run_review(
    ai: &flowbuddy_core::config::AiConfig,
    prompt: &str,
) -> anyhow::Result<Option<review::Parsed>> {
    let mut answer = Vec::new();
    match coach::ask(ai, prompt, &mut answer) {
        None => return Ok(None),
        Some(r) => r.map_err(|e| anyhow::anyhow!("{e}"))?,
    }
    let parsed = review::parse(&String::from_utf8_lossy(&answer)).map_err(anyhow::Error::msg)?;
    let path = review_file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&parsed.proposals)?)?;
    Ok(Some(parsed))
}

fn review_apply(numbers: &[usize]) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(review_file())
        .map_err(|_| anyhow::anyhow!("no review yet: run `flowbuddy review` first"))?;
    let proposals: Vec<review::Proposal> = serde_json::from_str(&text)?;
    let mut client = Client::connect(ClientKind::Cli)?;
    for &n in numbers {
        let Some(p) = n.checked_sub(1).and_then(|i| proposals.get(i)) else {
            println!("{n}: no such proposal");
            continue;
        };
        let command = match p {
            review::Proposal::Threshold { rule, value, .. } => Command::RuleThreshold {
                rule_id: rule.clone(),
                threshold: Some(*value),
            },
            review::Proposal::Mute { rule, .. } => Command::Rule {
                rule_id: rule.clone(),
                action: RuleAction::Mute,
            },
            review::Proposal::Unmute { rule, .. } => Command::Rule {
                rule_id: rule.clone(),
                action: RuleAction::Unmute,
            },
        };
        client.command(command)?;
        println!("{n}: done ({})", p.describe());
    }
    Ok(())
}

/// An ssh config block forwarding the daemon's socket to `host`, where
/// the Vim plugin looks for it (`/tmp/flowbuddy-$USER.sock`). sshd creates
/// the remote socket readable only by you (its StreamLocalBindMask 0177).
fn ssh_config(host: &str, socket: &std::path::Path) -> String {
    format!(
        "# Add to ~/.ssh/config, and install the Vim plugin on {host}:\n\
         Host {host}\n    \
         RemoteForward /tmp/flowbuddy-%r.sock {}\n    \
         StreamLocalBindUnlink yes\n",
        socket.display()
    )
}

/// No database yet: the daemon hasn't run.
fn nothing_yet(json: bool) {
    if json {
        print_json(&serde_json::json!({ "version": 1, "nothing_yet": true }));
    } else {
        println!("Nothing recorded yet. Is flowbuddyd running? Try `flowbuddy status`.");
    }
}

fn print_json(v: &serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

/// Which commands print `--json`.
fn takes_json(cmd: &Cmd) -> bool {
    match cmd {
        Cmd::Doctor | Cmd::Summary { .. } | Cmd::Rules { action: None } => true,
        Cmd::Rules { action: Some(a) } => matches!(a, RulesCmd::List),
        Cmd::Learn {
            know,
            forget,
            reset,
            list,
            audit,
            ..
        } => know.is_empty() && forget.is_empty() && reset.is_empty() && !list && !audit,
        Cmd::Config { action } => matches!(action, None | Some(config_cmd::ConfigCmd::List { .. })),
        _ => false,
    }
}

fn coverage_json() -> serde_json::Value {
    use flowbuddy_core::inventory;
    let modes: serde_json::Map<String, serde_json::Value> = inventory::coverage()
        .into_iter()
        .map(|(mode, t)| {
            (
                mode.to_string(),
                serde_json::json!({
                    "commands": t.commands, "coached": t.coached, "taught": t.taught,
                    "starting": t.starting, "skipped": t.skipped, "missing": t.missing,
                }),
            )
        })
        .collect();
    let missing: Vec<serde_json::Value> = inventory::gaps()
        .iter()
        .map(|e| serde_json::json!({"mode": e.mode, "keys": e.keys, "what": e.what}))
        .collect();
    serde_json::json!({ "version": 1, "modes": modes, "missing": missing })
}

fn learn_coverage() -> String {
    use flowbuddy_core::inventory;
    let mut out = String::from("Vim's command index (:help index)\n\n");
    out += &format!(
        "{:<10} {:>8} {:>8} {:>8} {:>9} {:>8} {:>8}\n",
        "mode", "commands", "coached", "taught", "starting", "skipped", "missing"
    );
    for (mode, t) in inventory::coverage() {
        out += &format!(
            "{mode:<10} {:>8} {:>8} {:>8} {:>9} {:>8} {:>8}\n",
            t.commands, t.coached, t.taught, t.starting, t.skipped, t.missing
        );
    }
    let gaps = inventory::gaps();
    if !gaps.is_empty() {
        const SHOWN: usize = 40;
        out += &format!(
            "\nNot covered yet, nor left out on purpose ({}):\n",
            gaps.len()
        );
        for e in gaps.iter().take(SHOWN) {
            out += &format!("  {:<8} {:<7} {}\n", e.keys, e.mode, e.what);
        }
        if gaps.len() > SHOWN {
            out += &format!("  … and {} more\n", gaps.len() - SHOWN);
        }
    }
    out
}

fn learn_report_cmd(json: bool) -> anyhow::Result<()> {
    match open_db()? {
        Some(store) if json => print_json(
            &serde_json::json!({ "version": 1, "text": report::learn_report(&store, now_ms())? }),
        ),
        Some(store) => print!("{}", report::learn_report(&store, now_ms())?),
        None => nothing_yet(json),
    }
    Ok(())
}

fn learn_audit() -> anyhow::Result<()> {
    match open_db()? {
        Some(store) => print!("{}", report::audit(&store, now_ms())?),
        None => println!("Nothing recorded yet."),
    }
    Ok(())
}

fn learn(json: bool) -> anyhow::Result<()> {
    match open_db()? {
        Some(store) => {
            let config =
                flowbuddy_core::config::Config::load(&flowbuddy_core::paths::config_file())
                    .unwrap_or_default();
            let levels = config.learn.level.known_levels();
            if json {
                print_json(&report::learn_json(&store, now_ms(), levels)?);
            } else {
                print!("{}", report::learn(&store, now_ms(), levels)?)
            }
        }
        None => nothing_yet(json),
    }
    Ok(())
}

fn next() -> anyhow::Result<()> {
    let Some(store) = open_db()? else {
        println!("Nothing recorded yet. Is flowbuddyd running? Try `flowbuddy status`.");
        return Ok(());
    };
    let config = flowbuddy_core::config::Config::load(&flowbuddy_core::paths::config_file())
        .unwrap_or_default();
    let state = flowbuddy_core::config::State::load(&flowbuddy_core::paths::state_file());
    let mut skip = state.muted();
    skip.extend(flowbuddy_core::config::rule_settings(&config, &state).disabled);
    print!("{}", report::next(&store, now_ms(), &skip)?);
    Ok(())
}

fn memory(cmd: MemoryCmd) -> anyhow::Result<()> {
    match cmd {
        MemoryCmd::Show => match open_db()? {
            Some(store) => print!("{}", report::memory_show(&store, now_ms())?),
            None => println!("The mistake memory is empty."),
        },
        MemoryCmd::Export => {
            let store = match open_db()? {
                Some(s) => s,
                None => flowbuddy_store::Store::open_in_memory()?,
            };
            let now = now_ms();
            let week = flowbuddy_store::days::range(now, 7, 0);
            let describe = |id: &str| Some(report::describe(id));
            let export =
                flowbuddy_store::Export::build(&store, &describe, (&week.0, &week.1), now / 1000)?;
            println!("{}", serde_json::to_string_pretty(&export)?);
        }
        MemoryCmd::Forget { rule, all, yes } => {
            let what = match (&rule, all) {
                (Some(r), false) => format!("everything remembered about {r}"),
                (None, true) => "ALL stats and the whole mistake memory".to_string(),
                _ => anyhow::bail!("name a rule, or pass --all"),
            };
            let path = flowbuddy_core::paths::database();
            if !path.exists() {
                println!("Nothing to forget.");
                return Ok(());
            }
            if !yes && !confirm(&format!("Delete {what}? This can't be undone. [y/N] "))? {
                println!("Kept.");
                return Ok(());
            }
            let store = flowbuddy_store::Store::open(&path)?;
            match rule {
                Some(r) => {
                    store.forget_rule(&r)?;
                }
                None => store.forget_all()?,
            }
            println!("Deleted {what}.");
        }
        MemoryCmd::FixAdoptions { before, yes } => {
            let path = flowbuddy_core::paths::database();
            if !path.exists() {
                println!("Nothing recorded.");
                return Ok(());
            }
            let store = flowbuddy_store::Store::open(&path)?;
            let found = store.inflated_adoptions(&before)?;
            if found.is_empty() {
                println!("Nothing to fix.");
                return Ok(());
            }
            for d in &found {
                println!(
                    "  {} {} ({}): better way {} → {}",
                    d.day, d.rule_id, d.context, d.adoptions, d.capped
                );
            }
            let days = if found.len() == 1 {
                "this day".to_string()
            } else {
                format!("these {} days", found.len())
            };
            if !yes && !confirm(&format!("Fix {days}? [y/N] "))? {
                println!("Kept.");
                return Ok(());
            }
            store.fix_adoptions(&before)?;
            println!("Fixed.");
        }
    }
    Ok(())
}

fn confirm(prompt: &str) -> anyhow::Result<bool> {
    use std::io::Write;
    print!("{prompt}");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}

fn simple(command: Command, done: &str) -> anyhow::Result<()> {
    Client::connect(ClientKind::Cli)?.command(command)?;
    println!("{done}");
    Ok(())
}

fn set_mode(mode: Option<CoachingMode>) -> anyhow::Result<()> {
    let mut client = Client::connect(ClientKind::Cli)?;
    match mode {
        None => {
            let s: Status = serde_json::from_value(client.query(Command::Status)?)?;
            println!("{}", s.mode.as_str());
        }
        Some(mode) => {
            client.command(Command::SetMode { mode })?;
            println!("mode: {}", mode.as_str());
        }
    }
    Ok(())
}

fn pause(duration: &str) -> anyhow::Result<()> {
    let seconds = parse_duration(duration, chrono::Local::now()).ok_or_else(|| {
        anyhow::anyhow!("can't read {duration:?}; try 30m, 1h, 2h30m or until-tomorrow")
    })?;
    Client::connect(ClientKind::Cli)?.command(Command::Pause { seconds })?;
    let until = chrono::Local::now() + chrono::Duration::seconds(seconds as i64);
    println!("paused until {}", until.format("%a %H:%M"));
    Ok(())
}

/// `30m`, `1h`, `2h30m`, `90s`, or `until-tomorrow` (next local midnight).
fn parse_duration(s: &str, now: chrono::DateTime<chrono::Local>) -> Option<u64> {
    if s == "until-tomorrow" {
        let midnight = (now.date_naive() + chrono::Days::new(1)).and_hms_opt(0, 0, 0)?;
        let midnight = midnight.and_local_timezone(chrono::Local).earliest()?;
        return u64::try_from((midnight - now).num_seconds()).ok();
    }
    let (mut total, mut num) = (0u64, String::new());
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        let n: u64 = std::mem::take(&mut num).parse().ok()?;
        total += n * match c {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86400,
            _ => return None,
        };
    }
    (num.is_empty() && total > 0).then_some(total)
}

fn rules(cmd: RulesCmd, json: bool) -> anyhow::Result<()> {
    let mut client = Client::connect(ClientKind::Cli)?;
    let (rule_id, action) = match cmd {
        RulesCmd::List => {
            let infos: Vec<RuleInfo> = serde_json::from_value(client.query(Command::Rules)?)?;
            if json {
                print_json(&serde_json::json!({ "version": 1, "rules": infos }));
                return Ok(());
            }
            for r in infos {
                let state = match (r.enabled, r.muted) {
                    (false, _) => "disabled",
                    (true, true) => "muted",
                    (true, false) => "on",
                };
                let threshold = match (r.threshold, r.raised) {
                    (Some(n), 0) => format!(" (≥{n})"),
                    (Some(n), up) => {
                        format!(" (≥{n}, raised by {up} after \"not helpful\" reports)")
                    }
                    (None, _) => String::new(),
                };
                println!("{:<22} {:<9} {}{threshold}", r.id, state, r.description);
            }
            return Ok(());
        }
        RulesCmd::Enable { rule } => (rule, RuleAction::Enable),
        RulesCmd::Disable { rule } => (rule, RuleAction::Disable),
        RulesCmd::Mute { rule } => (rule, RuleAction::Mute),
        RulesCmd::Unmute { rule } => (rule, RuleAction::Unmute),
        RulesCmd::Reset { rule } => (rule, RuleAction::Reset),
    };
    client.command(Command::Rule {
        rule_id: rule_id.clone(),
        action,
    })?;
    let done = match action {
        RuleAction::Enable => "enabled",
        RuleAction::Disable => "disabled",
        RuleAction::Mute => "muted",
        RuleAction::Unmute => "unmuted",
        RuleAction::Reset => "threshold back to its default",
    };
    println!("{rule_id}: {done}");
    Ok(())
}

fn ctx(event: CtxCmd, shell_pid: Option<u32>, tty: Option<String>) -> anyhow::Result<()> {
    let shell_pid = shell_pid.unwrap_or_else(parent_pid);
    let msg = match event {
        CtxCmd::Fg { command, shape } => ClientMsg::Ctx {
            event: CtxEvent::Fg,
            // Defence in depth: only ever send a basename, and only a plain
            // program name (never `^old^new` or anything else typed).
            command: Some(command.rsplit('/').next().unwrap_or_default().to_string()).filter(|c| {
                !c.is_empty()
                    && c.len() <= 64
                    && c.chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || "._+-".contains(ch))
            }),
            vi_mode: None,
            shell_pid,
            tty,
            // Only known shapes; anything else is dropped, never sent.
            shape: shape.and_then(|s| serde_json::from_value(serde_json::Value::String(s)).ok()),
            mode: None,
        },
        CtxCmd::Mode { mode } => ClientMsg::Ctx {
            event: CtxEvent::Mode,
            command: None,
            vi_mode: Some(true),
            shell_pid,
            tty,
            shape: None,
            mode: Some(match mode.as_str() {
                "normal" => EditorMode::Normal,
                "visual" => EditorMode::Visual,
                "replace" => EditorMode::Replace,
                _ => EditorMode::Insert,
            }),
        },
        CtxCmd::Prompt { vi_mode } => ClientMsg::Ctx {
            event: CtxEvent::Prompt,
            command: None,
            vi_mode: Some(vi_mode),
            shell_pid,
            tty,
            shape: None,
            mode: None,
        },
    };
    Client::connect(ClientKind::BashHook)?.send(&msg)
}

fn parent_pid() -> u32 {
    std::os::unix::process::parent_id()
}

fn run_setup() -> anyhow::Result<()> {
    let path = flowbuddy_core::paths::config_file();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    println!("A few questions; press Enter to keep the current choice (*).");
    let answers = setup::ask(&text, &mut std::io::stdin().lock(), &mut std::io::stdout())?;
    let written = setup::apply(&text, &answers);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, &written)?;
    println!(
        "\nSaved to {}; the daemon picks it up by itself.",
        path.display()
    );
    if written.contains("provider = \"command\"") && !written.contains("command = [") {
        println!(
            "Add the program to run under [ai], e.g. command = [\"ollama\", \"run\", \"llama3.2:3b\"]."
        );
    }
    println!("flowbuddy doctor checks the rest.");
    Ok(())
}

fn watch() -> anyhow::Result<()> {
    use flowbuddy_core::ipc::{RecentTip, ServerMsg};
    let mut said_waiting = false;
    loop {
        let mut client = match Client::connect(ClientKind::Watch) {
            Ok(c) => c,
            Err(_) => {
                if !said_waiting {
                    println!("Waiting for flowbuddyd…");
                    said_waiting = true;
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
                continue;
            }
        };
        said_waiting = false;
        let recent: Vec<RecentTip> = client
            .query(Command::RecentTips { limit: Some(3) })
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        if !recent.is_empty() {
            println!("Recent tips:\n");
            for t in recent.iter().rev() {
                print!("{}", watch_text(t.shown_at, &t.title, &t.text, &t.explain));
            }
        }
        println!("Watching for tips (Ctrl-C to stop).\n");
        loop {
            match client.next() {
                Ok(ServerMsg::Nudge(n)) => {
                    print!(
                        "{}",
                        watch_text(now_ms() / 1000, &n.title, &n.text, &n.explain)
                    );
                }
                Ok(_) => {}
                Err(_) => {
                    println!("The daemon went away; waiting for it…");
                    break;
                }
            }
        }
    }
}

/// One tip for `flowbuddy watch`: time and title, the tip, what its
/// commands do.
fn watch_text(at_s: u64, title: &str, text: &str, explain: &[String]) -> String {
    use flowbuddy_core::glossary::plain;
    let mut out = format!("{}  {title}\n  {}\n", format_time(at_s), plain(text));
    for line in explain {
        out.push_str(&format!("    {}\n", plain(line)));
    }
    out.push('\n');
    out
}

fn tips_history(search: &str, limit: usize, explain: bool) -> anyhow::Result<()> {
    let Some(store) = open_db()? else {
        println!("No tips yet.");
        return Ok(());
    };
    let tips = store.history(search, limit)?;
    print!("{}", history_text(&tips, search, explain, &format_day_time));
    Ok(())
}

fn history_text(
    tips: &[flowbuddy_store::HistoryTip],
    search: &str,
    explain: bool,
    when: &dyn Fn(u64) -> String,
) -> String {
    use flowbuddy_core::glossary::plain;
    if tips.is_empty() {
        return if search.is_empty() {
            "No tips yet.\n".into()
        } else {
            format!("No tips match \"{search}\".\n")
        };
    }
    let mut out = String::new();
    for t in tips {
        out.push_str(&format!(
            "{}  {}\n    {}\n",
            when(t.shown_at),
            t.title,
            plain(&t.text)
        ));
        if explain {
            for line in flowbuddy_core::glossary::explain(&t.text, t.insert) {
                out.push_str(&format!("      {}\n", plain(&line)));
            }
        }
    }
    out
}

/// `Oct 27 14:05`.
fn format_day_time(unix: u64) -> String {
    std::process::Command::new("date")
        .args(["-d", &format!("@{unix}"), "+%b %d %H:%M"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| unix.to_string())
}

/// Local HH:MM for a unix timestamp, via `date` to avoid a timezone dependency.
fn format_time(unix: u64) -> String {
    std::process::Command::new("date")
        .args(["-d", &format!("@{unix}"), "+%H:%M"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| unix.to_string())
}

#[cfg(test)]
mod tests {
    use super::parse_duration;
    use chrono::TimeZone;

    #[test]
    fn durations() {
        let now = chrono::Local
            .with_ymd_and_hms(2026, 10, 24, 22, 0, 0)
            .unwrap();
        assert_eq!(parse_duration("30m", now), Some(1800));
        assert_eq!(parse_duration("2h30m", now), Some(9000));
        assert_eq!(parse_duration("90s", now), Some(90));
        assert_eq!(parse_duration("until-tomorrow", now), Some(7200));
        assert_eq!(parse_duration("1x", now), None);
        assert_eq!(parse_duration("15", now), None);
        assert_eq!(parse_duration("", now), None);
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;

    fn status(context: &str, confident: bool) -> Status {
        Status {
            running: true,
            daemon_version: "0".into(),
            mode: CoachingMode::Active,
            paused_until: None,
            context: context.into(),
            app: Some("code_code".into()),
            confident,
            mode_from_editor: false,
            layout: None,
            layout_note: None,
            tip_filter: None,
            mode_guess: None,
            timing: None,
            keyboards: 1,
            clients: vec![],
        }
    }

    #[test]
    fn unsupported_does_not_mention_the_mode() {
        assert_eq!(
            context_line(&status("unsupported", false)),
            "unsupported (not watching)"
        );
    }

    #[test]
    fn uncertain_mode_says_how_to_fix_it() {
        assert_eq!(context_line(&status("vscode-vim", true)), "vscode-vim");
        assert!(context_line(&status("vscode-vim", false)).contains("press Esc"));
    }

    #[test]
    fn status_without_app_still_parses() {
        let json = r#"{"running":true,"daemon_version":"0","mode":"active","paused_until":null,
            "context":"unsupported","confident":false,"keyboards":0,"clients":[]}"#;
        let s: Status = serde_json::from_str(json).unwrap();
        assert_eq!(s.app, None);
    }

    #[test]
    fn ssh_config_forwards_the_socket() {
        let out = super::ssh_config(
            "devbox",
            std::path::Path::new("/run/user/1000/flowbuddy.sock"),
        );
        assert!(out.contains("Host devbox\n"), "{out}");
        assert!(
            out.contains("RemoteForward /tmp/flowbuddy-%r.sock /run/user/1000/flowbuddy.sock"),
            "{out}"
        );
        assert!(out.contains("StreamLocalBindUnlink yes"), "{out}");
    }

    #[test]
    fn last_slow_lines() {
        let last = flowbuddy_core::ipc::LastSlow {
            kind: "edit".into(),
            at_ms: 1_000,
            keys_used: 9,
            best: vec![("di(".into(), 3), ("dt<char>".into(), 3)],
        };
        assert_eq!(
            last_slow_text(&last, 121_000),
            "Your last slow edit (2 min ago) took 9 keys.\n  Best: di(  (3 keys)\n  Or:   dt<char>  (3 keys)\n"
        );
    }

    #[test]
    fn tip_history_lines() {
        let tip = flowbuddy_store::HistoryTip {
            shown_at: 5,
            rule_id: "vim.undo-run".into(),
            title: "Undo further at once".into(),
            text: "`u` 7 times. `7u` undoes that many at once.".into(),
            insert: false,
        };
        let when = |t: u64| format!("t{t}");
        assert_eq!(
            history_text(std::slice::from_ref(&tip), "", false, &when),
            "t5  Undo further at once\n    u 7 times. 7u undoes that many at once.\n"
        );
        let explained = history_text(&[tip], "", true, &when);
        assert!(explained.contains("      7: a count"), "{explained}");
        assert_eq!(
            history_text(&[], "undo", false, &when),
            "No tips match \"undo\".\n"
        );
    }

    #[test]
    fn watch_lines() {
        let text = watch_text(0, "Try a count", "Try `7j`.", &["`7j`: a count".into()]);
        assert!(
            text.ends_with("  Try a count\n  Try 7j.\n    7j: a count\n\n"),
            "{text}"
        );
    }

    #[test]
    fn tune_table_and_suggestion() {
        use flowbuddy_core::session::TuneRow;
        let row = |threshold, tips, unwanted| TuneRow {
            threshold,
            tips,
            unwanted,
        };
        let text = tune_text(
            &[(
                "vim.repeat-jk".into(),
                5,
                vec![row(4, 12, 8), row(5, 8, 6), row(6, 4, 1), row(10, 1, 0)],
            )],
            3,
        );
        assert!(
            text.contains("     5     8 tips,   6 unwanted  (now)\n"),
            "{text}"
        );
        assert!(
            text.contains("try [rules.\"vim.repeat-jk\"] threshold = 6"),
            "{text}"
        );
    }
}
