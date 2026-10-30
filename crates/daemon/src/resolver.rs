//! Combine focus and shell reports into one [`ContextKind`].
//!
//! See docs/architecture.md#resolving-the-context.

use std::collections::HashMap;

use flowbuddy_core::context::{ContextKind, VimMode};
use flowbuddy_core::editor::EditorKind;
use flowbuddy_core::ipc::Panel;

/// App ids and command names that select each context.
#[derive(Clone, Debug)]
pub struct AppIds {
    pub vscode: Vec<String>,
    pub terminals: Vec<String>,
    /// Window ids of Vim in a window of its own (gVim, Neovide).
    pub vim_windows: Vec<String>,
    pub vims: Vec<String>,
    /// Command names of Claude Code.
    pub claudes: Vec<String>,
}

impl Default for AppIds {
    fn default() -> Self {
        use flowbuddy_core::config::{CLAUDES, TERMINALS, VIM_WINDOWS, VIMS, VSCODES};
        let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
        AppIds {
            vscode: s(VSCODES),
            terminals: s(TERMINALS),
            vim_windows: s(VIM_WINDOWS),
            vims: s(VIMS),
            claudes: s(CLAUDES),
        }
    }
}

/// Normalize `org.gnome.Terminal.desktop` → `org.gnome.terminal`.
fn norm(id: &str) -> String {
    id.trim_end_matches(".desktop").to_ascii_lowercase()
}

fn contains(list: &[String], id: &str) -> bool {
    let id = norm(id);
    list.iter().any(|x| norm(x) == id)
}

/// Read-only view of the process tree, injectable for tests.
pub trait ProcessTree {
    fn parent(&self, pid: u32) -> Option<u32>;
    fn alive(&self, pid: u32) -> bool;
}

pub struct Procfs;

impl ProcessTree for Procfs {
    fn parent(&self, pid: u32) -> Option<u32> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // "pid (comm) state ppid ...": comm may contain spaces and parens.
        let after = &stat[stat.rfind(')')? + 1..];
        after.split_whitespace().nth(1)?.parse().ok()
    }

    fn alive(&self, pid: u32) -> bool {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellState {
    /// A command (basename only) is in the foreground.
    Fg(String),
    Prompt {
        vi: bool,
    },
}

struct Shell {
    state: ShellState,
    updated_ms: u64,
    /// The shell's terminal, to find its tmux client.
    tty: Option<String>,
}

/// What tmux says about the pane a client is showing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pane {
    /// The pane's shell.
    pub pid: u32,
    /// What runs in the pane right now, e.g. `vim` (tmux's
    /// `pane_current_command`).
    pub command: String,
}

/// Ask tmux which pane the client on a terminal shows; injectable for tests.
pub trait Tmux {
    fn active_pane(&self, client_tty: &str) -> Option<Pane>;
}

/// No tmux: what tests and machines without it get.
pub struct NoTmux;

impl Tmux for NoTmux {
    fn active_pane(&self, _client_tty: &str) -> Option<Pane> {
        None
    }
}

/// The real tmux, over its Unix socket. Only the pane's pid and command
/// name are read.
pub struct TmuxCli;

impl Tmux for TmuxCli {
    fn active_pane(&self, client_tty: &str) -> Option<Pane> {
        let out = std::process::Command::new("tmux")
            .args([
                "list-clients",
                "-F",
                "#{client_tty} #{pane_pid} #{pane_current_command}",
            ])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        parse_clients(&String::from_utf8_lossy(&out.stdout), client_tty)
    }
}

/// The pane for `client_tty` in `tmux list-clients` output.
pub fn parse_clients(out: &str, client_tty: &str) -> Option<Pane> {
    out.lines().find_map(|line| {
        let mut parts = line.splitn(3, ' ');
        let tty = parts.next()?;
        let pid = parts.next()?.parse().ok()?;
        let command = parts.next()?.trim().to_string();
        (tty == client_tty).then_some(Pane { pid, command })
    })
}

#[derive(Clone, Debug, Default)]
struct Focus {
    app_id: Option<String>,
    pid: Option<u32>,
}

