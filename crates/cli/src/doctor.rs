//! `flowbuddy doctor`: check each piece Flow Buddy needs and say exactly
//! what to do about anything missing.

use std::path::{Path, PathBuf};

use flowbuddy_core::config::{AiProvider, Config};
use flowbuddy_core::ipc::{ClientKind, Status};

/// What the checks look at, gathered from the system ([`Env::gather`]) or
/// made up in tests.
pub struct Env {
    /// The daemon's status, or why it couldn't be reached.
    pub status: Result<Status, String>,
    /// `systemctl --user is-enabled flowbuddyd`: `None` without systemd.
    pub service_enabled: Option<bool>,
    /// `XDG_CURRENT_DESKTOP`, e.g. `ubuntu:GNOME`.
    pub desktop: String,
    pub gnome_extension_installed: bool,
    /// (installed, has the companion)
    pub vscode: (bool, bool),
    /// (Vim or Neovim installed, has the plugin)
    pub vim: (bool, bool),
    /// Shells installed, and whether each sources its hook.
    pub shells: Vec<(&'static str, bool)>,
    pub config: Result<Config, String>,
    /// Whether a program is on PATH.
    pub on_path: fn(&str) -> bool,
    /// Whether an environment variable is set.
    pub env_set: fn(&str) -> bool,
}

pub struct Check {
    pub ok: bool,
    pub what: String,
    /// What to do, when not ok.
    pub fix: Option<String>,
}

fn ok(what: impl Into<String>) -> Check {
    Check {
        ok: true,
        what: what.into(),
        fix: None,
    }
}

fn bad(what: impl Into<String>, fix: impl Into<String>) -> Check {
    Check {
        ok: false,
        what: what.into(),
        fix: Some(fix.into()),
    }
}

pub fn checks(env: &Env) -> Vec<Check> {
    let mut out = Vec::new();
    match &env.status {
        Ok(s) => {
            out.push(ok(format!("the daemon is running ({})", s.daemon_version)));
            if s.keyboards == 0 {
                out.push(bad(
                    "no keyboard can be read",
                    "run scripts/install.sh --with-udev (or add yourself to the input group), then log out and back in",
                ));
            } else {
                out.push(ok(format!("reading {} keyboard(s)", s.keyboards)));
            }
            if let Some(note) = &s.layout_note {
                out.push(bad(
                    format!("keyboard layout: {note}"),
                    "install libxkbcommon-dev and run scripts/install.sh again",
                ));
            }
        }
        Err(e) => out.push(bad(
            format!("the daemon isn't running ({e})"),
            "systemctl --user start flowbuddyd; if it stops again, journalctl --user -u flowbuddyd",
        )),
    }
    match env.service_enabled {
        Some(true) => out.push(ok("the service starts at login")),
        Some(false) => out.push(bad(
            "the service doesn't start at login",
            "systemctl --user enable --now flowbuddyd",
        )),
        None => {}
    }
    if env.desktop.to_uppercase().contains("GNOME") {
        let connected = env
            .status
            .as_ref()
            .is_ok_and(|s| s.clients.contains(&ClientKind::GnomeExtension));
        out.push(match (env.gnome_extension_installed, connected) {
            (_, true) => ok("the GNOME extension is connected (focus and tips in the top bar)"),
            (true, false) => bad(
                "the GNOME extension is installed but not connected",
                "log out and back in; if it still isn't, gnome-extensions enable flowbuddy@abdelrahmanwm.github.io",
            ),
            (false, _) => bad("the GNOME extension isn't installed", "run scripts/install.sh"),
        });
    } else {
        out.push(ok(
            "not GNOME: tips come as notifications (flowbuddy watch shows them in a terminal)",
        ));
    }
    match env.vscode {
        (true, true) => out.push(ok("the VS Code companion is installed")),
        (true, false) => out.push(bad(
            "VS Code is here but the companion isn't",
            "run scripts/install.sh, then restart VS Code",
        )),
        _ => {}
    }
    match env.vim {
        (true, true) => out.push(ok("the Vim plugin is installed")),
        (true, false) => out.push(bad(
            "Vim is here but its plugin isn't",
            "run scripts/install.sh",
        )),
        _ => {}
    }
    for (shell, hooked) in &env.shells {
        out.push(if *hooked {
            ok(format!("{shell} reports its commands"))
        } else {
            bad(
                format!("{shell} doesn't load the Flow Buddy hook"),
                format!("run scripts/install.sh, or source shell/flowbuddy.{shell} from your {shell} config"),
            )
        });
    }
    match &env.config {
        Ok(config) => {
            out.push(ok("the config file is fine"));
            if let Some(c) = ai_check(config, env) {
                out.push(c);
            }
        }
        Err(e) => out.push(bad(
            format!("the config file has a problem: {e}"),
            "fix ~/.config/flowbuddy/config.toml; the daemon keeps the last good settings meanwhile",
        )),
    }
    out
}

fn ai_check(config: &Config, env: &Env) -> Option<Check> {
    let ai = &config.ai;
    let key = |default: &str| {
        let var = ai.api_key_env.as_deref().unwrap_or(default);
        ai.api_key_command.is_some() || (env.env_set)(var)
    };
    Some(match ai.provider {
        AiProvider::None if ai.command.is_none() => return None,
        AiProvider::None | AiProvider::Command => {
            let program = ai
                .command
                .as_ref()
                .and_then(|c| c.first())
                .cloned()
                .unwrap_or_default();
            if (env.on_path)(&program) {
                ok(format!("AI: runs {program}"))
            } else {
                bad(
                    format!("AI: {program} isn't on PATH"),
                    "install it, or change [ai] command",
                )
            }
        }
        AiProvider::ClaudeCode => {
            if (env.on_path)("claude") {
                ok("AI: Claude Code (claude -p)")
            } else {
                bad(
                    "AI: claude isn't on PATH",
                    "install Claude Code, or choose another [ai] provider",
                )
            }
        }
        AiProvider::Anthropic => {
            if key("ANTHROPIC_API_KEY") {
                ok("AI: the Anthropic API, with a key")
            } else {
                bad(
                    "AI: no key for the Anthropic API",
                    "export ANTHROPIC_API_KEY, or set [ai] api_key_command (needed for the timers too)",
                )
            }
        }
        AiProvider::Openai => {
            if key("OPENAI_API_KEY") {
                ok("AI: an OpenAI-compatible API, with a key")
            } else {
                bad(
                    "AI: no key for the OpenAI-compatible API",
                    "export OPENAI_API_KEY, or set [ai] api_key_command",
                )
            }
        }
    })
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(fallback))
}

