//! Mistake rules: keys that didn't do what you meant. Tips name commands
//! from the tables here, never what was typed.
//!
//! Their ids start with `mistake.`: they show right away in active and
//! balanced mode (see `crate::policy`), since you're stuck now.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::context::{ContextKind, VimMode};
use crate::rule::{Finding, Rule, RuleEvent, Shape};
use crate::rules::vim::command;
use crate::vim::{Step, target_ok};

const VIM: &[ContextKind] = &[ContextKind::TerminalVim, ContextKind::VsCodeVim];

fn finding(
    id: &'static str,
    title: &'static str,
    text: String,
    shape: Shape,
    step: &Step,
) -> RuleEvent {
    RuleEvent::Finding(Finding {
        rule_id: id,
        title,
        text,
        shape,
        mode: VimMode::Normal,
        at_ms: step.at_ms,
        start_ms: step.at_ms,
    })
}

/// The second keys Vim (or a common plugin, or vscodevim) takes after
/// `g`, `z`, `[` and `]`, from Vim's own index.
fn second_keys() -> &'static HashMap<char, String> {
    static KEYS: OnceLock<HashMap<char, String>> = OnceLock::new();
    KEYS.get_or_init(|| {
        let mut out: HashMap<char, String> = HashMap::new();
        for e in crate::inventory::entries() {
            if e.mode != "normal" {
                continue;
            }
            let keys = e.keys.replace("[\"x]", "");
            let mut chars = keys.chars();
            let (Some(first), Some(second)) = (chars.next(), chars.next()) else {
                continue;
            };
            if matches!(first, 'g' | 'z' | '[' | ']') && !matches!(second, '{' | '<' | ' ') {
                out.entry(first).or_default().push(second);
            }
        }
        // gx (open the link: netrw), gc (commenting: vim-commentary and
        // vscodevim), gb and gh (vscodevim: add a cursor, hover); and
        // counts, then the rest of the command (`z5<CR>`).
        out.entry('g').or_default().push_str("xcbh");
        out.entry('z').or_default().push_str("0123456789");
        out
    })
}

/// Keys that make no Vim command: `ciq`, `dq`, `gq` with nothing, `zq`.
#[derive(Default)]
pub struct NoCommand;

impl NoCommand {
    const ID: &'static str = "mistake.no-command";
    const TITLE: &'static str = "That isn't a Vim command";
}

impl Rule for NoCommand {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Keys that make no Vim command, such as ciq, and what was probably meant"
    }

    fn title(&self) -> &'static str {
        Self::TITLE
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        if !matches!(step.before, VimMode::Normal | VimMode::OpPending) || !step.confident {
            return;
        }
        let Some(c) = command(step) else {
            return;
        };
        let chars: Vec<Option<char>> = c.keys.iter().map(|k| k.plain_char()).collect();
        let Some(Some(first)) = chars.first().copied() else {
            return;
        };
        let text = if matches!(first, 'd' | 'c' | 'y' | '<' | '>' | '=') {
            // After the operator: skip a count and a forced v/V.
            let mut rest = &c.keys[1..];
            while rest
                .first()
                .and_then(|k| k.plain_char())
                .is_some_and(|k| k.is_ascii_digit() || matches!(k, 'v' | 'V'))
            {
                rest = &rest[1..];
            }
            if target_ok(first, rest) {
                return;
            }
            let op = first;
            match rest.first().and_then(|k| k.plain_char()) {
                Some(ia @ ('i' | 'a')) => {
                    let quoteish = rest.get(1).and_then(|k| k.plain_char()) == Some('q');
                    if quoteish {
                        format!(
                            "`{op}{ia}` then `q` isn't a text object: for quotes it's `{op}{ia}\"`, `{op}{ia}'` or `` {op}{ia}` ``."
                        )
                    } else {
                        format!(
                            "That key isn't a text object after `{op}{ia}`. The objects: w word, s sentence, p paragraph, ( [ {{ < brackets, t tag, \" ' quotes, as in `{op}{ia}w` or `{op}{ia}(`."
                        )
                    }
                }
                _ => format!(
                    "`{op}` needs a motion or a text object after it, and that key is neither: `{op}w`, `{op}$`, `{op}}}`, `{op}iw`."
                ),
            }
        } else if matches!(first, 'g' | 'z' | '[' | ']') && c.keys.len() == 2 {
            let Some(second) = chars[1] else {
                return;
            };
            if second_keys()
                .get(&first)
                .is_some_and(|ok| ok.contains(second))
            {
                return;
            }
            let examples = match first {
                'g' => {
                    "`gg` to the top, `gi` back to where you typed, `gv` the last selection, `gd` a definition"
                }
                'z' => "`zz` center the line, `za` open or close a fold, `zt` line to the top",
                '[' => "`[(` back to the unmatched bracket, `[[` the previous section",
                _ => "`])` to the unmatched bracket, `]]` the next section",
            };
            format!("`{first}` then that key does nothing in Vim. Maybe: {examples}.")
        } else {
            return;
        };
        out.push(finding(
            Self::ID,
            Self::TITLE,
            text,
            Shape::fixed("no command"),
            step,
        ));
    }

    fn reset(&mut self) {}
}

