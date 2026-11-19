# Privacy

Flow Buddy reads your keyboard. That is a lot of trust to ask for, so this document states exactly what happens to that data and how each promise is enforced. If the code ever disagrees with this document, that is a bug.

## Summary

- Nothing leaves your machine unless you choose a cloud AI for `flowbuddy coach` (off by default). The daemon, which reads the keys, has no networking code at all.
- One download, when you ask for it: installed with a Vim or Neovim plugin manager, `scripts/get-daemon.sh` (the plugin's build step, or `:FlowBuddyGetDaemon`) fetches the daemon from this project's GitHub releases and checks its checksum. It sends nothing about you. The VS Code extension downloads nothing: its daemon comes inside it.
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
| What happened after each tip: rule, context, learning stage, hour, mode, how many tips came before it that day, and whether you used the better way, dismissed it, marked it not helpful, or did nothing | Daemon | `~/.local/share/flowbuddy/`, 90 days | For the learned tip filter; `memory forget` clears it |
| Which curriculum features you use (ids such as `ciw`, counts per day), and which daily tips were shown and learned | Daemon | `~/.local/share/flowbuddy/` | Until `memory forget` |
| Over ssh (opt-in, with `flowbuddy ssh-config`): the same editor reports, from a remote Vim through a forwarded socket that only your remote user can open | Vim plugin on the remote host | Memory only | As above |
| On X11 desktops: the focused window's class and pid (not its title), read from the X server | Daemon | Memory only | Replaced on every focus change |
| Whether Claude Code's Vim mode is on: only `editorMode` is read from `~/.claude/settings.json` | Daemon | Memory only | Checked every few seconds |
| The command's **shape** from a fixed list: exactly `cd ..`, `cd ../..`, the previous command's long last argument again, the previous command with one word changed. The hook compares the lines inside the shell; neither line is sent | Shell hooks | Counts in the stats, like any habit | Like any habit |
| That a click happened (no position, no target) | GNOME Shell extension (or the mouse's input device, if the daemon can read it) | Not kept: it marks the Vim mode as unknown, and a cursor move that came with it gets the keys that would have done it | Immediately |
| Whether a VS Code editor has focus, vscodevim's mode, and cursor movements as deltas, counts and flags (never text, characters, file names or positions) | VS Code companion (`vscode-extension/`) | Memory only | Replaced on every report |
| **Only when the daemon reads no keyboard** (an editor-only install): the keys you type in Normal, Visual and Operator-pending mode, which are commands (`5j`, `dw`, `ci(`). Never keys typed in Insert, Replace, Command-line or Terminal mode, or at a prompt; the character after `f`, `t` or `r` is sent as `x`; keys a mapping or script ran aren't sent. `let g:flowbuddy_keys = 0` turns it off. From VS Code, only single cursor steps, as `j`, `k`, `h`, `l` | Vim plugin, VS Code companion | The in-memory key buffer, like keys from the keyboard | As for key events |
| Which text objects (`iw`, `i(`, `ap`…) a delete, change or yank covered, by name only | Vim plugin, VS Code companion | Memory only | Used for one nudge, then dropped |
| That Vim said a `:` command doesn't exist (E492), not which command. The daemon pairs it with the command's name, which it already has from your keys, to suggest the right one | Vim plugin | Memory only | Used for one tip, then dropped |
| **Opt-in** (`[text] shapes = true`): for a delete, change or yank on one line, which motions would have covered it exactly (`e` presses, `w` presses, `f`/`t` presses, whether it ran to the line's end). The editor plugin works this out from the line and sends the counts. Never in files on the exclusion list (`.env`, keys, `.ssh/`, password stores…, plus `[text] exclude`), nor in anything that isn't a file buffer | Vim plugin, VS Code companion | Memory only | Used for one nudge, then dropped |
| **Only while you record** (`flowbuddy record start`, or always with `[record] keep_days`, which keeps that many days and deletes older ones): what the rules see, which tips were shown and what you did with them, step by step. That means Normal-mode commands (`5j`, `ciw`, with `x` for the character after `f`, `t` or `r`), Insert-mode keys as their kind only (`text`, `backspace`), searches without their pattern, `:` commands with every letter and digit after the name replaced (`:%s/x/x/x`), and the editors' motion and text-object reports | Daemon | A file in `~/.local/state/flowbuddy/recordings/` | Until you delete it; never sent anywhere |

The key buffer is only filled while a supported context is focused: VS Code with vscodevim, Vim or Neovim (in a terminal, gVim or Neovide), a vi-mode bash, zsh or fish prompt, or Claude Code's Vim mode. In any other app, keys are dropped as soon as they are read.

## What is stored on disk

All of this lives in `~/.local/share/flowbuddy/`.

- **Stats:** per day, context and rule, Flow Buddy stores how many times the pattern happened, how many nudges were shown, how many were dismissed, and how many times you used the better command.
- **Mistake memory:** each pattern has one record holding when it was first and last seen, its counts, and its learning stage. Episode records add a time bucket, rule, context, mode and an abstract shape such as `x×5 then i`. Shapes describe *which commands* you used, never what you typed in Insert mode, file names or window titles.
- **Tip history:** each tip as shown (its rule, title and text) for 90 days, for `flowbuddy tips` and the menu's recent tips. Tip texts are built from fixed sentences and counts, never from what you typed.
- **Practice scores:** for each drill, when, which kind, and per round the keys you used, the fewest that would do and the time, in `~/.local/state/flowbuddy/practice.json` (the last 200). The drill's text is made up, and nothing of yours is opened.
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
| The Vim plugin never sends typed text | `Test_typed_text_is_never_kept` types in Insert mode, on the command line, in a search, after `r` and in Replace mode with real key events, and checks none of it is kept to send; `Test_keys_to_send` covers each mode | #258 |
| Devices are only read, never grabbed | The daemon never calls `EVIOCGRAB`, so it can never block your input | #6 |
| Stored data contains no typed text | `database_never_contains_typed_text` types a session with a password, a file name, a search and shell commands, then checks every text value in the database | #27, #30 |
| Socket is private | `$XDG_RUNTIME_DIR/flowbuddy.sock` (never `/tmp`), permissions `0600`, checked in tests. Over ssh, where the forwarded socket is in `/tmp`, the Vim plugin only connects to one your user owns | #10 |
| Files are private | The database, state, recordings and practice scores are written owner-only (`0600`, in `0700` folders). `flowbuddy coach` runs Claude Code in a private, empty folder | #266 |
| Messages can't crash the daemon | Lines over 16 KB are refused; every number from an editor is clamped to a sane range; `extreme_numbers_dont_panic` sends every kind of message with each number at its extremes | #266 |
| No network in the daemon | `crates/daemon/tests/no_network.rs` fails if network code or an HTTP client gets into the daemon or the crates it's built from; the systemd unit allows only Unix sockets (`RestrictAddressFamilies=AF_UNIX`); on X11, only a display on this machine is watched, never one forwarded over ssh | #9, #266 |

## Keyboard access

To read keys, the daemon needs access to `/dev/input/event*`. There are two ways to get it:

- **Recommended:** `scripts/install.sh --with-udev` installs a udev rule that tags keyboards with `uaccess`. systemd-logind then grants access only to the user sitting at the machine, and only while their session is active.
- **Not recommended:** joining the `input` group. It lets **any** program you run read all keystrokes, at any time, including over ssh.

**What this changes on your machine.** Either way, the access goes to your user, not to Flow Buddy alone: with the udev rule, any program you run can read the keyboard while your session is active. On X11 that was already so. On Wayland it wasn't: programs normally only see the keys typed into their own windows, and the rule removes that protection for programs running as you. Linux has no way to grant keyboard access to a single program without running it as root. If that trade isn't right for your machine, `scripts/install.sh --editor-only` coaches from the editor plugins alone, with no keyboard access ([install.md](install.md#editor-only)). `scripts/uninstall.sh` removes the rule.

The comparison, and what was found on a real machine, is in [spikes/evdev.md](spikes/evdev.md).

## Your control

- `flowbuddy mode off` or `flowbuddy pause 1h`: stop capturing.
- `flowbuddy memory show` / `flowbuddy memory export`: see everything that is stored.
- `flowbuddy memory forget <rule>` / `flowbuddy memory forget --all`: delete it.
- `flowbuddy rules mute <rule>` or `disable <rule>`: stop a rule from nudging, or from running at all.
- `scripts/uninstall.sh --purge` removes everything, data included.
- `flowbuddy doctor --report` shows what a bug report would contain before you paste it: versions, checks and changed settings, with paths, your user name and private settings left out.

Details of the memory and its export format are in [memory.md](memory.md).

## AI features (`flowbuddy coach`, the weekly note, the review)

Off until you choose an AI in `config.toml`. The daemon never talks to it: it has no network access at all. Only the `flowbuddy` command and its one-shot services do.

| `[ai] provider` | What it uses | Where the prompt goes |
|---|---|---|
| `command` (or just `command = [...]`) | Any program that reads a prompt on stdin, e.g. `ollama run llama3.2:3b` | Wherever that program sends it; a local model keeps it on this machine |
| `claude-code` | Claude Code's CLI, `claude -p`: your Claude subscription, no key | Anthropic, under your Claude account |
| `anthropic` | The Anthropic API with your key | Anthropic, under your API account |
| `openai` | Any OpenAI-compatible API with your key (`base_url` for others) | That service |

- **What's sent:** the prompt built from the mistake memory export: habit descriptions, contexts, learning stages, counts, abstract shapes such as `j×12`, and tip outcomes. Never keys, typed text, file names, window titles or positions.
- **Snippets, only if you turn them on** (`[text] ai_snippets = true` and `projects`): `coach` asks the daemon for the code around your last few tips. The daemon asks the editor plugins and passes their answer straight on; it never stores or logs it. Each snippet is up to five lines around a move or edit, from an allowed file in one of your projects. The CLI redacts it ([`redact`](../crates/core/src/text.rs): keys, tokens, emails, credentials in URLs, values after names like `password` or `token`, private key blocks) before it goes into the prompt. Snippets go only to a local provider (a `command`, or an OpenAI-compatible server on localhost) unless you also set `[text] cloud = true`. `--prompt` shows them.
- **Check first:** `flowbuddy coach --prompt` prints exactly what would be sent, without sending anything.
- **Keys:** read from an environment variable (`api_key_env`, default `ANTHROPIC_API_KEY` / `OPENAI_API_KEY`) or a command such as your keyring (`api_key_command`), never from `config.toml`, and never logged. The one-shot services don't see your shell's environment, so for them use `api_key_command`.
- **When it runs:** only when you run `flowbuddy coach` or `flowbuddy review`, unless you also turn on `[ai] weekly`, in which case it writes the weekly note and runs the review once a week too. A review only proposes: nothing changes until you `flowbuddy review --apply`. With `[ai] tips = true`, a daily timer has it write one sentence per recent habit from the same export. Each sentence is checked (short, plain, only commands Flow Buddy knows) before the daemon adds it to tips.

## Text features (`[text]`, all off by default)

Two features look at your text, and only inside the editor plugins:

- `shapes`: more counts about edits, as above. The text stays in the editor.
- `ai_snippets`: a few lines around recent slow edits, kept in the plugin's memory and handed to `flowbuddy coach` when you ask it. See the AI section.

Whatever the config says, the plugins never look at:

- **Secret files.** The built-in list is in `crates/core/src/text.rs` (`NEVER`): `.env*`, `*.pem`, `*.key`, SSH and GPG keys, `.netrc`, `.git-credentials`, cloud credentials, password stores, `*.tfvars`, and files whose name says secret, password or credential. `[text] exclude` adds your own globs.
- **Anything that isn't a file you're editing:** terminals, prompts, the Claude Code prompt, password prompts, untitled buffers, VS Code's panels, and Vim buffers encrypted with `:X`.
- **Anywhere outside `[text] projects`,** for snippets.

The daemon sends the plugins the list; they check paths themselves, so file paths are never sent.

