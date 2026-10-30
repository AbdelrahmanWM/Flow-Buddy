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
let s:facts_started = reltime()
let s:stats = {'facts': 0, 'over_budget': 0, 'max_ms': 0}
" What the daemon allows with your text ([text] in its config): off until
" it says otherwise. See flowbuddy#text_allowed.
let s:text = {'shapes': v:false, 'snippets': v:false, 'exclude': [], 'projects': []}
let s:partial = ''
" With [text] ai_snippets on: lines around recent moves and edits, as
" {at_ms, filetype, lines}, for `flowbuddy coach`. In memory only, and
" only sent when the daemon asks for the moments of recent tips.
let s:snippets = []
let s:SNIPPETS = 20
let s:SNIPPET_MS = 1800000

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
      let s:ch = sockconnect('pipe', path, {'on_data': function('s:on_data')})
    else
      let s:ch = ch_open('unix:' . path, {'mode': 'nl', 'callback': function('s:on_line')})
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
        \ 'pid': empty($SSH_CONNECTION) ? getpid() : v:null,
        \ 'shows_tips': flowbuddy#shows_tips()})
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

" From the daemon: only its text settings matter here; welcome and acks
" are ignored.
function! s:on_line(ch, line) abort
  call flowbuddy#on_daemon(a:line)
endfunction

" Neovim hands over chunks: the first continues the last partial line.
function! s:on_data(ch, data, event) abort
  if empty(a:data)
    return
  endif
  let lines = copy(a:data)
  let lines[0] = s:partial . lines[0]
  let s:partial = remove(lines, -1)
  for line in lines
    call flowbuddy#on_daemon(line)
  endfor
endfunction

function! flowbuddy#on_daemon(line) abort
  try
    let msg = json_decode(a:line)
  catch
    return
  endtry
  if type(msg) != v:t_dict
    return
  endif
  let type = get(msg, 'type', '')
  if type ==# 'editor_config'
    let s:text = {'shapes': get(msg, 'shapes', v:false) is v:true,
          \ 'snippets': get(msg, 'snippets', v:false) is v:true,
          \ 'exclude': get(msg, 'exclude', []), 'projects': get(msg, 'projects', [])}
    if !s:text.snippets
      let s:snippets = []
    endif
  elseif type ==# 'nudge'
    call flowbuddy#show_tip(msg)
  elseif type ==# 'snippet_request'
    let found = s:text.snippets ? flowbuddy#snippets_for(get(msg, 'times', [])) : []
    call s:send({'type': 'snippets', 'id': get(msg, 'id', 0), 'snippets': found})
  endif
endfunction

" Tips ------------------------------------------------------------------------

" Tips show here, in a small window at the top right, rather than on the
" desktop. Off with `let g:flowbuddy_tips = 0`.
let s:tip = {}
let s:tip_win = -1
let s:tip_timer = -1

function! flowbuddy#shows_tips() abort
  if !get(g:, 'flowbuddy_tips', 1)
    return v:false
  endif
  " A JSON true, not 1: the daemon wants a boolean.
  return (has('nvim') ? exists('*nvim_open_win') : has('popupwin')) ? v:true : v:false
endfunction

