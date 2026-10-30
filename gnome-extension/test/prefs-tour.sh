#!/usr/bin/env bash
# The settings window's tour in a real GTK window, drawn off screen by
# gtk4-broadwayd, against a sandboxed flowbuddy (no daemon, empty config):
#   gnome-extension/test/prefs-tour.sh [path/to/flowbuddy]
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cli=${1:-$here/../target/debug/flowbuddy}
command -v gtk4-broadwayd >/dev/null || { echo "skipped: no gtk4-broadwayd"; exit 0; }
[[ -x $cli ]] || { echo "no flowbuddy at $cli (cargo build -p flowbuddy-cli)" >&2; exit 1; }
cli=$(cd "$(dirname "$cli")" && pwd)/$(basename "$cli")
tmp=$(mktemp -d)
trap 'kill "$broadway" 2>/dev/null; rm -rf "$tmp"' EXIT
mkdir -p "$tmp/ext/schemas" "$tmp/bin" "$tmp/run"
cp "$here"/*.js "$here/test/prefs-tour.js" "$tmp/ext/"
cp "$here"/schemas/*.xml "$tmp/ext/schemas/"
glib-compile-schemas "$tmp/ext/schemas"
# GNOME Shell's ExtensionPreferences, which only exists inside the shell.
sed -i 's#^import {ExtensionPreferences} from .*#class ExtensionPreferences { getSettings() { return globalThis.fbSettings; } }#' "$tmp/ext/prefs.js"
ln -s "$cli" "$tmp/bin/flowbuddy"
display=:$((RANDOM % 50 + 50))
gtk4-broadwayd "$display" >/dev/null 2>&1 &
broadway=$!
sleep 1
cd "$tmp/ext"
env PATH="$tmp/bin:$PATH" HOME="$tmp" XDG_CONFIG_HOME="$tmp/c" XDG_DATA_HOME="$tmp/d" \
    XDG_STATE_HOME="$tmp/s" FLOWBUDDY_SOCKET="$tmp/run/none" \
    GDK_BACKEND=broadway BROADWAY_DISPLAY="$display" \
    timeout 60 gjs -m prefs-tour.js
