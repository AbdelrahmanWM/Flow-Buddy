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
    ClientKind, ClientMsg, Command, CommandShape, CtxEvent, ErrorCode, Nudge, NudgeAction,
    NudgeStyle, RuleAction, RuleInfo, ServerMsg, Status,
};
use flowbuddy_core::ipc::{ModeGuess, Panel, Recording, UiEvent};
use flowbuddy_core::key::{Key, KeyStroke, Sym};
use flowbuddy_core::motion::{self, MotionEvent, OperatorEvent};
use flowbuddy_core::policy::{Coach, CoachingMode, Gate, PolicyConfig, RuleFacts};
use flowbuddy_core::rule::{Finding, RuleEvent};
use flowbuddy_core::rules::vscode::{How, UiAction, UiWhat};
use flowbuddy_core::session::{self, Entry};
use flowbuddy_core::vim::{Flavor, InsertKey, Mappings, Tracker, VimEvent};
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

/// A `flowbuddy coach` waiting for editors' snippets.
struct SnippetWait {
    cli: ConnId,
    cli_id: Option<u64>,
    /// The request id sent to editors.
    id: u64,
    waiting: std::collections::HashSet<ConnId>,
    got: Vec<flowbuddy_core::ipc::Snippet>,
    deadline_ms: u64,
}

/// `when = "pause"`: no keys for this long is a pause.
/// Whether to hold this tip back: true for a `share` of them, at random.
fn held_back(share: f64, rule_id: &str, now_ms: u64) -> bool {
    use std::hash::BuildHasher;
    if share <= 0.0 {
        return false;
    }
    // A randomly keyed hash: random enough, without a dependency.
    let h = std::collections::hash_map::RandomState::new().hash_one((rule_id, now_ms));
    (h as f64 / u64::MAX as f64) < share
}

const CHALLENGE_RULE: &str = "learn.challenge";
/// A tip still waiting for a pause after this long is stale: dropped.
const PAUSE_GIVE_UP_MS: u64 = 120_000;

