//! `flowbuddy config`: every setting from the command line. What there is
//! comes from `flowbuddy_core::settings`, so this and the settings window
//! agree.

use std::path::Path;

use clap::Subcommand;
use flowbuddy_core::config::Config;
use flowbuddy_core::settings::{self, Kind, Setting};
use serde_json::json;

#[derive(Subcommand)]
pub enum ConfigCmd {
    /// Every setting: its value, its default, and what it does.
    List {
        /// As JSON, for scripts and the settings window.
        #[arg(long)]
        json: bool,
        /// Only the basics.
        #[arg(long)]
        basic: bool,
    },
    /// One setting's value: `flowbuddy config get learn.daily`.
    Get { key: String },
    /// What a setting does, at more length: `flowbuddy config explain coaching.hold_ms`.
    Explain { key: String },
    /// Change a setting: `flowbuddy config set coaching.quiet_hours 22:00-08:00`.
    /// Lists are comma-separated.
    Set { key: String, value: String },
    /// Back to the default: one setting, or with --all, every one.
    Reset {
        key: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// Open the config file in $EDITOR.
    Edit,
    /// Start from a profile: with no name, list them; with one, show what
    /// it changes and apply it (`flowbuddy config profile gentle`).
    Profile {
        name: Option<String>,
        /// Only show what it would change.
        #[arg(long)]
        dry_run: bool,
    },
}

/// Run `cmd`; `set_mode` switches the running daemon's mode (a profile
/// changes it now, not only at the next start).
pub fn run(cmd: ConfigCmd, path: &Path, set_mode: &dyn Fn(&str)) -> anyhow::Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => anyhow::bail!("{}: {e}", path.display()),
    };
    let config: Config =
        toml::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    match cmd {
        ConfigCmd::List { json, basic } => {
            if json {
                println!("{}", serde_json::to_string_pretty(&list_json(&config))?);
            } else {
                print!("{}", list_text(&config, basic));
            }
        }
        ConfigCmd::Get { key } => {
            settings::find(&key).ok_or_else(|| anyhow::anyhow!("no setting called {key}"))?;
            println!("{}", settings::show(&settings::value(&config, &key)));
        }
        ConfigCmd::Explain { key } => {
            let s =
                settings::find(&key).ok_or_else(|| anyhow::anyhow!("no setting called {key}"))?;
            let value = settings::show(&settings::value(&config, &key));
            let default = settings::show(&settings::value(&Config::default(), &key));
            println!("{key}: {}\n", s.what);
            if let Some(more) = settings::more(&key) {
                println!("{more}\n");
            }
            let shown = |v: &str| {
                if v.is_empty() {
                    "(not set)".to_string()
                } else {
                    v.to_string()
                }
            };
            println!("Now: {}   Default: {}", shown(&value), shown(&default));
            if let Some(why) = s.notice {
                println!("Turning it on: {why}");
            }
        }
        ConfigCmd::Set { key, value } => {
            let out = settings::set(&text, &key, &value).map_err(anyhow::Error::msg)?;
            write(path, &out)?;
            if let Some(why) = settings::find(&key)
                .and_then(|s| s.notice)
                .filter(|_| turns_on(&value))
            {
                println!("Note: {why}");
            }
            let now: Config = toml::from_str(&out)?;
            println!("{key} = {}", settings::show(&settings::value(&now, &key)));
        }
        ConfigCmd::Reset { key, all } => {
            let out = match (key, all) {
                (Some(key), false) => {
                    let out = settings::reset(&text, &key).map_err(anyhow::Error::msg)?;
                    println!("{key} is back to its default.");
                    out
                }
                (None, true) => {
                    let mut out = text.clone();
                    for s in settings::SETTINGS {
                        out = settings::reset(&out, s.key).map_err(anyhow::Error::msg)?;
                    }
                    println!("Every setting is back to its default (rules and comments are kept).");
                    out
                }
                _ => anyhow::bail!("name a setting, or --all"),
            };
            write(path, &out)?;
        }
        ConfigCmd::Profile { name: None, .. } => {
            let current = settings::current_profile(&config).map(|p| p.id);
            for p in settings::PROFILES {
                let mark = if Some(p.id) == current {
                    "  (yours)"
                } else {
                    ""
                };
                println!("{:<14} {}{mark}\n               {}", p.id, p.name, p.what);
            }
            if current.is_none() {
                println!("\nYour settings match none of them: they're your own.");
            }
            println!("\nSee what one changes: flowbuddy config profile <name> --dry-run");
        }
        ConfigCmd::Profile {
            name: Some(name),
            dry_run,
        } => {
            let profile = settings::profile(&name).ok_or_else(|| {
                let ids: Vec<&str> = settings::PROFILES.iter().map(|p| p.id).collect();
                anyhow::anyhow!("no profile called {name}: {}", ids.join(", "))
            })?;
            let changes = settings::profile_changes(&config, profile);
            if changes.is_empty() {
                println!("Your settings already match {}.", profile.name);
                return Ok(());
            }
            println!("{} changes:", profile.name);
            for (key, now, then) in &changes {
                let now = if now.is_empty() { "(not set)" } else { now };
                println!("  {key:<36} {now} → {then}");
            }
            if dry_run {
                return Ok(());
            }
            let out = settings::apply_profile(&text, profile).map_err(anyhow::Error::msg)?;
            write(path, &out)?;
            if let Some((_, mode)) = profile
                .values
                .iter()
                .find(|(k, _)| *k == "coaching.default_mode")
            {
                set_mode(mode);
            }
            println!("Done. Change any of these as you like: they're ordinary settings.");
        }
        ConfigCmd::Edit => {
            if !path.exists() {
                write(path, "")?;
            }
            let editor = std::env::var("VISUAL")
                .or_else(|_| std::env::var("EDITOR"))
                .unwrap_or_else(|_| "vi".into());
            std::process::Command::new(editor).arg(path).status()?;
            if let Err(e) = Config::load(path) {
                eprintln!("The file doesn't load now: {e}");
            }
        }
    }
    Ok(())
}