fn on_path(program: &str) -> bool {
    !program.is_empty()
        && std::env::var_os("PATH")
            .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

fn mentions(file: &Path, needle: &str) -> bool {
    std::fs::read_to_string(file).is_ok_and(|s| s.contains(needle))
}

impl Env {
    pub fn gather(status: Result<Status, String>) -> Env {
        let data = xdg("XDG_DATA_HOME", ".local/share");
        let config_dir = xdg("XDG_CONFIG_HOME", ".config");
        let vscode_dir = home().join(".vscode/extensions");
        let vscode_ext = std::fs::read_dir(&vscode_dir).is_ok_and(|d| {
            d.filter_map(|e| e.ok()).any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("flowbuddy.flowbuddy-vscode-")
            })
        });
        let vim_present = on_path("vim") || on_path("nvim");
        let vim_plugin = home()
            .join(".vim/pack/flowbuddy/start/flowbuddy/plugin/flowbuddy.vim")
            .is_file()
            || data
                .join("nvim/site/pack/flowbuddy/start/flowbuddy/plugin/flowbuddy.vim")
                .is_file();
        let zdot = std::env::var_os("ZDOTDIR").map_or_else(home, PathBuf::from);
        let mut shells = vec![("bash", mentions(&home().join(".bashrc"), "flowbuddy.bash"))];
        if on_path("zsh") {
            shells.push(("zsh", mentions(&zdot.join(".zshrc"), "flowbuddy.zsh")));
        }
        if on_path("fish") {
            shells.push((
                "fish",
                config_dir.join("fish/conf.d/flowbuddy.fish").is_file()
                    || mentions(&config_dir.join("fish/config.fish"), "flowbuddy.fish"),
            ));
        }
        let service_enabled = std::process::Command::new("systemctl")
            .args(["--user", "is-enabled", "flowbuddyd"])
            .output()
            .ok()
            .map(|o| o.status.success());
        let config_file = flowbuddy_core::paths::config_file();
        Env {
            status,
            service_enabled,
            desktop: std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
            gnome_extension_installed: data
                .join("gnome-shell/extensions/flowbuddy@abdelrahmanwm.github.io/extension.js")
                .is_file(),
            vscode: (
                on_path("code") || home().join(".vscode").is_dir(),
                vscode_ext,
            ),
            vim: (vim_present, vim_plugin),
            shells,
            config: Config::load(&config_file),
            on_path,
            env_set: |v| std::env::var_os(v).is_some_and(|x| !x.is_empty()),
        }
    }
}

