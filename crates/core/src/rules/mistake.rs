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
                            "That key isn't a text object after `{op}{ia}`. The objects: `w` word, `s` sentence, `p` paragraph, `(` `[` `{{` `<` brackets, `t` tag, `\"` `'` quotes: `{op}{ia}w`, `{op}{ia}(`."
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

pub fn all() -> Vec<Box<dyn Rule>> {
    vec![Box::new(NoCommand)]
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
    fn real_commands_and_plugins_are_fine() {
        for keys in [
            "dw", "ci\"", "d2w", "dvj", "dd", "yy", ">>", "dgg", "d]m", "dfx", "dt,", "d/x<CR>",
            "ds\"", "cs\"'", "ysiw\"", "gcc", "gx", "gg", "zz", "z5<CR>", "]]", "[s", "gh",
        ] {
            assert!(texts(keys).is_empty(), "{keys}: {:?}", texts(keys));
        }
    }
}