/// Whether `value` switches something on: `true`, a number above zero, a
/// non-empty list.
fn turns_on(value: &str) -> bool {
    let v = value.trim().to_ascii_lowercase();
    !matches!(v.as_str(), "" | "false" | "off" | "no" | "0" | "none")
}

/// Atomically: a temp file, then a rename. The daemon reloads by itself.
fn write(path: &Path, text: &str) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

fn kind_json(kind: Kind) -> serde_json::Value {
    match kind {
        Kind::Int { min, max } => json!({"type": "int", "min": min, "max": max}),
        Kind::Float { min, max } => json!({"type": "float", "min": min, "max": max}),
        Kind::Choice(c) => json!({"type": "choice", "choices": c}),
        k => json!({"type": k.name()}),
    }
}

fn setting_json(s: &Setting, key: &str, config: &Config) -> serde_json::Value {
    json!({
        "key": key,
        "kind": kind_json(s.kind),
        "group": s.group,
        "basic": s.basic,
        "what": s.what,
        "more": settings::more(key),
        "notice": s.notice,
        "value": settings::value(config, key),
        "default": settings::value(&Config::default(), key),
    })
}

/// The settings, then every rule's, as the settings window reads them.
pub fn list_json(config: &Config) -> serde_json::Value {
    let mut out: Vec<serde_json::Value> = settings::SETTINGS
        .iter()
        .map(|s| setting_json(s, s.key, config))
        .collect();
    let engine = flowbuddy_core::engine::Engine::standard();
    for rule in engine.rules() {
        for field in ["enabled", "threshold"] {
            let key = format!("rules.{}.{field}", rule.id());
            if let Some(s) = settings::find(&key) {
                let mut v = setting_json(&s, &key, config);
                v["rule"] = json!({
                    "id": rule.id(),
                    "title": rule.title(),
                    "description": rule.description(),
                    "default_threshold": engine.default_threshold(rule.id()),
                });
                out.push(v);
            }
        }
    }
    let current = settings::current_profile(config).map(|p| p.id);
    let profiles: Vec<serde_json::Value> = settings::PROFILES
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "name": p.name,
                "what": p.what,
                "current": Some(p.id) == current,
                "changes": settings::profile_changes(config, p)
                    .into_iter()
                    .map(|(key, now, then)| json!({"key": key, "now": now, "then": then}))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({ "version": 1, "settings": out, "profiles": profiles })
}

