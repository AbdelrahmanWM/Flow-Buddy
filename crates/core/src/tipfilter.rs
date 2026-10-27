//! The learned tip filter: a small logistic regression, trained on your own
//! history of what happened after each tip, that predicts whether the next
//! tip is worth showing.
//!
//! It runs and trains entirely on this machine, from counts and ids only
//! (see `TipRow` in the store). Until it has seen enough outcomes it lets
//! everything through.

/// A tip's features, as the store keeps them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TipFeatures<'a> {
    pub rule_id: &'a str,
    pub context: &'a str,
    pub stage: &'a str,
    pub hour: u32,
    pub mode: &'a str,
    pub prior_today: u32,
}

/// How much each outcome teaches, and which way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Label {
    pub useful: bool,
    pub weight: f64,
}

/// Rule ids are hashed into this many slots.
const RULE_SLOTS: usize = 64;
const CONTEXTS: [&str; 6] = [
    "vscode-vim",
    "terminal-vim",
    "bash-vi",
    "claude-vim",
    "shell",
    "unsupported",
];
const STAGES: [&str; 5] = ["new", "nudged", "improving", "mastered", "regressed"];
const MODES: [&str; 4] = ["off", "passive", "balanced", "active"];
const DIM: usize = 1 + RULE_SLOTS + CONTEXTS.len() + STAGES.len() + 4 + MODES.len() + 1;

/// Fewer labelled tips than this, and the filter lets everything through.
pub const MIN_OUTCOMES: usize = 30;

fn slot(rule: &str) -> usize {
    // FNV-1a: stable across runs and machines.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in rule.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    (h % RULE_SLOTS as u64) as usize
}

fn one_hot(x: &mut [f64], at: usize, options: &[&str], value: &str) {
    if let Some(i) = options.iter().position(|o| *o == value) {
        x[at + i] = 1.0;
    }
}

fn features(f: &TipFeatures<'_>) -> [f64; DIM] {
    let mut x = [0.0; DIM];
    x[0] = 1.0;
    let mut at = 1;
    x[at + slot(f.rule_id)] = 1.0;
    at += RULE_SLOTS;
    one_hot(&mut x, at, &CONTEXTS, f.context);
    at += CONTEXTS.len();
    one_hot(&mut x, at, &STAGES, f.stage);
    at += STAGES.len();
    // Night, morning, afternoon, evening.
    x[at + (f.hour.min(23) / 6) as usize] = 1.0;
    at += 4;
    one_hot(&mut x, at, &MODES, f.mode);
    at += MODES.len();
    x[at] = f64::from(f.prior_today.min(10)) / 10.0;
    x
}

fn sigmoid(z: f64) -> f64 {
    1.0 / (1.0 + (-z).exp())
}

/// A trained filter.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    weights: Vec<f64>,
    /// How many labelled tips it learned from.
    pub trained_on: usize,
}

impl Model {
    /// Train on labelled tips; `None` until there are [`MIN_OUTCOMES`].
    pub fn train(rows: &[(TipFeatures<'_>, Label)]) -> Option<Model> {
        if rows.len() < MIN_OUTCOMES {
            return None;
        }
        let data: Vec<([f64; DIM], f64, f64)> = rows
            .iter()
            .map(|(f, l)| (features(f), if l.useful { 1.0 } else { 0.0 }, l.weight))
            .collect();
        let total: f64 = data.iter().map(|(_, _, w)| w).sum();
        let mut w = vec![0.0; DIM];
        // Plain batch gradient descent with L2: small data, a few ms.
        const EPOCHS: usize = 300;
        const RATE: f64 = 0.5;
        const L2: f64 = 0.01;
        for _ in 0..EPOCHS {
            let mut grad = vec![0.0; DIM];
            for (x, y, weight) in &data {
                let p = sigmoid(x.iter().zip(&w).map(|(a, b)| a * b).sum());
                let err = (p - y) * weight;
                for (g, xi) in grad.iter_mut().zip(x) {
                    *g += err * xi;
                }
            }
            for (i, (wi, g)) in w.iter_mut().zip(&grad).enumerate() {
                let reg = if i == 0 { 0.0 } else { L2 * *wi };
                *wi -= RATE * (g / total + reg);
            }
        }
        Some(Model {
            weights: w,
            trained_on: rows.len(),
        })
    }

    /// The chance this tip turns out useful.
    pub fn score(&self, f: &TipFeatures<'_>) -> f64 {
        let x = features(f);
        sigmoid(x.iter().zip(&self.weights).map(|(a, b)| a * b).sum())
    }
}

/// Below this chance of being useful, a tip is skipped.
pub const CUTOFF: f64 = 0.12;
/// A rule's first few tips are always shown.
pub const ALWAYS_FIRST: u32 = 3;
/// Every so many skipped tips of a rule, one is shown anyway, so the
/// filter keeps learning whether it was right.
pub const EXPLORE_EVERY: u32 = 5;

/// Whether to show a tip: `shown_before` tips of this rule were shown, and
/// `skipped_in_a_row` have been skipped since the last one shown.
pub fn should_show(
    model: Option<&Model>,
    f: &TipFeatures<'_>,
    shown_before: u32,
    skipped_in_a_row: u32,
) -> bool {
    let Some(model) = model else { return true };
    shown_before < ALWAYS_FIRST || skipped_in_a_row + 1 >= EXPLORE_EVERY || model.score(f) >= CUTOFF
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tip<'a>(rule: &'a str, hour: u32, prior: u32) -> TipFeatures<'a> {
        TipFeatures {
            rule_id: rule,
            context: "vscode-vim",
            stage: "nudged",
            hour,
            mode: "active",
            prior_today: prior,
        }
    }

    fn label(useful: bool) -> Label {
        Label {
            useful,
            weight: 1.0,
        }
    }

    #[test]
    fn nothing_filtered_before_enough_outcomes() {
        let rows: Vec<_> = (0..10).map(|_| (tip("a", 10, 0), label(false))).collect();
        assert!(Model::train(&rows).is_none());
        assert!(should_show(None, &tip("a", 10, 0), 99, 0));
    }

    #[test]
    fn learns_which_rules_help() {
        // Tips for "helps" get adopted, tips for "noise" get dismissed.
        let mut rows = Vec::new();
        for i in 0..40 {
            rows.push((tip("vim.helps", i % 24, i % 5), label(true)));
            rows.push((tip("vim.noise", i % 24, i % 5), label(false)));
        }
        let m = Model::train(&rows).unwrap();
        let good = m.score(&tip("vim.helps", 14, 1));
        let bad = m.score(&tip("vim.noise", 14, 1));
        assert!(good > 0.8 && bad < 0.2, "{good} {bad}");
        assert!(should_show(Some(&m), &tip("vim.helps", 14, 1), 10, 0));
        assert!(!should_show(Some(&m), &tip("vim.noise", 14, 1), 10, 0));
        // But never a rule's first tips, and now and then anyway.
        assert!(should_show(Some(&m), &tip("vim.noise", 14, 1), 1, 0));
        assert!(should_show(
            Some(&m),
            &tip("vim.noise", 14, 1),
            10,
            EXPLORE_EVERY - 1
        ));
    }

    #[test]
    fn learns_too_many_tips_a_day() {
        // Early tips help; after many in a day they get ignored.
        let mut rows = Vec::new();
        for i in 0..60 {
            let prior = i % 10;
            rows.push((tip("vim.x", 12, prior), label(prior < 3)));
        }
        let m = Model::train(&rows).unwrap();
        assert!(m.score(&tip("vim.x", 12, 0)) > m.score(&tip("vim.x", 12, 9)));
    }
}
