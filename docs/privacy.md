# Privacy

Flow Buddy reads your keyboard. That is a lot of trust to ask for, so this document states exactly what happens to that data and how each promise is enforced. If the code ever disagrees with this document, that is a bug.

## Summary

- Nothing leaves your machine. Flow Buddy has no networking code.
- Raw keystrokes and typed text are never written to disk or logs.
- Only aggregate counts and abstract "shapes" of mistakes are stored (for example, `j×7 in Normal mode`).
- You can inspect, export and delete everything that is stored.

## What is captured

| Data | Source | Kept where | How long |
|---|---|---|---|
| Key events | Linux evdev (`/dev/input`) | In-memory ring buffer (last ~64 keys) | Until the context changes, you pause, or the daemon stops |
| Focused app id and window PID (titles are not sent) | GNOME Shell extension | Memory only | Replaced on every focus change |
| Terminal foreground command **name** (e.g. `vim`, never its arguments) | Shell hooks (bash, zsh, fish) | Memory only | Replaced on every command |
| Which tmux pane is active and the *name* of what runs in it (e.g. `vim`) | tmux | Memory only | Checked every second while tmux is in use |
| **Opt-in** (`[typing] accuracy = true`): characters typed and deleted again, and which pair of characters came right before each correction (`er`). The last two typed characters are held in memory to name the pair, and dropped on any other key | Daemon | Counts in `~/.local/share/flowbuddy/` | Until you `flowbuddy memory forget` |
| Whether Claude Code's Vim mode is on: only `editorMode` is read from `~/.claude/settings.json` | Daemon | Memory only | Checked every few seconds |
| The command's **shape** from a fixed list: exactly `cd ..`, `cd ../..`, the previous command's long last argument again, the previous command with one word changed. The hook compares the lines inside the shell; neither line is sent | Shell hooks | Counts in the stats, like any habit | Like any habit |
| That a click happened (no position, no target) | GNOME Shell extension | Not kept: it only marks the Vim mode as unknown | Immediately |
| Whether a VS Code editor has focus, vscodevim's mode, and cursor movements as deltas, counts and flags (never text, characters, file names or positions) | VS Code companion (`vscode-extension/`) | Memory only | Replaced on every report |
| Which text objects (`iw`, `i(`, `ap`…) a delete, change or yank covered, by name only | Vim plugin, VS Code companion | Memory only | Used for one nudge, then dropped |

The key buffer is only filled while a supported context is focused (VS Code with Vim, Vim in a terminal, bash vi-mode). In any other app, keys are dropped as soon as they are read.

## What is stored on disk

All of this lives in `~/.local/share/flowbuddy/`.

- **Stats:** per day, context and rule, Flow Buddy stores how many times the pattern happened, how many nudges were shown, how many were dismissed, and how many times you used the better command.
- **Mistake memory:** each pattern has one record holding when it was first and last seen, its counts, and its learning stage. Episode records add a time bucket, rule, context, mode and an abstract shape such as `x×5 then i`. Shapes describe *which commands* you used, never what you typed in Insert mode, file names or window titles.
- **Config:** your settings in `~/.config/flowbuddy/config.toml`.

## What is never stored

- Typed text, including anything typed in Insert mode or at the shell prompt
- Raw key sequences
- File names, window titles, command arguments, paths

## How it is enforced

| Guarantee | Enforcement | Ticket |
|---|---|---|
| Key buffer stays in memory | Buffer types have no serialization and no printable debug output in release builds | #8 |
| Buffer cleared on context change, pause and shutdown | Unit tests | #8 |
| Devices are only read, never grabbed | The daemon never calls `EVIOCGRAB`, so it can never block your input | #6 |
| Stored data contains no typed text | `database_never_contains_typed_text` types a session with a password, a file name, a search and shell commands, then checks every text value in the database | #27, #30 |
| Socket is private | `$XDG_RUNTIME_DIR/flowbuddy.sock`, permissions `0600`, checked in tests | #10 |
| No network | No networking dependencies; the systemd unit allows only Unix sockets (`RestrictAddressFamilies=AF_UNIX`) | #9 |

## Keyboard access

To read keys, the daemon needs access to `/dev/input/event*`. There are two ways to get it:

- **Recommended:** `scripts/install.sh --with-udev` installs a udev rule that tags keyboards and mice with `uaccess`. systemd-logind then grants access only to the user sitting at the machine, and only while their session is active.
- **Not recommended:** joining the `input` group. It lets **any** program you run read all keystrokes, at any time, including over ssh.

The comparison, and what was found on a real machine, is in [spikes/evdev.md](spikes/evdev.md).

## Your control

- `flowbuddy mode off` or `flowbuddy pause 1h`: stop capturing.
- `flowbuddy memory show` / `flowbuddy memory export`: see everything that is stored.
- `flowbuddy memory forget <rule>` / `flowbuddy memory forget --all`: delete it.
- `flowbuddy rules mute <rule>` or `disable <rule>`: stop a rule from nudging, or from running at all.
- Uninstalling offers to remove all data.

Details of the memory and its export format are in [memory.md](memory.md).

## AI coaching (`flowbuddy coach`)

Off until you set `[ai] command` in `config.toml` to a program on this machine, such as `["ollama", "run", "llama3.2:3b"]`. Flow Buddy never talks to a network itself: it runs that program, writes the prompt to its stdin and shows what it writes back.

- **What the model sees:** the mistake memory export only. That means habit descriptions, contexts, learning stages, counts, and abstract shapes such as `j×12`. It never sees keys, typed text, file names, window titles or positions.
- **Check it first:** `flowbuddy coach --prompt` prints exactly what would be sent, without running anything.
- **Only when you ask:** nothing runs unprompted, unless you also set `[ai] weekly = true`. Then, once a week, the daemon starts `flowbuddy-weekly.service`, which asks the model to write the weekly progress note from the same export. If the model fails, you get the usual note. The daemon itself still has no network access; that one-shot service has it so it can reach local model servers such as Ollama on localhost.
- **Your choice of program:** the command is yours to pick. If you point it at a tool that sends data to a server, the prompt goes there, so pick one that runs locally.
