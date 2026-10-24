mod client;

use clap::{Parser, Subcommand};
use flowbuddy_core::ipc::{ClientKind, Command, Status};

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
}

fn main() {
    let args = Args::parse();
    let result = match args.command {
        Cmd::Status => status(),
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
