//! What the commands in a tip do, in a few words each.
//!
//! Tips mark commands with backticks ("Try `Ctrl-d` then `5j`"). The
//! nudge's optional "what does it do?" section comes from [`explain`],
//! which splits each marked command into its pieces (count, operator,
//! motion or text object) and looks them up here.

/// One line per piece of each marked command in `text`, in order, without
/// repeats: "`Ctrl-d`: down half a page". `insert` picks Insert-mode
/// meanings where keys differ (Ctrl-u, Ctrl-w, Ctrl-r, Ctrl-o).
pub fn explain(text: &str, insert: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for span in code_spans(text) {
        for (piece, meaning) in explain_keys(span, insert) {
            let line = format!("{}: {meaning}", mark(&piece));
            if !out.contains(&line) {
                out.push(line);
            }
        }
    }
    out
}

/// A piece of tip text: plain words, or a command marked with backticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment<'a> {
    Text(&'a str),
    Code(&'a str),
}

/// Split `text` the Markdown way: a run of backticks opens a command, the
/// next run of the same length closes it, and one space just inside is
/// dropped, so `` `a `` marks a command with a backtick in it. An
/// unclosed run is plain text.
pub fn segments(text: &str) -> Vec<Segment<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('`') {
        let fence_len = rest[open..].len() - rest[open..].trim_start_matches('`').len();
        let body = &rest[open + fence_len..];
        let Some(close) = closing(body, fence_len) else {
            break;
        };
        if open > 0 {
            out.push(Segment::Text(&rest[..open]));
        }
        let inner = &body[..close];
        let inner = match inner.strip_prefix(' ').and_then(|i| i.strip_suffix(' ')) {
            Some(trimmed) if !trimmed.is_empty() => trimmed,
            _ => inner,
        };
        out.push(Segment::Code(inner));
        rest = &body[close + fence_len..];
    }
    if !rest.is_empty() {
        out.push(Segment::Text(rest));
    }
    out
}

/// Where a run of exactly `n` backticks starts in `body`.
fn closing(body: &str, n: usize) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = body[from..].find('`') {
        let at = from + i;
        let run = body[at..].len() - body[at..].trim_start_matches('`').len();
        if run == n {
            return Some(at);
        }
        from = at + run;
    }
    None
}

/// The commands marked in `text`.
pub fn code_spans(text: &str) -> Vec<&str> {
    segments(text)
        .into_iter()
        .filter_map(|s| match s {
            Segment::Code(c) => Some(c),
            Segment::Text(_) => None,
        })
        .collect()
}

/// `text` with the command markers removed, for plain notifications.
pub fn plain(text: &str) -> String {
    segments(text)
        .into_iter()
        .map(|s| match s {
            Segment::Text(t) | Segment::Code(t) => t,
        })
        .collect()
}

/// Mark a command, using a longer fence if it has a backtick in it.
pub fn mark(cmd: &str) -> String {
    if cmd.contains('`') {
        format!("`` {cmd} ``")
    } else {
        format!("`{cmd}`")
    }
}

/// The pieces of one command such as `3dw`, `Ctrl-d×10` or `f<char>`, each
/// with what it does. Unknown keys are left out.
pub fn explain_keys(keys: &str, insert: bool) -> Vec<(String, &'static str)> {
    let mut out = Vec::new();
    if let Some(m) = phrase(keys) {
        out.push((keys.to_string(), m));
        return out;
    }
    for word in keys.split_whitespace() {
        if matches!(word, "then" | "or" | "…" | "and") {
            continue;
        }
        // `Ctrl-d×10`: the count of presses is clear enough.
        let word = word.split('×').next().unwrap_or(word).trim_end_matches(',');
        if word.is_empty() {
            continue;
        }
        if let Some(m) = whole(word, insert) {
            out.push((word.to_string(), m));
            continue;
        }
        // A count before a chord: `6Ctrl-r`.
        let digits = word.len() - word.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits > 0
            && let Some(m) = whole(&word[digits..], insert)
        {
            out.push((
                word[..digits].to_string(),
                "a count: do the next command that many times",
            ));
            out.push((word[digits..].to_string(), m));
            continue;
        }
        parse(word, &mut out);
    }
    out
}

