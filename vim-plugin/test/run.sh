#!/usr/bin/env bash
# Run the plugin's tests headless: test/run.sh [vim|nvim]
set -euo pipefail
editor=${1:-vim}
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
out=$(mktemp)
trap 'rm -f "$out"' EXIT
case $editor in
    vim) OUT=$out vim -N -u NONE -i NONE --not-a-term -S "$here/facts.vim" >/dev/null 2>&1 || true ;;
    nvim) OUT=$out nvim --headless -u NONE -i NONE -S "$here/facts.vim" >/dev/null 2>&1 || true ;;
    *) echo "usage: test/run.sh [vim|nvim]" >&2; exit 2 ;;
esac
cat "$out"
grep -q '^PASSED$' "$out"
