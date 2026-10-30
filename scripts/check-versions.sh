#!/usr/bin/env bash
# Every piece says the same version: scripts/check-versions.sh [expected]
# (CI passes the tag's version on a release).
set -euo pipefail
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cargo=$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo/Cargo.toml" | head -1)
gnome=$(sed -n 's/.*"version-name": *"\([^"]*\)".*/\1/p' "$repo/gnome-extension/metadata.json")
vscode=$(sed -n 's/^  "version": *"\([^"]*\)".*/\1/p' "$repo/vscode-extension/package.json")
vim=$(sed -n "s/^let s:VERSION = '\(.*\)'/\1/p" "$repo/vim-plugin/autoload/flowbuddy.vim")
want=${1:-$cargo}
bad=0
for pair in "Cargo.toml:$cargo" "gnome-extension/metadata.json:$gnome" \
    "vscode-extension/package.json:$vscode" "vim-plugin/autoload/flowbuddy.vim:$vim"; do
    if [[ ${pair#*:} != "$want" ]]; then
        echo "${pair%%:*} says ${pair#*:}, not $want" >&2
        bad=1
    fi
done
((bad)) || echo "all at $want"
exit "$bad"
