//! A curriculum of Vim features, ordered from basic to advanced, for the
//! daily "something new" tip. Each item knows how to recognise itself in
//! the key tracker's steps, so Flow Buddy can tell what you already use.
//!
//! The items are also the skill tree: each can require others first
//! ([`requires`]), every rule teaches some of them ([`rule_skills`]), and
//! tips only suggest what you're [`Known::ready`] for.

use std::collections::HashSet;

use crate::context::VimMode;
use crate::key::Sym;
use crate::rules::vim::command;
use crate::vim::{Command, Step, VimEvent};

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
}

impl Item {
    pub fn used_in(&self, step: &Step) -> bool {
        (self.recognise)(step)
    }
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
        "`0`, `^` and `$`",
        "the start, first character, or end of the line",
        "Get to the end of a long line in one key.",
        |s| is(s, &["0", "^", "$"])
    ),
    item!(
        "file-ends",
        1,
        "`gg` and `G`",
        "the first or last line of the file",
        "Jump to the top to check the imports.",
        |s| is(s, &["gg", "G"])
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
        |s| is(s, &["u"]) || ctrl(s, "r")
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
    // Level 2: bigger moves and text objects.
    item!(
        "paragraph",
        2,
        "`}` and `{`",
        "jump to the next or previous blank line",
        "Move between functions or paragraphs.",
        |s| head(s, "{}")
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
        "`*` and `#`",
        "jump to the next or previous use of the word under the cursor",
        "Find where a variable is used next.",
        |s| head(s, "*#")
    ),
    item!(
        "search-next",
        2,
        "`n` and `N`",
        "the next or previous match of your last search",
        "After `/name`, hop through every match.",
        |s| is(s, &["n", "N"])
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
        "`H`, `M` and `L`",
        "the top, middle or bottom line of the window",
        "Jump to something you can see without counting lines.",
        |s| is(s, &["H", "M", "L"])
    ),
    item!(
        "ciw",
        2,
        "`ciw`",
        "change the whole word under the cursor, from anywhere in it",
        "Rename a variable without moving to its start.",
        |s| object(s, "c", "wW")
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
        "replace a character, or the whole line, and type",
        "Fix one wrong letter and keep typing.",
        |s| is(s, &["s", "S"])
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
        "`Ctrl-o` and `Ctrl-i`",
        "back to where you were before a jump, and forward again",
        "Return after `gg` or a search.",
        |s| ctrl(s, "oi") && s.before == VimMode::Normal
    ),
    item!(
        "center",
        2,
        "`zz`, `zt` and `zb`",
        "scroll so this line is in the middle, top or bottom",
        "Bring the line you're on into the middle of the screen.",
        |s| is(s, &["zz", "zt", "zb"])
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
        "marks",
        3,
        "`ma` and `'a`",
        "set a mark, and jump back to its line",
        "Bookmark a spot you keep returning to.",
        |s| head(s, "m'`") && cmd(s).is_some_and(|c| c.keys.len() == 2)
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
        "`dt<char>`",
        "delete up to just before a character",
        "Remove the rest of an argument up to the comma.",
        |s| op_motion(s, "dcy", "tT")
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
        |s| cmd(s).is_some_and(|c| c.register.is_some())
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
        "switch case, or make upper case over a motion",
        "Turn `const` into `CONST` with `gUiw`.",
        |s| is(s, &["~"]) || op_motion(s, "g", "Uu~")
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
        |s| ctrl(s, "v") && s.before == VimMode::Normal
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
    ("screen", &["half-page"]),
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
    ("gi", &["append"]),
    ("macro", &["dot"]),
    ("replay-macro", &["macro"]),
    ("cgn", &["dot", "star"]),
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
    ("vim.backspace-streak", &[]),
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
    ("motion.scroll", &["center"]),
    ("motion.mark", &["marks"]),
    ("motion.text-object", &["ciw"]),
    ("motion.operator", &["word-end", "till", "d-to-end"]),
    ("bash.up-history", &[]),
    ("bash.backspace-clear", &[]),
    ("bash.left-streak", &[]),
    ("shell.cd-up", &[]),
    ("shell.reused-arg", &[]),
    ("shell.one-word-changed", &[]),
    ("shell.keys", &[]),
    ("vscode.quick-open", &[]),
    ("vscode.terminal-toggle", &[]),
    ("vscode.tab-switch", &[]),
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

/// The items you already use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Known {
    /// Nothing is known about you (no memory): everything is ready.
    everything: bool,
    ids: HashSet<&'static str>,
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
        }
    }

    pub fn of(ids: impl IntoIterator<Item = &'static str>) -> Self {
        Known {
            everything: false,
            ids: ids.into_iter().collect(),
        }
    }

    /// Items used often enough, or learned, plus the skills of rules
    /// you've mastered.
    pub fn from_progress<'a>(
        progress: &dyn Fn(&str) -> Progress,
        mastered_rules: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        let mut ids: HashSet<&'static str> = CURRICULUM
            .iter()
            .filter(|i| {
                let p = progress(i.id);
                p.learned || p.uses >= KNOWN_USES
            })
            .map(|i| i.id)
            .collect();
        for rule in mastered_rules {
            ids.extend(rule_skills(rule).unwrap_or_default());
        }
        Known {
            everything: false,
            ids,
        }
    }

    pub fn knows(&self, id: &str) -> bool {
        self.everything || self.ids.contains(id)
    }

    /// Worth suggesting: known already, or one step up from what is.
    pub fn ready(&self, id: &str) -> bool {
        self.knows(id) || requires(id).iter().all(|r| self.knows(r))
    }

    /// Whether a rule's main advice is something you're ready for.
    pub fn ready_for_rule(&self, rule_id: &str) -> bool {
        rule_skills(rule_id)
            .and_then(|s| s.first())
            .is_none_or(|s| self.ready(s))
    }
}

