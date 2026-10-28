//! How well you know each skill, and how well you'd still recall it: see
//! docs/learning.md.
//!
//! Mastery is knowledge tracing (Corbett & Anderson 1995): each day you
//! use a skill is evidence you've learned it, each day you fall back on the
//! habit it replaces is evidence you haven't. Recall is half-life
//! regression (Settles & Meeder 2016): memory halves every `h` days, and
//! `h` grows with practice, fitted to your own gaps between uses.

use std::collections::HashMap;

/// One day of evidence about a skill.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Day {
    /// Days before today.
    pub ago: u64,
    /// Times used that day.
    pub uses: u64,
    /// How much that day's habits speak against it, 0 to 1: a rule that
    /// fired teaches several skills, and any one of them would have done.
    pub misses: f64,
}

/// Knowledge-tracing parameters, per day of evidence.
const P_INIT: f64 = 0.1;
const P_LEARN: f64 = 0.2;
/// Using it without having learned it: by accident, or a near miss of the
/// key tracker.
const P_GUESS: f64 = 0.2;
/// Having learned it and still reaching for the old habit.
const P_SLIP: f64 = 0.1;
/// Uses in one day count as at most this many pieces of evidence: a
/// hundred `w`s in an afternoon aren't a hundred days of practice.
const USES_PER_DAY: u64 = 3;

/// Mastered: at least this likely learned.
pub const MASTERED: f64 = 0.95;
/// And still recalled at least this likely.
pub const RECALLED: f64 = 0.5;

/// What the evidence says about one skill.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Skill {
    /// Chance you've learned it.
    pub mastery: f64,
    /// Days you used it, and the weight of days you didn't when you could
    /// have: the half-life's features.
    pub right: f64,
    pub wrong: f64,
    /// Days since you last used it.
    pub idle: Option<u64>,
}

impl Skill {
    /// Chance you'd recall it today.
    pub fn recall(&self, model: &Model) -> f64 {
        match self.idle {
            Some(idle) => recall(idle as f64, model.half_life(self.right, self.wrong)),
            None => 0.0,
        }
    }

    /// Mastery and recall in percent, for [`crate::curriculum::Progress`].
    pub fn percents(&self, model: &Model) -> (u8, u8) {
        let pct = |p: f64| (p * 100.0).round().clamp(0.0, 100.0) as u8;
        (pct(self.mastery), pct(self.recall(model)))
    }

    pub fn known(&self, model: &Model) -> bool {
        self.mastery >= MASTERED && self.recall(model) >= RECALLED
    }
}

/// Trace one skill through its days, oldest first or not.
pub fn trace(days: &[Day]) -> Skill {
    let mut days = days.to_vec();
    days.sort_by_key(|d| std::cmp::Reverse(d.ago));
    let mut s = Skill {
        mastery: P_INIT,
        ..Default::default()
    };
    for d in &days {
        // Each use, or each day of the old habit, is a chance to learn it.
        for _ in 0..d.uses.min(USES_PER_DAY) {
            s.mastery = learn(observe(s.mastery, 1.0, true));
        }
        if d.uses == 0 && d.misses > 0.0 {
            s.mastery = learn(observe(s.mastery, d.misses.min(1.0), false));
        }
        if d.uses > 0 {
            s.right += 1.0;
            s.idle = Some(d.ago);
        } else {
            s.wrong += d.misses.min(1.0);
        }
    }
    s
}

fn learn(p: f64) -> f64 {
    p + (1.0 - p) * P_LEARN
}

/// Bayes' rule for one observation, weighted: `weight` < 1 is evidence
/// that's only partly about this skill.
fn observe(p: f64, weight: f64, used: bool) -> f64 {
    let (if_known, if_not) = if used {
        (1.0 - P_SLIP, P_GUESS)
    } else {
        (P_SLIP, 1.0 - P_GUESS)
    };
    let known = p * (weight * if_known + 1.0 - weight);
    let not = (1.0 - p) * (weight * if_not + 1.0 - weight);
    known / (known + not)
}

fn recall(days: f64, half_life: f64) -> f64 {
    (-days / half_life).exp2()
}

/// The half-life model: `h = 2^(θ·x)` in days, where `x` is (1, √(1 +
/// days used), √(1 + days missed)).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Model {
    pub theta: [f64; 3],
}

/// Before your own data says otherwise: a week after the first day's use,
/// two months after nine, years after daily use for a month; misses
/// shorten it.
pub const PRIOR: Model = Model {
    theta: [0.8, 1.77, -0.5],
};

/// Half-lives are kept between a day and nine months, as in the paper.
const MIN_HALF_LIFE: f64 = 1.0;
const MAX_HALF_LIFE: f64 = 274.0;

