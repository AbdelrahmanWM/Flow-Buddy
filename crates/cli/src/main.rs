mod client;
mod coach;
mod report;

use clap::{Parser, Subcommand};
use flowbuddy_core::ipc::{ClientKind, ClientMsg, Command, CtxEvent, RuleAction, RuleInfo, Status};
use flowbuddy_core::policy::CoachingMode;

use crate::client::Client;

/// Flow Buddy: improve all the time without stopping to study.
#[derive(Parser)]
#[command(name = "flowbuddy", version = flowbuddy_core::VERSION)]
struct Args {
    #[command(subcommand)]
    command: Cmd,
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
    },
    /// What Vim keys do, piece by piece: `flowbuddy explain 'ci"'`.
    Explain {
        keys: String,
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
}

fn main() {
    let args = Args::parse();
    let result = match args.command {
        Cmd::Status { watch } => status(watch),
        Cmd::Rules { action } => rules(action.unwrap_or(RulesCmd::List)),
        Cmd::Mode { mode } => set_mode(mode),
        Cmd::Summary { week } => summary(week),
        Cmd::Next => next(),
        Cmd::Coach {
            weekly_note: true, ..
        } => weekly_note(),
        Cmd::Coach {
            question, prompt, ..
        } => coach(&question.join(" "), prompt),
        Cmd::Explain { keys, insert } => {
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
    println!("  keyboards: {}", s.keyboards);
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
    Ok(Some(flowbuddy_store::Store::open_read_only(&path)?))
}

fn summary(week: bool) -> anyhow::Result<()> {
    let Some(store) = open_db()? else {
        println!("Nothing recorded yet. Is flowbuddyd running? Try `flowbuddy status`.");
        return Ok(());
    };
    let period = if week {
        report::Period::Week
    } else {
        report::Period::Today
    };
    print!("{}", report::summary(&store, now_ms(), period)?);
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
    let prompt = coach::prompt(&export, Some(question).filter(|q| !q.trim().is_empty()));
    if show_prompt {
        print!("{prompt}");
        return Ok(());
    }
    let config = flowbuddy_core::config::Config::load(&flowbuddy_core::paths::config_file())
        .map_err(anyhow::Error::msg)?;
    let Some(command) = config.ai.command.filter(|c| !c.is_empty()) else {
        print!("{}", coach::SETUP);
        return Ok(());
    };
    coach::run(
        &command,
        &prompt,
        config.ai.timeout_seconds,
        &mut std::io::stdout(),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!();
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
    if let (true, Some(command)) = (config.ai.weekly, config.ai.command.as_ref()) {
        let week = flowbuddy_store::days::range(now, 7, 0);
        let full = |id: &str| Some(report::describe(id));
        let export = flowbuddy_store::Export::build(&store, &full, (&week.0, &week.1), now / 1000)?;
        let mut answer = Vec::new();
        match coach::run(
            command,
            &coach::weekly_prompt(&export, &fixed),
            config.ai.timeout_seconds,
            &mut answer,
        ) {
            Ok(()) => {
                if let Some(t) = coach::notification_text(&String::from_utf8_lossy(&answer)) {
                    text = t;
                }
            }
            Err(e) => {
                eprintln!("flowbuddy: the model didn't write the note ({e}); sending the usual one")
            }
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

fn rules(cmd: RulesCmd) -> anyhow::Result<()> {
    let mut client = Client::connect(ClientKind::Cli)?;
    let (rule_id, action) = match cmd {
        RulesCmd::List => {
            let infos: Vec<RuleInfo> = serde_json::from_value(client.query(Command::Rules)?)?;
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
            // Defence in depth: only ever send a basename.
            command: Some(command.rsplit('/').next().unwrap_or_default().to_string()),
            vi_mode: None,
            shell_pid,
            tty,
            // Only known shapes; anything else is dropped, never sent.
            shape: shape.and_then(|s| serde_json::from_value(serde_json::Value::String(s)).ok()),
        },
        CtxCmd::Prompt { vi_mode } => ClientMsg::Ctx {
            event: CtxEvent::Prompt,
            command: None,
            vi_mode: Some(vi_mode),
            shell_pid,
            tty,
            shape: None,
        },
    };
    Client::connect(ClientKind::BashHook)?.send(&msg)
}

fn parent_pid() -> u32 {
    std::os::unix::process::parent_id()
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
}
