//! The daemon's state and its reaction to every event.
//!
//! `App` is synchronous and takes the current time as input, so the whole
//! pipeline can be tested without sockets or devices.

use std::collections::HashMap;
use std::path::PathBuf;

use flowbuddy_core::config::{self, Config, State};
use flowbuddy_core::context::{ContextKind, VimMode};
use flowbuddy_core::engine::Engine;
use flowbuddy_core::ipc::{
    ClientKind, ClientMsg, Command, CtxEvent, ErrorCode, RuleAction, RuleInfo, ServerMsg, Status,
};
use flowbuddy_core::key::{Key, KeyStroke, Sym};
use flowbuddy_core::policy::CoachingMode;
use flowbuddy_core::rule::RuleEvent;
use flowbuddy_core::vim::{Flavor, Tracker};
use serde_json::json;
use tokio::sync::mpsc;

use crate::buffer::{ClearReason, KeyBuffer};
use crate::ipc_server::{ConnEvent, ConnId};
use crate::resolver::{AppIds, Resolved, Resolver, ShellState};

struct Client {
    kind: Option<ClientKind>,
    tx: mpsc::UnboundedSender<ServerMsg>,
}

pub struct App {
    clients: HashMap<ConnId, Client>,
    mode: CoachingMode,
    /// Unix seconds.
    paused_until: Option<u64>,
    keyboards: usize,
    resolver: Resolver,
    resolved: Resolved,
    tracker: Tracker,
    buffer: KeyBuffer,
    engine: Engine,
    config: Config,
    state: State,
    /// Where to persist `state`; `None` in tests.
    state_path: Option<PathBuf>,
}

/// App ids from the config, falling back to the built-in lists.
pub fn app_ids(config: &Config) -> AppIds {
    let d = AppIds::default();
    AppIds {
        vscode: config.apps.vscode.clone().unwrap_or(d.vscode),
        terminals: config.apps.terminals.clone().unwrap_or(d.terminals),
        vims: config.apps.vims.clone().unwrap_or(d.vims),
    }
}

fn flavor(kind: ContextKind) -> Flavor {
    match kind {
        ContextKind::BashVi => Flavor::Readline,
        _ => Flavor::Vim,
    }
}

/// VS Code shortcuts that move focus away from the editor (palette,
/// sidebar, terminal panel). Until the next Esc we can't know where keys go.
fn is_vscode_ui_shortcut(key: &Key) -> bool {
    let m = key.mods;
    match key.sym {
        Sym::F(1) => true,
        Sym::Tab => m.ctrl,
        Sym::Char(c) if m.ctrl && !m.alt => {
            matches!(c, 'P' | 'p' | 'E' | 'F' | 'G' | 'X' | 'D' | '`' | '~' | 'k')
        }
        _ => false,
    }
}

impl App {
    pub fn new(config: Config, state: State, resolver: Resolver) -> Self {
        let resolved = resolver.resolve();
        let mut engine = Engine::standard();
        engine.apply(&config::rule_settings(&config, &state));
        App {
            clients: HashMap::new(),
            mode: state.mode.unwrap_or(config.coaching.default_mode),
            paused_until: state.paused_until,
            keyboards: 0,
            resolver,
            resolved,
            tracker: Tracker::new(Flavor::Vim),
            buffer: KeyBuffer::default(),
            engine,
            config,
            state,
            state_path: None,
        }
    }

    pub fn with_state_path(mut self, path: PathBuf) -> Self {
        self.state_path = Some(path);
        self
    }

    fn save_state(&mut self) {
        self.state.mode = Some(self.mode);
        self.state.paused_until = self.paused_until;
        if let Some(path) = &self.state_path {
            if let Err(e) = self.state.save(path) {
                tracing::warn!(error = %e, "could not save state");
            }
        }
    }

    /// The config file changed.
    pub fn reload_config(&mut self, config: Config) {
        self.engine
            .apply(&config::rule_settings(&config, &self.state));
        self.resolver.set_ids(app_ids(&config));
        self.config = config;
        tracing::info!("config reloaded");
    }

    /// Findings and adoptions go here: coaching policy and storage hook in
    /// at this point.
    fn on_rule_events(&mut self, events: &[RuleEvent], _now_ms: u64) {
        for ev in events {
            if let RuleEvent::Finding(f) = ev {
                tracing::debug!(rule = f.rule_id, shape = %f.shape, "finding");
            }
        }
    }