/// An editor plugin connected to the daemon (VS Code companion, Vim plugin).
#[derive(Clone, Debug)]
pub struct Editor {
    pub kind: EditorKind,
    pub pid: Option<u32>,
    /// Whether a text editor has focus; `None` until it says.
    pub focused: Option<bool>,
    /// When it last said it was focused (VS Code sends this as a heartbeat
    /// while the editor is in use).
    pub focused_ms: u64,
    /// When it last sent anything.
    pub active_ms: u64,
    /// The mode it last reported.
    pub mode: Option<VimMode>,
    /// What has focus instead of the text editor, if it said.
    pub panel: Option<Panel>,
}

/// Connection id of an editor client.
pub type EditorId = u64;

/// The resolved context and what caused the latest change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub kind: ContextKind,
    /// The shell the context comes from, for terminal contexts.
    pub shell_pid: Option<u32>,
    /// The editor plugin the context comes from, when one is reporting.
    pub editor: Option<EditorId>,
}

impl Resolved {
    const UNSUPPORTED: Resolved = Resolved {
        kind: ContextKind::Unsupported,
        shell_pid: None,
        editor: None,
    };
}

pub struct Resolver {
    ids: AppIds,
    procs: Box<dyn ProcessTree>,
    focus: Focus,
    shells: HashMap<u32, Shell>,
    editors: HashMap<EditorId, Editor>,
    /// Claude Code's Vim mode is on (its settings say so).
    claude_vim: bool,
    tmux: Box<dyn Tmux>,
}

const VIMS: [EditorKind; 2] = [EditorKind::Vim, EditorKind::Nvim];

/// Walking more parents than this means something is odd; give up.
const MAX_DEPTH: usize = 64;

impl Resolver {
    pub fn new(ids: AppIds, procs: Box<dyn ProcessTree>) -> Self {
        Resolver {
            ids,
            procs,
            focus: Focus::default(),
            shells: HashMap::new(),
            editors: HashMap::new(),
            claude_vim: false,
            tmux: Box::new(NoTmux),
        }
    }

    pub fn set_ids(&mut self, ids: AppIds) {
        self.ids = ids;
    }

    pub fn set_focus(&mut self, app_id: Option<String>, pid: Option<u32>) {
        self.focus = Focus { app_id, pid };
    }

    pub fn set_claude_vim(&mut self, on: bool) {
        self.claude_vim = on;
    }

    /// Whether VS Code's terminal is the context only by inference (a shell
    /// spoke after the editor did), not because the companion said so.
    pub fn terminal_guessed(&self) -> bool {
        let Some(app_id) = self.focus.app_id.as_deref() else {
            return false;
        };
        contains(&self.ids.vscode, app_id)
            && self
                .editor_in(self.focus.pid, &[EditorKind::Vscode])
                .is_some_and(|(_, e)| {
                    e.focused != Some(false)
                        && self
                            .focus
                            .pid
                            .is_some_and(|w| self.terminal_spoke_since(w, e.focused_ms))
                })
    }

    /// Whether a shell or a Vim plugin under `window_pid` reported after
    /// `since_ms`.
    fn terminal_spoke_since(&self, window_pid: u32, since_ms: u64) -> bool {
        self.shells
            .iter()
            .any(|(pid, s)| s.updated_ms > since_ms && self.descends_from(*pid, window_pid))
            || self.editors.values().any(|e| {
                VIMS.contains(&e.kind)
                    && e.active_ms > since_ms
                    && e.pid.is_some_and(|p| self.descends_from(p, window_pid))
            })
    }

    pub fn focused_app(&self) -> Option<&str> {
        self.focus.app_id.as_deref()
    }

    /// The shell did something (its vi-mode switched): it's in use.
    pub fn shell_active(&mut self, shell_pid: u32, now_ms: u64) {
        if let Some(s) = self.shells.get_mut(&shell_pid) {
            s.updated_ms = now_ms;
        }
    }

    pub fn shell_report(
        &mut self,
        shell_pid: u32,
        state: ShellState,
        tty: Option<String>,
        now_ms: u64,
    ) {
        self.shells.insert(
            shell_pid,
            Shell {
                state,
                updated_ms: now_ms,
                tty,
            },
        );
    }

    pub fn with_tmux(mut self, tmux: Box<dyn Tmux>) -> Self {
        self.tmux = tmux;
        self
    }

