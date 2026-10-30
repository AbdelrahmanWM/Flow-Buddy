//! User configuration (`config.toml`, written by you) and runtime state
//! (`state.json`, written by the daemon).
//!
//! Keeping them apart means the daemon never rewrites your config file:
//! `flowbuddy mode` or `flowbuddy rules disable` only touch the state.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::engine::RuleSettings;
use crate::ipc::NudgeStyle;
use crate::learning::LearningConfig;
use crate::policy::CoachingMode;

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub coaching: CoachingConfig,
    pub keyboard: KeyboardConfig,
    pub apps: AppsConfig,
    /// `[rules."vim.repeat-jk"] enabled = false, threshold = 8`
    pub rules: BTreeMap<String, RuleConfig>,
    pub memory: MemoryConfig,
    pub learning: LearningConfig,
    pub ai: AiConfig,
    pub typing: TypingConfig,
    pub learn: LearnConfig,
    pub text: TextConfig,
    pub record: RecordConfig,
}

/// Recording what the rules see, to tune them (`flowbuddy record tune`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct RecordConfig {
    /// Keep a rolling recording of this many days (0: only when you run
    /// `flowbuddy record start`). Nothing typed is kept; see docs/privacy.md.
    pub keep_days: u32,
}

/// Opt-in features that look at your text, in the editor plugins only.
/// Everything is off by default; see `crate::text` for what's never looked
/// at whatever this says.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct TextConfig {
    /// The plugins work out more about the text around a movement or edit
    /// (which motions would have covered a change exactly) and send the
    /// result as numbers. The text stays in the editor.
    pub shapes: bool,
    /// The plugins keep a few lines around recent slow edits in memory,
    /// for `flowbuddy coach` to show your AI, redacted. Only in `projects`.
    pub ai_snippets: bool,
    /// Also send snippets to a cloud AI provider, not only a local one.
    pub cloud: bool,
    /// Folders where snippets may be taken (`~/code/myapp`). Empty means
    /// nowhere.
    pub projects: Vec<String>,
    /// More files never to look at, as globs (`*.sql`, `notes/*`), on top
    /// of the built-in list.
    pub exclude: Vec<String>,
}

/// The daily "something new".
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct LearnConfig {
    /// One small unused Vim feature a day, in every mode but off.
    pub daily: bool,
    /// Where you start: tips build on what you're taken to know already.
    pub level: Level,
    /// After a lesson, a challenge: use it the next time it fits. Whether
    /// you do tells Flow Buddy how well it's sinking in.
    pub challenges: bool,
    /// The share of tips held back at random, to measure what tips do
    /// (`flowbuddy learn --report`); 0 shows them all.
    pub holdout: f64,
    /// No daily lesson within this many minutes of another tip.
    pub gap_minutes: u32,
    /// Days a challenge stays open.
    pub challenge_days: u32,
    /// Days before a lesson you didn't take up can come again.
    pub again_after_days: u32,
    /// Times a lesson is shown at most, if you don't take it up.
    pub max_showings: u32,
    /// A skill you've mastered comes back for review once the chance
    /// you'd recall it drops under this, in percent.
    pub review_at: u8,
}

impl Default for LearnConfig {
    fn default() -> Self {
        LearnConfig {
            daily: true,
            level: Level::Beginner,
            challenges: true,
            holdout: 0.1,
            gap_minutes: 5,
            challenge_days: 2,
            again_after_days: 7,
            max_showings: 3,
            review_at: crate::curriculum::REVIEW_AT,
        }
    }
}

/// How much Vim you know to begin with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Level {
    /// Nothing is assumed: Flow Buddy learns what you use as you go.
    #[default]
    Beginner,
    /// The basics (curriculum level 1) are known.
    Intermediate,
    /// Levels 1 and 2 are known.
    Advanced,
}

impl Level {
    /// Curriculum levels taken as known.
    pub fn known_levels(self) -> u8 {
        match self {
            Level::Beginner => 0,
            Level::Intermediate => 1,
            Level::Advanced => 2,
        }
    }
}

/// Opt-in typing statistics.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct TypingConfig {
    /// Count characters typed and deleted again, and which pairs of
    /// characters come before a correction (counts only). Off by default.
    pub accuracy: bool,
}

/// Where the LLM features get their model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiProvider {
    /// Off, unless `command` is set (then it means `command`).
    #[default]
    None,
    /// `command`: any program that reads the prompt on stdin.
    Command,
    /// Claude Code's own CLI (`claude -p`): your Claude subscription, no key.
    ClaudeCode,
    /// The Anthropic API with your key.
    Anthropic,
    /// Any OpenAI-compatible chat API (OpenAI, or a local server) with
    /// your key.
    Openai,
}

