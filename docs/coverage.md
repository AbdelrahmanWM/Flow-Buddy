# What's coached where

Flow Buddy coaches in five places. Each one supports a different part of Vim, so a tip there only suggests commands that work there (`crates/core/src/support.rs`). A tip that would have nothing left isn't shown.

| | VS Code (vscodevim) | Vim / Neovim in a terminal | vi-mode shell (bash, zsh, fish) | Claude Code's Vim mode | Shell commands |
|---|---|---|---|---|---|
| How it knows where you are | the companion extension | the Vim plugin, or the shell hook | the shell hook | the shell hook + `~/.claude/settings.json` | the shell hook |
| Exact motion tips (`}}w`, `Ctrl-d×3 then 4j`) | ✓ | ✓ with the plugin | | | |
| Repeated keys (`jjjj`, `xxxx`, `llll`) | ✓ | ✓ | `xxxx`, `llll` | `xxxx`, `llll` | |
| Arrows, Backspace runs | ✓ | ✓ | history, Backspace, Left arrow | ✓ | |
| Change instead of delete + insert (`cw`, `s`, `C`) | ✓ | ✓ | ✓ | `cw`, `C`, `cc` | |
| One-key forms (`A`, `I`, `D`, `ZQ`) | ✓ | ✓ | `A`, `I`, `D`, `C` | `A`, `I`, `o`, `O`, `D` | |
| `.`, macros, `cgn`, block edits, registers | ✓ | ✓ | `.` | | |
| Counts for operators (`3dw`), `>>`, `J`, `p` | ✓ | ✓ | `3dw` | | |
| Undo runs, searches again, `:s` again | ✓ (no `:earlier`, no `g&`) | ✓ | | | |
| Text objects (`ciw`, `di(`) and shorter operator motions | ✓ with the companion | ✓ with the plugin | | | |
| Scrolling, marks, `g;`, `Ctrl-o`, `42G` | ✓ | ✓ with the plugin | | | |
| The same selection again (`gv`) | ✓ with the companion | ✓ with the plugin | | | |
| Windows and buffers | ✓ (editor groups) | ✓ | | | |
| Editor UI: files, terminal, tabs and splits by click | ✓ | | | | |
| `cd ..` twice, last argument again, one word changed, `clear`, `history` | | | | | ✓ |

## What each one can't do

- **vscodevim** has no `:earlier`/`:later`, `:g`/`:v`, `:wincmd`, `:changes`, `:undolist`, `g-`/`g+` or `g&`. This was checked against vscodevim 1.32.4's command table.
- **Claude Code's Vim mode** has movement, `x`, `d`/`c`/`y` with motions and text objects, `.`, `>>`, `J` and `p`. It has no Visual mode, search, marks, macros, `s`, `r`, `~` or `:` commands.
- **vi-mode shells** edit one line. They have no text objects (bash and fish), Visual mode, registers, `o`/`O` or `:` commands.

## Not coached yet

- Folding (`zc`, `zo`), the quickfix list, `:make`, and spelling.
- Insert-mode completion (`Ctrl-n`, `Ctrl-x Ctrl-f`), because what was completed can't be seen without text.
- `42G` when absolute line numbers aren't shown (the number would have to be sent).
- In VS Code: the command palette and sidebar by mouse (VS Code doesn't tell extensions), and multi-cursor.
- KDE on Wayland, browsers (Vimium) and JetBrains (IdeaVim): #123–#125.
