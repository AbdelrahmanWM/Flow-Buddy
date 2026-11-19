#!/usr/bin/env bash
# Install Flow Buddy for the current user.
#
#   scripts/install.sh               build (or, from a release, use bin/) and install everything
#   scripts/install.sh --dry-run     print what would be done
#   scripts/install.sh --with-udev   also install the keyboard access rule (sudo)
#   scripts/install.sh --editor-only  only the programs and the editor plugins: no
#                                     keyboard access, service, desktop extension
#                                     or shell hooks. Coaching in Vim, Neovim and
#                                     VS Code only; the plugins start the daemon.
#   scripts/install.sh --yes         don't ask before editing ~/.bashrc
#
# Installs:
#   ~/.local/bin/flowbuddyd, ~/.local/bin/flowbuddy
#   ~/.config/systemd/user/flowbuddyd.service (enabled and started)
#   ~/.config/systemd/user/flowbuddy-weekly.service (only runs with [ai] weekly)
#   ~/.config/systemd/user/flowbuddy-tips.{service,timer} (only acts with [ai] tips)
#   ~/.local/share/gnome-shell/extensions/flowbuddy@abdelrahmanwm.github.io/
#   ~/.local/share/flowbuddy/flowbuddy.{bash,zsh,fish}, sourced from ~/.bashrc,
#   ~/.zshrc and fish's conf.d (for the shells you have)
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
fish_conf=${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/flowbuddy.fish
udev_rule=/etc/udev/rules.d/70-flowbuddy.rules
marker_begin="# >>> flowbuddy >>>"
marker_end="# <<< flowbuddy <<<"

dry_run=0 with_udev=0 yes=0 editor_only=0
for arg in "$@"; do
    case $arg in
        --dry-run) dry_run=1 ;;
        --with-udev) with_udev=1 ;;
        --editor-only) editor_only=1 ;;
        --yes | -y) yes=1 ;;
        -h | --help) sed -n '2,24p' "$0"; exit 0 ;;
        *) echo "unknown option: $arg" >&2; exit 2 ;;
    esac
done

if ((editor_only && with_udev)); then
    echo "--editor-only reads no keyboard, so it doesn't go with --with-udev" >&2
    exit 2
fi

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
run() {
    if ((dry_run)); then
        printf '    + %s\n' "$*"
    else
        "$@"
    fi
}
# How to install a package here: install_hint DEBIAN_NAMES FEDORA_NAMES.
install_hint() {
    if command -v apt-get >/dev/null; then echo "sudo apt install $1"
    elif command -v dnf >/dev/null; then echo "sudo dnf install $2"
    else echo "install $1 (Fedora: $2) with your package manager"
    fi
}

ask() {
    ((yes)) && return 0
    local reply
    read -r -p "$1 [y/N] " reply
    [[ $reply == [yY]* ]]
}

# A release download comes with the binaries in bin/; from a clone, build.
if [[ -x $repo/bin/flowbuddyd && -x $repo/bin/flowbuddy ]]; then
    built=$repo/bin
    say "Using the prebuilt binaries in bin/"
else
    command -v cargo >/dev/null || { echo "cargo not found: install Rust from https://rustup.rs, or download a release, which needs no Rust" >&2; exit 1; }
    command -v cc >/dev/null || { echo "no C compiler, which Rust needs to build: $(install_hint build-essential gcc)" >&2; exit 1; }
    say "Building release binaries"
    # libxkbcommon reads keys in your layout (German, French, ...); without
    # it, keys are read as US.
    features=()
    if pkg-config --exists xkbcommon 2>/dev/null; then
        features=(--features flowbuddy-daemon/xkb)
    else
        echo "    note: libxkbcommon-dev isn't installed, so keys are read as US."
        echo "    For other layouts: $(install_hint 'pkg-config libxkbcommon-dev' 'pkgconf-pkg-config libxkbcommon-devel'), then run this again."
    fi
    run cargo build --release --manifest-path "$repo/Cargo.toml" -p flowbuddy-daemon -p flowbuddy-cli "${features[@]}"
    built=${CARGO_TARGET_DIR:-$repo/target}/release
