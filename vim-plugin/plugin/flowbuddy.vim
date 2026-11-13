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
command! -nargs=? -complete=customlist,s:drills FlowBuddyPractice call flowbuddy#practice#start(<f-args>)
command! FlowBuddyMap call flowbuddy#practice#map()
command! FlowBuddyTour call flowbuddy#practice#tour()

function! s:drills(lead, ...) abort
  return filter(['vertical', 'horizontal', 'delete', 'inside'], 'v:val =~# "^" . a:lead')
endfunction

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
  " The keys typed in moving modes, when the daemon reads no keyboard
  " (see "Keys" in autoload/flowbuddy.vim for what is never sent).
  if exists('##KeyInputPre')
    autocmd KeyInputPre * call flowbuddy#on_key(v:char, mode(1), v:event.typed)
  elseif has('nvim')
    " Neovim: `typed` is what you pressed, before mappings; empty for
    " keys a mapping or a script produced.
    execute "lua vim.on_key(function(key, typed) if typed and typed ~= '' then pcall(vim.fn['flowbuddy#on_key'], typed, vim.api.nvim_get_mode().mode, true) end end, vim.api.nvim_create_namespace('flowbuddy'))"
  endif
augroup END

" Started from a session or late: VimEnter has already happened.
if v:vim_did_enter
  call flowbuddy#start()
endif
