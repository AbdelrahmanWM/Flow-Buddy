//! IPC protocol v0: JSON lines over a Unix socket.
//!
//! See docs/architecture.md#ipc-protocol-v0 for the full description and an
//! example of every message.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::editor::{EditorKind, EditorMode, Motion, OperatorReport};
use crate::policy::CoachingMode;

pub const PROTOCOL_VERSION: u32 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClientKind {
    GnomeExtension,
    Cli,
    BashHook,
    /// An editor plugin: the VS Code companion or the Vim/Neovim plugin.
    Editor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CtxEvent {
    /// A command started in the foreground.
    Fg,
    /// The prompt is back.
    Prompt,
}

/// What has focus inside an editor window when the text editor doesn't.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Panel {
    /// The integrated terminal.
    Terminal,
    /// Anything else: a chat, the sidebar, a webview.
    Other,
}

/// The shape of a command line, from the shell hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommandShape {
    /// Exactly `cd ..`.
    CdUp,
    /// `cd ../..` or further up in one go: the better way.
    CdUpMany,
    /// Its last argument is the previous command's long last argument.
    ReusedArg,
    /// The previous command with one word changed.
    OneWordChanged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NudgeAction {
    Seen,
    Dismiss,
    MuteRule,
    FalsePositive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleAction {
    Enable,
    Disable,
    Mute,
    Unmute,
    /// Undo threshold raises made after "not helpful" reports.
    Reset,
}

/// Requests from the CLI or the indicator menu.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "name", content = "args", rename_all = "snake_case")]
pub enum Command {
    Status,
    SetMode {
        mode: CoachingMode,
    },
    Pause {
        seconds: u64,
    },
    Resume,
    Rules,
    Rule {
        rule_id: String,
        action: RuleAction,
    },
    /// Mark the most recent nudge as wrong.
    ReportFalsePositive,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Hello {
        client: ClientKind,
        client_version: String,
        /// Editor clients: which editor, and its process id so the daemon
        /// can match it to the focused window or terminal.
        #[serde(default)]
        editor: Option<EditorKind>,
        #[serde(default)]
        pid: Option<u32>,
    },
    Focus {
        app_id: Option<String>,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        pid: Option<u32>,
    },
    Ctx {
        event: CtxEvent,
        /// Basename of the command only, never its arguments.
        #[serde(default)]
        command: Option<String>,
        #[serde(default)]
        vi_mode: Option<bool>,
        shell_pid: u32,
        #[serde(default)]
        tty: Option<String>,
        /// What kind of command line it was, worked out by the hook from a
        /// fixed list; the command line itself is never sent.
        #[serde(default)]
        shape: Option<CommandShape>,
    },
    NudgeResponse {
        nudge_id: String,
        action: NudgeAction,
    },
    /// A mouse button was pressed or the touchpad tapped. No position, no target.
    PointerClick,
    /// The extension: the keyboard layout now in use, as xkb names (`de`,
    /// `nodeadkeys`); no layout for input methods such as IBus.
    InputSource {
        #[serde(default)]
        layout: Option<String>,
        #[serde(default)]
        variant: Option<String>,
    },
    /// Editor clients: whether a text editor has keyboard focus (as opposed
    /// to a terminal, chat or other panel inside the same window).
    EditorFocus {
        focused: bool,
        /// When not focused: what has focus instead, if known.
        #[serde(default)]
        panel: Option<Panel>,
    },
    /// Editor clients: the editor's real mode.
    EditorMode { mode: EditorMode },
    /// Editor clients: a finished movement and facts about where it ended.
    Motion(Motion),
    /// Editor clients: an operator's range matched these text objects.
    Operator(OperatorReport),
    Command {
        #[serde(default)]
        id: Option<u64>,
        #[serde(flatten)]
        command: Command,
    },
}

impl ClientMsg {
    pub const TYPES: [&'static str; 11] = [
        "hello",
        "focus",
        "ctx",
        "nudge_response",
        "pointer_click",
        "input_source",
        "editor_focus",
        "editor_mode",
        "motion",
        "operator",
        "command",
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BadVersion,
    BadRequest,
    UnknownType,
    UnknownRule,
    Internal,
}

/// How the extension shows a nudge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NudgeStyle {
    /// A short label next to the top-bar icon.
    #[default]
    Bar,
    /// A card at the top centre of the screen that fades out.
    Card,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Nudge {
    pub nudge_id: String,
    pub rule_id: String,
    pub title: String,
    pub text: String,
    pub ttl_ms: u64,
    /// Added after v0 shipped; older extensions ignore it.
    #[serde(default)]
    pub style: NudgeStyle,
    /// What the tip's commands do, one line each, for an optional
    /// "what does it do?" section. Commands in `text` and here are marked
    /// with backticks (see `glossary`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub explain: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Welcome {
        daemon_version: String,
        mode: CoachingMode,
    },
    Ack {
        id: Option<u64>,
    },
    Result {
        id: Option<u64>,
        data: Value,
    },
    Error {
        #[serde(default)]
        id: Option<u64>,
        code: ErrorCode,
        message: String,
    },
    Nudge(Nudge),
    ModeChanged {
        mode: CoachingMode,
        /// Unix seconds.
        paused_until: Option<u64>,
    },
}

/// Data returned by [`Command::Status`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub running: bool,
    pub daemon_version: String,
    pub mode: CoachingMode,
    pub paused_until: Option<u64>,
    /// Context *kind* only, never keys or titles.
    pub context: String,
    /// The focused app's id, e.g. `code_code`. Never its title.
    #[serde(default)]
    pub app: Option<String>,
    pub confident: bool,
    /// Whether the mode comes from an editor plugin rather than a guess.
    #[serde(default)]
    pub mode_from_editor: bool,
    /// The keyboard layout keys are read with, and why, if it isn't the one
    /// in use.
    #[serde(default)]
    pub layout: Option<String>,
    #[serde(default)]
    pub layout_note: Option<String>,
    pub keyboards: usize,
    pub clients: Vec<ClientKind>,
}

