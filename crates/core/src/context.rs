//! Where the user is typing, and in which mode.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContextKind {
    /// VS Code focused (vscodevim assumed).
    VsCodeVim,
    /// Terminal focused, foreground command is vim/vi/nvim.
    TerminalVim,
    /// Terminal focused, bash at the prompt with `set -o vi`.
    BashVi,
    /// Anything else: keys are dropped.
    Unsupported,
}

impl ContextKind {
    pub fn is_supported(self) -> bool {
        self != ContextKind::Unsupported
    }

    /// Stable name used in storage and IPC.
    pub fn as_str(self) -> &'static str {
        match self {
            ContextKind::VsCodeVim => "vscode-vim",
            ContextKind::TerminalVim => "terminal-vim",
            ContextKind::BashVi => "bash-vi",
            ContextKind::Unsupported => "unsupported",
        }
    }

    pub fn parse(s: &str) -> Option<ContextKind> {
        [
            ContextKind::VsCodeVim,
            ContextKind::TerminalVim,
            ContextKind::BashVi,
            ContextKind::Unsupported,
        ]
        .into_iter()
        .find(|k| k.as_str() == s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VimMode {
    Normal,
    Insert,
    Visual,
    VisualLine,
    VisualBlock,
    Cmdline,
    /// An operator (d, c, y, ...) is waiting for its motion.
    OpPending,
}

impl VimMode {
    pub fn is_visual(self) -> bool {
        matches!(
            self,
            VimMode::Visual | VimMode::VisualLine | VimMode::VisualBlock
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            VimMode::Normal => "normal",
            VimMode::Insert => "insert",
            VimMode::Visual => "visual",
            VimMode::VisualLine => "visual-line",
            VimMode::VisualBlock => "visual-block",
            VimMode::Cmdline => "cmdline",
            VimMode::OpPending => "op-pending",
        }
    }
}