fn list_text(config: &Config, basic_only: bool) -> String {
    let mut out = String::new();
    let shown: Vec<&Setting> = settings::SETTINGS
        .iter()
        .filter(|s| s.basic || !basic_only)
        .collect();
    // Each group once, in the order it first comes.
    let mut groups: Vec<&str> = Vec::new();
    for s in &shown {
        if !groups.contains(&s.group) {
            groups.push(s.group);
        }
    }
    let ordered = groups
        .iter()
        .flat_map(|g| shown.iter().filter(move |s| s.group == *g));
    let mut group = "";
    for s in ordered {
        if s.group != group {
            group = s.group;
            out += &format!("\n{group}\n");
        }
        let value = settings::show(&settings::value(config, s.key));
        let default = settings::show(&settings::value(&Config::default(), s.key));
        let shown = if value.is_empty() {
            "(not set)".into()
        } else {
            value.clone()
        };
        let changed = if value != default { "  *" } else { "" };
        out += &format!("  {:<36} {shown}{changed}\n      {}", s.key, s.what);
        match s.kind {
            Kind::Choice(c) => out += &format!(" [{}]", c.join(", ")),
            Kind::Int { min, max } => out += &format!(" [{min}–{max}]"),
            Kind::Float { min, max } => out += &format!(" [{min}–{max}]"),
            _ => {}
        }
        out.push('\n');
    }
    if basic_only {
        out += "\nThe rest: flowbuddy config list.";
    } else {
        out += "\nRules: flowbuddy config set rules.<id>.enabled false, or rules.<id>.threshold 8 (ids from `flowbuddy rules`).";
    }
    out += "\n* changed from the default. Change one with `flowbuddy config set <key> <value>`; more about one with `flowbuddy config explain <key>`.\n";
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_lists_every_setting_and_rule() {
        let v = list_json(&Config::default());
        let all = v["settings"].as_array().unwrap();
        assert!(all.len() > settings::SETTINGS.len());
        let holdout = all.iter().find(|s| s["key"] == "learn.holdout").unwrap();
        assert_eq!(holdout["kind"]["type"], "float");
        assert_eq!(holdout["value"], json!(0.1));
        let jk = all
            .iter()
            .find(|s| s["key"] == "rules.vim.repeat-jk.threshold")
            .unwrap();
        assert!(jk["rule"]["default_threshold"].as_u64().is_some());
        let text = list_text(&Config::default(), true);
        assert!(text.contains("learn.daily") && !text.contains("learn.holdout"));
    }

    #[test]
    fn set_writes_and_keeps_comments() {
        let dir = std::env::temp_dir().join(format!("fb-config-{}", std::process::id()));
        let path = dir.join("config.toml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "# mine\n").unwrap();
        run(
            ConfigCmd::Set {
                key: "learn.holdout".into(),
                value: "0".into(),
            },
            &path,
            &|_| {},
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with("# mine\n") && text.contains("holdout = 0.0"),
            "{text}"
        );
        assert!(
            run(
                ConfigCmd::Set {
                    key: "learn.holdout".into(),
                    value: "lots".into(),
                },
                &path,
                &|_| {},
            )
            .is_err()
        );
        run(
            ConfigCmd::Reset {
                key: None,
                all: true,
            },
            &path,
            &|_| {},
        )
        .unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("holdout"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
