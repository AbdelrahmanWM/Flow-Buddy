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
#     arguments;
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

__flowbuddy_send() {
    [[ -S $__flowbuddy_sock ]] || return 0
    (flowbuddy ctx "$@" --shell-pid "$$" ${__flowbuddy_tty:+--tty "$__flowbuddy_tty"} >/dev/null 2>&1 &)
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
    [[ -n $cmd ]] && __flowbuddy_send fg "$cmd"
}

__flowbuddy_precmd() {
    __flowbuddy_at_prompt=1
    local vi=false
    [[ :$SHELLOPTS: == *:vi:* ]] && vi=true
    __flowbuddy_send prompt --vi-mode "$vi"
}

trap '__flowbuddy_preexec' DEBUG
PROMPT_COMMAND="${PROMPT_COMMAND:+$PROMPT_COMMAND; }__flowbuddy_precmd"
