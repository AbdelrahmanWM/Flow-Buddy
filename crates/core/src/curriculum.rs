//! A curriculum of Vim features, ordered from basic to advanced, for the
//! daily "something new" tip. Each item knows how to recognise itself in
//! the key tracker's steps, so Flow Buddy can tell what you already use.
//!
//! The items are also the skill tree: each can require others first
//! ([`requires`]), every rule teaches some of them ([`rule_skills`]), and
//! tips only suggest what you're [`Known::ready`] for.

use std::collections::HashSet;

use crate::context::{ContextKind, VimMode};
use crate::ipc::CommandShape;
use crate::key::Sym;
use crate::rules::vim::command;
use crate::rules::vscode::UiWhat;
use crate::vim::{Command, InsertKey, Step, VimEvent};

pub struct Item {
    /// Stable id, kept in the database: `ci-quote`.
    pub id: &'static str,
    /// 1 basic … 4 advanced.
    pub level: u8,
    /// The commands, marked for the glossary: "`ci\"`".
    pub keys: &'static str,
    /// What it does, in one line.
    pub what: &'static str,
    /// When it helps.
    pub example: &'static str,
    recognise: fn(&Step) -> bool,
    /// Where it's taught and counted.
    pub places: &'static [ContextKind],
    /// Seen as a command the shell hook reports, rather than as keys.
    pub shape: Option<CommandShape>,
    /// Seen as a VS Code action done with its shortcut.
    pub ui: Option<UiWhat>,
}

/// Editing in Vim's ways: Vim, VS Code, Claude Code, vi-mode shells (each
/// where it supports the item; see `crate::support`).
const EDITING: &[ContextKind] = &[
    ContextKind::TerminalVim,
    ContextKind::VsCodeVim,
    ContextKind::ClaudeVim,
    ContextKind::BashVi,
];
const IN_VSCODE: &[ContextKind] = &[ContextKind::VsCodeVim];
const IN_SHELL: &[ContextKind] = &[ContextKind::BashVi, ContextKind::Shell];

impl Item {
    pub fn used_in(&self, step: &Step) -> bool {
        (self.recognise)(step)
    }

    /// Used while typing, in Insert mode: its keys mean their Insert-mode
    /// things.
    pub fn while_typing(&self) -> bool {
        self.keys.contains("while typing")
    }

    pub fn applies_in(&self, ctx: ContextKind) -> bool {
        self.places.contains(&ctx)
    }
}

/// Items the shell hook's `shape` shows you used.
pub fn used_by_shape(shape: CommandShape) -> impl Iterator<Item = &'static Item> {
    CURRICULUM.iter().filter(move |i| i.shape == Some(shape))
}

/// Items a VS Code action done with its shortcut shows you used.
pub fn used_by_ui(what: UiWhat) -> impl Iterator<Item = &'static Item> {
    CURRICULUM.iter().filter(move |i| i.ui == Some(what))
}

fn cmd(step: &Step) -> Option<&Command> {
    command(step)
}

fn is(step: &Step, spellings: &[&str]) -> bool {
    cmd(step).is_some_and(|c| spellings.iter().any(|s| c.is(s)))
}

fn head(step: &Step, heads: &str) -> bool {
    cmd(step).is_some_and(|c| c.head().is_some_and(|h| heads.contains(h)))
}

fn ctrl(step: &Step, keys: &str) -> bool {
    cmd(step).is_some_and(|c| {
        c.single()
            .is_some_and(|k| k.mods.ctrl && matches!(k.sym, Sym::Char(ch) if keys.contains(ch)))
    })
}

/// An operator (`d`, `c`, `y`, …) with a text object `i`/`a` + one of `objects`.
fn object(step: &Step, ops: &str, objects: &str) -> bool {
    cmd(step).is_some_and(|c| {
        let k: Vec<char> = c.keys.iter().filter_map(|k| k.plain_char()).collect();
        k.len() == 3 && ops.contains(k[0]) && matches!(k[1], 'i' | 'a') && objects.contains(k[2])
    })
}

/// An operator followed by a motion starting with one of `motions`.
fn op_motion(step: &Step, ops: &str, motions: &str) -> bool {
    cmd(step).is_some_and(|c| {
        let k: Vec<char> = c.keys.iter().filter_map(|k| k.plain_char()).collect();
        k.len() >= 2 && ops.contains(k[0]) && motions.contains(k[1])
    })
}

fn uncounted(step: &Step, spellings: &[&str]) -> bool {
    cmd(step).is_some_and(|c| c.count.is_none() && spellings.iter().any(|s| c.is(s)))
}

/// A `:` command whose text `test` accepts.
fn cmdline_is(step: &Step, test: impl Fn(&str) -> bool) -> bool {
    matches!(&step.event, Some(VimEvent::Cmdline { kind: ':', text: Some(t) }) if test(t.trim()))
}

fn counted(step: &Step, heads: &str) -> bool {
    cmd(step).is_some_and(|c| c.count.is_some() && c.head().is_some_and(|h| heads.contains(h)))
}

fn cmdline(step: &Step, prefixes: &[&str]) -> bool {
    match &step.event {
        Some(VimEvent::Cmdline {
            kind: ':',
            text: Some(t),
        }) => {
            let t = t.trim_start();
            prefixes.iter().any(|p| t.starts_with(p))
        }
        _ => false,
    }
}

/// The Ex command typed, by name: `names` holds (shortest, full)
/// spellings, as `:help` writes them (`ri[ght]` is ("ri", "right")); a
/// range before it is fine.
fn ex(step: &Step, names: &[(&str, &str)]) -> bool {
    let Some(VimEvent::Cmdline {
        kind: ':',
        text: Some(t),
    }) = &step.event
    else {
        return false;
    };
    ex_word(t).is_some_and(|w| {
        names
            .iter()
            .any(|(short, full)| w.starts_with(short) && full.starts_with(w))
    })
}

/// The command name in a command line: `:.,+3>` → `>`, `:'<,'>sort` →
/// `sort`, `:5,10m 0` → `m`.
fn ex_word(text: &str) -> Option<&str> {
    let mut rest = text.trim_start();
    // A range: numbers, . $ %, marks, patterns, offsets.
    loop {
        let before = rest.len();
        rest = rest.trim_start_matches(|c: char| c.is_ascii_digit() || ".$%,;+- ".contains(c));
        if let Some(r) = rest.strip_prefix('\'') {
            rest = r.get(1..).unwrap_or("");
        }
        for delim in ['/', '?'] {
            if let Some(r) = rest.strip_prefix(delim) {
                rest = r.split_once(delim).map_or("", |(_, after)| after);
            }
        }
        if rest.len() == before {
            break;
        }
    }
    let first = rest.chars().next()?;
    let end = if first.is_ascii_alphabetic() {
        rest.find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(rest.len())
    } else if matches!(first, '<' | '>') {
        // `:<` and `:>` repeat: `:>>>`.
        rest.len() - rest.trim_start_matches(first).len()
    } else if "&!=~#@*".contains(first) {
        1
    } else {
        0
    };
    (end > 0).then(|| &rest[..end])
}

/// A window command: `Ctrl-w` then one of `keys` (or its Ctrl- twin).
fn wincmd(step: &Step, keys: &str) -> bool {
    cmd(step).is_some_and(|c| {
        c.keys.len() == 2
            && c.keys[0].is_ctrl('w')
            && match c.keys[1].sym {
                Sym::Char(k) => keys.contains(k),
                _ => false,
            }
    })
}

const VIM_AND_VSCODE: &[ContextKind] = &[ContextKind::TerminalVim, ContextKind::VsCodeVim];
const VIM_ONLY: &[ContextKind] = &[ContextKind::TerminalVim];

fn insert_key(step: &Step) -> Option<InsertKey> {
    crate::rules::vim::insert_key(step)
}

fn insert_ctrl(step: &Step, keys: &str) -> bool {
    matches!(insert_key(step), Some(InsertKey::Ctrl(c)) if keys.contains(c))
}

fn cmdline_key(step: &Step) -> Option<InsertKey> {
    match step.event {
        Some(VimEvent::CmdlineKey(k)) => Some(k),
        _ => None,
    }
}

fn cmdline_ctrl(step: &Step, keys: &str) -> bool {
    matches!(cmdline_key(step), Some(InsertKey::Ctrl(c)) if keys.contains(c))
}

fn visual_then(step: &Step, keys: &str) -> bool {
    step.before.is_visual() && head(step, keys)
}

macro_rules! item {
    ($id:literal, $level:literal, $keys:literal, $what:literal, $example:literal, $rec:expr) => {
        Item {
            id: $id,
            level: $level,
            keys: $keys,
            what: $what,
            example: $example,
            recognise: $rec,
            places: EDITING,
            shape: None,
            ui: None,
        }
    };
    ($id:literal, $level:literal, $keys:literal, $what:literal, $example:literal, $rec:expr,
     $places:expr, $shape:expr, $ui:expr) => {
        Item {
            id: $id,
            level: $level,
            keys: $keys,
            what: $what,
            example: $example,
            recognise: $rec,
            places: $places,
            shape: $shape,
            ui: $ui,
        }
    };
}

