# shellcheck shell=bash
# Flow Buddy bash integration.
#
# Add to ~/.bashrc (after any other PROMPT_COMMAND setup):
#
#     set -o vi                          # Flow Buddy coaches bash vi-mode
#     source /path/to/flowbuddy.bash
#
# Tells the Flow Buddy daemon two things, so it can tell bash vi-mode apart
# from vim and other programs running in the same terminal:
#   - the *name* of each command when it starts (e.g. "vim"), never its
#     arguments, and its *shape* from a fixed list (exactly `cd ..`; reusing
#     the previous command's long last argument; the previous command with
#     one word changed), worked out here: the command line never leaves;
#   - when the prompt is back, and whether vi-mode is on.
#
# Reports run in the background and are skipped when the daemon isn't
# running, so the shell never waits on Flow Buddy.
#
# Note: this installs a DEBUG trap. If you already use one (for example
# bash-preexec), call __flowbuddy_preexec from it instead.

[[ $- == *i* ]] || return 0
command -v flowbuddy >/dev/null 2>&1 || return 0

__flowbuddy_sock="${FLOWBUDDY_SOCKET:-${XDG_RUNTIME_DIR:-/tmp}/flowbuddy.sock}"
__flowbuddy_tty="$(tty 2>/dev/null)" || __flowbuddy_tty=""
__flowbuddy_at_prompt=0
__flowbuddy_prev=""

__flowbuddy_send() {
    [[ -S $__flowbuddy_sock ]] || return 0
    (flowbuddy ctx "$@" --shell-pid "$$" ${__flowbuddy_tty:+--tty "$__flowbuddy_tty"} >/dev/null 2>&1 &)
}

# The shape of command line $1 after $2: cd-up, reused-arg, one-word-changed
# or nothing. Kept in step with flowbuddy.zsh and flowbuddy.fish.
__flowbuddy_shape() {
    local -a now prev
    read -r -a now <<<"$1"
    read -r -a prev <<<"$2"
    local n=${#now[@]} p=${#prev[@]}
    if [[ $n == 2 && ${now[0]} == cd && ${now[1]} == .. ]]; then
        echo cd-up
    elif [[ $n == 2 && ${now[0]} == cd && ${now[1]} =~ ^\.\.(/\.\.)+/?$ ]]; then
        echo cd-up-many
    elif ((n > 1 && p > 1)) && [[ ${now[0]} != "${prev[0]}" && ${now[n - 1]} == "${prev[p - 1]}" ]] &&
        ((${#now[n - 1]} >= 12)); then
        echo reused-arg
    elif ((n == p && n >= 3 && ${#1} >= 20)); then
        local i diff=0
        for ((i = 0; i < n; i++)); do
            [[ ${now[i]} == "${prev[i]}" ]] || ((diff++))
        done
        ((diff == 1)) && echo one-word-changed
    fi
}

__flowbuddy_preexec() {
    # DEBUG runs before every simple command; only report the first one
    # after a prompt, and never our own functions.
    [[ $__flowbuddy_at_prompt == 1 ]] || return 0
    [[ $BASH_COMMAND == __flowbuddy_* ]] && return 0
    __flowbuddy_at_prompt=0

    local -a words
    read -r -a words <<<"$BASH_COMMAND"
    local word cmd=""
    for word in "${words[@]}"; do
        case $word in
            *=*) continue ;;                                    # VAR=value prefix
            sudo | command | exec | nohup | time | env | nice) continue ;;
            -*) continue ;;                                     # flags of the above
        esac
        cmd=${word##*/}
        break
    done
    [[ -n $cmd ]] || return 0
    # The whole line, for its shape; history has it, BASH_COMMAND only the
    # first simple command.
    local line
    line=$(HISTTIMEFORMAT='' builtin history 1 2>/dev/null)
    line=${line#*[0-9]  }
    [[ -n $line ]] || line=$BASH_COMMAND
    local shape
    shape=$(__flowbuddy_shape "$line" "$__flowbuddy_prev")
    __flowbuddy_prev=$line
    __flowbuddy_send fg "$cmd" ${shape:+--shape "$shape"}
}

__flowbuddy_precmd() {
    __flowbuddy_at_prompt=1
    local vi=false
    [[ :$SHELLOPTS: == *:vi:* ]] && vi=true
    __flowbuddy_send prompt --vi-mode "$vi"
}

trap '__flowbuddy_preexec' DEBUG
PROMPT_COMMAND="${PROMPT_COMMAND:+$PROMPT_COMMAND; }__flowbuddy_precmd"
