//! Every setting in `config.toml`, described: what it is, what values it
//! takes, and whether it's one of the basics. `flowbuddy config` and the
//! settings window are both built from this, so they can't disagree.

use serde_json::Value;

use crate::config::Config;

/// What values a setting takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Bool,
    /// A whole number in a range.
    Int {
        min: i64,
        max: i64,
    },
    /// A number in a range.
    Float {
        min: f64,
        max: f64,
    },
    /// One of these words.
    Choice(&'static [&'static str]),
    /// A local time, `HH:MM`.
    Time,
    /// A local time range, `HH:MM-HH:MM`.
    TimeRange,
    /// Any text.
    Text,
    /// A list of texts, given comma-separated.
    List,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Bool => "bool",
            Kind::Int { .. } => "int",
            Kind::Float { .. } => "float",
            Kind::Choice(_) => "choice",
            Kind::Time => "time",
            Kind::TimeRange => "time-range",
            Kind::Text => "text",
            Kind::List => "list",
        }
    }
}

/// One setting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Setting {
    /// `section.key`, as in the file: `learn.holdout`.
    pub key: &'static str,
    pub kind: Kind,
    /// Where the settings window shows it.
    pub group: &'static str,
    /// One of the few shown first; the rest are under "Advanced".
    pub basic: bool,
    pub what: &'static str,
    /// What turning it on means for your privacy or your data, said before
    /// it's turned on.
    pub notice: Option<&'static str>,
}

const fn s(key: &'static str, kind: Kind, group: &'static str, what: &'static str) -> Setting {
    Setting {
        key,
        kind,
        group,
        basic: false,
        what,
        notice: None,
    }
}

const fn basic(mut x: Setting) -> Setting {
    x.basic = true;
    x
}

const fn notice(mut x: Setting, why: &'static str) -> Setting {
    x.notice = Some(why);
    x
}

const COACHING: &str = "Coaching";
const TIMING: &str = "Timing";
const LEARNING: &str = "Learning";
const PRIVACY: &str = "Privacy and text";
const AI: &str = "AI";
const RECORDING: &str = "Recording and memory";
const KEYBOARD: &str = "Keyboard and apps";

