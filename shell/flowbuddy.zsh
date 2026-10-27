# Flow Buddy zsh integration.
#
# Add to ~/.zshrc:
#
#     bindkey -v                         # Flow Buddy coaches zsh vi-mode
#     source /path/to/flowbuddy.zsh
#
# Same reports as flowbuddy.bash: the *name* of each command when it starts,
# never its arguments, plus its shape from a fixed list worked out here
# (the command line never leaves the shell); and when the prompt is back,
# with whether vi-mode is on. Reports run in the background and are skipped
# when the daemon isn't running.

[[ -o interactive ]] || return 0
(( $+commands[flowbuddy] )) || return 0

typeset -g __flowbuddy_sock="${FLOWBUDDY_SOCKET:-${XDG_RUNTIME_DIR:-/tmp}/flowbuddy.sock}"
typeset -g __flowbuddy_tty=""
__flowbuddy_tty=$(tty 2>/dev/null) || __flowbuddy_tty=""
typeset -g __flowbuddy_prev=""

__flowbuddy_send() {
    [[ -S $__flowbuddy_sock ]] || return 0
    local -a extra
    extra=(--shell-pid $$)
    [[ -n $__flowbuddy_tty ]] && extra+=(--tty $__flowbuddy_tty)
    ( flowbuddy ctx "$@" $extra >/dev/null 2>&1 & )
}

# The shape of command line $1 after $2. Kept in step with flowbuddy.bash.
__flowbuddy_shape() {
    local -a now prev
    now=(${(z)1})
    prev=(${(z)2})
    local n=${#now} p=${#prev}
    if (( n == 2 )) && [[ $now[1] == cd && $now[2] == .. ]]; then
        print cd-up
    elif (( n == 2 )) && [[ $now[1] == cd && $now[2] =~ '^\.\.(/\.\.)+/?$' ]]; then
        print cd-up-many
    elif (( n > 1 && p > 1 )) && [[ $now[1] != $prev[1] && $now[n] == $prev[p] ]] &&
        (( ${#now[n]} >= 12 )); then
        print reused-arg
    elif (( n == p && n >= 3 && ${#1} >= 20 )); then
        local i diff=0
        for (( i = 1; i <= n; i++ )); do
            [[ $now[i] == $prev[i] ]] || (( diff += 1 ))
        done
        (( diff == 1 )) && print one-word-changed
    fi
    return 0
}

__flowbuddy_preexec() {
    local -a words
    words=(${(z)1})
    local word cmd=""
    for word in $words; do
        case $word in
            *=*) continue ;;
            sudo|command|exec|nohup|time|env|nice|noglob|builtin) continue ;;
            -*) continue ;;
        esac
        cmd=${word:t}
        break
    done
    [[ -n $cmd ]] || return 0
    local shape
    shape=$(__flowbuddy_shape "$1" "$__flowbuddy_prev")
    __flowbuddy_prev=$1
    if [[ -n $shape ]]; then
        __flowbuddy_send fg "$cmd" --shape "$shape"
    else
        __flowbuddy_send fg "$cmd"
    fi
}

__flowbuddy_precmd() {
    local vi=false
    [[ $(bindkey -lL main) == *viins* ]] && vi=true
    __flowbuddy_send prompt --vi-mode $vi
}

# zsh says when the vi-mode switches: report it, so Flow Buddy needn't
# guess the mode from keys.
__flowbuddy_keymap() {
    case $KEYMAP in
        vicmd) __flowbuddy_send mode normal ;;
        viins | main) __flowbuddy_send mode insert ;;
    esac
}

autoload -Uz add-zsh-hook
add-zsh-hook preexec __flowbuddy_preexec
add-zsh-hook precmd __flowbuddy_precmd
autoload -Uz add-zle-hook-widget 2>/dev/null &&
    add-zle-hook-widget keymap-select __flowbuddy_keymap