" The tip's lines for a window `width` wide: the title, the text wrapped,
" then what its commands do. Commands keep their backticks.
function! flowbuddy#tip_lines(msg, width) abort
  let lines = [get(a:msg, 'title', 'Flow Buddy')]
  let lines += flowbuddy#wrap(get(a:msg, 'text', ''), a:width)
  for e in get(a:msg, 'explain', [])
    let lines += map(flowbuddy#wrap(e, a:width - 2), '"  " . v:val')
  endfor
  return lines
endfunction

function! flowbuddy#wrap(text, width) abort
  let out = []
  let line = ''
  for word in split(a:text)
    if line ==# ''
      let line = word
    elseif strdisplaywidth(line . ' ' . word) <= a:width
      let line .= ' ' . word
    else
      call add(out, line)
      let line = word
    endif
  endfor
  if line !=# ''
    call add(out, line)
  endif
  return out
endfunction

function! flowbuddy#show_tip(msg) abort
  if !flowbuddy#shows_tips() || type(get(a:msg, 'nudge_id', 0)) != v:t_string
    return
  endif
  " Not over a practice drill.
  if get(g:, 'flowbuddy_practising', 0)
    return
  endif
  call s:close_tip('seen')
  let width = max([20, min([60, &columns - 6])])
  let lines = flowbuddy#tip_lines(a:msg, width)
  let ttl = get(a:msg, 'ttl_ms', 6000)
  let s:tip = a:msg
  if has('nvim')
    let buf = nvim_create_buf(v:false, v:true)
    call nvim_buf_set_lines(buf, 0, -1, v:false, lines)
    let s:tip_win = nvim_open_win(buf, v:false, {'relative': 'editor', 'anchor': 'NE',
          \ 'row': 0, 'col': &columns, 'width': width, 'height': len(lines),
          \ 'style': 'minimal', 'border': 'rounded', 'focusable': v:false, 'zindex': 250})
    call nvim_win_set_option(s:tip_win, 'winhighlight', 'Normal:Pmenu')
    let s:tip_timer = timer_start(ttl, {-> s:close_tip('seen')})
  else
    let s:tip_win = popup_create(lines, {'line': 1, 'col': &columns, 'pos': 'topright',
          \ 'minwidth': 20, 'maxwidth': width, 'time': ttl, 'highlight': 'Pmenu',
          \ 'border': [], 'padding': [0, 1, 0, 1], 'zindex': 250, 'close': 'click',
          \ 'callback': {-> s:tip_closed()}})
  endif
endfunction

" The window went away on its own (timed out or clicked): seen.
function! s:tip_closed() abort
  " Closed by s:close_tip, which answers.
  if s:tip_win == -1
    return
  endif
  let s:tip_win = -1
  call s:answer('seen')
endfunction

" Close the tip, answering `action` for it.
function! s:close_tip(action) abort
  call timer_stop(s:tip_timer)
  let win = s:tip_win
  let s:tip_win = -1
  if win != -1
    if has('nvim')
      if nvim_win_is_valid(win)
        call nvim_win_close(win, v:true)
      endif
    else
      call popup_close(win)
    endif
  endif
  call s:answer(a:action)
endfunction

" Each tip gets one answer: the first.
function! s:answer(action) abort
  if empty(s:tip)
    return
  endif
  let msg = {'type': 'nudge_response', 'nudge_id': s:tip.nudge_id, 'action': a:action}
  call s:send(msg)
  call add(s:answers, msg)
  let s:answers = s:answers[max([0, len(s:answers) - 5]):]
  let s:tip = {}
endfunction

" The last answers sent, for the tests.
let s:answers = []
function! flowbuddy#answers() abort
  return s:answers
endfunction

" :FlowBuddyGotIt and :FlowBuddyNotHelpful, for the last tip.
function! flowbuddy#respond(action) abort
  if empty(s:tip)
    echo 'Flow Buddy: no tip to answer'
    return
  endif
  call s:close_tip(a:action)
endfunction

" Reselecting ------------------------------------------------------------------

" The last Visual selection as [buffer, start, end, kind, when]. Positions
" stay here: only "the same again" is sent.
let s:last_visual = []
let s:in_visual = 0
let s:RESELECT_MS = 60000

function! s:visual_ended(new_mode) abort
  let visual = a:new_mode =~# "^[vV\x16]"
  if visual || !s:in_visual
    let s:in_visual = visual
    return
  endif
  let s:in_visual = 0
  let sel = [bufnr(), getpos("'<")[1:2], getpos("'>")[1:2], visualmode()]
  if flowbuddy#same_selection(s:last_visual, sel, s:now_ms())
    call s:send({'type': 'editor_event', 'event': 'reselected'})
  endif
  let s:last_visual = sel + [s:now_ms()]
endfunction

" Whether `sel` is `last` again, soon after it.
function! flowbuddy#same_selection(last, sel, now) abort
  return !empty(a:last) && a:last[:3] == a:sel && a:now - a:last[4] <= s:RESELECT_MS
endfunction

" Text features --------------------------------------------------------------

" Whether `pattern` matches `path`, as crates/core/src/text.rs does: no
" `/` matches the file name, otherwise the end of the path; `*` is any run.
function! flowbuddy#glob_match(pattern, path) abort
  if a:pattern !~# '/'
    return fnamemodify(a:path, ':t') =~# glob2regpat(a:pattern)
  endif
  let p = substitute(a:pattern, '^/\+', '', '')
  return a:path =~# glob2regpat('*/' . p) || a:path =~# glob2regpat(p)
endfunction

" Whether this buffer's text may be looked at for shapes, or for
" snippets too: a real file, not encrypted, not excluded, and for
" snippets inside one of your projects. Terminals, prompts and other
" special buffers never are.
function! flowbuddy#text_allowed(snippets) abort
  if !(a:snippets ? s:text.snippets : s:text.shapes)
    return 0
  endif
  if &buftype !=# '' || (exists('&key') && !empty(&key))
    return 0
  endif
  let path = expand('%:p')
  if path !~# '^/'
    return 0
  endif
  for pattern in s:text.exclude
    if flowbuddy#glob_match(pattern, path)
      return 0
    endif
  endfor
  if a:snippets
    return !empty(filter(copy(s:text.projects), {_, p -> !empty(p) && stridx(path, p . '/') == 0}))
  endif
  return 1
endfunction

" Keep the lines around `line` of this buffer, if it may be looked at.
function! s:keep_snippet(line, at_ms) abort
  if !flowbuddy#text_allowed(1)
    return
  endif
  let lines = map(getline(max([1, a:line - 2]), min([line('$'), a:line + 2])),
        \ {_, l -> strcharpart(l, 0, 200)})
  call add(s:snippets, {'at_ms': a:at_ms, 'filetype': &filetype, 'lines': lines})
  let now = s:now_ms()
  call filter(s:snippets, {_, s -> now - s.at_ms <= s:SNIPPET_MS})
  if len(s:snippets) > s:SNIPPETS
    call remove(s:snippets, 0, len(s:snippets) - s:SNIPPETS - 1)
  endif
endfunction

" The kept snippet closest to each of `times`, within 3 s.
function! flowbuddy#snippets_for(times) abort
  let out = []
  for t in a:times
    let best = {}
    for s in s:snippets
      let d = abs(s.at_ms - t)
      if d <= 3000 && (empty(best) || d < abs(best.at_ms - t))
        let best = s
      endif
    endfor
    if !empty(best) && index(out, best) < 0
      call add(out, best)
    endif
  endfor
  return out
endfunction

" For tests.
function! flowbuddy#keep_snippet(line, at_ms) abort
  call s:keep_snippet(a:line, a:at_ms)
endfunction

" For tests.
function! flowbuddy#set_text(text) abort
  let s:text = extend(copy(s:text), a:text)
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
  call s:visual_ended(a:new_mode)
  call s:send({'type': 'editor_mode', 'mode': s:mode_name(a:new_mode)})
endfunction

" After a : command: if Vim said "Not an editor command" (E492), tell the
" daemon. Only that it happened: not the command, which the daemon knows
" the name of from the keys.
function! flowbuddy#on_cmdline_leave() abort
  let s:errmsg_before = v:errmsg
  call timer_start(0, function('s:check_ex_error'))
endfunction

function! s:check_ex_error(timer) abort
  if v:errmsg !=# s:errmsg_before && flowbuddy#is_unknown_ex(v:errmsg)
    call s:send({'type': 'editor_event', 'event': 'ex_error'})
  endif
endfunction

function! flowbuddy#is_unknown_ex(errmsg) abort
  return a:errmsg =~# '^E492:'
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
  call s:keep_snippet(end[0], seg.end_ms)
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
  let msg = {'type': 'operator', 'op': op, 'at_ms': s:now_ms(), 'objects': objects}
  call s:keep_snippet(l1, msg.at_ms)
  " With [text] shapes on: which motions would have covered it exactly.
  if a:event.regtype ==# 'v' && l1 == l2 && flowbuddy#text_allowed(0)
    let range = flowbuddy#range_facts(getline(l1), c1, c2)
    if !empty(range)
      let msg.range = range
    endif
  elseif a:event.regtype ==# 'V' && flowbuddy#text_allowed(0)
    let msg.range = flowbuddy#line_range(getline(l1, l2), l2 == line('$') ? '' : getline(l2 + 1), l2 == line('$'))
  endif
  if !empty(objects) || has_key(msg, 'range')
    call s:send(msg)
  endif
endfunction

" Facts about whole `lines` an operator covered, given the line after them
" and whether they run to the end: `d}` lands on the next blank line
" when there's none among them; `dG` runs to the end.
function! flowbuddy#line_range(lines, after, at_end) abort
  let f = {'file_end': a:at_end ? v:true : v:false}
  if (a:at_end || a:after !~# '\S') && empty(filter(copy(a:lines), {_, l -> l !~# '\S'}))
    let f.paragraph_hops = 1
  endif
  return {'linewise': v:true, 'lines': len(a:lines) - 1, 'facts': f}
endfunction

" Character classes for w and e within a line: 0 blank, 1 keyword, 2 other.
function! s:class(ch) abort
  return a:ch =~# '\s' ? 0 : a:ch =~# '\k' ? 1 : 2
endfunction

" Facts about a charwise range on one line of `text`, from its first
" character (byte column c1) to its last (c2), as an operator's motion
" could have covered it: e presses to the last character, w presses to the
" one after it (past the line's end counts too), f and t presses, and
" whether it ran to the end of the line. Numbers only.
function! flowbuddy#range_facts(text, c1, c2) abort
  let chars = []
  let col = 1
  for ch in split(a:text, '\zs')
    call add(chars, [col, ch])
    let col += strlen(ch)
  endfor
  let cols = map(copy(chars), {_, c -> c[0]})
  let i1 = index(cols, a:c1)
  let i2 = index(cols, a:c2)
  if i1 < 0 || i2 < i1
    return {}
  endif
  let n = len(chars)
  let cls = map(copy(chars), {_, c -> s:class(c[1])})
  let starts = filter(range(n), {_, j -> cls[j] != 0 && (j == 0 || cls[j - 1] != cls[j])})
  let ends = filter(range(n), {_, j -> cls[j] != 0 && (j == n - 1 || cls[j + 1] != cls[j])})
  let f = {}
  if index(ends, i2) >= 0
    let k = len(filter(copy(ends), {_, j -> j > i1 && j <= i2}))
    if k
      let f.word_end_hops = k
    endif
  endif
  let after = i2 + 1
  let words = 0
  if after >= n
    let words = len(filter(copy(starts), {_, j -> j > i1})) + 1
  elseif index(starts, after) >= 0
    let words = len(filter(copy(starts), {_, j -> j > i1 && j <= after}))
  endif
  let last = chars[i2][1]
  let k = len(filter(range(i1 + 1, i2), {_, j -> chars[j][1] ==# last}))
  if k
    let f.find_hops = k
  endif
  if after < n
    let past = chars[after][1]
    let k = len(filter(range(i1 + 2, after), {_, j -> chars[j][1] ==# past}))
    if k
      let f.till_hops = k
    endif
  endif
  let f.line_end = after >= n ? v:true : v:false
  let out = {'cols': i2 - i1, 'facts': f}
  if words
    let out.words = words
  endif
  " W and E: WORDs are anything between blanks.
  let bcls = map(copy(cls), {_, c -> c == 0 ? 0 : 1})
  let bstarts = filter(range(n), {_, j -> bcls[j] && (j == 0 || !bcls[j - 1])})
  let bends = filter(range(n), {_, j -> bcls[j] && (j == n - 1 || !bcls[j + 1])})
  if after >= n
    let out.bigwords = len(filter(copy(bstarts), {_, j -> j > i1})) + 1
  elseif index(bstarts, after) >= 0
    let out.bigwords = len(filter(copy(bstarts), {_, j -> j > i1 && j <= after}))
  endif
  if index(bends, i2) >= 0
    let k = len(filter(copy(bends), {_, j -> j > i1 && j <= i2}))
    if k
      let out.bigword_ends = k
    endif
  endif
  " % : from a bracket to its match.
  let pair = {'(': ')', '[': ']', '{': '}'}
  let first = chars[i1][1]
  if has_key(pair, first) && chars[i2][1] ==# pair[first]
        \ && s:balanced(join(map(chars[i1 : i2], {_, c -> c[1]}), ''), first, pair[first])
    let out.bracket = v:true
  endif
  " Backward from just after the range (where the cursor was for db, dF).
  if after < n
    let back = {}
    if index(starts, i1) >= 0
      let back.words = len(filter(copy(starts), {_, j -> j >= i1 && j < after}))
    endif
    if index(bstarts, i1) >= 0
      let back.bigwords = len(filter(copy(bstarts), {_, j -> j >= i1 && j < after}))
    endif
    let back.find_hops = len(filter(range(i1, after - 1), {_, j -> chars[j][1] ==# first}))
    if i1 > 0
      let prev = chars[i1 - 1][1]
      let k = len(filter(range(i1 - 1, after - 2), {_, j -> chars[j][1] ==# prev}))
      if k
        let back.till_hops = k
      endif
    endif
    let back.line_start = i1 == 0 ? v:true : v:false
    let back.first_nonblank = i1 == match(a:text, '\S') ? v:true : v:false
    let out.back = back
  endif
  return out
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
    if s:over_budget()
      return 0
    endif
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
    if s:over_budget()
      return 0
    endif
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
  let s:facts_started = reltime()
  try
    return s:facts(a:start, a:end, a:jump)
  finally
    let &eventignore = save_ei
    call winrestview(view)
    let s:simulating = 0
    let ms = reltimefloat(reltime(s:facts_started)) * 1000
    let s:stats.facts += 1
    let s:stats.max_ms = max([s:stats.max_ms, float2nr(ms)])
    if ms > s:budget_ms()
      let s:stats.over_budget += 1
    endif
  endtry
endfunction

" How long working out the facts for one movement may take; past it, the
" motions not tried yet are left out. Long files and slow folding stay
" responsive.
function! s:budget_ms() abort
  return get(g:, 'flowbuddy_facts_budget_ms', 15)
endfunction

function! s:over_budget() abort
  return reltimefloat(reltime(s:facts_started)) * 1000 > s:budget_ms()
endfunction

" How the facts are doing, for :echo flowbuddy#stats().
function! flowbuddy#stats() abort
  return copy(s:stats)
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
    " The number is in the margin: send only how long it is.
    if &number && !&relativenumber
      let f.line_number_digits = strlen(string(a:end[0]))
    endif
    let f.file_start = a:end[0] == 1 ? v:true : v:false
    let f.file_end = a:end[0] == line('$') ? v:true : v:false
    " How far the next few blank-line boundaries are, for }} then 2j.
    let para = lines > 0 ? '}' : '{'
    call cursor(a:start[0], a:start[1])
    let bounds = []
    let prev = a:start[0]
    for _ in range(9)
      if s:over_budget()
        break
      endif
      execute 'keepjumps normal! ' . para
      if line('.') == prev
        break
      endif
      let prev = line('.')
      call add(bounds, abs(prev - a:start[0]))
    endfor
    if !empty(bounds)
      let f.paragraph_lines = bounds
    endif
    let n = s:line_hops(a:start, a:end[0], lines > 0 ? ']]' : '[[')
    if n
      let f.section_hops = n
    endif
    if &wrap
      let n = s:hops(a:start, a:end, lines > 0 ? 'gj' : 'gk', 0)
      if n
        let f.screen_line_hops = n
      endif
    endif
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
  let back = lines < 0 || (lines == 0 && a:end[1] < a:start[1])
  if s:hops(a:start, a:end, back ? '[(' : '])', 0) == 1
    let f.unmatched_paren = v:true
  endif
  let n = s:hops(a:start, a:end, back ? '(' : ')', 0)
  if n
    let f.sentence_hops = n
  endif
  " / and the first few letters at the end: how many make it the first
  " match. Only the count is sent.
  let rest = strpart(getline(a:end[0]), a:end[1] - 1)
  for k in range(1, 8)
    if k > strchars(rest) || s:over_budget()
      break
    endif
    call cursor(a:start[0], a:start[1])
    let pat = '\V' . escape(strcharpart(rest, 0, k), '\')
    if searchpos(pat, back ? 'bnW' : 'nW') == a:end
      let f.search_chars = k
      break
    endif
  endfor
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
      if s:over_budget()
        break
      endif
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
      if s:over_budget()
        break
      endif
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
  " g;: the line of the last change, which '. marks.
  let changed = line("'.")
  let f.last_change = changed > 0 && changed == a:end[0] && a:start[0] != a:end[0]
        \ ? v:true : v:false
  let f.relative_numbers = &relativenumber ? v:true : v:false
  return f
endfunction
