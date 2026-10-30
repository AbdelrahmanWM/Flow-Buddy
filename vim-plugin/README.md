# Flow Buddy for Vim and Neovim

Tells the local `flowbuddyd` what Vim knows and keystrokes don't: the real mode, whether this Vim has focus, and where each movement ended. With it, Flow Buddy can coach `}` instead of `jjjjjj` only when `}` would really have got you there, and it works for Vim inside tmux or started by `git commit`.

## Install

`scripts/install.sh` installs it as a package for both editors:

- Vim 9 (`+channel`): `~/.vim/pack/flowbuddy/start/flowbuddy`
- Neovim 0.9+: `~/.local/share/nvim/site/pack/flowbuddy/start/flowbuddy`

It does nothing when the daemon isn't running, and retries every few seconds. To turn it off: `let g:flowbuddy_enabled = 0`.

The facts are worked out by trying motions on the buffer, which stops after 15 ms per movement so long files and slow folding stay responsive (`let g:flowbuddy_facts_budget_ms = 30` to allow more). `:echo flowbuddy#stats()` shows how many movements were looked at, how many ran out of time, and the slowest in milliseconds.

## Tips

Tips for what you do in Vim show in Vim, in a small window at the top right, for a few seconds (`coaching.nudge_seconds`). It doesn't take the cursor; a click closes it. `:FlowBuddyGotIt` and `:FlowBuddyNotHelpful` answer the tip on show, like the top-bar menu does. To have them on the desktop instead: `flowbuddy config set coaching.show_in desktop`, or `let g:flowbuddy_tips = 0` for this Vim only. Needs Vim's popup windows (`+popupwin`) or Neovim.

## Practice and the skill map

`:FlowBuddyPractice` runs a two-minute drill on your weakest habit in the Vim you're in (`:FlowBuddyPractice delete` for a given one; Tab completes the names), and reports how it went when it ends; `:bd` closes it. `:FlowBuddyMap` shows every skill as a tile in a split. Both need the `flowbuddy` command.

From a terminal, `flowbuddy practice` starts Vim with `flowbuddy#practice#run`, which runs the drill in a scratch buffer and counts your keys with buffer-local mappings that pass each key through. `:q` stops early; the rounds done are kept.

## Over ssh

Run `flowbuddy ssh-config <host>` on your machine and add what it prints to `~/.ssh/config`. It forwards Flow Buddy's socket to the remote host as `/tmp/flowbuddy-<you>.sock` (readable only by you), where the plugin looks for it whenever `$SSH_CONNECTION` is set. Install the plugin on the remote host too. Vim there then counts while its terminal has focus, if your terminal and ssh pass focus events through.

## What it sends

Over the daemon's Unix socket (`$FLOWBUDDY_SOCKET`, or `$XDG_RUNTIME_DIR/flowbuddy.sock`), as described in [docs/architecture.md](../docs/architecture.md#editor-clients):

- `hello` with the editor's name and process id.
- `editor_mode` on every mode change.
- `editor_focus` on focus gained/lost (needs a terminal with focus events; in tmux, `set -g focus-events on`) and on suspend/resume.
- `motion` once the cursor rests (1 s, `g:flowbuddy_rest_ms`) after moving in Normal or Visual mode: the line and column deltas, how many moves, and **facts**: numbers and flags such as "`}` twice lands here" or "`f` plus the target character, then `;` once".

- `operator` after `d`, `c` or `y` (from `TextYankPost`, before the text is removed): the names of the text objects its range was exactly, such as `i(` or `aw`.

It never sends text, characters, file names, paths or positions. Facts are worked out inside Vim and only the counts leave it.

## How facts are worked out

For motions with no side effects (`}`, `{`, `w`, `b`, `e`, `ge`, `W`, `B`, `%`), the plugin runs them from the start position with `keepjumps normal!`, counts the presses that land exactly on the end, and then restores the view. Autocommands are ignored while it does this, and the jumplist, registers and search pattern are untouched. `f`/`t` and `*`/`#` are counted from the line text and `searchpos()` instead, because running them would change your `;` and `n`. Facts are only worked out in Normal mode, never during a Visual selection or a pending operator.

## Tests

```sh
vim-plugin/test/run.sh
```

runs headless and checks each fact against hand-written buffers. Neovim support follows the same code path, with `sockconnect` in place of `ch_open`, but it hasn't been tested yet.