/// Look-alike commands, and how they differ: said when one is undone at
/// once and the other comes next.
const LOOK_ALIKES: &[(&str, &str, &str)] = &[
    (
        "dw",
        "de",
        "`dw` deletes to the start of the next word, taking the space with it; `de` stops at the end of this word.",
    ),
    (
        "cw",
        "ciw",
        "`cw` changes from the cursor to the end of the word; `ciw` changes the whole word, wherever the cursor is in it.",
    ),
    (
        "dd",
        "D",
        "`dd` deletes the whole line; `D` deletes from the cursor to the end of it.",
    ),
    (
        "cc",
        "C",
        "`cc` changes the whole line; `C` changes from the cursor to the end of it.",
    ),
    (
        "p",
        "P",
        "`p` puts after the cursor (below, for whole lines); `P` puts before it (above).",
    ),
    (
        "x",
        "X",
        "`x` deletes the character under the cursor; `X` the one before it.",
    ),
    ("o", "O", "`o` opens a new line below; `O` opens one above."),
    (
        "df<char>",
        "dt<char>",
        "`df<char>` deletes up to and including the character; `dt<char>` stops just before it.",
    ),
    (
        "cf<char>",
        "ct<char>",
        "`cf<char>` changes up to and including the character; `ct<char>` stops just before it.",
    ),
    (
        "J",
        "gJ",
        "`J` joins the lines with a space between; `gJ` joins them as they are.",
    ),
    (
        "diw",
        "daw",
        "`diw` deletes just the word; `daw` takes the space around it too.",
    ),
    (
        "ciw",
        "caw",
        "`ciw` changes just the word; `caw` takes the space around it too.",
    ),
    (
        "di(",
        "da(",
        "`di(` deletes inside the brackets; `da(` takes the brackets too.",
    ),
    (
        "ci(",
        "ca(",
        "`ci(` changes inside the brackets; `ca(` takes the brackets too.",
    ),
    (
        "di\"",
        "da\"",
        "`di\"` deletes inside the quotes; `da\"` takes the quotes too.",
    ),
    (
        "ci\"",
        "ca\"",
        "`ci\"` changes inside the quotes; `ca\"` takes the quotes too.",
    ),
    (
        "yy",
        "yw",
        "`yy` copies the whole line; `yw` copies to the next word.",
    ),
];

/// A command as the table names it: `dfx` → `df<char>`, `2dw` → `dw`.
fn named(c: &crate::vim::Command) -> Option<String> {
    let mut out = String::new();
    let mut after_find = false;
    for k in &c.keys {
        let ch = k.plain_char()?;
        if after_find {
            out.push_str("<char>");
            after_find = false;
        } else {
            out.push(ch);
            after_find = matches!(ch, 'f' | 't' | 'F' | 'T') && out.len() > 1;
        }
    }
    Some(out)
}

/// Undone at once, then its look-alike: "`dw` … `de` …".
#[derive(Default)]
pub struct UndoneLookAlike {
    /// The last command, and when.
    last: Option<(String, u64)>,
    /// Undone just after it: the command, and when the undo came.
    undone: Option<(String, u64)>,
}

impl UndoneLookAlike {
    const ID: &'static str = "mistake.look-alike";
    const TITLE: &'static str = "Not the one you meant?";
    /// The undo comes this soon after the command (longer for ones that
    /// start typing, like `o`).
    const UNDO_MS: u64 = 20_000;
    /// And the other command this soon after the undo.
    const NEXT_MS: u64 = 5_000;
}