/// Used this many times, an item counts as known.
pub const KNOWN_USES: u64 = 3;
/// An item is shown at most this many times.
pub const MAX_SHOWINGS: u32 = 3;
/// And not again within this many days.
pub const AGAIN_AFTER_DAYS: u64 = 7;

/// What's known about one item.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// Times used, ever.
    pub uses: u64,
    pub times_shown: u32,
    /// Unix seconds.
    pub last_shown: Option<u64>,
    pub learned: bool,
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
    CURRICULUM.iter().find(|item| {
        let p = progress(item.id);
        known.ready(item.id)
            && crate::support::supports_all(ctx, &tip_text(item))
            && p.uses < KNOWN_USES
            && !p.learned
            && p.times_shown < MAX_SHOWINGS
            && p.last_shown
                .is_none_or(|t| now_s.saturating_sub(t) >= AGAIN_AFTER_DAYS * 86_400)
    })
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
            (":earlier 1m<CR>", "undo-time"),
            ("gUiw", "case"),
            (">ip", "indent-object"),
            ("dt,", "till"),
            (":%s/a/b/g<CR>", "substitute-all"),
            (":g/TODO/d<CR>", "global"),
            ("Vj>", "visual-indent"),
            ("ZZ", "write-quit"),
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
        // A mastered rule's skills count as known.
        let k = Known::from_progress(&|_| Progress::default(), ["motion.find"]);
        assert!(k.knows("find") && k.ready("till"));
        assert!(!none.ready_for_rule("motion.text-object"));
        assert!(none.ready_for_rule("motion.count") && none.ready_for_rule("shell.cd-up"));
    }

    #[test]
    fn something_new_waits_for_what_it_builds_on() {
        // Everything is used but the text objects.
        let objects = ["ciw", "ci-quote", "ci-paren", "tag-object", "visual-object"];
        let progress = |id: &str| Progress {
            uses: if objects.contains(&id) { 0 } else { 99 },
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
                    !crate::glossary::explain_keys(span, false).is_empty(),
                    "{}: `{span}` has no explanation",
                    item.id
                );
            }
        }
    }
}
