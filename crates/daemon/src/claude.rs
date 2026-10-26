//! Whether Claude Code's Vim mode is on, from its settings file. Only the
//! `editorMode` setting is read; nothing else in the file is kept.

use std::path::PathBuf;

/// `$CLAUDE_CONFIG_DIR/settings.json`, or `~/.claude/settings.json`.
fn settings_path() -> Option<PathBuf> {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => Some(PathBuf::from(dir).join("settings.json")),
        None => std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude/settings.json")),
    }
}

/// Whether settings JSON `text` turns on Vim mode.
pub fn vim_mode_in(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get("editorMode")?.as_str().map(|m| m == "vim"))
        .unwrap_or(false)
}

pub fn vim_mode_on() -> bool {
    settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .is_some_and(|t| vim_mode_in(&t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_the_editor_mode() {
        assert!(vim_mode_in(r#"{"editorMode": "vim", "theme": "dark"}"#));
        assert!(!vim_mode_in(r#"{"editorMode": "normal"}"#));
        assert!(!vim_mode_in("{}"));
        assert!(!vim_mode_in("not json"));
    }
}