fi

say "Installing binaries to $bin_dir"
run install -Dm755 "$built/flowbuddyd" "$bin_dir/flowbuddyd"
run install -Dm755 "$built/flowbuddy" "$bin_dir/flowbuddy"
case :$PATH: in
    *:"$bin_dir":*) ;;
    *)
        # Ubuntu's ~/.profile and Fedora's ~/.bashrc add it once it exists,
        # after the log-out-and-in below; otherwise say how.
        if grep -qs '\.local/bin' "$HOME/.profile" "$HOME/.bash_profile" "$HOME/.bashrc"; then
            echo "    note: $bin_dir is on your PATH after you log out and back in (until then: $bin_dir/flowbuddy)"
        else
            echo "    note: $bin_dir is not on your PATH; add it in ~/.profile: export PATH=\"\$HOME/.local/bin:\$PATH\""
        fi
        ;;
esac

# Source a hook from a shell's rc file, between markers, after asking.
add_hook() {
    local rc=$1 vi=$2 hook=$3 name=${1/#$HOME/\~}
    if grep -qF "$marker_begin" "$rc" 2>/dev/null; then
        echo "    $name already sources it"
    elif ask "Add Flow Buddy (and '$vi') to the end of $name?"; then
        local block
        block=$(printf '%s\n%s\n%s\n%s\n' "$marker_begin" "$vi" \
            "[[ -f \"$hook\" ]] && source \"$hook\"" "$marker_end")
        if ((dry_run)); then
            printf '    + append to %s:\n        %s\n' "$rc" "${block//$'\n'/$'\n'        }"
        else
            printf '\n%s\n' "$block" >>"$rc"
        fi
    else
        echo "    skipped; add this to $name yourself:"
        echo "        $vi"
        echo "        source \"$hook\""
    fi
}

# The full install: the shell hooks, the desktop extension, and further
# down the service and keyboard access. Editor-only skips them all.
if ((!editor_only)); then
say "Installing the shell hooks"
for f in flowbuddy.bash flowbuddy.zsh flowbuddy.fish; do
    run install -Dm644 "$repo/shell/$f" "$share_dir/$f"
done
add_hook "$bashrc" "set -o vi" "$share_dir/flowbuddy.bash"
if command -v zsh >/dev/null; then
    add_hook "${ZDOTDIR:-$HOME}/.zshrc" "bindkey -v" "$share_dir/flowbuddy.zsh"
fi
if command -v fish >/dev/null; then
    # fish loads conf.d by itself. Vi bindings go in the same file, after
    # asking, as for bash and zsh, unless fish already has them.
    fish_dir=${XDG_CONFIG_HOME:-$HOME/.config}/fish
    fish_vi=""
    if grep -qs fish_vi_key_bindings "$fish_dir/config.fish" "$fish_dir/fish_variables"; then
        :
    elif grep -qs fish_vi_key_bindings "$fish_conf" || ask "Turn on vi key bindings in fish? Flow Buddy coaches the shell in vi mode"; then
        fish_vi=$'\nset -g fish_key_bindings fish_vi_key_bindings'
    else
        echo "    fish is coached in vi mode: add fish_vi_key_bindings to config.fish to turn it on"
    fi
    run install -Dm644 /dev/stdin "$fish_conf" <<<"source \"$share_dir/flowbuddy.fish\"$fish_vi"
fi

say "Installing the GNOME Shell extension"
run mkdir -p "$ext_dir"
for f in "$repo"/gnome-extension/*.js "$repo"/gnome-extension/*.json "$repo"/gnome-extension/*.css; do
    [[ -e $f ]] || continue
    run install -m644 "$f" "$ext_dir/${f##*/}"
