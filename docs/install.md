# Installing Flow Buddy

You need Linux with GNOME Shell 50 (or any X11 desktop), and bash, zsh or fish.

## From a release (no Rust needed)

Download `flowbuddy-vX.Y.Z-linux-x86_64.tar.gz` (or `aarch64`) from [Releases](https://github.com/AbdelrahmanWM/Flow-Buddy/releases), check it against its `.sha256`, and unpack it:

```sh
sha256sum -c flowbuddy-*-linux-x86_64.tar.gz.sha256
tar -xzf flowbuddy-*-linux-x86_64.tar.gz && cd flowbuddy-*/
scripts/install.sh --dry-run      # see what it will do
scripts/install.sh --with-udev    # install; --with-udev lets the active session read keyboards (sudo)
```

The binaries use `libxkbcommon` to read keys in your keyboard layout; every GNOME desktop has it.

## From source

With Rust (via [rustup](https://rustup.rs), 1.89 or newer):

```sh
git clone https://github.com/AbdelrahmanWM/Flow-Buddy && cd Flow-Buddy
scripts/install.sh --with-udev
```

For keyboard layouts other than US, install `libxkbcommon-dev` first.

## After installing

Then log out and back in so GNOME loads the extension, and take the tour:

```sh
flowbuddy tour            # how it works in seven short parts: checks your setup, shows a sample tip, asks the settings that matter
flowbuddy doctor          # every piece, and how to fix what's missing
flowbuddy status          # what it sees now; --watch, then switch windows
```

The tour is also **Tour** in the settings window's Tools, `:FlowBuddyTour` in Vim, and **Get started with Flow Buddy** in VS Code's Welcome page (or **Flow Buddy: Tour** in the command palette).

To update, pull and run `scripts/install.sh` again: it restarts the daemon, and GNOME picks up the new extension at the next login. Restart VS Code and Vim too.

`scripts/uninstall.sh` removes everything, and `--purge` also deletes your data.

## VS Code

`scripts/install.sh` also installs a small companion extension when VS Code is present (`~/.vscode/extensions/flowbuddy.flowbuddy-vscode-0.1.0`; restart VS Code afterwards). It tells the daemon whether the editor has focus, so typing in the terminal or a chat panel isn't coached as Vim, reads vscodevim's real mode, and reports cursor movements as counts and flags so motion rules can suggest a cheaper command. It never sends text or file names. See [vscode-extension/README.md](../vscode-extension/README.md).

## Vim and Neovim

`scripts/install.sh` also installs a plugin for Vim 9 and Neovim when they're present (`~/.vim/pack/flowbuddy`, `~/.local/share/nvim/site/pack/flowbuddy`). It reports the real mode, focus, and where each movement ended, as counts and flags only, so motion coaching works in the terminal too, including in tmux and in Vim started by `git commit`. See [vim-plugin/README.md](../vim-plugin/README.md).


## Keyboard access

`--with-udev` installs a rule that lets the person at the machine read keyboards, only while their session is active. The alternative, the `input` group, lets any program read every key; [privacy.md](privacy.md#keyboard-access) compares them.

## What it costs

Measured on one machine (a laptop, GNOME 50), a working day with Vim and VS Code:

- **CPU:** 8.9 seconds over 11 hours, under 0.03% on average.
- **Memory:** a 6.5 MB peak.
- **Disk:** the database was 360 KB; tip history and episodes are pruned after 90 days (`memory.episode_retention_days`).
- **Log:** about 150 lines a day at the default level, mostly starts and stops.

`flowbuddy doctor` flags a daemon from an older build, and `systemctl --user status flowbuddyd` shows its memory and CPU.
