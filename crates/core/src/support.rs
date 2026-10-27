//! Which commands work where. Tips are written for Vim; vscodevim, Claude
//! Code's Vim mode and shells' vi-modes each do less, so a suggestion they
//! can't follow is taken out of the tip ([`fit`]), and a tip with nothing
//! left isn't shown.

use crate::context::ContextKind;
use crate::glossary::{Segment, segments};

/// Commands vscodevim (1.32) parses but doesn't implement, or doesn't
/// have: its `:earlier` says "not implemented".
const NOT_IN_VSCODEVIM: &[&str] = &[
    ":earlier",
    ":later",
    ":undolist",
    ":g",
    ":v",
    ":global",
    ":vglobal",
    ":wincmd",
    ":changes",
    "g-",
    "g+",
    "g&",
];

/// Claude Code's Vim mode: Normal-mode movement and the common edits, no
/// Visual mode, search, marks, macros or `:` commands.
const CLAUDE_MOTIONS: &[&str] = &[
    "h", "j", "k", "l", "w", "e", "b", "W", "E", "B", "0", "$", "^", "gg", "G", ";", ",",
];
const CLAUDE_COMMANDS: &[&str] = &[
    "x", "dd", "D", "cc", "C", "yy", "Y", "p", "P", ".", "i", "I", "a", "A", "o", "O", ">>", "<<",
    "J", "Esc", "Ctrl-w", "Ctrl-u",
];
const CLAUDE_OBJECTS: &[&str] = &[
    "iw", "aw", "iW", "aW", "i\"", "a\"", "i'", "a'", "i(", "a(", "i)", "a)", "ib", "ab", "i[",
    "a[", "i]", "a]", "i{", "a{", "i}", "a}", "iB", "aB",
];

/// The vi-modes of bash (readline), zsh and fish share this much: no text
/// objects (bash and fish lack them), Visual mode, registers, marks, `o`,
/// or `:` commands.
const SHELL_COMMANDS: &[&str] = &[
    "x", "X", "dd", "D", "cc", "C", "S", "s", "yy", "p", "P", ".", "u", "~", "i", "I", "a", "A",
    "R", "Esc", "Ctrl-w", "Ctrl-u", "Ctrl-r", "Ctrl-l", "%", "/", "Up",
];

fn shell_supports(cmd: &str) -> bool {
    let cmd = cmd.trim_start_matches(|c: char| c.is_ascii_digit());
    if SHELL_COMMANDS.contains(&cmd)
        || (CLAUDE_MOTIONS.contains(&cmd) && !matches!(cmd, "gg" | "G"))
    {
        return true;
    }
    if cmd.strip_prefix(['f', 'F', 't', 'T', 'r']) == Some("<char>") {
        return true;
    }
    let Some(rest) = cmd.strip_prefix(['d', 'c', 'y']) else {
        return false;
    };
    let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
    matches!(rest, "w" | "e" | "b" | "W" | "E" | "B" | "$" | "0" | "^")
        || rest
            .strip_prefix(['f', 'F', 't', 'T'])
            .is_some_and(|r| r == "<char>")
}

fn claude_supports(cmd: &str) -> bool {
    let cmd = cmd.trim_start_matches(|c: char| c.is_ascii_digit());
    if CLAUDE_COMMANDS.contains(&cmd) || CLAUDE_MOTIONS.contains(&cmd) {
        return true;
    }
    if let Some(rest) = cmd.strip_prefix(['f', 'F', 't', 'T'])
        && rest == "<char>"
    {
        return true;
    }
    // An operator and a motion or text object: `dw`, `ce`, `yiw`, `di(`.
    let Some(rest) = cmd.strip_prefix(['d', 'c', 'y']) else {
        return false;
    };
    let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
    CLAUDE_OBJECTS.contains(&rest)
        || matches!(rest, "w" | "e" | "b" | "W" | "E" | "B" | "$" | "0" | "^")
        || rest
            .strip_prefix(['f', 'F', 't', 'T'])
            .is_some_and(|r| r == "<char>")
}

/// Whether one marked command (`7u`, `:earlier 1m`, `Ctrl-w h`) works in
/// `ctx`. Unknown commands are assumed to work.
pub fn supports(ctx: ContextKind, cmd: &str) -> bool {
    match ctx {
        ContextKind::VsCodeVim => {
            let cmd = cmd.trim_start_matches(|c: char| c.is_ascii_digit());
            !NOT_IN_VSCODEVIM.iter().any(|n| {
                cmd == *n
                    || (n.starts_with(':')
                        && cmd
                            .strip_prefix(n)
                            .is_some_and(|rest| rest.starts_with([' ', '/'])))
            })
        }
        // A tip's command may name several presses: `Ctrl-d` then `5j`.
        ContextKind::ClaudeVim => presses(cmd).all(claude_supports),
        ContextKind::BashVi => presses(cmd).all(shell_supports),
        _ => true,
    }
}