    /// Whether the context comes through tmux, which can change without
    /// telling anyone: worth resolving again now and then.
    pub fn via_tmux(&self) -> bool {
        self.shells
            .values()
            .any(|s| matches!(&s.state, ShellState::Fg(c) if c == "tmux"))
    }

    /// The context of the pane a tmux client on `tty` shows.
    fn tmux_pane(&self, tty: &str) -> Option<Resolved> {
        let pane = self.tmux.active_pane(tty)?;
        let resolved = |kind| Resolved {
            kind,
            shell_pid: Some(pane.pid),
            editor: None,
        };
        if self.ids.vims.contains(&pane.command) {
            return Some(resolved(ContextKind::TerminalVim));
        }
        if self.claude_vim && self.ids.claudes.contains(&pane.command) {
            return Some(resolved(ContextKind::ClaudeVim));
        }
        // The pane's shell at its prompt, if its hook reported.
        match &self.shells.get(&pane.pid)?.state {
            ShellState::Prompt { vi: true } => Some(resolved(ContextKind::BashVi)),
            _ => None,
        }
    }

    pub fn editor_hello(&mut self, id: EditorId, kind: EditorKind, pid: Option<u32>, now_ms: u64) {
        self.editors.insert(
            id,
            Editor {
                kind,
                pid,
                focused: None,
                focused_ms: 0,
                active_ms: now_ms,
                mode: None,
                panel: None,
            },
        );
    }

    pub fn editor_focus(&mut self, id: EditorId, focused: bool, panel: Option<Panel>, now_ms: u64) {
        if let Some(e) = self.editors.get_mut(&id) {
            e.focused = Some(focused);
            e.panel = if focused { None } else { panel };
            e.active_ms = now_ms;
            if focused {
                e.focused_ms = now_ms;
            }
        }
    }

    pub fn editor_mode(&mut self, id: EditorId, mode: Option<VimMode>, now_ms: u64) {
        if let Some(e) = self.editors.get_mut(&id) {
            e.mode = mode;
            e.active_ms = now_ms;
        }
    }

    pub fn editor_gone(&mut self, id: EditorId) {
        self.editors.remove(&id);
    }

    pub fn editor(&self, id: EditorId) -> Option<&Editor> {
        self.editors.get(&id)
    }

    /// The most recently focused editor of `kinds` running inside the
    /// window `window_pid` (any, if the window's pid is unknown).
    fn editor_in(
        &self,
        window_pid: Option<u32>,
        kinds: &[EditorKind],
    ) -> Option<(EditorId, &Editor)> {
        self.editors
            .iter()
            .filter(|(_, e)| kinds.contains(&e.kind))
            .filter(|(_, e)| match (e.pid, window_pid) {
                (Some(pid), Some(window)) => self.descends_from(pid, window),
                _ => true,
            })
            .max_by_key(|(_, e)| (e.focused_ms, e.active_ms))
            .map(|(id, e)| (*id, e))
    }

    /// Forget shells whose process is gone.
    pub fn prune(&mut self) {
        let procs = &self.procs;
        self.shells.retain(|pid, _| procs.alive(*pid));
    }

    fn descends_from(&self, mut pid: u32, ancestor: u32) -> bool {
        for _ in 0..MAX_DEPTH {
            if pid == ancestor {
                return true;
            }
            match self.procs.parent(pid) {
                Some(p) if p != 0 && p != pid => pid = p,
                _ => return false,
            }
        }
        false
    }