impl Default for Model {
    fn default() -> Self {
        PRIOR
    }
}

/// One review in your history: after `gap` days, with this much practice
/// before, did you use it (1) or fall back on the habit (towards 0)?
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub gap: f64,
    pub right: f64,
    pub wrong: f64,
    pub recalled: f64,
}

fn features(right: f64, wrong: f64) -> [f64; 3] {
    [1.0, (1.0 + right).sqrt(), (1.0 + wrong).sqrt()]
}

impl Model {
    pub fn half_life(&self, right: f64, wrong: f64) -> f64 {
        let x = features(right, wrong);
        let log_h: f64 = self.theta.iter().zip(x).map(|(t, x)| t * x).sum();
        log_h.exp2().clamp(MIN_HALF_LIFE, MAX_HALF_LIFE)
    }

    /// Fit to your own history, starting from and pulled towards
    /// [`PRIOR`]: with little data it stays near the prior. The loss is
    /// the paper's: squared error in recall plus, weighted by `ALPHA`,
    /// squared error in (log) half-life.
    pub fn fit(samples: &[Sample]) -> Model {
        const ALPHA: f64 = 0.01;
        const LAMBDA: f64 = 0.1;
        const RATE: f64 = 0.05;
        const ROUNDS: usize = 200;
        let mut theta = PRIOR.theta;
        if samples.is_empty() {
            return PRIOR;
        }
        let n = samples.len() as f64;
        for _ in 0..ROUNDS {
            let mut grad = [0.0; 3];
            for s in samples {
                let x = features(s.right, s.wrong);
                let log_h: f64 = theta.iter().zip(x).map(|(t, x)| t * x).sum();
                let h = log_h.exp2().clamp(MIN_HALF_LIFE, MAX_HALF_LIFE);
                let p = recall(s.gap, h);
                // The half-life this review implies.
                let target = s.recalled.clamp(0.0001, 0.9999);
                let log_h_seen = (-s.gap / target.log2())
                    .clamp(MIN_HALF_LIFE, MAX_HALF_LIFE)
                    .log2();
                // d p / d log_h = p ln2 · gap/h · ln2.
                let dp = p * std::f64::consts::LN_2.powi(2) * s.gap / h;
                let g = 2.0 * (p - s.recalled) * dp + 2.0 * ALPHA * (log_h - log_h_seen);
                for (gi, xi) in grad.iter_mut().zip(x) {
                    *gi += g * xi / n;
                }
            }
            for i in 0..3 {
                theta[i] -= RATE * (grad[i] + 2.0 * LAMBDA * (theta[i] - PRIOR.theta[i]));
            }
        }
        Model { theta }
    }
}

/// Your reviews: each day of evidence after a skill's first use, with the
/// gap since it was last used.
pub fn samples(history: &[Day]) -> Vec<Sample> {
    let mut days = history.to_vec();
    days.sort_by_key(|d| std::cmp::Reverse(d.ago));
    let mut out = Vec::new();
    let (mut last_use, mut right, mut wrong) = (None::<u64>, 0.0, 0.0);
    for d in &days {
        if let Some(last) = last_use {
            let recalled = if d.uses > 0 {
                1.0
            } else {
                1.0 - d.misses.min(1.0)
            };
            if d.uses > 0 || d.misses > 0.0 {
                out.push(Sample {
                    gap: (last - d.ago) as f64,
                    right,
                    wrong,
                    recalled,
                });
            }
        }
        if d.uses > 0 {
            right += 1.0;
            last_use = Some(d.ago);
        } else {
            wrong += d.misses.min(1.0);
        }
    }
    out
}

