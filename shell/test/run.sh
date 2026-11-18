#!/usr/bin/env bash
# Test the shell hooks: shell/test/run.sh [bash|zsh|fish ...]
#
# Runs each shell interactively on a pseudo-terminal with a fake
# `flowbuddy` that logs what it would send, types a few commands, and
# checks the log. Shells that aren't installed are skipped.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
hooks=$(dirname "$here")
shells=("$@")
((${#shells[@]})) || shells=(bash zsh fish)
failed=0

for sh in "${shells[@]}"; do
    if ! command -v "$sh" >/dev/null; then
        echo "skip $sh (not installed)"
        continue
    fi
    tmp=$(mktemp -d)
    mkdir "$tmp/bin"
    cat >"$tmp/bin/flowbuddy" <<FAKE
#!/bin/sh
echo "\$*" >>"$tmp/log"
FAKE
    chmod +x "$tmp/bin/flowbuddy"
    # The hooks only report when the daemon's socket exists.
    python3 -c "import socket,sys; socket.socket(socket.AF_UNIX).bind(sys.argv[1])" "$tmp/sock"
    secret=/tmp/flowbuddy-secret-argument-that-must-not-leak
    case $sh in
        bash)
            rc="set -o vi; source '$hooks/flowbuddy.bash'"
            start=(bash --norc --noprofile -i)
            ;;
        zsh)
            rc="bindkey -v; source '$hooks/flowbuddy.zsh'"
            start=(zsh -f -i)
            ;;
        fish)
            rc="fish_vi_key_bindings; source '$hooks/flowbuddy.fish'"
            start=(fish --no-config -i)
            ;;
    esac
    # Esc then A, to switch to Normal mode and back. Fish 4 reads an Esc
    # with a key right behind it as Alt plus that key, and piped input
    # comes all at once; so for fish, set the mode the way its key
    # bindings do, which is what the hook watches.
    switch_modes='echo mode'$'\033''A'
    [[ $sh == fish ]] && switch_modes='set -g fish_bind_mode default; set -g fish_bind_mode insert'
    input="$rc
true
cd ..
cd ..
cd ../..
cd -
ls $secret
cat $secret
echo one two three four
echo one two three five
$switch_modes
exit
"
    # Each line is typed after the previous prompt, give or take; the
    # reports run in the background, so wait a little at the end.
    printf '%s' "$input" | PATH="$tmp/bin:$PATH" FLOWBUDDY_SOCKET="$tmp/sock" \
        script -qec "${start[*]}" /dev/null >/dev/null 2>&1 || true
    sleep 1
    log=$(cat "$tmp/log" 2>/dev/null || true)
    check() {
        if grep -qE -- "$1" <<<"$log"; then
            echo "ok   $sh: $2"
        else
            echo "FAIL $sh: $2"
            failed=1
        fi
    }
    check '^ctx prompt --vi-mode true' "prompt reports vi-mode"
    check '^ctx fg true( |$)' "command names"
    check '^ctx fg cd --shape cd-up( |$)' "cd .. shape"
    check '^ctx fg cd --shape cd-up-many' "cd ../.. shape"
    check '^ctx fg cd --shape cd-back' "cd - shape"
    check '^ctx fg cat --shape reused-arg' "reused argument shape"
    check '^ctx fg echo --shape one-word-changed' "one word changed shape"
    # bash can't say when its vi-mode switches; zsh and fish do.
    if [[ $sh != bash ]]; then
        check '^ctx mode normal' "vi-mode switch to normal"
        check '^ctx mode insert' "vi-mode switch back to insert"
    fi
    if grep -q "secret" <<<"$log"; then
        echo "FAIL $sh: an argument leaked"
        failed=1
    else
        echo "ok   $sh: no arguments sent"
    fi
    if ((failed)); then
        echo "---- $sh log"
        echo "$log"
    fi
    rm -rf "$tmp"
done
exit "$failed"
