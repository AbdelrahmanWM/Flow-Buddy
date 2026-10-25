//! Coaching modes and the decision of when to nudge.

use serde::{Deserialize, Serialize};

/// How involved Flow Buddy is. See docs/architecture.md#coaching-modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoachingMode {
    /// Capture paused.
    Off,
    /// Count only; summaries on demand and once a day.
    Passive,
    /// A short batch of tips every few minutes.
    #[default]
    Balanced,
    /// Real-time nudges, rate-limited.
    Active,
}

impl CoachingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            CoachingMode::Off => "off",
            CoachingMode::Passive => "passive",
            CoachingMode::Balanced => "balanced",
            CoachingMode::Active => "active",
        }
    }
}

impl std::str::FromStr for CoachingMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "off" => Ok(CoachingMode::Off),
            "passive" => Ok(CoachingMode::Passive),
            "balanced" => Ok(CoachingMode::Balanced),
            "active" => Ok(CoachingMode::Active),
            _ => Err(format!(
                "unknown mode {s:?} (expected off, passive, balanced or active)"
            )),
        }
    }
}