/// Basic to advanced; within a level, roughly the order worth learning.
pub static CURRICULUM: &[Item] = &[
    // Level 1: moving and editing without the arrow keys.
    item!(
        "word",
        1,
        "`w` and `b`",
        "jump forward or back a word",
        "Skip across a line a word at a time instead of holding `l`.",
        |s| head(s, "wb") && cmd(s).is_some_and(|c| c.keys.len() == 1)
    ),
    item!(
        "word-end",
        1,
        "`e`",
        "jump to the end of the word",
        "Land on the last letter, ready to append with `a`.",
        |s| is(s, &["e"])
    ),
    item!(
        "line-ends",
        1,
        "`$`",
        "the end of the line",
        "Get to the end of a long line in one key.",
        |s| is(s, &["$"])
    ),
    item!(
        "line-start",
        1,
        "`0`",
        "the very start of the line",
        "Back to column one, before the indentation.",
        |s| is(s, &["0"])
    ),
    item!(
        "first-nonblank",
        1,
        "`^`",
        "the first character of the line that isn't a space",
        "Start of the code on an indented line.",
        |s| is(s, &["^"])
    ),
    item!(
        "file-ends",
        1,
        "`gg` and `G`",
        "the first or last line of the file",
        "Jump to the top to check the imports.",
        // `5G` is going to line 5, not the end.
        |s| uncounted(s, &["gg", "G"])
    ),
    item!(
        "append",
        1,
        "`A` and `I`",
        "start typing at the end or start of the line",
        "Add a semicolon at the end without moving there first.",
        |s| is(s, &["A", "I"])
    ),
    item!(
        "open-line",
        1,
        "`o` and `O`",
        "open a new line below or above and type",
        "Add a line under this one from anywhere in it.",
        |s| is(s, &["o", "O"])
    ),
    item!(
        "undo-redo",
        1,
        "`u` and `Ctrl-r`",
        "undo, and redo what you undid",
        "Step back and forward through your changes.",
        // In Visual mode `u` makes text lower case.
        |s| (is(s, &["u"]) && !s.before.is_visual()) || ctrl(s, "r")
    ),
    item!(
        "count",
        1,
        "`5j`",
        "a number before a motion repeats it",
        "Move five lines with two keys instead of five.",
        |s| counted(s, "jkhlwbe")
    ),
    item!(
        "dd",
        1,
        "`dd`",
        "delete the whole line",
        "Remove a line from anywhere in it.",
        |s| is(s, &["dd"])
    ),
    item!(
        "change-word",
        1,
        "`cw`",
        "change from the cursor to the end of the word",
        "Rename the rest of a word and keep typing.",
        |s| is(s, &["cw", "ce"])
    ),
    item!(
        "find",
        1,
        "`f<char>` and `;`",
        "jump to the next <char> on the line, `;` for the one after",
        "Get to the next comma in two keys.",
        |s| head(s, "fF") || is(s, &[";", ","])
    ),
    item!(
        "dot",
        1,
        "`.`",
        "repeat the last change",
        "Fix the same thing in several places: change it once, then `.` each time.",
        |s| is(s, &["."])
    ),
    item!(
        "operator",
        1,
        "`d}` and `yw`",
        "an operator then a motion: delete, change or copy as far as the motion goes",
        "Delete to the end of the paragraph, or copy a word, without selecting first.",
        |s| op_motion(s, "dcy", "wbeWBE$0^{}()GfFtTnN%HLM`'jk")
    ),
    item!(
        "yank-put",
        1,
        "`yy` and `p`",
        "copy (yank) the line, and put it after the cursor (`P` before)",
        "Duplicate a line: `yy` then `p`.",
        |s| (head(s, "y") && !s.before.is_visual()) || is(s, &["Y", "p", "P"])
    ),
    item!(
        "replace-char",
        1,
        "`r<char>`",
        "replace the character under the cursor (or every selected one), staying in Normal mode",
        "Fix a typo'd letter in two keys.",
        |s| cmd(s).is_some_and(|c| c.keys.len() == 2 && c.keys[0].plain_char() == Some('r'))
    ),
    // Level 2: bigger moves and text objects.
    item!(
        "search",
        1,
        "`/` and a few letters",
        "jump to the next place those letters appear",
        "See the word you want three screens down? Search for it.",
        |s| matches!(
            &s.event,
            Some(VimEvent::Cmdline {
                kind: '/' | '?',
                text: Some(_)
            })
        )
    ),
    item!(
        "history-search",
        1,
        "`Ctrl-r`",
        "search your shell history as you type",
        "Find last week's long docker command by a word in it.",
        |s| s.before == VimMode::Insert
            && matches!(
                crate::rules::vim::insert_key(s),
                Some(crate::vim::InsertKey::CtrlR)
            ),
        IN_SHELL,
        None,
        None
    ),
    item!(
        "quick-open",
        1,
        "`Ctrl-P`",
        "open a file by typing part of its name",
        "Jump to a file without clicking through the explorer.",
        |_| false,
        IN_VSCODE,
        None,
        Some(UiWhat::FileOpened)
    ),
    item!(
        "completion",
        2,
        "`Ctrl-n`",
        "complete the word you're typing from words in your files",
        "Type the start of a long name and let Vim finish it.",
        |s| matches!(insert_key(s), Some(InsertKey::Ctrl('n' | 'p')))
    ),
    item!(
        "screen-lines",
        2,
        "`gj` and `gk`",
        "down or up a line as it's shown, when long lines wrap",
        "Move through a long paragraph of prose line by line.",
        |s| is(s, &["gj", "gk"])
    ),
    item!(
        "paragraph",
        2,
        "`}` and `{`",
        "jump to the next or previous blank line",
        "Move between functions or paragraphs.",
        |s| head(s, "{}")
    ),
    item!(
        "goto-line",
        2,
        "`42G` and `:42`",
        "jump straight to a line by its number",
        "Go to the line an error message names.",
        // `:42` alone: `:1,5d` is a range.
        |s| counted(s, "G")
            || cmdline_is(s, |t| !t.is_empty()
                && t.chars().all(|c| c.is_ascii_digit()))
    ),
    item!(
        "half-page",
        2,
        "`Ctrl-d` and `Ctrl-u`",
        "move half a page down or up",
        "Scan a long file without holding `j`.",
        |s| ctrl(s, "du")
    ),
    item!(
        "star",
        2,
        "`*`",
        "jump to the next use of the word under the cursor",
        "Find where a variable is used next.",
        |s| head(s, "*")
    ),
    item!(
        "star-back",
        2,
        "`#`",
        "jump to the previous use of the word under the cursor",
        "Find where a variable was set.",
        |s| head(s, "#")
    ),
    item!(
        "search-next",
        2,
        "`n`",
        "the next match of your last search",
        "After `/name`, hop through every match.",
        |s| is(s, &["n"])
    ),
    item!(
        "search-back",
        2,
        "`N`",
        "the previous match of your last search",
        "Went past the one you wanted? Back one.",
        |s| is(s, &["N"])
    ),
    item!(
        "percent",
        2,
        "`%`",
        "jump to the matching bracket",
        "From an opening brace to its closing one.",
        |s| is(s, &["%"])
    ),
    item!(
        "screen",
        2,
        "`H` and `L`",
        "the top or bottom line of the window",
        "Jump to something you can see without counting lines.",
        |s| is(s, &["H", "L"])
    ),
    item!(
        "screen-middle",
        2,
        "`M`",
        "the middle line of the window",
        "Halfway down what you see, in one key.",
        |s| is(s, &["M"])
    ),
    item!(
        "ciw",
        2,
        "`ciw`",
        "change the whole word under the cursor, from anywhere in it",
        "Rename a variable without moving to its start.",
        // Any operator on a word: `diw` and `yiw` are the same skill.
        |s| object(s, "cdy", "wW")
    ),
    item!(
        "ci-quote",
        2,
        "`ci\"`",
        "change everything inside the quotes, from anywhere in them",
        "Replace a string's text in one go.",
        |s| object(s, "cdy", "\"'`")
    ),
    item!(
        "ci-paren",
        2,
        "`ci(`",
        "change everything inside the brackets",
        "Rewrite a function's arguments.",
        |s| object(s, "cdy", "()b[]{}B<>")
    ),
    item!(
        "d-to-end",
        2,
        "`D` and `C`",
        "delete or change to the end of the line",
        "Rewrite the rest of a line.",
        |s| is(s, &["D", "C"])
    ),
    item!(
        "substitute",
        2,
        "`s` and `S`",
        "replace a character, or the whole line (also `cc`), and type",
        "Fix one wrong letter and keep typing.",
        |s| is(s, &["s", "S", "cc"])
    ),
    item!(
        "clipboard",
        2,
        "`\"+y` and `\"+p`",
        "copy to and paste from the system clipboard",
        "Copy a function to paste into a chat or another program.",
        |s| cmd(s).is_some_and(|c| c.register.is_some_and(|r| "+*".contains(r)))
    ),
    item!(
        "goto-def",
        2,
        "`gd`",
        "go to where the word under the cursor is defined (`gD` for the whole file)",
        "Jump from a variable's use to where it was declared.",
        |s| is(s, &["gd", "gD"])
    ),
    item!(
        "last-nonblank",
        2,
        "`g_`",
        "to the last character of the line that isn't a space",
        "Land on the semicolon, not on trailing spaces.",
        |s| is(s, &["g_"])
    ),
    item!(
        "insert-delete",
        2,
        "`Ctrl-w` and `Ctrl-u` while typing",
        "delete the word, or everything, you typed before the cursor",
        "Retype a word without holding Backspace.",
        |s| matches!(insert_key(s), Some(InsertKey::CtrlW | InsertKey::CtrlU))
    ),
    item!(
        "cmdline-complete",
        2,
        "`Tab` on the command line",
        "complete a command or file name (`Ctrl-d` lists the choices)",
        "Type `:e src/ma` then `Tab` for the file name.",
        |s| cmdline_ctrl(s, "idnpla")
    ),
    item!(
        "cmdline-history",
        2,
        "`:` then a few letters and `Up`",
        "bring back an earlier command that starts with those letters",
        "Rerun last hour's `:%s` without retyping it.",
        |s| matches!(cmdline_key(s), Some(InsertKey::Arrow(_)))
    ),
    item!(
        "page",
        2,
        "`Ctrl-f` and `Ctrl-b`",
        "a whole screen forward or back",
        "Page through a long file you're reading.",
        |s| ctrl(s, "fb")
    ),
    item!(
        "delete-back",
        2,
        "`X`",
        "delete the character before the cursor",
        "Remove the letter you just passed without moving back first.",
        |s| is(s, &["X"]) && !s.before.is_visual()
    ),
    item!(
        "shift",
        2,
        "`>>` and `<<`",
        "indent or unindent the line (`>` with a motion for more)",
        "Indent a line one level from anywhere in it.",
        |s| is(s, &[">>", "<<"]) || op_motion(s, "<>", "wbeWBE$0^{}()GjkLHM%")
    ),
    item!(
        "reindent",
        2,
        "`==` and `=ap`",
        "re-indent the line, or a paragraph, the way the file's indenting says",
        "Fix a pasted line's indentation in two keys.",
        |s| is(s, &["=="])
            || op_motion(s, "=", "wbeWBE$0^{}()GjkLHM%")
            || object(s, "=", "pwsb(){}B[]")
    ),
    item!(
        "join",
        2,
        "`J`",
        "join the next line onto this one",
        "Pull a wrapped line back up.",
        |s| is(s, &["J"])
    ),
    item!(
        "visual-line",
        2,
        "`V`",
        "select whole lines",
        "Select a block of lines to move or indent.",
        |s| is(s, &["V"])
    ),
    item!(
        "jump-back",
        2,
        "`Ctrl-o`",
        "back to where you were before a jump",
        "Return after `gg` or a search.",
        |s| ctrl(s, "o") && s.before == VimMode::Normal
    ),
    item!(
        "jump-last",
        2,
        "``` `` ```",
        "back to where you jumped from, and again to swap back",
        "Flip between two places after a search or `G`.",
        |s| is(s, &["``", "''"])
    ),
    item!(
        "jump-forward",
        2,
        "`Ctrl-i`",
        "forward again after `Ctrl-o`",
        "Went back one jump too many? Forward again.",
        |s| ctrl(s, "i") && s.before == VimMode::Normal
    ),
    item!(
        "center",
        2,
        "`zz`, `zt` and `zb`",
        "scroll so this line is in the middle, top or bottom",
        "Bring the line you're on into the middle of the screen.",
        // `z.`, `z<CR>` and `z-` do the same, landing on the first non-blank.
        |s| is(s, &["zz", "zt", "zb", "z.", "z-"])
            || cmd(s).is_some_and(|c| {
                c.keys.len() == 2
                    && c.keys[0].plain_char() == Some('z')
                    && c.keys[1].sym == Sym::Enter
            })
    ),
    item!(
        "dap",
        2,
        "`dap` and `yap`",
        "delete or copy a paragraph and the blank line after it",
        "Move a whole block of code.",
        |s| object(s, "dcy", "p")
    ),
    // Level 3: power tools.
    item!(
        "cd-back",
        2,
        "`cd -`",
        "back to the folder you were in before",
        "Hop between two project folders.",
        |_| false,
        IN_SHELL,
        Some(CommandShape::CdBack),
        None
    ),
    item!(
        "last-arg",
        2,
        "`Alt-.`",
        "insert the previous command's last argument",
        "mkdir a folder, then cd into it without typing its name again.",
        |_| false,
        IN_SHELL,
        None,
        None
    ),
    item!(
        "tabs",
        2,
        "`gt` and `gT`",
        "the next or previous editor tab",
        "Flip between two files you're working on.",
        |s| is(s, &["gt", "gT"]),
        VIM_AND_VSCODE,
        None,
        None
    ),
    item!(
        "terminal-toggle",
        2,
        "`` Ctrl-` ``",
        "to the terminal and back",
        "Run the tests, then straight back to the code.",
        |_| false,
        IN_VSCODE,
        None,
        Some(UiWhat::TerminalFocused)
    ),
    item!(
        "marks",
        3,
        "`ma` and `'a`",
        "set a mark, and jump back to its line",
        "Bookmark a spot you keep returning to.",
        // A letter after it: `''` and `` `` `` jump back, `'.` is the last change.
        |s| head(s, "m'`")
            && cmd(s).is_some_and(|c| {
                c.keys.len() == 2
                    && c.keys[1]
                        .plain_char()
                        .is_some_and(|k| k.is_ascii_alphabetic())
            })
    ),
    item!(
        "last-change",
        3,
        "`` `. ``",
        "jump to where you last changed something",
        "Get back to where you were editing.",
        |s| is(s, &["`.", "'."])
    ),
    item!(
        "gv",
        3,
        "`gv`",
        "select the last selection again",
        "Re-select what you just indented.",
        |s| is(s, &["gv"])
    ),
    item!(
        "gi",
        3,
        "`gi`",
        "start typing where you last stopped typing",
        "Go back to your edit after looking around.",
        |s| is(s, &["gi"])
    ),
    item!(
        "till",
        3,
        "`t<char>` and `dt<char>`",
        "move to just before a character (`T` backwards), or delete up to it",
        "Remove the rest of an argument up to the comma.",
        |s| op_motion(s, "dcy", "tT")
            || (head(s, "tT") && cmd(s).is_some_and(|c| c.keys.len() == 2))
    ),
    item!(
        "format",
        3,
        "`gqip` and `gw`",
        "wrap lines to the text width, over a motion (`gw` keeps the cursor)",
        "Rewrap a comment paragraph after editing it.",
        |s| cmd(s).is_some_and(|c| {
            let k: Vec<char> = c.keys.iter().filter_map(|k| k.plain_char()).collect();
            k.len() >= 2 && k[0] == 'g' && matches!(k[1], 'q' | 'w')
        })
    ),
    item!(
        "join-raw",
        3,
        "`gJ`",
        "join the next line on without adding a space",
        "Glue a split identifier back together.",
        |s| is(s, &["gJ"])
    ),
    item!(
        "change-marks",
        3,
        "`` `[ `` and `` `] ``",
        "the start or end of what you last changed or pasted",
        "Jump to the end of a block you just pasted.",
        |s| is(s, &["`[", "`]", "'[", "']"])
    ),
    item!(
        "put-more",
        3,
        "`gp` and `]p`",
        "paste and land after it, or paste with the indent of this line",
        "Paste several lines in a row with `gp gp`, or paste code into a deeper block.",
        |s| is(s, &["gp", "gP", "]p", "[p", "]P", "[P"])
    ),
    item!(
        "visual-put",
        3,
        "`P` in Visual mode",
        "replace the selection with what you copied, and keep it copied",
        "Paste the same word over several others: `viwP`, move, `viwP`.",
        |s| visual_then(s, "pP")
    ),
    item!(
        "black-hole",
        3,
        "`\"_d`",
        "delete without overwriting what you copied",
        "Clear a line and still paste the word you yanked.",
        |s| cmd(s).is_some_and(|c| c.register.is_some_and(|r| "_".contains(r)))
    ),
    item!(
        "word-end-back",
        3,
        "`ge`",
        "back to the end of the previous word (`gE` for WORDs)",
        "Fix the end of the word you just passed.",
        |s| is(s, &["ge", "gE"])
    ),
    item!(
        "insert-col1",
        3,
        "`gI`",
        "start typing in column one, before the indentation",
        "Comment out a line at its very start.",
        |s| is(s, &["gI"])
    ),
    item!(
        "goto-file",
        3,
        "`gf`",
        "open the file whose name is under the cursor (`gF` at its line number)",
        "Follow an import or an error message's path.",
        |s| is(s, &["gf", "gF"])
    ),
    item!(
        "star-partial",
        3,
        "`g*`",
        "like `*`, but matching inside longer words too",
        "Find `count` in `countWords` as well.",
        |s| is(s, &["g*", "g#"])
    ),
    item!(
        "file-info",
        3,
        "`Ctrl-g` and `g Ctrl-g`",
        "show the file name and where you are, or count words",
        "Check a draft's word count.",
        |s| !s.before.is_visual()
            && (ctrl(s, "g")
                || cmd(s).is_some_and(|c| c.keys.len() == 2
                    && c.keys[0].plain_char() == Some('g')
                    && c.keys[1].is_ctrl('g'))
                || ex(s, &[("f", "file")]))
    ),
    item!(
        "spell",
        3,
        "`]s` and `z=`",
        "the next misspelled word, and suggestions for it (`zg` adds a word)",
        "Fix the typos in a README one by one.",
        |s| is(s, &["]s", "[s", "]S", "[S", "z=", "zg", "zw"])
    ),
    item!(
        "visual-swap",
        3,
        "`o` in Visual mode",
        "jump to the other end of the selection, to grow it from there",
        "Selected one line too few at the top? `o` and extend it.",
        |s| visual_then(s, "oO")
    ),
    item!(
        "visual-linewise",
        3,
        "`Y` and `D` in Visual mode",
        "copy or delete the selected lines whole, wherever the selection starts",
        "Delete every line a selection touches.",
        |s| visual_then(s, "YDXR")
    ),
    item!(
        "insert-indent",
        3,
        "`Ctrl-t` and `Ctrl-d` while typing",
        "indent or unindent the line without leaving Insert mode",
        "Fix the indent of the line you're writing as you go.",
        |s| insert_ctrl(s, "td")
    ),
    item!(
        "cmdline-word",
        3,
        "`Ctrl-r Ctrl-w` on the command line",
        "put the word under the cursor into the command you're typing",
        "Start `:%s/` and pull in the name you're on instead of typing it.",
        |s| matches!(cmdline_key(s), Some(InsertKey::Then('r', _)))
    ),
    item!(
        "cmdline-edit",
        3,
        "`Ctrl-b`, `Ctrl-e` and `Ctrl-w` on the command line",
        "to the start or end of the command, or delete the word before the cursor",
        "Fix the start of a long command without arrowing back.",
        |s| cmdline_ctrl(s, "bewuvkq")
    ),
    item!(
        "scroll-line",
        3,
        "`Ctrl-e` and `Ctrl-y`",
        "scroll the view a line down or up, leaving the cursor where it is",
        "Show one more line of context below without moving.",
        |s| ctrl(s, "ey")
    ),
    item!(
        "line-first",
        3,
        "`+` and `-`",
        "the first character of the next or previous line (Enter and `_` too)",
        "Go down a line and land on its text, not its indentation.",
        |s| is(s, &["+", "-", "_"])
            || cmd(s).is_some_and(|c| c.single().is_some_and(|k| k.sym == Sym::Enter))
    ),
    item!(
        "column",
        3,
        "`20|`",
        "go to a column of the line",
        "Line up with column 80 to check a line's length.",
        |s| is(s, &["|"])
    ),
    item!(
        "bigword",
        3,
        "`W`, `B` and `E`",
        "move by WORDs: anything between spaces",
        "Skip over `foo.bar(baz)` in one jump.",
        |s| head(s, "WBE") && cmd(s).is_some_and(|c| c.keys.len() == 1)
    ),
    item!(
        "indent-object",
        3,
        "`>ip` and `=ip`",
        "indent or re-indent a whole paragraph",
        "Fix the indentation of a block in three keys.",
        |s| object(s, "><=", "p{}B")
    ),
    item!(
        "macro",
        3,
        "`qq` … `q` and `@q`",
        "record keys into a macro, then replay them",
        "Make the same edit on twenty lines.",
        |s| head(s, "q@") && cmd(s).is_some_and(|c| c.keys.len() == 2)
    ),
    item!(
        "replay-macro",
        3,
        "`@@`",
        "replay the last macro again",
        "Run a macro once more, and again.",
        |s| is(s, &["@@"])
    ),
    item!(
        "cgn",
        3,
        "`cgn`",
        "change the next search match; then `.` jumps to the next one and changes it",
        "Rename a variable one use at a time, skipping some with `n`.",
        |s| is(s, &["cgn", "dgn"])
    ),
    item!(
        "registers",
        3,
        "`\"0p`",
        "paste the last yank, even after deleting something since",
        "Yank a word, delete the old one, and still paste the yanked word.",
        // Not `"_`: that's the black hole, for throwing text away.
        |s| cmd(s).is_some_and(|c| c.register.is_some_and(|r| r != '_'))
    ),
    item!(
        "windows",
        3,
        "`Ctrl-w h` and `Ctrl-^`",
        "go straight to the window on the left (or j, k, l), and switch back to the last file",
        "Hop between a split's two halves, or between two files, in two keys.",
        |s| cmd(s).is_some_and(|c| {
            (c.keys.len() == 2
                && c.keys[0].is_ctrl('w')
                && c.keys[1].plain_char().is_some_and(|k| "hjkl".contains(k)))
                || c.single().is_some_and(|k| k.is_ctrl('^') || k.is_ctrl('6'))
        })
    ),
    item!(
        "insert-keys",
        3,
        "`Ctrl-r\"`",
        "paste while typing, without leaving Insert mode (and Ctrl-o runs one Normal-mode command)",
        "Paste a yanked name into the middle of a sentence you're typing.",
        |s| matches!(
            crate::rules::vim::insert_key(s),
            Some(crate::vim::InsertKey::CtrlO | crate::vim::InsertKey::CtrlR)
        )
    ),
    item!(
        "change-list",
        3,
        "`g;`",
        "jump back to where you last changed something, then older changes",
        "Return to the function you were editing after reading elsewhere.",
        |s| is(s, &["g;", "g,"])
    ),
    item!(
        "undo-time",
        3,
        "`:earlier 1m` and `g-`",
        "go back through undo history by time, or step by step across branches",
        "Get back to how the file was before the last few minutes of edits.",
        |s| is(s, &["g-", "g+"]) || cmdline(s, &["earlier", "later"])
    ),
    item!(
        "case",
        3,
        "`~` and `gU`",
        "switch case, or make upper case over a motion (`U` and `u` on a selection)",
        "Turn `const` into `CONST` with `gUiw`.",
        |s| is(s, &["~"]) || op_motion(s, "g", "Uu~") || visual_then(s, "uU~")
    ),
    item!(
        "increment",
        3,
        "`Ctrl-a` and `Ctrl-x`",
        "add or subtract one from the number under the cursor",
        "Bump a version number without retyping it.",
        |s| ctrl(s, "ax")
    ),
    item!(
        "replace-mode",
        3,
        "`R`",
        "type over existing text",
        "Overwrite a fixed-width value.",
        |s| is(s, &["R"])
    ),
    item!(
        "visual-block",
        3,
        "`Ctrl-v`",
        "select a rectangle of text",
        "Add the same text at the start of several lines.",
        // From Visual mode too: `v` then `Ctrl-v` makes it a block.
        |s| ctrl(s, "v") && (s.before == VimMode::Normal || s.before.is_visual())
    ),
    item!(
        "block-append",
        3,
        "`Ctrl-v`, `$`, then `A`",
        "add text at the end of several lines, however long they are",
        "Put a semicolon at the end of ten lines at once.",
        |s| s.before == VimMode::VisualBlock && head(s, "A")
    ),
    item!(
        "sentence",
        3,
        "`das` and `cis`",
        "delete or change a whole sentence, from anywhere in it",
        "Rewrite a sentence in a comment or a README.",
        |s| object(s, "cdy", "s")
    ),
    item!(
        "gn-select",
        3,
        "`gn`",
        "select the next search match",
        "Search, then `gn` and `U` to upper-case each match in turn.",
        |s| is(s, &["gn", "gN"]) && !s.before.is_visual()
    ),
    item!(
        "section",
        3,
        "`]]` and `[[`",
        "the next or previous section: a line starting with `{`",
        "Hop between functions in C, where `{` starts the line.",
        |s| is(s, &["]]", "[[", "][", "[]"])
    ),
    item!(
        "unmatched",
        3,
        "`[(` and `])`",
        "the opening or closing bracket around the cursor",
        "From inside a long argument list, back to its `(`.",
        |s| is(s, &["[(", "])", "[{", "]}"])
    ),
    item!(
        "sentence-motion",
        3,
        "`(` and `)`",
        "back or forward a sentence",
        "Move through comments and documentation a sentence at a time.",
        |s| is(s, &["(", ")"])
    ),
    item!(
        "fold",
        3,
        "`za`, `zc` and `zo`",
        "toggle, close or open the fold under the cursor",
        "Hide a long function to see what's around it.",
        |s| is(s, &["za", "zc", "zo", "zR", "zM"])
    ),
    item!(
        "tag-object",
        3,
        "`cit`",
        "change everything inside an HTML/XML tag",
        "Rewrite a tag's contents.",
        |s| object(s, "cdy", "t")
    ),
    item!(
        "visual-object",
        3,
        "`vi(` then an operator",
        "select inside brackets, quotes or a paragraph",
        "Check what you're about to change first.",
        |s| s.before.is_visual()
            && cmd(s).is_some_and(|c| c.keys.len() == 2 && matches!(c.head(), Some('i' | 'a')))
    ),
    item!(
        "visual-indent",
        3,
        "`>` in Visual mode",
        "indent the selected lines",
        "Indent a selection of lines at once.",
        |s| visual_then(s, "<>=")
    ),
    // Level 4: the command line.
    item!(
        "quick-sub",
        3,
        "`^old^new`",
        "run the last command again with one word swapped",
        "Rerun a test with another file name.",
        |_| false,
        IN_SHELL,
        None,
        None
    ),
    item!(
        "split",
        3,
        "`Ctrl-w v`",
        "a second editor side by side",
        "Read the test while you write the code.",
        |s| wincmd(s, "vsSn")
            || ex(
                s,
                &[
                    ("sp", "split"),
                    ("vs", "vsplit"),
                    ("new", "new"),
                    ("vne", "vnew")
                ]
            ),
        VIM_AND_VSCODE,
        None,
        None
    ),
    item!(
        "window-close",
        3,
        "`Ctrl-w o` and `Ctrl-w q`",
        "keep only this window, or close it",
        "Close every split but the one you're in.",
        |s| wincmd(s, "ocq") || ex(s, &[("on", "only"), ("clo", "close")]),
        VIM_AND_VSCODE,
        None,
        None
    ),
    item!(
        "window-cycle",
        3,
        "`Ctrl-w w`",
        "to the next window, round and round (`Ctrl-w p`: the last one)",
        "Hop between splits without thinking about where they are.",
        |s| wincmd(s, "wWptb")
            || cmd(s).is_some_and(|c| c.keys.len() == 2
                && c.keys[0].is_ctrl('w')
                && c.keys[1].is_ctrl('w')),
        VIM_AND_VSCODE,
        None,
        None
    ),
    item!(
        "tab-cmds",
        3,
        "`:tabnew` and `:tabonly`",
        "open a tab, or close all the others (`:tabclose`, `:tabmove` too)",
        "Clear out tabs you no longer need.",
        |s| ex(
            s,
            &[
                ("tabnew", "tabnew"),
                ("tabe", "tabedit"),
                ("tabo", "tabonly"),
                ("tabc", "tabclose"),
                ("tabm", "tabmove"),
                ("tabn", "tabnext"),
                ("tabp", "tabprevious"),
                ("tabfir", "tabfirst"),
                ("tabl", "tablast"),
                ("tabs", "tabs")
            ]
        ),
        VIM_AND_VSCODE,
        None,
        None
    ),
    item!(
        "substitute-cmd",
        4,
        "`:s/old/new/`",
        "replace text on this line",
        "Rename something on the current line.",
        |s| cmdline(s, &["s/", "s#"])
    ),
    item!(
        "substitute-all",
        4,
        "`:%s/old/new/g`",
        "replace text everywhere in the file",
        "Rename a variable in the whole file.",
        |s| cmdline(s, &["%s"])
    ),
    item!(
        "quickfix",
        4,
        "`:cn` and `:cp`",
        "to the next or previous error or search result in the quickfix list",
        "After `:make` or `:grep`, walk through the results.",
        |s| cmdline(s, &["cn", "cp", "cnext", "cprev", "copen", "cc"])
    ),
    item!(
        "global",
        4,
        "`:g/pattern/d`",
        "run a command on every line that matches",
        "Delete every line with `TODO`.",
        |s| cmdline(s, &["g/", "g!", "v/"])
    ),
    item!(
        "normal-cmd",
        4,
        "`:norm`",
        "run Normal-mode keys on many lines",
        "Append a semicolon to every selected line: `:norm A;`.",
        |s| cmdline(s, &["norm", "'<,'>norm"])
    ),
    item!(
        "range",
        4,
        "`:.,+5d`",
        "run a command on a range of lines",
        "Delete the next five lines from the command line.",
        |s| cmdline(s, &[".,", "'<,'>"])
    ),
    item!(
        "sort",
        4,
        "`:sort`",
        "sort the selected lines",
        "Sort a list of imports.",
        |s| cmdline(s, &["sort", "'<,'>sort"])
    ),
    item!(
        "filter",
        4,
        "`!!` and `!ip`",
        "run lines through a shell command and put back what it prints",
        "Sort a paragraph with `!ip sort`, or format JSON with `!!jq .`.",
        |s| head(s, "!")
            || cmdline_is(s, |t| t.contains('!')
                && !t.starts_with('!')
                && ex_word(t) == Some("!"))
    ),
    item!(
        "retab",
        4,
        "`:retab`",
        "turn tabs into spaces (or back) by the file's settings",
        "Clean up a file with mixed tabs and spaces.",
        |s| ex(s, &[("ret", "retab")])
    ),
    item!(
        "align",
        4,
        "`:center`, `:right` and `:left`",
        "line up lines in the middle, on the right or the left",
        "Center a heading in a plain-text file.",
        |s| ex(s, &[("ce", "center"), ("ri", "right"), ("le", "left")])
    ),
    item!(
        "visual-marks",
        4,
        "`` `< `` and `` `> ``",
        "the start or end of the last selection",
        "Go back to where a selection began after working on it.",
        |s| is(s, &["`<", "`>", "'<", "'>"])
    ),
    item!(
        "mark-lists",
        4,
        "`:marks` and `:jumps`",
        "list your marks, or the places you jumped from (`:changes` for edits)",
        "Find the mark you set an hour ago.",
        |s| ex(
            s,
            &[
                ("marks", "marks"),
                ("ju", "jumps"),
                ("changes", "changes"),
                ("delm", "delmarks")
            ]
        )
    ),
    item!(
        "mark-hop",
        4,
        "`]'` and `['`",
        "to the next or previous line with a mark",
        "Walk through the places you've marked in a file.",
        |s| is(s, &["]'", "['", "]`", "[`"])
    ),
    item!(
        "special-registers",
        4,
        "`\".p`, `\":p` and `\"%p`",
        "paste what you last typed, your last command, or the file's name",
        "Paste the file name into a comment.",
        |s| cmd(s).is_some_and(|c| c.register.is_some_and(|r| ".:%/".contains(r)))
    ),
    item!(
        "register-list",
        4,
        "`:reg`",
        "show what every register holds",
        "Find which register has the text you copied earlier.",
        |s| ex(s, &[("reg", "registers"), ("di", "display")])
    ),
    item!(
        "put-cmd",
        4,
        "`:put`",
        "paste on a line of its own, whatever was copied",
        "Paste a word as a new line under this one: `:put`.",
        |s| ex(s, &[("pu", "put")])
    ),
    item!(
        "repeat-sub",
        4,
        "`&` and `g&`",
        "repeat the last `:s` on this line, or on every line",
        "Ran a substitute on one line and want it on others too.",
        |s| is(s, &["&", "g&"]) || ex(s, &[("&", "&"), ("&&", "&&")])
    ),
    item!(
        "line-middle",
        4,
        "`gM`",
        "to the middle of the line (`gm` the middle of the screen width)",
        "Jump into the middle of a long line, then refine.",
        |s| is(s, &["gm", "gM"])
    ),
    item!(
        "screen-line-ends",
        4,
        "`g0` and `g$`",
        "the start or end of the line as shown on screen, when lines wrap",
        "Get to the end of the visible part of a wrapped line.",
        |s| is(s, &["g0", "g^", "g$"])
    ),
    item!(
        "last-output",
        4,
        "`g<`",
        "show the last command's output again",
        "Read a message that scrolled away.",
        |s| is(s, &["g<"])
    ),
    item!(
        "side-scroll",
        4,
        "`zl`, `zh`, `zs` and `ze`",
        "scroll sideways when long lines don't wrap",
        "Read the end of a wide log line without moving the cursor along it.",
        |s| is(s, &["zh", "zl", "zH", "zL", "zs", "ze"])
    ),
    item!(
        "fold-move",
        4,
        "`zj` and `zk`",
        "to the next or previous fold (`[z` and `]z`: this fold's start and end)",
        "Step from one folded function to the next.",
        |s| is(s, &["zj", "zk", "[z", "]z"])
    ),
    item!(
        "fold-levels",
        4,
        "`zm` and `zr`",
        "fold one level more or less everywhere (`zv` shows the cursor's line)",
        "See a file's outline, then open it a level at a time.",
        |s| is(
            s,
            &[
                "zm", "zr", "zv", "zx", "zX", "zi", "zn", "zN", "zA", "zC", "zO"
            ]
        )
    ),
    item!(
        "fold-make",
        4,
        "`zfap` and `zd`",
        "make a fold over a motion, or remove one (with manual folding)",
        "Fold away a long block you don't need to see.",
        |s| head(s, "z")
            && cmd(s).is_some_and(|c| c
                .keys
                .get(1)
                .and_then(|k| k.plain_char())
                .is_some_and(|k| "fFdDE".contains(k)))
    ),
    item!(
        "window-size",
        4,
        "`Ctrl-w =` and `Ctrl-w _`",
        "make windows the same size, or this one as tall as it goes (`+`, `-`, `<`, `>` step)",
        "Even out splits after resizing the terminal.",
        |s| wincmd(s, "=_|+-<>") || ex(s, &[("res", "resize")]),
        VIM_AND_VSCODE,
        None,
        None
    ),
    item!(
        "window-move",
        4,
        "`Ctrl-w H` and `Ctrl-w T`",
        "move this window to the far left (or J, K, L), or out to a tab of its own",
        "Turn a horizontal split into a vertical one.",
        |s| wincmd(s, "HJKLTrRx"),
        VIM_ONLY,
        None,
        None
    ),
    item!(
        "window-under",
        4,
        "`Ctrl-w f` and `Ctrl-w ]`",
        "open the file, or the tag, under the cursor in a new window",
        "Peek at an imported file beside the one you're in.",
        |s| wincmd(s, "fF]^")
            || cmd(s).is_some_and(|c| c.keys.len() == 3
                && c.keys[0].is_ctrl('w')
                && c.keys[1].plain_char() == Some('g')
                && c.keys[2].plain_char().is_some_and(|k| "fF".contains(k))),
        VIM_ONLY,
        None,
        None
    ),
    item!(
        "last-tab",
        4,
        "`g<Tab>`",
        "back to the tab you were in last",
        "Flip between two tabs among many.",
        |s| cmd(s).is_some_and(|c| c.keys.last().is_some_and(|k| k.sym == Sym::Tab)
            && c.keys.iter().any(|k| k.plain_char() == Some('g'))),
        VIM_ONLY,
        None,
        None
    ),
    item!(
        "diff",
        4,
        "`]c`, `do` and `dp`",
        "to the next change between files being compared; take it from, or give it to, the other",
        "Merge a conflict side by side with `vimdiff`.",
        |s| is(s, &["]c", "[c", "do", "dp"])
            || ex(
                s,
                &[
                    ("diffthis", "diffthis"),
                    ("diffo", "diffoff"),
                    ("dif", "diffupdate"),
                    ("diffs", "diffsplit"),
                    ("diffg", "diffget"),
                    ("diffpu", "diffput"),
                    ("diffp", "diffpatch")
                ]
            ),
        VIM_ONLY,
        None,
        None
    ),
    item!(
        "method-jump",
        4,
        "`]m` and `[m`",
        "to the start of the next or previous method (`]M`, `[M`: their ends)",
        "Step through a class's methods.",
        |s| is(s, &["]m", "[m", "]M", "[M"]),
        VIM_AND_VSCODE,
        None,
        None
    ),
    item!(
        "comment-jump",
        4,
        "`[/` and `]/`",
        "to the start or end of a /* */ comment",
        "Get past a long block comment.",
        |s| is(s, &["[/", "]/", "[*", "]*"]),
        VIM_ONLY,
        None,
        None
    ),
    item!(
        "word-in-files",
        4,
        "`[I`",
        "list every line with the word under the cursor, here and in included files",
        "See every use of a constant without searching.",
        |s| is(s, &["[I", "]I", "[i", "]i"])
            || cmd(s).is_some_and(|c| c.keys.len() == 2
                && matches!(c.keys[0].plain_char(), Some('[' | ']'))
                && c.keys[1].is_ctrl('i')),
        VIM_ONLY,
        None,
        None
    ),
    item!(
        "tags",
        4,
        "`Ctrl-]` and `Ctrl-t`",
        "jump to a definition through the tags file, and back again",
        "Follow a function call into another file and return (run `ctags -R` first).",
        |s| ctrl(s, "]t")
            || ex(
                s,
                &[
                    ("ta", "tag"),
                    ("tn", "tnext"),
                    ("tp", "tprevious"),
                    ("ts", "tselect"),
                    ("po", "pop")
                ]
            ),
        VIM_ONLY,
        None,
        None
    ),
    item!(
        "keyword-help",
        4,
        "`K`",
        "look up the word under the cursor (its man page, or Vim's help)",
        "Check what a shell command's flag does from inside a script.",
        |s| is(s, &["K"]),
        VIM_ONLY,
        None,
        None
    ),
    item!(
        "visual-increment",
        4,
        "`g Ctrl-a` in Visual mode",
        "count up down the selected lines: 1, 2, 3…",
        "Number a list: select the zeros, `g Ctrl-a`.",
        |s| s.before.is_visual()
            && cmd(s).is_some_and(|c| {
                c.keys.len() == 2
                    && c.keys[0].plain_char() == Some('g')
                    && (c.keys[1].is_ctrl('a') || c.keys[1].is_ctrl('x'))
            }),
        VIM_AND_VSCODE,
        None,
        None
    ),
    item!(
        "insert-again",
        4,
        "`Ctrl-a` while typing",
        "type again what you typed last time in Insert mode",
        "Add the same closing text in several places.",
        |s| insert_ctrl(s, "a@")
    ),
    item!(
        "insert-copy",
        4,
        "`Ctrl-y` and `Ctrl-e` while typing",
        "copy the character from the line above, or below",
        "Repeat most of the line above, a character at a time.",
        |s| insert_ctrl(s, "ye")
    ),
    item!(
        "completion-kinds",
        4,
        "`Ctrl-x Ctrl-f` and `Ctrl-x Ctrl-l`",
        "complete a file name, or a whole line (`Ctrl-x Ctrl-o` for code)",
        "Type a path without looking it up.",
        |s| matches!(
            insert_key(s),
            Some(InsertKey::Then('x', _) | InsertKey::Ctrl('x'))
        )
    ),
    item!(
        "insert-literal",
        4,
        "`Ctrl-v` and `Ctrl-k` while typing",
        "type a key as it is (a real Tab), or a special character (`Ctrl-k e'` is é)",
        "Type a Tab in a file that turns tabs into spaces.",
        |s| insert_ctrl(s, "vkq")
    ),
    item!(
        "insert-undo-break",
        4,
        "`Ctrl-g u` while typing",
        "start a new undo step without leaving Insert mode",
        "Keep one undo from wiping out a whole paragraph you typed.",
        |s| matches!(insert_key(s), Some(InsertKey::Then('g', _)))
    ),
    item!(
        "insert-expr",
        4,
        "`Ctrl-r =` while typing",
        "work out a sum and type the answer",
        "Type `Ctrl-r =` 60*24 Enter to insert 1440.",
        |s| matches!(insert_key(s), Some(InsertKey::Then('r', '=')))
    ),
    item!(
        "cmdline-window",
        4,
        "`q:` and `Ctrl-f`",
        "edit earlier commands (or searches with `q/`) in a window, with all of Vim",
        "Change one part of a long command you ran an hour ago.",
        |s| is(s, &["q:", "q/", "q?"]) || cmdline_ctrl(s, "f")
    ),
    item!(
        "search-step",
        4,
        "`Ctrl-g` and `Ctrl-t` while searching",
        "to the next or previous match before pressing Enter",
        "Search for a word and step to its third use, then Enter.",
        |s| cmdline_ctrl(s, "gt")
    ),
    item!(
        "line-undo",
        4,
        "`U`",
        "undo every change on the last changed line at once",
        "Put a line back the way it was after several edits to it.",
        |s| is(s, &["U"]) && !s.before.is_visual()
    ),
    item!(
        "write-quit",
        4,
        "`ZZ`",
        "save and quit in two keys",
        "Finish a commit message.",
        |s| is(s, &["ZZ"]) || cmdline(s, &["x", "wq"])
    ),
];