/// Shell commands explained as a whole.
fn phrase(keys: &str) -> Option<&'static str> {
    Some(match keys {
        "cd ../.." => "go up two folders at once (add more ../ for more)",
        "cd -" => "go back to the folder you were in before",
        "cd .." => "go up one folder",
        "!$" => "the previous command's last argument",
        "Alt-." => "insert the previous command's last argument (again for older ones)",
        "^old^new" => "rerun the last command with old replaced by new",
        "Up" => "bring back the previous command to edit",
        "Ctrl-P" => "open a file by typing part of its name (VS Code)",
        ":s/old/new/" => "replace old with new on this line",
        ":%s/old/new/g" => "replace old with new everywhere in the file",
        ":g/pattern/d" => "delete every line that matches the pattern",
        ":norm" => "run Normal-mode keys on each selected line",
        ":.,+5d" => "delete this line and the five after it",
        ":sort" => "sort the selected lines",
        ":earlier 1m" => "go back to how the file was a minute ago",
        ":cn" => "to the next result in the quickfix list (errors, grep matches)",
        ":cp" => "to the previous result in the quickfix list",
        "<line>G" => "to the line with that number (type the number, then G)",
        ":<line>" => "to the line with that number",
        "42G" => "to line 42",
        ":42" => "to line 42",
        "Ctrl-w h" => "go to the window on the left",
        "Ctrl-w j" => "go to the window below",
        "Ctrl-w k" => "go to the window above",
        "Ctrl-w l" => "go to the window on the right",
        "Ctrl-w w" => "go to the next window",
        "Ctrl-w v" => "split this editor side by side",
        ":vsplit" => "split this editor side by side",
        ":b" => "switch to a buffer by typing part of its name",
        ":bn" => "switch to the next buffer",
        ":e #" => "open the file you had open before",
        "Ctrl-r\"" => "paste while typing (Ctrl-r then any register's name)",
        ":later 1m" => "forward again a minute's worth of changes",
        "Ctrl-`" => "jump to the terminal and back (VS Code)",
        _ => return None,
    })
}

/// Commands that are explained as a whole.
fn whole(word: &str, insert: bool) -> Option<&'static str> {
    if insert {
        match word {
            "Ctrl-w" => return Some("delete the word before the cursor"),
            "Ctrl-u" => return Some("delete back to the start of the line"),
            "Ctrl-r" => return Some("search back through history as you type (bash)"),
            "Ctrl-o" => return Some("run one Normal-mode command, then keep typing"),
            "Ctrl-l" => return Some("clear the screen, keeping what you've typed (shell)"),
            "Ctrl-n" => return Some("complete the word from words in your open files"),
            "Ctrl-a" => return Some("insert the text you typed last time"),
            "Esc" => return Some("leave Insert mode"),
            _ => {}
        }
    }
    Some(match word {
        "Ctrl-d" => "down half a page",
        "Ctrl-u" => "up half a page",
        "Ctrl-f" => "down a whole page",
        "Ctrl-b" => "up a whole page",
        "Ctrl-e" => "scroll the view down a line; the cursor stays",
        "Ctrl-y" => "scroll the view up a line; the cursor stays",
        "Ctrl-o" => "back to where you were before the last jump",
        "Ctrl-i" => "forward again after Ctrl-o",
        "Ctrl-r" => "redo (in bash: search back through history as you type)",
        "Ctrl-w" => "a window command: the next key says which",
        "Ctrl-v" => "select a block (columns)",
        "Ctrl-n" => "complete the word you're typing (in Insert mode)",
        "Ctrl-^" => "switch to the file you had open before",
        "Ctrl-a" => "add one to the number under the cursor",
        "Ctrl-x" => "subtract one from the number under the cursor",
        "Esc" => "back to Normal mode",
        ":wq" | ":x" => "save and quit",
        ":w" => "save",
        ":q" => "quit",
        "ZZ" => "save and quit",
        "ZQ" => "quit without saving",
        ":q!" => "quit without saving",
        "/" => "search forward for what you type, then Enter",
        "''" => "back to the line you jumped from",
        "@@" => "replay the macro you played last",
        "qq" => "start recording a macro into register q; q stops",
        "@q" => "play the macro recorded in q",
        "q" => "stop recording the macro",
        _ => return None,
    })
}