/// Every skill's days of evidence, from uses per (days ago, skill) and
/// rule hits per (days ago, rule); `teaches` gives a rule's skills.
pub fn histories<'a>(
    uses: impl IntoIterator<Item = (u64, &'a str, u64)>,
    hits: impl IntoIterator<Item = (u64, &'a str, u64)>,
    teaches: impl Fn(&str) -> Vec<&'static str>,
) -> HashMap<String, Vec<Day>> {
    let mut by: HashMap<String, HashMap<u64, Day>> = HashMap::new();
    for (ago, item, n) in uses {
        let d = by
            .entry(item.to_string())
            .or_default()
            .entry(ago)
            .or_default();
        d.ago = ago;
        d.uses += n;
    }
    for (ago, rule, n) in hits {
        let skills = teaches(rule);
        if n == 0 || skills.is_empty() {
            continue;
        }
        let w = 1.0 / skills.len() as f64;
        for item in skills {
            let d = by
                .entry(item.to_string())
                .or_default()
                .entry(ago)
                .or_default();
            d.ago = ago;
            d.misses = (d.misses + w).min(1.0);
        }
    }
    by.into_iter()
        .map(|(k, v)| (k, v.into_values().collect()))
        .collect()
}

/// Add a whole miss on the day `ago`: a challenge to use it, missed.
pub fn missed(history: &mut Vec<Day>, ago: u64) {
    match history.iter_mut().find(|d| d.ago == ago) {
        Some(d) => d.misses = 1.0,
        None => history.push(Day {
            ago,
            uses: 0,
            misses: 1.0,
        }),
    }
}

/// Every skill traced, and the half-life model fitted to all of them.
pub fn assess(histories: &HashMap<String, Vec<Day>>) -> (HashMap<String, Skill>, Model) {
    let samples: Vec<Sample> = histories.values().flat_map(|h| samples(h)).collect();
    let model = Model::fit(&samples);
    let skills = histories
        .iter()
        .map(|(k, h)| (k.clone(), trace(h)))
        .collect();
    (skills, model)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn used(ago: u64, uses: u64) -> Day {
        Day {
            ago,
            uses,
            misses: 0.0,
        }
    }
    fn missed(ago: u64, misses: f64) -> Day {
        Day {
            ago,
            uses: 0,
            misses,
        }
    }

    #[test]
    fn mastery_grows_with_use_and_falls_with_the_old_habit() {
        assert!(trace(&[used(0, 1)]).mastery < MASTERED, "once isn't enough");
        assert!(
            trace(&[used(0, 3)]).mastery >= MASTERED,
            "three times will do"
        );
        let three = trace(&[used(5, 1), used(3, 1), used(1, 1)]);
        assert!(three.mastery >= MASTERED, "{three:?}");
        let heavy_day = trace(&[used(0, 200)]);
        assert!(heavy_day.mastery < 0.99, "one day is one day's evidence");
        let slipping = trace(&[used(5, 1), missed(4, 1.0), missed(3, 1.0), missed(2, 1.0)]);
        assert!(slipping.mastery < 0.5, "{slipping:?}");
        // A rule that teaches four skills says little about each one.
        let vague = trace(&[used(5, 1), used(4, 1), used(3, 1), missed(2, 0.25)]);
        assert!(vague.mastery > 0.9, "{vague:?}");
    }

    #[test]
    fn recall_halves_and_practice_lengthens_it() {
        let once = trace(&[used(7, 1)]);
        let r = once.recall(&PRIOR);
        assert!((0.4..0.6).contains(&r), "a week after one use: {r}");
        let h1 = PRIOR.half_life(1.0, 0.0);
        let h9 = PRIOR.half_life(9.0, 0.0);
        let h30 = PRIOR.half_life(30.0, 0.0);
        assert!(
            h1 < h9 && h9 < h30 && h30 <= MAX_HALF_LIFE,
            "{h1} {h9} {h30}"
        );
        assert!(PRIOR.half_life(9.0, 4.0) < h9, "misses shorten it");
        let daily: Vec<_> = (0..20).map(|d| used(d + 10, 2)).collect();
        assert!(trace(&daily).known(&PRIOR), "used daily, ten days ago");
        assert!(!trace(&[used(200, 50)]).known(&PRIOR), "faded");
    }

    #[test]
    fn fitting_follows_your_data() {
        assert_eq!(Model::fit(&[]), PRIOR);
        // You forget faster than the prior expects: used, then a week later
        // back on the habit, again and again.
        let forgetful: Vec<Sample> = (0..200)
            .map(|i| Sample {
                gap: 7.0,
                right: (i % 5) as f64,
                wrong: 0.0,
                recalled: 0.0,
            })
            .collect();
        let m = Model::fit(&forgetful);
        assert!(m.half_life(2.0, 0.0) < PRIOR.half_life(2.0, 0.0), "{m:?}");
        // One sample barely moves it.
        let m = Model::fit(&forgetful[..1]);
        assert!((m.theta[0] - PRIOR.theta[0]).abs() < 0.5, "{m:?}");
    }

    #[test]
    fn histories_spread_a_rule_over_its_skills() {
        let h = histories(
            [(3, "ciw", 2), (1, "ciw", 1)],
            [(2, "change-run", 4), (2, "nothing", 1)],
            |rule| match rule {
                "change-run" => vec!["ciw", "dot"],
                _ => vec![],
            },
        );
        let ciw = &h["ciw"];
        assert_eq!(ciw.len(), 3);
        assert!(ciw.iter().any(|d| d.ago == 2 && d.misses == 0.5));
        assert_eq!(h["dot"], vec![missed(2, 0.5)]);
        let s = samples(ciw);
        assert_eq!(s.len(), 2, "{s:?}");
        assert_eq!((s[0].gap, s[0].recalled), (1.0, 0.5));
        assert_eq!((s[1].gap, s[1].right), (2.0, 1.0));
    }
}