fn presses(cmd: &str) -> impl Iterator<Item = &str> {
    cmd.split_whitespace()
        .filter(|w| !matches!(*w, "then" | "or" | "and"))
        .map(|w| w.split('×').next().unwrap_or(w).trim_end_matches(','))
}

/// Whether every command marked in `text` works in `ctx`.
pub fn supports_all(ctx: ContextKind, text: &str) -> bool {
    crate::glossary::code_spans(text)
        .into_iter()
        .all(|c| supports(ctx, c))
}

/// `text` split into sentences, keeping each one's own punctuation; a
/// sentence ends at ". " outside a marked command.
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for seg in segments(text) {
        match seg {
            Segment::Code(c) => current.push_str(&crate::glossary::mark(c)),
            Segment::Text(t) => {
                let mut rest = t;
                while let Some(i) = rest.find(". ") {
                    current.push_str(&rest[..=i]);
                    out.push(std::mem::take(&mut current));
                    rest = &rest[i + 2..];
                }
                current.push_str(rest);
            }
        }
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

/// A tip made to fit `ctx`: sentences suggesting something that doesn't
/// work there are taken out. `None` when no suggestion is left.
pub fn fit(ctx: ContextKind, text: &str) -> Option<String> {
    if supports_all(ctx, text) {
        return Some(text.to_string());
    }
    // The first sentence usually says what you did (`u` 7 times): what's
    // left must suggest something after it.
    let kept: Vec<(usize, String)> = sentences(text)
        .into_iter()
        .enumerate()
        .filter(|(_, s)| supports_all(ctx, s))
        .collect();
    let suggests = kept
        .iter()
        .any(|(i, s)| *i > 0 && !crate::glossary::code_spans(s).is_empty());
    suggests.then(|| {
        kept.into_iter()
            .map(|(_, s)| s)
            .collect::<Vec<_>>()
            .join(" ")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vscodevim_has_no_undo_tree() {
        let v = ContextKind::VsCodeVim;
        assert!(!supports(v, ":earlier 1m") && !supports(v, "g-"));
        assert!(!supports(v, ":g/pattern/d"));
        assert!(supports(v, ":s/old/new/") && supports(v, "7u") && supports(v, "g;"));
        assert!(supports(v, ":sort") && supports(v, ":b") && supports(v, "Ctrl-^"));
        assert!(supports(ContextKind::TerminalVim, ":earlier 1m"));
    }

    #[test]
    fn claude_code_does_less() {
        let c = ContextKind::ClaudeVim;
        for ok in [
            "5w", "e", "f<char>", "3f<char>", "dw", "ciw", "di(", "dt<char>", "D", ".",
        ] {
            assert!(supports(c, ok), "{ok}");
        }
        for no in [
            "s",
            "r<char>",
            "~",
            "g~iw",
            "gI",
            "Ctrl-v",
            "*",
            "%",
            "ma",
            "qq",
            ":s/old/new/",
        ] {
            assert!(!supports(c, no), "{no}");
        }
    }

    #[test]
    fn vi_mode_shells() {
        let b = ContextKind::BashVi;
        for ok in [
            "3w", "f<char>", "dw", "cw", "D", "C", "x", "s", "r<char>", "~", ".", "A", "Ctrl-r",
        ] {
            assert!(supports(b, ok), "{ok}");
        }
        for no in [
            "ciw",
            "di(",
            "o",
            "O",
            "gI",
            "Ctrl-v",
            "\"0p",
            ":s/old/new/",
            "gg",
        ] {
            assert!(!supports(b, no), "{no}");
        }
    }

    #[test]
    fn tips_lose_what_doesnt_work() {
        let undo = "`u` 7 times. `7u` undoes that many at once. `:earlier 1m` goes back to how the file was a minute ago.";
        assert_eq!(
            fit(ContextKind::VsCodeVim, undo).as_deref(),
            Some("`u` 7 times. `7u` undoes that many at once.")
        );
        assert_eq!(fit(ContextKind::TerminalVim, undo).as_deref(), Some(undo));
        // Nothing left to suggest: no tip.
        assert_eq!(
            fit(
                ContextKind::ClaudeVim,
                "`x` then `i` is `s`: substitute the character."
            ),
            None
        );
        assert_eq!(fit(ContextKind::VsCodeVim, "Try `:earlier 1m`."), None);
    }
}
