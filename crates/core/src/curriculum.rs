//! A curriculum of Vim features, ordered from basic to advanced, for the
//! daily "something new" tip. Each item knows how to recognise itself in
//! the key tracker's steps, so Flow Buddy can tell what you already use.

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