pub fn find(id: &str) -> Option<&'static Item> {
    CURRICULUM.iter().find(|i| i.id == id)
}

/// What to know before an item: the next step up from it, never a
/// sideways one. Items not listed need nothing.
static REQUIRES: &[(&str, &[&str])] = &[
    ("word-end", &["word"]),
    ("bigword", &["word"]),
    ("till", &["find"]),
    ("scroll-line", &["half-page"]),
    ("cmdline-word", &["search"]),
    ("cmdline-edit", &["cmdline-history"]),
    ("cmdline-window", &["cmdline-history"]),
    ("search-step", &["search"]),
    ("insert-again", &["insert-keys"]),
    ("insert-copy", &["insert-keys"]),
    ("completion-kinds", &["completion"]),
    ("insert-literal", &["insert-keys"]),
    ("insert-undo-break", &["insert-keys"]),
    ("insert-expr", &["insert-keys"]),
    ("visual-swap", &["visual-line"]),
    ("visual-linewise", &["visual-line"]),
    ("visual-increment", &["increment", "visual-block"]),
    ("window-close", &["split"]),
    ("window-cycle", &["split"]),
    ("window-size", &["split"]),
    ("window-move", &["split"]),
    ("window-under", &["split"]),
    ("last-tab", &["tabs"]),
    ("fold-move", &["fold"]),
    ("fold-levels", &["fold"]),
    ("fold-make", &["fold"]),
    ("word-end-back", &["word-end"]),
    ("star-partial", &["star"]),
    ("repeat-sub", &["substitute-cmd"]),
    ("screen-line-ends", &["screen-lines"]),
    ("put-more", &["yank-put"]),
    ("visual-put", &["yank-put", "visual-line"]),
    ("black-hole", &["registers"]),
    ("special-registers", &["registers"]),
    ("register-list", &["registers"]),
    ("put-cmd", &["yank-put"]),
    ("clipboard", &["yank-put"]),
    ("format", &["dap"]),
    ("filter", &["range"]),
    ("indent-object", &["shift"]),
    ("column", &["count"]),
    ("line-undo", &["undo-redo"]),
    ("screen", &["half-page"]),
    ("screen-middle", &["screen"]),
    ("jump-forward", &["jump-back"]),
    ("star-back", &["star"]),
    ("search-back", &["search-next"]),
    ("center", &["half-page"]),
    ("ciw", &["change-word"]),
    ("ci-quote", &["ciw"]),
    ("ci-paren", &["ciw"]),
    ("tag-object", &["ci-paren"]),
    ("dap", &["dd"]),
    ("indent-object", &["dap"]),
    ("d-to-end", &["dd"]),
    ("substitute", &["change-word"]),
    ("gv", &["visual-line"]),
    ("visual-block", &["visual-line"]),
    ("visual-object", &["visual-line", "ciw"]),
    ("visual-indent", &["visual-line"]),
    ("last-change", &["marks"]),
    ("change-marks", &["marks"]),
    ("visual-marks", &["marks"]),
    ("mark-lists", &["marks"]),
    ("mark-hop", &["marks"]),
    ("gi", &["append"]),
    ("macro", &["dot"]),
    ("replay-macro", &["macro"]),
    ("cgn", &["dot", "star"]),
    ("gn-select", &["cgn"]),
    ("search-next", &["search"]),
    ("unmatched", &["percent"]),
    ("sentence", &["ciw"]),
    ("block-append", &["visual-block"]),
    ("undo-time", &["undo-redo"]),
    ("increment", &["count"]),
    ("substitute-all", &["substitute-cmd"]),
    ("global", &["substitute-all"]),
    ("normal-cmd", &["dot", "range"]),
    ("range", &["substitute-cmd"]),
    ("sort", &["range"]),
];

