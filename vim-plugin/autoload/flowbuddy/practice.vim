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
let s:panel = -1
let s:loading = 0
let s:done = 0
let s:quit = 1
" With :FlowBuddyPractice: the flowbuddy command and the drill's name, to
" record the result when it ends.
let s:record = []

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
function! flowbuddy#practice#run(drill_path, out_path, ...) abort
  let s:quit = get(a:000, 0, 1)
  let s:record = get(a:000, 1, [])
  let s:drill = json_decode(join(readfile(a:drill_path), "\n"))
  let s:out = a:out_path
  let s:results = []
  let s:round = 0
  let s:done = 0
  " Tips about the drill's keys would only get in the way.
  if exists('*flowbuddy#on_focus')
    call flowbuddy#on_focus(0)
  endif
  " Changes in the buffer you were in stay there.
  if &modified
    silent new
  else
    silent enew
  endif
  setlocal buftype=nofile bufhidden=wipe noswapfile nolist nowrap nospell
  setlocal number
  let &l:relativenumber = get(s:drill, 'relative_numbers', v:false) is v:true
  silent! file [Flow\ Buddy\ practice]
  highlight default link FlowBuddyTarget Search
  call s:map_keys()
  augroup flowbuddy_practice
    autocmd!
    autocmd CursorMoved,TextChanged <buffer> call s:check()
    autocmd VimLeavePre * call s:finish(1)
  augroup END
  call s:load()
endfunction

function! s:load() abort
  let r = s:drill.rounds[s:round]
  let s:loading = 1
  silent call deletebufline('%', 1, '$')
  call setline(1, r.lines)
  call clearmatches()
  let [ml, mc, mlen] = r.mark
  if mlen == 0
    call matchaddpos('FlowBuddyTarget', [ml])
  else
    call matchaddpos('FlowBuddyTarget', [[ml, mc, mlen]])
  endif
  call cursor(r.cursor[0], r.cursor[1])
  let lines = [printf('%s: round %d of %d', s:drill.title, s:round + 1, len(s:drill.rounds)), r.ask]
  if type(r.goal) == v:t_dict && has_key(r.goal, 'text')
    let lines += ['It should read:', '  ' . join(r.goal.text, ' / ')]
  endif
  if s:round == 0
    let lines += [''] + flowbuddy#wrap(s:drill.about, 56)
  endif
  let lines += ['', ':q to stop']
  call s:show(lines)
  let s:keys = 0
  let s:started = reltime()
  " The setline above fires TextChanged later; it isn't a move.
  call timer_start(0, {-> s:loaded()})
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
  if s:loading || s:done || empty(s:drill)
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
  let msg = s:keys <= r.par ? printf('Done in %d: the fewest.', s:keys)
        \ : printf('Done in %d keys; %s does it in %d.', s:keys, r.best, r.par)
  call s:show([msg])
  call timer_start(900, {-> s:load()})
endfunction

" Write the results; `leaving`: Vim is quitting (stopped early, or done).
function! s:finish(leaving) abort
  if s:done
    return
  endif
  let s:done = 1
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
    if !a:leaving
      call s:show(report + ['', ':bd closes the drill'])
      call timer_start(15000, {-> s:close_panel()})
    endif
    return
  endif
  if !a:leaving
    call s:show(s:quit ? ['All done.', 'Closing…'] : ['All done.'])
    if s:quit
      call timer_start(700, {-> execute('qa!')})
    endif
  endif
endfunction

function! s:close_panel() abort
  if s:panel == -1
    return
  endif
  if has('nvim')
    if nvim_win_is_valid(s:panel)
      call nvim_win_close(s:panel, v:true)
    endif
  else
    call popup_close(s:panel)
  endif
  let s:panel = -1
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

" A small window at the top right with the round's instructions.
function! s:show(lines) abort
  if has('nvim')
    if s:panel != -1 && nvim_win_is_valid(s:panel)
      call nvim_win_close(s:panel, v:true)
    endif
    let buf = nvim_create_buf(v:false, v:true)
    call nvim_buf_set_lines(buf, 0, -1, v:false, a:lines)
    let width = max(map(copy(a:lines), 'strdisplaywidth(v:val)')) + 1
    let s:panel = nvim_open_win(buf, v:false, {'relative': 'editor', 'anchor': 'NE',
          \ 'row': 0, 'col': &columns, 'width': width, 'height': len(a:lines),
          \ 'style': 'minimal', 'border': 'rounded', 'focusable': v:false, 'zindex': 250})
  else
    if s:panel != -1
      call popup_close(s:panel)
    endif
    let s:panel = popup_create(a:lines, {'line': 1, 'col': &columns, 'pos': 'topright',
          \ 'highlight': 'Pmenu', 'border': [], 'padding': [0, 1, 0, 1], 'zindex': 250})
  endif
endfunction

" For the tests.
function! flowbuddy#practice#state() abort
  return {'round': s:round, 'keys': s:keys, 'results': s:results, 'done': s:done}
endfunction