/// `flowbuddy coach` and the other LLM features: your own AI, off until
/// you choose one.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct AiConfig {
    pub provider: AiProvider,
    /// For the API providers; a sensible default otherwise.
    pub model: Option<String>,
    /// For `openai`: the API's base URL (default OpenAI's).
    pub base_url: Option<String>,
    /// The environment variable holding the API key (default
    /// `ANTHROPIC_API_KEY` or `OPENAI_API_KEY`).
    pub api_key_env: Option<String>,
    /// Or a command that prints it, e.g. `["secret-tool", "lookup",
    /// "service", "flowbuddy"]`. Keys never go in this file.
    pub api_key_command: Option<Vec<String>>,
    /// A program that reads a prompt on stdin and writes an answer to
    /// stdout, e.g. `["ollama", "run", "llama3.2:3b"]`. Run directly, not
    /// through a shell.
    pub command: Option<Vec<String>>,
    /// Give up after this long.
    pub timeout_seconds: u32,
    /// Let the model write the weekly progress note. Off by default: the
    /// model then runs once a week without being asked.
    pub weekly: bool,
    /// The learned tip filter (no LLM involved): skip tips your history says
    /// you'd dismiss. On by default; it does nothing until it has learned.
    pub filter: bool,
    /// Once a day, the model writes a personal sentence for each recent
    /// habit, added to its tips.
    pub tips: bool,
}

impl Default for AiConfig {
    fn default() -> Self {
        AiConfig {
            provider: AiProvider::None,
            model: None,
            base_url: None,
            api_key_env: None,
            api_key_command: None,
            command: None,
            timeout_seconds: 120,
            weekly: false,
            filter: true,
            tips: false,
        }
    }
}

/// What's wrong with `[ai] model`, if anything. Model names are one word:
/// an alias like `opus`, or a full name like `claude-opus-5-5`.
pub fn model_problem(model: &str) -> Option<&'static str> {
    if model.trim().contains(char::is_whitespace) {
        return Some(
            "a model name has no spaces: an alias like opus or sonnet, or a full name like claude-opus-5-5",
        );
    }
    None
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct CoachingConfig {
    /// Mode on first start; later changes are remembered in the state.
    pub default_mode: CoachingMode,
    /// Balanced mode: minutes between batches of tips.
    pub balanced_interval_minutes: u32,
    /// Balanced mode: tips per batch.
    pub batch_size: u32,
    /// Active mode: minutes before the same rule may nudge again.
    pub cooldown_minutes: u32,
    /// Active mode: nudges per hour at most.
    pub hourly_cap: u32,
    /// How long a hint stays on screen.
    pub nudge_seconds: u32,
    /// "card" or "bar"; by default a card in active and balanced mode, the
    /// top bar otherwise.
    pub nudge_style: Option<NudgeStyle>,
    /// No nudges in this local time range, e.g. "22:00-08:00".
    pub quiet_hours: Option<String>,
    /// Passive mode: local time of the end-of-day summary notification.
    pub summary_time: String,
    /// When a live tip shows: right away, or once you pause typing.
    pub when: TipTiming,
    /// How long a habit waits before its tip, in milliseconds, so the tip
    /// isn't about keys you're still in the middle of.
    pub hold_ms: u32,
    /// With `when = "pause"`: how long you stop typing for it to count as
    /// a pause, in milliseconds.
    pub pause_ms: u32,
    /// Where tips show: in the editor you're typing in, when its plugin
    /// can show them; on the desktop; or both.
    pub show_in: ShowIn,
}

/// Where a tip shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShowIn {
    /// In the editor the keys came from, if its plugin shows tips; on the
    /// desktop otherwise.
    #[default]
    Editor,
    /// Always on the desktop: the top bar, a card or a notification.
    Desktop,
    /// In the editor and on the desktop.
    Both,
}

/// When a live tip shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TipTiming {
    /// As soon as the habit is seen.
    #[default]
    Now,
    /// Once you stop typing for a moment, so it doesn't land mid-flow.
    Pause,
}

