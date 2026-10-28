# What's coached where

Flow Buddy coaches in five places. Each one supports a different part of Vim, so a tip there only suggests commands that work there (`crates/core/src/support.rs`). A tip that would have nothing left isn't shown.

| | VS Code (vscodevim) | Vim / Neovim in a terminal | vi-mode shell (bash, zsh, fish) | Claude Code's Vim mode | Shell commands |
|---|---|---|---|---|---|
| How it knows where you are | the companion extension | the Vim plugin, or the shell hook | the shell hook | the shell hook + `~/.claude/settings.json` | the shell hook |
| Exact motion tips (`}}w`, `Ctrl-d×3 then 4j`) | ✓ | ✓ with the plugin | | | |
| Repeated keys (`jjjj`, `xxxx`, `llll`) | ✓ | ✓ | `xxxx`, `llll` | `xxxx`, `llll` | |
| Arrows, Backspace runs | ✓ | ✓ | history, Backspace, Left arrow | ✓ | |
| Change instead of delete + insert (`cw`, `s`, `C`) | ✓ | ✓ | ✓ | ✓ | |
| One-key forms (`A`, `I`, `D`, `ZQ`) | ✓ | ✓ | `A`, `I`, `D`, `C` | `A`, `I`, `o`, `O`, `D` | |
| `.`, macros, `cgn`, block edits, registers | ✓ | ✓ | `.` | | |
| Counts for operators (`3dw`), `>>`, `J`, `p` | ✓ | ✓ | `3dw` | | |
| Undo runs, searches again, `:s` again | ✓ (no `:earlier`, no `g&`) | ✓ | | | |
| Text objects (`ciw`, `di(`) and shorter operator motions | ✓ with the companion | ✓ with the plugin | | | |
| Scrolling, marks, `g;`, `Ctrl-o`, `42G` | ✓ | ✓ with the plugin | | | |
| `}}` then `2j`, `/` a few letters, `]]`, `[(` | ✓ | ✓ with the plugin | | | |
| `(` `)` sentences, `gj` `gk` on wrapped lines | | ✓ with the plugin | | | |
| The same selection again (`gv`) | ✓ with the companion | ✓ with the plugin | | | |
| Windows and buffers | ✓ (editor groups) | ✓ | | | |
| Editor UI: files, terminal, tabs and splits by click | ✓ | | | | |
| `cd ..` twice, last argument again, one word changed, `clear`, `history` | | | | | ✓ |

## What each one can't do

- **vscodevim** has no `:earlier`/`:later`, `:g`/`:v`, `:wincmd`, `:changes`, `:undolist`, `g-`/`g+` or `g&`. This was checked against vscodevim 1.32.4's command table.
- **Claude Code's Vim mode** (checked against Claude Code 2.1.312's own key tables) has counts, motions including `%`, `gg`, `gj`/`gk`, `f`/`t` with `;`, operators with text objects, Visual and Visual Line mode, `x`, `s`, `r`, `~`, `J`, `p`, `.` and `u`. It has no search (`/` searches the prompt history), `*`/`n`, marks, macros, registers, redo, `:` commands, or other `g` commands (`gv`, `g;`, `gI`, `g~`).
- **vi-mode shells** edit one line. They have no text objects (bash and fish), Visual mode, registers, `o`/`O` or `:` commands.

## Taught, but not coached

These are in the curriculum, so the daily "something new" teaches them and `flowbuddy learn` tracks them, but no rule spots a habit that calls for them:

- Folding (`za`, `zc`, `zo`), the quickfix list (`:cn`, `:cp`), `gn` on its own, sentence objects (`das`, `cis`) and `Ctrl-v` then `$A`.
- Insert-mode completion (`Ctrl-n`): Flow Buddy can't see that you used it, so say so with `flowbuddy learn --know completion`.
- In the shell, `Alt-.` and `^old^new` the same way; `cd -` is seen through the shell hook.

## Not coached yet

- `:make` and spelling.
- `42G` when absolute line numbers aren't shown (the number would have to be sent).
- In VS Code: the command palette and sidebar by mouse (VS Code doesn't tell extensions), and multi-cursor.
- KDE on Wayland, browsers (Vimium) and JetBrains (IdeaVim): #123–#125.
