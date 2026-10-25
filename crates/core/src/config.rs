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
    /// "card" or "bar"; by default a card in active mode, the top bar otherwise.
    pub nudge_style: Option<NudgeStyle>,
    /// No nudges in this local time range, e.g. "22:00-08:00".
    pub quiet_hours: Option<String>,
    /// Passive mode: local time of the end-of-day summary notification.
    pub summary_time: String,
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
}

/// Replace the built-in app id / command lists.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct AppsConfig {
    pub vscode: Option<Vec<String>>,
    pub terminals: Option<Vec<String>>,
    pub vims: Option<Vec<String>>,
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
        if let Some(n) = configured.and_then(|c| c.threshold) {
            settings.thresholds.insert(id.clone(), n);
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
            },
        );
        assert!(rule_settings(&c, &st).disabled.is_empty());
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
            },
        );
        st.save(&path).unwrap();
        assert_eq!(State::load(&path), st);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
