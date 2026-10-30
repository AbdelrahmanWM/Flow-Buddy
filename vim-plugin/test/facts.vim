" Run with test/run.sh. Each test sets up a buffer and checks
" flowbuddy#facts against what Vim's own motions would do.
set nocompatible
let &runtimepath = expand('<sfile>:p:h:h') . ',' . &runtimepath
runtime autoload/flowbuddy.vim
set lines=30 columns=80

function! s:buf(lines) abort
  %delete _
  call setline(1, a:lines)
endfunction

function! s:facts(start, end) abort
  return flowbuddy#facts(a:start, a:end, [0, 0])
endfunction

function! Test_paragraphs() abort
  call s:buf(['a', 'b', '', 'c', 'd', '', 'e'])
  call assert_equal(1, s:facts([1, 1], [3, 1]).paragraph_hops)
  call assert_equal(2, s:facts([1, 1], [6, 1]).paragraph_hops)
  call assert_false(has_key(s:facts([1, 1], [5, 1]), 'paragraph_hops'))
  call assert_equal(1, s:facts([7, 1], [6, 1]).paragraph_hops)
endfunction

function! Test_words() abort
  call s:buf(['foo bar.baz qux'])
  let f = s:facts([1, 1], [1, 9])
  " w: foo -> bar -> . -> baz
  call assert_equal(3, f.word_hops)
  call assert_false(has_key(f, 'bigword_hops'))
  call assert_equal(2, s:facts([1, 1], [1, 13]).bigword_hops)
  call assert_equal(1, s:facts([1, 13], [1, 5]).bigword_hops)
  call assert_equal(2, s:facts([1, 13], [1, 1]).bigword_hops)
  " e: foo -> bar
  call assert_equal(2, s:facts([1, 1], [1, 7]).word_end_hops)
endfunction

function! Test_find_and_till() abort
  call s:buf(['a,b,c,d'])
  " f, then ; twice lands on the third comma.
  call assert_equal(3, s:facts([1, 1], [1, 6]).find_hops)
  " t lands just before the character: before the second 'c'... here 'c' once.
  call assert_equal(1, s:facts([1, 1], [1, 4]).till_hops)
  call assert_equal(1, s:facts([1, 7], [1, 6]).find_hops)
endfunction

function! Test_line_positions() abort
  call s:buf(['    indented line'])
  let f = s:facts([1, 10], [1, 5])
  call assert_true(f.first_nonblank)
  call assert_false(f.line_start)
  call assert_true(s:facts([1, 10], [1, 1]).line_start)
  call assert_true(s:facts([1, 5], [1, 17]).line_end)
  " A vertical move that keeps the column says nothing about line ends.
  call s:buf(['abc', 'def'])
  call assert_false(has_key(s:facts([1, 1], [2, 1]), 'line_start'))
endfunction

function! Test_brackets_and_star() abort
  call s:buf(['call(x, (y))', 'x = 1', 'x = 2'])
  call assert_true(s:facts([1, 5], [1, 12]).bracket_match)
  call assert_false(s:facts([1, 5], [1, 9]).bracket_match)
  " * on x (col 6) -> next x on line 2, then line 3.
  call assert_equal(1, s:facts([1, 6], [2, 1]).star_hops)
  call assert_equal(2, s:facts([1, 6], [3, 1]).star_hops)
  " # from line 3 back to line 2.
  call assert_equal(1, s:facts([3, 1], [2, 1]).star_hops)
endfunction

