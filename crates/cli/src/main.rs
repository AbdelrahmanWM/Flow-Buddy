mod client;

use clap::{Parser, Subcommand};
use flowbuddy_core::ipc::{ClientKind, ClientMsg, Command, CtxEvent, RuleAction, RuleInfo, Status};

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
    Status,
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
}

#[derive(Subcommand)]
enum CtxCmd {
    /// A command started; pass only its name.
    Fg { command: String },
    /// The prompt is back.
    Prompt {
        #[arg(long, action = clap::ArgAction::Set, default_value_t = false)]
        vi_mode: bool,
    },
}

fn main() {
    let args = Args::parse();
    let result = match args.command {
        Cmd::Status => status(),
        Cmd::Rules { action } => rules(action.unwrap_or(RulesCmd::List)),
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

fn status() -> anyhow::Result<()> {
    let mut client = match Client::connect(ClientKind::Cli) {
        Ok(c) => c,
        Err(e) => {
            println!("flowbuddyd: not running");
            println!("  {e:#}");
            println!("  start it with: systemctl --user start flowbuddyd");
            std::process::exit(3);
        }
    };
    let s: Status = serde_json::from_value(client.query(Command::Status)?)?;
    println!("flowbuddyd {}: running", s.daemon_version);
    let paused = s
        .paused_until
        .map(|t| format!(" (paused until {})", format_time(t)))
        .unwrap_or_default();
    println!("  mode:      {}{paused}", s.mode.as_str());
    println!(
        "  context:   {}{}",
        s.context,
        if s.confident { "" } else { " (mode uncertain)" }
    );
    println!("  keyboards: {}", s.keyboards);
    if s.keyboards == 0 {
        println!("             no readable keyboard: see docs/privacy.md#keyboard-access");
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
    Ok(())
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
                let threshold = r.threshold.map(|n| format!(" (≥{n})")).unwrap_or_default();
                println!("{:<22} {:<9} {}{threshold}", r.id, state, r.description);
            }
            return Ok(());
        }
        RulesCmd::Enable { rule } => (rule, RuleAction::Enable),
        RulesCmd::Disable { rule } => (rule, RuleAction::Disable),
        RulesCmd::Mute { rule } => (rule, RuleAction::Mute),
        RulesCmd::Unmute { rule } => (rule, RuleAction::Unmute),
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
    };
    println!("{rule_id}: {done}");
    Ok(())
}

fn ctx(event: CtxCmd, shell_pid: Option<u32>, tty: Option<String>) -> anyhow::Result<()> {
    let shell_pid = shell_pid.unwrap_or_else(parent_pid);
    let msg = match event {
        CtxCmd::Fg { command } => ClientMsg::Ctx {
            event: CtxEvent::Fg,
            // Defence in depth: only ever send a basename.
            command: Some(command.rsplit('/').next().unwrap_or_default().to_string()),
            vi_mode: None,
            shell_pid,
            tty,
        },
        CtxCmd::Prompt { vi_mode } => ClientMsg::Ctx {
            event: CtxEvent::Prompt,
            command: None,
            vi_mode: Some(vi_mode),
            shell_pid,
            tty,
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