/// The items `id` builds on.
pub fn requires(id: &str) -> &'static [&'static str] {
    REQUIRES
        .iter()
        .find(|(i, _)| *i == id)
        .map_or(&[], |(_, r)| r)
}

/// The skills each rule's advice teaches, the main one first. Rules for
/// the shell and VS Code, and Vim basics everyone starts with (hjkl over
/// the arrows), teach nothing in the tree.
static RULE_SKILLS: &[(&str, &[&str])] = &[
    ("vim.repeat-jk", &["count", "paragraph", "half-page"]),
    ("vim.repeat-hl", &["word", "find"]),
    ("vim.repeat-x", &["word", "d-to-end"]),
    ("vim.arrows-normal", &[]),
    ("vim.arrows-insert", &[]),
    ("vim.backspace-streak", &["insert-delete"]),
    ("vim.repeat-dd", &["count", "dap"]),
    ("vim.w-then-q", &["write-quit"]),
    (
        "vim.delete-then-insert",
        &["change-word", "substitute", "d-to-end"],
    ),
    ("vim.dot-repeat", &["dot"]),
    ("vim.macro", &["macro"]),
    ("vim.repeat-indent", &["count", "indent-object"]),
    ("vim.repeat-join", &["count", "join"]),
    ("vim.repeat-paste", &["count"]),
    ("vim.undo-run", &["count", "undo-time"]),
    ("vim.visual-block", &["visual-block"]),
    ("vim.registers", &["registers"]),
    ("vim.case-run", &["case"]),
    ("vim.increment", &["increment"]),
    ("vim.insert-keys", &["insert-keys"]),
    ("vim.windows", &["windows"]),
    ("vim.overshoot", &["count"]),
    ("vim.find-repeat", &["find", "search"]),
    ("vim.operator-run", &["count"]),
    ("vim.scroll-run", &["half-page", "center"]),
    ("vim.search-again", &["search-next"]),
    ("vim.change-matches", &["cgn", "substitute-all"]),
    ("vim.shorthand", &["append", "open-line", "d-to-end"]),
    ("vim.visual-operator", &["ciw", "visual-object"]),
    ("motion.count", &["count"]),
    ("motion.paragraph", &["paragraph"]),
    ("motion.screen", &["screen"]),
    ("motion.file", &["file-ends"]),
    ("motion.word", &["word"]),
    ("motion.find", &["find"]),
    ("motion.line-ends", &["line-ends"]),
    ("motion.bracket", &["percent"]),
    ("motion.star", &["star"]),
    ("motion.jump-back", &["jump-back"]),
    ("motion.page", &["half-page"]),
    ("motion.search", &["search-next"]),
    ("motion.last-change", &["change-list"]),
    ("motion.line-number", &["goto-line"]),
    ("motion.section", &["section"]),
    ("motion.unmatched", &["unmatched"]),
    ("motion.sentence", &["sentence-motion"]),
    ("motion.screen-line", &["screen-lines"]),
    ("motion.search-to", &["search"]),
    ("motion.scroll", &["center"]),
    ("motion.mark", &["marks"]),
    ("motion.text-object", &["ciw"]),
    ("motion.operator", &["word-end", "till", "d-to-end"]),
    ("bash.up-history", &["history-search"]),
    ("bash.backspace-clear", &[]),
    ("bash.left-streak", &[]),
    ("shell.cd-up", &["cd-back"]),
    ("shell.reused-arg", &["last-arg"]),
    ("shell.one-word-changed", &["quick-sub"]),
    ("shell.keys", &["history-search"]),
    ("vscode.quick-open", &["quick-open"]),
    ("vscode.terminal-toggle", &["terminal-toggle"]),
    ("vscode.tab-switch", &["tabs"]),
    ("vim.reselect", &["gv"]),
    ("vscode.split", &["split"]),
];

