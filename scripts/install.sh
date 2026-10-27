#!/usr/bin/env bash
# Install Flow Buddy for the current user.
#
#   scripts/install.sh               build and install everything
#   scripts/install.sh --dry-run     print what would be done
#   scripts/install.sh --with-udev   also install the keyboard access rule (sudo)
#   scripts/install.sh --yes         don't ask before editing ~/.bashrc
#
# Installs:
#   ~/.local/bin/flowbuddyd, ~/.local/bin/flowbuddy
#   ~/.config/systemd/user/flowbuddyd.service (enabled and started)
#   ~/.config/systemd/user/flowbuddy-weekly.service (only runs with [ai] weekly)
#   ~/.local/share/gnome-shell/extensions/flowbuddy@abdelrahmanwm.github.io/
#   ~/.local/share/flowbuddy/flowbuddy.bash, sourced from ~/.bashrc
#   ~/.vscode/extensions/flowbuddy.flowbuddy-vscode-<version>/ (if VS Code is installed)
#   ~/.vim/pack/flowbuddy/start/flowbuddy/ (if Vim is installed)
#   ~/.local/share/nvim/site/pack/flowbuddy/start/flowbuddy/ (if Neovim is installed)
#   /etc/udev/rules.d/70-flowbuddy.rules (only with --with-udev)

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
uuid=flowbuddy@abdelrahmanwm.github.io
bin_dir=$HOME/.local/bin
unit_dir=${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user
ext_dir=${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/$uuid
share_dir=${XDG_DATA_HOME:-$HOME/.local/share}/flowbuddy
vscode_dir=$HOME/.vscode/extensions
vscode_version=$(sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' "$repo/vscode-extension/package.json")
vscode_ext=$vscode_dir/flowbuddy.flowbuddy-vscode-$vscode_version
bashrc=$HOME/.bashrc
udev_rule=/etc/udev/rules.d/70-flowbuddy.rules
marker_begin="# >>> flowbuddy >>>"
marker_end="# <<< flowbuddy <<<"

dry_run=0 with_udev=0 yes=0
for arg in "$@"; do
    case $arg in
        --dry-run) dry_run=1 ;;
        --with-udev) with_udev=1 ;;
        --yes | -y) yes=1 ;;
        -h | --help) sed -n '2,17p' "$0"; exit 0 ;;
        *) echo "unknown option: $arg" >&2; exit 2 ;;
    esac
done

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
run() {
    if ((dry_run)); then
        printf '    + %s\n' "$*"
    else
        "$@"
    fi
}
ask() {
    ((yes)) && return 0
    local reply
    read -r -p "$1 [y/N] " reply
    [[ $reply == [yY]* ]]
}

command -v cargo >/dev/null || { echo "cargo not found: install Rust from https://rustup.rs" >&2; exit 1; }

say "Building release binaries"
run cargo build --release --manifest-path "$repo/Cargo.toml" -p flowbuddy-daemon -p flowbuddy-cli

say "Installing binaries to $bin_dir"
run install -Dm755 "$repo/target/release/flowbuddyd" "$bin_dir/flowbuddyd"
run install -Dm755 "$repo/target/release/flowbuddy" "$bin_dir/flowbuddy"
case :$PATH: in
    *:"$bin_dir":*) ;;
    *) echo "    note: $bin_dir is not on your PATH" ;;
esac

say "Installing the bash hook"
run install -Dm644 "$repo/shell/flowbuddy.bash" "$share_dir/flowbuddy.bash"
if grep -qF "$marker_begin" "$bashrc" 2>/dev/null; then
    echo "    ~/.bashrc already sources it"
elif ask "Add Flow Buddy (and 'set -o vi') to the end of ~/.bashrc?"; then
    block=$(printf '%s\n%s\n%s\n%s\n' "$marker_begin" "set -o vi" \
        "[[ -f \"$share_dir/flowbuddy.bash\" ]] && source \"$share_dir/flowbuddy.bash\"" "$marker_end")
    if ((dry_run)); then
        printf '    + append to %s:\n        %s\n' "$bashrc" "${block//$'\n'/$'\n'        }"
    else
        printf '\n%s\n' "$block" >>"$bashrc"
    fi
else
    echo "    skipped; add this to ~/.bashrc yourself:"
    echo "        set -o vi"
    echo "        source \"$share_dir/flowbuddy.bash\""
fi

say "Installing the GNOME Shell extension"
run mkdir -p "$ext_dir"
for f in "$repo"/gnome-extension/*.js "$repo"/gnome-extension/*.json "$repo"/gnome-extension/*.css; do
    [[ -e $f ]] || continue
    run install -m644 "$f" "$ext_dir/${f##*/}"
