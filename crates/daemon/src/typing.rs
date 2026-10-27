//! Opt-in typing accuracy (`[typing] accuracy`): how many characters are
//! typed and deleted again, and which pair of characters comes right before
//! a correction. Counts only. At most the last two typed characters are
//! held, in memory, to name the pair; any other key forgets them.

use std::collections::HashMap;

use flowbuddy_core::context::ContextKind;

#[derive(Default)]
pub struct Typing {
    counts: HashMap<ContextKind, (u64, u64)>,
    pairs: HashMap<String, u64>,
    recent: Vec<char>,
    /// Inside a run of Backspaces: only its first one names a pair.
    correcting: bool,
}

/// What to write to the store: (context, typed, corrections) and pairs.
pub type Counts = (Vec<(ContextKind, u64, u64)>, Vec<(String, u64)>);

impl Typing {
    pub fn typed(&mut self, ctx: ContextKind, c: char) {
        self.counts.entry(ctx).or_default().0 += 1;
        self.correcting = false;
        self.recent.push(c);
        if self.recent.len() > 2 {
            self.recent.remove(0);
        }
    }

    pub fn backspace(&mut self, ctx: ContextKind) {
        self.counts.entry(ctx).or_default().1 += 1;
        if !self.correcting
            && let [before, deleted] = self.recent[..]
            && before.is_alphanumeric()
            && deleted.is_alphanumeric()
        {
            let pair: String = [before, deleted]
                .iter()
                .flat_map(|c| c.to_lowercase())
                .collect();
            *self.pairs.entry(pair).or_default() += 1;
        }
        self.correcting = true;
        self.recent.pop();
    }

    /// Any other key: the characters before it no longer matter.
    pub fn other(&mut self) {
        self.recent.clear();
        self.correcting = false;
    }

    /// Everything counted since the last call.
    pub fn take(&mut self) -> Counts {
        let counts = self.counts.drain().map(|(c, (t, k))| (c, t, k)).collect();
        let pairs = self.pairs.drain().collect();
        (counts, pairs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTX: ContextKind = ContextKind::VsCodeVim;

    fn type_str(t: &mut Typing, s: &str) {
        for c in s.chars() {
            if c == '<' {
                t.backspace(CTX);
            } else {
                t.typed(CTX, c);
            }
        }
    }

    #[test]
    fn counts_and_pairs() {
        let mut t = Typing::default();
        // "teh" fixed to "the": two corrections, the first after "eh".
        type_str(&mut t, "teh<<he");
        let (counts, pairs) = t.take();
        assert_eq!(counts, [(CTX, 5, 2)]);
        assert_eq!(pairs, [("eh".to_string(), 1)]);
        assert_eq!(t.take(), (vec![], vec![]), "taken");
    }

    #[test]
    fn other_keys_forget_the_characters() {
        let mut t = Typing::default();
        type_str(&mut t, "ab");
        t.other();
        t.backspace(CTX);
        assert!(t.take().1.is_empty());
        // Punctuation doesn't make a pair.
        type_str(&mut t, "a.<");
        assert!(t.take().1.is_empty());
    }
}