impl Default for CoachingConfig {
    fn default() -> Self {
        CoachingConfig {
            default_mode: CoachingMode::Balanced,
            balanced_interval_minutes: 30,
            batch_size: 2,
            cooldown_minutes: 10,
            hourly_cap: 6,
            nudge_seconds: 6,
            nudge_style: None,
            quiet_hours: None,
            summary_time: "18:00".into(),
            when: TipTiming::Now,
            hold_ms: 800,
            pause_ms: 1500,
            show_in: ShowIn::Editor,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct KeyboardConfig {
    /// Overrides the xkb options read from GNOME, e.g. `["caps:escape"]`.
    pub xkb_options: Option<Vec<String>>,
    /// xkb layout used with the `xkb` feature (default "us"). Vim
    /// commands are typed on a Latin layout even if you also use others.
    pub xkb_layout: Option<String>,
    /// Two-key Insert-mode mappings to Esc, e.g. `["jk"]`, for setups
    /// without an editor plugin to report them.
    pub insert_escapes: Vec<String>,
}

/// Window ids of VS Code builds (and X11 window classes).
pub const VSCODES: &[&str] = &[
    "code",
    "code_code",
    "code-url-handler",
    "code-oss",
    "codium",
    "com.visualstudio.code",
    "Code",
    "VSCodium",
];

/// Window ids of terminals, then X11 window classes.
pub const TERMINALS: &[&str] = &[
    "org.gnome.Terminal",
    "org.gnome.Ptyxis",
    "org.gnome.Console",
    "kitty",
    "Alacritty",
    "org.wezfurlong.wezterm",
    "com.mitchellh.ghostty",
    "foot",
    "terminator",
    "com.gexperts.Tilix",
    "xterm",
    "org.kde.konsole",
    "konsole",
    "Gnome-terminal",
    "XTerm",
    "URxvt",
    "st-256color",
    "Xfce4-terminal",
];

/// Window ids of Vim in a window of its own: gVim, Neovide, Neovim Qt
/// (and X11 window classes).
pub const VIM_WINDOWS: &[&str] = &["gvim", "Gvim", "neovide", "nvim-qt"];

/// Command names of Vim.
pub const VIMS: &[&str] = &["vim", "vi", "nvim", "view", "vimdiff", "nvimdiff"];

/// Command names of Claude Code.
pub const CLAUDES: &[&str] = &["claude"];

/// The built-in list an `apps.*` setting adds to.
pub fn builtin_apps(key: &str) -> Option<&'static [&'static str]> {
    Some(match key {
        "apps.vscode" => VSCODES,
        "apps.terminals" => TERMINALS,
        "apps.vims" => VIMS,
        "apps.claudes" => CLAUDES,
        _ => return None,
    })
}

/// App ids and command names to add to the built-in lists.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct AppsConfig {
    pub vscode: Option<Vec<String>>,
    pub terminals: Option<Vec<String>>,
    pub vims: Option<Vec<String>>,
    /// Command names of Claude Code.
    pub claudes: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct RuleConfig {
    pub enabled: Option<bool>,
    pub threshold: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct MemoryConfig {
    /// Episodes older than this are deleted; pattern records are kept.
    pub episode_retention_days: u32,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        MemoryConfig {
            episode_retention_days: 90,
        }
    }
}

impl Config {
    /// Load the config, or defaults if the file doesn't exist.
    pub fn load(path: &Path) -> Result<Config, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }
}

/// A local time range in minutes since midnight; may wrap past midnight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeRange {
    pub start: u32,
    pub end: u32,
}

impl TimeRange {
    pub fn contains(&self, minute: u32) -> bool {
        if self.start <= self.end {
            (self.start..self.end).contains(&minute)
        } else {
            minute >= self.start || minute < self.end
        }
    }
}

