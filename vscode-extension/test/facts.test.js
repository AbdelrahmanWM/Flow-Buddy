// Tests for facts.js. Run with gjs (no node needed):
//
//   gjs vscode-extension/test/facts.test.js
//
// Each buffer is small and the expected numbers are worked out by hand
// from Vim's behaviour.

const GLib = imports.gi.GLib;
const System = imports.system;

function load() {
    const dir = GLib.path_get_dirname(GLib.path_get_dirname(System.programPath));
    const [, bytes] = GLib.file_get_contents(GLib.build_filenamev([dir, 'facts.js']));
    const module = {exports: {}};
    new Function('module', new TextDecoder().decode(bytes))(module);
    return module.exports;
}

const {computeFacts, Cursor, wordForward, wordBackward, wordEndForward} = load();

// Every field of MotionFacts; the daemon rejects anything else.
const FIELDS = [
    'paragraph_hops', 'screen', 'window_lines', 'file_start', 'file_end',
    'word_hops', 'word_end_hops', 'bigword_hops', 'find_hops', 'till_hops',
    'line_start', 'first_nonblank', 'line_end', 'bracket_match', 'star_hops',
    'jump_back', 'search_hops', 'relative_numbers',
];

let failed = 0;
let passed = 0;

function canon(obj) {
    return JSON.stringify(Object.keys(obj).sort().map(k => [k, obj[k]]));
}

function check(name, actual, expected) {
    if (canon(actual) === canon(expected)) {
        passed++;
    } else {
        failed++;
        print(`FAIL ${name}\n  expected ${JSON.stringify(expected)}\n  got      ${JSON.stringify(actual)}`);
    }
}

// Facts from [line, col] to [line, col]; checks only `keys` when given.
function facts(name, lines, [sl, sc], [el, ec], opts, expected, keys) {
    const got = computeFacts(lines, {line: sl, col: sc}, {line: el, col: ec}, opts);
    for (const k of Object.keys(got)) {
        if (!FIELDS.includes(k))
            check(`${name}: unknown field ${k}`, {}, {[k]: got[k]});
    }
    if (keys) {
        const picked = {};
        for (const k of keys) {
            if (k in got)
                picked[k] = got[k];
        }
        check(name, picked, expected);
    } else {
        check(name, got, expected);
    }
}

function walk(name, lines, from, step, expected) {
    const c = new Cursor(lines, {line: from[0], col: from[1]});
    const got = [];
    for (let i = 0; i < expected.length; i++) {
        if (!step(c))
            break;
        got.push([c.line, c.col]);
    }
    check(name, {got}, {got: expected});
}

// Words: keyword runs, punctuation runs and WORDs.
//            0123456789012345
const words = ['foo bar.baz  qux'];
facts('w to baz', words, [0, 0], [0, 8], {},
    {word_hops: 3, find_hops: 2, till_hops: 2});
facts('w and W to qux', words, [0, 0], [0, 13], {},
    {word_hops: 4, bigword_hops: 2, find_hops: 1, till_hops: 1});
facts('w, e and W to the last char', words, [0, 0], [0, 15], {},
    {word_hops: 5, word_end_hops: 5, bigword_hops: 3, find_hops: 1, line_end: true});
facts('b and B back to bar', words, [0, 15], [0, 4], {},
    {word_hops: 4, bigword_hops: 2, find_hops: 2, till_hops: 3});

// Word motions across lines: an empty line is a word, blanks are skipped.
const gap = ['foo', '', '  bar'];
walk('w stops on the empty line', gap, [0, 0], c => wordForward(c, false), [[1, 0], [2, 2]]);
walk('b stops on the empty line', gap, [2, 2], c => wordBackward(c, false), [[1, 0], [0, 0]]);
walk('e skips the empty line', gap, [0, 0], c => wordEndForward(c, false), [[0, 2], [2, 4]]);

// Within-line facts start from the start column clamped on the end line.
const clamp = ['abcdefgh', 'xy z'];
facts('j onto a shorter line is } to the end of the file', clamp, [0, 6], [1, 3], {},
    {paragraph_hops: 1, file_end: true});
facts('then b to the line start', clamp, [0, 6], [1, 0], {},
    {word_hops: 1, bigword_hops: 1, find_hops: 1, line_start: true, first_nonblank: true, file_end: true});

//            01234567890123
const stmt = ['    let x = 1;'];
facts('b to the first non-blank', stmt, [0, 10], [0, 4], {},
    {word_hops: 2, bigword_hops: 2, find_hops: 1, till_hops: 3, first_nonblank: true});