/// The skills `rule_id` teaches, or `None` for a rule not in the tree.
pub fn rule_skills(rule_id: &str) -> Option<&'static [&'static str]> {
    RULE_SKILLS
        .iter()
        .find(|(r, _)| *r == rule_id)
        .map(|(_, s)| *s)
}

/// The item a text object belongs to: `i(` → `ci-paren`.
pub fn object_skill(object: &str) -> &'static str {
    match object.chars().last() {
        Some('w' | 'W') => "ciw",
        Some('"' | '\'' | '`') => "ci-quote",
        Some('(' | ')' | 'b' | '{' | '}' | 'B' | '[' | ']' | '<' | '>') => "ci-paren",
        Some('p') => "dap",
        Some('t') => "tag-object",
        _ => "ciw",
    }
}

/// The keys a tip's command stands for: `Ctrl-d` → Ctrl+d, `f<char>` →
/// `f` and a letter, `<line>G` → `42G`, `:sort` → `:sort` and Enter.
fn command_keys(press: &str) -> Vec<crate::key::Key> {
    use crate::key::Key;
    let press = press
        .replace("<char>", "x")
        .replace("<line>", "42")
        .replace("old", "x")
        .replace("new", "y");
    if let Some(rest) = press.strip_prefix(':') {
        let mut keys = vec![Key::ch(':')];
        keys.extend(rest.chars().map(Key::ch));
        keys.push(Key::ENTER);
        return keys;
    }
    let mut keys = Vec::new();
    let mut rest = press.as_str();
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("Ctrl-")
            && let Some(c) = r.chars().next()
        {
            keys.push(Key::ctrl(c));
            rest = &r[c.len_utf8()..];
        } else if let Some(r) = rest.strip_prefix("Esc") {
            keys.push(Key::ESC);
            rest = r;
        } else {
            let c = rest.chars().next().unwrap_or(' ');
            keys.push(Key::ch(c));
            rest = &rest[c.len_utf8()..];
        }
    }
    keys
}