function! Test_file_and_jump() abort
  call s:buf(map(range(1, 10), 'string(v:val)'))
  let f = s:facts([5, 1], [10, 1])
  call assert_true(f.file_end)
  call assert_false(f.file_start)
  call assert_true(s:facts([5, 1], [1, 1]).file_start)
  call assert_true(flowbuddy#facts([5, 1], [2, 1], [2, 1]).jump_back)
  call assert_false(flowbuddy#facts([5, 1], [3, 1], [2, 1]).jump_back)
endfunction

function! Test_line_number_digits() abort
  call s:buf(map(range(1, 200), 'string(v:val)'))
  setlocal number norelativenumber
  call assert_equal(3, s:facts([5, 1], [150, 1]).line_number_digits)
  setlocal relativenumber
  call assert_false(has_key(s:facts([5, 1], [150, 1]), 'line_number_digits'))
  setlocal nonumber norelativenumber
  call assert_false(has_key(s:facts([5, 1], [150, 1]), 'line_number_digits'))
endfunction

function! Test_same_selection() abort
  let sel = [1, [2, 3], [4, 5], 'v']
  call assert_true(flowbuddy#same_selection(sel + [1000], sel, 5000))
  call assert_false(flowbuddy#same_selection(sel + [1000], sel, 90000), 'too long ago')
  call assert_false(flowbuddy#same_selection(sel + [1000], [1, [2, 3], [4, 6], 'v'], 5000))
  call assert_false(flowbuddy#same_selection([], sel, 5000))
endfunction

function! Test_wider_facts() abort
  call s:buf(['a', 'b', '', 'c', 'd', '', 'e', 'f', 'g', 'h'])
  let f = s:facts([1, 1], [8, 1])
  call assert_equal([2, 5, 9], f.paragraph_lines)
  " Sections are lines starting with {, C style.
  call s:buf(['int a()', '{', '  x;', '}', 'int b()', '{', 'let z = (1, 2, 3);'])
  call assert_equal(2, s:facts([1, 1], [6, 1]).section_hops)
  " From inside the parentheses back to the (.
  call assert_true(s:facts([7, 14], [7, 9]).unmatched_paren)
  " Searching for 'z' from line 1 finds it first: one letter.
  call assert_equal(1, s:facts([1, 1], [7, 5]).search_chars)
  call s:buf(['One sentence. Two sentences here. Three.'])
  call assert_equal(2, s:facts([1, 1], [1, 35]).sentence_hops)
endfunction

function! Test_time_budget() abort
  call s:buf(['a', 'b', '', 'c', 'd', '', 'e'])
  call assert_equal(1, s:facts([1, 1], [3, 1]).paragraph_hops)
  let before = flowbuddy#stats().over_budget
  let g:flowbuddy_facts_budget_ms = -1
  let f = s:facts([1, 1], [3, 1])
  unlet g:flowbuddy_facts_budget_ms
  " Out of time: the motions it would have tried are left out.
  call assert_equal(v:t_dict, type(f))
  call assert_false(has_key(f, 'paragraph_hops'))
  call assert_equal(before + 1, flowbuddy#stats().over_budget)
endfunction

function! Test_last_change() abort
  call s:buf(map(range(1, 10), 'string(v:val)'))
  call cursor(3, 1)
  normal! x
  call assert_true(s:facts([9, 1], [3, 1]).last_change)
  call assert_false(s:facts([9, 1], [4, 1]).last_change)
  call assert_false(s:facts([3, 1], [3, 1]).last_change)
endfunction

function! Test_search_hops() abort
  call s:buf(['TODO one', 'x', 'TODO two', 'y', 'TODO three'])
  let @/ = 'TODO'
  call assert_equal(2, s:facts([1, 1], [5, 1]).search_hops)
  call assert_equal(1, s:facts([5, 1], [3, 1]).search_hops)
  call assert_equal('TODO', @/)
  let @/ = ''
  call assert_false(has_key(s:facts([1, 1], [5, 1]), 'search_hops'))
endfunction

function! Test_relative_numbers() abort
  call s:buf(['a', 'b'])
  set norelativenumber
  call assert_false(s:facts([1, 1], [2, 1]).relative_numbers)
  set relativenumber
  call assert_true(s:facts([1, 1], [2, 1]).relative_numbers)
  set norelativenumber
endfunction

function! Test_revisits() abort
  call assert_equal(0, flowbuddy#rested(40, 1000))
  call assert_equal(0, flowbuddy#rested(12, 2000))
  call assert_equal(1, flowbuddy#rested(40, 3000))
  call assert_equal(2, flowbuddy#rested(40, 4000))
  " Forgotten after ten minutes.
  call assert_equal(0, flowbuddy#rested(40, 4000 + 600000))
endfunction

function! Test_char_objects() abort
  call assert_equal(['iw', 'iW'], flowbuddy#char_objects('foo', ' ', ' '))
  call assert_equal(['iw'], flowbuddy#char_objects('foo', '(', ','))
  call assert_equal(['aw', 'aW'], flowbuddy#char_objects('foo ', '', 'b'))
  call assert_equal(['i('], flowbuddy#char_objects('foo, (bar)', '(', ')'))
  call assert_equal(['a('], flowbuddy#char_objects('(a, b)', 'f', ';'))
  " Unbalanced: not the inside of the brackets.
  call assert_equal([], flowbuddy#char_objects('a), (b', '(', ')'))
  call assert_equal(['i"'], flowbuddy#char_objects('hello there', '"', '"'))
  call assert_equal(['a"'], flowbuddy#char_objects('"hi"', '=', ';'))
  call assert_equal([], flowbuddy#char_objects('oo', 'f', ' '))
endfunction

function! Test_line_objects() abort
  call assert_equal(['ip'], flowbuddy#line_objects(['a', 'b'], 1, 1))
  call assert_equal(['ap'], flowbuddy#line_objects(['a', 'b', ''], 1, 0))
  call assert_equal([], flowbuddy#line_objects(['a', 'b'], 0, 1))
  call assert_equal([], flowbuddy#line_objects(['a', '', 'b'], 1, 1))
endfunction

function! Test_operator_reports_from_vim() abort
  " TextYankPost runs before the text goes, so the buffer still has it.
  let got = []
  call s:buf(['call(foo, bar)'])
  call cursor(1, 6)
  let [l1, c1, l2, c2] = [1, 6, 1, 13]
  let before = matchstr(strpart(getline(l1), 0, c1 - 1), '.$')
  let last = matchstr(getline(l2), '\%' . c2 . 'c.')
  let after = matchstr(strpart(getline(l2), c2 - 1 + strlen(last)), '^.')
  call assert_equal(['(', ')'], [before, after])
  call assert_equal(['i('], flowbuddy#char_objects('foo, bar', before, after))
endfunction

function! Test_mappings() abort
  let imap = "i  jk          * <Esc>\ni  jj            <Esc>\ni  <C-s>         * <Esc>:w<CR>\ni  ab            hello"
  let nmap = "n  H           * ^\nn  <Space>f    * :Files<CR>\nn  s             <Plug>(easymotion)"
  let m = flowbuddy#mappings(imap, nmap)
  call assert_equal(['jk', 'jj'], m.insert_escapes)
  call assert_equal(['H', 's'], m.normal)
endfunction

function! Test_facts_leave_no_trace() abort
  call s:buf(['a', '', 'b', '', 'c'])
  call cursor(1, 1)
  let jumps = getjumplist()
  let view = winsaveview()
  call s:facts([1, 1], [4, 1])
  call assert_equal(view, winsaveview())
  call assert_equal(jumps, getjumplist())
  call assert_equal('', @/)
endfunction

function! Test_no_text_in_facts() abort
  call s:buf(['secret = hunter2'])
  let f = s:facts([1, 1], [1, 10])
  for v in values(f)
    call assert_true(type(v) == v:t_number || type(v) == v:t_bool || index(['top', 'middle', 'bottom'], v) >= 0, string(v))
  endfor
endfunction

function! Test_range_facts() abort
  " 'call(foo, bar)': dw from c (col 1) covers 'call', the next word is (.
  let r = flowbuddy#range_facts('call(foo, bar)', 1, 4)
  call assert_equal(1, r.words)
  call assert_equal(1, r.facts.word_end_hops)
  " From f (col 6) to the comma's left: dt, de, or dw (it stops at ,).
  let r = flowbuddy#range_facts('call(foo, bar)', 6, 8)
  call assert_equal(1, r.facts.till_hops)
  call assert_equal(1, r.facts.word_end_hops)
  call assert_equal(1, r.words)
  " 'foo,' and not the space: no w lands just after it.
  call assert_false(has_key(flowbuddy#range_facts('call(foo, bar)', 6, 9), 'words'))
  " To the end of the line: d$, and f) once.
  let r = flowbuddy#range_facts('call(foo, bar)', 6, 14)
  call assert_true(r.facts.line_end)
  call assert_equal(1, r.facts.find_hops)
  " Two words and the space after: d2w.
  let r = flowbuddy#range_facts('one two three', 1, 8)
  call assert_equal(2, r.words)
  call assert_equal({}, flowbuddy#range_facts('abc', 5, 6))
  " Backward, W/E and %: 'x = foo(a, b) + 1', the range 'foo(a, b)'.
  let text = 'x = foo(a, b) + 1'
  " dW from f takes 'foo(a, ' (up to b); dE takes 'foo(a,'.
  call assert_equal(1, flowbuddy#range_facts(text, 5, 11).bigwords)
  call assert_equal(1, flowbuddy#range_facts(text, 5, 10).bigword_ends)
  " From the space after 'foo(a, b)': B twice gets back to f, F once.
  let r = flowbuddy#range_facts(text, 5, 13)
  call assert_equal(2, r.back.bigwords)
  call assert_equal(1, r.back.find_hops)
  call assert_false(r.back.line_start)
  " The bracket part alone: d%.
  call assert_true(flowbuddy#range_facts('x = foo(a, b) + 1', 8, 13).bracket)
  " From the start of the line: d0.
  call assert_true(flowbuddy#range_facts('  abc def', 1, 5).back.line_start)
  call assert_true(flowbuddy#range_facts('  abc def', 3, 5).back.first_nonblank)
endfunction

function! Test_line_range() abort
  let r = flowbuddy#line_range(['a', 'b', 'c'], '', 0)
  call assert_equal({'linewise': v:true, 'lines': 2, 'facts': {'file_end': v:false, 'paragraph_hops': 1}}, r)
  call assert_false(has_key(flowbuddy#line_range(['a', '', 'c'], '', 0).facts, 'paragraph_hops'))
  call assert_false(has_key(flowbuddy#line_range(['a'], 'b', 0).facts, 'paragraph_hops'))
  call assert_true(flowbuddy#line_range(['a'], '', 1).facts.file_end)
endfunction

function! Test_text_guard() abort
  call assert_true(flowbuddy#glob_match('*.pem', '/a/b/c.pem'))
  call assert_false(flowbuddy#glob_match('*.pem', '/a/b.pem/c.txt'))
  call assert_true(flowbuddy#glob_match('.ssh/*', '/home/me/.ssh/known_hosts'))
  call assert_false(flowbuddy#glob_match('.ssh/*', '/home/me/ssh/known_hosts'))
  call assert_true(flowbuddy#glob_match('.env.*', '/x/.env.local'))
  enew!
  call flowbuddy#set_text({'shapes': v:false, 'snippets': v:false, 'exclude': ['.env', '*.pem'], 'projects': ['/home/me/app']})
  execute 'file /home/me/app/main.c'
  call assert_false(flowbuddy#text_allowed(0), 'off')
  call flowbuddy#set_text({'shapes': v:true, 'snippets': v:true})
  call assert_true(flowbuddy#text_allowed(0))
  call assert_true(flowbuddy#text_allowed(1))
  execute 'file /home/me/app/.env'
  call assert_false(flowbuddy#text_allowed(0), '.env')
  execute 'file /tmp/x.c'
  call assert_true(flowbuddy#text_allowed(0))
  call assert_false(flowbuddy#text_allowed(1), 'outside projects')
  setlocal buftype=nofile
  call assert_false(flowbuddy#text_allowed(0), 'not a file')
  bwipe!
  call flowbuddy#set_text({'shapes': v:false, 'snippets': v:false})
  " The daemon's message turns it on.
  call flowbuddy#on_daemon('{"type":"editor_config","shapes":true,"snippets":false,"exclude":[],"projects":[]}')
  enew!
  execute 'file /tmp/y.c'
  call assert_true(flowbuddy#text_allowed(0))
  call assert_false(flowbuddy#text_allowed(1))
  bwipe!
  call flowbuddy#on_daemon('not json')
endfunction

function! Test_snippets() abort
  enew!
  call setline(1, ['a', 'b', 'c', 'd', 'e', 'f'])
  execute 'file /home/me/app/x.py'
  call flowbuddy#set_text({'shapes': v:false, 'snippets': v:true, 'exclude': ['.env'], 'projects': ['/home/me/app']})
  call flowbuddy#keep_snippet(4, 1000)
  call flowbuddy#keep_snippet(1, 9000)
  let got = flowbuddy#snippets_for([1500, 50000])
  call assert_equal(1, len(got))
  call assert_equal(['b', 'c', 'd', 'e', 'f'], got[0].lines)
  " Not in an excluded file, nor once snippets are off.
  execute 'file /home/me/app/.env'
  call flowbuddy#keep_snippet(2, 20000)
  call assert_equal([], flowbuddy#snippets_for([20000]))
  call flowbuddy#on_daemon('{"type":"editor_config","shapes":false,"snippets":false,"exclude":[],"projects":[]}')
  call assert_equal([], flowbuddy#snippets_for([9000]))
  bwipe!
endfunction

function! Test_unknown_ex_command() abort
  call assert_true(flowbuddy#is_unknown_ex('E492: Not an editor command: W'))
  call assert_false(flowbuddy#is_unknown_ex('E37: No write since last change'))
  call assert_false(flowbuddy#is_unknown_ex(''))
endfunction

function! Test_tip_lines() abort
  let msg = {'title': 'Delete inside', 'text': 'That took 6 keys. `di(` does it from anywhere inside, in 3.',
        \ 'explain': ['`di(` delete inside the brackets']}
  call assert_equal(['Delete inside', 'That took 6 keys. `di(`', 'does it from anywhere', 'inside, in 3.',
        \ '  `di(` delete inside', '  the brackets'], flowbuddy#tip_lines(msg, 23))
  call assert_equal(['Flow Buddy'], flowbuddy#tip_lines({}, 40))
endfunction

function! Test_shows_tips_is_a_boolean() abort
  call assert_equal('true', json_encode(flowbuddy#shows_tips()))
  let g:flowbuddy_tips = 0
  call assert_equal('false', json_encode(flowbuddy#shows_tips()))
  unlet g:flowbuddy_tips
endfunction

function! Test_a_tip_is_answered_once() abort
  " Not connected: shown and closed without errors, and nothing is left.
  call flowbuddy#on_daemon('{"type":"nudge","nudge_id":"n-1","rule_id":"r","title":"T","text":"x","ttl_ms":5000}')
  call flowbuddy#respond('dismiss')
  call assert_equal([{'type': 'nudge_response', 'nudge_id': 'n-1', 'action': 'dismiss'}], flowbuddy#answers())
  call assert_equal('Flow Buddy: no tip to answer', trim(execute('call flowbuddy#respond("dismiss")')))
  call assert_true(empty(popup_list()))
endfunction

function! Test_practice_counts_keys() abort
  let drill = tempname()
  let out = tempname()
  call writefile([json_encode({'version': 1, 'family': 'vertical', 'title': 'T', 'about': 'a',
        \ 'relative_numbers': v:true, 'rounds': [
        \ {'lines': ['a b', 'c d', 'e f', 'g h', 'i j', 'k l'], 'cursor': [1, 1], 'ask': 'x',
        \  'goal': {'line': 6}, 'mark': [6, 1, 0], 'par': 2, 'best': '5j'},
        \ {'lines': ['alpha bright, cedar'], 'cursor': [1, 1], 'ask': 'x',
        \  'goal': {'text': ['alpha , cedar']}, 'mark': [1, 7, 6], 'par': 3, 'best': 'dt,'}]})], drill)
  call flowbuddy#practice#run(drill, out, 0)
  sleep 20m
  call feedkeys('jjj2j', 'xt')
  call flowbuddy#practice#check()
  call assert_equal([5, 1], [flowbuddy#practice#state().results[0].keys, flowbuddy#practice#state().round])
  sleep 1200m
  " t counts the character it takes.
  call feedkeys('wdt,', 'xt')
  call flowbuddy#practice#check()
  let got = json_decode(readfile(out)[0])
  call assert_equal([5, 4], map(copy(got.rounds), 'v:val.keys'))
  call assert_equal('false', json_encode(got.stopped))
  call popup_clear()
  bwipe!
endfunction

" Runner
let s:failed = 0
for s:name in sort(filter(map(split(execute('function /^Test_'), "\n"), {_, l -> matchstr(l, 'Test_\w\+')}), '!empty(v:val)'))
  let v:errors = []
  try
    call call(s:name, [])
  catch
    call add(v:errors, v:throwpoint . ': ' . v:exception)
  endtry
  if empty(v:errors)
    call writefile(['ok   ' . s:name], $OUT, 'a')
  else
    let s:failed += 1
    call writefile(['FAIL ' . s:name] + map(copy(v:errors), '"     " . v:val'), $OUT, 'a')
  endif
endfor
call writefile([s:failed ? 'FAILED' : 'PASSED'], $OUT, 'a')
qa!
