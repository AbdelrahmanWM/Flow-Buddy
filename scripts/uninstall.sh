#!/usr/bin/env bash
# Remove Flow Buddy for the current user.
#
#   scripts/uninstall.sh            remove programs, keep your data
#   scripts/uninstall.sh --purge    also delete stats, memory, config and state
#   scripts/uninstall.sh --dry-run  print what would be done

set -euo pipefail

uuid=flowbuddy@abdelrahmanwm.github.io
bin_dir=$HOME/.local/bin
unit=${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/flowbuddyd.service
ext_dir=${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/$uuid
share_dir=${XDG_DATA_HOME:-$HOME/.local/share}/flowbuddy
vscode_dir=$HOME/.vscode/extensions
config_dir=${XDG_CONFIG_HOME:-$HOME/.config}/flowbuddy
state_dir=${XDG_STATE_HOME:-$HOME/.local/state}/flowbuddy
bashrc=$HOME/.bashrc
udev_rule=/etc/udev/rules.d/70-flowbuddy.rules

dry_run=0 purge=0
for arg in "$@"; do
    case $arg in
        --dry-run) dry_run=1 ;;
        --purge) purge=1 ;;
        -h | --help) sed -n '2,7p' "$0"; exit 0 ;;
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

say "Stopping the service"
run systemctl --user disable --now flowbuddyd.service 2>/dev/null || true
run rm -f "${unit%/*}/flowbuddy-weekly.service"
run rm -f "$unit"
run systemctl --user daemon-reload

say "Removing programs and the extension"
run gnome-extensions disable "$uuid" 2>/dev/null || true
if command -v gsettings >/dev/null; then
    enabled=$(gsettings get org.gnome.shell enabled-extensions)
    if [[ $enabled == *"'$uuid'"* ]]; then
        list=${enabled//", '$uuid'"/}
        list=${list//"'$uuid', "/}
        list=${list//"'$uuid'"/}
        run gsettings set org.gnome.shell enabled-extensions "$list"
    fi
fi
run rm -rf "$ext_dir"
run rm -f "$bin_dir/flowbuddyd" "$bin_dir/flowbuddy"
run rm -f "$share_dir/flowbuddy.bash"

vscode_exts=("$vscode_dir"/flowbuddy.flowbuddy-vscode-*)
if [[ -e ${vscode_exts[0]} ]]; then
    say "Removing the VS Code companion"
    run rm -rf "${vscode_exts[@]}"
    if ((dry_run)); then
        printf '    + unregister flowbuddy.flowbuddy-vscode in %s\n' "$vscode_dir/extensions.json"
    elif [[ -f $vscode_dir/extensions.json ]]; then
        python3 - "$vscode_dir/extensions.json" <<'PY'
import json, os, sys
path = sys.argv[1]
with open(path) as f:
    entries = json.load(f)
entries = [e for e in entries if e.get("identifier", {}).get("id") != "flowbuddy.flowbuddy-vscode"]
with open(path + ".flowbuddy", "w") as f:
    json.dump(entries, f)
os.replace(path + ".flowbuddy", path)
PY
    fi
fi

for pack in "$HOME/.vim/pack/flowbuddy" "${XDG_DATA_HOME:-$HOME/.local/share}/nvim/site/pack/flowbuddy"; do
    if [[ -d $pack ]]; then
        say "Removing the Vim plugin from ${pack%/pack/flowbuddy}"
        run rm -rf "$pack"
    fi
done

say "Removing the ~/.bashrc block"
if grep -qF "# >>> flowbuddy >>>" "$bashrc" 2>/dev/null; then
    run sed -i '/# >>> flowbuddy >>>/,/# <<< flowbuddy <<</d' "$bashrc"
fi

if [[ -f $udev_rule ]]; then
    say "Removing the keyboard access rule (needs sudo)"
    run sudo rm -f "$udev_rule"
    run sudo udevadm control --reload
fi

if ((purge)); then
    say "Deleting all Flow Buddy data"
    run rm -rf "$share_dir" "$config_dir" "$state_dir"
else
    echo "    kept your data in $share_dir, $config_dir and $state_dir (use --purge to delete)"
fi
say "Done"
