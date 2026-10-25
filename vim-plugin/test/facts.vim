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

" Runner
let s:failed = 0
for s:name in sort(filter(map(split(execute('function /^Test_'), "\n"), {_, l -> matchstr(l, 'Test_\w\+')}), '!empty(v:val)'))
  let v:errors = []
  call call(s:name, [])
  if empty(v:errors)
    call writefile(['ok   ' . s:name], $OUT, 'a')
  else
    let s:failed += 1
    call writefile(['FAIL ' . s:name] + map(copy(v:errors), '"     " . v:val'), $OUT, 'a')
  endif
endfor
call writefile([s:failed ? 'FAILED' : 'PASSED'], $OUT, 'a')
qa!
