# Changelog

What changed in each release. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/). Before 1.0, a minor version may change the config file or the IPC protocol; the notes say how.

## [Unreleased]

## [0.1.0] - unreleased

The first public release.

### Coaching

- Tips for a shorter way to do what you just did, in Vim and Neovim (terminal, gVim, Neovide), VS Code with vscodevim, vi-mode bash, zsh and fish, and Claude Code's Vim mode. One tip at a time, with the keys it took and the keys that would do.
- Exact motion tips from the editor plugins (`}}w`, `Ctrl-d×3 then 4j`, `/` and a few letters, marks, `g;`, `42G`), text objects and shorter operator motions, `.`, macros, `cgn`, counts, undo runs and `:s` again.
- Mistakes shown straight away: an unknown Ex command, a look-alike undo, typing in Normal mode, Caps Lock.
- A click to move the cursor gets the keys that would do it.
- Shell commands: `cd ..` twice, the last argument again, one word changed, `clear`, `history`.
- VS Code: files, the terminal, tabs and splits opened by mouse.
- Each place only gets tips for commands it supports (vscodevim, Claude Code and the vi-mode shells each lack some).

### Learning

- A curriculum of 178 items covering every command in Vim's `:help index`, each one coached, taught or skipped with a reason.
- What you already use is learned from your keys (knowledge tracing, with half-life recall), so tips move on as you do.
- A daily lesson with a worked example, and reviews for what you stop using.
- `flowbuddy practice`: two-minute drills in Vim that count your keys against the fewest that would do.
- `flowbuddy learn --map`: every skill as a tile. `next`, `summary`, `tips`, `explain` and `watch`.
- Tip outcomes, a learned tip filter, and a 10% holdout to check that tips help (`learn --report`).

### AI (optional, off by default)

- `flowbuddy coach`, the weekly note, `flowbuddy review` and AI-written tips, with Claude Code, a local command such as ollama, the Anthropic API or an OpenAI-compatible API.
- Only habits and counts are sent. Code snippets are sent only if you opt in, are redacted first, and never come from files that look secret.

### Setting up

- `scripts/install.sh` for the daemon, CLI, GNOME extension, shell hooks, Vim plugin and VS Code companion; `scripts/uninstall.sh --purge` removes everything.
- `flowbuddy tour`, `setup`, `doctor`, `config`, and profiles. Also a GNOME settings window, VS Code palette commands and a VS Code walkthrough.
- Prebuilt binaries for Linux x86_64 and aarch64.

### Privacy

- Keys are reduced to counts and habits in memory. Typed text, file names and window titles are never stored or sent.
- The daemon makes no network connections, and a test enforces it.
- See [docs/privacy.md](docs/privacy.md).

[Unreleased]: https://github.com/AbdelrahmanWM/Flow-Buddy/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/AbdelrahmanWM/Flow-Buddy/releases/tag/v0.1.0
