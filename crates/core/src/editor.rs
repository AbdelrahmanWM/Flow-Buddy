//! What editors report about themselves (IPC messages `editor_focus`,
//! `editor_mode` and `motion`).
//!
//! Keystrokes alone can't tell which pane has focus, what mode the editor
//! is really in, or where a movement was heading. Editor plugins can, and
//! they send it as numbers and flags only: never text, file names or paths.
//! Every struct here rejects unknown fields, so a plugin can't add
//! something text-like by accident.

use serde::{Deserialize, Serialize};

use crate::context::VimMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorKind {
    Vscode,
    Vim,
    Nvim,
}

/// The editor's own idea of its mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorMode {
    Normal,
    Insert,
    Replace,
    Visual,
    VisualLine,
    VisualBlock,
    Cmdline,
    OpPending,
    /// Anything else (Vim's terminal mode, a plugin's own mode, ...).
    Other,
}

impl EditorMode {
    /// The tracker mode this corresponds to; `None` for [`EditorMode::Other`].
    pub fn vim_mode(self) -> Option<VimMode> {
        Some(match self {
            EditorMode::Normal => VimMode::Normal,
            EditorMode::Insert | EditorMode::Replace => VimMode::Insert,
            EditorMode::Visual => VimMode::Visual,
            EditorMode::VisualLine => VimMode::VisualLine,
            EditorMode::VisualBlock => VimMode::VisualBlock,
            EditorMode::Cmdline => VimMode::Cmdline,
            EditorMode::OpPending => VimMode::OpPending,
            EditorMode::Other => return None,
        })
    }
}

/// Where in the window a line is, for `H`, `M` and `L`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenPos {
    Top,
    Middle,
    Bottom,
}

/// One movement: the cursor moving in Normal or Visual mode, from the first
/// move until it rests (the editor decides; about 1 s without moving).
///
/// `lines` and `cols` are deltas, never positions. Times are Unix epoch
/// milliseconds, so the daemon can line them up with the keys it saw.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Motion {
    pub start_ms: u64,
    pub end_ms: u64,
    /// How many separate cursor moves the editor saw.
    pub moves: u32,
    pub mode: EditorMode,
    pub lines: i64,
    pub cols: i64,
    #[serde(default)]
    pub facts: MotionFacts,
}

/// Facts about where a movement ended, computed by the editor from the
/// text it has and sent as numbers only.
///
/// A `*_hops` field is how many presses of that motion land *exactly* on
/// the end position, going in the movement's direction; absent when the
/// motion never lands there exactly (or the editor didn't compute it).
///
/// Vertical facts are about the end line. Within-line facts are about the
/// end column, counted from where the cursor would be on the end line after
/// moving vertically (the start column, clamped to the line's length), so
/// for a move on one line they are simply from the start.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct MotionFacts {
    /// `}` (down) or `{` (up).
    pub paragraph_hops: Option<u32>,
    /// The end line's place in the window, if it's at the top, middle or
    /// bottom row: `H`, `M`, `L`.
    pub screen: Option<ScreenPos>,
    /// Lines visible in the window, for `Ctrl-d`/`Ctrl-u` (half of it).
    pub window_lines: Option<u32>,
    /// The end is the first line (`gg`) or the last (`G`).
    pub file_start: bool,
    pub file_end: bool,
    /// `w` (right) or `b` (left).
    pub word_hops: Option<u32>,
    /// `e` (right) or `ge` (left).
    pub word_end_hops: Option<u32>,
    /// `W` (right) or `B` (left).
    pub bigword_hops: Option<u32>,
    /// `f<c>` then `;` (right) or `F<c>` then `;` (left), where `<c>` is
    /// the character at the end. The character itself is never sent.
    pub find_hops: Option<u32>,
    /// `t<c>` / `T<c>`, with the character just past the end.
    pub till_hops: Option<u32>,
    /// The end is column 0 (`0`), the first non-blank (`^`) or the last
    /// character (`$`).
    pub line_start: bool,
    pub first_nonblank: bool,
    pub line_end: bool,
    /// The end is the bracket matching the one at the start (`%`).
    pub bracket_match: bool,
    /// `*` (down) or `#` (up): the end is that many occurrences away of the
    /// word under the cursor at the start.
    pub star_hops: Option<u32>,
    /// The end is where the cursor was before the last jump (`''`, `Ctrl-o`).
    pub jump_back: bool,
    /// The end is on the line of the last change, away from the start
    /// line (`g;`).
    pub last_change: bool,
    /// `n` or `N` presses: the end is that many matches of the last search
    /// away, in the movement's direction. The pattern is never sent.
    pub search_hops: Option<u32>,
    /// 'relativenumber' is on, so line counts on screen can be read off.
    pub relative_numbers: bool,
    /// How many lines the view scrolled during the movement (the top
    /// visible line's delta; positive is further into the file).
    pub view_scrolled: Option<i64>,
    /// How many times recently (about 10 minutes) the cursor rested on the
    /// end line before. The editor keeps the history; only the count is sent.
    pub revisits: Option<u32>,
}

/// The operators an editor reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    Delete,
    Change,
    Yank,
}

impl Operator {
    pub fn key(self) -> &'static str {
        match self {
            Operator::Delete => "d",
            Operator::Change => "c",
            Operator::Yank => "y",
        }
    }
}

