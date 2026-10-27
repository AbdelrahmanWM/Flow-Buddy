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
- **Tips you can keep:** the top-bar menu lists recent tips and can keep one on screen while you practise it. `Super+Alt+T` keeps the current tip, `Super+Alt+D` dismisses it, `Super+Alt+N` marks it not helpful (change them with `gsettings` under `org.gnome.shell.extensions.flowbuddy`).
- **Mistake memory:** remembers which patterns you repeat and how each habit is progressing (new → improving → mastered), so coaching adapts over time.

Details, including the IPC protocol and the context model, are in [docs/architecture.md](docs/architecture.md).

## What it coaches

Vim in VS Code (vscodevim), Vim and Neovim in the terminal, vi-mode in bash, zsh and fish, Claude Code's Vim mode (in a terminal or VS Code's terminal), and a few shell habits such as `cd ..` again and again.

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
| `n.n.n.` through search matches | `cgn` then `...`, or `:%s` |
| `uuuuuuu` | `7u`, `:earlier 1m` |
| `I// <Esc>j` on line after line | `Ctrl-v`, `j`…, `I`, type once |
| Yank, delete, `p` pastes the wrong thing | `"0p` |
| `Ctrl-w w` round the windows, `:bn :bn :bn` | `Ctrl-w h/j/k/l`, `:b name`, `Ctrl-^` |
| `:w` then `:q` | `:wq` or `ZZ` |
| `↑↑↑↑` hunting shell history | `Ctrl-r` |

`flowbuddy rules` lists all of them, and [docs/coverage.md](docs/coverage.md) shows what's coached in each place. Tips move one step at a time: Flow Buddy notices which commands you already use, and won't suggest `ci(` before you use `ciw`, or `t` before `f` (`flowbuddy learn` shows where you are). Motion coaching (the first three rows) compares each movement with the cheapest motion that lands in the same place. It needs the editor plugins, which tell Flow Buddy where a movement ended without sending any text: [vim-plugin/](vim-plugin/README.md) and [vscode-extension/](vscode-extension/README.md). Without them, the simpler rules that count repeated keys take over.

`flowbuddy coach` asks an AI of your choice (your Claude subscription, an API key, or a local model) for advice based on the mistake memory. It's off until you set it up, and `--prompt` shows exactly what would be sent:

```toml
[learn]
daily = true                                  # one small Vim feature you don't use yet, once a day
level = "beginner"                            # or "intermediate" / "advanced": skip ahead instead of waiting to see you use the basics

[typing]
accuracy = false                              # true: count corrections (counts only) for `flowbuddy summary`

[ai]
provider = "claude-code"     # your Claude subscription via `claude -p`; or "anthropic" / "openai" with a key
# command = ["ollama", "run", "llama3.2:3b"]  # or any local program: prompt on stdin, answer on stdout
weekly = false               # true: the model writes the weekly progress note and reviews the tips
tips = false                 # true: once a day the model writes a personal sentence for each recent habit
filter = true                # the learned tip filter (no LLM); skips tips you tend to dismiss

[text]                       # opt-in, off by default; secret files are never looked at (docs/privacy.md)
shapes = false               # true: plugins work out which motions an edit could have used (counts only)
ai_snippets = false          # true: `coach` shows your AI a few redacted lines around recent tips
projects = []                # folders snippets may come from, e.g. ["~/code/myapp"]
cloud = false                # true: snippets may go to a cloud provider, not only a local one
exclude = []                 # more files never to look at, e.g. ["*.sql"]
```

## Platform

Linux: GNOME on Wayland (with the top-bar indicator), or any X11 desktop such as KDE, Xfce or i3 (tips as notifications). KDE on Wayland is #123.

## Install

Requires Rust (via [rustup](https://rustup.rs)), GNOME Shell 50, and bash, zsh or fish.

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
flowbuddy setup                   # a few questions: how often, your level, AI, text features
flowbuddy doctor                  # checks every piece and says how to fix what's missing
flowbuddy mode balanced           # off | passive | balanced | active
flowbuddy pause 1h                # or 30m, until-tomorrow; `flowbuddy resume`
flowbuddy summary [--week]        # how you're doing vs. before
flowbuddy next                    # the one habit worth working on now
flowbuddy learn                   # today's "something new" and your progress through Vim
flowbuddy watch                   # tips as they come, with what they mean, in a terminal you keep open
flowbuddy tips [words] [-e]       # your recent tips, or those that match; -e says what each command does
flowbuddy explain 'ci"'            # what keys do, piece by piece; `explain --last`: your last slow move, done better
flowbuddy coach [question]        # advice from your AI ([ai]; --prompt shows what it's sent)
flowbuddy review [--apply 1 2]    # your AI proposes tuning the tips; apply what you agree with
flowbuddy rules                   # list; `rules mute|disable|enable|reset <id>`
flowbuddy memory show             # the mistake memory; `memory export`, `memory forget`, `memory fix-adoptions`
flowbuddy report-fp               # the last nudge was wrong
flowbuddy record start|stop       # record what the rules see (nothing typed) to check them; `record check FILE`
```

Settings live in `~/.config/flowbuddy/config.toml` and are reloaded automatically:

```toml
[coaching]
default_mode = "balanced"
balanced_interval_minutes = 30
quiet_hours = "22:00-08:00"
when = "now"                 # or "pause": tips wait until you stop typing for a moment
nudge_style = "card"              # or "bar"; default: card in active mode

[rules."vim.repeat-jk"]
threshold = 8
```

## Develop

See [CONTRIBUTING.md](CONTRIBUTING.md) and [docs/writing-rules.md](docs/writing-rules.md).

## License

[MIT](LICENSE)
