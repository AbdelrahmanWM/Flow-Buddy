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
        "Tips as a label in the top bar, or a card at the top of the screen (default: a card in active and balanced mode)",
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
        "The model: an alias like opus or sonnet, or a full name like claude-opus-5-5",
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
        "More window ids to take as VS Code, besides the built-in ones",
    ),
    s(
        "apps.terminals",
        Kind::List,
        KEYBOARD,
        "More window ids to take as terminals, besides the built-in ones",
    ),
    s(
        "apps.vims",
        Kind::List,
        KEYBOARD,
        "More command names to take as Vim, besides the built-in ones",
    ),
    s(
        "apps.claudes",
        Kind::List,
        KEYBOARD,
        "More command names to take as Claude Code, besides the built-in ones",
    ),
];

/// A word more about each setting: when you'd change it, and what it's
/// easily confused with. `flowbuddy config explain` and the settings
/// window's info buttons show it.
pub static MORE: &[(&str, &str)] = &[
    (
        "coaching.default_mode",
        "Off stops watching. Passive only counts, and gives a summary at the end of the day. Balanced holds tips and shows the most useful one or two every half hour, so it rarely interrupts. Active shows a tip as soon as a habit is seen, with limits per hour. This is the mode Flow Buddy starts in; switching mode from the menu or `flowbuddy mode` lasts until you switch again.",
    ),
    (
        "coaching.when",
        "With \"now\", a tip appears as soon as the habit is seen. With \"pause\", it waits until you stop typing for a moment (see Pause length under Timing), so it never lands in the middle of a word. Pause is gentler; now ties the tip more clearly to what you just did.",
    ),
    (
        "coaching.quiet_hours",
        "During these hours no tips appear, in any mode; counting goes on. Write it as start-end in 24-hour time; it can run past midnight (22:00-08:00). Leave it empty for none. For a break right now, Pause in the menu is quicker.",
    ),
    (
        "coaching.nudge_seconds",
        "How long a tip stays on screen before fading. It stays in the menu after that, and Keep on screen keeps it up until you close it.",
    ),
    (
        "coaching.nudge_style",
        "A label in the top bar is the quietest; a card at the top of the screen is easier to read. By default active and balanced mode use the card, passive the bar.",
    ),
    (
        "coaching.balanced_interval_minutes",
        "Balanced mode only. Habits seen in this time are weighed against each other and only the best one or two are shown at the end of it. Shorter means fresher tips but more often; longer means fewer interruptions. Not the same as the cooldown, which is for active mode.",
    ),
    (
        "coaching.batch_size",
        "Balanced mode only: how many tips each batch shows at most.",
    ),
    (
        "coaching.cooldown_minutes",
        "Active mode only: after a tip about a habit, no tip about the same habit for this long, even if you keep doing it. Other habits can still get tips (up to the hourly cap).",
    ),
    (
        "coaching.hourly_cap",
        "Active mode only: tips per hour at most, all habits together. With the cooldown it keeps active mode from becoming noise.",
    ),
    (
        "coaching.summary_time",
        "Passive mode: when the end-of-day summary notification comes, in 24-hour time.",
    ),
    (
        "coaching.hold_ms",
        "A habit isn't shown the instant it's seen: for this long Flow Buddy waits for a better tip about the same keys (you might be in the middle of something a second rule covers better). 0 shows tips immediately. Not the same as the pause length, which waits for you to stop typing.",
    ),
    (
        "coaching.pause_ms",
        "Only when tips appear after a pause: how long you must stop typing for it to count. Not the same as the hold, which waits for a better tip, not for you.",
    ),
    (
        "learn.gap_minutes",
        "The daily lesson waits until no other tip has come for this long, so it never arrives on top of one.",
    ),
    (
        "learn.challenge_days",
        "After a lesson, a challenge to use it. If you use it within this many days, it's met; if you go back to the old habit, it's missed (and counts against the skill); if neither, it just ends.",
    ),
    (
        "learn.again_after_days",
        "A lesson you didn't take up can come back, but not sooner than this. Learning something works best spaced out, not repeated day after day.",
    ),
    (
        "learn.max_showings",
        "How many times a lesson you don't take up is shown at all. After that it's left alone (\"I know this\" in the menu skips it at once).",
    ),
    (
        "learn.daily",
        "Once a day, one Vim feature you don't use yet, picked by what your habits say would help you most. In every mode but off. The menu's \"I know this\" and \"Show me another\" work on it.",
    ),
    (
        "learn.level",
        "Beginner assumes nothing and learns what you know from what you use. Intermediate takes the basics as known; advanced takes the first two levels as known. Either way, what you use counts.",
    ),
    (
        "learn.challenges",
        "After a lesson, a challenge to use it the next time it fits. Using something beats reading about it; whether you do also tells Flow Buddy how well it's sinking in.",
    ),
    (
        "learn.holdout",
        "To know whether tips help you at all, a share of them is held back at random and what you do then is compared (`flowbuddy learn --report`). 0.1 holds back one in ten; 0 shows them all but leaves nothing to compare against.",
    ),
    (
        "learning.improving_ratio",
        "A habit counts as improving when, over a week, you do it the better way at least this share of the time. Improving habits get fewer tips.",
    ),
    (
        "learning.mastered_ratio",
        "A habit counts as mastered when, over two weeks, you do it the better way at least this share of the time. Mastered habits get no tips, only an occasional check.",
    ),
    (
        "learning.mastered_min_events",
        "Mastered needs at least this many times the habit came up, so a lucky few don't count.",
    ),
    (
        "learning.mastered_clean_minutes",
        "Or mastered when you simply stopped: this many minutes of work over two weeks without the habit at all.",
    ),
    (
        "learning.regress_hits",
        "A mastered or improving habit counts as slipping when it comes back this many times in a week without the better way.",
    ),
    (
        "text.shapes",
        "Off by default. The Vim plugin and the VS Code companion look at the text around a movement or edit to tell which shorter command would have done it exactly, and send only numbers. Files that look secret (.env, keys, passwords) are never looked at.",
    ),
    (
        "text.ai_snippets",
        "Off by default. A few lines around recent slow edits are kept in memory, in your listed projects only, for `flowbuddy coach` to show your AI, with secrets redacted first. Needs Projects set.",
    ),
    (
        "text.cloud",
        "Off by default. Without it, snippets go only to a local AI. With it, also to the cloud AI you set up.",
    ),
    (
        "text.projects",
        "The folders where snippets may be taken. Empty means nowhere, even with snippets on.",
    ),
    (
        "text.exclude",
        "More files never to look at, on top of the built-in list of secret-looking files. Patterns like *.sql or notes/*.",
    ),
    (
        "typing.accuracy",
        "Off by default. Counts characters typed and deleted again, for the summary. Which pairs of characters come before a correction are counted too. Never the text.",
    ),
    (
        "ai.provider",
        "None: no AI. Command: a program of yours, such as a local model. Claude Code: its CLI, with your Claude subscription. Anthropic or OpenAI: their APIs with your key (OpenAI also covers local servers that speak its API). The AI only ever sees counts of your habits, and snippets if you turn those on.",
    ),
    (
        "ai.model",
        "Which model to use, for Claude Code and the API providers. Claude Code takes an alias (opus, sonnet, haiku) or a full name (claude-opus-5-5); OpenAI-compatible servers take their own names. Empty picks a sensible default.",
    ),
    (
        "ai.base_url",
        "For openai: the API's address. Point it at a local server (Ollama, llama.cpp) to keep everything on your machine.",
    ),
    (
        "ai.api_key_env",
        "The environment variable your API key is in. Keys never go in the config file.",
    ),
    (
        "ai.api_key_command",
        "Or a command that prints your API key, such as a password manager's. Run each time the AI is used.",
    ),
    (
        "ai.command",
        "For the command provider: the program and its arguments. It gets the prompt on standard input and writes the answer.",
    ),
    (
        "ai.timeout_seconds",
        "How long to wait for the AI before giving up.",
    ),
    (
        "ai.weekly",
        "Off by default. Your AI writes the weekly progress note. It then runs once a week without being asked.",
    ),
    (
        "ai.filter",
        "On by default, and no AI involved: Flow Buddy learns from what you do after tips which ones you'd dismiss, and skips those. It does nothing until it has learned enough.",
    ),
    (
        "ai.tips",
        "Off by default. Once a day your AI writes a personal sentence for each recent habit, added to its tips.",
    ),
    (
        "record.keep_days",
        "0 by default. Keeps a rolling recording of what the rules see (the shape of key presses, never what you type) so `flowbuddy record tune` can check rule changes against your real work.",
    ),
    (
        "memory.episode_retention_days",
        "Each time a habit is seen, a small record of its shape is kept for this long, for summaries and tuning. The totals per habit stay after.",
    ),
    (
        "keyboard.xkb_options",
        "Usually read from GNOME. Set it if you remap keys some other way (Caps Lock as Escape: caps:escape), so Flow Buddy reads your keys as you mean them.",
    ),
    (
        "keyboard.xkb_layout",
        "The layout Vim commands are typed on. Only needed when it isn't us and GNOME doesn't say.",
    ),
    (
        "keyboard.insert_escapes",
        "Two-key mappings you use to leave Insert mode, such as jk. The Vim plugin reports them itself; set this without it.",
    ),
    (
        "apps.vscode",
        "Only needed for a build the built-in list doesn't know; `flowbuddy status` shows the id of the focused window. These are added to the built-in ones, which `flowbuddy config explain` lists.",
    ),
    (
        "apps.terminals",
        "Only needed for a terminal the built-in list doesn't know; `flowbuddy status` shows the id of the focused window. These are added to the built-in ones, which `flowbuddy config explain` lists.",
    ),
    (
        "apps.vims",
        "Command names taken as Vim in a terminal, such as a wrapper script. These are added to the built-in ones.",
    ),
    (
        "apps.claudes",
        "Command names taken as Claude Code, such as a wrapper script. These are added to the built-in ones.",
    ),
];

