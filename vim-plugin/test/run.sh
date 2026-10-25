#!/usr/bin/env bash
# Run the Vim plugin's tests headless: test/run.sh
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
out=$(mktemp)
trap 'rm -f "$out"' EXIT
OUT=$out vim -N -u NONE -i NONE --not-a-term -S "$here/facts.vim" >/dev/null 2>&1 || true
cat "$out"
grep -q '^PASSED$' "$out"