fn motion(c: char) -> Option<&'static str> {
    Some(match c {
        'h' => "left one character",
        'j' => "down one line",
        'k' => "up one line",
        'l' => "right one character",
        'w' => "to the start of the next word",
        'b' => "back to the start of a word",
        'e' => "to the end of the word",
        'W' => "to the next WORD (anything between spaces)",
        'B' => "back a WORD",
        'E' => "to the end of the WORD",
        '0' => "to the start of the line",
        '^' => "to the first non-blank character",
        '$' => "to the end of the line",
        '}' => "down to the next blank line (end of the paragraph)",
        '{' => "up to the previous blank line",
        'H' => "to the top line of the window",
        'M' => "to the middle line of the window",
        'L' => "to the bottom line of the window",
        'G' => "to the last line",
        '%' => "to the matching bracket",
        '*' => "to the next use of the word under the cursor",
        '#' => "to the previous use of the word under the cursor",
        'n' => "to the next match of the last search",
        'N' => "to the previous match of the last search",
        ';' => "repeat the last f, t, F or T",
        ',' => "repeat the last f, t, F or T backwards",
        _ => return None,
    })
}

fn find(c: char) -> Option<&'static str> {
    Some(match c {
        'f' => "forward to that character on the line",
        'F' => "back to that character on the line",
        't' => "forward to just before that character",
        'T' => "back to just after that character",
        _ => return None,
    })
}

fn operator(c: char) -> Option<&'static str> {
    Some(match c {
        'd' => "delete",
        'c' => "delete, then start typing (change)",
        'y' => "copy (yank)",
        '>' => "indent",
        '<' => "unindent",
        '=' => "re-indent",
        _ => return None,
    })
}

fn object(c: char) -> Option<&'static str> {
    Some(match c {
        'w' => "a word",
        'W' => "a WORD (anything between spaces)",
        's' => "a sentence",
        'p' => "a paragraph",
        '(' | ')' | 'b' => "the ( ) brackets",
        '[' | ']' => "the [ ] brackets",
        '{' | '}' | 'B' => "the { } braces",
        '<' | '>' => "the < > brackets",
        't' => "an XML/HTML tag",
        '"' => "the \" quotes",
        '\'' => "the ' quotes",
        '`' => "the ` quotes",
        _ => return None,
    })
}

fn single(c: char) -> Option<&'static str> {
    Some(match c {
        'x' => "delete the character under the cursor",
        'X' => "delete the character before the cursor",
        's' => "delete the character and start typing",
        'S' => "replace the whole line: clear it and start typing",
        'C' => "change from the cursor to the end of the line",
        'D' => "delete from the cursor to the end of the line",
        'i' => "start typing before the cursor",
        'a' => "start typing after the cursor",
        'I' => "start typing at the first non-blank",
        'A' => "start typing at the end of the line",
        'o' => "open a new line below and type",
        'O' => "open a new line above and type",
        'p' => "paste after the cursor",
        'P' => "paste before the cursor",
        'J' => "join the next line onto this one",
        'u' => "undo",
        'U' => "undo every change on the last changed line",
        '&' => "repeat the last :s on this line",
        '.' => "repeat the last change",
        '~' => "switch the case of the character",
        'R' => "type over the existing text (Replace mode)",
        'v' => "select characters",
        'V' => "select whole lines",
        _ => return None,
    })
}