pub static SETTINGS: &[Setting] = &[
    basic(s(
        "coaching.default_mode",
        Kind::Choice(&["off", "passive", "balanced", "active"]),
        COACHING,
        "How involved Flow Buddy is when it starts: off, counting only, a few tips now and then, or tips as they happen",
    )),
    basic(s(
        "coaching.when",
        Kind::Choice(&["now", "pause"]),
        COACHING,
        "When a tip shows: as soon as the habit is seen, or once you stop typing for a moment",
    )),
    basic(s(
        "coaching.quiet_hours",
        Kind::TimeRange,
        COACHING,
        "No tips in these hours, e.g. 22:00-08:00",
    )),
    basic(s(
        "coaching.nudge_seconds",
        Kind::Int { min: 2, max: 60 },
        COACHING,
        "How long a tip stays on screen, in seconds",
    )),
    s(
        "coaching.nudge_style",
        Kind::Choice(&["bar", "card"]),
        COACHING,
        "Tips as a label in the top bar, or a card at the top of the screen (default: a card in active mode)",
    ),
    s(
        "coaching.balanced_interval_minutes",
        Kind::Int { min: 1, max: 480 },
        TIMING,
        "Balanced mode: minutes between batches of tips",
    ),
    s(
        "coaching.batch_size",
        Kind::Int { min: 1, max: 10 },
        TIMING,
        "Balanced mode: tips per batch",
    ),
    s(
        "coaching.cooldown_minutes",
        Kind::Int { min: 0, max: 480 },
        TIMING,
        "Active mode: minutes before the same habit gets a tip again",
    ),
    s(
        "coaching.hourly_cap",
        Kind::Int { min: 1, max: 60 },
        TIMING,
        "Active mode: tips per hour at most",
    ),
    s(
        "coaching.summary_time",
        Kind::Time,
        TIMING,
        "Passive mode: when the end-of-day summary comes",
    ),
    s(
        "coaching.hold_ms",
        Kind::Int { min: 0, max: 10000 },
        TIMING,
        "How long a habit waits before its tip, in milliseconds, so it isn't about keys you're still typing",
    ),
    s(
        "coaching.pause_ms",
        Kind::Int {
            min: 300,
            max: 30000,
        },
        TIMING,
        "When tips wait for a pause: how long you stop typing for it to count, in milliseconds",
    ),
    s(
        "learn.gap_minutes",
        Kind::Int { min: 0, max: 240 },
        TIMING,
        "No daily lesson within this many minutes of another tip",
    ),
    s(
        "learn.challenge_days",
        Kind::Int { min: 1, max: 30 },
        TIMING,
        "Days a challenge stays open",
    ),
    s(
        "learn.again_after_days",
        Kind::Int { min: 1, max: 90 },
        TIMING,
        "Days before a lesson you didn't take up can come again",
    ),
    s(
        "learn.max_showings",
        Kind::Int { min: 1, max: 20 },
        TIMING,
        "Times a lesson is shown at most, if you don't take it up",
    ),
    basic(s(
        "learn.daily",
        Kind::Bool,
        LEARNING,
        "One small Vim feature you don't use yet, once a day",
    )),
    basic(s(
        "learn.level",
        Kind::Choice(&["beginner", "intermediate", "advanced"]),
        LEARNING,
        "Where you start: skip the basics you already know",
    )),
    basic(s(
        "learn.challenges",
        Kind::Bool,
        LEARNING,
        "After a lesson, a challenge to use it the next time it fits",
    )),
    s(
        "learn.holdout",
        Kind::Float { min: 0.0, max: 0.5 },
        LEARNING,
        "The share of tips held back at random, to measure whether tips help (0 shows them all)",
    ),
    s(
        "learning.improving_ratio",
        Kind::Float { min: 0.0, max: 1.0 },
        LEARNING,
        "How often you do it the better way, over a week, to count as improving",
    ),
    s(
        "learning.mastered_ratio",
        Kind::Float { min: 0.0, max: 1.0 },
        LEARNING,
        "How often you do it the better way, over two weeks, to count as mastered",
    ),
    s(
        "learning.mastered_min_events",
        Kind::Int { min: 1, max: 1000 },
        LEARNING,
        "How many times a habit must come up before it can count as mastered",
    ),
    s(
        "learning.mastered_clean_minutes",
        Kind::Int { min: 0, max: 10000 },
        LEARNING,
        "Minutes of work without the habit at all that also count as mastered",
    ),
    s(
        "learning.regress_hits",
        Kind::Int { min: 1, max: 1000 },
        LEARNING,
        "Times a habit comes back in a week to count as slipping",
    ),
    notice(
        s(
            "text.shapes",
            Kind::Bool,
            PRIVACY,
            "Editor plugins look at the text around a move or edit to give exact tips; only numbers leave the editor",
        ),
        "The editor plugins will read the text around your edits (secret files never). Only counts and shapes are sent to Flow Buddy.",
    ),
    notice(
        s(
            "text.ai_snippets",
            Kind::Bool,
            PRIVACY,
            "Keep a few redacted lines around slow edits, in your chosen projects only, for your AI to look at",
        ),
        "Lines of your code from the projects you list will be kept in memory and shown to your AI, with secrets redacted.",
    ),
    notice(
        s(
            "text.cloud",
            Kind::Bool,
            PRIVACY,
            "Also send those snippets to a cloud AI, not only a local one",
        ),
        "Redacted snippets of your code will be sent to the cloud AI provider you set up.",
    ),
    s(
        "text.projects",
        Kind::List,
        PRIVACY,
        "Folders where snippets may be taken, e.g. ~/code/myapp",
    ),
    s(
        "text.exclude",
        Kind::List,
        PRIVACY,
        "More files never to look at, as patterns (*.sql, notes/*)",
    ),
    notice(
        s(
            "typing.accuracy",
            Kind::Bool,
            PRIVACY,
            "Count characters typed and deleted again (counts only), for the summary",
        ),
        "Flow Buddy will count how often you correct what you type, and which pairs of characters come before a correction. Counts only, never text.",
    ),
    s(
        "ai.provider",
        Kind::Choice(&["none", "command", "claude-code", "anthropic", "openai"]),
        AI,
        "Where the AI features get their model: none, a program of yours, Claude Code, or an API",
    ),
    s(
        "ai.model",
        Kind::Text,
        AI,
        "The model, for the API providers",
    ),
    s(
        "ai.base_url",
        Kind::Text,
        AI,
        "For openai: the API's address (default OpenAI's; or a local server's)",
    ),
    s(
        "ai.api_key_env",
        Kind::Text,
        AI,
        "The environment variable holding the API key (keys never go in the file)",
    ),
    notice(
        s(
            "ai.api_key_command",
            Kind::List,
            AI,
            "Or a command that prints the API key, e.g. secret-tool, lookup, service, flowbuddy",
        ),
        "This command will be run to get your API key whenever the AI is used.",
    ),
    s(
        "ai.command",
        Kind::List,
        AI,
        "A program that reads a prompt and writes an answer, e.g. ollama, run, llama3.2:3b",
    ),
    s(
        "ai.timeout_seconds",
        Kind::Int { min: 5, max: 3600 },
        AI,
        "Give up on the AI after this many seconds",
    ),
    notice(
        s(
            "ai.weekly",
            Kind::Bool,
            AI,
            "Let your AI write the weekly progress note",
        ),
        "Your AI will run once a week without being asked, on counts of your habits.",
    ),
    s(
        "ai.filter",
        Kind::Bool,
        AI,
        "Skip tips your history says you'd dismiss (learned on this machine, no AI involved)",
    ),
    notice(
        s(
            "ai.tips",
            Kind::Bool,
            AI,
            "Once a day, your AI writes a personal sentence for each recent habit",
        ),
        "Your AI will run once a day without being asked, on counts of your habits.",
    ),
    notice(
        s(
            "record.keep_days",
            Kind::Int { min: 0, max: 30 },
            RECORDING,
            "Keep a rolling recording of what the rules see for this many days, to tune them (0: off)",
        ),
        "Flow Buddy will keep a recording of the keys' shapes (never what you type) on this machine.",
    ),
    s(
        "memory.episode_retention_days",
        Kind::Int { min: 1, max: 3650 },
        RECORDING,
        "Delete the details of each habit seen after this many days (the totals stay)",
    ),
    s(
        "keyboard.xkb_options",
        Kind::List,
        KEYBOARD,
        "Keyboard options, if not read from GNOME, e.g. caps:escape",
    ),
    s(
        "keyboard.xkb_layout",
        Kind::Text,
        KEYBOARD,
        "The keyboard layout Vim commands are typed on (default us)",
    ),
    s(
        "keyboard.insert_escapes",
        Kind::List,
        KEYBOARD,
        "Two-key mappings to Esc in Insert mode, e.g. jk, for setups without an editor plugin",
    ),
    s(
        "apps.vscode",
        Kind::List,
        KEYBOARD,
        "Window ids taken as VS Code, instead of the built-in list",
    ),
    s(
        "apps.terminals",
        Kind::List,
        KEYBOARD,
        "Window ids taken as terminals, instead of the built-in list",
    ),
    s(
        "apps.vims",
        Kind::List,
        KEYBOARD,
        "Command names taken as Vim, instead of the built-in list",
    ),
    s(
        "apps.claudes",
        Kind::List,
        KEYBOARD,
        "Command names taken as Claude Code, instead of the built-in list",
    ),
];

