//! The daemon's state and its reaction to every event.
//!
//! `App` is synchronous and takes the current time as input, so the whole
//! pipeline can be tested without sockets or devices.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use flowbuddy_core::config::{self, Config, State};
use flowbuddy_core::context::{ContextKind, VimMode};
use flowbuddy_core::editor::{EditorKind, Motion};
use flowbuddy_core::engine::Engine;
use flowbuddy_core::glossary;
use flowbuddy_core::ipc::{
    ClientKind, ClientMsg, Command, CtxEvent, ErrorCode, Nudge, NudgeAction, NudgeStyle,
    RuleAction, RuleInfo, ServerMsg, Status,
};
use flowbuddy_core::key::{Key, KeyStroke, Sym};
use flowbuddy_core::motion::{self, MotionEvent, OperatorEvent};
use flowbuddy_core::policy::{Coach, CoachingMode, Gate, PolicyConfig, RuleFacts};
use flowbuddy_core::rule::{Finding, RuleEvent};
use flowbuddy_core::vim::{Flavor, Tracker, VimEvent};
use serde_json::json;
use tokio::sync::mpsc;

use crate::buffer::{ClearReason, KeyBuffer};
use crate::ipc_server::{ConnEvent, ConnId};
use crate::memory::{self, Memory};
use crate::notify::Notifier;
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
    coach: Coach,
    notifier: Box<dyn Notifier>,
    /// Recent nudges (id, rule), newest last, for responses and reports.
    nudges: VecDeque<(String, &'static str)>,
    nudge_seq: u64,
    memory: Memory,
    /// Recent commands, just enough to count the keys behind a movement or
    /// an operator an editor reports. No keys are kept.
    commands: VecDeque<Cmd>,
    claude_vim: bool,
    claude_checked_ms: Option<u64>,
    /// Reads Claude Code's settings; replaced in tests.
    claude_probe: fn() -> bool,
    /// The layout the extension reports, and whether the keymap should
    /// follow it (main rebuilds it).
    layout: Option<(String, String)>,
    layout_changed: bool,
    /// What main settled on: the layout used, and a note if it differs.
    keymap_layout: (String, Option<String>),
}

const NUDGE_HISTORY: usize = 20;

/// One command in the history used to count keys.
#[derive(Clone, Copy)]
struct Cmd {
    at_ms: u64,
    keys: u32,
    kind: CmdKind,
}

#[derive(Clone, Copy, PartialEq)]
enum CmdKind {
    Motion,
    /// d, c or y with its motion or text object.
    Operator,
    Other,
}

/// The most a threshold is raised after "not helpful" reports.
const MAX_RAISE: u32 = 3;

/// How often to look at Claude Code's settings for its Vim mode.
const CLAUDE_CHECK_MS: u64 = 5000;

/// Commands remembered for counting the keys behind a movement.
const COMMAND_HISTORY: usize = 256;

/// Editors notice the first move a little after its key; count keys this
/// long before a movement's reported start, and this long after its end.
const MOTION_LEAD_MS: u64 = 1500;
const MOTION_LAG_MS: u64 = 200;

/// How recent VS Code editor activity must be for keys to count as typed
/// into the editor.
const EDITOR_ACTIVITY_MS: u64 = 2000;

/// App ids from the config, falling back to the built-in lists.
pub fn app_ids(config: &Config) -> AppIds {
    let d = AppIds::default();
    AppIds {
        vscode: config.apps.vscode.clone().unwrap_or(d.vscode),
        terminals: config.apps.terminals.clone().unwrap_or(d.terminals),
        vims: config.apps.vims.clone().unwrap_or(d.vims),
        claudes: config.apps.claudes.clone().unwrap_or(d.claudes),
    }
}