    /// Called about once a second: ends streaks that stopped without
    /// another key.
    pub fn tick(&mut self, now_ms: u64) {
        let events = self.engine.tick(self.resolved.kind, now_ms);
        self.on_rule_events(&events, now_ms);
    }

    fn capturing(&self, now_ms: u64) -> bool {
        self.mode != CoachingMode::Off && self.paused_until.is_none_or(|t| t * 1000 <= now_ms)
    }

    /// Feed one key; returns the rule events it caused (already handled).
    pub fn on_key(&mut self, stroke: KeyStroke, now_ms: u64) -> Vec<RuleEvent> {
        if !self.capturing(now_ms) {
            return Vec::new();
        }
        let kind = self.resolved.kind;
        if !self.buffer.push(stroke, kind) {
            return Vec::new();
        }
        if kind == ContextKind::VsCodeVim && is_vscode_ui_shortcut(&stroke.key) {
            self.tracker.invalidate();
            self.engine.reset();
            return Vec::new();
        }
        let step = self.tracker.feed(stroke);
        let events = self.engine.on_step(kind, &step);
        self.on_rule_events(&events, now_ms);
        events
    }

    /// A mouse click may have changed the editor's mode or focus.
    pub fn on_pointer_click(&mut self) {
        self.tracker.invalidate();
        self.engine.reset();
    }

    #[cfg(test)]
    pub fn context(&self) -> Resolved {
        self.resolved
    }

    #[cfg(test)]
    pub fn mode_known(&self) -> (VimMode, bool) {
        (self.tracker.mode(), self.tracker.confident())
    }

    /// Re-resolve the context. `known` is the mode we are sure of if this
    /// update was caused by the context's own shell (vim started, prompt drawn).
    fn update_context(&mut self, known_for: Option<(u32, VimMode)>, now_ms: u64) {
        let new = self.resolver.resolve();
        let known = known_for
            .filter(|(pid, _)| new.shell_pid == Some(*pid))
            .map(|(_, mode)| mode);
        if new != self.resolved {
            tracing::debug!(from = ?self.resolved.kind, to = ?new.kind, "context changed");
            self.buffer.clear(ClearReason::ContextChanged);
            let events = self.engine.context_changed(self.resolved.kind, now_ms);
            self.on_rule_events(&events, now_ms);
            self.resolved = new;
            self.tracker = Tracker::new(flavor(new.kind));
        }
        if let Some(mode) = known {
            self.tracker.reset_known(mode);
        }
    }

    pub fn set_keyboards(&mut self, n: usize) {
        self.keyboards = n;
    }

    pub fn on_conn(&mut self, ev: ConnEvent, now_ms: u64) {
        match ev {
            ConnEvent::Opened { id, tx } => {
                self.clients.insert(id, Client { kind: None, tx });
            }
            ConnEvent::Closed { id } => {
                self.clients.remove(&id);
            }
            ConnEvent::Msg { id, msg } => self.on_msg(id, msg, now_ms),
        }
    }

    fn reply(&self, id: ConnId, msg: ServerMsg) {
        if let Some(c) = self.clients.get(&id) {
            let _ = c.tx.send(msg);
        }
    }

    /// Send to every client that stays connected (the extension).
    fn broadcast(&self, msg: ServerMsg) {
        for c in self.clients.values() {
            if c.kind == Some(ClientKind::GnomeExtension) {
                let _ = c.tx.send(msg.clone());
            }
        }
    }

    fn on_msg(&mut self, conn: ConnId, msg: ClientMsg, now_ms: u64) {
        match msg {
            ClientMsg::Hello { client, .. } => {
                if let Some(c) = self.clients.get_mut(&conn) {
                    c.kind = Some(client);
                }
                self.reply(
                    conn,
                    ServerMsg::Welcome {
                        daemon_version: flowbuddy_core::VERSION.into(),
                        mode: self.mode,
                    },
                );
            }
            ClientMsg::Focus { app_id, pid, .. } => {
                self.resolver.set_focus(app_id, pid);
                self.resolver.prune();
                self.update_context(None, now_ms);
            }
            ClientMsg::Ctx {
                event,
                command,
                vi_mode,
                shell_pid,
                ..
            } => {
                let (state, known) = match event {
                    CtxEvent::Fg => (ShellState::Fg(command.unwrap_or_default()), VimMode::Normal),
                    CtxEvent::Prompt => (
                        ShellState::Prompt {
                            vi: vi_mode.unwrap_or(false),
                        },
                        VimMode::Insert,
                    ),
                };
                self.resolver.shell_report(shell_pid, state, now_ms);
                self.update_context(Some((shell_pid, known)), now_ms);
            }
            ClientMsg::NudgeResponse { .. } => {}
            ClientMsg::Command { id, command } => {
                let reply = self.on_command(id, command, now_ms);
                self.reply(conn, reply);
            }
        }
    }