    pub fn resolve(&self) -> Resolved {
        let Some(app_id) = self.focus.app_id.as_deref() else {
            return Resolved::UNSUPPORTED;
        };
        if contains(&self.ids.vscode, app_id) {
            // The companion knows whether the editor or a panel has focus.
            return match self.editor_in(self.focus.pid, &[EditorKind::Vscode]) {
                // The integrated terminal: whatever runs there, as in any
                // terminal (Vim, a vi-mode shell, Claude Code, tmux).
                Some((_, e)) if e.focused == Some(false) && e.panel == Some(Panel::Terminal) => {
                    self.focus
                        .pid
                        .map_or(Resolved::UNSUPPORTED, |w| self.in_terminal(w))
                }
                Some((_, e)) if e.focused == Some(false) => Resolved::UNSUPPORTED,
                // Clicking into a terminal that's already open says nothing,
                // but if a shell or Vim in its terminal spoke after the
                // editor last did, that's where you are.
                Some((_, e))
                    if self
                        .focus
                        .pid
                        .is_some_and(|w| self.terminal_spoke_since(w, e.focused_ms)) =>
                {
                    self.focus
                        .pid
                        .map_or(Resolved::UNSUPPORTED, |w| self.in_terminal(w))
                }
                editor => Resolved {
                    kind: ContextKind::VsCodeVim,
                    shell_pid: None,
                    editor: editor.map(|(id, _)| id),
                },
            };
        }
        // Vim's own window: its plugin, if it has one, says the mode.
        if contains(&self.ids.vim_windows, app_id) {
            return Resolved {
                kind: ContextKind::TerminalVim,
                shell_pid: None,
                editor: self.editor_in(self.focus.pid, &VIMS).map(|(id, _)| id),
            };
        }
        if !contains(&self.ids.terminals, app_id) {
            return Resolved::UNSUPPORTED;
        }
        let Some(window_pid) = self.focus.pid else {
            return Resolved::UNSUPPORTED;
        };
        self.in_terminal(window_pid)
    }