done
# Its settings: the shortcuts for the current tip.
run install -Dm644 "$repo"/gnome-extension/schemas/*.gschema.xml -t "$ext_dir/schemas"
if command -v glib-compile-schemas >/dev/null; then
    run glib-compile-schemas "$ext_dir/schemas"
else
    echo "    glib-compile-schemas is missing: the tip shortcuts won't work"
fi
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

fi # full install

if command -v code >/dev/null || [[ -d $HOME/.vscode ]]; then
    say "Installing the VS Code companion"
    # Same folder for the snap and the .deb. Older versions are replaced.
    for old in "$vscode_dir"/flowbuddy.flowbuddy-vscode-*; do
        if [[ -e $old && $old != "$vscode_ext" ]]; then
            run rm -rf "$old"
        fi
    done
    run mkdir -p "$vscode_ext"
    for f in "$repo"/vscode-extension/*.js "$repo"/vscode-extension/*.json "$repo"/vscode-extension/README.md "$repo"/vscode-extension/icon.png; do
        run install -m644 "$f" "$vscode_ext/${f##*/}"
    done
    run mkdir -p "$vscode_ext/walkthrough"
    for f in "$repo"/vscode-extension/walkthrough/*.md; do
        run install -m644 "$f" "$vscode_ext/walkthrough/${f##*/}"
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
        for f in plugin/flowbuddy.vim autoload/flowbuddy.vim autoload/flowbuddy/practice.vim README.md; do
            run install -Dm644 "$repo/vim-plugin/$f" "$pack/$f"
        done
    done
fi

if ((!editor_only)); then
say "Installing the systemd user service"
run install -Dm644 "$repo/packaging/systemd/flowbuddyd.service" "$unit_dir/flowbuddyd.service"
run install -Dm644 "$repo/packaging/systemd/flowbuddy-weekly.service" "$unit_dir/flowbuddy-weekly.service"
run install -Dm644 "$repo/packaging/systemd/flowbuddy-tips.service" "$unit_dir/flowbuddy-tips.service"
run install -Dm644 "$repo/packaging/systemd/flowbuddy-tips.timer" "$unit_dir/flowbuddy-tips.timer"
run systemctl --user daemon-reload
run systemctl --user enable flowbuddyd.service
# Restart, not just start: after an update the old daemon would keep running.
run systemctl --user restart flowbuddyd.service
# Does nothing until you set [ai] tips = true.
run systemctl --user enable --now flowbuddy-tips.timer

if ((with_udev)); then
    say "Installing the keyboard access rule (needs sudo)"
    run sudo install -m644 "$repo/packaging/udev/70-flowbuddy.rules" "$udev_rule"
    run sudo udevadm control --reload
    run sudo udevadm trigger --subsystem-match=input --action=change
    run systemctl --user restart flowbuddyd.service
fi
fi # full install

# Whether this user can read a keyboard already (the udev rule, or the
# input group from before).
can_read_keys=0
for dev in /dev/input/event*; do
    [[ -r $dev ]] && can_read_keys=1 && break
done

say "Done"
if ((editor_only)); then
    cat <<MSG
    Editor only: Flow Buddy coaches in Vim, Neovim and VS Code (with the
    Vim extension), and reads no keyboard.
      - Open one of them: its plugin starts the daemon, which stops by
        itself ten minutes after the last editor closes.
      - Check with: flowbuddy doctor
      - New to Flow Buddy? Take the tour, a few minutes: flowbuddy tour
      - For the shell, Claude Code and tips on the desktop too, run
        scripts/install.sh --with-udev later: everything learned is kept.
MSG
    if [[ -f $unit_dir/flowbuddyd.service ]]; then
        echo "    note: the full install's service is still here, and still runs the daemon."
    fi
    exit 0
fi
cat <<MSG
    Next:
      - GNOME Shell loads new and updated extensions at login: log out and
        back in.
      - Restart VS Code and Vim if they're open, so they load the update.
      - Check everything with: flowbuddy doctor
      - New to Flow Buddy? Take the tour, a few minutes: flowbuddy tour
MSG
if ((!with_udev && !can_read_keys)); then
    cat <<MSG
      - Flow Buddy can't read your keyboard yet. Either re-run with
        --with-udev (recommended: active session only), or see
        docs/privacy.md#keyboard-access for the alternatives. Until then
        it coaches from the editor plugins alone.
MSG
fi