    fn on_command(&mut self, id: Option<u64>, command: Command, now_ms: u64) -> ServerMsg {
        let now_s = now_ms / 1000;
        match command {
            Command::Status => ServerMsg::Result {
                id,
                data: json!(self.status(now_s)),
            },
            Command::SetMode { mode } => {
                self.mode = mode;
                if mode == CoachingMode::Off {
                    self.buffer.clear(ClearReason::ModeOff);
                }
                self.mode_changed();
                self.save_state();
                ServerMsg::Ack { id }
            }
            Command::Pause { seconds } => {
                self.paused_until = Some(now_s + seconds);
                self.buffer.clear(ClearReason::Paused);
                self.tracker.invalidate();
                self.engine.reset();
                self.mode_changed();
                self.save_state();
                ServerMsg::Ack { id }
            }
            Command::Resume => {
                self.paused_until = None;
                self.mode_changed();
                self.save_state();
                ServerMsg::Ack { id }
            }
            Command::Rules => ServerMsg::Result {
                id,
                data: json!(self.rule_infos()),
            },
            Command::Rule { rule_id, action } => self.rule_command(id, &rule_id, action),
            Command::ReportFalsePositive => ServerMsg::Ack { id },
        }
    }

    fn rule_infos(&self) -> Vec<RuleInfo> {
        let muted = self.state.muted();
        self.engine
            .rules()
            .map(|r| RuleInfo {
                id: r.id().into(),
                description: r.description().into(),
                enabled: self.engine.is_enabled(r.id()),
                muted: muted.contains(r.id()),
                threshold: r.threshold(),
            })
            .collect()
    }

    fn rule_command(&mut self, id: Option<u64>, rule_id: &str, action: RuleAction) -> ServerMsg {
        if !self.engine.has_rule(rule_id) {
            return ServerMsg::Error {
                id,
                code: ErrorCode::UnknownRule,
                message: format!("no rule named {rule_id}"),
            };
        }
        let entry = self.state.rules.entry(rule_id.to_string()).or_default();
        match action {
            RuleAction::Enable => entry.enabled = Some(true),
            RuleAction::Disable => entry.enabled = Some(false),
            RuleAction::Mute => entry.muted = true,
            RuleAction::Unmute => entry.muted = false,
        }
        self.engine
            .apply(&config::rule_settings(&self.config, &self.state));
        self.save_state();
        ServerMsg::Ack { id }
    }

    fn mode_changed(&self) {
        self.broadcast(ServerMsg::ModeChanged {
            mode: self.mode,
            paused_until: self.paused_until,
        });
    }