/// Text objects, by their Vim names. A fixed list: an unknown name is
/// rejected like an unknown field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextObject {
    #[serde(rename = "iw")]
    InnerWord,
    #[serde(rename = "aw")]
    AWord,
    #[serde(rename = "iW")]
    InnerBigWord,
    #[serde(rename = "aW")]
    ABigWord,
    #[serde(rename = "ip")]
    InnerParagraph,
    #[serde(rename = "ap")]
    AParagraph,
    #[serde(rename = "i(")]
    InnerParen,
    #[serde(rename = "a(")]
    AParen,
    #[serde(rename = "i[")]
    InnerBracket,
    #[serde(rename = "a[")]
    ABracket,
    #[serde(rename = "i{")]
    InnerBrace,
    #[serde(rename = "a{")]
    ABrace,
    #[serde(rename = "i<")]
    InnerAngle,
    #[serde(rename = "a<")]
    AAngle,
    #[serde(rename = "i\"")]
    InnerDoubleQuote,
    #[serde(rename = "a\"")]
    ADoubleQuote,
    #[serde(rename = "i'")]
    InnerSingleQuote,
    #[serde(rename = "a'")]
    ASingleQuote,
    #[serde(rename = "i`")]
    InnerBacktick,
    #[serde(rename = "a`")]
    ABacktick,
}

impl TextObject {
    pub fn keys(self) -> &'static str {
        match self {
            TextObject::InnerWord => "iw",
            TextObject::AWord => "aw",
            TextObject::InnerBigWord => "iW",
            TextObject::ABigWord => "aW",
            TextObject::InnerParagraph => "ip",
            TextObject::AParagraph => "ap",
            TextObject::InnerParen => "i(",
            TextObject::AParen => "a(",
            TextObject::InnerBracket => "i[",
            TextObject::ABracket => "a[",
            TextObject::InnerBrace => "i{",
            TextObject::ABrace => "a{",
            TextObject::InnerAngle => "i<",
            TextObject::AAngle => "a<",
            TextObject::InnerDoubleQuote => "i\"",
            TextObject::ADoubleQuote => "a\"",
            TextObject::InnerSingleQuote => "i'",
            TextObject::ASingleQuote => "a'",
            TextObject::InnerBacktick => "i`",
            TextObject::ABacktick => "a`",
        }
    }
}

/// An operator ran (Delete, Change or Yank), and the range it covered was
/// exactly these text objects. Worked out by the editor; the text itself
/// is never sent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorReport {
    pub op: Operator,
    /// Unix epoch milliseconds.
    pub at_ms: u64,
    /// Best match first.
    pub objects: Vec<TextObject>,
    /// With `[text] shapes` on: which motions would have covered the range
    /// exactly (one line, charwise). Numbers only.
    #[serde(default)]
    pub range: Option<OperatorRange>,
}

/// A charwise range on one line, seen as an operator's motion: facts about
/// its last character from its first, and `w` presses to just after it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorRange {
    pub cols: i64,
    #[serde(default)]
    pub facts: MotionFacts,
    #[serde(default)]
    pub words: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_parses_with_and_without_facts() {
        let m: Motion = serde_json::from_str(
            r#"{"start_ms":1,"end_ms":900,"moves":12,"mode":"normal","lines":12,"cols":0,
                "facts":{"paragraph_hops":2,"window_lines":40}}"#,
        )
        .unwrap();
        assert_eq!(m.facts.paragraph_hops, Some(2));
        assert!(!m.facts.file_end);
        let bare: Motion = serde_json::from_str(
            r#"{"start_ms":1,"end_ms":2,"moves":1,"mode":"visual","lines":0,"cols":-3}"#,
        )
        .unwrap();
        assert_eq!(bare.facts, MotionFacts::default());
    }

    #[test]
    fn unknown_fields_are_rejected() {
        // A plugin must not be able to slip text in under a new name.
        let extra_fact = r#"{"start_ms":1,"end_ms":2,"moves":1,"mode":"normal","lines":1,"cols":0,
            "facts":{"target_char":"x"}}"#;
        assert!(serde_json::from_str::<Motion>(extra_fact).is_err());
        let extra_field = r#"{"start_ms":1,"end_ms":2,"moves":1,"mode":"normal","lines":1,"cols":0,
            "line_text":"secret"}"#;
        assert!(serde_json::from_str::<Motion>(extra_field).is_err());
    }

    #[test]
    fn editor_modes_map_to_tracker_modes() {
        assert_eq!(EditorMode::Replace.vim_mode(), Some(VimMode::Insert));
        assert_eq!(
            EditorMode::VisualBlock.vim_mode(),
            Some(VimMode::VisualBlock)
        );
        assert_eq!(EditorMode::Other.vim_mode(), None);
    }

    #[test]
    fn operator_reports_use_vim_names_only() {
        let r: OperatorReport =
            serde_json::from_str(r#"{"op":"delete","at_ms":5,"objects":["i(","a\"","iw"]}"#)
                .unwrap();
        assert_eq!(
            r.objects,
            [
                TextObject::InnerParen,
                TextObject::ADoubleQuote,
                TextObject::InnerWord
            ]
        );
        assert_eq!(r.objects[1].keys(), "a\"");
        assert!(
            serde_json::from_str::<OperatorReport>(
                r#"{"op":"delete","at_ms":5,"objects":["secret"]}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<OperatorReport>(
                r#"{"op":"delete","at_ms":5,"objects":[],"text":"x"}"#
            )
            .is_err()
        );
    }
}