fn flavor(kind: ContextKind) -> Flavor {
    match kind {
        // Like readline vi-mode: Insert to start with, a small Normal mode.
        ContextKind::BashVi | ContextKind::ClaudeVim => Flavor::Readline,
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
    pub fn new(
        config: Config,
        state: State,
        resolver: Resolver,
        notifier: Box<dyn Notifier>,
    ) -> Self {
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
            coach: Coach::new(PolicyConfig::from(&config.coaching)),
            config,
            state,
            state_path: None,
            notifier,
            nudges: VecDeque::new(),
            nudge_seq: 0,
            memory: Memory::new(None, &Config::default()),
            commands: VecDeque::new(),
            claude_vim: false,
            claude_checked_ms: None,
            claude_probe: crate::claude::vim_mode_on,
            layout: None,
            layout_changed: false,
            keymap_layout: ("us".into(), None),
        }
        .with_memory_config()
    }

    fn with_memory_config(mut self) -> Self {
        self.memory.set_config(&self.config);
        self
    }

    /// Record stats and the mistake memory in `store`.
    pub fn with_store(mut self, store: flowbuddy_store::Store) -> Self {
        self.memory = Memory::new(Some(store), &self.config);
        self
    }

    fn gate(&self, now_ms: u64) -> Gate {
        use chrono::Timelike;
        let local = chrono::Local::now();
        Gate {
            mode: self.mode,
            paused: !self.capturing(now_ms) && self.mode != CoachingMode::Off,
            local_minute: local.hour() * 60 + local.minute(),
        }
    }

    fn describe(&self, rule_id: &str) -> String {
        self.engine
            .rules()
            .find(|r| r.id() == rule_id)
            .map_or_else(|| rule_id.to_string(), |r| r.description().to_string())
    }

    /// Passive mode: one notification about the day, at `summary_time`.
    fn end_of_day(&mut self, now_ms: u64) {
        use chrono::{TimeZone, Timelike};
        if self.mode != CoachingMode::Passive {
            return;
        }
        let Some(target) = config::parse_hm(&self.config.coaching.summary_time) else {
            return;
        };
        let Some(local) = chrono::Local.timestamp_millis_opt(now_ms as i64).single() else {
            return;
        };
        let today = memory::day(now_ms);
        if local.hour() * 60 + local.minute() < target
            || self.state.last_summary_day.as_deref() == Some(today.as_str())
        {
            return;
        }
        self.state.last_summary_day = Some(today);
        self.save_state();
        let describe = |id: &str| self.describe(id).to_lowercase();
        let summary = self.memory.today_summary(now_ms, &describe);
        if let Some(text) = summary {
            self.notifier.notify("Flow Buddy: your day", &text, 15_000);
        }
    }

    /// Every mode but off: one note a week about progress, at `summary_time`.
    fn weekly_note(&mut self, now_ms: u64) {
        use chrono::{TimeZone, Timelike};
        if self.mode == CoachingMode::Off || !self.capturing(now_ms) {
            return;
        }
        let today = memory::day(now_ms);
        let Some(last) = self.state.last_weekly_day.clone() else {
            // The first week starts now.
            self.state.last_weekly_day = Some(today);
            self.save_state();
            return;
        };
        let week_ago = flowbuddy_store::days::range(now_ms, 1, 7).0;
        let Some(target) = config::parse_hm(&self.config.coaching.summary_time) else {
            return;
        };
        let Some(local) = chrono::Local.timestamp_millis_opt(now_ms as i64).single() else {
            return;
        };
        let minute = local.hour() * 60 + local.minute();
        let quiet = self
            .config
            .coaching
            .quiet_hours
            .as_deref()
            .and_then(config::parse_range)
            .is_some_and(|q| q.contains(minute));
        if last > week_ago || minute < target || quiet {
            return;
        }
        self.state.last_weekly_day = Some(today);
        self.save_state();
        // The model writes it, outside this sandbox; the fixed note if
        // that can't start.
        let ai = &self.config.ai;
        if ai.weekly && ai.command.is_some() && self.notifier.start_weekly_note() {
            return;
        }
        let describe = |id: &str| self.describe(id).to_lowercase();
        if let Some(text) = self.memory.week_summary(now_ms, &describe) {
            self.notifier
                .notify("Flow Buddy: your week", &glossary::plain(&text), 20_000);
        }
    }

    fn extension_connected(&self) -> bool {
        self.clients
            .values()
            .any(|c| c.kind == Some(ClientKind::GnomeExtension))
    }

    /// Show a nudge: in the top bar if the extension is there, otherwise
    /// as a desktop notification.
    fn deliver(&mut self, f: &Finding, now_ms: u64) {
        self.nudge_seq += 1;
        let nudge = Nudge {
            nudge_id: format!("n-{now_ms}-{}", self.nudge_seq),
            rule_id: f.rule_id.into(),
            title: f.title.into(),
            text: f.text.clone(),
            ttl_ms: u64::from(self.config.coaching.nudge_seconds) * 1000,
            style: self.config.coaching.nudge_style.unwrap_or(match self.mode {
                CoachingMode::Active => NudgeStyle::Card,
                _ => NudgeStyle::Bar,
            }),
            explain: glossary::explain(&f.text, f.mode == VimMode::Insert),
        };
        if self.extension_connected() {
            self.broadcast(ServerMsg::Nudge(nudge.clone()));
        } else {
            // Plain notifications show no markup: drop the markers and put
            // the explanation underneath.
            let mut body = glossary::plain(&nudge.text);
            for line in &nudge.explain {
                body.push('\n');
                body.push_str(&glossary::plain(line));
            }
            self.notifier.notify(&nudge.title, &body, nudge.ttl_ms);
        }
        if self.nudges.len() == NUDGE_HISTORY {
            self.nudges.pop_front();
        }
        self.nudges.push_back((nudge.nudge_id, f.rule_id));
        self.memory.nudged(f.rule_id, now_ms);
    }

    fn nudge_rule(&self, nudge_id: &str) -> Option<&'static str> {
        self.nudges
            .iter()
            .find(|(id, _)| id == nudge_id)
            .map(|(_, rule)| *rule)
    }

    fn on_nudge_response(&mut self, nudge_id: &str, action: NudgeAction, now_ms: u64) {
        let Some(rule_id) = self.nudge_rule(nudge_id) else {
            return;
        };
        self.memory.response(rule_id, action, now_ms);
        if action == NudgeAction::MuteRule {
            self.set_rule(rule_id, RuleAction::Mute);
        }
        if action == NudgeAction::FalsePositive {
            self.raise_threshold(rule_id, now_ms);
        }
    }

    /// Every two "not helpful" reports in two weeks raise a rule's threshold
    /// by one, up to [`MAX_RAISE`] above its default. Never when the config
    /// sets a threshold: that's the user's call.
    fn raise_threshold(&mut self, rule_id: &'static str, now_ms: u64) {
        if self
            .config
            .rules
            .get(rule_id)
            .is_some_and(|r| r.threshold.is_some())
            || self.engine.default_threshold(rule_id).is_none()
        {
            return;
        }
        let reports = self.memory.recent_false_positives(rule_id, now_ms);
        let entry = self.state.rules.entry(rule_id.to_string()).or_default();
        if entry.raised < MAX_RAISE && reports >= 2 * u64::from(entry.raised + 1) {
            entry.raised += 1;
            tracing::info!(
                rule = rule_id,
                raised = entry.raised,
                "threshold raised after not-helpful reports"
            );
            self.engine
                .apply(&config::rule_settings(&self.config, &self.state));
            self.save_state();
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
        self.coach.set_config(PolicyConfig::from(&config.coaching));
        self.memory.set_config(&config);
        self.config = config;
        tracing::info!("config reloaded");
    }

    /// Findings and adoptions go here: coaching policy and storage hook in
    /// at this point.
    fn on_rule_events(&mut self, events: &[RuleEvent], now_ms: u64) {
        self.on_rule_events_in(self.resolved.kind, events, now_ms);
    }

    /// Rule events from `ctx`, which is the focused context except for the
    /// shell pack's (see [`ContextKind::Shell`]).
    fn on_rule_events_in(&mut self, ctx: ContextKind, events: &[RuleEvent], now_ms: u64) {
        let gate = self.gate(now_ms);
        let muted = self.state.muted();
        let keyed = ctx != ContextKind::Shell;
        for ev in events {
            match ev {
                RuleEvent::Finding(f) if keyed && !self.editor_saw(f.at_ms) => {
                    tracing::debug!(
                        rule = f.rule_id,
                        "finding dropped: keys didn't reach the editor"
                    );
                }
                RuleEvent::Adoption { at_ms, .. } if keyed && !self.editor_saw(*at_ms) => {}
                RuleEvent::Finding(f) => {
                    tracing::debug!(rule = f.rule_id, shape = %f.shape, "finding");
                    self.memory.hit(f, ctx, now_ms);
                    let facts = RuleFacts {
                        muted: muted.contains(f.rule_id),
                        stage: self.memory.stage(f.rule_id, ctx),
                        focus: self.memory.is_focus(f.rule_id),
                        check_in: self.memory.check_in_due(f.rule_id, ctx),
                    };
                    if let Some(f) = self.coach.on_finding(f, facts, &gate, now_ms) {
                        self.deliver(&f, now_ms);
                    }
                }
                RuleEvent::Adoption { rule_id, .. } => self.memory.adoption(rule_id, ctx, now_ms),
            }
        }
    }

    /// Keys pressed for a movement: the motion commands just before its end,
    /// back to its start or the last command that wasn't a motion.
    fn keys_for(&self, m: &Motion) -> u32 {
        let from = m.start_ms.saturating_sub(MOTION_LEAD_MS);
        let to = m.end_ms + MOTION_LAG_MS;
        self.commands
            .iter()
            .rev()
            .skip_while(|c| c.at_ms > to)
            .take_while(|c| c.at_ms >= from && c.kind == CmdKind::Motion)
            .map(|c| c.keys)
            .sum()
    }

    /// Keys pressed for an operator the editor reported: the operator
    /// command itself, plus the motions just before it that positioned the
    /// cursor (a text object works from anywhere inside, so it wouldn't
    /// need them). 0 if the operator isn't in the history.
    fn keys_for_operator(&self, at_ms: u64) -> u32 {
        let mut recent = self
            .commands
            .iter()
            .rev()
            .skip_while(|c| c.at_ms > at_ms + MOTION_LAG_MS);
        let Some(op) = recent.next().filter(|c| {
            c.kind == CmdKind::Operator && at_ms.saturating_sub(c.at_ms) <= MOTION_LEAD_MS
        }) else {
            return 0;
        };
        let mut keys = op.keys;
        let mut next_at = op.at_ms;
        for c in recent {
            if c.kind != CmdKind::Motion || next_at.saturating_sub(c.at_ms) > MOTION_LEAD_MS {
                break;
            }
            keys += c.keys;
            next_at = c.at_ms;
        }
        keys
    }

    /// Whether keys up to `at_ms` plausibly went to the editor. The VS Code
    /// companion reports editor activity as a heartbeat; keys typed into the
    /// terminal or a chat panel come with none.
    fn editor_saw(&self, at_ms: u64) -> bool {
        match self.resolved.editor.and_then(|id| self.resolver.editor(id)) {
            Some(e) if e.kind == EditorKind::Vscode => {
                e.focused == Some(true) && e.focused_ms + EDITOR_ACTIVITY_MS >= at_ms
            }
            _ => true,
        }
    }

    fn ids_claude(&self, command: &str) -> bool {
        app_ids(&self.config).claudes.iter().any(|c| c == command)
    }

    /// Claude Code's settings can change at any time; look every few seconds.
    fn check_claude(&mut self, now_ms: u64) {
        if self
            .claude_checked_ms
            .is_some_and(|t| now_ms.saturating_sub(t) < CLAUDE_CHECK_MS)
        {
            return;
        }
        self.claude_checked_ms = Some(now_ms);
        let on = (self.claude_probe)();
        if on != self.claude_vim {
            self.claude_vim = on;
            self.resolver.set_claude_vim(on);
            self.update_context(None, now_ms);
        }
    }

    /// Called about once a second: ends streaks that stopped without
    /// another key.
    pub fn tick(&mut self, now_ms: u64) {
        self.check_claude(now_ms);
        // Switching tmux panes tells no one; look again.
        if self.resolver.via_tmux() {
            self.update_context(None, now_ms);
        }
        if self.paused_until.is_some_and(|t| t * 1000 <= now_ms) {
            self.paused_until = None;
            self.mode_changed();
            self.save_state();
        }
        let events = self.engine.tick(self.resolved.kind, now_ms);
        self.on_rule_events(&events, now_ms);
        let gate = self.gate(now_ms);
        for f in self.coach.on_tick(&gate, now_ms) {
            self.deliver(&f, now_ms);
        }
        self.memory.maintain(now_ms);
        self.end_of_day(now_ms);
        self.weekly_note(now_ms);
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
        self.memory.active(kind, now_ms);
        let step = self.tracker.feed(stroke);
        if step.confident
            && let Some(VimEvent::Command(c)) = &step.event
        {
            if self.commands.len() == COMMAND_HISTORY {
                self.commands.pop_front();
            }
            let kind = if motion::is_motion(c) {
                CmdKind::Motion
            } else if c.keys.len() >= 2 && c.head().is_some_and(|h| matches!(h, 'd' | 'c' | 'y')) {
                CmdKind::Operator
            } else {
                CmdKind::Other
            };
            self.commands.push_back(Cmd {
                at_ms: stroke.at_ms,
                keys: motion::command_keys(c),
                kind,
            });
        }
        let events = self.engine.on_step(kind, &step);
        self.on_rule_events(&events, now_ms);
        events
    }

    /// A mouse click may have changed the editor's mode or focus.
    pub fn on_pointer_click(&mut self) {
        self.tracker.invalidate();
        self.engine.reset();
        // An editor that reports its mode still knows it.
        self.apply_editor_mode();
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
        let changed = new != self.resolved;
        if changed {
            tracing::debug!(from = ?self.resolved.kind, to = ?new.kind, "context changed");
            self.buffer.clear(ClearReason::ContextChanged);
            let events = self.engine.context_changed(self.resolved.kind, now_ms);
            self.on_rule_events(&events, now_ms);
            self.resolved = new;
            self.tracker = Tracker::new(flavor(new.kind));
            self.commands.clear();
            self.engine.set_editor_motions(new.editor.is_some());
        }
        if let Some(mode) = known {
            self.tracker.reset_known(mode);
        }
        if changed {
            self.apply_editor_mode();
        }
    }

    /// Use the mode the context's editor reported, if any, over our guess.
    fn apply_editor_mode(&mut self) {
        let mode = self
            .resolved
            .editor
            .and_then(|id| self.resolver.editor(id)?.mode);
        if let Some(mode) = mode {
            self.tracker.reset_known(mode);
        }
    }

    /// The layout to build the keymap for, once each time it changes.
    pub fn take_layout(&mut self) -> Option<(String, String)> {
        std::mem::take(&mut self.layout_changed)
            .then(|| self.layout.clone())
            .flatten()
    }

    /// The layout main built the keymap for, and a note if it isn't the one
    /// in use.
    pub fn set_keymap_layout(&mut self, layout: String, note: Option<String>) {
        self.keymap_layout = (layout, note);
        self.tracker.invalidate();
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
                if self.resolver.editor(id).is_some() {
                    self.resolver.editor_gone(id);
                    self.update_context(None, now_ms);
                }
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
            ClientMsg::Hello {
                client,
                editor,
                pid,
                ..
            } => {
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
                if let (ClientKind::Editor, Some(editor)) = (client, editor) {
                    self.resolver.editor_hello(conn, editor, pid, now_ms);
                    self.update_context(None, now_ms);
                }
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
                shape,
                tty,
            } => {
                if let Some(shape) = shape.filter(|_| self.capturing(now_ms)) {
                    let ev = flowbuddy_core::rules::shell::ShellEvent {
                        shape,
                        at_ms: now_ms,
                    };
                    let events = self.engine.on_shell(&ev);
                    self.on_rule_events_in(ContextKind::Shell, &events, now_ms);
                }
                let (state, known) = match event {
                    // Vim starts in Normal mode; Claude Code's prompt in Insert.
                    CtxEvent::Fg => {
                        let command = command.unwrap_or_default();
                        let known = if self.ids_claude(&command) {
                            VimMode::Insert
                        } else {
                            VimMode::Normal
                        };
                        (ShellState::Fg(command), known)
                    }
                    CtxEvent::Prompt => (
                        ShellState::Prompt {
                            vi: vi_mode.unwrap_or(false),
                        },
                        VimMode::Insert,
                    ),
                };
                self.resolver.shell_report(shell_pid, state, tty, now_ms);
                self.update_context(Some((shell_pid, known)), now_ms);
            }
            ClientMsg::NudgeResponse { nudge_id, action } => {
                self.on_nudge_response(&nudge_id, action, now_ms);
            }
            ClientMsg::PointerClick => self.on_pointer_click(),
            ClientMsg::InputSource { layout, variant } => {
                // xkb names only: anything else is ignored.
                let name = |s: Option<String>| {
                    s.filter(|s| {
                        s.len() <= 32
                            && s.chars()
                                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                    })
                };
                let new = name(layout).map(|l| (l, name(variant).unwrap_or_default()));
                if new.is_some() && new != self.layout {
                    self.layout = new;
                    self.layout_changed = true;
                }
            }
            ClientMsg::EditorFocus { focused, panel } => {
                self.resolver.editor_focus(conn, focused, panel, now_ms);
                self.update_context(None, now_ms);
            }
            ClientMsg::EditorMode { mode } => {
                self.resolver.editor_mode(conn, mode.vim_mode(), now_ms);
                if self.resolved.editor == Some(conn) {
                    self.apply_editor_mode();
                } else {
                    // A Vim plugin that just started may make itself the context.
                    self.update_context(None, now_ms);
                }
            }
            ClientMsg::Motion(m) if self.resolved.editor == Some(conn) => {
                let ev = MotionEvent {
                    keys_used: self.keys_for(&m),
                    motion: m,
                };
                let events = self.engine.on_motion(self.resolved.kind, &ev);
                self.on_rule_events(&events, now_ms);
            }
            // From an editor that isn't the current context.
            ClientMsg::Motion(_) => {}
            ClientMsg::Operator(report) if self.resolved.editor == Some(conn) => {
                let ev = OperatorEvent {
                    keys_used: self.keys_for_operator(report.at_ms),
                    report,
                };
                let events = self.engine.on_operator(self.resolved.kind, &ev);
                self.on_rule_events(&events, now_ms);
            }
            ClientMsg::Operator(_) => {}
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
            Command::ReportFalsePositive => match self.nudges.back() {
                Some((nudge_id, _)) => {
                    let nudge_id = nudge_id.clone();
                    self.on_nudge_response(&nudge_id, NudgeAction::FalsePositive, now_ms);
                    ServerMsg::Ack { id }
                }
                None => ServerMsg::Error {
                    id,
                    code: ErrorCode::BadRequest,
                    message: "no recent nudge to report".into(),
                },
            },
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
                raised: self.state.rules.get(r.id()).map_or(0, |s| s.raised),
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
        self.set_rule(rule_id, action);
        ServerMsg::Ack { id }
    }

    fn set_rule(&mut self, rule_id: &str, action: RuleAction) {
        let entry = self.state.rules.entry(rule_id.to_string()).or_default();
        match action {
            RuleAction::Enable => entry.enabled = Some(true),
            RuleAction::Disable => entry.enabled = Some(false),
            RuleAction::Mute => entry.muted = true,
            RuleAction::Unmute => entry.muted = false,
            RuleAction::Reset => entry.raised = 0,
        }
        self.engine
            .apply(&config::rule_settings(&self.config, &self.state));
        self.save_state();
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
            app: self.resolver.focused_app().map(Into::into),
            layout: Some(self.keymap_layout.0.clone()),
            layout_note: self.keymap_layout.1.clone(),
            mode_from_editor: self
                .resolved
                .editor
                .and_then(|id| self.resolver.editor(id)?.mode)
                .is_some(),
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
    use flowbuddy_core::editor::EditorMode;
    use flowbuddy_core::learning::Stage;
    use flowbuddy_core::vim::notation::strokes;
    use tokio::sync::mpsc::UnboundedReceiver;

    pub(crate) fn test_app(mode: CoachingMode) -> App {
        let state = State {
            mode: Some(mode),
            ..Default::default()
        };
        let mut app = App::new(
            Config::default(),
            state,
            test_resolver(),
            Box::new(crate::notify::fake::Recorder::default()),
        );
        app.claude_probe = || false;
        app
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
            editor: None,
            pid: None,
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
        app.tick(1_061_000);
        assert_eq!(app.paused_until, None, "expired pause is cleared");
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
                shape: None,
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
    fn clicks_reported_by_the_extension_make_mode_uncertain() {
        let mut app = test_app(CoachingMode::Active);
        let _rx = connect(&mut app, 1);
        focus(&mut app, "code_code", 500);
        keys(&mut app, "<Esc>");
        assert!(app.mode_known().1);
        send(&mut app, 1, ClientMsg::PointerClick, 0);
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

    fn ext_hello(app: &mut App, id: ConnId) -> UnboundedReceiver<ServerMsg> {
        let mut rx = connect(app, id);
        send(app, id, hello(ClientKind::GnomeExtension), 0);
        rx.try_recv().unwrap();
        rx
    }

    fn nudges(rx: &mut UnboundedReceiver<ServerMsg>) -> Vec<Nudge> {
        std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(|m| match m {
                ServerMsg::Nudge(n) => Some(n),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn active_mode_nudges_the_extension() {
        let mut app = test_app(CoachingMode::Active);
        let mut ext = ext_hello(&mut app, 1);
        focus(&mut app, "code_code", 500);
        keys(&mut app, "<Esc>jjjjjjjw");
        let n = nudges(&mut ext);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].rule_id, "vim.repeat-jk");
        assert!(n[0].text.contains("7j"));
        assert_eq!(n[0].style, NudgeStyle::Card);
    }

    #[test]
    fn nudge_style_can_be_configured() {
        let mut app = test_app(CoachingMode::Active);
        app.config.coaching.nudge_style = Some(NudgeStyle::Bar);
        let mut ext = ext_hello(&mut app, 1);
        focus(&mut app, "code_code", 500);
        keys(&mut app, "<Esc>jjjjjjjw");
        assert_eq!(nudges(&mut ext)[0].style, NudgeStyle::Bar);
    }

    #[test]
    fn without_extension_nudges_become_notifications() {
        let recorder = crate::notify::fake::Recorder::default();
        let state = State {
            mode: Some(CoachingMode::Active),
            ..Default::default()
        };
        let mut app = App::new(
            Config::default(),
            state,
            test_resolver(),
            Box::new(recorder.clone()),
        );
        app.claude_probe = || false;
        let _cli = connect(&mut app, 1);
        focus(&mut app, "code_code", 500);
        keys(&mut app, "<Esc>xxxxxw");
        assert_eq!(recorder.0.borrow().len(), 1);
        assert_eq!(recorder.0.borrow()[0].0, "Delete by word");
        // No markers in a plain notification, and the commands explained.
        let body = &recorder.0.borrow()[0].1;
        assert!(!body.contains('`'), "{body}");
        assert!(body.contains("\nd: delete\nw: "), "{body}");
    }

    #[test]
    fn passive_mode_never_nudges() {
        let mut app = test_app(CoachingMode::Passive);
        let mut ext = ext_hello(&mut app, 1);
        focus(&mut app, "code_code", 500);
        keys(&mut app, "<Esc>jjjjjjjw");
        app.tick(2 * 60 * 60 * 1000);
        assert!(nudges(&mut ext).is_empty());
    }

    #[test]
    fn balanced_mode_sends_a_batch_later() {
        let mut app = test_app(CoachingMode::Balanced);
        let mut ext = ext_hello(&mut app, 1);
        focus(&mut app, "code_code", 500);
        app.tick(0);
        keys(&mut app, "<Esc>jjjjjjjw");
        assert!(nudges(&mut ext).is_empty(), "nothing in the moment");
        app.tick(31 * 60 * 1000);
        assert_eq!(nudges(&mut ext).len(), 1);
    }

    #[test]
    fn mute_from_the_indicator_stops_that_rule() {
        let mut app = test_app(CoachingMode::Active);
        let mut ext = ext_hello(&mut app, 1);
        focus(&mut app, "code_code", 500);
        keys(&mut app, "<Esc>jjjjjjjw");
        let n = nudges(&mut ext).remove(0);
        send(
            &mut app,
            1,
            ClientMsg::NudgeResponse {
                nudge_id: n.nudge_id,
                action: NudgeAction::MuteRule,
            },
            0,
        );
        assert!(app.state.muted().contains("vim.repeat-jk"));
        let later = 60 * 60 * 1000;
        for s in strokes("<Esc>jjjjjjjw", later) {
            app.on_key(s, s.at_ms);
        }
        assert!(nudges(&mut ext).is_empty());
    }

    fn app_with_store(mode: CoachingMode) -> (App, crate::notify::fake::Recorder) {
        let recorder = crate::notify::fake::Recorder::default();
        let state = State {
            mode: Some(mode),
            ..Default::default()
        };
        let store = flowbuddy_store::Store::open_in_memory().unwrap();
        let mut app = App::new(
            Config::default(),
            state,
            test_resolver(),
            Box::new(recorder.clone()),
        )
        .with_store(store);
        app.claude_probe = || false;
        (app, recorder)
    }

    fn type_at(app: &mut App, s: &str, t0: u64) -> Vec<RuleEvent> {
        strokes(s, t0)
            .into_iter()
            .flat_map(|k| app.on_key(k, k.at_ms))
            .collect()
    }

    #[test]
    fn mastered_habits_get_spaced_check_ins() {
        const DAY: u64 = 86_400_000;
        let now = 1_800_000_000_000;
        let (mut app, _) = app_with_store(CoachingMode::Active);
        let mut ext = ext_hello(&mut app, 1);
        {
            let store = app.memory.store().unwrap();
            let old = now - 30 * DAY;
            let hit = flowbuddy_store::Event::Hit {
                mode: "normal",
                shape: "x×5",
            };
            store
                .record(
                    &memory::day(old),
                    "vscode-vim",
                    "vim.repeat-x",
                    hit,
                    old / 1000,
                )
                .unwrap();
            store
                .set_stage(
                    "vim.repeat-x",
                    "vscode-vim",
                    Stage::Mastered,
                    (now - 8 * DAY) / 1000,
                )
                .unwrap();
        }
        app.memory.maintain(now);
        assert!(
            app.memory
                .check_in_due("vim.repeat-x", ContextKind::VsCodeVim)
        );
        focus(&mut app, "code_code", 500);
        type_at(&mut app, "<Esc>xxxxxw", now);
        assert_eq!(nudges(&mut ext).len(), 1, "one check-in");
        type_at(&mut app, "<Esc>xxxxxw", now + 20 * 60_000);
        assert!(nudges(&mut ext).is_empty(), "then quiet until the next one");
    }

    /// Acceptance test for #27/#30: a whole typing session, and no typed
    /// text anywhere in the database.
    #[test]
    fn database_never_contains_typed_text() {
        let (mut app, _) = app_with_store(CoachingMode::Active);
        let _ext = ext_hello(&mut app, 1);
        focus(&mut app, "code_code", 500);
        type_at(
            &mut app,
            "<Esc>isecretpassword hunter2<BS><BS><BS><BS><BS><BS><BS><Esc>\
             :e secretfile.txt<CR>/secretsearch<CR>jjjjjjjxxxxxdddddd:w<CR>:q<CR>w",
            1_000,
        );
        focus(&mut app, "org.gnome.Terminal", 100);
        ctx(&mut app, CtxEvent::Prompt, None, 200);
        type_at(&mut app, "ssh secrethost<Up><Up><Up><Up><Up><CR>", 60_000);
        app.tick(120_000);

        let store = app.memory.store().unwrap();
        let text = store.all_text().unwrap();
        assert!(!text.is_empty(), "the session was recorded");
        for t in &text {
            for secret in ["secret", "hunter", "password", "file.txt", "ssh"] {
                assert!(!t.contains(secret), "{t:?} leaks {secret:?}");
            }
        }
        let rules: Vec<_> = store
            .patterns()
            .unwrap()
            .into_iter()
            .map(|p| p.rule_id)
            .collect();
        for expected in [
            "vim.repeat-jk",
            "vim.repeat-x",
            "vim.repeat-dd",
            "vim.backspace-streak",
            "vim.w-then-q",
            "bash.up-history",
        ] {
            assert!(
                rules.iter().any(|r| r == expected),
                "{expected} missing from {rules:?}"
            );
        }
    }

    #[test]
    fn hits_adoptions_nudges_and_responses_are_recorded() {
        let (mut app, _) = app_with_store(CoachingMode::Active);
        let mut ext = ext_hello(&mut app, 1);
        focus(&mut app, "code_code", 500);
        type_at(&mut app, "<Esc>jjjjjjjw5j", 1_000);
        let n = nudges(&mut ext).remove(0);
        send(
            &mut app,
            1,
            ClientMsg::NudgeResponse {
                nudge_id: n.nudge_id,
                action: NudgeAction::Dismiss,
            },
            5_000,
        );
        let store = app.memory.store().unwrap();
        let p = store
            .patterns()
            .unwrap()
            .into_iter()
            .find(|p| p.rule_id == "vim.repeat-jk")
            .unwrap();
        assert_eq!(p.context, "vscode-vim");
        assert_eq!(
            (p.occurrences, p.adoptions, p.nudges, p.dismissed),
            (1, 1, 1, 1)
        );
        assert_eq!(p.stage, Stage::Nudged);
        let shapes = store
            .recent_episodes("vim.repeat-jk", "vscode-vim", 5)
            .unwrap();
        assert_eq!(shapes[0].shape, "j×7");
    }

    #[test]
    fn passive_mode_sends_one_end_of_day_summary() {
        use chrono::TimeZone;
        let (mut app, recorder) = app_with_store(CoachingMode::Passive);
        app.config.coaching.summary_time = "18:00".into();
        let at = |h, m| {
            chrono::Local
                .with_ymd_and_hms(2026, 10, 24, h, m, 0)
                .unwrap()
                .timestamp_millis() as u64
        };
        focus(&mut app, "code_code", 500);
        type_at(&mut app, "<Esc>xxxxxw", at(10, 0));
        app.tick(at(17, 59));
        assert!(recorder.0.borrow().is_empty(), "not before summary_time");
        app.tick(at(18, 1));
        app.tick(at(18, 2));
        let sent = recorder.0.borrow();
        assert_eq!(sent.len(), 1, "exactly once");
        assert!(sent[0].1.contains("1 habits spotted"), "{}", sent[0].1);
    }

    fn threshold(app: &App, id: &str) -> Option<u32> {
        app.engine.rules().find(|r| r.id() == id)?.threshold()
    }

    #[test]
    fn not_helpful_reports_raise_the_threshold() {
        let (mut app, _) = app_with_store(CoachingMode::Active);
        let _ext = ext_hello(&mut app, 1);
        focus(&mut app, "code_code", 500);
        type_at(&mut app, "<Esc>xxxxxw", 1_000);
        let report = |app: &mut App| {
            send(
                app,
                1,
                ClientMsg::Command {
                    id: None,
                    command: Command::ReportFalsePositive,
                },
                5_000,
            )
        };
        assert_eq!(threshold(&app, "vim.repeat-x"), Some(4));
        report(&mut app);
        assert_eq!(
            threshold(&app, "vim.repeat-x"),
            Some(4),
            "one report isn't enough"
        );
        report(&mut app);
        assert_eq!(threshold(&app, "vim.repeat-x"), Some(5));
        assert_eq!(app.state.rules["vim.repeat-x"].raised, 1);
        app.set_rule("vim.repeat-x", RuleAction::Reset);
        assert_eq!(threshold(&app, "vim.repeat-x"), Some(4));
    }

    #[test]
    fn a_configured_threshold_is_never_raised() {
        let (mut app, _) = app_with_store(CoachingMode::Active);
        app.config.rules.insert(
            "vim.repeat-x".into(),
            config::RuleConfig {
                enabled: None,
                threshold: Some(6),
            },
        );
        app.engine
            .apply(&config::rule_settings(&app.config, &app.state));
        let _ext = ext_hello(&mut app, 1);
        focus(&mut app, "code_code", 500);
        type_at(&mut app, "<Esc>xxxxxxxw", 1_000);
        for _ in 0..4 {
            send(
                &mut app,
                1,
                ClientMsg::Command {
                    id: None,
                    command: Command::ReportFalsePositive,
                },
                5_000,
            );
        }
        assert_eq!(threshold(&app, "vim.repeat-x"), Some(6));
    }

    #[test]
    fn shell_shapes_are_coached_from_any_window() {
        use flowbuddy_core::ipc::CommandShape;
        let (mut app, _) = app_with_store(CoachingMode::Active);
        let mut ext = ext_hello(&mut app, 1);
        focus(&mut app, "firefox", 900);
        let cd = |app: &mut App, at| {
            send(
                app,
                1,
                ClientMsg::Ctx {
                    event: CtxEvent::Fg,
                    command: Some("cd".into()),
                    vi_mode: None,
                    shell_pid: 200,
                    tty: None,
                    shape: Some(CommandShape::CdUp),
                },
                at,
            )
        };
        cd(&mut app, 1_000);
        cd(&mut app, 3_000);
        let n = nudges(&mut ext);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].rule_id, "shell.cd-up");
        let store = app.memory.store().unwrap();
        assert!(
            store
                .patterns()
                .unwrap()
                .iter()
                .any(|p| p.context == "shell")
        );
    }

    #[test]
    fn weekly_note_by_the_model_when_opted_in() {
        use chrono::TimeZone;
        let (mut app, recorder) = app_with_store(CoachingMode::Balanced);
        app.config.ai.weekly = true;
        app.config.ai.command = Some(vec!["ollama".into(), "run".into(), "m".into()]);
        let at = |d| {
            chrono::Local
                .with_ymd_and_hms(2026, 10, d, 19, 0, 0)
                .unwrap()
                .timestamp_millis() as u64
        };
        app.tick(at(15));
        app.tick(at(22));
        let sent = recorder.0.borrow();
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!(sent[0].0, "weekly service");
    }

    #[test]
    fn weekly_note_once_a_week() {
        use chrono::TimeZone;
        let (mut app, recorder) = app_with_store(CoachingMode::Balanced);
        let at = |d, h| {
            chrono::Local
                .with_ymd_and_hms(2026, 10, d, h, 0, 0)
                .unwrap()
                .timestamp_millis() as u64
        };
        let x = |app: &mut App, t| {
            let store = app.memory.store().unwrap();
            let hit = flowbuddy_store::Event::Hit {
                mode: "normal",
                shape: "x×5",
            };
            store
                .record(&memory::day(t), "vscode-vim", "vim.repeat-x", hit, t / 1000)
                .unwrap();
        };
        // Last week: x streaks five times. This week: once.
        for _ in 0..5 {
            x(&mut app, at(14, 10));
        }
        x(&mut app, at(22, 10));
        app.tick(at(15, 19));
        assert!(
            recorder.0.borrow().is_empty(),
            "the first tick starts the week"
        );
        app.tick(at(21, 19));
        assert!(recorder.0.borrow().is_empty(), "not a week yet");
        app.tick(at(22, 17));
        assert!(recorder.0.borrow().is_empty(), "not before summary_time");
        app.tick(at(22, 19));
        app.tick(at(22, 20));
        let sent = recorder.0.borrow();
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!(sent[0].0, "Flow Buddy: your week");
        assert!(
            sent[0]
                .1
                .contains("Less often than last week: deleting character by character"),
            "{}",
            sent[0].1
        );
    }

    #[test]
    fn maintenance_moves_patterns_to_improving() {
        let (mut app, _) = app_with_store(CoachingMode::Active);
        let _ext = ext_hello(&mut app, 1);
        focus(&mut app, "code_code", 500);
        // One nudged streak, then the better way used repeatedly.
        type_at(&mut app, "<Esc>jjjjjjjw", 1_000);
        type_at(&mut app, "5j3j}{8j", 20_000);
        app.tick(30_000);
        assert_eq!(
            app.memory.stage("vim.repeat-jk", ContextKind::VsCodeVim),
            Stage::Improving
        );
    }

    fn editor(
        app: &mut App,
        id: ConnId,
        kind: EditorKind,
        pid: u32,
    ) -> UnboundedReceiver<ServerMsg> {
        let rx = connect(app, id);
        send(
            app,
            id,
            ClientMsg::Hello {
                client: ClientKind::Editor,
                client_version: "test".into(),
                editor: Some(kind),
                pid: Some(pid),
            },
            0,
        );
        rx
    }

    #[test]
    fn vscode_keys_without_editor_activity_are_not_coached() {
        let mut app = test_app(CoachingMode::Active);
        let mut ext = ext_hello(&mut app, 1);
        let _vs = editor(&mut app, 2, EditorKind::Vscode, 510);
        focus(&mut app, "code_code", 500);
        // Typing in the terminal panel or a chat: no editor heartbeat.
        keys(&mut app, "<Esc>xxxxx<Esc>");
        assert!(nudges(&mut ext).is_empty());
        // Back in the editor.
        send(
            &mut app,
            2,
            ClientMsg::EditorFocus {
                focused: true,
                panel: None,
            },
            0,
        );
        keys(&mut app, "<Esc>xxxxx<Esc>");
        assert_eq!(nudges(&mut ext).len(), 1);
    }

    #[test]
    fn vscode_panel_focus_stops_capture() {
        let mut app = test_app(CoachingMode::Active);
        let _rx = connect(&mut app, 1);
        let _vs = editor(&mut app, 2, EditorKind::Vscode, 510);
        focus(&mut app, "code_code", 500);
        send(
            &mut app,
            2,
            ClientMsg::EditorFocus {
                focused: false,
                panel: None,
            },
            0,
        );
        assert_eq!(app.context().kind, ContextKind::Unsupported);
        assert!(keys(&mut app, "jjjjjjj").is_empty());
    }

    #[test]
    fn editor_mode_replaces_the_guess() {
        let mut app = test_app(CoachingMode::Active);
        let _rx = connect(&mut app, 1);
        let _vs = editor(&mut app, 2, EditorKind::Vscode, 510);
        focus(&mut app, "code_code", 500);
        send(
            &mut app,
            2,
            ClientMsg::EditorFocus {
                focused: true,
                panel: None,
            },
            0,
        );
        assert!(!app.mode_known().1);
        send(
            &mut app,
            2,
            ClientMsg::EditorMode {
                mode: EditorMode::Insert,
            },
            0,
        );
        assert_eq!(app.mode_known(), (VimMode::Insert, true));
        // A click no longer loses the mode: the editor still knows it.
        app.on_pointer_click();
        assert_eq!(app.mode_known(), (VimMode::Insert, true));
    }

    #[test]
    fn vim_plugin_mode_applies_when_its_terminal_is_focused() {
        let mut app = test_app(CoachingMode::Active);
        let _rx = connect(&mut app, 1);
        let _vim = editor(&mut app, 3, EditorKind::Vim, 210);
        send(
            &mut app,
            3,
            ClientMsg::EditorMode {
                mode: EditorMode::Visual,
            },
            0,
        );
        focus(&mut app, "org.gnome.Terminal", 100);
        assert_eq!(app.context().kind, ContextKind::TerminalVim);
        assert_eq!(app.mode_known(), (VimMode::Visual, true));
        app.on_conn(ConnEvent::Closed { id: 3 }, 0);
        assert_eq!(app.context().kind, ContextKind::Unsupported);
    }

    #[test]
    fn editor_motions_replace_the_repeat_rules() {
        let mut app = test_app(CoachingMode::Active);
        let mut ext = ext_hello(&mut app, 1);
        let _vim = editor(&mut app, 3, EditorKind::Vim, 210);
        send(
            &mut app,
            3,
            ClientMsg::EditorMode {
                mode: EditorMode::Normal,
            },
            0,
        );
        focus(&mut app, "org.gnome.Terminal", 100);
        // Seven j presses, 100 ms apart from t=0: the repeat rule stays quiet.
        keys(&mut app, "jjjjjjj");
        assert!(nudges(&mut ext).is_empty());
        // The plugin reports where they went: a paragraph boundary.
        let motion = Motion {
            start_ms: 10,
            end_ms: 610,
            moves: 7,
            mode: EditorMode::Normal,
            lines: 7,
            cols: 0,
            facts: flowbuddy_core::editor::MotionFacts {
                paragraph_hops: Some(1),
                ..Default::default()
            },
        };
        send(&mut app, 3, ClientMsg::Motion(motion), 700);
        let n = nudges(&mut ext);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].rule_id, "motion.paragraph");
        assert_eq!(
            glossary::plain(&n[0].text),
            "That took 7 keys. } gets there in 1, or 7j in 2."
        );
        assert_eq!(
            n[0].explain[0],
            "`}`: down to the next blank line (end of the paragraph)"
        );
    }

    #[test]
    fn operator_reports_suggest_text_objects() {
        use flowbuddy_core::editor::{Operator, OperatorReport, TextObject};
        let mut app = test_app(CoachingMode::Active);
        let mut ext = ext_hello(&mut app, 1);
        let _vim = editor(&mut app, 3, EditorKind::Vim, 210);
        send(
            &mut app,
            3,
            ClientMsg::EditorMode {
                mode: EditorMode::Normal,
            },
            0,
        );
        focus(&mut app, "org.gnome.Terminal", 100);
        // F( and l to get inside, then dt): six keys, 100 ms apart.
        keys(&mut app, "F(ldt)");
        let report = OperatorReport {
            op: Operator::Delete,
            at_ms: 650,
            objects: vec![TextObject::InnerParen],
        };
        send(&mut app, 3, ClientMsg::Operator(report), 700);
        let n = nudges(&mut ext);
        assert_eq!(n.len(), 1);
        assert_eq!(
            glossary::plain(&n[0].text),
            "That took 6 keys. di( does it from anywhere inside, in 3."
        );
    }

    #[test]
    fn motions_from_other_editors_are_ignored() {
        let mut app = test_app(CoachingMode::Active);
        let mut ext = ext_hello(&mut app, 1);
        let _vim = editor(&mut app, 3, EditorKind::Vim, 950);
        focus(&mut app, "code_code", 500);
        keys(&mut app, "<Esc>jjjjjjj");
        let motion = Motion {
            start_ms: 10,
            end_ms: 710,
            moves: 7,
            mode: EditorMode::Normal,
            lines: 7,
            cols: 0,
            facts: Default::default(),
        };
        send(&mut app, 3, ClientMsg::Motion(motion), 800);
        assert!(
            nudges(&mut ext)
                .iter()
                .all(|n| !n.rule_id.starts_with("motion."))
        );
    }

    #[test]
    fn claude_code_prompt_is_coached_with_vim_mode_on() {
        let mut app = test_app(CoachingMode::Active);
        app.claude_probe = || true;
        let mut ext = ext_hello(&mut app, 1);
        app.tick(0);
        focus(&mut app, "org.gnome.Ptyxis", 100);
        send(
            &mut app,
            1,
            ClientMsg::Ctx {
                event: CtxEvent::Fg,
                command: Some("claude".into()),
                vi_mode: None,
                shell_pid: 200,
                tty: None,
                shape: None,
            },
            0,
        );
        assert_eq!(app.context().kind, ContextKind::ClaudeVim);
        // The prompt starts in Insert mode: a run of Backspaces.
        keys(
            &mut app,
            "hello<BS><BS><BS><BS><BS><BS><BS><BS><BS><BS><BS><Esc>",
        );
        let n = nudges(&mut ext);
        assert!(n.iter().any(|n| n.rule_id == "vim.backspace-streak" || n.rule_id == "bash.backspace-clear"), "{n:?}");
    }

    #[test]
    fn input_sources_are_followed_once_per_change() {
        let mut app = test_app(CoachingMode::Active);
        let _ext = ext_hello(&mut app, 1);
        let source = |app: &mut App, layout: &str, variant: &str| {
            send(
                app,
                1,
                ClientMsg::InputSource {
                    layout: Some(layout.into()),
                    variant: Some(variant.into()),
                },
                0,
            )
        };
        source(&mut app, "de", "nodeadkeys");
        assert_eq!(app.take_layout(), Some(("de".into(), "nodeadkeys".into())));
        assert_eq!(app.take_layout(), None, "once per change");
        source(&mut app, "de", "nodeadkeys");
        assert_eq!(app.take_layout(), None, "same layout again");
        // Only xkb-style names.
        source(&mut app, "de; rm -rf /", "");
        assert_eq!(app.take_layout(), None);
        app.set_keymap_layout("us".into(), Some("a note".into()));
        let st = app.status(0);
        assert_eq!(
            (st.layout.as_deref(), st.layout_note.as_deref()),
            (Some("us"), Some("a note"))
        );
    }
}
