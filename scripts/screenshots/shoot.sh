#!/usr/bin/env bash
# The README's screenshots, made in a throwaway headless GNOME session:
#   scripts/screenshots/shoot.sh [path/to/flowbuddy]
#
# GNOME Shell runs with a virtual screen and its own D-Bus, config and
# runtime folders, with the Flow Buddy extension, the Vim plugin, and a
# stand-in daemon (standin-daemon.py) that sends tips on cue. Nothing
# reads your keyboard, and your own session isn't touched. A helper
# extension (shot@flowbuddy.test) follows `script`: keys, screenshots.
# The pictures land in docs/images/.
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../.." && pwd)
cli=${1:-$repo/target/debug/flowbuddy}
[[ -x $cli ]] || { echo "no flowbuddy at $cli (cargo build -p flowbuddy-cli)" >&2; exit 1; }
cli=$(cd "$(dirname "$cli")" && pwd)/$(basename "$cli")
for tool in gnome-shell gjs vim dbus-run-session python3; do
    command -v "$tool" >/dev/null || { echo "needs $tool" >&2; exit 1; }
done
python3 -c 'import PIL' 2>/dev/null || { echo "needs python3-pil" >&2; exit 1; }

tmp=$(mktemp -d /tmp/flowbuddy-shots.XXXX)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/run" "$tmp/config" "$tmp/data/gnome-shell/extensions" "$tmp/shots" "$tmp/bin" "$tmp/.vim/pack/flowbuddy/start"
chmod 700 "$tmp/run"
uuid=flowbuddy@abdelrahmanwm.github.io
ext=$tmp/data/gnome-shell/extensions/$uuid
mkdir -p "$ext/schemas"
cp "$repo"/gnome-extension/*.js "$repo"/gnome-extension/metadata.json "$repo"/gnome-extension/stylesheet.css "$ext/"
cp "$repo"/gnome-extension/schemas/*.xml "$ext/schemas/"
glib-compile-schemas "$ext/schemas"
cp -r "$here/shot@flowbuddy.test" "$tmp/data/gnome-shell/extensions/"
cp -r "$repo/vim-plugin" "$tmp/.vim/pack/flowbuddy/start/flowbuddy"
cp "${SHOT_SCRIPT:-$here/script}" "$tmp/shots/script"
cp "$here/demo.txt" "$tmp/demo.txt"
ln -s "$cli" "$tmp/bin/flowbuddy"
printf 'set nocompatible\nsyntax on\nset number\nset laststatus=2\n' > "$tmp/.vimrc"

cat > "$tmp/session.sh" <<'SESSION'
set -u
gsettings set org.gnome.shell enabled-extensions "['flowbuddy@abdelrahmanwm.github.io', 'shot@flowbuddy.test']"
gsettings set org.gnome.shell disable-user-extensions false
gsettings set org.gnome.shell welcome-dialog-last-shown-version '999'
gsettings set org.gnome.desktop.interface color-scheme 'prefer-dark'
gsettings --schemadir "$XDG_DATA_HOME/gnome-shell/extensions/flowbuddy@abdelrahmanwm.github.io/schemas" \
    set org.gnome.shell.extensions.flowbuddy welcomed true
gsettings --schemadir "$XDG_DATA_HOME/gnome-shell/extensions/flowbuddy@abdelrahmanwm.github.io/schemas" \
    set org.gnome.shell.extensions.flowbuddy open-tour true
python3 "$SHOT_HERE/standin-daemon.py" &
daemon=$!
gnome-shell --headless --virtual-monitor 1280x800 --wayland --no-x11 --wayland-display flowbuddy-shots \
    > "$SHOT_DIR/shell.log" 2>&1 &
shell=$!
for _ in $(seq 1 120); do [ -e "$SHOT_DIR/done" ] && break; sleep 1; done
kill "$shell" "$daemon" 2>/dev/null
wait "$shell" 2>/dev/null
SESSION

cd "$tmp"
env -i HOME="$tmp" PATH="$tmp/bin:/usr/local/bin:/usr/bin:/bin" LANG=C.UTF-8 TERM=xterm-256color \
    XDG_RUNTIME_DIR="$tmp/run" XDG_CONFIG_HOME="$tmp/config" XDG_DATA_HOME="$tmp/data" \
    XDG_CACHE_HOME="$tmp/cache" XDG_STATE_HOME="$tmp/state" \
    SHOT_DIR="$tmp/shots" SHOT_HERE="$here" \
    timeout 180 dbus-run-session -- bash "$tmp/session.sh" >/dev/null 2>&1 || true
[[ -e $tmp/shots/done ]] || { echo "the session didn't finish; its log:" >&2; tail -20 "$tmp/shots/log" "$tmp/shots/shell.log" >&2; exit 1; }
grep '^error' "$tmp/shots/log" >&2 || true
python3 "$here/crop.py" "$tmp/shots" "${SHOT_OUT:-$repo/docs/images}"