impl Rule for UndoneLookAlike {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "Undoing a command at once and using its look-alike (dw then u then de): how the two differ"
    }

    fn title(&self) -> &'static str {
        Self::TITLE
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        let Some(c) = command(step).filter(|_| step.confident) else {
            return;
        };
        let Some(name) = named(c) else {
            return;
        };
        if name == "u" {
            self.undone = self
                .last
                .take()
                .filter(|(_, at)| step.at_ms.saturating_sub(*at) <= Self::UNDO_MS)
                .map(|(n, _)| (n, step.at_ms));
            return;
        }
        let undone = self.undone.take();
        self.last = Some((name.clone(), step.at_ms));
        let Some((before, _)) =
            undone.filter(|(_, at)| step.at_ms.saturating_sub(*at) <= Self::NEXT_MS)
        else {
            return;
        };
        if let Some((_, _, why)) = LOOK_ALIKES
            .iter()
            .find(|(a, b, _)| (*a == before && *b == name) || (*b == before && *a == name))
        {
            out.push(finding(
                Self::ID,
                Self::TITLE,
                (*why).into(),
                Shape::fixed("undone look-alike"),
                step,
            ));
        }
    }

    fn reset(&mut self) {
        self.last = None;
        self.undone = None;
    }
}

/// Keys meant as text that went in as commands, or in capitals: noticed
/// when you undo them. Only how many and how fast is kept, never the keys.
/// One instance reports text typed as commands, the other Caps Lock.
#[derive(Default)]
pub struct WrongMode {
    caps: bool,
    /// The run of single-letter commands typed at typing speed: when the
    /// last came, how many, how many different, how many were H/J/K/L.
    run: Vec<(u64, char)>,
    /// The run just ended, waiting for an undo: (letters, distinct,
    /// capital moves, when it ended).
    ended: Option<(usize, usize, usize, u64)>,
}

impl WrongMode {
    const ID_TYPING: &'static str = "mistake.normal-typing";
    const ID_CAPS: &'static str = "mistake.caps-lock";
    /// Keys this close are typing, not commands one by one.
    const TYPING_GAP_MS: u64 = 250;
    /// The undo comes this soon after.
    const UNDO_MS: u64 = 8_000;

    fn end_run(&mut self) {
        if self.run.len() >= 3 {
            let mut distinct: Vec<char> = self.run.iter().map(|(_, c)| *c).collect();
            distinct.sort_unstable();
            distinct.dedup();
            let caps = self
                .run
                .iter()
                .filter(|(_, c)| matches!(c, 'H' | 'J' | 'K' | 'L'))
                .count();
            let last = self.run.last().map_or(0, |(at, _)| *at);
            self.ended = Some((self.run.len(), distinct.len(), caps, last));
        }
        self.run.clear();
    }
}

impl Rule for WrongMode {
    fn id(&self) -> &'static str {
        if self.caps {
            Self::ID_CAPS
        } else {
            Self::ID_TYPING
        }
    }

    fn description(&self) -> &'static str {
        if self.caps {
            "Commands in capitals (J, K) where lower case was meant, undone: Caps Lock on?"
        } else {
            "Typing text in Normal mode by accident, then undoing it"
        }
    }

    fn title(&self) -> &'static str {
        if self.caps {
            "Caps Lock on?"
        } else {
            "That went in as commands"
        }
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        let Some(c) = command(step) else {
            // Typing after an accidental `o` or `i`: the run is over.
            if !self.run.is_empty() {
                self.end_run();
            }
            return;
        };
        if c.is("u") {
            self.end_run();
            if let Some((n, distinct, caps, at)) = self.ended.take()
                && step.at_ms.saturating_sub(at) <= Self::UNDO_MS
            {
                let capitals = caps >= 3 && caps * 2 >= n;
                if self.caps && capitals {
                    out.push(finding(
                        Self::ID_CAPS,
                        "Caps Lock on?",
                        "Those were capitals: `J` joins lines and `K` looks up a word, where `j` and `k` move. Is Caps Lock on?".into(),
                        Shape::fixed("capitals, undone"),
                        step,
                    ));
                } else if !self.caps && !capitals && n >= 4 && distinct >= 3 {
                    out.push(finding(
                        Self::ID_TYPING,
                        "That went in as commands",
                        "You were in Normal mode, so those keys ran as commands. `i` (or `a`, `o`) first to type, and `u` undoes what they did.".into(),
                        Shape::fixed("text as commands, undone"),
                        step,
                    ));
                }
            }
            return;
        }
        let letter = c
            .single()
            .and_then(|k| k.plain_char())
            .filter(|ch| ch.is_ascii_alphabetic() && c.count.is_none());
        let fast = self
            .run
            .last()
            .is_some_and(|(at, _)| step.at_ms.saturating_sub(*at) <= Self::TYPING_GAP_MS);
        match letter {
            Some(ch) if fast || self.run.is_empty() => self.run.push((step.at_ms, ch)),
            Some(ch) => {
                self.end_run();
                self.run.push((step.at_ms, ch));
            }
            None => self.end_run(),
        }
    }

    fn reset(&mut self) {
        self.run.clear();
        self.ended = None;
    }
}

