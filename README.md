# Flow Buddy

**Improve all the time without stopping to study.**

Flow Buddy is a local companion that helps you get better at your tools continuously. It watches how you work, spots inefficient patterns, and suggests a better way, quietly and only when it's useful.

> **Status:** early development. The MVP is built but not yet tried on a real desktop. Track progress in the [milestones](https://github.com/AbdelrahmanWM/Flow-Buddy/milestones).

## Why

- Most people aren't sure whether they use their tools well, but rarely set aside time to find out.
- Learning happens in big, disconnected blocks (tutorials, courses) that don't turn into real habits.
- Existing habit tools (e.g. hardtime.nvim) punish or block instead of coaching, and each one only works with a single tool.
- Monitoring tools from unknown companies are hard to trust with keystroke data.

## Principles

- **Local only.** No data leaves the machine. Raw text is never stored. See [docs/privacy.md](docs/privacy.md).
- **Not annoying.** Nudges are rate-limited and dismissible, and there's an easy off switch.
- **Coach, don't block.** It suggests the better way and never prevents the action.
- **Measurable.** Improvement should show up in the data, not just feel better.
- **Open source.** Trust comes from being inspectable.

## How it works

```
[Capture daemon] -> [Context layer] -> [Rule engine] -> [Nudge layer]
   key events        focused app,       rule packs       GNOME top-bar hints,
   (Linux evdev)     terminal command,  (Vim first)      summaries
                     inferred Vim mode       |
                                    [Stats + mistake memory]
                                    aggregates and abstract shapes only
```

- **Coaching modes:** `off`, `passive` (summaries only), `balanced` (occasional batched tips) and `active` (real-time nudges).
- **Mistake memory:** remembers which patterns you repeat and how each habit is progressing (new → improving → mastered), so coaching adapts over time.

Details, including the IPC protocol and the context model, are in [docs/architecture.md](docs/architecture.md).

## What it coaches

Vim in VS Code (vscodevim), Vim and Neovim in the terminal, and bash vi-mode.

| You did | Try |
|---|---|
| `jjjjjj` to a blank line | `}` |
| `llllll` to a character | `f<char>`, `w`, `e` |
| `kkkk…` to the top of the window | `H` |
| `xxxx` | `dw` / `de` |
| Arrow keys in Normal mode | `h` `j` `k` `l` |
| `x` then `i`, `dw` then `i` | `s`, `cw` |
| `viwd`, `Vjjd` | `diw`, `3dd` |
| The same change again | `.` |
| The same edit on every line | a macro: `qq…q`, `@q` |
| `:w` then `:q` | `:wq` or `ZZ` |
| `↑↑↑↑` hunting shell history | `Ctrl-r` |

`flowbuddy rules` lists all of them. Motion coaching (the first three rows) compares each movement with the cheapest motion that lands in the same place. It needs the editor plugins, which tell Flow Buddy where a movement ended without sending any text: [vim-plugin/](vim-plugin/README.md) and [vscode-extension/](vscode-extension/README.md). Without them, the simpler rules that count repeated keys take over.

`flowbuddy coach` asks a model running on your machine for advice based on the mistake memory. It's off until you set it up, and `--prompt` shows exactly what the model sees.

## Platform

Linux, initially Ubuntu with GNOME on Wayland.

## Install

Requires Rust (via [rustup](https://rustup.rs)), GNOME Shell 50 and bash.

```sh
scripts/install.sh --dry-run      # see what it will do
scripts/install.sh --with-udev    # install; --with-udev lets the active session read keyboards (sudo)
```

Then log out and back in so GNOME loads the extension, and check:

```sh
flowbuddy status          # or --watch, then switch windows
```

`scripts/uninstall.sh` removes everything, and `--purge` also deletes your data.

### VS Code

`scripts/install.sh` also installs a small companion extension when VS Code is present (`~/.vscode/extensions/flowbuddy.flowbuddy-vscode-0.1.0`; restart VS Code afterwards). It tells the daemon whether the editor has focus, so typing in the terminal or a chat panel isn't coached as Vim, reads vscodevim's real mode, and reports cursor movements as counts and flags so motion rules can suggest a cheaper command. It never sends text or file names. See [vscode-extension/README.md](vscode-extension/README.md).

### Vim and Neovim

`scripts/install.sh` also installs a plugin for Vim 9 and Neovim when they're present (`~/.vim/pack/flowbuddy`, `~/.local/share/nvim/site/pack/flowbuddy`). It reports the real mode, focus, and where each movement ended, as counts and flags only, so motion coaching works in the terminal too, including in tmux and in Vim started by `git commit`. See [vim-plugin/README.md](vim-plugin/README.md).

## Use

```sh
flowbuddy mode balanced           # off | passive | balanced | active
flowbuddy pause 1h                # or 30m, until-tomorrow; `flowbuddy resume`
flowbuddy summary [--week]        # how you're doing vs. before
flowbuddy next                    # the one habit worth working on now
flowbuddy explain 'ci"'            # what keys do, piece by piece
flowbuddy coach [question]        # advice from a local model ([ai] command; --prompt shows what it sees)
flowbuddy rules                   # list; `rules mute|disable|enable|reset <id>`
flowbuddy memory show             # the mistake memory; `memory export`, `memory forget`
flowbuddy report-fp               # the last nudge was wrong
```

Settings live in `~/.config/flowbuddy/config.toml` and are reloaded automatically:

```toml
[coaching]
default_mode = "balanced"
balanced_interval_minutes = 30
quiet_hours = "22:00-08:00"
nudge_style = "card"              # or "bar"; default: card in active mode

[rules."vim.repeat-jk"]
threshold = 8
```

## Develop

See [CONTRIBUTING.md](CONTRIBUTING.md) and [docs/writing-rules.md](docs/writing-rules.md).

## License

[MIT](LICENSE)