done
# A shell that hasn't seen the extension yet refuses to enable it, so also
# add it to the list GNOME reads at login.
if ! { command -v gnome-extensions >/dev/null && run gnome-extensions enable "$uuid" 2>/dev/null; }; then
    if command -v gsettings >/dev/null; then
        enabled=$(gsettings get org.gnome.shell enabled-extensions)
        if [[ $enabled != *"'$uuid'"* ]]; then
            if [[ $enabled == *"[]" ]]; then
                list="['$uuid']"
            else
                list="${enabled%]}, '$uuid']"
            fi
            run gsettings set org.gnome.shell enabled-extensions "$list"
        fi
        echo "    enabled; it starts after you log out and back in"
    else
        echo "    enable it after logging back in: gnome-extensions enable $uuid"
    fi
fi

if command -v code >/dev/null || [[ -d $HOME/.vscode ]]; then
    say "Installing the VS Code companion"
    # Same folder for the snap and the .deb. Older versions are replaced.
    for old in "$vscode_dir"/flowbuddy.flowbuddy-vscode-*; do
        if [[ -e $old && $old != "$vscode_ext" ]]; then
            run rm -rf "$old"
        fi
    done
    run mkdir -p "$vscode_ext"
    for f in "$repo"/vscode-extension/*.js "$repo"/vscode-extension/*.json "$repo"/vscode-extension/README.md; do
        run install -m644 "$f" "$vscode_ext/${f##*/}"
    done
    # VS Code only loads extensions listed in extensions.json (and skips
    # ones in .obsolete), so register it there. Without the file, VS Code
    # scans the folder itself on its next start.
    if ((dry_run)); then
        printf '    + register flowbuddy.flowbuddy-vscode in %s\n' "$vscode_dir/extensions.json"
    else
        python3 - "$vscode_dir" "$vscode_ext" "$vscode_version" <<'PY'
import json, os, sys, time
root, location, version = sys.argv[1:]
ext_id, name = "flowbuddy.flowbuddy-vscode", os.path.basename(location)

def rewrite(path, change):
    try:
        with open(path) as f:
            data = json.load(f)
    except FileNotFoundError:
        return
    with open(path + ".flowbuddy", "w") as f:
        json.dump(change(data), f)
    os.replace(path + ".flowbuddy", path)

rewrite(os.path.join(root, "extensions.json"), lambda entries: [
    e for e in entries if e.get("identifier", {}).get("id") != ext_id
] + [{
    "identifier": {"id": ext_id},
    "version": version,
    "location": {"$mid": 1, "path": location, "scheme": "file"},
    "relativeLocation": name,
    "metadata": {"installedTimestamp": int(time.time() * 1000), "source": "vsix"},
}])
rewrite(os.path.join(root, ".obsolete"), lambda d: {k: v for k, v in d.items() if k != name})
PY
    fi
    echo "    restart VS Code to load it (or install a .vsix with 'code --install-extension')"
fi

# The same plugin serves Vim and Neovim.
vim_packs=()
command -v vim >/dev/null && vim_packs+=("$HOME/.vim/pack/flowbuddy/start/flowbuddy")
command -v nvim >/dev/null && vim_packs+=("${XDG_DATA_HOME:-$HOME/.local/share}/nvim/site/pack/flowbuddy/start/flowbuddy")
if ((${#vim_packs[@]})); then
    say "Installing the Vim plugin"
    for pack in "${vim_packs[@]}"; do
        for f in plugin/flowbuddy.vim autoload/flowbuddy.vim README.md; do
            run install -Dm644 "$repo/vim-plugin/$f" "$pack/$f"
        done
    done
fi

say "Installing the systemd user service"
run install -Dm644 "$repo/packaging/systemd/flowbuddyd.service" "$unit_dir/flowbuddyd.service"
run install -Dm644 "$repo/packaging/systemd/flowbuddy-weekly.service" "$unit_dir/flowbuddy-weekly.service"
run systemctl --user daemon-reload
run systemctl --user enable --now flowbuddyd.service

if ((with_udev)); then
    say "Installing the keyboard access rule (needs sudo)"
    run sudo install -m644 "$repo/packaging/udev/70-flowbuddy.rules" "$udev_rule"
    run sudo udevadm control --reload
    run sudo udevadm trigger --subsystem-match=input --action=change
    run systemctl --user restart flowbuddyd.service
fi

say "Done"
cat <<MSG
    Next:
      - Wayland loads new extensions on the next login: log out and back in.
      - Check everything with: flowbuddy status
MSG
if ((!with_udev)); then
    cat <<MSG
      - Flow Buddy can't read your keyboard yet. Either re-run with
        --with-udev (recommended: active session only), or see
        docs/privacy.md#keyboard-access for the alternatives.
MSG
fi
