" Flow Buddy practice: runs a drill written by `flowbuddy practice`, one
" round at a time, counting the keys you use, and writes how it went.
"
" Keys are counted with buffer-local mappings that pass each key straight
" through. A key that takes a character after it (f, t, r, m, ', `, and
" i or a after an operator) counts that character too.

let s:drill = {}
let s:round = 0
let s:keys = 0
let s:started = []
let s:results = []
let s:out = ''
let s:loading = 0
let s:done = 0
let s:quit = 1
" With :FlowBuddyPractice: the flowbuddy command and the drill's name, to
" record the result when it ends.
let s:record = []
let s:buf = -1
let s:counter = -1
let s:shown_keys = 0

" Keys that read one more character.
let s:TAKES_CHAR = ['f', 't', 'F', 'T', 'r', 'm', "'", '`', '"', 'q', '@']

" Every key you might press, by its mapping name.
function! s:counted_keys() abort
  let keys = map(range(char2nr('!'), char2nr('~')), 'nr2char(v:val)')
  call filter(keys, 'v:val !=# "|" && v:val !=# "\\" && v:val !=# "<"')
  let keys += ['<Bar>', '<Bslash>', '<lt>', '<Space>', '<CR>', '<BS>', '<Del>',
        \ '<C-d>', '<C-u>', '<C-f>', '<C-b>', '<C-e>', '<C-y>', '<C-o>', '<C-i>',
        \ '<C-r>', '<C-v>', '<Left>', '<Right>', '<Up>', '<Down>', '<Home>', '<End>',
        \ '<PageUp>', '<PageDown>']
  return keys
endfunction

