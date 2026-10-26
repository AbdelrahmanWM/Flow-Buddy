" Flow Buddy plugin internals. See plugin/flowbuddy.vim.
"
" Messages follow docs/architecture.md#editor-clients. A movement starts at
" the first cursor move in Normal or Visual mode and ends after the cursor
" rests for g:flowbuddy_rest_ms; then facts about where it ended are worked
" out here, from the buffer, and only the resulting numbers are sent.

let s:VERSION = '0.1.0'
" Give up looking for a motion that lands exactly after this many presses.
let s:MAX_HOPS = 20
let s:RECONNECT_MS = 5000

let s:ch = v:null
let s:reconnect_timer = -1
let s:epoch_base_ms = 0
let s:started = []
let s:last = []
let s:last_top = 0
let s:seg = {}
" Lines the cursor rested on lately, as [line, ms], for revisits. Never sent.
let s:rests = []
let s:REST_HISTORY_MS = 600000
let s:rest_timer = -1
let s:simulating = 0

" Connection ----------------------------------------------------------------

function! s:socket_path() abort
  if !empty($FLOWBUDDY_SOCKET)
    return $FLOWBUDDY_SOCKET
  endif
  " Over ssh: the daemon's socket, forwarded (see `flowbuddy ssh-config`).
  if !empty($SSH_CONNECTION)
    return '/tmp/flowbuddy-' . $USER . '.sock'
  endif
  let dir = $XDG_RUNTIME_DIR
  if empty(dir)
    let dir = '/run/user/' . trim(system('id -u'))
  endif
  return dir . '/flowbuddy.sock'
endfunction

function! s:now_ms() abort
  return s:epoch_base_ms + float2nr(reltimefloat(reltime(s:started)) * 1000)
endfunction