/// The longer explanation of `key`: from [`MORE`], or a rule's own.
pub fn more(key: &str) -> Option<String> {
    if let Some((_, m)) = MORE.iter().find(|(k, _)| *k == key) {
        return Some((*m).to_string());
    }
    let (id, field) = rule_key(key)?;
    let rules = crate::rules::all();
    let rule = rules.iter().find(|r| r.id() == id)?;
    Some(match field {
        "enabled" => format!(
            "{}. Turned off, the rule doesn't run at all; to keep counting without tips, mute it instead (`flowbuddy rules mute {id}`).",
            rule.description()
        ),
        _ => format!(
            "{}. The threshold is how many repeats it waits for before a tip: higher means fewer tips, only for longer runs.",
            rule.description()
        ),
    })
}

/// A starting point: a few ordinary settings, set together. Applying one
/// only writes those settings; change any of them afterwards.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    pub id: &'static str,
    pub name: &'static str,
    pub what: &'static str,
    pub values: &'static [(&'static str, &'static str)],
}

pub static PROFILES: &[Profile] = &[
    Profile {
        id: "gentle",
        name: "Gentle",
        what: "One tip now and then, never while you're typing. A lesson a day, no challenges.",
        values: &[
            ("coaching.default_mode", "balanced"),
            ("coaching.when", "pause"),
            ("coaching.balanced_interval_minutes", "45"),
            ("coaching.batch_size", "1"),
            ("coaching.nudge_style", "bar"),
            ("learn.daily", "true"),
            ("learn.challenges", "false"),
        ],
    },
    Profile {
        id: "learning-fast",
        name: "Learning fast",
        what: "Tips as you work, a lesson and a challenge every day. The most coaching.",
        values: &[
            ("coaching.default_mode", "active"),
            ("coaching.when", "now"),
            ("coaching.cooldown_minutes", "10"),
            ("coaching.hourly_cap", "8"),
            ("coaching.nudge_style", "card"),
            ("learn.daily", "true"),
            ("learn.challenges", "true"),
        ],
    },
    Profile {
        id: "just-watching",
        name: "Just watching",
        what: "No tips while you work: counts and a summary at the end of the day.",
        values: &[
            ("coaching.default_mode", "passive"),
            ("learn.daily", "false"),
            ("learn.challenges", "false"),
        ],
    },
    Profile {
        id: "good-at-vim",
        name: "Already good at Vim",
        what: "Skips the basics; a few sharper tips after you pause, and a lesson a day on what's left.",
        values: &[
            ("coaching.default_mode", "balanced"),
            ("coaching.when", "pause"),
            ("coaching.batch_size", "1"),
            ("learn.level", "advanced"),
            ("learn.daily", "true"),
            ("learn.challenges", "true"),
        ],
    },
];