/// Per-rule settings: `rules.<id>.enabled` and `rules.<id>.threshold`.
pub const RULE_GROUP: &str = "Rules";

/// The setting for `key`: one from [`SETTINGS`], or a rule's.
pub fn find(key: &str) -> Option<Setting> {
    if let Some(s) = SETTINGS.iter().find(|s| s.key == key) {
        return Some(*s);
    }
    let (id, field) = rule_key(key)?;
    let rules = crate::rules::all();
    rules.iter().find(|r| r.id() == id)?;
    Some(Setting {
        key: "rules",
        kind: match field {
            "enabled" => Kind::Bool,
            _ => Kind::Int { min: 1, max: 100 },
        },
        group: RULE_GROUP,
        basic: false,
        what: match field {
            "enabled" => "Whether this rule gives tips",
            _ => "How many times in a row before this rule gives a tip",
        },
        notice: None,
    })
}

/// `rules.vim.repeat-jk.threshold` → (`vim.repeat-jk`, `threshold`).
fn rule_key(key: &str) -> Option<(&str, &str)> {
    let rest = key.strip_prefix("rules.")?;
    let (id, field) = rest.rsplit_once('.')?;
    matches!(field, "enabled" | "threshold").then_some((id, field))
}

/// The value of `key` in `config`, as JSON (`null` when not set).
pub fn value(config: &Config, key: &str) -> Value {
    let all = serde_json::to_value(config).unwrap_or(Value::Null);
    if let Some((id, field)) = rule_key(key) {
        return all["rules"][id][field].clone();
    }
    let Some((section, name)) = key.split_once('.') else {
        return Value::Null;
    };
    all[section][name].clone()
}

