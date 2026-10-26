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
    pub vims: Vec<String>,
    /// Command names of Claude Code.
    pub claudes: Vec<String>,
}

impl Default for AppIds {
    fn default() -> Self {
        let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
        AppIds {
            vscode: s(&[
                "code",
                "code_code",
                "code-url-handler",
                "code-oss",
                "codium",
                "com.visualstudio.code",
            ]),
            terminals: s(&[
                "org.gnome.Terminal",
                "org.gnome.Ptyxis",
                "org.gnome.Console",
                "kitty",
                "Alacritty",
                "org.wezfurlong.wezterm",
                "com.mitchellh.ghostty",
                "foot",
                "terminator",
                "com.gexperts.Tilix",
                "xterm",
            ]),
            vims: s(&["vim", "vi", "nvim", "view", "vimdiff", "nvimdiff"]),
            claudes: s(&["claude"]),
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

    /// The most recent shell under `window_pid` running Claude Code with
    /// its Vim mode on.
    fn claude_in(&self, window_pid: Option<u32>) -> Option<Resolved> {
        if !self.claude_vim {
            return None;
        }
        let (&pid, shell) = self
            .shells
            .iter()
            .filter(|(pid, _)| window_pid.is_none_or(|w| self.descends_from(**pid, w)))
            .max_by_key(|(_, s)| s.updated_ms)?;
        matches!(&shell.state, ShellState::Fg(cmd) if self.ids.claudes.contains(cmd)).then_some(
            Resolved {
                kind: ContextKind::ClaudeVim,
                shell_pid: Some(pid),
                editor: None,
            },
        )
    }

    pub fn focused_app(&self) -> Option<&str> {
        self.focus.app_id.as_deref()
    }

    pub fn shell_report(&mut self, shell_pid: u32, state: ShellState, now_ms: u64) {
        self.shells.insert(
            shell_pid,
            Shell {
                state,
                updated_ms: now_ms,
            },
        );
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
                // The integrated terminal: maybe Claude Code's prompt.
                Some((_, e)) if e.focused == Some(false) && e.panel == Some(Panel::Terminal) => {
                    self.claude_in(self.focus.pid)
                        .unwrap_or(Resolved::UNSUPPORTED)
                }
                Some((_, e)) if e.focused == Some(false) => Resolved::UNSUPPORTED,
                editor => Resolved {
                    kind: ContextKind::VsCodeVim,
                    shell_pid: None,
                    editor: editor.map(|(id, _)| id),
                },
            };
        }
        if !contains(&self.ids.terminals, app_id) {
            return Resolved::UNSUPPORTED;
        }
        let Some(window_pid) = self.focus.pid else {
            return Resolved::UNSUPPORTED;
        };
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

        r.shell_report(200, ShellState::Prompt { vi: true }, 1);
        assert_eq!(kind(&r), ContextKind::BashVi);

        r.shell_report(200, ShellState::Fg("vim".into()), 2);
        assert_eq!(kind(&r), ContextKind::TerminalVim);

        r.shell_report(200, ShellState::Fg("git".into()), 3);
        assert_eq!(kind(&r), ContextKind::Unsupported);

        r.shell_report(200, ShellState::Prompt { vi: false }, 4);
        assert_eq!(kind(&r), ContextKind::Unsupported, "emacs-mode bash");
    }

    #[test]
    fn most_recent_shell_wins_within_one_terminal_server() {
        let mut r = resolver();
        r.set_focus(Some("org.gnome.Terminal".into()), Some(100));
        r.shell_report(200, ShellState::Fg("vim".into()), 1);
        r.shell_report(300, ShellState::Prompt { vi: true }, 2);
        assert_eq!(r.resolve().shell_pid, Some(300));
        assert_eq!(kind(&r), ContextKind::BashVi);
    }

    #[test]
    fn shells_from_other_windows_are_ignored() {
        let mut r = resolver();
        r.shell_report(200, ShellState::Fg("vim".into()), 1);
        r.set_focus(Some("kitty".into()), Some(900));
        assert_eq!(kind(&r), ContextKind::Unsupported);
    }

    #[test]
    fn dead_shells_are_pruned() {
        let mut r = resolver();
        r.shell_report(4242, ShellState::Prompt { vi: true }, 1);
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
        r.shell_report(200, ShellState::Fg("git".into()), 1);
        assert_eq!(kind(&r), ContextKind::Unsupported);
        r.editor_hello(8, EditorKind::Vim, Some(210), 2);
        assert_eq!(kind(&r), ContextKind::TerminalVim);
        assert_eq!(r.resolve().editor, Some(8));
        // Another window's shell drew a prompt more recently.
        r.shell_report(300, ShellState::Prompt { vi: true }, 3);
        assert_eq!(kind(&r), ContextKind::BashVi);
        // Focus events settle it.
        r.editor_focus(8, true, None, 4);
        assert_eq!(kind(&r), ContextKind::TerminalVim);
        r.editor_focus(8, false, None, 5);
        assert_eq!(kind(&r), ContextKind::BashVi);
    }

    #[test]
    fn vim_in_tmux_counts_when_it_has_focus() {
        let mut r = resolver();
        r.set_focus(Some("org.gnome.Terminal".into()), Some(100));
        r.shell_report(200, ShellState::Fg("tmux".into()), 1);
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
        r.shell_report(200, ShellState::Fg("claude".into()), 1);
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
        r.shell_report(510, ShellState::Fg("claude".into()), 2);
        r.editor_focus(7, false, Some(Panel::Other), 3);
        assert_eq!(
            kind(&r),
            ContextKind::Unsupported,
            "a chat panel isn't the terminal"
        );
        r.editor_focus(7, false, Some(Panel::Terminal), 4);
        assert_eq!(kind(&r), ContextKind::ClaudeVim);
        r.shell_report(510, ShellState::Prompt { vi: false }, 5);
        assert_eq!(kind(&r), ContextKind::Unsupported, "claude exited");
    }
}
