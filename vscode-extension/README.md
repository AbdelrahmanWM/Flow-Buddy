# Flow Buddy for VS Code

A companion extension for [Flow Buddy](../README.md). The daemon only knows which *window* is focused, so inside VS Code it can't tell the editor from the integrated terminal or a chat panel (such as Claude Code's), and it has to guess vscodevim's mode from your keys. This extension tells it:

- whether a text editor has focus (`editor_focus`),
- vscodevim's real mode (`editor_mode`),
- how the cursor moved in Normal and Visual mode (`motion`), with facts such as "`}` twice lands here".

Plain JavaScript, no build step and no dependencies. `scripts/install.sh` installs it; it does nothing until the daemon is running, and reconnects when the daemon restarts.

## Privacy

The extension sends only the fields in `crates/core/src/editor.rs`: flags, modes, counts and deltas. It never sends text, characters, file names, paths or absolute positions. Motion facts are computed here from the text, and only their numbers leave the extension (for example "`f` plus the character at the end, then `;` twice", without the character). The daemon rejects any field it doesn't know, and it is on the same machine: nothing goes over the network.

## Editor focus

VS Code has no API for "the editor has keyboard focus", so this is a heuristic.

Sent as `true`, as a heartbeat at most every 500 ms (plus one trailing beat so the last activity is never more than 500 ms older than the last beat):

- the cursor or selection moves in the active editor by keyboard, mouse or a command (vscodevim moves the cursor with commands),
- the active editor's text changes within a line of a cursor (typing),
- another editor becomes active,
- the window regains focus while the editor had it before.

Sent as `false`:

- the VS Code window loses focus,
- a terminal becomes active, is first typed into, or starts a command,
- the active tab becomes a terminal, webview or custom editor,
- no text editor is active, or the active one is a panel (output, comments, the commit message box, chat inputs).

Limits:

- **Webview panels show nothing.** Typing in Claude Code's panel, or any other webview in the sidebar or panel, fires no event at all. The editor's last heartbeat simply gets old, and the daemon treats keys that arrive without recent heartbeats as not going to the editor.
- A cursor moved by an edit (no selection-change kind) doesn't count, so an extension or tool rewriting the open file while you type in a chat doesn't look like editor activity. An edit landing next to your cursor still does.
- Another extension that opens an editor, or moves its cursor with a command, while you type elsewhere looks like activity. The heartbeat then stops, and the daemon falls back as above.
- Normal-mode keys that move nothing (`j` on the last line, `:`) send no heartbeat.

## Mode

vscodevim exports no API, but it sets the cursor style on every mode change: your `vim.cursorStylePerMode.<mode>` setting if there is one, otherwise a fixed style per mode (vscodevim 1.32):

| Mode | Default cursor |
|---|---|
| Normal | block |
| Insert | `editor.cursorStyle` (line by default) |
| Visual, Visual Line, Visual Block | line-thin |
| Replace | underline |
| Command line, search, operator pending | underline-thin |

The extension reads the active editor's cursor style and your settings, and finds the modes that use that style. Visual modes always select something (except on an empty line) and Normal never does, which separates them when they share a style. Visual, Visual Line and Visual Block are told apart by the selection's shape: several single-line selections on consecutive lines are Visual Block, one selection of whole lines is Visual Line, anything else is Visual. Whatever is still ambiguous, such as the command line versus operator pending, is sent as `other`, which makes the daemon fall back to its own guess. Without vscodevim, or with `vim.disableExtension`, the mode is `other`.

Limits: a charwise selection of exactly whole lines reads as Visual Line, a one-line Visual Block reads as Visual, and on an empty line the previous Visual kind is kept. vscodevim's EasyMotion mode looks like Normal, and "Vim disabled" via its toggle command looks like Insert.

## UI events

For the VS Code pack (`flowbuddy rules` lists `vscode.*`), the companion reports when a file opens in a new tab, as the bare event `file_opened`: never which file. The daemon adds whether a click or `Ctrl-P` came just before, and the same for the terminal panel taking focus.

## Text objects

After a delete or change in Normal or Visual mode, the companion works out which text objects (`iw`, `aw`, `i(`, `a"`, `ip`, `ap`…) the removed range was exactly, and sends only their names. VS Code doesn't say what an edit removed, so while you're in Normal or Visual mode it keeps a copy of the active document's lines (documents over 20 000 lines are skipped). The copy stays in the extension and is dropped when you start typing in Insert mode. Yanks don't change the document, so they aren't seen; the Vim plugin sees them.

## Motions

In Normal, Visual and Visual Line mode, a movement starts at the first cursor move and ends when the cursor has rested for a second, the mode changes or you click. Edits and switching editors drop it without sending anything, and nothing is sent in Insert mode or Visual Block.

Each movement sends its start and end time (Unix ms), the number of cursor moves seen, the mode, the line and column deltas and its facts. In Visual Line mode the column isn't known, so `cols` is 0 and only the vertical facts are sent.

The facts are computed in `facts.js`, which follows Vim's own code for `w`/`b`/`e`/`ge`/`W`/`B`, `}`/`{`, `f`/`t` with `;`, `%` and `*`/`#`, and the semantics in `crates/core/src/editor.rs`. A fact is left out rather than guessed: word facts next to characters outside Latin-1, `%` on preprocessor lines, in C comments or with unpaired quotes, and anything that would need the last search (`search_hops`) or the jump list (`jump_back`). `*`/`#` honour `vim.ignorecase` and don't wrap around the file.

## Tests

```sh
gjs vscode-extension/test/facts.test.js
gjs vscode-extension/test/tips.test.js
```

The tests run `facts.js` on small hand-written buffers. No node needed.

## Tips

Tips for what you do in VS Code show in the status bar at the bottom left, for a few seconds. Hover for the whole tip and what its commands do; click it, or use **Flow Buddy: Got it** / **Not helpful** from the Command Palette, to answer it. To have them on the desktop instead: `flowbuddy config set coaching.show_in desktop`.

## Settings and tools

In the Command Palette: **Flow Buddy: Settings…** (the basics, then everything else and each rule), **Check setup**, **Progress**, **Today** and **Recent tips**. They run the `flowbuddy` command, so it needs to be installed (`scripts/install.sh`).
