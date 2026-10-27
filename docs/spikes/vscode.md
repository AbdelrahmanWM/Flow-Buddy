# Spike: what a VS Code extension can observe (#54)

Done on 2026-10-25 against VS Code 1.143.1 (snap) with vscodevim 1.32.4. The result is the companion in `vscode-extension/`, which was tested in a real VS Code and works: typing in the integrated terminal and in Claude Code's chat panel is no longer coached as Vim, the mode shows as "from the editor", and motions arrive with facts.

## Signals

| Question | Signal | Reliability |
|---|---|---|
| Does a text editor have focus? | Selection changes with a kind (Keyboard, Mouse, Command), text changes near a cursor, active editor changes | Good while you use the editor. There is no "editor focused" API, so it's a heartbeat |
| Focus left the editor | Window blur; terminal activated, typed into, or running a command; active tab is a terminal, webview or custom editor; panel documents (output, SCM input, chat inputs) | Good for terminals and tabs |
| Typing in a webview (chat panels in the sidebar) | **Nothing fires** | None. The daemon treats keys with no recent heartbeat (2 s) as not going to the editor |
| vscodevim's mode | The active editor's `options.cursorStyle`, which vscodevim sets per mode (`vim.cursorStylePerMode.*` or its defaults), plus the selection's shape for the Visual kinds | Good. Ambiguous cases are sent as `other` so the daemon guesses instead |
| Context keys (`editorTextFocus`, `vim.mode`) | Not readable from extensions | — |
| vscodevim API | Exports nothing | — |
| Cursor movement | `onDidChangeTextEditorSelection`; facts computed from the document text inside the extension | Good. In Visual Line mode the column isn't known |

## Findings that shaped the design

- The extension host's pid descends from the window's pid, also under the snap, so the daemon can match the companion to the focused window.
- Under the snap, `XDG_RUNTIME_DIR` can't be trusted; the companion builds the socket path from its uid.
- Selection changes with no kind come from edits. Counting them would make a tool rewriting the open file look like editor activity, so they're ignored.
- Heartbeats are throttled to one every 500 ms, plus a trailing beat. The daemon's 2 s window covers that and the delay between a key and vscodevim handling it.

Full details, including every heuristic and its limits: [vscode-extension/README.md](../../vscode-extension/README.md).
