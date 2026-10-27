# Flow Buddy fish integration.
#
# Add to ~/.config/fish/config.fish:
#
#     fish_vi_key_bindings               # Flow Buddy coaches fish vi-mode
#     source /path/to/flowbuddy.fish
#
# Same reports as flowbuddy.bash: the *name* of each command when it starts,
# never its arguments, plus its shape from a fixed list worked out here
# (the command line never leaves the shell); and when the prompt is back,
# with whether vi-mode is on. Reports run in the background and are skipped
# when the daemon isn't running.

status is-interactive; or exit 0
command -q flowbuddy; or exit 0

if set -q FLOWBUDDY_SOCKET
    set -g __flowbuddy_sock $FLOWBUDDY_SOCKET
else if set -q XDG_RUNTIME_DIR
    set -g __flowbuddy_sock $XDG_RUNTIME_DIR/flowbuddy.sock
else
    set -g __flowbuddy_sock /tmp/flowbuddy.sock
end
set -g __flowbuddy_tty ""
if tty -s
    set -g __flowbuddy_tty (tty)
end
set -g __flowbuddy_prev ""

function __flowbuddy_send
    test -S $__flowbuddy_sock; or return 0
    set -l extra --shell-pid $fish_pid
    test -n "$__flowbuddy_tty"; and set -a extra --tty $__flowbuddy_tty
    flowbuddy ctx $argv $extra >/dev/null 2>&1 &
    disown 2>/dev/null
end

# The shape of command line $argv[1] after $argv[2]. Kept in step with
# flowbuddy.bash.
function __flowbuddy_shape
    set -l now (string split -n ' ' -- $argv[1])
    set -l prev (string split -n ' ' -- $argv[2])
    set -l n (count $now)
    set -l p (count $prev)
    if test $n -eq 2; and test "$now[1]" = cd; and test "$now[2]" = ..
        echo cd-up
    else if test $n -eq 2; and test "$now[1]" = cd; and string match -qr '^\.\.(/\.\.)+/?$' -- $now[2]
        echo cd-up-many
    else if test $n -eq 2; and test "$now[1]" = cd; and test "$now[2]" = -
        echo cd-back
    else if test $n -gt 1 -a $p -gt 1; and test "$now[1]" != "$prev[1]"; and test "$now[-1]" = "$prev[-1]"; and test (string length -- $now[-1]) -ge 12
        echo reused-arg
    else if test $n -eq $p -a $n -ge 3; and test (string length -- $argv[1]) -ge 20
        set -l diff 0
        for i in (seq $n)
            test "$now[$i]" = "$prev[$i]"; or set diff (math $diff + 1)
        end
        test $diff -eq 1; and echo one-word-changed
    end
    return 0
end

function __flowbuddy_preexec --on-event fish_preexec
    set -l cmd
    for word in (string split -n ' ' -- $argv[1])
        switch $word
            case '*=*' sudo command exec nohup time env nice builtin '-*'
                continue
        end
        set cmd (string split -r -m1 / -- $word)[-1]
        break
    end
    test -n "$cmd"; or return 0
    set -l shape (__flowbuddy_shape $argv[1] "$__flowbuddy_prev")
    set -g __flowbuddy_prev $argv[1]
    if test -n "$shape"
        __flowbuddy_send fg $cmd --shape $shape
    else
        __flowbuddy_send fg $cmd
    end
end

# fish says when the vi-mode switches: report it, so Flow Buddy needn't
# guess the mode from keys.
function __flowbuddy_mode --on-variable fish_bind_mode
    switch $fish_bind_mode
        case default
            __flowbuddy_send mode normal
        case insert replace replace_one
            __flowbuddy_send mode insert
        case visual
            __flowbuddy_send mode visual
    end
end

function __flowbuddy_prompt --on-event fish_prompt
    set -l vi false
    test "$fish_key_bindings" = fish_vi_key_bindings; and set vi true
    __flowbuddy_send prompt --vi-mode $vi
end
