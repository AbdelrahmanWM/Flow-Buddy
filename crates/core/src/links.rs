//! Which of the curriculum's "builds on" links hold for you, from the order
//! you picked things up in: see docs/learning.md.
//!
//! Each link starts from the hand-made tree as a prior (likely, not
//! certain), and each environment's order of first use is one observation:
//! the skill after what it builds on is evidence for, before it is evidence
//! against. Order alone says little (Scheines, Silver & Goldin 2014), so
//! the prior leads until the evidence clearly disagrees.

use std::collections::HashMap;

use crate::curriculum::{CURRICULUM, requires};

/// The prior: as if seen holding twice and failing once.
const PRIOR_FOR: f64 = 2.0;
const PRIOR_AGAINST: f64 = 1.0;
/// A link is dropped once it's less likely than this to hold.
pub const HOLDS: f64 = 0.5;

/// The evidence on one link: `item` builds on `on`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Link {
    pub item: &'static str,
    pub on: &'static str,
    /// Orders where `on` came first (or the same day), and where `item` did.
    pub for_: u32,
    pub against: u32,
}

impl Link {
    /// The chance it holds: the Beta posterior's mean.
    pub fn strength(&self) -> f64 {
        let a = PRIOR_FOR + f64::from(self.for_);
        a / (a + PRIOR_AGAINST + f64::from(self.against))
    }

    pub fn holds(&self) -> bool {
        self.strength() >= HOLDS
    }
}

/// Every link in the tree, with the evidence from `orders`: per
/// environment, the day (any sortable key) each item was first used.
pub fn learn<'a>(orders: impl IntoIterator<Item = &'a HashMap<String, String>>) -> Vec<Link> {
    let mut links: Vec<Link> = CURRICULUM
        .iter()
        .flat_map(|i| {
            requires(i.id).iter().map(move |on| Link {
                item: i.id,
                on,
                ..Default::default()
            })
        })
        .collect();
    for first in orders {
        for l in &mut links {
            match (first.get(l.item), first.get(l.on)) {
                (Some(item), Some(on)) if on <= item => l.for_ += 1,
                (Some(_), _) => l.against += 1,
                // Not used yet: nothing to tell.
                (None, _) => {}
            }
        }
    }
    links
}

/// The links your history says don't hold: (item, what it builds on).
pub fn dropped(links: &[Link]) -> Vec<(&'static str, &'static str)> {
    links
        .iter()
        .filter(|l| !l.holds())
        .map(|l| (l.item, l.on))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(i, d)| (i.to_string(), d.to_string()))
            .collect()
    }

    #[test]
    fn the_tree_leads_until_your_order_clearly_disagrees() {
        let find = |links: &[Link]| *links.iter().find(|l| l.item == "till").unwrap();
        // No history: every link holds.
        assert!(dropped(&learn([])).is_empty());
        // t before f once: weaker, still there.
        let once = [order(&[("till", "2026-10-01"), ("find", "2026-10-05")])];
        let l = find(&learn(&once));
        assert_eq!((l.for_, l.against), (0, 1));
        assert!(l.holds() && l.strength() < 0.7);
        // In three places t came first: dropped.
        let thrice: Vec<_> = (0..3).map(|_| order(&[("till", "2026-10-01")])).collect();
        let links = learn(&thrice);
        assert!(!find(&links).holds());
        assert!(dropped(&links).contains(&("till", "find")));
        // f first, then t: stronger.
        let right = [order(&[("find", "2026-10-01"), ("till", "2026-10-02")])];
        assert!(find(&learn(&right)).strength() > 0.7);
    }

    #[test]
    fn a_dropped_link_no_longer_holds_a_lesson_back() {
        use crate::curriculum::Known;
        let known = Known::of(["word"]);
        assert!(!known.ready("till"), "needs f first");
        let known = known.with_links(&[("till", "find")]);
        assert!(known.ready("till"));
        assert_eq!(known.builds_on("till").count(), 0);
    }
}