" Called by every counting mapping: count it, and give the key back.
function! flowbuddy#practice#key(key) abort
  let s:keys += 1
  let m = mode(1)
  if index(s:TAKES_CHAR, a:key) >= 0 || (a:key =~# '^[ia]$' && m =~# '^\(no\|[vV\x16]\)')
    let s:keys += 1
  endif
  return a:key
endfunction

function! s:map_keys() abort
  for k in s:counted_keys()
    let rhs = k =~# '^<' ? '"\' . k . '"' : string(k)
    execute 'noremap <buffer> <silent> <expr> ' . k . ' flowbuddy#practice#key(' . rhs . ')'
  endfor
endfunction

" Run the drill in `drill_path`; results go to `out_path` when it ends,
" and then Vim quits (`flowbuddy practice` started it for this), unless
" the third argument is 0.
"
" The round's instructions are in a panel at the top of the screen; the
" goal is marked in the drill with a bright bar and a ▶ in the sign
" column, so it shows on any colour scheme, blank lines too.
function! flowbuddy#practice#run(drill_path, out_path, ...) abort
  let s:quit = get(a:000, 0, 1)
  let s:record = get(a:000, 1, [])
  let s:drill = json_decode(join(readfile(a:drill_path), "\n"))
  let s:out = a:out_path
  let s:results = []
  let s:round = 0
  let s:done = 0
  " Tips about the drill's keys would only get in the way: none show
  " while it runs (see flowbuddy#show_tip), and none are coached.
  let g:flowbuddy_practising = 1
  if exists('*flowbuddy#on_focus')
    call flowbuddy#on_focus(0)
  endif
  " Changes in the buffer you were in stay there.
  if &modified
    silent new
  else
    silent enew
  endif
  setlocal buftype=nofile bufhidden=wipe noswapfile nolist nowrap nospell nobuflisted
  setlocal number signcolumn=yes
  let &l:relativenumber = get(s:drill, 'relative_numbers', v:false) is v:true
  silent! file [Flow\ Buddy\ practice]
  let s:buf = bufnr()
  call s:colours()
  call s:map_keys()
  augroup flowbuddy_practice
    autocmd!
    autocmd CursorMoved,TextChanged <buffer> call s:check()
    autocmd BufWipeout <buffer> call s:abandoned()
    autocmd VimLeavePre * call s:finish(1)
    autocmd ColorScheme * call s:colours()
  augroup END
  call s:load()
endfunction

function! s:colours() abort
  " Fixed colours, not the scheme's: they must stand out on any of them.
  highlight FlowBuddyGoal ctermfg=16 ctermbg=220 cterm=bold guifg=#000000 guibg=#f6d32d gui=bold
  highlight FlowBuddyCut ctermfg=231 ctermbg=160 cterm=bold guifg=#ffffff guibg=#c01c28 gui=bold
  highlight FlowBuddyStart ctermfg=40 cterm=bold guifg=#26a269 gui=bold
  highlight FlowBuddyBar ctermfg=231 ctermbg=25 cterm=bold guifg=#ffffff guibg=#1c71d8 gui=bold
  highlight FlowBuddyLabel ctermfg=75 cterm=bold guifg=#62a0ea gui=bold
  highlight FlowBuddyExpect ctermfg=40 cterm=bold guifg=#26a269 gui=bold
  highlight FlowBuddyOver ctermfg=203 cterm=bold guifg=#f66151 gui=bold
  highlight FlowBuddyPerfect ctermfg=16 ctermbg=114 cterm=bold guifg=#000000 guibg=#8ff0a4 gui=bold
  highlight FlowBuddyClose ctermfg=16 ctermbg=220 cterm=bold guifg=#000000 guibg=#f6d32d gui=bold
  highlight FlowBuddyDotDone ctermfg=120 ctermbg=25 guifg=#8ff0a4 guibg=#1c71d8
  highlight FlowBuddyDotNow ctermfg=220 ctermbg=25 guifg=#f6d32d guibg=#1c71d8
  call sign_define('FlowBuddyGoal', {'text': '▶', 'texthl': 'FlowBuddyGoal', 'linehl': 'FlowBuddyGoal'})
  call sign_define('FlowBuddySpot', {'text': '▶', 'texthl': 'FlowBuddyGoal'})
  call sign_define('FlowBuddyCutHere', {'text': '▶', 'texthl': 'FlowBuddyCut'})
  call sign_define('FlowBuddyStart', {'text': '●', 'texthl': 'FlowBuddyStart'})
endfunction

" What the panel says for round `round`: a bar with the drill and the
" rounds done, the goal, the fewest keys and yours so far, and how.
function! flowbuddy#practice#instructions(drill, round, keys) abort
  let r = a:drill.rounds[a:round]
  let n = len(a:drill.rounds)
  let dots = join(map(range(n), 'v:val < a:round ? "●" : v:val == a:round ? "◉" : "○"'), ' ')
  let lines = [printf(' Flow Buddy practice · %s   %s   round %d of %d ', a:drill.title, dots, a:round + 1, n)]
  if has_key(r.goal, 'text')
    let lines += ['Goal   ' . r.ask . ' (in red), so the line reads:', '         ' . join(r.goal.text, ' / ')]
  elseif has_key(r.goal, 'line')
    let d = r.goal.line - r.cursor[0]
    let lines += [printf('Goal   %s ▶, %d line%s %s', r.ask, abs(d), abs(d) == 1 ? '' : 's', d > 0 ? 'down' : 'up')]
  else
    let lines += ['Goal   ' . r.ask . ' (in yellow)']
  endif
  let lines += [flowbuddy#practice#score_line(r.par, a:keys)]
  let lines += ['How    ' . get(a:drill, 'how', a:drill.about)]
  return lines + ['       :q stops; the rounds done are kept']
endfunction

" "Aim 3 keys   You 5 keys (2 over)".
function! flowbuddy#practice#score_line(par, keys) abort
  let line = printf('Aim    %d key%s      You  %d key%s', a:par, a:par == 1 ? '' : 's', a:keys, a:keys == 1 ? '' : 's')
  return a:keys > a:par ? line . printf(' (%d over)', a:keys - a:par) : line
endfunction

function! s:load() abort
  let r = s:drill.rounds[s:round]
  let s:loading = 1
  silent call deletebufline(s:buf, 1, '$')
  call setbufline(s:buf, 1, r.lines)
  call clearmatches()
  call sign_unplace('flowbuddy_practice')
  let [ml, mc, mlen] = r.mark
  let deleting = has_key(r.goal, 'text')
  if mlen == 0
    call sign_place(0, 'flowbuddy_practice', 'FlowBuddyGoal', s:buf, {'lnum': ml})
  else
    call matchaddpos(deleting ? 'FlowBuddyCut' : 'FlowBuddyGoal', [[ml, mc, mlen]])
    call sign_place(0, 'flowbuddy_practice', deleting ? 'FlowBuddyCutHere' : 'FlowBuddySpot', s:buf, {'lnum': ml})
  endif
  " Where you start, when it's another line.
  if r.cursor[0] != ml
    call sign_place(0, 'flowbuddy_practice', 'FlowBuddyStart', s:buf, {'lnum': r.cursor[0]})
  endif
  call s:panel(flowbuddy#practice#instructions(s:drill, s:round, 0))
  " Where you start and the goal both on screen, the goal never hidden.
  call winrestview({'topline': flowbuddy#practice#topline(r.cursor[0], ml, winheight(0)),
        \ 'lnum': r.cursor[0], 'col': r.cursor[1] - 1, 'curswant': r.cursor[1] - 1})
  let s:keys = 0
  let s:shown_keys = 0
  let s:started = reltime()
  " Your keys so far, as you type them.
  call timer_stop(s:counter)
  let s:counter = timer_start(150, {-> s:update_count()}, {'repeat': -1})
  " The setline above fires TextChanged later; it isn't a move.
  call timer_start(0, {-> s:loaded()})
endfunction

" The first line to show, in a window `height` tall, so lines `a` and `b`
" both fit with a line or two around them.
function! flowbuddy#practice#topline(a, b, height) abort
  let [top, bot] = [min([a:a, a:b]), max([a:a, a:b])]
  let span = bot - top + 1
  if span >= a:height
    return max([1, bot - a:height + 2])
  endif
  return max([1, top - (a:height - span) / 2])
endfunction

function! s:update_count() abort
  if s:loading || s:done || s:panel_buf == -1 || s:keys == s:shown_keys
    return
  endif
  let s:shown_keys = s:keys
  let r = s:drill.rounds[s:round]
  let lines = getbufline(s:panel_buf, 1, '$')
  let row = match(lines, '^Aim ')
  if row >= 0
    call setbufvar(s:panel_buf, '&modifiable', 1)
    call setbufline(s:panel_buf, row + 1, flowbuddy#practice#score_line(r.par, s:keys))
    call setbufvar(s:panel_buf, '&modifiable', 0)
  endif
endfunction

function! s:loaded() abort
  let s:loading = 0
endfunction

function! s:reached(goal) abort
  if has_key(a:goal, 'line')
    return line('.') == a:goal.line
  elseif has_key(a:goal, 'cursor')
    return line('.') == a:goal.cursor[0] && col('.') == a:goal.cursor[1]
  endif
  return getline(1, '$') ==# a:goal.text
endfunction

function! flowbuddy#practice#check() abort
  call s:check()
endfunction

function! s:check() abort
  if s:loading || s:done || empty(s:drill) || bufnr() != s:buf
    return
  endif
  let r = s:drill.rounds[s:round]
  if !s:reached(r.goal)
    return
  endif
  call add(s:results, {'keys': s:keys, 'par': r.par, 'best': r.best,
        \ 'ms': float2nr(reltimefloat(reltime(s:started)) * 1000)})
  let s:round += 1
  if s:round >= len(s:drill.rounds)
    call s:finish(0)
    return
  endif
  " A moment to see it worked, with the fewest keys if you used more.
  let s:loading = 1
  call sign_unplace('flowbuddy_practice')
  call clearmatches()
  call timer_stop(s:counter)
  let msg = s:keys <= r.par ? printf(' ✓ Perfect: %d key%s, the fewest there is. ', s:keys, s:keys == 1 ? '' : 's')
        \ : printf(' ✓ Done in %d keys. %s does it in %d. ', s:keys, r.best, r.par)
  call s:panel([msg, '', 'Next round…'])
  call timer_start(1200, {-> s:load()})
endfunction

" The drill buffer went away (:q, :bd): keep what was done.
function! s:abandoned() abort
  call s:finish(1)
  call s:panel_close()
endfunction

" Write the results; `leaving`: the drill or Vim is going away.
function! s:finish(leaving) abort
  if s:done
    return
  endif
  let s:done = 1
  call timer_stop(s:counter)
  unlet! g:flowbuddy_practising
  call sign_unplace('flowbuddy_practice')
  " JSON booleans, not 0 and 1.
  let stopped = s:round < len(get(s:drill, 'rounds', [])) ? v:true : v:false
  call writefile([json_encode({'rounds': s:results, 'stopped': stopped})], s:out)
  if !empty(s:record)
    " Back to being coached; the result is kept and reported.
    if exists('*flowbuddy#on_focus')
      call flowbuddy#on_focus(1)
    endif
    let report = systemlist(s:record[0] . ' practice ' . s:record[1] . ' --record ' . shellescape(s:out))
    call delete(s:out)
    let s:record = []
    if a:leaving
      echo get(report, 0, '')
    else
      call s:panel(report + ['', ':bd closes the drill.'])
    endif
    return
  endif
  if !a:leaving
    call s:panel(s:quit ? ['All done. Closing…'] : ['All done.'])
    if s:quit
      call timer_start(700, {-> execute('qa!')})
    endif
  endif
endfunction

" The panel at the top: a window of its own, as tall as what it says.
let s:panel_buf = -1

function! s:panel(lines) abort
  let back = win_getid()
  if s:panel_buf == -1 || bufwinid(s:panel_buf) == -1
    silent topleft new
    setlocal buftype=nofile bufhidden=wipe noswapfile nobuflisted nonumber norelativenumber
    setlocal nowrap nolist nocursorline winfixheight signcolumn=no
    silent! file [Flow\ Buddy\ drill]
    syntax match FlowBuddyBar /^ Flow Buddy practice .*/
    syntax match FlowBuddyDotDone /●/ contained containedin=FlowBuddyBar
    syntax match FlowBuddyDotNow /◉/ contained containedin=FlowBuddyBar
    syntax match FlowBuddyLabel /^\(Goal\|Aim\|How\)\>/
    syntax match FlowBuddyLabel /\<You\>/
    syntax match FlowBuddyOver /(\d\+ over)/
    syntax match FlowBuddyExpect /^         \S.*/
    syntax match FlowBuddyGoal /▶/
    syntax match FlowBuddyPerfect /^ ✓ Perfect.*/
    syntax match FlowBuddyClose /^ ✓ Done.*/
    let s:panel_buf = bufnr()
  endif
  call win_gotoid(back)
  call setbufvar(s:panel_buf, '&modifiable', 1)
  silent call deletebufline(s:panel_buf, 1, '$')
  call setbufline(s:panel_buf, 1, a:lines)
  call setbufvar(s:panel_buf, '&modifiable', 0)
  call win_execute(bufwinid(s:panel_buf), 'resize ' . len(a:lines))
endfunction

function! s:panel_close() abort
  let win = bufwinid(s:panel_buf)
  if win != -1 && winnr('$') > 1
    call win_execute(win, 'close')
  endif
  let s:panel_buf = -1
endfunction

" What the panel says now, for the tests.
function! flowbuddy#practice#panel() abort
  return s:panel_buf == -1 ? [] : getbufline(s:panel_buf, 1, '$')
endfunction

" The flowbuddy command: on the PATH, or where the installer puts it.
function! flowbuddy#practice#cli() abort
  let cli = exepath('flowbuddy')
  if empty(cli) && executable(expand('~/.local/bin/flowbuddy'))
    let cli = expand('~/.local/bin/flowbuddy')
  endif
  return cli
endfunction

" :FlowBuddyPractice [drill]: a drill here, in this Vim.
function! flowbuddy#practice#start(...) abort
  let cli = flowbuddy#practice#cli()
  if empty(cli)
    echohl ErrorMsg | echo 'Flow Buddy: the flowbuddy command isn''t installed (scripts/install.sh)' | echohl None
    return
  endif
  let args = a:0 ? ' ' . shellescape(a:1) : ''
  let out = system(shellescape(cli) . ' practice --print-drill' . args)
  if v:shell_error
    echohl ErrorMsg | echo 'Flow Buddy: ' . trim(substitute(out, '^flowbuddy: ', '', '')) | echohl None
    return
  endif
  let drill = tempname()
  call writefile([out], drill)
  let family = json_decode(out).family
  if type(json_decode(out).why) == v:t_string
    echo 'A drill for: ' . json_decode(out).why
  endif
  call flowbuddy#practice#run(drill, tempname(), 0, [shellescape(cli), family])
  call delete(drill)
endfunction

" :FlowBuddyMap: the skill map in a scratch window.
function! flowbuddy#practice#map() abort
  let cli = flowbuddy#practice#cli()
  if empty(cli)
    echohl ErrorMsg | echo 'Flow Buddy: the flowbuddy command isn''t installed (scripts/install.sh)' | echohl None
    return
  endif
  let lines = systemlist(shellescape(cli) . ' learn --map')
  new
  setlocal buftype=nofile bufhidden=wipe noswapfile nonumber norelativenumber
  silent! file [Flow\ Buddy\ skills]
  call setline(1, lines)
  setlocal nomodifiable
  syntax match FlowBuddyKnown /+ [^ ].\{-}\ze\(   \|$\)/
  syntax match FlowBuddySlipping /! [^ ].\{-}\ze\(   \|$\)/
  syntax match FlowBuddyLearning /\~ [^ ].\{-}\ze\(   \|$\)/
  syntax match FlowBuddyReady /> [^ ].\{-}\ze\(   \|$\)/
  syntax match FlowBuddyLater /\. [^ ].\{-}\ze\(   \|$\)/
  highlight default link FlowBuddyKnown DiffAdd
  highlight default link FlowBuddySlipping WarningMsg
  highlight default link FlowBuddyLearning Identifier
  highlight default link FlowBuddyReady Title
  highlight default link FlowBuddyLater Comment
endfunction

" For the tests.
function! flowbuddy#practice#state() abort
  return {'round': s:round, 'keys': s:keys, 'results': s:results, 'done': s:done}
endfunction