facts('w to the line end', stmt, [0, 4], [0, 13], {},
    {word_hops: 4, word_end_hops: 5, bigword_hops: 4, find_hops: 1, line_end: true});
facts('t skips a char right next to the cursor on ;', ['axbxc'], [0, 0], [0, 2], {},
    {till_hops: 2}, ['till_hops']);

// Paragraphs and the window.
const paras = ['a', 'b', '', '', 'c', '', 'd'];
const win = {window: {top: 0, bottom: 6}};
facts('} once', paras, [0, 0], [2, 0], win, {window_lines: 7, paragraph_hops: 1});
facts('} twice skips a run of empty lines', paras, [0, 0], [5, 0], win,
    {window_lines: 7, paragraph_hops: 2});
facts('} to the last line', paras, [0, 0], [6, 0], win,
    {window_lines: 7, paragraph_hops: 3, screen: 'bottom', file_end: true});
facts('{ twice lands on M', paras, [6, 0], [3, 0], win,
    {window_lines: 7, paragraph_hops: 2, screen: 'middle'});
facts('{ to the first line', paras, [6, 0], [0, 0], win,
    {window_lines: 7, paragraph_hops: 3, screen: 'top', file_start: true});
facts('j onto a non-empty line is no paragraph', paras, [0, 0], [1, 0], {}, {});
facts('Visual Line mode has no columns', paras, [0, null], [2, null], {}, {paragraph_hops: 1});
facts('screen only inside the window', paras, [0, 0], [6, 0], {window: {top: 0, bottom: 3}},
    {window_lines: 4, paragraph_hops: 3, file_end: true});

// %: the first bracket at or after the cursor, same kind only.
//               0123456789012345
const parens = ['if (a(b) [c]) {', '  x', '}'];
facts('% across nested parens', parens, [0, 0], [0, 12], {}, {bracket_match: true}, ['bracket_match']);
facts('% not to the inner paren', parens, [0, 0], [0, 7], {}, {}, ['bracket_match']);
facts('% over lines', parens, [0, 14], [2, 0], {}, {bracket_match: true}, ['bracket_match']);
facts('% backwards', parens, [2, 0], [0, 14], {}, {bracket_match: true}, ['bracket_match']);
facts('% ignores brackets in strings', ['f("(", x)'], [0, 0], [0, 8], {},
    {bracket_match: true}, ['bracket_match']);
facts('% ignores char literals', ['f(\'(\', 1)'], [0, 0], [0, 8], {},
    {bracket_match: true}, ['bracket_match']);
facts('% gives up on preprocessor lines', ['#if (x)'], [0, 0], [0, 6], {}, {}, ['bracket_match']);

// * and #: whole words only, the word under the cursor is skipped.
const stars = ['foo bar', 'x foo foobar', 'foo'];
facts('* once', stars, [0, 1], [1, 2], {}, {star_hops: 1}, ['star_hops']);
facts('* skips foobar', stars, [0, 1], [2, 0], {}, {star_hops: 2}, ['star_hops']);
facts('# twice', stars, [2, 0], [0, 0], {}, {star_hops: 2}, ['star_hops']);
facts('* not onto a partial word', stars, [0, 0], [1, 6], {}, {}, ['star_hops']);
facts('# respects ignorecase', ['Foo foo'], [0, 4], [0, 0], {ignorecase: true},
    {star_hops: 1}, ['star_hops']);
facts('# is case sensitive by default', ['Foo foo'], [0, 4], [0, 0], {}, {}, ['star_hops']);

// Characters outside Latin-1 leave word facts out instead of guessing.
//            012345678901234
const intl = ['héllo wörld ☃ x'];
facts('Latin-1 letters are keyword chars', intl, [0, 0], [0, 6], {},
    {word_hops: 1}, ['word_hops']);
facts('a snowman makes word facts unknown', intl, [0, 6], [0, 14], {},
    {find_hops: 1}, ['word_hops', 'word_end_hops', 'bigword_hops', 'find_hops']);

// Relative line numbers let the daemon suggest counts you can read.
facts('relative numbers on', words, [0, 0], [0, 4], {relativeNumbers: true},
    {relative_numbers: true}, ['relative_numbers']);
facts('relative numbers off', words, [0, 0], [0, 4], {},
    {}, ['relative_numbers']);

// Nothing at all for a move that goes nowhere.
facts('no move', words, [0, 3], [0, 3], {}, {});

print(`${passed} passed, ${failed} failed`);
if (failed)
    System.exit(1);
