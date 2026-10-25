//! XDG locations used by Flow Buddy.

use std::env;
use std::path::PathBuf;

fn xdg(var: &str, home_fallback: &str) -> PathBuf {
    env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(home_fallback))
}

fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// `$XDG_RUNTIME_DIR/flowbuddy.sock`. `FLOWBUDDY_SOCKET` overrides it (tests).
pub fn socket() -> PathBuf {
    if let Some(p) = env::var_os("FLOWBUDDY_SOCKET") {
        return p.into();
    }
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("flowbuddy.sock")
}

/// `~/.config/flowbuddy/config.toml`: written by the user.
pub fn config_file() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("flowbuddy/config.toml")
}

/// `~/.local/state/flowbuddy/state.json`: written by the daemon
/// (mode, pause, rule overrides).
pub fn state_file() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("flowbuddy/state.json")
}

/// `~/.local/share/flowbuddy/flowbuddy.db`: stats and mistake memory.
pub fn database() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join("flowbuddy/flowbuddy.db")
}