/// The curriculum items a tip's command is (`ci(` → `ci-paren`), by
/// typing it into the key tracker from Normal mode.
pub fn items_of_command(cmd: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    for press in cmd
        .split_whitespace()
        .filter(|w| !matches!(*w, "then" | "or" | "and" | "…"))
        .map(|w| w.split('×').next().unwrap_or(w).trim_end_matches(','))
    {
        for id in items_of_keys(&command_keys(press)) {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

/// The curriculum items these keys are, typed from Normal mode.
pub fn items_of_keys(keys: &[crate::key::Key]) -> Vec<&'static str> {
    let mut out = Vec::new();
    let mut t = crate::vim::Tracker::new(crate::vim::Flavor::Vim);
    t.reset_known(VimMode::Normal);
    for (i, key) in keys.iter().enumerate() {
        let step = t.feed(crate::key::KeyStroke {
            key: *key,
            repeat: false,
            at_ms: i as u64,
        });
        for item in CURRICULUM {
            if item.used_in(&step) && !out.contains(&item.id) {
                out.push(item.id);
            }
        }
    }
    out
}

/// The items you already use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Known {
    /// Nothing is known about you (no memory): everything is ready.
    everything: bool,
    ids: HashSet<&'static str>,
    /// Links your history says don't hold: (item, what it builds on). See
    /// [`crate::links`].
    unlinked: HashSet<(&'static str, &'static str)>,
}

impl Default for Known {
    fn default() -> Self {
        Known::everything()
    }
}

impl Known {
    /// No knowledge either way: no tip is held back.
    pub fn everything() -> Self {
        Known {
            everything: true,
            ids: HashSet::new(),
            unlinked: HashSet::new(),
        }
    }

    pub fn of(ids: impl IntoIterator<Item = &'static str>) -> Self {
        Known {
            everything: false,
            ids: ids.into_iter().collect(),
            unlinked: HashSet::new(),
        }
        .closed()
    }

    /// Using something means knowing what it builds on, all the way down:
    /// someone who uses `ci(` knows `ciw` and `cw` too.
    fn closed(mut self) -> Self {
        let mut todo: Vec<&'static str> = self.ids.iter().copied().collect();
        while let Some(id) = todo.pop() {
            for r in requires(id) {
                if self.ids.insert(r) {
                    todo.push(r);
                }
            }
        }
        self
    }

    /// Items used often enough, or learned, plus the skills of rules
    /// you've mastered that you've actually used: mastering `j`-streaks
    /// with counts says nothing about `Ctrl-d`.
    pub fn from_progress<'a>(
        progress: &dyn Fn(&str) -> Progress,
        mastered_rules: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        let mut ids: HashSet<&'static str> = CURRICULUM
            .iter()
            .filter(|i| progress(i.id).known())
            .map(|i| i.id)
            .collect();
        for rule in mastered_rules {
            ids.extend(
                rule_skills(rule)
                    .unwrap_or_default()
                    .iter()
                    .filter(|s| progress(s).uses > 0),
            );
        }
        Known {
            everything: false,
            ids,
            unlinked: HashSet::new(),
        }
        .closed()
    }

    /// Also everything up to curriculum level `up_to`, as `[learn] level`
    /// says you know it.
    pub fn with_levels(mut self, up_to: u8) -> Self {
        self.ids
            .extend(CURRICULUM.iter().filter(|i| i.level <= up_to).map(|i| i.id));
        self.closed()
    }

    /// With what you said: known items added (and what they build on),
    /// then not-known ones taken out.
    pub fn with_overrides(mut self, overrides: &std::collections::HashMap<String, bool>) -> Self {
        if self.everything {
            return self;
        }
        for item in CURRICULUM {
            if overrides.get(item.id) == Some(&true) {
                self.ids.insert(item.id);
            }
        }
        let mut k = self.closed();
        for item in CURRICULUM {
            if overrides.get(item.id) == Some(&false) {
                k.ids.remove(item.id);
            }
        }
        k
    }

    pub fn knows(&self, id: &str) -> bool {
        self.everything || self.ids.contains(id)
    }

    /// Worth suggesting: known already, or one step up from what is.
    pub fn ready(&self, id: &str) -> bool {
        self.knows(id) || self.builds_on(id).all(|r| self.knows(r))
    }

    /// What `id` builds on, less the links your history says don't hold.
    pub fn builds_on<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'static str> + 'a {
        requires(id)
            .iter()
            .copied()
            .filter(move |r| !self.unlinked.contains(&(id, *r)))
    }

    /// Drop the links your history says don't hold.
    pub fn with_links(mut self, dropped: &[(&'static str, &'static str)]) -> Self {
        self.unlinked.extend(dropped.iter().copied());
        self
    }

    /// Whether a tip's command is something you're ready for: every item
    /// it is (commands the curriculum doesn't know are fine).
    pub fn ready_for_command(&self, cmd: &str) -> bool {
        self.everything || items_of_command(cmd).iter().all(|i| self.ready(i))
    }

    /// Whether a rule's main advice is something you're ready for.
    pub fn ready_for_rule(&self, rule_id: &str) -> bool {
        rule_skills(rule_id)
            .and_then(|s| s.first())
            .is_none_or(|s| self.ready(s))
    }
}

/// Used this many times lately, an item counts as known.
pub const KNOWN_USES: u64 = 3;
/// "Lately".
pub const RECENT_DAYS: u64 = 30;
/// Or used this many times overall, and not long ago.
pub const REGULAR_USES: u64 = 9;
/// Unused this long, it's no longer known: it may have been forgotten.
pub const FADE_DAYS: u64 = 90;

impl Progress {
    /// Known: by the mastery model when there's evidence (likely learned
    /// and likely still recalled: see [`crate::mastery`]). Without it: used
    /// a few times lately, or regularly and not too long ago, or learned
    /// from a daily tip and not left unused for long.
    pub fn known(&self) -> bool {
        if let (Some(m), Some(r)) = (self.mastery, self.recall) {
            return f64::from(m) / 100.0 >= crate::mastery::MASTERED
                && f64::from(r) / 100.0 >= crate::mastery::RECALLED;
        }
        let fresh = self.days_idle.is_none_or(|d| d <= FADE_DAYS);
        self.recent_uses >= KNOWN_USES
            || (fresh && self.uses >= REGULAR_USES)
            || (fresh && self.learned)
    }
}
/// An item is shown at most this many times.
pub const MAX_SHOWINGS: u32 = 3;
/// And not again within this many days.
pub const AGAIN_AFTER_DAYS: u64 = 7;

/// What's known about one item.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// Times used, ever.
    pub uses: u64,
    /// Times used in the last [`RECENT_DAYS`].
    pub recent_uses: u64,
    /// Days since it was last used; `None` if not known.
    pub days_idle: Option<u64>,
    pub times_shown: u32,
    /// Unix seconds.
    pub last_shown: Option<u64>,
    pub learned: bool,
    /// Percent chance it's learned, and that you'd recall it today, from
    /// [`crate::mastery`]; `None` without evidence.
    pub mastery: Option<u8>,
    pub recall: Option<u8>,
}

/// The day's "something new": the first item you're ready for but don't
/// use yet, haven't learned, and weren't shown lately.
pub fn pick(
    progress: &dyn Fn(&str) -> Progress,
    known: &Known,
    now_s: u64,
) -> Option<&'static Item> {
    pick_for(
        progress,
        known,
        now_s,
        crate::context::ContextKind::TerminalVim,
    )
}

/// [`pick`], skipping what doesn't work in `ctx` (no `:g` in vscodevim).
pub fn pick_for(
    progress: &dyn Fn(&str) -> Progress,
    known: &Known,
    now_s: u64,
    ctx: crate::context::ContextKind,
) -> Option<&'static Item> {
    CURRICULUM
        .iter()
        .find(|item| worth_teaching(item, progress, known, now_s, ctx))
}

/// The day's lesson from your habits: see [`choose`]; else nothing.
pub fn pick_from_habits(
    progress: &dyn Fn(&str) -> Progress,
    known: &Known,
    now_s: u64,
    ctx: crate::context::ContextKind,
    habits: &[(String, u64)],
) -> Option<&'static Item> {
    choose(progress, known, now_s, ctx, habits).map(|p| p.item)
}

/// The day's pick: something new, or a review of something slipping.
#[derive(Clone, Copy)]
pub struct Pick {
    pub item: &'static Item,
    pub review: bool,
}

/// Each level past where you are, a lesson is this much less likely to
/// stick: one step up lands near the 85% that learning is fastest at
/// (Wilson et al. 2019, docs/learning.md).
pub const LEVEL_STEP: f64 = 0.85;
/// A skill you've mastered comes back for review once you'd recall it
/// less likely than this: before it's forgotten (below
/// [`crate::mastery::RECALLED`]), when reviewing does the most.
pub const REVIEW_AT: f64 = 0.7;