pub fn profile(id: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.id == id)
}

/// Each setting `profile` would change in `config`: (key, now, then).
pub fn profile_changes(config: &Config, profile: &Profile) -> Vec<(&'static str, String, String)> {
    profile
        .values
        .iter()
        .filter_map(|(key, want)| {
            let now = show(&value(config, key));
            let now_or_default = if now.is_empty() {
                show(&value(&Config::default(), key))
            } else {
                now.clone()
            };
            (now_or_default != *want).then(|| (*key, now, (*want).to_string()))
        })
        .collect()
}

/// The profile your settings match, if any.
pub fn current_profile(config: &Config) -> Option<&'static Profile> {
    PROFILES
        .iter()
        .find(|p| profile_changes(config, p).is_empty())
}

/// The config file's text with `profile` applied.
pub fn apply_profile(text: &str, profile: &Profile) -> Result<String, String> {
    profile
        .values
        .iter()
        .try_fold(text.to_string(), |t, (key, v)| set(&t, key, v))
}

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
        Kind::TimeRange => match crate::config::parse_range(v) {
            Some(r) if r.start == r.end => {
                Err("the start and end are the same, so it would never begin".into())
            }
            Some(_) => Ok(quoted(v)),
            None => Err("expected a time range like 22:00-08:00".into()),
        },
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
    if key == "ai.model"
        && let Some(e) = crate::config::model_problem(value)
    {
        return Err(format!("{key}: {e}"));
    }
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
    fn every_setting_has_a_word_more() {
        for s in SETTINGS {
            assert!(
                more(s.key).is_some_and(|m| m.len() > 20),
                "{} needs a word more",
                s.key
            );
        }
        assert!(
            MORE.iter().all(|(k, _)| find(k).is_some()),
            "MORE has a key that isn't a setting"
        );
        assert!(
            more("rules.vim.repeat-jk.threshold")
                .unwrap()
                .contains("repeats")
        );
    }

    #[test]
    fn values_that_would_do_nothing_are_refused() {
        assert!(set("", "coaching.quiet_hours", "22:00-22:00").is_err());
        assert!(set("", "coaching.quiet_hours", "22:00-07:00").is_ok());
    }

    #[test]
    fn model_names_are_one_word() {
        let e = set("", "ai.model", "opus 5.5").unwrap_err();
        assert!(e.contains("no spaces"), "{e}");
        assert!(set("", "ai.model", "opus").is_ok());
        assert!(set("", "ai.model", "claude-opus-5-5").is_ok());
        assert!(set("", "ai.model", "llama3.2:3b").is_ok());
    }

    #[test]
    fn profiles_are_ordinary_settings() {
        for p in PROFILES {
            let text = apply_profile("# mine\n", p).unwrap_or_else(|e| panic!("{}: {e}", p.id));
            assert!(text.starts_with("# mine\n"));
            let c: Config = toml::from_str(&text).unwrap();
            assert_eq!(current_profile(&c).map(|p| p.id), Some(p.id), "{}", p.id);
            assert!(profile_changes(&c, p).is_empty());
        }
        let gentle = profile("gentle").unwrap();
        let changes = profile_changes(&Config::default(), gentle);
        assert!(
            changes
                .iter()
                .any(|(k, now, then)| *k == "coaching.when" && now == "now" && then == "pause")
        );
        assert!(
            !changes.iter().any(|(k, _, _)| *k == "learn.daily"),
            "already the default"
        );
        assert_eq!(current_profile(&Config::default()), None);
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
