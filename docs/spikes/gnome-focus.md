# Spike: the focused window on GNOME 50 Wayland (#5)

Checked on 2026-10-25 with GNOME Shell 50.1 on Wayland, using the real extension in `gnome-extension/` rather than a throwaway prototype.

## Result

A Shell extension can follow focus with `global.display`'s `notify::focus-window` and read the window's app and pid. Nothing else on Wayland can. The extension sends the app id and pid only, never the title: titles hold file names, and the daemon doesn't need them.

## Observed values

| App | App id sent | Pid | Context it resolved to |
|---|---|---|---|
| VS Code (snap) | `code_code` | the main `code` process; the companion's extension host descends from it | `vscode-vim` (with the companion: "mode from the editor") |
| Ptyxis | `org.gnome.Ptyxis` | the Ptyxis process; shells descend from it | `bash-vi` at a prompt, `terminal-vim` running Vim |

The app id comes from `Shell.WindowTracker` (the `.desktop` name without the suffix), falling back to the GTK application id and then the WM class. The defaults in `crates/daemon/src/resolver.rs` also list GNOME Terminal, Console, kitty, Alacritty, WezTerm, Ghostty, foot, Tilix and xterm. Those weren't checked here; `[apps]` in `config.toml` can override every list.

## Terminal pid to shell

Each bash hook report carries the shell's `$$`. The daemon walks `/proc/<pid>/stat` parents until it reaches the window's pid. GNOME Terminal and Ptyxis serve every window from one process, so shells in other windows match too. The daemon takes the shell that reported most recently, which is right as long as you type in the window you focused. Editor plugins report focus themselves and settle this for Vim.

## How to check another app

```sh
flowbuddy status --watch
```

and focus the app: the `focused:` line shows the id it reports. Add it to `[apps]` if Flow Buddy should treat it as a terminal or VS Code.