/// The report: one line per check, the fix under each problem.
pub fn report(checks: &[Check]) -> String {
    let mut out = String::new();
    for c in checks {
        out.push_str(&format!("{} {}\n", if c.ok { "✓" } else { "✗" }, c.what));
        if let Some(fix) = &c.fix {
            out.push_str(&format!("    → {fix}\n"));
        }
    }
    let problems = checks.iter().filter(|c| !c.ok).count();
    out.push_str(&match problems {
        0 => "\nAll good.\n".to_string(),
        1 => "\n1 thing to fix.\n".to_string(),
        n => format!("\n{n} things to fix.\n"),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowbuddy_core::policy::CoachingMode;

    fn status(keyboards: usize, clients: Vec<ClientKind>) -> Status {
        Status {
            running: true,
            daemon_version: "0.1.0".into(),
            mode: CoachingMode::Balanced,
            paused_until: None,
            context: "unsupported".into(),
            app: None,
            confident: false,
            mode_from_editor: false,
            layout: None,
            layout_note: None,
            tip_filter: None,
            mode_guess: None,
            timing: None,
            keyboards,
            clients,
        }
    }

    fn env() -> Env {
        Env {
            status: Ok(status(1, vec![ClientKind::GnomeExtension])),
            service_enabled: Some(true),
            desktop: "ubuntu:GNOME".into(),
            gnome_extension_installed: true,
            vscode: (true, true),
            vim: (true, true),
            shells: vec![("bash", true)],
            config: Ok(Config::default()),
            on_path: |_| true,
            env_set: |_| false,
        }
    }

    #[test]
    fn all_good() {
        let c = checks(&env());
        assert!(c.iter().all(|c| c.ok), "{}", report(&c));
        assert!(report(&c).ends_with("All good.\n"));
    }

    #[test]
    fn says_what_to_do() {
        let mut e = env();
        e.status = Ok(status(0, vec![]));
        e.vscode = (true, false);
        e.shells = vec![("bash", true), ("zsh", false)];
        let mut config = Config::default();
        config.ai.provider = AiProvider::Anthropic;
        e.config = Ok(config);
        let text = report(&checks(&e));
        assert!(
            text.contains("✗ no keyboard can be read\n    → run scripts/install.sh --with-udev"),
            "{text}"
        );
        assert!(
            text.contains("✗ the GNOME extension is installed but not connected"),
            "{text}"
        );
        assert!(
            text.contains("✗ VS Code is here but the companion isn't"),
            "{text}"
        );
        assert!(
            text.contains("✗ zsh doesn't load the Flow Buddy hook"),
            "{text}"
        );
        assert!(
            text.contains("✗ AI: no key for the Anthropic API"),
            "{text}"
        );
        assert!(text.ends_with("5 things to fix.\n"), "{text}");
    }

    #[test]
    fn daemon_down() {
        let mut e = env();
        e.status = Err("connection refused".into());
        let text = report(&checks(&e));
        assert!(text.contains("✗ the daemon isn't running (connection refused)\n    → systemctl --user start flowbuddyd"), "{text}");
    }
}
