//! IPC protocol v0: JSON lines over a Unix socket.
//!
//! See docs/architecture.md#ipc-protocol-v0 for the full description and an
//! example of every message.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::policy::CoachingMode;

pub const PROTOCOL_VERSION: u32 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClientKind {
    GnomeExtension,
    Cli,
    BashHook,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CtxEvent {
    /// A command started in the foreground.
    Fg,
    /// The prompt is back.
    Prompt,
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
    },
    NudgeResponse {
        nudge_id: String,
        action: NudgeAction,
    },
    Command {
        #[serde(default)]
        id: Option<u64>,
        #[serde(flatten)]
        command: Command,
    },
}

impl ClientMsg {
    pub const TYPES: [&'static str; 5] = ["hello", "focus", "ctx", "nudge_response", "command"];
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
            r#"{"v":0,"type":"ctx","event":"prompt","vi_mode":true,"shell_pid":51002,"tty":"/dev/pts/3"}"#,
            r#"{"v":0,"type":"nudge_response","nudge_id":"n-1729740000-3","action":"dismiss"}"#,
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