/// The pick worth most: what it saves you, times the chance it sticks.
///
/// What it saves: one, plus this week's hits of the rules (`habits`) whose
/// advice it is, shared among the skills each teaches. The chance it
/// sticks: for something new, how well you know what it builds on, less
/// [`LEVEL_STEP`] for each level past the lowest with something left; for a
/// review, how much of it you've lost since you mastered it. New lessons
/// and reviews compete in one ranking, so they come mixed. Ties go to the
/// curriculum's order.
pub fn choose(
    progress: &dyn Fn(&str) -> Progress,
    known: &Known,
    now_s: u64,
    ctx: crate::context::ContextKind,
    habits: &[(String, u64)],
) -> Option<Pick> {
    let saves = |id: &str| {
        1.0 + habits
            .iter()
            .filter_map(|(rule, n)| {
                rule_skills(rule)
                    .filter(|s| s.contains(&id))
                    .map(|s| *n as f64 / s.len() as f64)
            })
            .sum::<f64>()
    };
    let pct = |p: Option<u8>| p.map(|p| f64::from(p) / 100.0);
    let new: Vec<&Item> = CURRICULUM
        .iter()
        .filter(|item| worth_teaching(item, progress, known, now_s, ctx))
        .collect();
    let frontier = new.iter().map(|i| i.level).min().unwrap_or(1);
    let lessons = new.into_iter().map(|item| {
        let base: f64 = known
            .builds_on(item.id)
            // Mastered counts in full.
            .map(|r| {
                pct(progress(r).mastery).map_or(1.0, |m| (m / crate::mastery::MASTERED).min(1.0))
            })
            .product();
        let gap = i32::from(item.level.saturating_sub(frontier));
        let chance = base * LEVEL_STEP.powi(gap);
        (
            saves(item.id) * chance,
            Pick {
                item,
                review: false,
            },
        )
    });
    let reviews = CURRICULUM
        .iter()
        .filter(|item| item.applies_in(ctx) && crate::support::supports_all(ctx, &tip_text(item)))
        .filter_map(|item| {
            let p = progress(item.id);
            let (mastery, recall) = (pct(p.mastery)?, pct(p.recall)?);
            let due = mastery >= crate::mastery::MASTERED
                && recall < REVIEW_AT
                && p.last_shown
                    .is_none_or(|t| now_s.saturating_sub(t) >= AGAIN_AFTER_DAYS * 86_400);
            due.then(|| {
                (
                    saves(item.id) * mastery * (1.0 - recall),
                    Pick { item, review: true },
                )
            })
        });
    let mut best: Option<(f64, Pick)> = None;
    for (score, pick) in lessons.chain(reviews) {
        if best.as_ref().is_none_or(|(b, _)| score > *b) {
            best = Some((score, pick));
        }
    }
    best.map(|(_, p)| p)
}

fn worth_teaching(
    item: &Item,
    progress: &dyn Fn(&str) -> Progress,
    known: &Known,
    now_s: u64,
    ctx: crate::context::ContextKind,
) -> bool {
    {
        let p = progress(item.id);
        known.ready(item.id)
            && item.applies_in(ctx)
            && crate::support::supports_all(ctx, &tip_text(item))
            // Not known now: learned and since forgotten comes back, as a
            // review if it was mastered.
            && !known.knows(item.id)
            && p.mastery.is_none_or(|m| f64::from(m) / 100.0 < crate::mastery::MASTERED)
            && p.times_shown < MAX_SHOWINGS
            && p.last_shown
                .is_none_or(|t| now_s.saturating_sub(t) >= AGAIN_AFTER_DAYS * 86_400)
    }
}