/// One rule as listed by [`Command::Rules`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuleInfo {
    pub id: String,
    pub description: String,
    pub enabled: bool,
    pub muted: bool,
    pub threshold: Option<u32>,
    /// How much of `threshold` was added after "not helpful" reports.
    #[serde(default)]
    pub raised: u32,
}

#[derive(Serialize)]
struct EnvelopeOut<'a, T> {
    v: u32,
    #[serde(flatten)]
    msg: &'a T,
}

/// Serialize a message as one protocol line (without the newline).
pub fn encode<T: Serialize>(msg: &T) -> String {
    serde_json::to_string(&EnvelopeOut {
        v: PROTOCOL_VERSION,
        msg,
    })
    .expect("protocol messages always serialize")
}

#[derive(Debug, PartialEq)]
pub struct DecodeError {
    pub code: ErrorCode,
    pub message: String,
    /// The request id, if one could be read.
    pub id: Option<u64>,
}

impl DecodeError {
    fn new(code: ErrorCode, message: impl Into<String>, id: Option<u64>) -> Self {
        DecodeError {
            code,
            message: message.into(),
            id,
        }
    }

    /// Whether the connection must be closed after replying.
    pub fn is_fatal(&self) -> bool {
        self.code == ErrorCode::BadVersion
    }

    pub fn to_server_msg(&self) -> ServerMsg {
        ServerMsg::Error {
            id: self.id,
            code: self.code,
            message: self.message.clone(),
        }
    }
}

/// Parse one line from a client, checking version and type.
pub fn decode_client(line: &str) -> Result<ClientMsg, DecodeError> {
    let value: Value = serde_json::from_str(line)
        .map_err(|e| DecodeError::new(ErrorCode::BadRequest, format!("invalid JSON: {e}"), None))?;
    let id = value.get("id").and_then(Value::as_u64);
    match value.get("v").and_then(Value::as_u64) {
        Some(v) if v == u64::from(PROTOCOL_VERSION) => {}
        Some(v) => {
            return Err(DecodeError::new(
                ErrorCode::BadVersion,
                format!("protocol version {v} not supported (expected {PROTOCOL_VERSION})"),
                id,
            ));
        }
        None => {
            return Err(DecodeError::new(ErrorCode::BadVersion, "missing \"v\"", id));
        }
    }
    let ty = value.get("type").and_then(Value::as_str).unwrap_or("");
    if !ClientMsg::TYPES.contains(&ty) {
        return Err(DecodeError::new(
            ErrorCode::UnknownType,
            format!("unknown message type {ty:?}"),
            id,
        ));
    }
    let mut value = value;
    // The envelope's version isn't part of any message, and messages that
    // reject unknown fields would otherwise trip over it.
    if let Some(map) = value.as_object_mut() {
        map.remove("v");
    }
    serde_json::from_value(value)
        .map_err(|e| DecodeError::new(ErrorCode::BadRequest, e.to_string(), id))
}

