# Flow Buddy

**Improve all the time without stopping to study.**

Flow Buddy is a local companion that helps you get better at your tools continuously. It watches how you work, spots inefficient patterns, and suggests a better way, quietly and only when it's useful.

> **Status:** early development. Nothing works yet. Track progress in the [milestones](https://github.com/AbdelrahmanWM/Flow-Buddy/milestones).

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

## MVP scope

The first rule pack covers Vim in VS Code (vscodevim), Vim in the terminal, and bash vi-mode. Example nudges:

| You did | Try |
|---|---|
| `jjjjjj` | `5j`, `}` or `Ctrl-d` |
| `xxxx` | `dw` / `de` |
| Arrow keys in Normal mode | `h` `j` `k` `l` |
| `:w` then `:q` | `:wq` or `ZZ` |
| `↑↑↑↑` hunting shell history | `Ctrl-r` |

Later packs: VS Code feature discovery, terminal and shell efficiency, English writing feedback, and typing accuracy.

## Platform

Linux, initially Ubuntu with GNOME on Wayland.

## Building

```sh
cargo build --workspace
cargo test --workspace
```

## License

[MIT](LICENSE)
