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
    pieces(keys, insert, false)
}

/// What `explain_keys` says of a key it doesn't know, when asked to keep
/// those.
pub const UNKNOWN: &str = "not sure what this does";

/// Keys as someone types them to ask what they do: `<C-w>v`, `C-d`,
/// `ct,`, `:%s/a/b/g`. Unlike [`explain_keys`], keys it doesn't know are
/// kept, with [`UNKNOWN`].
pub fn explain_typed(keys: &str, insert: bool) -> Vec<(String, &'static str)> {
    let keys = ctrl_notation(keys.trim());
    if let Some(m) = phrase(&keys) {
        return vec![(keys, m)];
    }
    if keys.starts_with(':') {
        return vec![(keys.clone(), ex(&keys))];
    }
    pieces(&keys, insert, true)
}

/// `<C-w>v`, `<c-w>v`, `C-w v`, `Ctrl+w v` → `Ctrl-w v`.
fn ctrl_notation(keys: &str) -> String {
    let chars: Vec<char> = keys.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let rest: String = chars[i..].iter().collect();
        let lower = rest.to_ascii_lowercase();
        let prefix = ["<c-", "ctrl-", "ctrl+", "c-"]
            .into_iter()
            .find(|p| lower.starts_with(p));
        let at_start = i == 0 || chars[i - 1].is_whitespace() || chars[i - 1] == '>';
        if let Some(p) = prefix
            && (p == "<c-" || at_start)
            && let Some(&k) = chars.get(i + p.len())
            && k.is_ascii_graphic()
        {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str("Ctrl-");
            // Ctrl takes letters as lower case, but Ctrl-P is VS Code's.
            out.push(if p == "<c-" {
                k.to_ascii_lowercase()
            } else {
                k
            });
            i += p.len() + 1;
            if p == "<c-" && chars.get(i) == Some(&'>') {
                i += 1;
            }
            if chars.get(i).is_some_and(|c| !c.is_whitespace()) {
                out.push(' ');
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// An Ex command: known ones as a whole, else by its name after any range.
fn ex(cmd: &str) -> &'static str {
    if let Some(m) = whole(cmd, false) {
        return m;
    }
    let body = cmd[1..].trim_start_matches(|c: char| {
        c.is_ascii_digit()
            || matches!(
                c,
                '%' | ',' | '.' | '$' | '\'' | '<' | '>' | '+' | '-' | ' '
            )
    });
    let name: String = body
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    let rest = &body[name.len()..];
    match name.as_str() {
        "s" | "substitute" => {
            if cmd.contains('%') {
                "replace a pattern on every line (:%s/old/new/g)"
            } else {
                "replace a pattern (:s/old/new/), on this line or the range given"
            }
        }
        "g" | "global" => "run a command on every line that matches (:g/pattern/d)",
        "v" | "vglobal" => "run a command on every line that doesn't match",
        "" if rest.is_empty() => "to that line",
        _ => whole(&format!(":{name}"), false)
            .unwrap_or("an Ex command: type it after :, Enter runs it"),
    }
}

fn pieces(keys: &str, insert: bool, keep_unknown: bool) -> Vec<(String, &'static str)> {
    let mut out = Vec::new();
    if let Some(m) = phrase(keys) {
        out.push((keys.to_string(), m));
        return out;
    }
    let words: Vec<&str> = keys.split_whitespace().collect();
    let mut skip = false;
    for (n, &word) in words.iter().enumerate() {
        if std::mem::take(&mut skip) || matches!(word, "then" | "or" | "…" | "and") {
            continue;
        }
        // A chord and the key after it: `Ctrl-w v`.
        if let Some(next) = words.get(n + 1)
            && let pair = format!("{word} {next}")
            && let Some(m) = phrase(&pair)
        {
            out.push((pair, m));
            skip = true;
            continue;
        }
        // `Ctrl-d×10`: the count of presses is clear enough.
        let mut word = word.split('×').next().unwrap_or(word);
        // A comma after a command in a tip's list; typed, it's a key.
        if !keep_unknown {
            word = word.trim_end_matches(',');
        }
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
        if keep_unknown && word.starts_with("Ctrl-") {
            out.push((word.to_string(), UNKNOWN));
            continue;
        }
        parse(word, &mut out, keep_unknown);
    }
    out
}

/// Shell commands explained as a whole.
fn phrase(keys: &str) -> Option<&'static str> {
    Some(match keys {
        "cd ../.." => "go up two folders at once (add more ../ for more)",
        "cd -" => "go back to the folder you were in before",
        "Ctrl-x Ctrl-f" => "while typing: complete a file name",
        "Ctrl-x Ctrl-l" => "while typing: complete a whole line",
        "Ctrl-x Ctrl-o" => "while typing: complete code (omni completion)",
        "Ctrl-r Ctrl-w" => "on the command line: put in the word under the cursor",
        "q:" => "edit earlier commands in a window",
        "Tab" => "complete the name you're typing",
        "Ctrl-w N" => "in a terminal window: stop it to scroll and copy",
        ":terminal" => "a shell in a Vim window",
        "Ctrl-r =" => "while typing: work out a sum and type the answer",
        "Ctrl-g u" => "while typing: start a new undo step",
        "," => "back to the character you last found (f, t, F or T)",
        "g Ctrl-a" => "on a selection: count up line by line (1, 2, 3…)",
        "g Ctrl-g" => "count the words, lines and characters",
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
        "/<text>" => "search forward for a few letters of what's there, then Enter",
        "?<text>" => "search back for a few letters of what's there, then Enter",
        "]]" => "to the next section (a line starting with {)",
        "[[" => "to the previous section",
        "[(" => "back to the unmatched ( around the cursor",
        "])" => "forward to the unmatched ) around the cursor",
        ":%s" => "repeat the last substitute on every line",
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
            "Ctrl-t" => return Some("indent the line"),
            "Ctrl-d" => return Some("unindent the line"),
            "Ctrl-e" => return Some("copy the character below the cursor"),
            "Ctrl-y" => return Some("copy the character above the cursor"),
            "Ctrl-v" => return Some("type the next key as it is (a real Tab, say)"),
            "Ctrl-k" => return Some("type a special character from two keys (e' is é)"),
            "Ctrl-p" => return Some("complete the word from earlier words"),
            "Ctrl-x" => return Some("start a kind of completion: the next key says which"),
            "Ctrl-f" => return Some("after Ctrl-x: complete a file name"),
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
        ":" => "start a command (Enter runs it)",
        ":w" => "save",
        ":q" => "quit",
        "ZZ" => "save and quit",
        "ZQ" => "quit without saving",
        ":q!" => "quit without saving",
        ":e" => "open a file by name",
        ":e!" => "reload the file, throwing away changes",
        ":noh" => "clear the search highlighting",
        ":h" => "Vim's help on what you type after it",
        ":wa" => "save every file",
        ":up" => "save only if something changed",
        ":ls" => "list the open files",
        ":set" => "change a setting",
        ":cd" => "change the folder Vim works in",
        ":pwd" => "show the folder Vim works in",
        ":!" => "run a shell command",
        ":r !" => "put a shell command's output into the file",
        ":m" => "move lines (to after the line number given)",
        ":t" => "copy lines (to after the line number given)",
        ":d" => "delete lines",
        ":y" => "copy lines",
        ":j" => "join lines",
        ":bufdo" => "run a command in every open file",
        ":argdo" => "run a command in every file in the list",
        ":args" => "set or show the list of files to work through",
        ":next" => "the next file in the list",
        ":vimgrep" => "search many files; matches go to the quickfix list",
        ":grep" => "search many files with grep; matches go to the quickfix list",
        ":make" => "build; errors go to the quickfix list",
        ":mksession" => "save windows and files to reopen later",
        ":iabbrev" => "a short word that expands as you type",
        ":oldfiles" => "list the files you opened recently",
        ":messages" => "show messages you missed",
        ":vert" => "make the next split side by side",
        ":marks" => "list your marks",
        "Ctrl-]" => "jump to the definition through the tags file",
        "Ctrl-t" => "back from a tag jump (in Insert mode: indent the line)",
        ":tabnew" => "open a new tab",
        ":tabonly" => "close every tab but this one",
        "g<Tab>" => "back to the tab you were in last",
        "Ctrl-g" => "show the file's name and where you are in it",
        ":reg" => "show what every register holds",
        ":put" => "paste on a line of its own",
        ":jumps" => "list the places you jumped from",
        ":retab" => "turn tabs into spaces, or back, by the file's settings",
        ":center" => "center the lines",
        ":right" => "line the lines up on the right",
        ":left" => "line the lines up on the left",
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
        '(' => "back to the start of the sentence",
        ')' => "forward to the next sentence",
        ',' => "repeat the last f, t, F or T backwards",
        '+' => "to the first character of the next line",
        '-' => "to the first character of the previous line",
        '_' => "to the first character of this line (a count goes down)",
        '|' => "to that column (the count) of the line",
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
        '!' => "filter through a shell command",
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
        'D' => "delete from the cursor to the end of the line (in Visual mode: the selected lines)",
        'i' => "start typing before the cursor",
        'a' => "start typing after the cursor",
        'I' => "start typing at the first non-blank",
        'A' => "start typing at the end of the line",
        'o' => "open a new line below and type (in Visual mode: to the other end of the selection)",
        'O' => "open a new line above and type",
        'p' => "paste after the cursor",
        'P' => {
            "paste before the cursor (in Visual mode: over the selection, keeping what you copied)"
        }
        'J' => "join the next line onto this one",
        'u' => "undo (in Visual mode: make lower case)",
        'U' => "undo every change on the last changed line (in Visual mode: make upper case)",
        'Y' => "copy the line (in Visual mode: the selected lines)",
        '&' => "repeat the last :s on this line",
        '.' => "repeat the last change",
        '~' => "switch the case of the character",
        'R' => "type over the existing text (Replace mode)",
        'v' => "select characters",
        'V' => "select whole lines",
        'K' => "look up the word under the cursor (man page or help)",
        _ => return None,
    })
}

/// Parse a run of Normal-mode keys: `3dw`, `di(`, `ma`, `}^`, `f<char>;`.
fn parse(word: &str, out: &mut Vec<(String, &'static str)>, keep_unknown: bool) {
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
            && matches!(c, 'd' | 'c' | 'y' | '>' | '<' | '=' | '!' | 'g' | 'z')
        {
            let m = match c {
                'd' => "delete the whole line",
                'c' => "replace the whole line: clear it and start typing",
                'y' => "copy the whole line",
                '>' => "indent the line",
                '<' => "unindent the line",
                '=' => "re-indent the line",
                '!' => "filter the line through a shell command",
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
                ('g', 'q') => Some("wrap lines to the text width over a motion"),
                ('g', 'w') => Some("wrap lines over a motion, keeping the cursor"),
                ('g', 'J') => Some("join lines without adding a space"),
                ('g', 'd') => Some("to where the word under the cursor is defined"),
                ('g', 'D') => Some("to where the word is defined, from the top of the file"),
                ('g', 'E') => Some("back to the end of the previous WORD"),
                ('g', 'f') => Some("open the file named under the cursor"),
                ('g', 'F') => Some("open the file named under the cursor, at its line number"),
                ('g', '*') => Some("to the next match of the word, inside longer words too"),
                ('g', '#') => {
                    Some("back to the previous match of the word, inside longer words too")
                }
                ('g', 'm') => Some("to the middle of the screen width"),
                ('g', 'M') => Some("to the middle of the line"),
                ('g', '0') => Some("to the start of the line as shown on screen"),
                ('g', '^') => Some("to the first non-blank of the line as shown on screen"),
                ('g', '$') => Some("to the end of the line as shown on screen"),
                ('g', '<') => Some("show the last command's output again"),
                ('g', 'p') => Some("paste after the cursor and land after the pasted text"),
                ('g', 'P') => Some("paste before the cursor and land after the pasted text"),
                (']' | '[', 'p') => Some("paste with the indent of this line"),
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
                ('g', 'j') => Some("down a line as shown on screen (wrapped lines)"),
                ('g', 'k') => Some("up a line as shown on screen"),
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
                ('`', '`') => Some("back to where you jumped from"),
                ('`' | '\'', '[') => Some("to the start of what you last changed or pasted"),
                ('`' | '\'', ']') => Some("to the end of what you last changed or pasted"),
                ('`' | '\'', '<') => Some("to the start of the last selection"),
                ('`' | '\'', '>') => Some("to the end of the last selection"),
                (']', '\'' | '`') => Some("to the next mark"),
                ('[', '\'' | '`') => Some("to the previous mark"),
                (']', 'c') => Some("to the next change between the files being compared"),
                ('[', 'c') => Some("to the previous change between the files being compared"),
                ('d', 'o') => Some("take this change from the other file (diff obtain)"),
                ('d', 'p') => Some("give this change to the other file (diff put)"),
                (']', 'm') => Some("to the start of the next method"),
                ('[', 'm') => Some("to the start of the previous method"),
                ('[', '/') => Some("to the start of the comment"),
                (']', '/') => Some("to the end of the comment"),
                ('[', 'I') => Some("list every line with the word under the cursor"),
                ('z', 'h') => Some("scroll the view left (when lines don't wrap)"),
                ('z', 'l') => Some("scroll the view right (when lines don't wrap)"),
                ('z', 's') => Some("scroll sideways so the cursor is at the left edge"),
                ('z', 'e') => Some("scroll sideways so the cursor is at the right edge"),
                ('z', 'j') => Some("to the start of the next fold"),
                ('z', 'k') => Some("to the end of the previous fold"),
                ('z', 'm') => Some("fold one more level everywhere"),
                ('z', 'r') => Some("open one more level of folds everywhere"),
                ('z', 'f') => Some("make a fold over a motion"),
                ('z', 'd') => Some("remove the fold under the cursor"),
                ('z', '=') => Some("suggest spellings for the word"),
                ('z', 'g') => Some("add the word to your spelling list"),
                (']', 's') => Some("to the next misspelled word"),
                ('[', 's') => Some("to the previous misspelled word"),
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
                if matches!((c, n), ('g', 'U' | 'u' | '~' | 'q' | 'w') | ('z', 'f')) {
                    i += object_after(&chars, i, out);
                }
                continue;
            }
        }
        // `f<char>` in tips; a real character when typed: `ct,`.
        if let Some(m) = find(c)
            && next.is_some()
        {
            out.push((piece(&chars[i..i + 2]), m));
            i += 2;
            continue;
        }
        if let Some(m) = operator(c) {
            out.push((piece(&chars[i..i + 1]), m));
            i += 1;
            let took = object_after(&chars, i, out);
            // `ci` on its own: the start of a text object.
            if took == 0
                && i + 1 == chars.len()
                && let Some(&k @ ('i' | 'a')) = chars.get(i)
            {
                out.push((
                    k.to_string(),
                    if k == 'i' {
                        "inside: then say what (w a word, ( brackets, \" quotes…)"
                    } else {
                        "around: then say what (w a word, ( brackets, \" quotes…)"
                    },
                ));
                i += 1;
            }
            i += took;
            continue;
        }
        if let Some(m) = motion(c).or_else(|| single(c)) {
            out.push((piece(&chars[i..i + 1]), m));
        } else if keep_unknown {
            out.push((piece(&chars[i..i + 1]), UNKNOWN));
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

/// A text object at `chars[i..]` (i or a, then what it's around), after an
/// operator: pushed, and how many keys it took.
fn object_after(chars: &[char], i: usize, out: &mut Vec<(String, &'static str)>) -> usize {
    if let (Some(&k @ ('i' | 'a')), Some(&o)) = (chars.get(i), chars.get(i + 1))
        && let Some(what) = object(o)
    {
        let m = if k == 'i' { inside(what) } else { around(what) };
        out.push((chars[i..i + 2].iter().collect(), m));
        return 2;
    }
    0
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

    fn typed(k: &str) -> Vec<(String, &'static str)> {
        explain_typed(k, false)
    }

    #[test]
    fn typed_keys() {
        let p = |k: &str| typed(k).into_iter().map(|(p, _)| p).collect::<Vec<_>>();
        assert_eq!(p("ct,"), ["c", "t,"]);
        assert_eq!(p("gUiw"), ["gU", "iw"]);
        assert_eq!(p("<C-w>v"), ["Ctrl-w v"]);
        assert_eq!(p("C-w v"), ["Ctrl-w v"]);
        assert_eq!(p("<C-d>"), ["Ctrl-d"]);
        assert_eq!(p("<C-d>5j"), ["Ctrl-d", "5", "j"]);
        assert_eq!(p("Ctrl+u"), ["Ctrl-u"]);
        assert_eq!(p("3d2w"), ["3", "d", "2", "w"]);
        assert!(typed("ci")[1].1.starts_with("inside"));
        assert_eq!(
            typed(":%s/a/b/g")[0].1,
            "replace a pattern on every line (:%s/old/new/g)"
        );
        assert_eq!(typed(":wq")[0].1, "save and quit");
        assert_eq!(typed(":12")[0].1, "to that line");
        assert_eq!(
            typed(":colorscheme x")[0].1,
            "an Ex command: type it after :, Enter runs it"
        );
        assert_eq!(
            typed("xyz€")[2..],
            [("z".into(), UNKNOWN), ("€".into(), UNKNOWN)]
        );
        assert_eq!(typed("<C-q>"), [("Ctrl-q".to_string(), UNKNOWN)]);
        assert!(typed("").is_empty());
    }

    #[test]
    fn tips_leave_unknown_keys_out() {
        assert_eq!(keys("xz"), ["x"]);
        assert_eq!(keys("f<char>"), ["f<char>"]);
    }

    #[test]
    fn unmarked_text_is_not_explained() {
        assert!(explain("You pressed j 7 times.", false).is_empty());
    }
}
