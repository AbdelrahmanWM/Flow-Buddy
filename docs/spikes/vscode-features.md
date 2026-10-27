# Spike: what the VS Code pack can see (#101)

Done on 2026-10-26 against the VS Code 1.143 extension API.

| Habit | What the companion can see | What the daemon adds | Result |
|---|---|---|---|
| Clicking through the explorer to open files | `tabGroups.onDidChangeTabs`: a text tab opened | A click just before, and no `Ctrl-P` | `vscode.quick-open` |
| Clicking into the terminal panel | Focus left the editor for the terminal (`panel: terminal`) | A click just before, and no `` Ctrl-` `` | `vscode.terminal-toggle` |
| Scrolling to find a symbol (instead of `Ctrl-Shift-O`, `F12`) | Scrolling, but not what was looked for | — | Not done: can't tell searching from reading |
| Repeating an edit on several lines (instead of multi-cursor) | Edits, but the Vim pack's macro rule already sees the keys | — | Covered by `vim.macro` |
| Commands run from the palette or menus | No API reports executed commands | — | Not possible without one |

Both rules need three clicks within ten minutes before they nudge, because a single click can just as well be go-to-definition or a search result. The file opened is never reported, only that one did.