    pub fn status(&self, now_s: u64) -> Status {
        let mut clients: Vec<ClientKind> = self.clients.values().filter_map(|c| c.kind).collect();
        clients.sort_by_key(|k| *k as u8);
        clients.dedup();
        Status {
            running: true,
            daemon_version: flowbuddy_core::VERSION.into(),
            mode: self.mode,
            paused_until: self.paused_until.filter(|&t| t > now_s),
            context: self.resolved.kind.as_str().into(),
            confident: self.tracker.confident(),
            keyboards: self.keyboards,
            clients,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::resolver::AppIds;
    use flowbuddy_core::vim::notation::strokes;
    use tokio::sync::mpsc::UnboundedReceiver;

    pub(crate) fn test_app(mode: CoachingMode) -> App {
        let state = State {
            mode: Some(mode),
            ..Default::default()
        };
        App::new(Config::default(), state, test_resolver())
    }

    pub(crate) fn test_resolver() -> Resolver {
        Resolver::new(AppIds::default(), crate::resolver::tests::tree())
    }

    pub(crate) fn connect(app: &mut App, id: ConnId) -> UnboundedReceiver<ServerMsg> {
        let (tx, rx) = mpsc::unbounded_channel();
        app.on_conn(ConnEvent::Opened { id, tx }, 0);
        rx
    }

    pub(crate) fn send(app: &mut App, id: ConnId, msg: ClientMsg, now_ms: u64) {
        app.on_conn(ConnEvent::Msg { id, msg }, now_ms);
    }

    fn hello(kind: ClientKind) -> ClientMsg {
        ClientMsg::Hello {
            client: kind,
            client_version: "test".into(),
        }
    }

    #[test]
    fn hello_gets_welcome_with_mode() {
        let mut app = test_app(CoachingMode::Passive);
        let mut rx = connect(&mut app, 1);
        send(&mut app, 1, hello(ClientKind::Cli), 0);
        assert_eq!(
            rx.try_recv().unwrap(),
            ServerMsg::Welcome {
                daemon_version: flowbuddy_core::VERSION.into(),
                mode: CoachingMode::Passive
            }
        );
    }

    #[test]
    fn mode_change_is_broadcast_to_extension_only() {
        let mut app = test_app(CoachingMode::Balanced);
        let mut ext = connect(&mut app, 1);
        let mut cli = connect(&mut app, 2);
        send(&mut app, 1, hello(ClientKind::GnomeExtension), 0);
        send(&mut app, 2, hello(ClientKind::Cli), 0);
        ext.try_recv().unwrap();
        cli.try_recv().unwrap();

        let cmd = ClientMsg::Command {
            id: Some(5),
            command: Command::SetMode {
                mode: CoachingMode::Active,
            },
        };
        send(&mut app, 2, cmd, 0);
        assert_eq!(cli.try_recv().unwrap(), ServerMsg::Ack { id: Some(5) });
        assert!(cli.try_recv().is_err());
        assert_eq!(
            ext.try_recv().unwrap(),
            ServerMsg::ModeChanged {
                mode: CoachingMode::Active,
                paused_until: None
            }
        );
    }

    #[test]
    fn pause_shows_in_status_until_it_expires() {
        let mut app = test_app(CoachingMode::Balanced);
        let _rx = connect(&mut app, 1);
        let pause = ClientMsg::Command {
            id: None,
            command: Command::Pause { seconds: 60 },
        };
        send(&mut app, 1, pause, 1_000_000);
        assert_eq!(app.status(1_000).paused_until, Some(1_060));
        assert_eq!(app.status(1_061).paused_until, None);
    }

    fn focus(app: &mut App, app_id: &str, pid: u32) {
        send(
            app,
            1,
            ClientMsg::Focus {
                app_id: Some(app_id.into()),
                title: None,
                pid: Some(pid),
            },
            0,
        );
    }

    fn ctx(app: &mut App, event: CtxEvent, command: Option<&str>, shell_pid: u32) {
        send(
            app,
            1,
            ClientMsg::Ctx {
                event,
                command: command.map(Into::into),
                vi_mode: Some(true),
                shell_pid,
                tty: None,
            },
            0,
        );
    }

    fn keys(app: &mut App, s: &str) -> Vec<RuleEvent> {
        strokes(s, 0)
            .into_iter()
            .flat_map(|k| app.on_key(k, k.at_ms))
            .collect()
    }

    #[test]
    fn keys_outside_supported_contexts_are_dropped() {
        let mut app = test_app(CoachingMode::Active);
        let _rx = connect(&mut app, 1);
        focus(&mut app, "firefox", 900);
        assert!(keys(&mut app, "hello").is_empty());
        assert!(app.buffer.is_empty());
    }

    #[test]
    fn vim_started_in_focused_terminal_is_known_normal() {
        let mut app = test_app(CoachingMode::Active);
        let _rx = connect(&mut app, 1);
        focus(&mut app, "org.gnome.Terminal", 100);
        ctx(&mut app, CtxEvent::Fg, Some("vim"), 200);
        assert_eq!(app.context().kind, ContextKind::TerminalVim);
        keys(&mut app, "jj");
        assert_eq!(app.mode_known(), (VimMode::Normal, true));
    }

    #[test]
    fn prompt_means_bash_insert_mode() {
        let mut app = test_app(CoachingMode::Active);
        let _rx = connect(&mut app, 1);
        focus(&mut app, "org.gnome.Terminal", 100);
        ctx(&mut app, CtxEvent::Prompt, None, 200);
        assert_eq!(app.context().kind, ContextKind::BashVi);
        assert_eq!(app.mode_known(), (VimMode::Insert, true));
    }

    #[test]
    fn context_change_clears_buffer_and_trust() {
        let mut app = test_app(CoachingMode::Active);
        let _rx = connect(&mut app, 1);
        focus(&mut app, "code_code", 500);
        keys(&mut app, "<Esc>jj");
        assert!(!app.buffer.is_empty());
        focus(&mut app, "firefox", 900);
        assert!(app.buffer.is_empty());
        focus(&mut app, "code_code", 500);
        assert!(!app.mode_known().1, "mode unknown after coming back");
    }

    #[test]
    fn vscode_palette_and_clicks_make_mode_uncertain() {
        let mut app = test_app(CoachingMode::Active);
        let _rx = connect(&mut app, 1);
        focus(&mut app, "code_code", 500);
        keys(&mut app, "<Esc>");
        assert!(app.mode_known().1);
        let palette = KeyStroke {
            key: Key::ctrl('P'),
            repeat: false,
            at_ms: 0,
        };
        assert!(app.on_key(palette, 0).is_empty());
        assert!(!app.mode_known().1);
        keys(&mut app, "<Esc>");
        app.on_pointer_click();
        assert!(!app.mode_known().1);
    }

    #[test]
    fn paused_and_off_capture_nothing() {
        let mut app = test_app(CoachingMode::Active);
        let _rx = connect(&mut app, 1);
        focus(&mut app, "code_code", 500);
        let pause = ClientMsg::Command {
            id: None,
            command: Command::Pause { seconds: 60 },
        };
        send(&mut app, 1, pause, 0);
        keys(&mut app, "<Esc>jj");
        assert!(app.buffer.is_empty(), "paused");
        let off = ClientMsg::Command {
            id: None,
            command: Command::SetMode {
                mode: CoachingMode::Off,
            },
        };
        send(&mut app, 1, off, 0);
        send(
            &mut app,
            1,
            ClientMsg::Command {
                id: None,
                command: Command::Resume,
            },
            0,
        );
        keys(&mut app, "<Esc>jj");
        assert!(app.buffer.is_empty(), "off");
    }

    fn has_finding(events: &[RuleEvent], id: &str) -> bool {
        events
            .iter()
            .any(|e| matches!(e, RuleEvent::Finding(f) if f.rule_id == id))
    }

    #[test]
    fn keys_reach_the_rules() {
        let mut app = test_app(CoachingMode::Active);
        let _rx = connect(&mut app, 1);
        focus(&mut app, "code_code", 500);
        let events = keys(&mut app, "<Esc>jjjjjjjw");
        assert!(has_finding(&events, "vim.repeat-jk"));
    }

    #[test]
    fn rules_can_be_listed_disabled_and_muted() {
        let mut app = test_app(CoachingMode::Active);
        let mut rx = connect(&mut app, 1);
        let rule = |rule_id: &str, action| ClientMsg::Command {
            id: Some(1),
            command: Command::Rule {
                rule_id: rule_id.into(),
                action,
            },
        };
        send(&mut app, 1, rule("vim.repeat-jk", RuleAction::Disable), 0);
        assert_eq!(rx.try_recv().unwrap(), ServerMsg::Ack { id: Some(1) });
        send(&mut app, 1, rule("vim.repeat-x", RuleAction::Mute), 0);
        rx.try_recv().unwrap();

        let list = ClientMsg::Command {
            id: Some(2),
            command: Command::Rules,
        };
        send(&mut app, 1, list, 0);
        let ServerMsg::Result { data, .. } = rx.try_recv().unwrap() else {
            panic!("expected result")
        };
        let infos: Vec<RuleInfo> = serde_json::from_value(data).unwrap();
        let get = |id: &str| infos.iter().find(|r| r.id == id).unwrap();
        assert!(!get("vim.repeat-jk").enabled);
        assert!(get("vim.repeat-x").muted);

        send(&mut app, 1, rule("nope", RuleAction::Mute), 0);
        assert!(matches!(
            rx.try_recv().unwrap(),
            ServerMsg::Error {
                code: ErrorCode::UnknownRule,
                ..
            }
        ));

        focus(&mut app, "code_code", 500);
        let events = keys(&mut app, "<Esc>jjjjjjjw");
        assert!(!has_finding(&events, "vim.repeat-jk"));
    }
}