/// Parse a run of Normal-mode keys: `3dw`, `di(`, `ma`, `}^`, `f<char>;`.
fn parse(word: &str, out: &mut Vec<(String, &'static str)>) {
    let chars: Vec<char> = word.replace("<char>", "\u{0}").chars().collect();
    let piece = |s: &[char]| s.iter().collect::<String>().replace('\u{0}', "<char>");
    let mut i = 0;
    while i < chars.len() {
        let start = i;
        if chars[i].is_ascii_digit() && chars[i] != '0' {
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            out.push((
                piece(&chars[start..i]),
                "a count: do the next command that many times",
            ));
            continue;
        }
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        // Doubled operators and line commands.
        if let Some(n) = next
            && n == c
            && matches!(c, 'd' | 'c' | 'y' | '>' | '<' | 'g' | 'z')
        {
            let m = match c {
                'd' => "delete the whole line",
                'c' => "replace the whole line: clear it and start typing",
                'y' => "copy the whole line",
                '>' => "indent the line",
                '<' => "unindent the line",
                'g' => "to the first line",
                _ => "scroll so this line is in the middle",
            };
            out.push((piece(&chars[i..i + 2]), m));
            i += 2;
            continue;
        }
        if let Some(n) = next {
            let two = match (c, n) {
                ('g', 'e') => Some("back to the end of the previous word"),
                ('g', '_') => Some("to the last non-blank character"),
                ('z', 't') => Some("scroll so this line is at the top"),
                ('z', 'a') => Some("open or close the fold under the cursor"),
                ('z', 'c') => Some("close the fold under the cursor"),
                ('z', 'o') => Some("open the fold under the cursor"),
                ('g', 'U') => Some("make upper case over a motion"),
                ('g', 'u') => Some("make lower case over a motion"),
                ('g', 'v') => Some("select the last selection again"),
                ('g', 'i') => Some("start typing where you last stopped typing"),
                ('g', 'I') => Some("start typing at the very start of the line"),
                ('g', '~') => Some("switch case over a motion"),
                ('g', 't') => Some("to the next tab"),
                ('g', 'T') => Some("to the previous tab"),
                ('g', '&') => Some("repeat the last :s on every line"),
                ('"', _) => {
                    Some("use that register for the next command (\"0 holds the last yank)")
                }
                ('r', '\u{0}') => {
                    Some("replace the character under the cursor with the one you type")
                }
                ('g', 'n') => Some("the next search match (select it, or change it with cgn)"),
                ('g', '-') => Some("back to the previous state in undo history"),
                ('g', ';') => Some("back to where you last changed something (again for older)"),
                ('`' | '\'', '.') => Some("jump to where you last changed something"),
                ('z', 'b') => Some("scroll so this line is at the bottom"),
                ('m', _) if n.is_ascii_lowercase() => Some("set mark here (any letter names it)"),
                ('\'', _) if n.is_ascii_lowercase() => Some("jump to the mark's line"),
                ('`', _) if n.is_ascii_lowercase() => Some("jump to the mark's exact spot"),
                ('q', _) if n.is_ascii_lowercase() => Some("start recording a macro; q stops"),
                ('@', _) if n.is_ascii_lowercase() || n == '@' => Some("play a recorded macro"),
                _ => None,
            };
            if let Some(m) = two {
                out.push((piece(&chars[i..i + 2]), m));
                i += 2;
                continue;
            }
        }
        if let Some(m) = find(c)
            && next == Some('\u{0}')
        {
            out.push((piece(&chars[i..i + 2]), m));
            i += 2;
            continue;
        }
        if let Some(m) = operator(c) {
            out.push((piece(&chars[i..i + 1]), m));
            i += 1;
            // A text object: i or a, then what it's around.
            if let (Some(&k @ ('i' | 'a')), Some(&o)) = (chars.get(i), chars.get(i + 1))
                && let Some(what) = object(o)
            {
                let m = if k == 'i' { inside(what) } else { around(what) };
                out.push((piece(&chars[i..i + 2]), m));
                i += 2;
            }
            continue;
        }
        if let Some(m) = motion(c).or_else(|| single(c)) {
            out.push((piece(&chars[i..i + 1]), m));
        }
        i += 1;
        // Visual mode, then a text object to select (`vi(`).
        if matches!(c, 'v' | 'V')
            && let (Some(&k @ ('i' | 'a')), Some(&o)) = (chars.get(i), chars.get(i + 1))
            && let Some(what) = object(o)
        {
            out.push((
                piece(&chars[i..i + 2]),
                if k == 'i' { inside(what) } else { around(what) },
            ));
            i += 2;
        }
    }
}

fn inside(what: &str) -> &'static str {
    match what {
        "a word" => "inside the word (just the word)",
        "a WORD (anything between spaces)" => "inside the WORD",
        "a sentence" => "inside the sentence",
        "a paragraph" => "inside the paragraph (its lines)",
        "the ( ) brackets" => "inside the ( ) brackets, from anywhere in them",
        "the [ ] brackets" => "inside the [ ] brackets, from anywhere in them",
        "the { } braces" => "inside the { } braces, from anywhere in them",
        "the < > brackets" => "inside the < > brackets",
        "an XML/HTML tag" => "inside the tag",
        "the \" quotes" => "inside the \" quotes, from anywhere in them",
        "the ' quotes" => "inside the ' quotes",
        _ => "inside the ` quotes",
    }
}

