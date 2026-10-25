# Spike: a Vim/Neovim plugin that reports to the daemon (#59)

Tested on 2026-10-25 with Vim 9.1 (patches 1–2141, `+channel +job +timers`). Neovim isn't installed on the test machine.

## Recommendation

A single plugin in legacy Vim script (`vim-plugin/`) that runs in both Vim 9 and Neovim. Only the socket calls differ, so the same code covers both:

| | Vim 9 | Neovim 0.9+ |
|---|---|---|
| Connect | `ch_open('unix:' . path, {'mode': 'nl'})` | `sockconnect('pipe', path, {...})` |
| Send | `ch_sendraw(ch, json . "\n")` | `chansend(ch, json . "\n")` |
| Epoch ms | `date +%s%3N` once at start, then `reltime()` | `vim.uv.gettimeofday()` once, then `reltime()` |

A Lua plugin for Neovim alone would be nicer there, but two plugins with the same logic would drift apart.

## Findings

- `ch_open` over a Unix socket works in Vim 9.1, but the `waittime` option is rejected with E475. Leave it out: connecting to a Unix socket doesn't block.
- `ModeChanged` (Vim 8.2.3430+, Neovim 0.7+) gives every mode change, including Operator-pending (`no`) and Visual block.
- `CursorMoved` fires once per batch of typeahead, not once per key: `6j` typed very fast, or keys fed in one go, can show as a single move. So the plugin reports how many moves it *saw*, and the daemon counts the keys you pressed from its own key stream, using `start_ms` and `end_ms` to line them up.
- Facts can be found exactly by running the motions themselves from the start position (`keepjumps normal! }` …) with `eventignore=all`, then restoring the view. The tests check that the view, jumplist and search register are unchanged afterwards. Motions that change user state (`f`/`t`, which set `;`; `*`/`#`, which set `/`) are counted from the text and with `searchpos()` instead.
- Running motions during a Visual selection or a pending operator would break them, so facts are only worked out in plain Normal mode. Otherwise the rest timer retries 300 ms later.
- `FocusGained`/`FocusLost` work in terminals with focus reporting (VTE-based terminals such as GNOME Terminal and Ptyxis do). tmux passes them through only with `set -g focus-events on`.

## Matching the plugin to the focused window

The plugin sends `getpid()`. The daemon walks parents from that pid to the focused terminal's pid, just as it does for shells. GNOME Terminal serves every window from one process, so a Vim in another window matches too. As with shells, the most recently active one wins, unless the plugin has reported focus.

Vim inside tmux or over ssh isn't a child of the terminal. There, a plugin that reported `editor_focus: true` most recently wins while a terminal has focus (see `crates/daemon/src/resolver.rs`).

## Not done

- Neovim: same code path, untested.
- Vim inside ssh: the plugin runs on the remote host and can't reach the local socket. Forwarding the socket (`ssh -R`) would work, but is out of scope for now.
