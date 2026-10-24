//! Where each habit stands, from "new" to "mastered".

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Seen, never nudged.
    #[default]
    New,
    /// Nudged at least once; not improving yet.
    Nudged,
    /// Using the better way about as often as the old one.
    Improving,
    /// The better way is the habit now. No more nudges.
    Mastered,
    /// Was mastered or improving, and slipped back.
    Regressed,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::New => "new",
            Stage::Nudged => "nudged",
            Stage::Improving => "improving",
            Stage::Mastered => "mastered",
            Stage::Regressed => "regressed",
        }
    }

    pub fn parse(s: &str) -> Stage {
        match s {
            "nudged" => Stage::Nudged,
            "improving" => Stage::Improving,
            "mastered" => Stage::Mastered,
            "regressed" => Stage::Regressed,
            _ => Stage::New,
        }
    }

    /// How much a finding at this stage is worth nudging about.
    pub fn nudge_weight(self) -> f64 {
        match self {
            Stage::New | Stage::Nudged => 1.0,
            Stage::Improving => 0.5,
            Stage::Mastered => 0.0,
            Stage::Regressed => 1.5,
        }
    }
}