/// A value as you'd type it: `true`, `0.1`, `22:00-08:00`, `a, b`.
pub fn show(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(show).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

/// `value`, checked for `setting`, as a TOML value to write.
pub fn to_toml(setting: &Setting, value: &str) -> Result<String, String> {
    let v = value.trim();
    let quoted = |t: &str| toml::Value::String(t.to_string()).to_string();
    match setting.kind {
        Kind::Bool => match v.to_ascii_lowercase().as_str() {
            "true" | "on" | "yes" | "1" => Ok("true".into()),
            "false" | "off" | "no" | "0" => Ok("false".into()),
            _ => Err("expected true or false".into()),
        },
        Kind::Int { min, max } => match v.parse::<i64>() {
            Ok(n) if (min..=max).contains(&n) => Ok(n.to_string()),
            _ => Err(format!("expected a whole number from {min} to {max}")),
        },
        Kind::Float { min, max } => match v.parse::<f64>() {
            Ok(n) if n >= min && n <= max => Ok(format!("{n:?}")),
            _ => Err(format!("expected a number from {min} to {max}")),
        },
        Kind::Choice(choices) => {
            if choices.contains(&v) {
                Ok(quoted(v))
            } else {
                Err(format!("expected one of: {}", choices.join(", ")))
            }
        }
        Kind::Time => crate::config::parse_hm(v)
            .map(|_| quoted(v))
            .ok_or_else(|| "expected a time like 18:00".into()),
        Kind::TimeRange => crate::config::parse_range(v)
            .map(|_| quoted(v))
            .ok_or_else(|| "expected a time range like 22:00-08:00".into()),
        Kind::Text => Ok(quoted(v)),
        Kind::List => {
            let items: Vec<String> = v
                .split(',')
                .map(str::trim)
                .filter(|i| !i.is_empty())
                .map(quoted)
                .collect();
            Ok(format!("[{}]", items.join(", ")))
        }
    }
}

/// Where `key` lives in the file: (section header name, key).
fn place(key: &str) -> Option<(String, String)> {
    if let Some((id, field)) = rule_key(key) {
        return Some((format!("rules.\"{id}\""), field.to_string()));
    }
    let (section, name) = key.split_once('.')?;
    Some((section.to_string(), name.to_string()))
}

/// The config file's text with `key` set to `value`, checked: an unknown
/// key or a wrong value is an error saying what's allowed, and the result
/// must still load.
pub fn set(text: &str, key: &str, value: &str) -> Result<String, String> {
    let setting = find(key)
        .ok_or_else(|| format!("no setting called {key} (see `flowbuddy config list`)"))?;
    let literal = to_toml(&setting, value).map_err(|e| format!("{key}: {e}"))?;
    let (section, name) = place(key).ok_or_else(|| format!("no setting called {key}"))?;
    let out = set_key(text, &section, &name, &literal);
    toml::from_str::<Config>(&out).map_err(|e| format!("{key}: {e}"))?;
    Ok(out)
}

/// The config file's text with `key` back to its default (its line gone).
pub fn reset(text: &str, key: &str) -> Result<String, String> {
    find(key).ok_or_else(|| format!("no setting called {key}"))?;
    let (section, name) = place(key).ok_or_else(|| format!("no setting called {key}"))?;
    Ok(remove_key(text, &section, &name))
}

/// The lines of `[section]` in `lines`: (header index, end).
fn section_bounds(lines: &[String], section: &str) -> Option<(usize, usize)> {
    let header = format!("[{section}]");
    let start = lines.iter().position(|l| l.trim() == header)?;
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map_or(lines.len(), |i| start + 1 + i);
    Some((start, end))
}

fn key_line(lines: &[String], (start, end): (usize, usize), key: &str) -> Option<usize> {
    (start + 1..end).find(|&i| {
        lines[i]
            .trim_start()
            .strip_prefix(key)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    })
}

/// Set `key = value` (TOML already, e.g. `"active"` or `true`) in
/// `[section]` of `text`: replace the key's line, keeping a trailing
/// comment, or add it to the section, or add the section. Everything else
/// in the file, comments too, stays as it was.
pub fn set_key(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let Some(bounds) = section_bounds(&lines, section) else {
        let mut out = text.trim_end().to_string();
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!("[{section}]\n{key} = {value}\n"));
        return out;
    };
    match key_line(&lines, bounds, key) {
        Some(i) => {
            // A trailing comment, with the spaces before it.
            let comment = lines[i]
                .find(" #")
                .map(|c| {
                    let start = lines[i][..c].trim_end().len();
                    lines[i][start..].to_string()
                })
                .unwrap_or_default();
            lines[i] = format!("{key} = {value}{comment}");
        }
        None => {
            // After the section's last line that isn't blank.
            let (start, end) = bounds;
            let at = (start + 1..end)
                .rev()
                .find(|&i| !lines[i].trim().is_empty())
                .map_or(start + 1, |i| i + 1);
            lines.insert(at, format!("{key} = {value}"));
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Remove `key`'s line from `[section]`, if it's there.
pub fn remove_key(text: &str, section: &str, key: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    if let Some(bounds) = section_bounds(&lines, section)
        && let Some(i) = key_line(&lines, bounds, key)
    {
        lines.remove(i);
    } else {
        return text.to_string();
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every key `Config` has, as `section.key`.
    fn config_keys() -> Vec<String> {
        let v = serde_json::to_value(Config::default()).unwrap();
        let mut out = Vec::new();
        for (section, fields) in v.as_object().unwrap() {
            if section == "rules" {
                continue;
            }
            for name in fields.as_object().unwrap().keys() {
                out.push(format!("{section}.{name}"));
            }
        }
        out
    }

    #[test]
    fn every_setting_is_described() {
        for key in config_keys() {
            assert!(find(&key).is_some(), "{key} isn't in settings::SETTINGS");
        }
        for s in SETTINGS {
            assert!(
                config_keys().contains(&s.key.to_string()),
                "{} isn't a setting",
                s.key
            );
            // Its default is a value it accepts.
            let default = show(&value(&Config::default(), s.key));
            if !default.is_empty() {
                assert!(
                    to_toml(s, &default).is_ok(),
                    "{}: default {default:?}",
                    s.key
                );
            }
        }
        assert!(SETTINGS.iter().filter(|s| s.basic).count() <= 8);
    }

    #[test]
    fn set_checks_and_keeps_the_rest() {
        let text = "# my settings\n[learn]\ndaily = true # keep this\n";
        let out = set(text, "learn.daily", "off").unwrap();
        assert_eq!(out, "# my settings\n[learn]\ndaily = false # keep this\n");
        let out = set(&out, "learn.holdout", "0").unwrap();
        assert!(out.contains("holdout = 0.0"), "{out}");
        assert!(
            set(text, "learn.holdout", "2")
                .unwrap_err()
                .contains("0 to 0.5")
        );
        assert!(
            set(text, "learn.level", "expert")
                .unwrap_err()
                .contains("beginner")
        );
        assert!(
            set(text, "learn.nope", "1")
                .unwrap_err()
                .contains("no setting")
        );
        let out = set("", "coaching.quiet_hours", "22:00-08:00").unwrap();
        assert!(set("", "coaching.quiet_hours", "late").is_err());
        let c: Config = toml::from_str(&out).unwrap();
        assert_eq!(c.coaching.quiet_hours.as_deref(), Some("22:00-08:00"));
        let out = set("", "apps.terminals", "kitty, foot").unwrap();
        assert!(out.contains(r#"terminals = ["kitty", "foot"]"#), "{out}");
        let out = reset(&out, "apps.terminals").unwrap();
        assert!(!out.contains("terminals"), "{out}");
    }

    #[test]
    fn rules_have_settings_too() {
        let out = set("", "rules.vim.repeat-jk.threshold", "8").unwrap();
        assert!(
            out.contains("[rules.\"vim.repeat-jk\"]\nthreshold = 8"),
            "{out}"
        );
        let c: Config = toml::from_str(&out).unwrap();
        assert_eq!(
            value(&c, "rules.vim.repeat-jk.threshold"),
            serde_json::json!(8)
        );
        assert!(set("", "rules.vim.nope.enabled", "true").is_err());
    }
}