/// Recent tips kept for snippets: this many, at most.
const RECENT_TIPS: usize = 8;
/// How long editors get to answer.
const SNIPPET_WAIT_MS: u64 = 1000;

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
    /// For the VS Code pack: when the last click came, and the last VS Code
    /// shortcut (Ctrl-P, Ctrl-`) as its letter and time.
    last_click_ms: Option<u64>,
    last_shortcut: Option<(char, u64)>,
    /// Recent tips in an editor: (rule, text, when), for snippets.
    recent_tips: VecDeque<(&'static str, String, u64)>,
    snippet_waits: Vec<SnippetWait>,
    snippet_seq: u64,
    /// Findings waiting a moment for a better one about the same keys:
    /// (finding, context, when it came).
    holding: Vec<(Finding, ContextKind, u64)>,
    hold_ms: u64,
    /// Today's "something new", so a tip that uses it can say so.
    todays_lesson: Option<&'static str>,
    /// When the last key came, for `when = "pause"`.
    last_key_ms: Option<u64>,
    /// When `gv` was last typed, to tell it from reselecting by hand.
    last_gv_ms: Option<u64>,
    /// The last movement or edit that could have been shorter.
    last_slow: Option<flowbuddy_core::ipc::LastSlow>,
    /// How the mode guessed from keys compares with editors' reports.
    mode_guess: ModeGuess,
    /// How long keys and messages take to handle.
    timings: crate::timing::Timings,
    /// The day the rolling recording (`[record] keep_days`) was started for.
    auto_recording: Option<String>,
    /// A session being recorded (`flowbuddy record`), and where to.
    recorder: Option<crate::recorder::Recorder>,
    recordings: PathBuf,
    /// When the last tip was shown.
    last_nudge_ms: Option<u64>,
    /// Each editor's key mappings, as it reported them.
    editor_mappings: HashMap<ConnId, Mappings>,
    /// VS Code findings waiting for editor activity after their keys.
    held_findings: Vec<(Finding, ContextKind)>,
    /// Personal sentences your AI wrote for each rule (`[ai] tips`).
    personal_tips: HashMap<String, String>,
    /// Curriculum items used since the last write, by environment and id.
    usage: HashMap<(ContextKind, &'static str), u64>,
    /// Opt-in typing accuracy, written once a minute.
    typing: crate::typing::Typing,
    typing_flushed_ms: u64,
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
    /// An operator whose motion goes backwards (`db`, `dF<char>`, `d0`).
    backward: bool,
}

/// Whether an operator command's motion goes backwards: `db`, `d2B`,
/// `dF,`, `dT,`, `d0`, `d^`, `dge`.
fn backward_motion(c: &flowbuddy_core::vim::Command) -> bool {
    let rest: Vec<char> = c
        .keys
        .iter()
        .skip(1)
        .filter_map(|k| k.plain_char())
        .skip_while(|ch| ch.is_ascii_digit() && *ch != '0')
        .collect();
    match rest.as_slice() {
        [m, ..] if "bBFT0^h".contains(*m) => true,
        ['g', 'e' | 'E', ..] => true,
        _ => false,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum CmdKind {
    Motion,
    /// d, c or y with its motion or text object.
    Operator,
    /// `v` or `V` from Normal mode: a selection starts.
    VisualStart,
    /// An operator on a Visual selection (`d`, `y`, `>`…).
    VisualOp,
    Other,
}

/// The most a threshold is raised after "not helpful" reports.
const MAX_RAISE: u32 = 3;

/// A shortcut this recent led to the UI event (Ctrl-P, then typing a name).
const UI_SHORTCUT_MS: u64 = 15_000;
/// A click this recent did.
const UI_CLICK_MS: u64 = 1500;

/// A VS Code finding waits this long for editor activity after its keys;
/// in the editor a heartbeat follows within about half a second.
const HELD_MS: u64 = 1000;

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

fn local_hour(now_ms: u64) -> u32 {
    use chrono::{TimeZone, Timelike};
    chrono::Local
        .timestamp_millis_opt(now_ms as i64)
        .single()
        .map_or(12, |t| t.hour())
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
/// The time now, for the few places not handed one.
fn wall_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn is_vscode_ui_shortcut(key: &Key) -> bool {
    let m = key.mods;
    match key.sym {
        Sym::F(1) => true,
        Sym::Tab => m.ctrl,
        Sym::Char(c) if m.ctrl && !m.alt => {
            matches!(
                c,
                'P' | 'p' | 'E' | 'F' | 'G' | 'X' | 'D' | '`' | '~' | 'k' | '\\'
            )
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
        let hold_ms = u64::from(config.coaching.hold_ms);
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
            last_click_ms: None,
            last_shortcut: None,
            mode_guess: ModeGuess::default(),
            timings: Default::default(),
            last_slow: None,
            last_gv_ms: None,
            last_key_ms: None,
            todays_lesson: None,
            holding: Vec::new(),
            hold_ms,
            recent_tips: VecDeque::new(),
            snippet_waits: Vec::new(),
            snippet_seq: 0,
            recorder: None,
            auto_recording: None,
            recordings: flowbuddy_core::paths::recordings_dir(),
            last_nudge_ms: None,
            held_findings: Vec::new(),
            editor_mappings: HashMap::new(),
            personal_tips: HashMap::new(),
            usage: HashMap::new(),
            typing: crate::typing::Typing::default(),
            typing_flushed_ms: 0,
            layout: None,
            layout_changed: false,
            keymap_layout: ("us".into(), None),
        }
        .with_memory_config()
    }

    /// Add an entry to the recording, if one is going; `entry` gets the
    /// time since it started.
    fn record(&mut self, at_ms: u64, entry: impl FnOnce(u64) -> Option<Entry>) {
        if let Some(r) = self.recorder.as_mut()
            && let Some(e) = entry(r.t(at_ms))
        {
            r.write(&e);
        }
    }

    /// Drop in-progress streaks, and note it in a recording.
    fn reset_rules(&mut self, now_ms: u64) {
        self.engine.reset();
        self.record(now_ms, |t| Some(Entry::Reset { t }));
    }

    /// With `[record] keep_days`: keep recording, one file per day, and
    /// delete the old ones. A recording you started yourself is left alone.
    fn roll_recording(&mut self, now_ms: u64) {
        let days = self.config.record.keep_days;
        let today = memory::day(now_ms);
        if days == 0 {
            if self.auto_recording.take().is_some() {
                self.recorder.take().map(|r| r.stop());
            }
            return;
        }
        let manual = self.recorder.is_some() && self.auto_recording.is_none();
        if manual || self.auto_recording.as_deref() == Some(today.as_str()) {
            return;
        }
        self.recorder.take().map(|r| r.stop());
        crate::recorder::Recorder::prune(&self.recordings, days);
        match crate::recorder::Recorder::start_as(&self.recordings, "auto", now_ms) {
            Ok(r) => {
                self.recorder = Some(r);
                let (context, editor) = (self.resolved.kind, self.resolved.editor.is_some());
                self.record(now_ms, |t| Some(Entry::Context { t, context, editor }));
            }
            Err(e) => tracing::warn!(error = %e, "can't keep the rolling recording"),
        }
        self.auto_recording = Some(today);
    }

    fn on_record(&mut self, on: Option<bool>, now_ms: u64) -> Result<Recording, String> {
        match on {
            Some(true) if self.recorder.is_none() => {
                let r = crate::recorder::Recorder::start(&self.recordings, now_ms)
                    .map_err(|e| format!("can't start recording: {e}"))?;
                tracing::info!(path = %r.path().display(), "recording a session");
                self.recorder = Some(r);
                let (context, editor) = (self.resolved.kind, self.resolved.editor.is_some());
                self.record(now_ms, |t| Some(Entry::Context { t, context, editor }));
            }
            Some(false) => {
                if let Some(r) = self.recorder.take() {
                    let path = r.stop();
                    return Ok(Recording {
                        recording: false,
                        path: Some(path.display().to_string()),
                    });
                }
            }
            _ => {}
        }
        Ok(Recording {
            recording: self.recorder.is_some(),
            path: self
                .recorder
                .as_ref()
                .map(|r| r.path().display().to_string()),
        })
    }

    /// Ask every editor for the lines around recent tips; answer `cli` when
    /// they all have, or after a second.
    fn ask_for_snippets(&mut self, cli: ConnId, cli_id: Option<u64>, now_ms: u64) {
        let editors: std::collections::HashSet<ConnId> = self
            .clients
            .iter()
            .filter(|(_, c)| c.kind == Some(ClientKind::Editor))
            .map(|(id, _)| *id)
            .collect();
        self.snippet_seq += 1;
        let id = self.snippet_seq;
        if self.config.text.ai_snippets && !self.recent_tips.is_empty() {
            let times: Vec<u64> = self.recent_tips.iter().map(|(_, _, t)| *t).collect();
            for e in &editors {
                self.reply(
                    *e,
                    ServerMsg::SnippetRequest {
                        id,
                        times: times.clone(),
                    },
                );
            }
        } else {
            return self.reply(
                cli,
                ServerMsg::Result {
                    id: cli_id,
                    data: json!([]),
                },
            );
        }
        self.snippet_waits.push(SnippetWait {
            cli,
            cli_id,
            id,
            waiting: editors,
            got: Vec::new(),
            deadline_ms: now_ms + SNIPPET_WAIT_MS,
        });
        self.answer_snippets(now_ms);
    }

    /// Answer the waits that are complete or out of time: each recent tip
    /// with the snippet closest to it.
    fn answer_snippets(&mut self, now_ms: u64) {
        let (done, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.snippet_waits)
            .into_iter()
            .partition(|w| w.waiting.is_empty() || now_ms >= w.deadline_ms);
        self.snippet_waits = waiting;
        for w in done {
            let tips: Vec<flowbuddy_core::ipc::TipSnippet> = self
                .recent_tips
                .iter()
                .filter_map(|(rule, tip, at)| {
                    let s = w
                        .got
                        .iter()
                        .filter(|s| s.at_ms.abs_diff(*at) <= 3000)
                        .min_by_key(|s| s.at_ms.abs_diff(*at))?;
                    Some(flowbuddy_core::ipc::TipSnippet {
                        rule_id: rule.to_string(),
                        tip: tip.clone(),
                        snippet: s.clone(),
                    })
                })
                .collect();
            self.reply(
                w.cli,
                ServerMsg::Result {
                    id: w.cli_id,
                    data: serde_json::to_value(tips).unwrap_or_default(),
                },
            );
        }
    }

    /// Remember a movement or edit if the best way saves two keys or more.
    fn note_slow(&mut self, kind: &str, at_ms: u64, keys_used: u32, mut best: Vec<(String, u32)>) {
        if best.first().is_none_or(|(_, cost)| keys_used < cost + 2) {
            return;
        }
        best.truncate(3);
        self.last_slow = Some(flowbuddy_core::ipc::LastSlow {
            kind: kind.into(),
            at_ms,
            keys_used,
            best,
        });
    }

    /// A key or message took this long to handle.
    pub fn took(&mut self, d: std::time::Duration) {
        self.timings.record(d);
    }

    fn with_memory_config(mut self) -> Self {
        self.memory.set_config(&self.config);
        self
    }

    /// Record stats and the mistake memory in `store`.
    pub fn with_store(mut self, store: flowbuddy_store::Store) -> Self {
        self.memory = Memory::new(Some(store), &self.config);
        self.engine
            .set_known(self.memory.known(Some(self.resolved.kind), wall_ms()));
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
        let has_ai =
            ai.command.is_some() || ai.provider != flowbuddy_core::config::AiProvider::None;
        if ai.weekly && has_ai && self.notifier.start_weekly_note() {
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
        let hour = local_hour(now_ms);
        if self.config.ai.filter
            && !self
                .memory
                .filter_allows(f.rule_id, self.mode.as_str(), hour, now_ms)
        {
            return;
        }
        // A few held back at random: comparing them with the ones shown
        // tells whether tips help.
        if held_back(self.config.learn.holdout, f.rule_id, now_ms) {
            self.memory
                .tip_held(f.rule_id, self.mode.as_str(), hour, now_ms);
            return;
        }
        // The model's sentence too must only name what works where the
        // habit was seen.
        let ctx = self
            .memory
            .finding_context(f.rule_id)
            .unwrap_or(self.resolved.kind);
        let text = match self
            .personal_tips
            .get(f.rule_id)
            .filter(|p| flowbuddy_core::support::supports_all(ctx, p))
        {
            Some(personal) => format!("{} {personal}", f.text),
            None => f.text.clone(),
        };
        // A tip that builds on today's lesson ties the two together.
        let lesson = self.todays_lesson.filter(|l| {
            flowbuddy_core::curriculum::rule_skills(f.rule_id).is_some_and(|s| s.contains(l))
        });
        let text = match lesson {
            Some(_) => format!("{text} That's today's something new."),
            None => text,
        };
        // Worded for where you are with it.
        let text = flowbuddy_core::learning::adapt(
            &text,
            self.memory.stage(f.rule_id, ctx),
            self.memory.unhelpful_in_a_row(f.rule_id, now_ms),
        );
        self.show(f.rule_id, f.title, &text, f.mode == VimMode::Insert, now_ms);
        self.memory.nudged(f.rule_id, now_ms);
        self.memory
            .tip_shown(f.rule_id, self.mode.as_str(), hour, now_ms);
    }

    /// Today's sentences from `tips.json`, written by `flowbuddy coach
    /// --write-tips`; older than three days, they're not used.
    fn load_personal_tips(&mut self, now_ms: u64) {
        #[derive(serde::Deserialize)]
        struct Cache {
            generated_at: u64,
            tips: HashMap<String, String>,
        }
        self.personal_tips.clear();
        if !self.config.ai.tips {
            return;
        }
        let path = match &self.state_path {
            Some(p) => p.with_file_name("tips.json"),
            None => return,
        };
        let Some(cache) = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<Cache>(&t).ok())
        else {
            return;
        };
        if (now_ms / 1000).saturating_sub(cache.generated_at) <= 3 * 86_400 {
            self.personal_tips = cache.tips;
        }
    }

    /// Put a tip on screen: through the extension, or as a notification.
    fn show(&mut self, rule_id: &'static str, title: &str, text: &str, insert: bool, now_ms: u64) {
        self.nudge_seq += 1;
        let nudge = Nudge {
            nudge_id: format!("n-{now_ms}-{}", self.nudge_seq),
            rule_id: rule_id.into(),
            title: title.into(),
            text: text.into(),
            ttl_ms: u64::from(self.config.coaching.nudge_seconds) * 1000,
            style: self.config.coaching.nudge_style.unwrap_or(match self.mode {
                CoachingMode::Active => NudgeStyle::Card,
                _ => NudgeStyle::Bar,
            }),
            explain: glossary::explain(text, insert),
        };
        // To the extension and anyone watching; a notification if there's
        // no extension to show it.
        self.broadcast(ServerMsg::Nudge(nudge.clone()));
        if !self.extension_connected() {
            // Plain notifications show no markup: drop the markers and put
            // the explanation underneath.
            let mut body = glossary::plain(&nudge.text);
            for line in &nudge.explain {
                body.push('\n');
                body.push_str(&glossary::plain(line));
            }
            self.notifier.notify(&nudge.title, &body, nudge.ttl_ms);
        }
        self.record(now_ms, |t| {
            Some(Entry::Feedback {
                t,
                rule: rule_id.into(),
                what: "shown".into(),
            })
        });
        self.memory.add_history(&flowbuddy_store::HistoryTip {
            shown_at: now_ms / 1000,
            rule_id: rule_id.into(),
            title: title.into(),
            text: text.into(),
            insert,
        });
        if self.nudges.len() == NUDGE_HISTORY {
            self.nudges.pop_front();
        }
        self.nudges.push_back((nudge.nudge_id, rule_id));
        self.last_nudge_ms = Some(now_ms);
    }

    /// Once a day, in every mode but off: one thing from the curriculum you
    /// don't use yet, while you're in Vim and nothing else was just shown.
    fn something_new(&mut self, now_ms: u64) {
        const RULE: &str = "learn.daily";
        let today = memory::day(now_ms);
        let quiet = self
            .config
            .coaching
            .quiet_hours
            .as_deref()
            .and_then(config::parse_range)
            .is_some_and(|q| q.contains(self.gate(now_ms).local_minute));
        // Wherever there's something to teach: each place has its own.
        let editing = matches!(
            self.resolved.kind,
            ContextKind::VsCodeVim
                | ContextKind::TerminalVim
                | ContextKind::ClaudeVim
                | ContextKind::BashVi
        );
        if !self.config.learn.daily
            || self.mode == CoachingMode::Off
            || !self.capturing(now_ms)
            || quiet
            || !editing
            || self.state.last_learn_day.as_deref() == Some(today.as_str())
            || self.state.muted().contains(RULE)
            || self.last_nudge_ms.is_some_and(|t| {
                now_ms.saturating_sub(t) < u64::from(self.config.learn.gap_minutes) * 60_000
            })
        {
            return;
        }
        let Some(pick) = self.memory.pick_something_new(self.resolved.kind, now_ms) else {
            return;
        };
        let item = pick.item;
        self.state.last_learn_day = Some(today);
        self.save_state();
        let mut text = flowbuddy_core::curriculum::tip_text(item);
        // Producing it beats reading about it: a challenge to use it.
        if self.config.learn.challenges {
            text.push_str(" Challenge: use it the next time it fits.");
            if let Some(old) = self.state.challenge.take() {
                self.memory
                    .challenge_ended(&old.item, old.set_at, "expired", now_ms);
            }
            self.state.challenge = Some(config::Challenge {
                item: item.id.into(),
                set_at: now_ms / 1000,
            });
            self.save_state();
        }
        // A review: you'd mastered it, and it's slipping.
        let title = if pick.review {
            "Remember this?"
        } else {
            "Something new"
        };
        self.show(RULE, title, &text, item.while_typing(), now_ms);
        self.memory.learn_shown(item.id, now_ms);
        self.todays_lesson = Some(item.id);
    }

    /// The challenge is met when its item is used.
    fn check_challenge(&mut self, usage: &HashMap<(ContextKind, &'static str), u64>, now_ms: u64) {
        let Some(c) = self.state.challenge.clone() else {
            return;
        };
        if usage.keys().any(|(_, id)| *id == c.item) {
            self.state.challenge = None;
            self.save_state();
            self.memory
                .challenge_ended(&c.item, c.set_at, "done", now_ms);
            if let Some(item) = flowbuddy_core::curriculum::find(&c.item) {
                let text = format!("You used {}: challenge done.", item.keys);
                self.show(CHALLENGE_RULE, "Nice", &text, false, now_ms);
            }
        } else if now_ms / 1000 >= c.set_at + u64::from(self.config.learn.challenge_days) * 86_400 {
            self.state.challenge = None;
            self.save_state();
            self.memory
                .challenge_ended(&c.item, c.set_at, "expired", now_ms);
        }
    }

    /// Missed: the old habit, where the challenge's item would have done,
    /// and could have here.
    fn challenge_missed(&mut self, rule_id: &str, ctx: ContextKind, now_ms: u64) {
        let Some(c) = self.state.challenge.clone() else {
            return;
        };
        let Some(item) = flowbuddy_core::curriculum::find(&c.item) else {
            return;
        };
        let teaches =
            flowbuddy_core::curriculum::rule_skills(rule_id).is_some_and(|s| s.contains(&item.id));
        if teaches
            && item.applies_in(ctx)
            && flowbuddy_core::support::supports_all(
                ctx,
                &flowbuddy_core::curriculum::tip_text(item),
            )
        {
            self.state.challenge = None;
            self.save_state();
            self.memory
                .challenge_ended(&c.item, c.set_at, "missed", now_ms);
        }
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
        let what = serde_json::to_value(action)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        self.record(now_ms, |t| {
            Some(Entry::Feedback {
                t,
                rule: rule_id.into(),
                what,
            })
        });
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
        self.hold_ms = u64::from(config.coaching.hold_ms);
        self.memory.set_config(&config);
        self.engine
            .set_known(self.memory.known(Some(self.resolved.kind), wall_ms()));
        let text_changed = config.text != self.config.text;
        self.config = config;
        if text_changed {
            let msg = self.editor_config();
            for c in self.clients.values() {
                if c.kind == Some(ClientKind::Editor) {
                    let _ = c.tx.send(msg.clone());
                }
            }
        }
        tracing::info!("config reloaded");
    }

    fn editor_config(&self) -> ServerMsg {
        ServerMsg::EditorConfig(flowbuddy_core::ipc::EditorConfig::from_config(
            &self.config.text,
        ))
    }

    /// Findings and adoptions go here: coaching policy and storage hook in
    /// at this point.
    fn on_rule_events(&mut self, events: &[RuleEvent], now_ms: u64) {
        self.on_rule_events_in(self.resolved.kind, events, now_ms, true);
    }

    /// Rule events from `ctx`, which is the focused context except for
    /// events that don't come from keys (the shell and VS Code packs).
    /// `keyed`: they come from keys, so check the keys reached the editor.
    fn on_rule_events_in(
        &mut self,
        ctx: ContextKind,
        events: &[RuleEvent],
        now_ms: u64,
        keyed: bool,
    ) {
        for ev in events {
            match ev {
                // VS Code: wait for editor activity after the keys (#110).
                RuleEvent::Finding(f) if keyed && self.vscode_editor().is_some() => {
                    let seen_after = self
                        .vscode_editor()
                        .is_some_and(|e| e.focused == Some(true) && e.focused_ms + 50 >= f.at_ms);
                    if seen_after {
                        self.on_rule_events_in(ctx, std::slice::from_ref(ev), now_ms, false);
                    } else {
                        self.held_findings.push((f.clone(), ctx));
                    }
                }
                RuleEvent::Finding(f) if keyed && !self.editor_saw(f.at_ms) => {
                    tracing::debug!(
                        rule = f.rule_id,
                        "finding dropped: keys didn't reach the editor"
                    );
                }
                RuleEvent::Adoption { at_ms, .. } if keyed && !self.editor_saw(*at_ms) => {}
                RuleEvent::Finding(f) => self.hold(f.clone(), ctx, now_ms),
                RuleEvent::Adoption { rule_id, .. } => self.memory.adoption(rule_id, ctx, now_ms),
            }
        }
    }

    /// Hold a finding a moment: rules that see the same keys can fire a
    /// little apart (`.` on the third repeat, a macro on the fourth), and
    /// only the better-ranked one should become a tip.
    fn hold(&mut self, f: Finding, ctx: ContextKind, now_ms: u64) {
        if self.hold_ms == 0 && !self.wait_for_pause() {
            return self.take(&f, ctx, now_ms);
        }
        let rank = self.engine.rank_of(f.rule_id);
        if let Some(p) = self.holding.iter_mut().find(|(h, _, _)| h.overlaps(&f)) {
            // The better-ranked leads; the other stays in it as another way.
            if rank < self.engine.rank_of(p.0.rule_id) {
                tracing::debug!(kept = f.rule_id, joined = p.0.rule_id, "same gesture");
                let mut best = f;
                best.absorb(&p.0);
                *p = (best, ctx, now_ms);
            } else {
                tracing::debug!(kept = p.0.rule_id, joined = f.rule_id, "same gesture");
                p.0.absorb(&f);
            }
            return;
        }
        self.holding.push((f, ctx, now_ms));
    }

    fn pause_ms(&self) -> u64 {
        u64::from(self.config.coaching.pause_ms)
    }

    fn wait_for_pause(&self) -> bool {
        self.config.coaching.when == config::TipTiming::Pause
    }

    /// Findings held long enough become tips (or don't, by the policy);
    /// with `when = "pause"`, only once no key came for a moment.
    fn release(&mut self, now_ms: u64) {
        let paused = !self.wait_for_pause()
            || self
                .last_key_ms
                .is_none_or(|t| now_ms.saturating_sub(t) >= self.pause_ms());
        let stale = |at: u64| now_ms.saturating_sub(at) > PAUSE_GIVE_UP_MS;
        let (ready, wait): (Vec<_>, Vec<_>) = std::mem::take(&mut self.holding)
            .into_iter()
            .filter(|(_, _, at)| !stale(*at))
            .partition(|(_, _, at)| paused && now_ms.saturating_sub(*at) >= self.hold_ms);
        self.holding = wait;
        for (f, ctx, _) in ready {
            self.take(&f, ctx, now_ms);
        }
    }

    fn take(&mut self, f: &Finding, ctx: ContextKind, now_ms: u64) {
        tracing::debug!(rule = f.rule_id, shape = %f.shape, "finding");
        self.memory.hit(f, ctx, now_ms);
        self.challenge_missed(f.rule_id, ctx, now_ms);
        if self.resolved.editor.is_some() {
            if self.recent_tips.len() == RECENT_TIPS {
                self.recent_tips.pop_front();
            }
            self.recent_tips
                .push_back((f.rule_id, f.text.clone(), f.at_ms));
        }
        let gate = self.gate(now_ms);
        let facts = RuleFacts {
            muted: self.state.muted().contains(f.rule_id),
            stage: self.memory.stage(f.rule_id, ctx),
            focus: self.memory.is_focus(f.rule_id),
            check_in: self.memory.check_in_due(f.rule_id, ctx),
        };
        if let Some(f) = self.coach.on_finding(f, facts, &gate, now_ms) {
            self.deliver(&f, now_ms);
        }
    }

    fn is_vscode(&self, conn: ConnId) -> bool {
        self.resolver
            .editor(conn)
            .is_some_and(|e| e.kind == EditorKind::Vscode)
    }

    /// A VS Code UI event for the VS Code pack: did the shortcut (`key`
    /// with Ctrl) or a click lead to it?
    fn on_ui(&mut self, what: UiWhat, key: char, now_ms: u64) {
        if !self.capturing(now_ms) {
            return;
        }
        let recent = |t: u64, ms: u64| now_ms.saturating_sub(t) <= ms;
        let how = if self
            .last_shortcut
            .is_some_and(|(k, t)| k == key && recent(t, UI_SHORTCUT_MS))
        {
            How::Shortcut
        } else if self.last_click_ms.is_some_and(|t| recent(t, UI_CLICK_MS)) {
            How::Click
        } else {
            How::Other
        };
        if how == How::Shortcut {
            for item in flowbuddy_core::curriculum::used_by_ui(what) {
                *self
                    .usage
                    .entry((ContextKind::VsCodeVim, item.id))
                    .or_default() += 1;
            }
        }
        let action = UiAction {
            what,
            how,
            at_ms: now_ms,
        };
        let events = self.engine.on_ui(&action);
        self.on_rule_events_in(ContextKind::VsCodeVim, &events, now_ms, false);
    }

    /// Keys pressed for a movement: the motion commands just before its end,
    /// back to its start or the last command that wasn't a motion. 0 when
    /// the count can't be trusted: a click came during it, or VS Code saw
    /// fewer moves than the commands counted (keys from another moment).
    /// Vim merges cursor events while keys are typed fast, so only VS
    /// Code's count is checked.
    fn keys_for(&self, m: &Motion, conn: ConnId) -> u32 {
        let from = m.start_ms.saturating_sub(MOTION_LEAD_MS);
        let to = m.end_ms + MOTION_LAG_MS;
        let (keys, commands) = self
            .commands
            .iter()
            .rev()
            .skip_while(|c| c.at_ms > to)
            .take_while(|c| c.at_ms >= from && c.kind == CmdKind::Motion)
            .fold((0, 0), |(k, n), c| (k + c.keys, n + 1));
        let clicked = self
            .last_click_ms
            .is_some_and(|t| t >= m.start_ms && t <= m.end_ms);
        let too_many = self.is_vscode(conn) && commands > m.moves + 1;
        if clicked || too_many {
            tracing::debug!(clicked, commands, moves = m.moves, "key count not trusted");
            return 0;
        }
        keys
    }

    /// Keys pressed for an operator the editor reported: the operator
    /// command itself, plus the motions just before it that positioned the
    /// cursor (a text object works from anywhere inside, so it wouldn't
    /// need them). 0 if the operator isn't in the history.
    /// Keys for the operator that made `at_ms`'s report, and with the
    /// moves into place before it.
    fn keys_for_operator(&self, at_ms: u64) -> (u32, u32, bool) {
        let mut recent = self
            .commands
            .iter()
            .rev()
            .skip_while(|c| c.at_ms > at_ms + MOTION_LAG_MS);
        let Some(op) = recent.next().filter(|c| {
            matches!(c.kind, CmdKind::Operator | CmdKind::VisualOp)
                && at_ms.saturating_sub(c.at_ms) <= MOTION_LEAD_MS
        }) else {
            return (0, 0, false);
        };
        // A selection: everything from `v` to the operator is what a
        // better command would replace.
        if op.kind == CmdKind::VisualOp {
            let mut keys = op.keys;
            for c in recent {
                keys += c.keys;
                match c.kind {
                    CmdKind::VisualStart => return (keys, keys, false),
                    CmdKind::Motion => {}
                    _ => break,
                }
            }
            return (0, 0, false);
        }
        let mut keys = op.keys;
        let mut next_at = op.at_ms;
        for c in recent {
            if c.kind != CmdKind::Motion || next_at.saturating_sub(c.at_ms) > MOTION_LEAD_MS {
                break;
            }
            keys += c.keys;
            next_at = c.at_ms;
        }
        (keys, op.keys, op.backward)
    }

    /// The mappings for the current context: its editor's, else the config's.
    fn mappings_now(&self) -> Mappings {
        self.resolved
            .editor
            .and_then(|id| self.editor_mappings.get(&id).cloned())
            .unwrap_or_else(|| Mappings::from_names(&self.config.keyboard.insert_escapes, &[]))
    }

    /// The context's VS Code companion, if that's where the keys go.
    fn vscode_editor(&self) -> Option<&crate::resolver::Editor> {
        self.resolved
            .editor
            .and_then(|id| self.resolver.editor(id))
            .filter(|e| e.kind == EditorKind::Vscode)
    }

    /// Editor activity came in: show the held findings whose keys it
    /// followed; drop the ones that waited too long.
    fn release_held(&mut self, now_ms: u64) {
        if self.held_findings.is_empty() {
            return;
        }
        let beat = self
            .vscode_editor()
            .filter(|e| e.focused == Some(true))
            .map(|e| e.focused_ms);
        let held = std::mem::take(&mut self.held_findings);
        for (f, ctx) in held {
            if beat.is_some_and(|b| b + 50 >= f.at_ms && b <= f.at_ms + HELD_MS) {
                self.on_rule_events_in(ctx, &[RuleEvent::Finding(f)], now_ms, false);
            } else if now_ms.saturating_sub(f.at_ms) > HELD_MS {
                tracing::debug!(
                    rule = f.rule_id,
                    "finding dropped: no editor activity after its keys"
                );
            } else {
                self.held_findings.push((f, ctx));
            }
        }
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
        self.release_held(now_ms);
        self.memory.expire_tips(now_ms);
        if now_ms.saturating_sub(self.typing_flushed_ms) >= 60_000 {
            self.typing_flushed_ms = now_ms;
            let counts = self.typing.take();
            self.memory.typing(counts, now_ms);
            let usage = std::mem::take(&mut self.usage);
            self.check_challenge(&usage, now_ms);
            self.memory.command_uses(usage, now_ms);
            self.engine
                .set_known(self.memory.known(Some(self.resolved.kind), now_ms));
            if let Some(r) = self.recorder.as_mut() {
                r.flush();
            }
            self.load_personal_tips(now_ms);
        }
        // Switching tmux panes tells no one; look again.
        if self.resolver.via_tmux() {
            self.update_context(None, now_ms);
        }
        if self.paused_until.is_some_and(|t| t * 1000 <= now_ms) {
            self.paused_until = None;
            self.mode_changed();
            self.save_state();
        }
        self.something_new(now_ms);
        self.answer_snippets(now_ms);
        self.roll_recording(now_ms);
        self.release(now_ms);
        let events = self.engine.tick(self.resolved.kind, now_ms);
        self.on_rule_events(&events, now_ms);
        // A batch waits for a pause too, if you asked for that.
        let typing = self.wait_for_pause()
            && self
                .last_key_ms
                .is_some_and(|t| now_ms.saturating_sub(t) < self.pause_ms());
        if !typing {
            let gate = self.gate(now_ms);
            for f in self.coach.on_tick(&gate, now_ms) {
                self.deliver(&f, now_ms);
            }
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
            if let Sym::Char(c) = stroke.key.sym {
                self.last_shortcut = Some((c.to_ascii_lowercase(), stroke.at_ms));
            }
            self.tracker.invalidate();
            self.reset_rules(now_ms);
            return Vec::new();
        }
        self.memory.active(kind, now_ms);
        self.last_key_ms = Some(stroke.at_ms);
        let step = self.tracker.feed(stroke);
        if !step.confident {
            self.mode_guess.keys_unsure += 1;
        }
        if step.confident
            && matches!(
                kind,
                ContextKind::VsCodeVim
                    | ContextKind::TerminalVim
                    | ContextKind::ClaudeVim
                    | ContextKind::BashVi
            )
        {
            for item in flowbuddy_core::curriculum::CURRICULUM {
                if item.applies_in(kind) && item.used_in(&step) {
                    *self.usage.entry((kind, item.id)).or_default() += 1;
                }
            }
        }
        if self.config.typing.accuracy {
            match (&step.event, stroke.key.sym) {
                (Some(VimEvent::Insert(InsertKey::Text)), Sym::Char(c)) => {
                    self.typing.typed(kind, c)
                }
                (Some(VimEvent::Insert(InsertKey::Backspace)), _) => self.typing.backspace(kind),
                (None, _) => {}
                _ => self.typing.other(),
            }
        }
        if step.confident
            && let Some(VimEvent::Command(c)) = &step.event
        {
            if c.is("gv") {
                self.last_gv_ms = Some(stroke.at_ms);
            }
            if self.commands.len() == COMMAND_HISTORY {
                self.commands.pop_front();
            }
            let kind = if motion::is_motion(c) {
                CmdKind::Motion
            } else if step.before.is_visual()
                && c.single()
                    .and_then(|k| k.plain_char())
                    .is_some_and(|h| matches!(h, 'd' | 'c' | 'y' | 'x' | 's' | '>' | '<'))
            {
                CmdKind::VisualOp
            } else if !step.before.is_visual() && (c.is("v") || c.is("V")) {
                CmdKind::VisualStart
            } else if c.keys.len() >= 2 && c.head().is_some_and(|h| matches!(h, 'd' | 'c' | 'y')) {
                CmdKind::Operator
            } else {
                CmdKind::Other
            };
            self.commands.push_back(Cmd {
                at_ms: stroke.at_ms,
                keys: motion::command_keys(c),
                kind,
                backward: kind == CmdKind::Operator && backward_motion(c),
            });
        }
        self.record(stroke.at_ms, |t| session::step_entry(t, &step));
        let events = self.engine.on_step(kind, &step);
        self.on_rule_events(&events, now_ms);
        events
    }

    /// A mouse click may have changed the editor's mode or focus.
    pub fn on_pointer_click(&mut self, now_ms: u64) {
        self.last_click_ms = Some(now_ms);
        self.tracker.invalidate();
        self.reset_rules(now_ms);
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
            self.tracker.set_mappings(self.mappings_now());
            self.commands.clear();
            self.typing.other();
            self.engine.set_editor_motions(new.editor.is_some());
            // What you know here: progress is kept per environment.
            self.engine
                .set_known(self.memory.known(Some(new.kind), now_ms));
            let (context, editor) = (new.kind, new.editor.is_some());
            self.record(now_ms, |t| Some(Entry::Context { t, context, editor }));
        }
        // Only guessed that you went to VS Code's terminal (a background
        // command can finish there too): the mode stays unknown until
        // there's more to go on, so no tip comes from the guess.
        let guessed = changed && self.resolver.terminal_guessed();
        if guessed {
            self.tracker.invalidate();
        } else if let Some(mode) = known {
            self.tracker.reset_known(mode);
        }
        if changed && !guessed {
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

    /// The focused window changed (from the GNOME extension, or X11).
    pub fn set_focus(&mut self, app_id: Option<String>, pid: Option<u32>, now_ms: u64) {
        self.resolver.set_focus(app_id, pid);
        self.resolver.prune();
        self.update_context(None, now_ms);
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
                for w in &mut self.snippet_waits {
                    w.waiting.remove(&id);
                }
                self.answer_snippets(now_ms);
                if self.resolver.editor(id).is_some() {
                    self.resolver.editor_gone(id);
                    self.update_context(None, now_ms);
                }
            }
            ConnEvent::Msg { id, msg } => self.on_msg(id, *msg, now_ms),
        }
    }

    fn reply(&self, id: ConnId, msg: ServerMsg) {
        if let Some(c) = self.clients.get(&id) {
            let _ = c.tx.send(msg);
        }
    }

    /// Send to every client that stays connected to show things: the
    /// extension and `flowbuddy watch`.
    fn broadcast(&self, msg: ServerMsg) {
        for c in self.clients.values() {
            if matches!(c.kind, Some(ClientKind::GnomeExtension | ClientKind::Watch)) {
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
                    self.reply(conn, self.editor_config());
                }
            }
            ClientMsg::Focus { app_id, pid, .. } => self.set_focus(app_id, pid, now_ms),
            ClientMsg::Ctx {
                event: CtxEvent::Mode,
                mode,
                shell_pid,
                ..
            } => {
                // The shell said which vi-mode it's in: better than our guess.
                // Only a disagreement resets, so keys typed meanwhile count.
                self.resolver.shell_active(shell_pid, now_ms);
                self.update_context(None, now_ms);
                if let Some(m) = mode.and_then(|m| m.vim_mode())
                    && self.resolved.shell_pid == Some(shell_pid)
                    && (!self.tracker.confident() || !self.tracker.mode().same_kind(m))
                {
                    self.tracker.reset_known(m);
                }
            }
            ClientMsg::Ctx {
                event,
                command,
                vi_mode,
                shell_pid,
                shape,
                tty,
                ..
            } => {
                // Two habits are plain from the command's name alone.
                let named = match (event, command.as_deref()) {
                    (CtxEvent::Fg, Some("clear")) => Some(CommandShape::Clear),
                    (CtxEvent::Fg, Some("history")) => Some(CommandShape::History),
                    _ => None,
                };
                if let Some(shape) = shape.filter(|_| self.capturing(now_ms)) {
                    for item in flowbuddy_core::curriculum::used_by_shape(shape) {
                        *self.usage.entry((ContextKind::Shell, item.id)).or_default() += 1;
                    }
                }
                if let Some(shape) = shape.or(named).filter(|_| self.capturing(now_ms)) {
                    let ev = flowbuddy_core::rules::shell::ShellEvent {
                        shape,
                        at_ms: now_ms,
                    };
                    let events = self.engine.on_shell(&ev);
                    self.on_rule_events_in(ContextKind::Shell, &events, now_ms, false);
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
                    CtxEvent::Prompt | CtxEvent::Mode => (
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
            ClientMsg::PointerClick => self.on_pointer_click(now_ms),
            ClientMsg::EditorEvent {
                event: UiEvent::FileOpened,
            } if self.is_vscode(conn) => self.on_ui(UiWhat::FileOpened, 'p', now_ms),
            ClientMsg::EditorEvent {
                event: UiEvent::Reselected,
            } if self.resolved.editor == Some(conn) => {
                // `gv` itself makes the same selection: that's the better way.
                let how = if self
                    .last_gv_ms
                    .is_some_and(|t| now_ms.saturating_sub(t) <= UI_SHORTCUT_MS)
                {
                    How::Shortcut
                } else {
                    How::Other
                };
                let action = UiAction {
                    what: UiWhat::Reselected,
                    how,
                    at_ms: now_ms,
                };
                let events = self.engine.on_ui_in(self.resolved.kind, &action);
                self.on_rule_events_in(self.resolved.kind, &events, now_ms, false);
            }
            // Ctrl-\ splits from the keyboard.
            ClientMsg::EditorEvent {
                event: UiEvent::SplitOpened,
            } if self.is_vscode(conn) => self.on_ui(UiWhat::SplitOpened, '\\', now_ms),
            // `gt` isn't a Ctrl shortcut: only clicks are told apart.
            ClientMsg::EditorEvent {
                event: UiEvent::TabSwitched,
            } if self.is_vscode(conn) => self.on_ui(UiWhat::TabSwitched, '\0', now_ms),
            ClientMsg::EditorEvent { .. } => {}
            ClientMsg::EditorMappings {
                insert_escapes,
                normal,
            } => {
                self.editor_mappings
                    .insert(conn, Mappings::from_names(&insert_escapes, &normal));
                if self.resolved.editor == Some(conn) {
                    self.tracker.set_mappings(self.mappings_now());
                }
            }
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
                if !focused && panel == Some(Panel::Terminal) && self.is_vscode(conn) {
                    self.on_ui(UiWhat::TerminalFocused, '`', now_ms);
                }
                self.resolver.editor_focus(conn, focused, panel, now_ms);
                self.update_context(None, now_ms);
                self.release_held(now_ms);
            }
            ClientMsg::EditorMode { mode } => {
                if self.resolved.editor == Some(conn)
                    && let Some(m) = mode.vim_mode()
                {
                    let g = &mut self.mode_guess;
                    match (self.tracker.confident(), self.tracker.mode().same_kind(m)) {
                        (false, _) => g.unsure += 1,
                        (true, true) => g.right += 1,
                        (true, false) => g.wrong += 1,
                    }
                }
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
                    keys_used: self.keys_for(&m, conn),
                    motion: m,
                };
                self.record(now_ms, |t| {
                    Some(Entry::Motion {
                        t,
                        motion: ev.motion.clone(),
                        keys_used: ev.keys_used,
                    })
                });
                let best = motion::best_for_motion(&ev, self.engine.known());
                self.note_slow("move", ev.motion.end_ms, ev.keys_used, best);
                let events = self.engine.on_motion(self.resolved.kind, &ev);
                self.on_rule_events(&events, now_ms);
            }
            // From an editor that isn't the current context.
            ClientMsg::Motion(_) => {}
            ClientMsg::Operator(report) if self.resolved.editor == Some(conn) => {
                let (keys_used, op_keys, backward) = self.keys_for_operator(report.at_ms);
                let ev = OperatorEvent {
                    keys_used,
                    op_keys,
                    backward,
                    report,
                };
                self.record(now_ms, |t| {
                    Some(Entry::Operator {
                        t,
                        report: ev.report.clone(),
                        keys_used: ev.keys_used,
                        op_keys: ev.op_keys,
                        backward: ev.backward,
                    })
                });
                let best = motion::best_for_edit(&ev, self.engine.known());
                self.note_slow("edit", ev.report.at_ms, ev.keys_used, best);
                let events = self.engine.on_operator(self.resolved.kind, &ev);
                self.on_rule_events(&events, now_ms);
            }
            ClientMsg::Operator(_) => {}
            ClientMsg::Command {
                id,
                command: Command::Snippets,
            } => self.ask_for_snippets(conn, id, now_ms),
            ClientMsg::Command { id, command } => {
                let reply = self.on_command(id, command, now_ms);
                self.reply(conn, reply);
            }
            ClientMsg::Snippets { id, snippets } => {
                if let Some(w) = self.snippet_waits.iter_mut().find(|w| w.id == id) {
                    w.waiting.remove(&conn);
                    w.got.extend(snippets.into_iter().take(RECENT_TIPS));
                }
                self.answer_snippets(now_ms);
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
            Command::RuleThreshold { rule_id, threshold } => {
                match self.engine.default_threshold(&rule_id) {
                    None => ServerMsg::Error {
                        id,
                        code: ErrorCode::UnknownRule,
                        message: format!("{rule_id} has no threshold"),
                    },
                    Some(_) => {
                        self.state.rules.entry(rule_id).or_default().threshold =
                            threshold.filter(|n| *n > 0);
                        self.engine
                            .apply(&config::rule_settings(&self.config, &self.state));
                        self.save_state();
                        ServerMsg::Ack { id }
                    }
                }
            }
            // Answered once the editors have (ask_for_snippets).
            Command::Snippets => ServerMsg::Result {
                id,
                data: json!([]),
            },
            Command::KnowItems { items, known } => {
                let unknown: Vec<&String> = items
                    .iter()
                    .filter(|i| flowbuddy_core::curriculum::find(i).is_none())
                    .collect();
                if !unknown.is_empty() {
                    return ServerMsg::Error {
                        id,
                        code: ErrorCode::BadRequest,
                        message: format!(
                            "not curriculum items: {unknown:?} (see flowbuddy learn --list)"
                        ),
                    };
                }
                self.memory.set_known(&items, known);
                self.engine
                    .set_known(self.memory.known(Some(self.resolved.kind), now_ms));
                ServerMsg::Ack { id }
            }
            Command::RecentTips { limit } => {
                let limit = limit.unwrap_or(10).clamp(1, 50) as usize;
                let tips: Vec<flowbuddy_core::ipc::RecentTip> = self
                    .memory
                    .history(limit)
                    .into_iter()
                    .map(|t| flowbuddy_core::ipc::RecentTip {
                        explain: glossary::explain(&t.text, t.insert),
                        shown_at: t.shown_at,
                        rule_id: t.rule_id,
                        title: t.title,
                        text: t.text,
                    })
                    .collect();
                ServerMsg::Result {
                    id,
                    data: serde_json::to_value(tips).unwrap_or_default(),
                }
            }
            Command::LastSlow => ServerMsg::Result {
                id,
                data: serde_json::to_value(&self.last_slow).unwrap_or_default(),
            },
            Command::Record { on } => match self.on_record(on, now_ms) {
                Ok(r) => ServerMsg::Result {
                    id,
                    data: serde_json::to_value(r).unwrap_or_default(),
                },
                Err(message) => ServerMsg::Error {
                    id,
                    code: ErrorCode::BadRequest,
                    message,
                },
            },
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
            tip_filter: Some(if self.config.ai.filter {
                self.memory.filter_status(now_s * 1000)
            } else {
                "off".into()
            }),
            mode_from_editor: self
                .resolved
                .editor
                .and_then(|id| self.resolver.editor(id)?.mode)
                .is_some(),
            confident: self.tracker.confident(),
            mode_guess: (self.mode_guess != ModeGuess::default()).then_some(self.mode_guess),
            timing: self.timings.summary(),
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

    /// Every tip shown: none held back, so tests see them.
    pub(crate) fn test_config() -> Config {
        let mut c = Config::default();
        c.learn.holdout = 0.0;
        c
    }

    pub(crate) fn test_app(mode: CoachingMode) -> App {
        let state = State {
            mode: Some(mode),
            ..Default::default()
        };
        let mut app = App::new(
            test_config(),
            state,
            test_resolver(),
            Box::new(crate::notify::fake::Recorder::default()),
        );
        app.claude_probe = || false;
        // Tips straight away, as most tests expect; the hold has its own.
        app.hold_ms = 0;
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
        app.on_conn(
            ConnEvent::Msg {
                id,
                msg: Box::new(msg),
            },
            now_ms,
        );
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
                mode: None,
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
        app.on_pointer_click(0);
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
            test_config(),
            state,
            test_resolver(),
            Box::new(recorder.clone()),
        );
        app.claude_probe = || false;
        app.hold_ms = 0;
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
            test_config(),
            state,
            test_resolver(),
            Box::new(recorder.clone()),
        )
        .with_store(store);
        app.claude_probe = || false;
        app.hold_ms = 0;
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
             :e secretfile.txt<CR>/secretsearch<CR>jjjjjjjjjjjjxxxxxdddddd:w<CR>:q<CR>w",
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
        // Only the summary here; the daily tip has its own test.
        app.config.learn.daily = false;
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
                    mode: None,
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
        let beat = |app: &mut App, at| {
            send(
                app,
                2,
                ClientMsg::EditorFocus {
                    focused: true,
                    panel: None,
                },
                at,
            )
        };
        // Editing, then typing in a chat panel: nothing follows the keys.
        beat(&mut app, 900);
        type_at(&mut app, "<Esc>xxxxx<Esc>", 1_000);
        app.tick(3_000);
        // A beat long after (back in the editor) doesn't vouch for them.
        beat(&mut app, 3_500);
        assert!(nudges(&mut ext).is_empty());
        // In the editor: a heartbeat follows the keys.
        type_at(&mut app, "<Esc>xxxxx<Esc>", 10_000);
        beat(&mut app, 10_700);
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
        app.on_pointer_click(0);
        assert_eq!(app.mode_known(), (VimMode::Insert, true));
        // The guess from keys is scored against what the editor says.
        let mode = |app: &mut App, mode| send(app, 2, ClientMsg::EditorMode { mode }, 0);
        keys(&mut app, "<Esc>");
        mode(&mut app, EditorMode::Normal);
        keys(&mut app, "v");
        mode(&mut app, EditorMode::VisualLine);
        // A mapping we didn't know about: `;` entered Insert mode.
        keys(&mut app, "<Esc>;");
        mode(&mut app, EditorMode::Insert);
        let g = app.status(0).mode_guess.unwrap();
        assert_eq!((g.right, g.wrong, g.unsure), (2, 1, 1), "{g:?}");
        assert_eq!(
            g.describe().unwrap(),
            "67% right over 3 mode changes, 1 while unsure"
        );
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
            range: None,
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
                mode: None,
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

    #[test]
    fn clicking_files_open_suggests_quick_open() {
        let mut app = test_app(CoachingMode::Active);
        let mut ext = ext_hello(&mut app, 1);
        let _vs = editor(&mut app, 2, EditorKind::Vscode, 510);
        focus(&mut app, "code_code", 500);
        let opened = |app: &mut App, at| {
            send(
                app,
                2,
                ClientMsg::EditorEvent {
                    event: UiEvent::FileOpened,
                },
                at,
            )
        };
        for at in [10_000, 20_000, 30_000] {
            app.on_pointer_click(at - 200);
            opened(&mut app, at);
        }
        let n = nudges(&mut ext);
        assert_eq!(n.len(), 1, "{n:?}");
        assert_eq!(n[0].rule_id, "vscode.quick-open");
        // With Ctrl-P first it's the better way, even after a click.
        app.on_pointer_click(39_000);
        let ctrl_p = KeyStroke {
            key: Key::ctrl('p'),
            repeat: false,
            at_ms: 39_500,
        };
        app.on_key(ctrl_p, 39_500);
        for at in [40_000, 41_000, 42_000] {
            opened(&mut app, at);
        }
        assert!(nudges(&mut ext).is_empty());
    }

    #[test]
    fn typing_accuracy_is_opt_in_and_counts_only() {
        for on in [false, true] {
            let (mut app, _) = app_with_store(CoachingMode::Active);
            app.config.typing.accuracy = on;
            let _ext = ext_hello(&mut app, 1);
            focus(&mut app, "code_code", 500);
            type_at(
                &mut app,
                "<Esc>isecretpasswrod<BS><BS><BS>ord hunter2<Esc>",
                1_000,
            );
            app.tick(120_000);
            let store = app.memory.store().unwrap();
            let (typed, corrections) = store.typing("1960-01-01", "2100-01-01").unwrap();
            if !on {
                assert_eq!((typed, corrections), (0, 0), "off by default");
                continue;
            }
            assert_eq!((typed, corrections), (25, 3));
            assert_eq!(
                store.typo_pairs("1960-01-01", "2100-01-01", 5).unwrap(),
                [("od".to_string(), 1)]
            );
            for t in store.all_text().unwrap() {
                for secret in ["secret", "pass", "hunter"] {
                    assert!(!t.contains(secret), "{t:?} leaks {secret:?}");
                }
            }
        }
    }

    #[test]
    fn tip_outcomes_are_recorded() {
        use flowbuddy_store::Outcome;
        let (mut app, _) = app_with_store(CoachingMode::Active);
        let _ext = ext_hello(&mut app, 1);
        focus(&mut app, "code_code", 500);
        // A tip, then the better way: adopted.
        type_at(&mut app, "<Esc>xxxxxw", 1_000);
        type_at(&mut app, "dw", 60_000);
        // Another rule's tip, marked not helpful.
        type_at(&mut app, "jjjjjjjw", 120_000);
        send(
            &mut app,
            1,
            ClientMsg::Command {
                id: None,
                command: Command::ReportFalsePositive,
            },
            125_000,
        );
        // A third, left alone for over half an hour: ignored.
        type_at(&mut app, "<Down><Down><Down><Esc>", 200_000);
        app.tick(200_000 + 31 * 60_000);
        let rows = app.memory.store().unwrap().tips(0).unwrap();
        let got: Vec<_> = rows
            .iter()
            .map(|r| (r.rule_id.as_str(), r.outcome))
            .collect();
        assert_eq!(
            got,
            [
                ("vim.repeat-x", Some(Outcome::Adopted)),
                ("vim.repeat-jk", Some(Outcome::NotHelpful)),
                ("vim.arrows-normal", Some(Outcome::Ignored)),
            ]
        );
        assert_eq!(rows[1].prior_today, 1);
        assert_eq!(rows[0].mode, "active");
    }

    #[test]
    fn the_learned_filter_skips_tips_you_dismiss() {
        use flowbuddy_store::{Outcome, TipShown};
        let (mut app, _) = app_with_store(CoachingMode::Active);
        let mut ext = ext_hello(&mut app, 1);
        let now = 1_800_000_000_000u64;
        {
            let store = app.memory.store().unwrap();
            for i in 0..40 {
                for (rule, outcome) in [
                    ("vim.repeat-x", Outcome::Dismissed),
                    ("vim.repeat-jk", Outcome::Adopted),
                ] {
                    let id = store
                        .tip_shown(&TipShown {
                            shown_at: now / 1000 - 86_400 + i * 60,
                            rule_id: rule,
                            context: "vscode-vim",
                            stage: "nudged",
                            hour: 14,
                            mode: "active",
                            prior_today: (i % 3) as u32,
                            held: false,
                        })
                        .unwrap();
                    store.close_tip(id, outcome).unwrap();
                }
            }
        }
        app.memory.maintain(now);
        assert!(
            app.status(now / 1000)
                .tip_filter
                .unwrap()
                .starts_with("on, learned from 80 tips")
        );
        focus(&mut app, "code_code", 500);
        type_at(&mut app, "<Esc>xxxxxw", now);
        type_at(&mut app, "jjjjjjjw", now + 20 * 60_000);
        let shown: Vec<_> = nudges(&mut ext).into_iter().map(|n| n.rule_id).collect();
        assert_eq!(shown, ["vim.repeat-jk"]);
        assert!(
            app.status(now / 1000)
                .tip_filter
                .unwrap()
                .ends_with("1 skipped today")
        );
    }

    #[test]
    fn something_new_once_a_day() {
        use chrono::TimeZone;
        let (mut app, _) = app_with_store(CoachingMode::Balanced);
        let mut ext = ext_hello(&mut app, 1);
        let at = |d, h| {
            chrono::Local
                .with_ymd_and_hms(2026, 10, d, h, 0, 0)
                .unwrap()
                .timestamp_millis() as u64
        };
        // Not while you're elsewhere.
        focus(&mut app, "firefox", 900);
        app.tick(at(20, 10));
        assert!(nudges(&mut ext).is_empty());
        focus(&mut app, "code_code", 500);
        // You already move by words: that's not new to you.
        type_at(&mut app, "<Esc>wwwbbb", at(20, 10));
        app.tick(at(20, 10) + 61_000);
        let n = nudges(&mut ext);
        assert_eq!(n.len(), 1, "{n:?}");
        assert_eq!(n[0].rule_id, "learn.daily");
        assert!(
            n[0].text.starts_with("`e`: jump to the end of the word"),
            "{}",
            n[0].text
        );
        assert!(!n[0].explain.is_empty());
        // Once a day.
        app.tick(at(20, 15));
        assert!(nudges(&mut ext).is_empty());
        // You start using it: learned.
        assert!(
            n[0].text
                .ends_with("Challenge: use it the next time it fits.")
        );
        type_at(&mut app, "<Esc>eee", at(20, 16));
        app.tick(at(20, 16) + 120_000);
        let learned = app.memory.store().unwrap().learn_items().unwrap();
        assert!(learned[0].learned_at.is_some(), "{learned:?}");
        // The challenge, met.
        let n = nudges(&mut ext);
        assert!(n.iter().any(|n| n.rule_id == "learn.challenge"), "{n:?}");
        assert!(app.state.challenge.is_none());
        let outcomes = app.memory.store().unwrap().challenge_outcomes().unwrap();
        assert_eq!(outcomes["word-end"], (1, 0, 0));
        // A habit this week: walking along lines with l. The next day's
        // lesson comes from it (`f` is what that tip teaches next), not
        // from the curriculum's order.
        type_at(&mut app, "<Esc>llllllll<Esc>", at(20, 17));
        app.tick(at(20, 17) + 61_000);
        nudges(&mut ext);
        app.tick(at(21, 10));
        let n = nudges(&mut ext);
        assert!(n[0].text.starts_with("`f<char>` and `;`"), "{}", n[0].text);
    }

    #[test]
    fn reviewed_thresholds_apply_and_restore() {
        let mut app = test_app(CoachingMode::Active);
        let mut rx = connect(&mut app, 1);
        let set = |app: &mut App, threshold| {
            send(
                app,
                1,
                ClientMsg::Command {
                    id: Some(1),
                    command: Command::RuleThreshold {
                        rule_id: "vim.repeat-x".into(),
                        threshold,
                    },
                },
                0,
            )
        };
        set(&mut app, Some(7));
        assert_eq!(rx.try_recv().unwrap(), ServerMsg::Ack { id: Some(1) });
        assert_eq!(threshold(&app, "vim.repeat-x"), Some(7));
        set(&mut app, None);
        assert_eq!(threshold(&app, "vim.repeat-x"), Some(4));
        send(
            &mut app,
            1,
            ClientMsg::Command {
                id: Some(2),
                command: Command::RuleThreshold {
                    rule_id: "vim.w-then-q".into(),
                    threshold: Some(3),
                },
            },
            0,
        );
        rx.try_recv().unwrap();
        assert!(matches!(rx.try_recv().unwrap(), ServerMsg::Error { .. }));
    }

    #[test]
    fn personal_tips_are_added_when_fresh() {
        let dir = std::env::temp_dir().join(format!("flowbuddy-tips-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = test_app(CoachingMode::Active).with_state_path(dir.join("state.json"));
        app.config.ai.tips = true;
        let now = 1_800_000_000_000u64;
        std::fs::write(
            dir.join("tips.json"),
            format!(
                r#"{{"generated_at": {}, "tips": {{"vim.repeat-x": "You did this 30 times; `dw` is your friend."}}}}"#,
                now / 1000 - 3600
            ),
        )
        .unwrap();
        app.tick(now);
        let mut ext = ext_hello(&mut app, 1);
        focus(&mut app, "code_code", 500);
        type_at(&mut app, "<Esc>xxxxxw", now);
        let n = nudges(&mut ext);
        assert!(
            n[0].text
                .ends_with("You did this 30 times; `dw` is your friend."),
            "{}",
            n[0].text
        );
        // Stale: not used.
        app.tick(now + 4 * 86_400_000);
        assert!(app.personal_tips.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn records_a_session_without_text() {
        let dir = std::env::temp_dir().join(format!("flowbuddy-rec-{}", std::process::id()));
        let mut app = test_app(CoachingMode::Active);
        app.recordings = dir.clone();
        focus(&mut app, "code_code", 500);
        let started = app.on_record(Some(true), 1000).unwrap();
        assert!(started.recording);
        type_at(&mut app, "<Esc>jjjjjjjihello there<Esc>", 2000);
        let done = app.on_record(Some(false), 9000).unwrap();
        assert!(!done.recording);
        let text = std::fs::read_to_string(done.path.unwrap()).unwrap();
        assert!(!text.contains("hello") && !text.contains("there"), "{text}");
        let found = session::replay(&session::parse(&text).unwrap());
        assert!(
            found.iter().any(|f| f.rule_id == "vim.repeat-jk"),
            "{found:?}"
        );
        assert!(!app.on_record(None, 9000).unwrap().recording);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn remembers_the_last_slow_move() {
        let mut app = test_app(CoachingMode::Active);
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
        keys(&mut app, "jjjjjjj");
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
        let last = app.last_slow.clone().unwrap();
        assert_eq!((last.kind.as_str(), last.keys_used), ("move", 7));
        assert_eq!(last.best[0], ("}".to_string(), 1));
    }

    #[test]
    fn snippets_go_from_the_editor_to_coach() {
        let mut app = test_app(CoachingMode::Active);
        app.config.text.ai_snippets = true;
        let mut cli = connect(&mut app, 1);
        let mut vim = editor(&mut app, 3, EditorKind::Vim, 210);
        send(
            &mut app,
            3,
            ClientMsg::EditorMode {
                mode: EditorMode::Normal,
            },
            0,
        );
        focus(&mut app, "org.gnome.Terminal", 100);
        // A tip in the editor: a text object instead of moving in.
        keys(&mut app, "llllldw");
        use flowbuddy_core::editor::{Operator, OperatorReport, TextObject};
        let report = OperatorReport {
            op: Operator::Delete,
            at_ms: 700,
            objects: vec![TextObject::InnerWord],
            range: None,
        };
        send(&mut app, 3, ClientMsg::Operator(report), 750);
        assert_eq!(app.recent_tips.len(), 1, "{:?}", app.recent_tips);
        while vim.try_recv().is_ok() {}
        send(
            &mut app,
            1,
            ClientMsg::Command {
                id: Some(9),
                command: Command::Snippets,
            },
            800,
        );
        let ServerMsg::SnippetRequest { id, times } = vim.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(times, [700]);
        while cli.try_recv().is_ok() {}
        let lines = vec!["let x = foo(bar);".to_string()];
        send(
            &mut app,
            3,
            ClientMsg::Snippets {
                id,
                snippets: vec![flowbuddy_core::ipc::Snippet {
                    at_ms: 690,
                    filetype: Some("rust".into()),
                    lines: lines.clone(),
                }],
            },
            900,
        );
        let ServerMsg::Result { id: Some(9), data } = cli.try_recv().unwrap() else {
            panic!()
        };
        let tips: Vec<flowbuddy_core::ipc::TipSnippet> = serde_json::from_value(data).unwrap();
        assert_eq!(tips[0].snippet.lines, lines);
        // Off: an empty answer straight away, and no editor is asked.
        app.config.text.ai_snippets = false;
        send(
            &mut app,
            1,
            ClientMsg::Command {
                id: Some(10),
                command: Command::Snippets,
            },
            1000,
        );
        assert!(vim.try_recv().is_err());
        let ServerMsg::Result { data, .. } = cli.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(data, json!([]));
    }

    #[test]
    fn recent_tips_from_the_history() {
        let (mut app, _) = app_with_store(CoachingMode::Active);
        let mut cli = connect(&mut app, 1);
        app.show(
            "vim.undo-run",
            "Undo further at once",
            "`7u` undoes that many.",
            false,
            1_000_000,
        );
        app.show(
            "vim.repeat-jk",
            "Try a count",
            "Try `7j`.",
            false,
            2_000_000,
        );
        while cli.try_recv().is_ok() {}
        send(
            &mut app,
            1,
            ClientMsg::Command {
                id: Some(4),
                command: Command::RecentTips { limit: Some(5) },
            },
            3_000_000,
        );
        let ServerMsg::Result { data, .. } = cli.try_recv().unwrap() else {
            panic!()
        };
        let tips: Vec<flowbuddy_core::ipc::RecentTip> = serde_json::from_value(data).unwrap();
        assert_eq!(tips.len(), 2);
        assert_eq!(tips[0].title, "Try a count");
        assert!(!tips[1].explain.is_empty());
    }

    #[test]
    fn reselecting_by_hand_or_with_gv() {
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
        let reselected = |app: &mut App, at| {
            send(
                app,
                3,
                ClientMsg::EditorEvent {
                    event: UiEvent::Reselected,
                },
                at,
            )
        };
        // With gv: the better way, no tip.
        type_at(&mut app, "gv", 1_000);
        reselected(&mut app, 2_000);
        reselected(&mut app, 3_000);
        assert!(nudges(&mut ext).is_empty());
        // By hand, twice: gv.
        reselected(&mut app, 100_000);
        reselected(&mut app, 160_000);
        let n = nudges(&mut ext);
        assert_eq!(n.len(), 1, "{n:?}");
        assert_eq!(n[0].rule_id, "vim.reselect");
    }

    #[test]
    fn shells_say_their_vi_mode() {
        let mut app = test_app(CoachingMode::Active);
        let _rx = connect(&mut app, 1);
        focus(&mut app, "org.gnome.Terminal", 100);
        ctx(&mut app, CtxEvent::Prompt, None, 200);
        assert_eq!(app.context().kind, ContextKind::BashVi);
        assert_eq!(app.mode_known(), (VimMode::Insert, true));
        let mode = |app: &mut App, m, pid| {
            send(
                app,
                1,
                ClientMsg::Ctx {
                    event: CtxEvent::Mode,
                    command: None,
                    vi_mode: Some(true),
                    shell_pid: pid,
                    tty: None,
                    shape: None,
                    mode: Some(m),
                },
                0,
            )
        };
        mode(&mut app, EditorMode::Normal, 200);
        assert_eq!(app.mode_known(), (VimMode::Normal, true));
        // Another shell's switch doesn't count here.
        mode(&mut app, EditorMode::Insert, 300);
        assert_eq!(app.mode_known(), (VimMode::Normal, true));
    }

    #[test]
    fn a_guessed_terminal_switch_gives_no_tips() {
        let mut app = test_app(CoachingMode::Active);
        let mut ext = ext_hello(&mut app, 1);
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
        // A shell under VS Code reports: maybe you clicked into the
        // terminal, maybe a background command finished.
        send(
            &mut app,
            1,
            ClientMsg::Ctx {
                event: CtxEvent::Prompt,
                command: None,
                vi_mode: Some(true),
                shell_pid: 510,
                tty: None,
                shape: None,
                mode: None,
            },
            10,
        );
        assert_eq!(app.context().kind, ContextKind::BashVi);
        assert!(!app.mode_known().1, "mode unknown after a guess");
        keys(&mut app, "lllllllll");
        assert!(nudges(&mut ext).is_empty());
    }

    #[test]
    fn key_counts_that_dont_add_up_are_dropped() {
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
        send(
            &mut app,
            2,
            ClientMsg::EditorMode {
                mode: EditorMode::Normal,
            },
            0,
        );
        keys(&mut app, "jjjjjjj");
        let motion = |moves| Motion {
            start_ms: 0,
            end_ms: 700,
            moves,
            mode: EditorMode::Normal,
            lines: 7,
            cols: 0,
            facts: Default::default(),
        };
        assert_eq!(app.keys_for(&motion(7), 2), 7);
        // VS Code saw two moves: five of those keys belong elsewhere.
        assert_eq!(app.keys_for(&motion(2), 2), 0);
        // A click in the middle: the keys didn't make the whole move.
        app.on_pointer_click(300);
        assert_eq!(app.keys_for(&motion(7), 2), 0);
    }

    #[test]
    fn one_tip_when_two_rules_fire_a_moment_apart() {
        let mut app = test_app(CoachingMode::Active);
        app.hold_ms = u64::from(Config::default().coaching.hold_ms);
        let mut ext = ext_hello(&mut app, 1);
        focus(&mut app, "org.gnome.Terminal", 100);
        ctx(&mut app, CtxEvent::Fg, Some("vim"), 200);
        let finding = |rule_id, at_ms| Finding {
            rule_id,
            title: "t",
            text: "Use `.`".into(),
            shape: flowbuddy_core::rule::Shape::fixed("x"),
            mode: VimMode::Normal,
            at_ms,
            start_ms: at_ms,
        };
        // The dot-repeat tip, then the macro tip a second later: the macro
        // ranks higher and is the one shown.
        app.on_rule_events_in(
            ContextKind::TerminalVim,
            &[RuleEvent::Finding(finding("vim.dot-repeat", 1_000))],
            1_000,
            false,
        );
        app.on_rule_events_in(
            ContextKind::TerminalVim,
            &[RuleEvent::Finding(finding("vim.macro", 1_900))],
            1_900,
            false,
        );
        assert!(nudges(&mut ext).is_empty(), "held");
        app.tick(3_000);
        let n = nudges(&mut ext);
        assert_eq!(
            n.iter().map(|n| n.rule_id.as_str()).collect::<Vec<_>>(),
            ["vim.macro"]
        );
    }

    #[test]
    fn a_long_selection_gets_a_shorter_operator() {
        use flowbuddy_core::editor::{Operator, OperatorRange, OperatorReport};
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
        type_at(&mut app, "Vjjjjjjjjjjd", 1_000);
        let report = OperatorReport {
            op: Operator::Delete,
            at_ms: 2_200,
            objects: vec![],
            range: Some(Box::new(OperatorRange {
                linewise: true,
                lines: 10,
                facts: flowbuddy_core::editor::MotionFacts {
                    paragraph_hops: Some(1),
                    ..Default::default()
                },
                ..Default::default()
            })),
        };
        send(&mut app, 3, ClientMsg::Operator(report), 2_250);
        let n = nudges(&mut ext);
        assert!(
            n.iter()
                .any(|n| n.rule_id == "motion.operator" && n.text.contains("`d}`")),
            "{n:?}"
        );
    }

    #[test]
    fn watchers_get_tips_too() {
        let (mut app, notes) = app_with_store(CoachingMode::Active);
        let mut watch = connect(&mut app, 2);
        send(&mut app, 2, hello(ClientKind::Watch), 0);
        // Without the extension: a notification, and the watcher too.
        app.show("vim.repeat-jk", "Try a count", "Try `7j`.", false, 1_000);
        assert_eq!(nudges(&mut watch).len(), 1);
        assert_eq!(notes.0.borrow().len(), 1);
        // With it: the extension and the watcher, no notification.
        let mut ext = ext_hello(&mut app, 1);
        app.show("vim.repeat-jk", "Try a count", "Try `7j`.", false, 2_000);
        assert_eq!(nudges(&mut ext).len(), 1);
        assert_eq!(nudges(&mut watch).len(), 1);
        assert_eq!(notes.0.borrow().len(), 1);
    }

    #[test]
    fn tips_wait_for_a_pause_when_asked() {
        let mut app = test_app(CoachingMode::Active);
        app.config.coaching.when = config::TipTiming::Pause;
        let mut ext = ext_hello(&mut app, 1);
        focus(&mut app, "org.gnome.Terminal", 100);
        ctx(&mut app, CtxEvent::Fg, Some("vim"), 200);
        // jjjjjjj then more typing: the tip waits.
        type_at(&mut app, "<Esc>jjjjjjjxw", 10_000);
        app.tick(11_200);
        assert!(nudges(&mut ext).is_empty(), "still typing");
        app.tick(13_000);
        let n = nudges(&mut ext);
        assert_eq!(n.len(), 1, "{n:?}");
    }

    #[test]
    fn a_rolling_recording_with_feedback() {
        let dir = std::env::temp_dir().join(format!("flowbuddy-roll-{}", std::process::id()));
        let mut app = test_app(CoachingMode::Active);
        app.recordings = dir.clone();
        app.config.record.keep_days = 3;
        let mut ext = ext_hello(&mut app, 1);
        app.tick(1_000);
        assert!(app.on_record(None, 1_000).unwrap().recording);
        app.show("vim.repeat-jk", "Try a count", "Try `7j`.", false, 2_000);
        let id = nudges(&mut ext)[0].nudge_id.clone();
        send(
            &mut app,
            1,
            ClientMsg::NudgeResponse {
                nudge_id: id,
                action: NudgeAction::FalsePositive,
            },
            3_000,
        );
        let path = app.on_record(None, 3_000).unwrap().path.unwrap();
        app.recorder.as_mut().unwrap().flush();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(path.contains("/auto-"), "{path}");
        assert!(
            text.contains(r#""what":"shown""#) && text.contains(r#""what":"false_positive""#),
            "{text}"
        );
        // Turned off: it stops.
        app.config.record.keep_days = 0;
        app.tick(4_000);
        assert!(!app.on_record(None, 4_000).unwrap().recording);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn held_back_tips_are_recorded_not_shown() {
        let (mut app, _) = app_with_store(CoachingMode::Active);
        let mut ext = ext_hello(&mut app, 1);
        focus(&mut app, "code_code", 500);
        app.config.learn.holdout = 1.0;
        type_at(&mut app, "<Esc>jjjjjjjjjw", 1_000_000);
        app.tick(1_000_000 + 61_000);
        let n = nudges(&mut ext);
        assert!(n.iter().all(|n| n.rule_id == "learn.daily"), "{n:?}");
        let rows = app.memory.store().unwrap().tips(0).unwrap();
        assert!(!rows.is_empty() && rows.iter().all(|r| r.held), "{rows:?}");
        assert!(!held_back(0.0, "vim.repeat-jk", 1));
    }

    #[test]
    fn a_challenge_stays_open_as_long_as_you_say() {
        let (mut app, _) = app_with_store(CoachingMode::Active);
        app.config.learn.challenge_days = 1;
        app.state.challenge = Some(config::Challenge {
            item: "find".into(),
            set_at: 0,
        });
        app.tick(86_400_000 / 2);
        assert!(app.state.challenge.is_some(), "half a day: still open");
        app.tick(86_400_000 + 61_000);
        assert!(app.state.challenge.is_none());
        let outcomes = app.memory.store().unwrap().challenge_outcomes().unwrap();
        assert_eq!(outcomes["find"], (0, 0, 1));
    }

    #[test]
    fn a_challenge_missed_is_evidence_against() {
        let (mut app, _) = app_with_store(CoachingMode::Active);
        let _ext = ext_hello(&mut app, 1);
        focus(&mut app, "code_code", 500);
        app.state.challenge = Some(config::Challenge {
            item: "find".into(),
            set_at: 1_000,
        });
        // Walking along the line with l, where f would have done.
        type_at(&mut app, "<Esc>llllllllll<Esc>", 1_000_000);
        app.tick(1_000_000 + 61_000);
        assert!(app.state.challenge.is_none());
        let store = app.memory.store().unwrap();
        assert_eq!(store.challenge_outcomes().unwrap()["find"], (0, 1, 0));
        let today = memory::day(1_000_000);
        let (skills, _) = store.skills(&today, None).unwrap();
        assert_eq!(skills["find"].wrong, 1.0, "{:?}", skills["find"]);
    }

    #[test]
    fn progress_is_kept_per_environment() {
        let (mut app, _) = app_with_store(CoachingMode::Active);
        let _ext = ext_hello(&mut app, 1);
        focus(&mut app, "code_code", 500);
        type_at(&mut app, "<Esc>ciwx<Esc>ciwx<Esc>ciwx<Esc>", 1_000_000);
        app.tick(1_000_000 + 61_000);
        let now = 1_000_000 + 62_000;
        assert!(
            app.memory
                .known(Some(ContextKind::VsCodeVim), now)
                .knows("ciw")
        );
        assert!(
            !app.memory
                .known(Some(ContextKind::TerminalVim), now)
                .knows("ciw")
        );
        assert!(app.memory.known(None, now).knows("ciw"));
    }

    #[test]
    fn saying_what_you_know() {
        let (mut app, _) = app_with_store(CoachingMode::Active);
        let mut cli = connect(&mut app, 1);
        let know = |app: &mut App, items: &[&str], known| {
            send(
                app,
                1,
                ClientMsg::Command {
                    id: Some(1),
                    command: Command::KnowItems {
                        items: items.iter().map(|s| s.to_string()).collect(),
                        known,
                    },
                },
                1_000,
            )
        };
        know(&mut app, &["ci-paren"], Some(true));
        assert!(app.engine.known().knows("ciw"));
        know(&mut app, &["ciw"], Some(false));
        assert!(!app.engine.known().knows("ciw"));
        know(&mut app, &["ciw", "ci-paren"], None);
        assert!(!app.engine.known().knows("ci-paren"));
        while cli.try_recv().is_ok() {}
        know(&mut app, &["nonsense"], Some(true));
        assert!(matches!(cli.try_recv().unwrap(), ServerMsg::Error { .. }));
    }
}