/// Parse one line from the daemon.
pub fn decode_server(line: &str) -> Result<ServerMsg, serde_json::Error> {
    serde_json::from_str(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every client example in docs/architecture.md must parse.
    #[test]
    fn documented_client_examples_parse() {
        let examples = [
            r#"{"v":0,"type":"hello","client":"gnome-extension","client_version":"0.1.0"}"#,
            r#"{"v":0,"type":"focus","app_id":"code_code","title":"main.rs - Flow-Buddy - Visual Studio Code","pid":41230}"#,
            r#"{"v":0,"type":"ctx","event":"fg","command":"vim","shell_pid":51002,"tty":"/dev/pts/3"}"#,
            r#"{"v":0,"type":"ctx","event":"fg","command":"cd","shell_pid":51002,"shape":"cd-up"}"#,
            r#"{"v":0,"type":"ctx","event":"prompt","vi_mode":true,"shell_pid":51002,"tty":"/dev/pts/3"}"#,
            r#"{"v":0,"type":"nudge_response","nudge_id":"n-1729740000-3","action":"dismiss"}"#,
            r#"{"v":0,"type":"pointer_click"}"#,
            r#"{"v":0,"type":"input_source","layout":"de","variant":"nodeadkeys"}"#,
            r#"{"v":0,"type":"hello","client":"editor","client_version":"0.1.0","editor":"vim","pid":61234}"#,
            r#"{"v":0,"type":"editor_focus","focused":false}"#,
            r#"{"v":0,"type":"editor_focus","focused":false,"panel":"terminal"}"#,
            r#"{"v":0,"type":"operator","op":"delete","at_ms":1729740000500,"objects":["i(","a("]}"#,
            r#"{"v":0,"type":"editor_mode","mode":"visual_line"}"#,
            r#"{"v":0,"type":"motion","start_ms":1729740000000,"end_ms":1729740000900,"moves":12,"mode":"normal","lines":12,"cols":0,"facts":{"paragraph_hops":2,"window_lines":40}}"#,
            r#"{"v":0,"type":"command","id":7,"name":"status"}"#,
            r#"{"v":0,"type":"command","id":8,"name":"set_mode","args":{"mode":"active"}}"#,
            r#"{"v":0,"type":"command","id":9,"name":"pause","args":{"seconds":3600}}"#,
            r#"{"v":0,"type":"command","id":10,"name":"resume"}"#,
            r#"{"v":0,"type":"command","id":11,"name":"rule","args":{"rule_id":"vim.repeat-jk","action":"disable"}}"#,
        ];
        for line in examples {
            decode_client(line).unwrap_or_else(|e| panic!("{line}: {e:?}"));
        }
    }

    #[test]
    fn vim_plugin_motion_parses() {
        // Captured from vim-plugin/ sending to a test socket.
        let line = r#"{"start_ms":1792969179805,"moves":1,"lines":6,"facts":{"relative_numbers":false,"file_start":false,"file_end":false,"jump_back":false,"view_scrolled":0,"window_lines":23,"paragraph_hops":2},"v":0,"mode":"normal","type":"motion","end_ms":1792969179805,"cols":0}"#;
        match decode_client(line).unwrap() {
            ClientMsg::Motion(m) => assert_eq!(m.facts.paragraph_hops, Some(2)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn motion_with_a_text_field_is_rejected() {
        let line = r#"{"v":0,"type":"motion","start_ms":1,"end_ms":2,"moves":1,"mode":"normal","lines":1,"cols":0,"text":"secret"}"#;
        assert_eq!(decode_client(line).unwrap_err().code, ErrorCode::BadRequest);
    }

    #[test]
    fn documented_server_examples_parse() {
        let examples = [
            r#"{"v":0,"type":"welcome","daemon_version":"0.1.0","mode":"balanced"}"#,
            r#"{"v":0,"type":"ack","id":8}"#,
            r#"{"v":0,"type":"result","id":7,"data":{"running":true}}"#,
            r#"{"v":0,"type":"error","id":11,"code":"unknown_rule","message":"no rule named vim.repeat-jk"}"#,
            r#"{"v":0,"type":"nudge","nudge_id":"n-1729740000-3","rule_id":"vim.repeat-jk","title":"Try a count","text":"You pressed j 7 times. Try 7j, } or Ctrl-d.","ttl_ms":6000}"#,
            r#"{"v":0,"type":"nudge","nudge_id":"n-1729740000-4","rule_id":"vim.repeat-x","title":"Delete by word","text":"x","ttl_ms":6000,"style":"card"}"#,
            r#"{"v":0,"type":"mode_changed","mode":"active","paused_until":null}"#,
        ];
        for line in examples {
            decode_server(line).unwrap_or_else(|e| panic!("{line}: {e}"));
        }
    }

    #[test]
    fn command_round_trip() {
        let msg = ClientMsg::Command {
            id: Some(8),
            command: Command::SetMode {
                mode: CoachingMode::Active,
            },
        };
        let line = encode(&msg);
        assert_eq!(
            line,
            r#"{"v":0,"type":"command","id":8,"name":"set_mode","args":{"mode":"active"}}"#
        );
        assert_eq!(decode_client(&line).unwrap(), msg);
    }

    #[test]
    fn version_and_type_errors() {
        let e = decode_client(r#"{"v":9,"type":"hello"}"#).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadVersion);
        assert!(e.is_fatal());

        let e = decode_client(r#"{"v":0,"type":"teleport","id":3}"#).unwrap_err();
        assert_eq!(e.code, ErrorCode::UnknownType);
        assert_eq!(e.id, Some(3));
        assert!(!e.is_fatal());

        let e = decode_client(r#"{"v":0,"type":"ctx","event":"fg"}"#).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest);
    }
}
