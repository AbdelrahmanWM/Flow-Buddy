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

/// Counts for one pattern over a window of days.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Window {
    /// Times the inefficient pattern happened.
    pub hits: u64,
    /// Times the better way was used.
    pub adoptions: u64,
    /// Minutes of activity in this context.
    pub active_minutes: u64,
}

impl Window {
    /// Share of the time the better way was used, if anything happened.
    pub fn adoption_ratio(&self) -> Option<f64> {
        let total = self.hits + self.adoptions;
        (total > 0).then(|| self.adoptions as f64 / total as f64)
    }

    fn ratio_or(&self, default: f64) -> f64 {
        self.adoption_ratio().unwrap_or(default)
    }
}

/// Thresholds for moving between stages. `[learning]` in config.toml.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct LearningConfig {
    /// Adoption ratio over 7 days to count as improving.
    pub improving_ratio: f64,
    /// Adoption ratio over 14 days to count as mastered.
    pub mastered_ratio: f64,
    /// Hits + adoptions over 14 days needed before "mastered" is believed.
    pub mastered_min_events: u64,
    /// Minutes of activity over 14 days without a single hit that also
    /// counts as mastered (you simply stopped doing it).
    pub mastered_clean_minutes: u64,
    /// Hits over 7 days, with a low adoption ratio, that mean regression.
    pub regress_hits: u64,
}

impl Default for LearningConfig {
    fn default() -> Self {
        LearningConfig {
            improving_ratio: 0.5,
            mastered_ratio: 0.85,
            mastered_min_events: 10,
            mastered_clean_minutes: 120,
            regress_hits: 5,
        }
    }
}

/// The next stage for a pattern, given the last 7 and 14 days.
pub fn next_stage(
    current: Stage,
    nudged: bool,
    week: Window,
    fortnight: Window,
    cfg: &LearningConfig,
) -> Stage {
    let mastered = (fortnight.hits + fortnight.adoptions >= cfg.mastered_min_events
        && fortnight.ratio_or(0.0) >= cfg.mastered_ratio)
        || (current == Stage::Improving
            && fortnight.hits == 0
            && fortnight.active_minutes >= cfg.mastered_clean_minutes);
    let improving = week.hits + week.adoptions >= 4 && week.ratio_or(0.0) >= cfg.improving_ratio;
    let slipping = week.hits >= cfg.regress_hits;

    match current {
        Stage::New if nudged => Stage::Nudged,
        Stage::New => Stage::New,
        Stage::Nudged | Stage::Regressed if mastered => Stage::Mastered,
        Stage::Nudged | Stage::Regressed if improving => Stage::Improving,
        Stage::Nudged | Stage::Regressed => current,
        Stage::Improving if mastered => Stage::Mastered,
        Stage::Improving if slipping && week.ratio_or(1.0) < 0.3 => Stage::Regressed,
        Stage::Improving => Stage::Improving,
        Stage::Mastered if slipping && week.ratio_or(1.0) < cfg.improving_ratio => Stage::Regressed,
        Stage::Mastered => Stage::Mastered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(hits: u64, adoptions: u64) -> Window {
        Window {
            hits,
            adoptions,
            active_minutes: 600,
        }
    }

    fn next(current: Stage, week: Window, fortnight: Window) -> Stage {
        next_stage(current, true, week, fortnight, &LearningConfig::default())
    }

    #[test]
    fn new_becomes_nudged_once_nudged() {
        let cfg = LearningConfig::default();
        assert_eq!(
            next_stage(Stage::New, false, w(9, 0), w(9, 0), &cfg),
            Stage::New
        );
        assert_eq!(
            next_stage(Stage::New, true, w(9, 0), w(9, 0), &cfg),
            Stage::Nudged
        );
    }

    #[test]
    fn using_the_better_way_means_improving_then_mastered() {
        assert_eq!(next(Stage::Nudged, w(4, 5), w(10, 6)), Stage::Improving);
        assert_eq!(next(Stage::Improving, w(1, 12), w(2, 20)), Stage::Mastered);
    }

    #[test]
    fn stopping_entirely_also_counts_as_mastered() {
        assert_eq!(next(Stage::Improving, w(0, 0), w(0, 0)), Stage::Mastered);
        let idle = Window {
            active_minutes: 10,
            ..w(0, 0)
        };
        assert_eq!(
            next(Stage::Improving, idle, idle),
            Stage::Improving,
            "not enough activity"
        );
    }

    #[test]
    fn slipping_back_is_regression() {
        assert_eq!(next(Stage::Mastered, w(8, 2), w(8, 30)), Stage::Regressed);
        assert_eq!(next(Stage::Improving, w(9, 1), w(12, 6)), Stage::Regressed);
        assert_eq!(next(Stage::Regressed, w(2, 6), w(10, 8)), Stage::Improving);
    }

    #[test]
    fn little_data_changes_nothing() {
        assert_eq!(next(Stage::Nudged, w(1, 1), w(1, 1)), Stage::Nudged);
        assert_eq!(next(Stage::Mastered, w(1, 0), w(1, 20)), Stage::Mastered);
    }
}

/// How much a habit deserves attention now: how often it happened lately,
/// weighted by where it is in learning, and trusted less the more often
/// its nudges were marked "not helpful". Mastered habits score 0.
pub fn priority(stage: Stage, recent_hits: u64, false_positives: u64) -> f64 {
    let weight = match stage {
        Stage::Regressed => 1.5,
        Stage::Nudged => 1.2,
        Stage::New => 1.0,
        Stage::Improving => 0.6,
        Stage::Mastered => 0.0,
    };
    recent_hits as f64 * weight / (1.0 + false_positives as f64)
}

const WEEK_S: u64 = 7 * 86_400;

/// Spaced check-ins for a mastered habit: once it has been mastered for a
/// week, one reminder may be shown if it shows up again; then after two
/// more weeks, four more, and so on.
pub fn checkin_due(mastered_since_s: u64, checkins_done: u64, now_s: u64) -> bool {
    let weeks = (1u64 << (checkins_done.min(20) as u32 + 1)) - 1;
    now_s >= mastered_since_s.saturating_add(weeks * WEEK_S)
}

#[cfg(test)]
mod checkin_tests {
    use super::*;

    #[test]
    fn intervals_double() {
        let w = WEEK_S;
        assert!(!checkin_due(0, 0, w - 1));
        assert!(checkin_due(0, 0, w));
        // After one check-in: 1 + 2 weeks.
        assert!(!checkin_due(0, 1, 3 * w - 1));
        assert!(checkin_due(0, 1, 3 * w));
        // After two: 1 + 2 + 4.
        assert!(!checkin_due(0, 2, 7 * w - 1));
        assert!(checkin_due(0, 2, 7 * w));
    }
}

#[cfg(test)]
mod priority_tests {
    use super::*;

    #[test]
    fn slipping_back_matters_most() {
        assert!(priority(Stage::Regressed, 10, 0) > priority(Stage::Nudged, 10, 0));
        assert!(priority(Stage::Nudged, 10, 0) > priority(Stage::New, 10, 0));
        assert!(priority(Stage::New, 10, 0) > priority(Stage::Improving, 10, 0));
        assert_eq!(priority(Stage::Mastered, 100, 0), 0.0);
    }

    #[test]
    fn frequency_and_trust() {
        assert!(priority(Stage::Improving, 30, 0) > priority(Stage::New, 10, 0));
        assert!(priority(Stage::New, 10, 3) < priority(Stage::New, 10, 0));
        assert_eq!(priority(Stage::New, 0, 0), 0.0);
    }
}