function! s:open() abort
  let path = s:socket_path()
  if !filereadable(path) && getftype(path) !=# 'socket'
    return 0
  endif
  try
    if has('nvim')
      let s:ch = sockconnect('pipe', path, {'on_data': function('s:ignore')})
    else
      let s:ch = ch_open('unix:' . path, {'mode': 'nl', 'callback': function('s:ignore')})
      if ch_status(s:ch) !=# 'open'
        let s:ch = v:null
      endif
    endif
  catch
    let s:ch = v:null
  endtry
  if s:ch is v:null || s:ch is 0
    let s:ch = v:null
    return 0
  endif
  " Over ssh this pid is on another machine: it would only confuse.
  call s:send({'type': 'hello', 'client': 'editor', 'client_version': s:VERSION,
        \ 'editor': has('nvim') ? 'nvim' : 'vim',
        \ 'pid': empty($SSH_CONNECTION) ? getpid() : v:null})
  call s:send({'type': 'editor_mode', 'mode': s:mode_name(mode(1))})
  call s:send(extend({'type': 'editor_mappings'}, flowbuddy#mappings(execute('imap'), execute('nmap'))))
  return 1
endfunction

" Your mappings that matter for following the mode, from :imap and :nmap
" output: two-key Insert mappings to Esc (`jk`), and single Normal keys
" mapped to something else. Only the keys are sent.
function! flowbuddy#mappings(imap, nmap) abort
  let escapes = []
  for line in split(a:imap, "\n")
    let m = matchlist(line, '^\S*\s\+\(\S\+\)\s\+[*&@ ]*\(.*\)$')
    if !empty(m) && m[1] =~# '^[^<]\{2}$' && m[2] =~? '<Esc>\|<C-\[>'
      call add(escapes, m[1])
    endif
  endfor
  let normal = []
  for line in split(a:nmap, "\n")
    let m = matchlist(line, '^\S*\s\+\(\S\+\)\s')
    if !empty(m) && m[1] =~# '^[^<]$'
      call add(normal, m[1])
    endif
  endfor
  return {'insert_escapes': escapes[:15], 'normal': uniq(sort(normal))[:63]}
endfunction

function! s:connected() abort
  if s:ch is v:null
    return 0
  endif
  if has('nvim')
    return 1
  endif
  return ch_status(s:ch) ==# 'open'
endfunction

function! s:send(msg) abort
  if !s:connected()
    return
  endif
  let line = json_encode(extend({'v': 0}, a:msg)) . "\n"
  try
    if has('nvim')
      call chansend(s:ch, line)
    else
      call ch_sendraw(s:ch, line)
    endif
  catch
    " The daemon went away; reconnect later.
    let s:ch = v:null
  endtry
endfunction

" The daemon's replies (welcome, acks) aren't needed.
function! s:ignore(...) abort
endfunction

function! s:retry(...) abort
  if !s:connected()
    call s:open()
  endif
endfunction

function! flowbuddy#start() abort
  if !empty(s:started)
    return
  endif
  let s:started = reltime()
  if has('nvim')
    let [sec, usec] = luaeval('{vim.uv.gettimeofday()}')
    let s:epoch_base_ms = sec * 1000 + usec / 1000
  else
    let s:epoch_base_ms = str2nr(trim(system('date +%s%3N')))
  endif
  let s:last = getpos('.')[1:2]
  let s:last_top = line('w0')
  call s:open()
  let s:reconnect_timer = timer_start(s:RECONNECT_MS, function('s:retry'), {'repeat': -1})
endfunction

function! flowbuddy#stop() abort
  call timer_stop(s:reconnect_timer)
  call timer_stop(s:rest_timer)
  if s:connected()
    if has('nvim')
      call chanclose(s:ch)
    else
      call ch_close(s:ch)
    endif
  endif
  let s:ch = v:null
endfunction

" Events --------------------------------------------------------------------

" Vim's mode() → the protocol's mode names.
function! s:mode_name(m) abort
  if a:m =~# '^no'
    return 'op_pending'
  elseif a:m =~# '^n'
    return 'normal'
  elseif a:m =~# '^i'
    return 'insert'
  elseif a:m =~# '^R'
    return 'replace'
  elseif a:m ==# 'v'
    return 'visual'
  elseif a:m ==# 'V'
    return 'visual_line'
  elseif a:m ==# "\<C-v>"
    return 'visual_block'
  elseif a:m =~# '^c'
    return 'cmdline'
  endif
  return 'other'
endfunction

function! s:moving_mode(m) abort
  return a:m =~# "^[nvV\<C-v>]$"
endfunction

function! flowbuddy#on_mode(new_mode) abort
  if s:simulating
    return
  endif
  if !s:moving_mode(a:new_mode)
    call s:finish()
  endif
  call s:send({'type': 'editor_mode', 'mode': s:mode_name(a:new_mode)})
endfunction

function! flowbuddy#on_focus(focused) abort
  call s:finish()
  call s:send({'type': 'editor_focus', 'focused': a:focused ? v:true : v:false})
endfunction

function! flowbuddy#on_cursor() abort
  if s:simulating
    return
  endif
  let pos = getpos('.')[1:2]
  if !s:moving_mode(mode(1)) || empty(s:last)
    let s:last = pos
    let s:last_top = line('w0')
    return
  endif
  if empty(s:seg)
    let s:seg = {'start': s:last, 'start_ms': s:now_ms(), 'moves': 0,
          \ 'mode': s:mode_name(mode(1)), 'jump': getpos("''")[1:2], 'top': s:last_top}
  endif
  let s:seg.moves += 1
  let s:seg.end_ms = s:now_ms()
  let s:last = pos
  let s:last_top = line('w0')
  call timer_stop(s:rest_timer)
  let s:rest_timer = timer_start(get(g:, 'flowbuddy_rest_ms', 1000), function('s:rested'))
endfunction

function! s:rested(...) abort
  " Facts are worked out by running motions; not while an operator or a
  " count is pending.
  if mode(1) !=# 'n' && mode(1) !~# "^[vV\<C-v>]$"
    let s:rest_timer = timer_start(300, function('s:rested'))
    return
  endif
  call s:finish()
endfunction

" End the current movement, if any, and report it.
function! s:finish() abort
  call timer_stop(s:rest_timer)
  if empty(s:seg)
    return
  endif
  let seg = s:seg
  let s:seg = {}
  let end = getpos('.')[1:2]
  if end == seg.start || !s:connected()
    return
  endif
  " Facts run motions, which would change a Visual selection.
  let facts = mode(1) ==# 'n' ? flowbuddy#facts(seg.start, end, seg.jump) : {}
  let facts.view_scrolled = line('w0') - seg.top
  let visits = flowbuddy#rested(end[0], s:now_ms())
  if visits
    let facts.revisits = visits
  endif
  call s:send({'type': 'motion', 'start_ms': seg.start_ms, 'end_ms': seg.end_ms,
        \ 'moves': seg.moves, 'mode': seg.mode, 'lines': end[0] - seg.start[0],
        \ 'cols': end[1] - seg.start[1], 'facts': facts})
endfunction

" Count earlier rests on `line` in the last few minutes, then remember this
" one. Lines shift with edits; that's fine for spotting a habit.
function! flowbuddy#rested(line, now) abort
  call filter(s:rests, {_, r -> a:now - r[1] < s:REST_HISTORY_MS})
  let visits = len(filter(copy(s:rests), {_, r -> r[0] == a:line}))
  call add(s:rests, [a:line, a:now])
  if len(s:rests) > 100
    call remove(s:rests, 0)
  endif
  return visits
endfunction

" Operators -----------------------------------------------------------------

let s:OPERATORS = {'d': 'delete', 'c': 'change', 'y': 'yank'}
let s:PAIRS = [['(', ')'], ['[', ']'], ['{', '}'], ['<', '>']]
let s:QUOTES = ['"', "'", '`']

" d, c or y just ran (TextYankPost, before the text is removed): report
" which text objects its range was exactly. The text stays here.
function! flowbuddy#on_operator(event) abort
  let op = get(s:OPERATORS, a:event.operator, '')
  if empty(op) || !s:connected() || index(['v', 'V'], a:event.regtype) < 0
    return
  endif
  let [l1, c1] = getpos("'[")[1:2]
  let [l2, c2] = getpos("']")[1:2]
  if a:event.regtype ==# 'V'
    let objects = flowbuddy#line_objects(a:event.regcontents,
          \ l1 == 1 || getline(l1 - 1) !~# '\S', l2 == line('$') || getline(l2 + 1) !~# '\S')
  else
    let before = matchstr(strpart(getline(l1), 0, c1 - 1), '.$')
    " '] is where the last character starts; the one after follows it.
    let last = matchstr(getline(l2), '\%' . c2 . 'c.')
    let after = matchstr(strpart(getline(l2), c2 - 1 + strlen(last)), '^.')
    let objects = flowbuddy#char_objects(join(a:event.regcontents, "\n"), before, after)
  endif
  if !empty(objects)
    call s:send({'type': 'operator', 'op': op, 'at_ms': s:now_ms(), 'objects': objects})
  endif
endfunction

function! s:edge_or_space(c) abort
  return a:c ==# '' || a:c =~# '\s'
endfunction

function! s:balanced(text, open, close) abort
  let depth = 0
  for ch in split(a:text, '\zs')
    let depth += ch ==# a:open ? 1 : ch ==# a:close ? -1 : 0
    if depth < 0
      return 0
    endif
  endfor
  return depth == 0
endfunction

" Text objects a charwise range `text` is exactly, given the characters just
" before and after it ('' at a line's edge). Best match first.
function! flowbuddy#char_objects(text, before, after) abort
  let found = []
  if empty(a:text)
    return found
  endif
  if a:text =~# '^\k\+$' && a:before !~# '\k' && a:after !~# '\k'
    call add(found, 'iw')
  endif
  if a:text =~# '^\k\+\s\+$' && a:before !~# '\k' && a:after !~# '\s'
    call add(found, 'aw')
  endif
  if a:text =~# '^\S\+$' && s:edge_or_space(a:before) && s:edge_or_space(a:after)
    call add(found, 'iW')
  endif
  if a:text =~# '^\S\+\s\+$' && s:edge_or_space(a:before) && a:after !~# '\s'
    call add(found, 'aW')
  endif
  for [open, close] in s:PAIRS
    if a:before ==# open && a:after ==# close && s:balanced(a:text, open, close)
      call add(found, 'i' . open)
    endif
    if a:text[0] ==# open && a:text[-1:] ==# close && strlen(a:text) >= 2
          \ && s:balanced(a:text[1 : -2], open, close)
      call add(found, 'a' . open)
    endif
  endfor
  if a:text !~# "\n"
    for q in s:QUOTES
      if a:before ==# q && a:after ==# q && stridx(a:text, q) < 0
        call add(found, 'i' . q)
      endif
      if strlen(a:text) >= 2 && a:text[0] ==# q && a:text[-1:] ==# q && stridx(a:text[1 : -2], q) < 0
        call add(found, 'a' . q)
      endif
    endfor
  endif
  return found
endfunction

" Text objects whole lines `lines` are, given whether the lines around
" them are blank (or missing).
function! flowbuddy#line_objects(lines, blank_before, blank_after) abort
  let filled = len(filter(copy(a:lines), {_, l -> l =~# '\S'}))
  if !a:blank_before || filled == 0
    return []
  endif
  if filled == len(a:lines) && a:blank_after
    return ['ip']
  endif
  " A paragraph, then the blank lines after it.
  let text_end = match(a:lines, '^\s*$')
  if text_end > 0 && filled == text_end && !a:blank_after
    return ['ap']
  endif
  return []
endfunction

" Facts ---------------------------------------------------------------------

" How many presses of `keys` from `from` land exactly on `to`; `same_line`
" requires every press to stay on the end line. 0 if none do.
function! s:hops(from, to, keys, same_line) abort
  call cursor(a:from[0], a:from[1])
  let prev = a:from
  for n in range(1, s:MAX_HOPS)
    execute 'keepjumps normal! ' . a:keys
    let pos = getpos('.')[1:2]
    if pos == prev || (a:same_line && pos[0] != a:to[0])
      return 0
    endif
    if pos == a:to
      return n
    endif
    let prev = pos
  endfor
  return 0
endfunction

" Like s:hops, but only the line has to match (vertical motions).
function! s:line_hops(from, to_line, keys) abort
  call cursor(a:from[0], a:from[1])
  let prev = a:from[0]
  for n in range(1, s:MAX_HOPS)
    execute 'keepjumps normal! ' . a:keys
    let line = line('.')
    if line == prev
      return 0
    endif
    if line == a:to_line
      return n
    endif
    if (a:to_line - a:from[0]) * (line - a:to_line) > 0
      return 0
    endif
    let prev = line
  endfor
  return 0
endfunction

" Byte index (1-based) of each occurrence of `ch` in `line`.
function! s:occurrences(line, ch) abort
  let found = []
  let i = stridx(a:line, a:ch)
  while i >= 0
    call add(found, i + 1)
    let i = stridx(a:line, a:ch, i + 1)
  endwhile
  return found
endfunction

function! s:char_at(line, col) abort
  return matchstr(a:line, '\%' . a:col . 'c.')
endfunction

" Facts about moving from `start` to `end` ([line, col], 1-based bytes),
" following MotionFacts in crates/core/src/editor.rs. `jump` is the
" position of the '' mark when the movement started. Moves the cursor
" around to find out and puts everything back.
function! flowbuddy#facts(start, end, jump) abort
  let view = winsaveview()
  let s:simulating = 1
  let save_ei = &eventignore
  set eventignore=all
  try
    return s:facts(a:start, a:end, a:jump)
  finally
    let &eventignore = save_ei
    call winrestview(view)
    let s:simulating = 0
  endtry
endfunction

function! s:facts(start, end, jump) abort
  let f = {}
  let lines = a:end[0] - a:start[0]
  let text = getline(a:end[0])

  " Vertical: about the end line.
  if lines != 0
    let n = s:line_hops(a:start, a:end[0], lines > 0 ? '}' : '{')
    if n
      let f.paragraph_hops = n
    endif
    let top = line('w0')
    let bottom = line('w$')
    if a:end[0] == top
      let f.screen = 'top'
    elseif a:end[0] == bottom
      let f.screen = 'bottom'
    elseif a:end[0] == (top + bottom) / 2
      let f.screen = 'middle'
    endif
    let f.window_lines = winheight(0)
    let f.file_start = a:end[0] == 1 ? v:true : v:false
    let f.file_end = a:end[0] == line('$') ? v:true : v:false
  endif

  " Within the end line, from where a vertical move would leave the cursor.
  let from = [a:end[0], min([a:start[1], max([strlen(text), 1])])]
  let cols = a:end[1] - from[1]
  if cols != 0
    let right = cols > 0
    for [name, fwd, back] in [['word_hops', 'w', 'b'], ['word_end_hops', 'e', 'ge'], ['bigword_hops', 'W', 'B']]
      let n = s:hops(from, a:end, right ? fwd : back, 1)
      if n
        let f[name] = n
      endif
    endfor
    " f/F: the character at the end; t/T: the one just past it.
    let target = s:char_at(text, a:end[1])
    let end_col = a:end[1]
    let hits = filter(s:occurrences(text, target), right
          \ ? {_, c -> c > from[1] && c <= end_col}
          \ : {_, c -> c < from[1] && c >= end_col})
    if !empty(target) && !empty(hits)
      let f.find_hops = len(hits)
    endif
    let past_col = right ? a:end[1] + strlen(target) : a:end[1] - 1
    let past = right ? s:char_at(text, past_col) : matchstr(strpart(text, 0, a:end[1] - 1), '.$')
    if !empty(past)
      let past_col = right ? past_col : a:end[1] - strlen(past)
      let hits = filter(s:occurrences(text, past), right
            \ ? {_, c -> c > from[1] + 1 && c <= past_col}
            \ : {_, c -> c < from[1] - 1 && c >= past_col})
      if !empty(hits)
        let f.till_hops = len(hits)
      endif
    endif
    " Only when the column changed: a j that happens to end in column 1
    " doesn't call for 0.
    let f.line_start = a:end[1] == 1 ? v:true : v:false
    let f.first_nonblank = a:end[1] == match(text, '\S') + 1 ? v:true : v:false
    let last = strlen(text) - strlen(matchstr(text, '.$')) + 1
    let f.line_end = !empty(text) && a:end[1] == last ? v:true : v:false
  endif

  " Anywhere.
  let start_text = getline(a:start[0])
  if s:char_at(start_text, a:start[1]) =~# '[][(){}]'
    call cursor(a:start[0], a:start[1])
    keepjumps normal! %
    let f.bracket_match = getpos('.')[1:2] == a:end ? v:true : v:false
  endif
  call cursor(a:start[0], a:start[1])
  let word = expand('<cword>')
  if word =~# '^\k\+$'
    let pattern = '\V\<' . escape(word, '\') . '\>'
    let back = lines < 0 || (lines == 0 && a:end[1] < a:start[1])
    " Like * and #, count from the start of the word under the cursor.
    call search('\<', 'bcW', a:start[0])
    for n in range(1, s:MAX_HOPS)
      let pos = searchpos(pattern, back ? 'bW' : 'W')
      if pos == [0, 0]
        break
      endif
      if pos == a:end
        let f.star_hops = n
        break
      endif
    endfor
  endif
  " n/N: matches of the last search pattern, which stays here.
  let pattern = getreg('/')
  if !empty(pattern)
    call cursor(a:start[0], a:start[1])
    let back = lines < 0 || (lines == 0 && a:end[1] < a:start[1])
    for n in range(1, s:MAX_HOPS)
      let pos = searchpos(pattern, back ? 'bW' : 'W', 0, 20)
      if pos == [0, 0]
        break
      endif
      if pos == a:end
        let f.search_hops = n
        break
      endif
    endfor
  endif
  let f.jump_back = a:jump == a:end && a:jump != [0, 0] ? v:true : v:false
  let f.relative_numbers = &relativenumber ? v:true : v:false
  return f
endfunction