/// Every built-in Ex command, as (shortest, full) spellings, from Vim's
/// index: `:sor[t]` is ("sor", "sort").
fn ex_names() -> &'static Vec<(String, String)> {
    static NAMES: OnceLock<Vec<(String, String)>> = OnceLock::new();
    NAMES.get_or_init(|| {
        crate::inventory::entries()
            .iter()
            .filter(|e| e.mode == "ex")
            .filter_map(|e| {
                let word = e.keys.trim_start_matches(':').split_whitespace().next()?;
                let short: String = word.split('[').next()?.to_string();
                let full: String = word.chars().filter(|c| !matches!(c, '[' | ']')).collect();
                (short.chars().all(|c| c.is_ascii_alphabetic()) && !short.is_empty())
                    .then_some((short, full))
            })
            .collect()
    })
}

/// Whether Vim knows `word` as an Ex command (or an abbreviation of one).
pub fn known_ex(word: &str) -> bool {
    ex_names()
        .iter()
        .any(|(short, full)| word.starts_with(short.as_str()) && full.starts_with(word))
}

/// Commands people mean, for "maybe …".
const COMMON_EX: &[&str] = &[
    "w", "q", "wq", "x", "wa", "qa", "wqa", "q!", "e", "e!", "sp", "vs", "s", "g", "sort", "noh",
    "set", "help", "b", "bn", "bp", "bd", "ls", "tabnew", "only", "undo", "redo", "r", "m", "t",
    "norm", "make", "cn", "cp", "copen", "vimgrep", "retab", "reg", "marks",
];

/// The common command one or two edits away from `word`, if any.
fn closest(word: &str) -> Option<&'static str> {
    fn distance(a: &str, b: &str) -> usize {
        let b: Vec<char> = b.chars().collect();
        let mut row: Vec<usize> = (0..=b.len()).collect();
        for (i, ca) in a.chars().enumerate() {
            let mut prev = row[0];
            row[0] = i + 1;
            for j in 0..b.len() {
                let cur = row[j + 1];
                row[j + 1] = (prev + usize::from(ca != b[j]))
                    .min(row[j] + 1)
                    .min(cur + 1);
                prev = cur;
            }
        }
        row[b.len()]
    }
    let lower = word.to_ascii_lowercase();
    // Two letters swapped (`qw`, `sotr`) is the likeliest slip.
    let swapped = |c: &str| {
        let (a, b): (Vec<char>, Vec<char>) = (lower.chars().collect(), c.chars().collect());
        a.len() == b.len()
            && (0..a.len().saturating_sub(1)).any(|i| {
                let mut t = a.clone();
                t.swap(i, i + 1);
                t == b
            })
    };
    if let Some(c) = COMMON_EX.iter().find(|c| swapped(c)) {
        return Some(c);
    }
    let (d, best) = COMMON_EX
        .iter()
        .map(|c| (distance(&lower, c), *c))
        .min_by_key(|(d, _)| *d)?;
    let near = if word.len() <= 3 { 1 } else { 2 };
    (d <= near).then_some(best)
}

/// The tip for an Ex command Vim didn't know: never the typed name, only
/// a real command it's close to.
pub fn unknown_ex_text(word: &str) -> String {
    match closest(word) {
        Some(c) => format!("That isn't a Vim command. Did you mean `:{c}`?"),
        None => "That isn't a Vim command. `:help ex-cmd-index` lists them all.".into(),
    }
}

/// A `:` command Vim doesn't have. Only lower-case names are certain from
/// the keys: upper case may be a command of yours (the Vim plugin reports
/// Vim's own "Not an editor command" for those).
#[derive(Default)]
pub struct UnknownEx;

impl UnknownEx {
    pub const ID: &'static str = "mistake.unknown-ex";
    pub const TITLE: &'static str = "Not a Vim command";
}

impl Rule for UnknownEx {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn description(&self) -> &'static str {
        "A : command Vim doesn't have, and the one you probably meant"
    }

    fn title(&self) -> &'static str {
        Self::TITLE
    }

    fn contexts(&self) -> &'static [ContextKind] {
        VIM
    }

    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>) {
        let Some(crate::vim::VimEvent::Cmdline {
            kind: ':',
            text: Some(t),
        }) = &step.event
        else {
            return;
        };
        let Some(word) = crate::curriculum::ex_word(t) else {
            return;
        };
        if !word.starts_with(|c: char| c.is_ascii_lowercase())
            || !word.chars().all(|c| c.is_ascii_alphabetic())
            || known_ex(word)
        {
            return;
        }
        out.push(finding(
            Self::ID,
            Self::TITLE,
            unknown_ex_text(word),
            Shape::fixed("unknown : command"),
            step,
        ));
    }

    fn reset(&mut self) {}
}