fn around(what: &str) -> &'static str {
    match what {
        "a word" => "a word and the space after it",
        "a WORD (anything between spaces)" => "a WORD and the space after it",
        "a sentence" => "a sentence and the space after it",
        "a paragraph" => "a paragraph and the blank line after it",
        "the ( ) brackets" => "the ( ) brackets and everything in them",
        "the [ ] brackets" => "the [ ] brackets and everything in them",
        "the { } braces" => "the { } braces and everything in them",
        "the < > brackets" => "the < > brackets and everything in them",
        "an XML/HTML tag" => "the tag and everything in it",
        "the \" quotes" => "the \" quotes and everything in them",
        "the ' quotes" => "the ' quotes and everything in them",
        _ => "the ` quotes and everything in them",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(k: &str) -> Vec<String> {
        explain_keys(k, false).into_iter().map(|(p, _)| p).collect()
    }

    #[test]
    fn splits_commands_into_pieces() {
        assert_eq!(keys("3dw"), ["3", "d", "w"]);
        assert_eq!(keys("di("), ["d", "i("]);
        assert_eq!(keys("ci\""), ["c", "i\""]);
        assert_eq!(keys("dd"), ["dd"]);
        assert_eq!(keys("f<char>;"), ["f<char>", ";"]);
        assert_eq!(keys("Ctrl-d×10 then 7k"), ["Ctrl-d", "7", "k"]);
        assert_eq!(keys("}^"), ["}", "^"]);
        assert_eq!(keys("ma"), ["ma"]);
        assert_eq!(keys("'a"), ["'a"]);
        assert_eq!(keys("gg"), ["gg"]);
        assert_eq!(keys("zt"), ["zt"]);
        assert_eq!(keys("@q"), ["@q"]);
        assert_eq!(keys(":wq"), [":wq"]);
    }

    #[test]
    fn explains_marked_commands_once() {
        let lines = explain(
            "That took 9 keys. `Ctrl-d` then `5j` gets there; or `Ctrl-d`.",
            false,
        );
        assert_eq!(
            lines,
            [
                "`Ctrl-d`: down half a page",
                "`5`: a count: do the next command that many times",
                "`j`: down one line",
            ]
        );
    }

    #[test]
    fn text_objects() {
        let lines = explain("`di(` does it from anywhere inside.", false);
        assert_eq!(
            lines,
            [
                "`d`: delete",
                "`i(`: inside the ( ) brackets, from anywhere in them"
            ]
        );
    }

    #[test]
    fn insert_mode_meanings() {
        assert_eq!(
            explain("Try `Ctrl-u`.", true),
            ["`Ctrl-u`: delete back to the start of the line"]
        );
        assert_eq!(
            explain("Try `Ctrl-u`.", false),
            ["`Ctrl-u`: up half a page"]
        );
    }

    #[test]
    fn spans_with_backticks_inside() {
        assert_eq!(
            code_spans("Try `ma`, then `` `a `` or `'a`."),
            ["ma", "`a", "'a"]
        );
        assert_eq!(code_spans("no marks"), Vec::<&str>::new());
        assert_eq!(code_spans("unclosed `x"), Vec::<&str>::new());
        assert_eq!(
            explain("`` `a ``", false),
            ["`` `a ``: jump to the mark's exact spot"]
        );
    }

    #[test]
    fn plain_text() {
        assert_eq!(plain("Try `7j`, `}` or `Ctrl-d`."), "Try 7j, } or Ctrl-d.");
        assert_eq!(plain("Then `` `a `` jumps."), "Then `a jumps.");
    }

    #[test]
    fn unmarked_text_is_not_explained() {
        assert!(explain("You pressed j 7 times.", false).is_empty());
    }
}