/// Parse "HH:MM" into minutes since midnight.
pub fn parse_hm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Parse "22:00-08:00".
pub fn parse_range(s: &str) -> Option<TimeRange> {
    let (a, b) = s.split_once('-')?;
    Some(TimeRange {
        start: parse_hm(a)?,
        end: parse_hm(b)?,
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct RuleState {
    /// Set by `flowbuddy rules enable/disable`; wins over the config.
    pub enabled: Option<bool>,
    /// Muted rules still count, but never nudge.
    pub muted: bool,
    /// How far Flow Buddy raised the threshold after "not helpful" reports
    /// (`flowbuddy rules reset` clears it). Ignored when the config sets one.
    pub raised: u32,
    /// A threshold accepted from `flowbuddy review`. The config's wins.
    pub threshold: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct State {
    pub mode: Option<CoachingMode>,
    /// Unix seconds.
    pub paused_until: Option<u64>,
    pub rules: BTreeMap<String, RuleState>,
    /// Local date (YYYY-MM-DD) of the last end-of-day notification.
    pub last_summary_day: Option<String>,
    /// Local date of the last weekly progress note, or when the first
    /// week started.
    pub last_weekly_day: Option<String>,
    /// Local date of the last "something new".
    pub last_learn_day: Option<String>,
    /// The open challenge: use this item the next time it fits.
    pub challenge: Option<Challenge>,
    /// The one-time hint that text shapes make tips exact was given.
    pub shapes_hinted: bool,
    /// The last week's daily lessons: when (Unix seconds), and whether
    /// each was a review. Reviews don't outnumber new lessons.
    pub recent_lessons: Vec<(u64, bool)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Challenge {
    /// A curriculum id.
    pub item: String,
    /// Unix seconds.
    pub set_at: u64,
}

impl State {
    pub fn load(path: &Path) -> State {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Write atomically (temp file + rename).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)
    }

    pub fn muted(&self) -> BTreeSet<String> {
        self.rules
            .iter()
            .filter(|(_, r)| r.muted)
            .map(|(id, _)| id.clone())
            .collect()
    }
}

/// Combine config and state into what the engine needs.
pub fn rule_settings(config: &Config, state: &State) -> RuleSettings {
    let mut settings = RuleSettings::default();
    let ids: BTreeSet<&String> = config.rules.keys().chain(state.rules.keys()).collect();
    for id in ids {
        let configured = config.rules.get(id);
        let enabled = state
            .rules
            .get(id)
            .and_then(|r| r.enabled)
            .or(configured.and_then(|c| c.enabled))
            .unwrap_or(true);
        if !enabled {
            settings.disabled.insert(id.clone());
        }
        let reviewed = state.rules.get(id).and_then(|r| r.threshold);
        if let Some(n) = configured.and_then(|c| c.threshold).or(reviewed) {
            settings.thresholds.insert(id.clone(), n);
        } else if let Some(r) = state.rules.get(id).map(|r| r.raised).filter(|r| *r > 0) {
            settings.raised.insert(id.clone(), r);
        }
    }
    settings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_is_default() {
        assert_eq!(toml::from_str::<Config>("").unwrap(), Config::default());
    }

    #[test]
    fn parses_a_full_config() {
        let c: Config = toml::from_str(
            r#"
            [coaching]
            default_mode = "active"
            quiet_hours = "22:00-08:00"

            [apps]
            terminals = ["kitty"]

            [rules."vim.repeat-jk"]
            threshold = 8

            [rules."bash.up-history"]
            enabled = false
            "#,
        )
        .unwrap();
        assert_eq!(c.coaching.default_mode, CoachingMode::Active);
        assert_eq!(c.coaching.balanced_interval_minutes, 30, "defaults kept");
        let s = rule_settings(&c, &State::default());
        assert_eq!(s.thresholds["vim.repeat-jk"], 8);
        assert!(s.disabled.contains("bash.up-history"));
    }

    #[test]
    fn state_overrides_config() {
        let mut c = Config::default();
        c.rules.insert(
            "vim.repeat-x".into(),
            RuleConfig {
                enabled: Some(false),
                threshold: None,
            },
        );
        let mut st = State::default();
        st.rules.insert(
            "vim.repeat-x".into(),
            RuleState {
                enabled: Some(true),
                muted: false,
                raised: 0,
                threshold: None,
            },
        );
        assert!(rule_settings(&c, &st).disabled.is_empty());
    }

    #[test]
    fn raised_thresholds_unless_configured() {
        let mut st = State::default();
        st.rules.insert(
            "vim.repeat-x".into(),
            RuleState {
                raised: 2,
                ..Default::default()
            },
        );
        let s = rule_settings(&Config::default(), &st);
        assert_eq!(s.raised.get("vim.repeat-x"), Some(&2));
        let c: Config = toml::from_str("[rules.\"vim.repeat-x\"]\nthreshold = 9").unwrap();
        let s = rule_settings(&c, &st);
        assert_eq!(s.thresholds.get("vim.repeat-x"), Some(&9));
        assert!(s.raised.is_empty());
    }

    #[test]
    fn time_ranges() {
        let night = parse_range("22:00-08:00").unwrap();
        assert!(night.contains(parse_hm("23:30").unwrap()));
        assert!(night.contains(parse_hm("07:59").unwrap()));
        assert!(!night.contains(parse_hm("12:00").unwrap()));
        let lunch = parse_range("12:00-13:00").unwrap();
        assert!(lunch.contains(12 * 60 + 30));
        assert!(!lunch.contains(13 * 60));
        assert_eq!(parse_hm("25:00"), None);
    }

    #[test]
    fn state_round_trip() {
        let dir = std::env::temp_dir().join(format!("fb-state-{}", std::process::id()));
        let path = dir.join("state.json");
        let mut st = State {
            mode: Some(CoachingMode::Passive),
            ..Default::default()
        };
        st.rules.insert(
            "vim.repeat-jk".into(),
            RuleState {
                enabled: None,
                muted: true,
                raised: 0,
                threshold: None,
            },
        );
        st.save(&path).unwrap();
        assert_eq!(State::load(&path), st);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
