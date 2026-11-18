//! XDG locations used by Flow Buddy.

use std::env;
use std::path::{Path, PathBuf};

fn xdg(var: &str, home_fallback: &str) -> PathBuf {
    env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(home_fallback))
}

fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// This user's id, from who owns our own process entry.
fn uid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").ok().map(|m| m.uid())
}

/// A folder only this user can reach, for the socket and for short-lived
/// files: `$XDG_RUNTIME_DIR`, or `/run/user/<uid>`, or the state folder.
/// Never a shared one like `/tmp`, where another user could put a socket
/// or a file of their own under the name we'd look for.
pub fn runtime_dir() -> PathBuf {
    if let Some(dir) = env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        return dir.into();
    }
    if let Some(dir) = uid()
        .map(|u| PathBuf::from(format!("/run/user/{u}")))
        .filter(|d| d.is_dir())
    {
        return dir;
    }
    xdg("XDG_STATE_HOME", ".local/state").join("flowbuddy")
}

/// `<runtime folder>/flowbuddy.sock`. `FLOWBUDDY_SOCKET` overrides it (tests).
pub fn socket() -> PathBuf {
    if let Some(p) = env::var_os("FLOWBUDDY_SOCKET") {
        return p.into();
    }
    runtime_dir().join("flowbuddy.sock")
}

/// Create `dir` for this user only (0700), with its parents.
pub fn private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

/// Write `bytes` to `path` for this user only (0600), in a private folder.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    if let Some(dir) = path.parent() {
        private_dir(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    // A file from before this was private keeps its old mode otherwise.
    f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    f.write_all(bytes)
}

/// A new, empty, private folder for short-lived files, named `name`:
/// `<runtime folder>/flowbuddy-<name>-<pid>`. Fails if it's already there
/// and isn't ours to make private.
pub fn scratch_dir(name: &str) -> std::io::Result<PathBuf> {
    let dir = runtime_dir().join(format!("flowbuddy-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    private_dir(&dir)?;
    Ok(dir)
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

/// `~/.local/state/flowbuddy/recordings/`: sessions you chose to record
/// (`flowbuddy record`).
pub fn recordings_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("flowbuddy/recordings")
}

/// `~/.local/share/flowbuddy/flowbuddy.db`: stats and mistake memory.
pub fn database() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join("flowbuddy/flowbuddy.db")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn private_files_are_owner_only() {
        let dir = env::temp_dir().join(format!("fb-paths-{}", std::process::id()));
        let file = dir.join("deep/state.json");
        // Even over a file that was readable by everyone.
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "old").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&file, b"new").unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(file.parent().unwrap()), 0o700);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "new");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_runtime_folder_is_never_shared() {
        // Whatever the environment, not the system's temporary folder.
        let dir = runtime_dir();
        assert_ne!(dir, env::temp_dir());
        assert!(!dir.starts_with("/tmp"), "{}", dir.display());
    }
}
