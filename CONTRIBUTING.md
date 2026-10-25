# Contributing

Thanks for wanting to help. Flow Buddy reads keyboards, so changes are reviewed for privacy as well as correctness.

## Layout

| Path | What |
|---|---|
| `crates/core` | Shared types and logic: keys, Vim mode tracking, rules and packs, engine, coaching policy, learning stages, config, IPC protocol |
| `crates/store` | SQLite stats and the mistake memory, export format |
| `crates/daemon` | `flowbuddyd`: evdev capture, keymap, IPC server, context resolver, the `App` state machine |
| `crates/cli` | `flowbuddy` |
| `gnome-extension/` | Focus reporting and the top-bar indicator (GJS, GNOME 50) |
| `shell/flowbuddy.bash` | Bash hook |
| `docs/` | [architecture](docs/architecture.md), [privacy](docs/privacy.md), [memory](docs/memory.md), [writing rules](docs/writing-rules.md) |

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

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
- New rules follow [docs/writing-rules.md](docs/writing-rules.md), including its privacy checklist.
- Anything that changes what is captured, stored or sent must update [docs/privacy.md](docs/privacy.md) in the same PR.
- Changes to the memory export format must bump its version and update [docs/memory.md](docs/memory.md) and the golden test.

## Privacy rules for every change

- No raw keys or typed text in storage, logs, IPC or error messages.
- No networking. The service is restricted to Unix sockets, and new dependencies must not phone home.
- Coach, don't block: never grab input devices.