/// The tip text for an item, with its commands marked.
pub fn tip_text(item: &Item) -> String {
    format!("{}: {}. {}", item.keys, item.what, item.example)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vim::notation::strokes;
    use crate::vim::{Flavor, Tracker};

    /// The ids recognised in `keys`, typed from Normal mode.
    fn used(keys: &str) -> Vec<&'static str> {
        let mut t = Tracker::new(Flavor::Vim);
        t.reset_known(VimMode::Normal);
        let mut ids = Vec::new();
        for k in strokes(keys, 0) {
            let step = t.feed(k);
            for item in CURRICULUM {
                if item.used_in(&step) && !ids.contains(&item.id) {
                    ids.push(item.id);
                }
            }
        }
        ids
    }

    #[test]
    fn ids_are_unique_and_levels_ordered() {
        let mut ids: Vec<_> = CURRICULUM.iter().map(|i| i.id).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
        assert!(n >= 50, "{n}");
        assert!(CURRICULUM.windows(2).all(|w| w[0].level <= w[1].level));
    }

    #[test]
    fn recognisers() {
        let cases: &[(&str, &str)] = &[
            ("w", "word"),
            ("5j", "count"),
            ("ci\"x<Esc>", "ci-quote"),
            ("ciwx<Esc>", "ciw"),
            ("di(", "ci-paren"),
            ("}", "paragraph"),
            ("<C-d>", "half-page"),
            ("*", "star"),
            ("zz", "center"),
            ("dap", "dap"),
            ("ma", "marks"),
            ("`.", "last-change"),
            ("qqq", "macro"),
            ("@@", "replay-macro"),
            ("cgnx<Esc>", "cgn"),
            ("\"0p", "registers"),
            ("<C-w>l", "windows"),
            ("ia<C-r>", "insert-keys"),
            ("g;", "change-list"),
            ("zc", "fold"),
            (":cn<CR>", "quickfix"),
            ("das", "sentence"),
            ("<C-v>jj$A;<Esc>", "block-append"),
            ("gn", "gn-select"),
            ("42G", "goto-line"),
            (":120<CR>", "goto-line"),
            (":earlier 1m<CR>", "undo-time"),
            ("gUiw", "case"),
            (">ip", "indent-object"),
            ("dt,", "till"),
            (":%s/a/b/g<CR>", "substitute-all"),
            (":g/TODO/d<CR>", "global"),
            ("Vj>", "visual-indent"),
            ("ZZ", "write-quit"),
            ("dw", "operator"),
            ("y}", "operator"),
            ("yy", "yank-put"),
            ("p", "yank-put"),
            ("rx", "replace-char"),
            ("cc", "substitute"),
            ("tx", "till"),
            ("<C-f>", "page"),
            ("X", "delete-back"),
            ("<C-e>", "scroll-line"),
            ("+", "line-first"),
            ("<CR>", "line-first"),
            ("20|", "column"),
            ("U", "line-undo"),
            (">>", "shift"),
            ("<lt><lt>", "shift"),
            ("==", "reindent"),
            ("=ap", "reindent"),
            ("gqip", "format"),
            ("gJ", "join-raw"),
            ("!!sort<CR>", "filter"),
            (":retab<CR>", "retab"),
            (":ce<CR>", "align"),
            (":'<lt>,'>right<CR>", "align"),
            ("``", "jump-last"),
            ("`]", "change-marks"),
            ("'<lt>", "visual-marks"),
            (":marks<CR>", "mark-lists"),
            (":ju<CR>", "mark-lists"),
            ("]'", "mark-hop"),
            ("\"+yy", "clipboard"),
            ("gp", "put-more"),
            ("]p", "put-more"),
            ("viwP", "visual-put"),
            ("\"_dd", "black-hole"),
            ("\"%p", "special-registers"),
            (":reg<CR>", "register-list"),
            (":pu<CR>", "put-cmd"),
            ("gd", "goto-def"),
            ("g_", "last-nonblank"),
            ("ge", "word-end-back"),
            ("gIx<Esc>", "insert-col1"),
            ("gf", "goto-file"),
            ("g*", "star-partial"),
            ("&", "repeat-sub"),
            ("g&", "repeat-sub"),
            ("<C-g>", "file-info"),
            ("g<C-g>", "file-info"),
            ("gM", "line-middle"),
            ("g$", "screen-line-ends"),
            ("g<lt>", "last-output"),
            ("z.", "center"),
            ("z<CR>", "center"),
            ("]s", "spell"),
            ("z=", "spell"),
            ("zl", "side-scroll"),
            ("zj", "fold-move"),
            ("zm", "fold-levels"),
            ("zfap", "fold-make"),
            ("zd", "fold-make"),
            ("<C-w>s", "split"),
            (":vs<CR>", "split"),
            ("<C-w>o", "window-close"),
            (":only<CR>", "window-close"),
            ("<C-w>w", "window-cycle"),
            ("<C-w><C-w>", "window-cycle"),
            (":tabnew<CR>", "tab-cmds"),
            ("<C-w>=", "window-size"),
            ("<C-w>H", "window-move"),
            ("<C-w>f", "window-under"),
            ("g<Tab>", "last-tab"),
            ("]c", "diff"),
            ("do", "diff"),
            (":diffthis<CR>", "diff"),
            ("]m", "method-jump"),
            ("[/", "comment-jump"),
            ("[I", "word-in-files"),
            ("<C-]>", "tags"),
            ("<C-t>", "tags"),
            ("K", "keyword-help"),
            ("vjU", "case"),
            ("vjrx", "replace-char"),
            ("v<C-v>", "visual-block"),
            ("vjo", "visual-swap"),
            ("vjD", "visual-linewise"),
            ("<C-v>jjg<C-a>", "visual-increment"),
            ("ia<C-n>", "completion"),
            ("ia<C-w>", "insert-delete"),
            ("ia<C-t>", "insert-indent"),
            ("i<C-a>", "insert-again"),
            ("i<C-y>", "insert-copy"),
            ("i<C-x><C-f>", "completion-kinds"),
            ("i<C-v><Tab>", "insert-literal"),
            ("i<C-g>u", "insert-undo-break"),
            ("i<C-r>=", "insert-expr"),
            (":e<Tab><CR>", "cmdline-complete"),
            (":s<Up><CR>", "cmdline-history"),
            (":%s/<C-r><C-w><CR>", "cmdline-word"),
            (":abc<C-b><CR>", "cmdline-edit"),
            ("q:", "cmdline-window"),
            ("/ab<C-g><CR>", "search-step"),
        ];
        for (keys, id) in cases {
            assert!(
                used(keys).contains(id),
                "{keys:?} should count as {id}: {:?}",
                used(keys)
            );
        }
        // An argument isn't a motion: `fw` is find, not word.
        assert!(!used("fw").contains(&"word"));
        // Near misses.
        assert!(!used(":1,5d<CR>").contains(&"goto-line"));
        assert!(!used("5G").contains(&"file-ends") && used("5G").contains(&"goto-line"));
        assert!(!used("\"_dd").contains(&"registers") && used("\"ayy").contains(&"registers"));
        assert!(!used("''").contains(&"marks") && !used("'.").contains(&"marks"));
        assert!(!used("vu").contains(&"undo-redo") && used("u").contains(&"undo-redo"));
    }

    #[test]
    fn ex_names_by_vims_abbreviations() {
        assert_eq!(ex_word("sort"), Some("sort"));
        assert_eq!(ex_word("'<,'>sort u"), Some("sort"));
        assert_eq!(ex_word(".,+3>"), Some(">"));
        assert_eq!(ex_word("5,10m 0"), Some("m"));
        assert_eq!(ex_word("/foo/,/bar/d"), Some("d"));
        assert_eq!(ex_word("%!jq ."), Some("!"));
        assert_eq!(ex_word("42"), None);
        assert!(
            !used(":let x = 1<CR>").contains(&"align"),
            ":le is :left, :let isn't"
        );
        assert!(!used(":c<CR>").contains(&"align"), "too short for :center");
    }

    #[test]
    fn picks_the_first_thing_you_dont_use() {
        use std::collections::HashMap;
        let mut known: HashMap<&str, Progress> = HashMap::new();
        let first = |known: &HashMap<&str, Progress>, now| {
            let progress = |id: &str| known.get(id).copied().unwrap_or_default();
            pick(&progress, &Known::from_progress(&progress, []), now).map(|i| i.id)
        };
        assert_eq!(first(&known, 0), Some("word"));
        known.insert(
            "word",
            Progress {
                uses: 50,
                ..Default::default()
            },
        );
        assert_eq!(first(&known, 0), Some("word-end"));
        // Shown yesterday: not again for a week.
        known.insert(
            "word-end",
            Progress {
                times_shown: 1,
                last_shown: Some(0),
                ..Default::default()
            },
        );
        assert_eq!(first(&known, 86_400), Some("line-ends"));
        assert_eq!(first(&known, 8 * 86_400), Some("word-end"));
        // Learned, or shown three times: done with it.
        known.insert(
            "word-end",
            Progress {
                learned: true,
                ..Default::default()
            },
        );
        assert_eq!(first(&known, 8 * 86_400), Some("line-ends"));
        assert_eq!(
            tip_text(find("ci-quote").unwrap()),
            "`ci\"`: change everything inside the quotes, from anywhere in them. Replace a string's text in one go."
        );
    }

    #[test]
    fn the_tree_is_whole() {
        for (id, reqs) in REQUIRES {
            let item = find(id).unwrap_or_else(|| panic!("{id} isn't an item"));
            for r in *reqs {
                let req = find(r).unwrap_or_else(|| panic!("{id} requires unknown {r}"));
                assert!(req.level <= item.level, "{id} requires the harder {r}");
            }
        }
        for rule in crate::rules::all() {
            let skills = rule_skills(rule.id())
                .unwrap_or_else(|| panic!("{} isn't in the skill tree", rule.id()));
            for s in skills {
                assert!(find(s).is_some(), "{} teaches unknown {s}", rule.id());
            }
        }
    }

    #[test]
    fn ready_one_step_up() {
        let none = Known::of([]);
        assert!(none.ready("find") && !none.ready("till"));
        assert!(!none.ready("ci-paren"), "not ci( before ciw");
        let k = Known::of(["find", "change-word"]);
        assert!(k.ready("till") && k.ready("ciw") && !k.ready("ci-paren"));
        // Using it already makes it ready, whatever it builds on.
        assert!(Known::of(["ci-paren"]).ready("ci-paren"));
        assert!(Known::everything().ready("global"));
        // A mastered rule's skills count as known, the ones you've used.
        let used_find = |id: &str| Progress {
            uses: u64::from(id == "find"),
            ..Default::default()
        };
        let k = Known::from_progress(&used_find, ["motion.find"]);
        assert!(k.knows("find") && k.ready("till"));
        let used_count = |id: &str| Progress {
            uses: u64::from(id == "count"),
            ..Default::default()
        };
        let k = Known::from_progress(&used_count, ["vim.repeat-jk"]);
        assert!(k.knows("count") && !k.knows("half-page") && !k.knows("paragraph"));
        assert!(!none.ready_for_rule("motion.text-object"));
        assert!(none.ready_for_rule("motion.count") && none.ready_for_rule("shell.cd-up"));
    }

    #[test]
    fn commands_are_items() {
        assert_eq!(items_of_command("ci("), ["ci-paren"]);
        assert_eq!(items_of_command("Ctrl-d"), ["half-page"]);
        assert_eq!(items_of_command("3f<char>"), ["find"]);
        assert_eq!(items_of_command("<line>G"), ["goto-line"]);
        assert_eq!(items_of_command(":sort"), ["sort"]);
        assert_eq!(items_of_command("Ctrl-d×3 then 4j"), ["half-page", "count"]);
        let k = Known::of(["change-word"]);
        assert!(k.ready_for_command("ciw") && !k.ready_for_command("ci("));
        assert!(!k.ready_for_command("zz"), "zz builds on Ctrl-d");
        assert!(k.ready_for_command("ga"), "not a tree item");
    }

    #[test]
    fn what_you_say_you_know() {
        let said = std::collections::HashMap::from([
            ("ci-paren".to_string(), true),
            ("word".to_string(), false),
        ]);
        let k = Known::of(["word"]).with_overrides(&said);
        assert!(
            k.knows("ci-paren") && k.knows("ciw"),
            "and what it builds on"
        );
        assert!(!k.knows("word"));
    }

    #[test]
    fn each_place_has_its_own_lessons() {
        let nothing = |_: &str| Progress::default();
        let known = Known::of([]);
        let first = |ctx| pick_for(&nothing, &known, 0, ctx).map(|i| i.id);
        assert_eq!(first(ContextKind::Shell), Some("history-search"));
        assert_eq!(first(ContextKind::TerminalVim), Some("word"));
        // Shell and VS Code items stay in their place.
        let shell_only: Vec<_> = CURRICULUM
            .iter()
            .filter(|i| i.applies_in(ContextKind::Shell))
            .map(|i| i.id)
            .collect();
        assert_eq!(
            shell_only,
            ["history-search", "cd-back", "last-arg", "quick-sub"]
        );
        assert!(
            !find("quick-open")
                .unwrap()
                .applies_in(ContextKind::TerminalVim)
        );
        assert_eq!(
            used_by_shape(CommandShape::CdBack)
                .map(|i| i.id)
                .collect::<Vec<_>>(),
            ["cd-back"]
        );
        assert_eq!(
            used_by_ui(UiWhat::FileOpened)
                .map(|i| i.id)
                .collect::<Vec<_>>(),
            ["quick-open"]
        );
    }

    #[test]
    fn the_lesson_comes_from_your_habits() {
        let nothing = |_: &str| Progress::default();
        let known = Known::of(["word", "count"]);
        // You walked with j a lot this week: } before the curriculum's next.
        let habits = vec![
            ("vim.repeat-jk".to_string(), 30),
            ("vim.repeat-x".to_string(), 2),
        ];
        let pick = pick_from_habits(&nothing, &known, 0, ContextKind::TerminalVim, &habits);
        assert_eq!(pick.map(|i| i.id), Some("paragraph"));
        // Nothing this week: the curriculum's order.
        let pick = pick_from_habits(&nothing, &known, 0, ContextKind::TerminalVim, &[]);
        assert_eq!(
            pick.map(|i| i.id),
            pick_for(&nothing, &known, 0, ContextKind::TerminalVim).map(|i| i.id)
        );
    }

    #[test]
    fn worth_most_wins_and_reviews_come_mixed_in() {
        use crate::context::ContextKind::TerminalVim;
        let known = Known::of(["word", "count", "find"]);
        let with = |id: &'static str, mastery, recall| {
            move |i: &str| Progress {
                mastery: (i == id).then_some(mastery),
                recall: (i == id).then_some(recall),
                ..Default::default()
            }
        };
        // Half-learned prerequisites make a lesson less likely to stick.
        let shaky = |i: &str| Progress {
            mastery: (i == "word").then_some(40),
            recall: (i == "word").then_some(90),
            ..Default::default()
        };
        let habits = vec![("vim.repeat-hl".to_string(), 2)];
        let first = choose(&|_| Progress::default(), &known, 0, TerminalVim, &habits).unwrap();
        let after = choose(&shaky, &known, 0, TerminalVim, &habits).unwrap();
        assert_eq!(first.item.id, "word-end");
        assert_ne!(after.item.id, "word-end", "builds on a shaky w");
        // Mastered and slipping: a review, ahead of a new lesson.
        let slipping = with("find", 98, 40);
        let pick = choose(&slipping, &known, 0, TerminalVim, &habits).unwrap();
        assert_eq!((pick.item.id, pick.review), ("find", true));
        // Still well recalled: no review yet.
        let fresh = with("find", 98, 90);
        let pick = choose(&fresh, &known, 0, TerminalVim, &habits).unwrap();
        assert!(!pick.review, "{}", pick.item.id);
        // Reviewed lately: not again within the week.
        let shown = |i: &str| Progress {
            last_shown: (i == "find").then_some(86_400),
            ..slipping(i)
        };
        let pick = choose(&shown, &known, 2 * 86_400, TerminalVim, &habits).unwrap();
        assert!(!pick.review, "{}", pick.item.id);
    }

    #[test]
    fn known_means_lately_and_often() {
        let p = |uses, recent_uses, idle| Progress {
            uses,
            recent_uses,
            days_idle: Some(idle),
            ..Default::default()
        };
        assert!(p(3, 3, 1).known(), "three times this month");
        assert!(!p(3, 1, 20).known(), "three times ever, mostly long ago");
        assert!(p(12, 0, 40).known(), "regular use, not long ago");
        assert!(!p(50, 0, 200).known(), "not used for half a year: faded");
        let learned = |idle| Progress {
            learned: true,
            days_idle: Some(idle),
            ..Default::default()
        };
        assert!(learned(10).known() && !learned(120).known());
        let modelled = |mastery, recall| Progress {
            uses: 50,
            recent_uses: 50,
            mastery: Some(mastery),
            recall: Some(recall),
            ..Default::default()
        };
        assert!(modelled(97, 80).known());
        assert!(!modelled(97, 30).known(), "likely forgotten");
        assert!(!modelled(60, 90).known(), "not learned yet");
        // A macro replays its keys without typing them: only `@q` counts.
        let mut t = Tracker::new(Flavor::Vim);
        t.reset_known(VimMode::Normal);
        let ids: Vec<_> = strokes("@q", 0)
            .into_iter()
            .map(|k| t.feed(k))
            .flat_map(|s| {
                CURRICULUM
                    .iter()
                    .filter(move |i| i.used_in(&s))
                    .map(|i| i.id)
            })
            .collect();
        assert_eq!(ids, ["macro"]);
    }

    #[test]
    fn using_something_means_knowing_what_it_builds_on() {
        // ci( without ever typing ciw: ciw and cw are known, and what
        // builds on ci( is ready.
        let k = Known::of(["ci-paren"]);
        assert!(k.knows("ciw") && k.knows("change-word"));
        assert!(k.ready("tag-object") && k.ready("ci-quote"));
        // All the way down: :sort means :s and ranges too.
        let k = Known::of(["sort"]);
        assert!(k.knows("range") && k.knows("substitute-cmd"));
    }

    #[test]
    fn your_level_counts_as_known() {
        let k = Known::of([]).with_levels(1);
        assert!(k.knows("change-word") && k.ready("ciw") && !k.knows("ciw"));
        let k = Known::of([]).with_levels(2);
        assert!(k.knows("ciw") && k.ready("ci-paren"));
        assert!(!Known::of([]).with_levels(0).knows("word"));
    }

    #[test]
    fn something_new_waits_for_what_it_builds_on() {
        // Everything is used but the text objects.
        let objects = [
            "ciw",
            "ci-quote",
            "ci-paren",
            "tag-object",
            "visual-object",
            "sentence",
        ];
        let progress = |id: &str| Progress {
            // ciw once (not enough to count), the other objects never.
            uses: match id {
                "ciw" => 1,
                _ if objects.contains(&id) => 0,
                _ => 99,
            },
            // ciw was shown three times already and won't be again.
            times_shown: if id == "ciw" { 3 } else { 0 },
            ..Default::default()
        };
        let known = Known::from_progress(&progress, []);
        // ci" and ci( build on ciw, so nothing is left for now.
        assert!(pick(&progress, &known, 0).is_none());
        let known = Known::from_progress(&progress, ["motion.text-object"]);
        assert_eq!(pick(&progress, &known, 0).map(|i| i.id), Some("ci-quote"));
    }

    #[test]
    fn every_item_is_explained() {
        for item in CURRICULUM {
            for span in crate::glossary::code_spans(item.keys) {
                assert!(
                    !crate::glossary::explain_keys(span, item.while_typing()).is_empty(),
                    "{}: `{span}` has no explanation",
                    item.id
                );
            }
        }
    }
}
