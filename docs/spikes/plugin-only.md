# Spike: a plugin-only level, with no keyboard access (#253)

hardtime.nvim and Key Promoter X are one plugin install. Flow Buddy today needs Rust, a systemd service, keyboard access (a udev rule or the `input` group), a GNOME extension, a logout and shell hooks. This spike asks how much of Flow Buddy could work from the editor plugin alone, and how to get there without a second copy of the rules.

## What I checked

- **The daemon runs without a keyboard.** With no readable `/dev/input/event*` it warns ("no readable keyboard found") and keeps scanning (`capture.rs`); everything else starts.
- **Most coaching already comes from the plugins.** Motion coaching (`motion.*`), text objects and operators run on the plugins' reports (where a movement ended, which objects an edit covered), not on raw keys. Tips can be shown in the editor (#250), and `flowbuddy practice` counts keys inside Vim (#251).
- **Context needs a window-focus source.** `Resolver::resolve` starts from the focused window's app id, from the GNOME extension or X11; without one, everything is `Unsupported`, even with a plugin saying it has focus.
- **Key counts come from the keyboard.** Tips like "That took 6 keys" and the repeat rules (`jjjj`, `xxxx`) count evdev keys.
- **Editors can see keys themselves:**
  - Vim 9.1.0563 and later has a `KeyInputPre` autocommand with the key in `v:char`. The Vim here (9.1, patches to 2141) has it.
  - Neovim has `vim.on_key()`.
  - A VS Code extension can't see keystrokes when vscodevim owns the `type` command, and two extensions can't both own it. The companion can still see each cursor move (`onDidChangeTextEditorSelection`), which is enough to rebuild a run like `j×7` but not keys that don't move the cursor.

## Recommendation: the same daemon, started by the plugin

Don't port the rules to Lua or JavaScript, or to WebAssembly. Two copies would drift, and WebAssembly has no easy home in Vim. Instead:

1. **An editor-only mode in `flowbuddyd`** (`flowbuddyd --editor-only`, or detected when there's no focus source): no keyboard reading, and context from the editor that last said it has focus (`EditorFocus`), its kind giving the context (`TerminalVim`, `VsCodeVim`).
2. **Keys from the plugin, in moving modes only.** A new `editor_keys` message carries keys typed in Normal, Visual and Operator-pending mode, batched, from `KeyInputPre` / `vim.on_key`. Insert and Command-line mode keys are never sent: there they're your text. The daemon feeds them to the rules as it does evdev keys, and ignores them while it reads a keyboard itself, so nothing is counted twice.
3. **VS Code: cursor moves as keys.** The companion already sends motions; adding "one line down, again" steps lets the repeat rules see `j×7`. Keys that don't move the cursor go uncounted, and the README says so.
4. **The plugin starts the daemon.** If there's no socket, the plugin starts `flowbuddyd --editor-only` detached (found on `PATH` or in `~/.local/bin`), so there's no service to install. A daemon from the full install is used if it's running.
5. **Prebuilt binaries.** A GitHub release with `flowbuddyd` and `flowbuddy` for Linux x86_64 and aarch64 means no Rust. Without evdev, macOS is in reach too; that would be its own ticket.

## What works at each level

| | Editor only | Full install |
|---|---|---|
| Vim and Neovim: motions, text objects, operators, repeats, mistakes | yes | yes |
| VS Code with vscodevim | motions, text objects, operators; repeats from cursor moves | everything |
| Tips | in the editor | in the editor, the top bar, notifications |
| Daily lessons, reviews, practice, skill map, summaries, `coach` | yes | yes |
| Shell (bash/zsh/fish vi mode), Claude Code's prompt, tmux | no | yes |
| One memory across editors and the shell | across editors | everything |
| Install | the plugin, plus `flowbuddy` and `flowbuddyd` on `PATH` | `scripts/install.sh --with-udev`, log out and back in |

## Moving up

Both levels use the same database (`~/.local/share/flowbuddy/flowbuddy.db`) and config, so nothing moves. Running the full installer starts the service and the plugin finds its socket. With keyboard access the daemon reads the keyboard and stops taking keys from the plugin.

## Follow-up tickets (M32)

1. #257 Editor-only mode: context from the editor's focus when there's no window-focus source; `--editor-only`.
2. #258 `editor_keys`: Vim (`KeyInputPre`, with a fallback to motion reports on older Vim) and Neovim (`vim.on_key`), moving modes only, plus tests that Insert-mode keys are never sent.
3. #259 The plugins start the daemon when there's no socket.
4. #260 VS Code: repeated cursor steps as key runs.
5. #261 Release binaries, and an install section for the editor-only level.