    /// What has focus in a terminal window whose process is `window_pid`.
    fn in_terminal(&self, window_pid: u32) -> Resolved {
        // Terminals like GNOME Terminal serve every window from one process,
        // so several shells may match: take the one that reported last.
        let shell = self
            .shells
            .iter()
            .filter(|(pid, _)| self.descends_from(**pid, window_pid))
            .max_by_key(|(_, s)| s.updated_ms);
        let vim = |id: EditorId| Resolved {
            kind: ContextKind::TerminalVim,
            shell_pid: None,
            editor: Some(id),
        };
        // A Vim plugin inside this terminal, wherever Vim was started from
        // (`git commit`, a script). With focus events it says whether it has
        // focus; without, it wins if it was active after the last shell.
        if let Some((id, _)) = self.editor_in(Some(window_pid), &VIMS).filter(|(_, e)| {
            e.pid.is_some()
                && match e.focused {
                    Some(f) => f,
                    None => shell.is_none_or(|(_, s)| e.active_ms > s.updated_ms),
                }
        }) {
            return vim(id);
        }
        // Vim inside tmux or ssh isn't a child of the terminal. If a Vim
        // plugin says it has focus, trust it.
        let focused_vim = || {
            self.editors
                .iter()
                .filter(|(_, e)| VIMS.contains(&e.kind) && e.focused == Some(true))
                .max_by_key(|(_, e)| e.focused_ms)
                .map(|(id, _)| vim(*id))
        };
        let Some((&pid, shell)) = shell else {
            return focused_vim().unwrap_or(Resolved::UNSUPPORTED);
        };
        let kind = match &shell.state {
            ShellState::Fg(cmd) if self.ids.vims.iter().any(|v| v == cmd) => {
                ContextKind::TerminalVim
            }
            ShellState::Prompt { vi: true } => ContextKind::BashVi,
            ShellState::Fg(cmd) if self.claude_vim && self.ids.claudes.contains(cmd) => {
                ContextKind::ClaudeVim
            }
            // tmux knows what its active pane is running.
            ShellState::Fg(cmd) if cmd == "tmux" => {
                return shell
                    .tty
                    .as_deref()
                    .and_then(|tty| self.tmux_pane(tty))
                    .or_else(focused_vim)
                    .unwrap_or(Resolved::UNSUPPORTED);
            }
            ShellState::Fg(_) => return focused_vim().unwrap_or(Resolved::UNSUPPORTED),
            _ => ContextKind::Unsupported,
        };
        Resolved {
            kind,
            shell_pid: Some(pid),
            editor: None,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A fake process tree: (pid, parent) pairs.
    pub(crate) struct FakeTree(pub Vec<(u32, u32)>);

    impl ProcessTree for FakeTree {
        fn parent(&self, pid: u32) -> Option<u32> {
            self.0.iter().find(|(p, _)| *p == pid).map(|(_, pp)| *pp)
        }
        fn alive(&self, pid: u32) -> bool {
            self.0.iter().any(|(p, _)| *p == pid)
        }
    }

    /// Terminal server 100 with shells 200 and 300, Vim 210 under shell
    /// 200; VS Code 500 with its extension host 510; Vim 950 inside a tmux
    /// server 940 that isn't under the terminal.
    pub(crate) fn tree() -> Box<FakeTree> {
        Box::new(FakeTree(vec![
            (100, 1),
            (200, 100),
            (210, 200),
            (300, 100),
            (500, 1),
            (510, 500),
            (520, 510),
            (900, 1),
            (940, 1),
            (950, 940),
        ]))
    }

    fn resolver() -> Resolver {
        Resolver::new(AppIds::default(), tree())
    }

    fn kind(r: &Resolver) -> ContextKind {
        r.resolve().kind
    }

    #[test]
    fn nothing_focused_is_unsupported() {
        assert_eq!(kind(&resolver()), ContextKind::Unsupported);
    }

    #[test]
    fn vscode_snap_and_desktop_ids() {
        let mut r = resolver();
        for id in ["code_code", "code.desktop", "Code"] {
            r.set_focus(Some(id.into()), Some(500));
            assert_eq!(kind(&r), ContextKind::VsCodeVim, "{id}");
        }
    }

    #[test]
    fn unknown_app_is_unsupported() {
        let mut r = resolver();
        r.set_focus(Some("firefox".into()), Some(900));
        assert_eq!(kind(&r), ContextKind::Unsupported);
    }

    #[test]
    fn terminal_follows_its_shell() {
        let mut r = resolver();
        r.set_focus(Some("org.gnome.Terminal.desktop".into()), Some(100));
        assert_eq!(kind(&r), ContextKind::Unsupported, "no shell report yet");

        r.shell_report_t(200, ShellState::Prompt { vi: true }, 1);
        assert_eq!(kind(&r), ContextKind::BashVi);

        r.shell_report_t(200, ShellState::Fg("vim".into()), 2);
        assert_eq!(kind(&r), ContextKind::TerminalVim);

        r.shell_report_t(200, ShellState::Fg("git".into()), 3);
        assert_eq!(kind(&r), ContextKind::Unsupported);

        r.shell_report_t(200, ShellState::Prompt { vi: false }, 4);
        assert_eq!(kind(&r), ContextKind::Unsupported, "emacs-mode bash");
    }

    #[test]
    fn most_recent_shell_wins_within_one_terminal_server() {
        let mut r = resolver();
        r.set_focus(Some("org.gnome.Terminal".into()), Some(100));
        r.shell_report_t(200, ShellState::Fg("vim".into()), 1);
        r.shell_report_t(300, ShellState::Prompt { vi: true }, 2);
        assert_eq!(r.resolve().shell_pid, Some(300));
        assert_eq!(kind(&r), ContextKind::BashVi);
    }

    #[test]
    fn shells_from_other_windows_are_ignored() {
        let mut r = resolver();
        r.shell_report_t(200, ShellState::Fg("vim".into()), 1);
        r.set_focus(Some("kitty".into()), Some(900));
        assert_eq!(kind(&r), ContextKind::Unsupported);
    }

    #[test]
    fn dead_shells_are_pruned() {
        let mut r = resolver();
        r.shell_report_t(4242, ShellState::Prompt { vi: true }, 1);
        r.prune();
        assert!(r.shells.is_empty());
    }

    #[test]
    fn procfs_parent_of_self() {
        let me = std::process::id();
        assert!(Procfs.parent(me).is_some());
        assert!(Procfs.alive(me));
    }

    #[test]
    fn vscode_companion_decides_editor_or_panel() {
        let mut r = resolver();
        r.set_focus(Some("code_code".into()), Some(500));
        r.editor_hello(7, EditorKind::Vscode, Some(510), 1);
        assert_eq!(r.resolve().editor, Some(7));
        r.editor_focus(7, false, None, 2);
        assert_eq!(kind(&r), ContextKind::Unsupported);
        r.editor_focus(7, true, None, 3);
        assert_eq!(kind(&r), ContextKind::VsCodeVim);
        r.editor_gone(7);
        assert_eq!(r.resolve().editor, None);
        assert_eq!(kind(&r), ContextKind::VsCodeVim);
    }

    #[test]
    fn vim_plugin_in_the_terminal_wins_over_the_shell() {
        let mut r = resolver();
        r.set_focus(Some("org.gnome.Terminal".into()), Some(100));
        // Started by `git commit`: the shell only knows `git` is running.
        r.shell_report_t(200, ShellState::Fg("git".into()), 1);
        assert_eq!(kind(&r), ContextKind::Unsupported);
        r.editor_hello(8, EditorKind::Vim, Some(210), 2);
        assert_eq!(kind(&r), ContextKind::TerminalVim);
        assert_eq!(r.resolve().editor, Some(8));
        // Another window's shell drew a prompt more recently.
        r.shell_report_t(300, ShellState::Prompt { vi: true }, 3);
        assert_eq!(kind(&r), ContextKind::BashVi);
        // Focus events settle it.
        r.editor_focus(8, true, None, 4);
        assert_eq!(kind(&r), ContextKind::TerminalVim);
        r.editor_focus(8, false, None, 5);
        assert_eq!(kind(&r), ContextKind::BashVi);
    }

    #[test]
    fn vim_in_its_own_window() {
        let mut r = resolver();
        for id in ["gvim", "Gvim", "neovide", "nvim-qt"] {
            r.set_focus(Some(id.into()), Some(900));
            assert_eq!(kind(&r), ContextKind::TerminalVim, "{id}");
            assert_eq!(r.resolve().editor, None);
        }
        // Neovide runs nvim as its child; the plugin there reports.
        r.set_focus(Some("neovide".into()), Some(940));
        r.editor_hello(8, EditorKind::Nvim, Some(950), 1);
        assert_eq!(r.resolve().editor, Some(8));
    }

    #[test]
    fn vim_in_tmux_counts_when_it_has_focus() {
        let mut r = resolver();
        r.set_focus(Some("org.gnome.Terminal".into()), Some(100));
        r.shell_report_t(200, ShellState::Fg("tmux".into()), 1);
        r.editor_hello(9, EditorKind::Nvim, Some(950), 2);
        assert_eq!(kind(&r), ContextKind::Unsupported);
        r.editor_focus(9, true, None, 3);
        assert_eq!(kind(&r), ContextKind::TerminalVim);
        // Not while the terminal isn't focused.
        r.set_focus(Some("firefox".into()), Some(900));
        assert_eq!(kind(&r), ContextKind::Unsupported);
    }

    #[test]
    fn claude_code_in_a_terminal() {
        let mut r = resolver();
        r.set_focus(Some("org.gnome.Ptyxis".into()), Some(100));
        r.shell_report_t(200, ShellState::Fg("claude".into()), 1);
        assert_eq!(kind(&r), ContextKind::Unsupported, "Vim mode off");
        r.set_claude_vim(true);
        assert_eq!(kind(&r), ContextKind::ClaudeVim);
    }

    #[test]
    fn claude_code_in_the_vscode_terminal() {
        let mut r = resolver();
        r.set_claude_vim(true);
        r.set_focus(Some("code_code".into()), Some(500));
        r.editor_hello(7, EditorKind::Vscode, Some(510), 1);
        // A shell in VS Code's terminal (under the window's process).
        r.shell_report_t(510, ShellState::Fg("claude".into()), 2);
        r.editor_focus(7, false, Some(Panel::Other), 3);
        assert_eq!(
            kind(&r),
            ContextKind::Unsupported,
            "a chat panel isn't the terminal"
        );
        r.editor_focus(7, false, Some(Panel::Terminal), 4);
        assert_eq!(kind(&r), ContextKind::ClaudeVim);
        r.shell_report_t(510, ShellState::Prompt { vi: false }, 5);
        assert_eq!(kind(&r), ContextKind::Unsupported, "claude exited");
    }

    #[test]
    fn vim_and_vi_mode_in_the_vscode_terminal() {
        let mut r = resolver();
        r.set_focus(Some("code_code".into()), Some(500));
        r.editor_hello(7, EditorKind::Vscode, Some(510), 1);
        r.editor_focus(7, false, Some(Panel::Terminal), 2);
        // A vi-mode shell in the terminal.
        r.shell_report_t(510, ShellState::Prompt { vi: true }, 3);
        assert_eq!(kind(&r), ContextKind::BashVi);
        // Vim started there, without its plugin: from the shell hook.
        r.shell_report_t(510, ShellState::Fg("vim".into()), 4);
        assert_eq!(kind(&r), ContextKind::TerminalVim);
        // With its plugin, the plugin is the context.
        r.editor_hello(8, EditorKind::Vim, Some(520), 5);
        let res = r.resolve();
        assert_eq!((res.kind, res.editor), (ContextKind::TerminalVim, Some(8)));
        // Back in the editor: VS Code again.
        r.editor_focus(7, true, None, 6);
        assert_eq!(kind(&r), ContextKind::VsCodeVim);
    }

    #[test]
    fn typing_in_an_open_vscode_terminal() {
        let mut r = resolver();
        r.set_focus(Some("code_code".into()), Some(500));
        r.editor_hello(7, EditorKind::Vscode, Some(510), 1);
        r.editor_focus(7, true, None, 2);
        assert_eq!(kind(&r), ContextKind::VsCodeVim);
        // You clicked into the terminal (no event) and a prompt came back.
        r.shell_report_t(510, ShellState::Prompt { vi: true }, 3);
        assert_eq!(kind(&r), ContextKind::BashVi);
        // Typing in the editor again sends a heartbeat.
        r.editor_focus(7, true, None, 4);
        assert_eq!(kind(&r), ContextKind::VsCodeVim);
    }

    impl Resolver {
        /// `shell_report` without a tty, for tests.
        pub(crate) fn shell_report_t(&mut self, pid: u32, state: ShellState, now_ms: u64) {
            self.shell_report(pid, state, None, now_ms);
        }
    }

    struct FakeTmux(Pane);

    impl Tmux for FakeTmux {
        fn active_pane(&self, client_tty: &str) -> Option<Pane> {
            (client_tty == "/dev/pts/4").then(|| self.0.clone())
        }
    }

    #[test]
    fn tmux_pane_decides() {
        let pane = |pid, command: &str| {
            Box::new(FakeTmux(Pane {
                pid,
                command: command.into(),
            }))
        };
        let mut r = resolver().with_tmux(pane(950, "vim"));
        r.set_focus(Some("org.gnome.Ptyxis".into()), Some(100));
        r.shell_report(
            200,
            ShellState::Fg("tmux".into()),
            Some("/dev/pts/4".into()),
            1,
        );
        assert_eq!(kind(&r), ContextKind::TerminalVim);
        assert!(r.via_tmux());

        // A shell pane at its vi-mode prompt (its hook reported).
        let mut r = resolver().with_tmux(pane(940, "bash"));
        r.set_focus(Some("org.gnome.Ptyxis".into()), Some(100));
        r.shell_report(
            200,
            ShellState::Fg("tmux".into()),
            Some("/dev/pts/4".into()),
            1,
        );
        assert_eq!(
            kind(&r),
            ContextKind::Unsupported,
            "no report from the pane yet"
        );
        r.shell_report(940, ShellState::Prompt { vi: true }, None, 2);
        assert_eq!(kind(&r), ContextKind::BashVi);
    }

    #[test]
    fn parses_tmux_clients() {
        let out = "/dev/pts/2 3100 bash\n/dev/pts/4 3200 nvim\n";
        assert_eq!(
            parse_clients(out, "/dev/pts/4"),
            Some(Pane {
                pid: 3200,
                command: "nvim".into()
            })
        );
        assert_eq!(parse_clients(out, "/dev/pts/9"), None);
        assert_eq!(parse_clients("garbage", "/dev/pts/4"), None);
    }

    #[test]
    fn vim_over_ssh_counts_when_it_has_focus() {
        let mut r = resolver();
        r.set_focus(Some("org.gnome.Ptyxis".into()), Some(100));
        r.shell_report_t(200, ShellState::Fg("ssh".into()), 1);
        // The remote plugin sends no pid.
        r.editor_hello(11, EditorKind::Vim, None, 2);
        assert_eq!(
            kind(&r),
            ContextKind::Unsupported,
            "until it says it has focus"
        );
        r.editor_focus(11, true, None, 3);
        assert_eq!(kind(&r), ContextKind::TerminalVim);
        // A remote pid-less Vim never claims a terminal by pid.
        r.shell_report_t(200, ShellState::Prompt { vi: true }, 4);
        r.editor_focus(11, false, None, 5);
        assert_eq!(kind(&r), ContextKind::BashVi);
    }
}