pub fn all() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(NoCommand),
        Box::new(UndoneLookAlike::default()),
        Box::new(UnknownEx),
        Box::new(WrongMode::default()),
        Box::new(WrongMode {
            caps: true,
            ..WrongMode::default()
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glossary;
    use crate::rules::testing::{findings, run};
    use crate::vim::Flavor;

    fn texts(keys: &str) -> Vec<String> {
        findings(&run(
            Box::new(NoCommand),
            Flavor::Vim,
            VimMode::Normal,
            keys,
        ))
        .iter()
        .map(|f| glossary::plain(&f.text))
        .collect()
    }

    #[test]
    fn keys_that_make_no_command() {
        assert_eq!(
            texts("ciq"),
            ["ci then q isn't a text object: for quotes it's ci\", ci' or ci`."]
        );
        assert!(texts("diz")[0].contains("isn't a text object after di"));
        assert!(texts("dq")[0].starts_with("d needs a motion"));
        assert!(texts("zq")[0].starts_with("z then that key does nothing"));
        assert!(texts("gy")[0].contains("gg to the top"));
    }

    #[test]
    fn undone_then_its_look_alike() {
        let look = |keys: &str| -> Vec<String> {
            findings(&run(
                Box::new(UndoneLookAlike::default()),
                Flavor::Vim,
                VimMode::Normal,
                keys,
            ))
            .iter()
            .map(|f| glossary::plain(&f.text))
            .collect()
        };
        assert!(look("dwude")[0].starts_with("dw deletes to the start of the next word"));
        assert!(look("dfxudtx")[0].contains("df<char> deletes up to and including"));
        assert!(look("ohi<Esc>uO")[0].starts_with("o opens a new line below"));
        assert!(look("dwu").is_empty(), "an undo on its own");
        assert!(look("dwudd").is_empty(), "not a look-alike");
        assert!(look("dwjde").is_empty(), "no undo between");
    }

    #[test]
    fn typing_in_normal_mode_or_in_capitals() {
        // Keys 100 ms apart, as `run` types them: typing speed.
        let fired = |caps, keys: &str| {
            findings(&run(
                Box::new(WrongMode {
                    caps,
                    ..WrongMode::default()
                }),
                Flavor::Vim,
                VimMode::Normal,
                keys,
            ))
            .len()
        };
        assert_eq!(fired(false, "wbheu"), 1, "letters as commands, undone");
        assert_eq!(fired(false, "wbhe"), 0, "not undone");
        assert_eq!(fired(false, "jjjju"), 0, "one key repeated is moving");
        assert_eq!(fired(true, "JJJKu"), 1, "capitals, undone");
        assert_eq!(fired(false, "JJJKu"), 0, "said once, as Caps Lock");
        assert_eq!(fired(true, "wbheu"), 0);
    }

    #[test]
    fn ex_commands_vim_doesnt_have() {
        let ex = |keys: &str| -> Vec<String> {
            findings(&run(
                Box::new(UnknownEx),
                Flavor::Vim,
                VimMode::Normal,
                keys,
            ))
            .iter()
            .map(|f| glossary::plain(&f.text))
            .collect()
        };
        assert_eq!(
            ex(":qw<CR>"),
            ["That isn't a Vim command. Did you mean :wq?"]
        );
        assert_eq!(
            ex(":sotr<CR>"),
            ["That isn't a Vim command. Did you mean :sort?"]
        );
        assert!(ex(":zzzzzz<CR>")[0].contains("ex-cmd-index"));
        for ok in [
            ":w<CR>",
            ":sor<CR>",
            ":'<lt>,'>s/a/b/<CR>",
            ":42<CR>",
            ":W<CR>",
            ":tabnew<CR>",
            ":norm .<CR>",
        ] {
            assert!(ex(ok).is_empty(), "{ok}: {:?}", ex(ok));
        }
    }

    #[test]
    fn real_commands_and_plugins_are_fine() {
        for keys in [
            "dw", "ci\"", "d2w", "dvj", "dd", "yy", ">>", "dgg", "d]m", "dfx", "dt,", "d/x<CR>",
            "ds\"", "cs\"'", "ysiw\"", "gcc", "gx", "gg", "zz", "z5<CR>", "]]", "[s", "gh",
        ] {
            assert!(texts(keys).is_empty(), "{keys}: {:?}", texts(keys));
        }
    }
}
