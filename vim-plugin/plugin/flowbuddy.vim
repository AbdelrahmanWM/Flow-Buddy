" Flow Buddy: tell the local flowbuddyd which mode Vim is in and how the
" cursor moved, so it can coach better motions.
"
" Sends numbers and flags only: never text, file names or paths. Does
" nothing if the daemon isn't running. Turn it off with
"   let g:flowbuddy_enabled = 0
" Requires Vim 9 with +channel, or Neovim 0.9.

if exists('g:loaded_flowbuddy') || !get(g:, 'flowbuddy_enabled', 1)
  finish
endif
if !has('nvim') && (v:version < 900 || !has('channel') || !has('timers'))
  finish
endif
if has('nvim') && !has('nvim-0.9')
  finish
endif
let g:loaded_flowbuddy = 1

command! FlowBuddyGotIt call flowbuddy#respond('dismiss')
command! FlowBuddyNotHelpful call flowbuddy#respond('false_positive')

augroup flowbuddy
  autocmd!
  autocmd VimEnter * call flowbuddy#start()
  autocmd ModeChanged * call flowbuddy#on_mode(v:event.new_mode)
  autocmd CursorMoved * call flowbuddy#on_cursor()
  autocmd TextYankPost * call flowbuddy#on_operator(v:event)
  autocmd FocusGained * call flowbuddy#on_focus(1)
  autocmd FocusLost * call flowbuddy#on_focus(0)
  autocmd VimSuspend * call flowbuddy#on_focus(0)
  autocmd VimResume * call flowbuddy#on_focus(1)
  autocmd VimLeavePre * call flowbuddy#stop()
  autocmd CmdlineLeave : call flowbuddy#on_cmdline_leave()
augroup END

" Started from a session or late: VimEnter has already happened.
if v:vim_did_enter
  call flowbuddy#start()
endif
