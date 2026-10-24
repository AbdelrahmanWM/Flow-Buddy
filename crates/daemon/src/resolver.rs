//! Combine focus and shell reports into one [`ContextKind`].
//!
//! See docs/architecture.md#resolving-the-context.

use std::collections::HashMap;

use flowbuddy_core::context::ContextKind;

/// App ids and command names that select each context.
#[derive(Clone, Debug)]
pub struct AppIds {
    pub vscode: Vec<String>,
    pub terminals: Vec<String>,
    pub vims: Vec<String>,
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

/// The resolved context and what caused the latest change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub kind: ContextKind,
    /// The shell the context comes from, for terminal contexts.
    pub shell_pid: Option<u32>,
}

impl Resolved {
    const UNSUPPORTED: Resolved = Resolved {
        kind: ContextKind::Unsupported,
        shell_pid: None,
    };
}

pub struct Resolver {
    ids: AppIds,
    procs: Box<dyn ProcessTree>,
    focus: Focus,
    shells: HashMap<u32, Shell>,
}

/// Walking more parents than this means something is odd; give up.
const MAX_DEPTH: usize = 64;

impl Resolver {
    pub fn new(ids: AppIds, procs: Box<dyn ProcessTree>) -> Self {
        Resolver {
            ids,
            procs,
            focus: Focus::default(),
            shells: HashMap::new(),
        }
    }

    pub fn set_focus(&mut self, app_id: Option<String>, pid: Option<u32>) {
        self.focus = Focus { app_id, pid };
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
            return Resolved {
                kind: ContextKind::VsCodeVim,
                shell_pid: None,
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
        let Some((&pid, shell)) = shell else {
            return Resolved::UNSUPPORTED;
        };
        let kind = match &shell.state {
            ShellState::Fg(cmd) if self.ids.vims.iter().any(|v| v == cmd) => {
                ContextKind::TerminalVim
            }
            ShellState::Prompt { vi: true } => ContextKind::BashVi,
            _ => ContextKind::Unsupported,
        };
        Resolved {
            kind,
            shell_pid: Some(pid),
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

    /// Terminal server 100 with shells 200 and 300; VS Code 500.
    pub(crate) fn tree() -> Box<FakeTree> {
        Box::new(FakeTree(vec![
            (100, 1),
            (200, 100),
            (300, 100),
            (500, 1),
            (900, 1),
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
}
