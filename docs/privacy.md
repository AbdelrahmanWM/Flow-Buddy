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
| Terminal foreground command **name** (e.g. `vim`, never its arguments) | Bash hook | Memory only | Replaced on every command |
| That a click happened (no position, no target) | GNOME Shell extension | Not kept: it only marks the Vim mode as unknown | Immediately |

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

To read keys, the daemon needs access to `/dev/input/event*`. On Linux that usually means being in the `input` group. Be aware that being in that group lets **any** program you run read all keystrokes, not just Flow Buddy. Ticket #4 compares this with a narrower udev rule, and the setup docs will recommend the safer option.

## Your control

- `flowbuddy mode off` or `flowbuddy pause 1h`: stop capturing.
- `flowbuddy memory show` / `flowbuddy memory export`: see everything that is stored.
- `flowbuddy memory forget <rule>` / `flowbuddy memory forget --all`: delete it.
- `flowbuddy rules mute <rule>` or `disable <rule>`: stop a rule from nudging, or from running at all.
- Uninstalling offers to remove all data.

Details of the memory and its export format are in [memory.md](memory.md).

## Future AI features

When AI-based suggestions arrive, they will receive the **mistake memory export** (aggregates and abstract shapes), never raw input. Any feature that would send data off the machine will be opt-in, off by default, and documented here first.
