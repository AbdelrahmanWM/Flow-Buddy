# Contributing

Thanks for wanting to help. Flow Buddy reads keyboards, so changes are reviewed for privacy as well as correctness.

## Layout

| Path | What |
|---|---|
| `crates/core` | Shared types and logic: keys, Vim mode tracking, rules and packs, engine, coaching policy, learning, curriculum, config and settings, IPC protocol |
| `crates/store` | SQLite stats and the mistake memory, export format |
| `crates/daemon` | `flowbuddyd`: evdev capture, keymap, IPC server, context resolver, the `App` state machine |
| `crates/cli` | `flowbuddy`: every command, including the AI providers (the only code that uses the network) |
| `gnome-extension/` | Focus reporting, the top-bar indicator and tip card, the settings window (GJS, GNOME 50) |
| `vim-plugin/` | The Vim and Neovim plugin: mode, focus, motion facts, tips, practice |
| `vscode-extension/` | The VS Code companion for vscodevim |
| `shell/` | The bash, zsh and fish hooks |
| `scripts/` | Install, uninstall, version check |
| `docs/` | [architecture](docs/architecture.md), [privacy](docs/privacy.md), [memory](docs/memory.md), [writing rules](docs/writing-rules.md), [coverage](docs/coverage.md), [CLI JSON](docs/cli-json.md) |

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check                       # advisories, licences, sources
vim-plugin/test/run.sh                 # the Vim plugin (needs vim)
shell/test/run.sh                      # the shell hooks (bash, zsh, fish)
for t in vscode-extension/test/*.test.js; do gjs "$t"; done
for t in gnome-extension/test/*.test.js; do gjs -m "$t"; done
gnome-extension/test/prefs-tour.sh     # the settings window's tour, drawn off screen (needs gtk4-broadwayd)
```

CI runs all of these, plus a secret scan of the history and a build with the oldest supported Rust.

Run a development daemon next to an installed one, using a separate socket:

```sh
export FLOWBUDDY_SOCKET=$XDG_RUNTIME_DIR/flowbuddy-dev.sock
cargo run -p flowbuddy-daemon -- --debug-keys   # prints keys to stderr, debug builds only
cargo run -p flowbuddy-cli -- status
```

To use libxkbcommon instead of the built-in US keymap, install `libxkbcommon-dev` and build with `--features xkb`.

Most logic is in `App` (`crates/daemon/src/app.rs`) and `crates/core`, and it is synchronous with time passed in. Test it there rather than through sockets.

## Pull requests

- Keep them small and link the issue (`Closes #N`).
- Questions and ideas are welcome in Discussions first.
- New rules follow [docs/writing-rules.md](docs/writing-rules.md), including its privacy checklist.
- Anything that changes what is captured, stored or sent must update [docs/privacy.md](docs/privacy.md) in the same PR.
- Changes to the memory export format must bump its version and update [docs/memory.md](docs/memory.md) and the golden test.

## Privacy rules for every change

- No raw keys or typed text in storage, logs, IPC or error messages.
- No networking in the daemon: it's limited to Unix sockets, and `crates/daemon/tests/no_network.rs` fails if network code or an HTTP client gets in. The AI providers in the CLI are the only network use, and only when you choose one. New dependencies must not phone home.
- Coach, don't block: never grab input devices.

## Reporting

- **Bugs:** use the bug template, and paste `flowbuddy doctor --report`; it has nothing private in it.
- **A tip that was wrong or not helpful:** the "wrong tip" template. `flowbuddy tips` shows the text.
- **Security:** never in a public issue; see [SECURITY.md](SECURITY.md).

Everyone taking part follows the [code of conduct](CODE_OF_CONDUCT.md).
