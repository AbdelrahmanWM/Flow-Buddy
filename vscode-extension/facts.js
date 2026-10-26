// Facts about where a cursor movement ended (`MotionFacts` in
// crates/core/src/editor.rs), computed from the buffer text.
//
// Pure: no vscode import, so the tests run it under gjs. It takes the lines
// as strings and positions as {line, col} (0-based, UTF-16 columns, which
// is what VS Code reports) and returns numbers and flags only.
//
// The motions follow Vim's own code (fwd_word, end_word, findpar, searchc,
// findmatch, find_ident_under_cursor). When a fact would depend on
// something this can't model, such as the word class of a character
// outside Latin-1, the fact is left out rather than guessed.

'use strict';

const MAX_HOPS = 500;
const BRACKETS = {'(': ')', '[': ']', '{': '}', ')': '(', ']': '[', '}': '{'};
const OPENERS = '([{';

// Thrown when a fact can't be computed exactly; the fact is then omitted.
const UNKNOWN = Symbol('unknown');

function attempt(fn) {
    try {
        return fn();
    } catch (e) {
        if (e === UNKNOWN)
            return undefined;
        throw e;
    }
}

// Vim's cls(): 0 blank (or end of line), 1 other, 2 keyword
// (default 'iskeyword': @,48-57,_,192-255). WORDs make every non-blank 1.
function charClass(line, col, big) {
    if (col >= line.length)
        return 0;
    const c = line.charCodeAt(col);
    if (c === 0x20 || c === 0x09 || c === 0xa0)
        return 0;
    if (c > 0xff)
        throw UNKNOWN;
    if (big)
        return 1;
    const keyword = (c >= 0x30 && c <= 0x39) || (c >= 0x41 && c <= 0x5a) ||
        (c >= 0x61 && c <= 0x7a) || c === 0x5f || c >= 0xc0;
    return keyword ? 2 : 1;
}

function isKeyword(line, col) {
    return col >= 0 && charClass(line, col, false) === 2;
}

function cmp(a, b) {
    return a.line === b.line ? a.col - b.col : a.line - b.line;
}

// A cursor that walks the buffer like Vim's inc()/dec(): it can stand on
// the end of a line (the NUL), which counts as blank.
class Cursor {
    constructor(lines, pos) {
        this.lines = lines;
        this.line = pos.line;
        this.col = pos.col;
    }

    get text() {
        return this.lines[this.line];
    }

    cls(big) {
        return charClass(this.text, this.col, big);
    }

    onEmptyLine() {
        return this.col === 0 && this.text.length === 0;
    }

    // 0 moved within the line, 2 moved onto the end of the line, 1 moved to
    // the next line, -1 at the end of the buffer.
    inc() {
        const len = this.text.length;
        if (this.col < len) {
            this.col++;
            return this.col < len ? 0 : 2;
        }
        if (this.line < this.lines.length - 1) {
            this.line++;
            this.col = 0;
            return 1;
        }
        return -1;
    }

    dec() {
        if (this.col > 0) {
            this.col--;
            return 0;
        }
        if (this.line > 0) {
            this.line--;
            this.col = this.text.length;
            return 1;
        }
        return -1;
    }

    // skip_chars(): true when the buffer ran out.
    skip(cls, forward, big) {
        while (this.cls(big) === cls) {
            if ((forward ? this.inc() : this.dec()) === -1)
                return true;
        }
        return false;
    }
}

// One press of `w`/`W` (fwd_word). Returns false when the motion fails.
function wordForward(c, big) {
    const sclass = c.cls(big);
    const lastLine = c.line === c.lines.length - 1;
    const i = c.inc();
    if (i === -1 || (i >= 1 && lastLine))
        return false;
    let done = false;
    if (sclass !== 0) {
        while (c.cls(big) === sclass) {
            if (c.inc() === -1) {
                done = true;
                break;
            }
        }
    }
    while (!done && c.cls(big) === 0) {
        if (c.onEmptyLine())
            break;
        if (c.inc() === -1)
            break;
    }
    // Normal mode doesn't leave the cursor past the end of the line.
    if (c.col > 0 && c.col >= c.text.length)
        c.col = c.text.length - 1;
    return true;
}

// One press of `b`/`B` (bck_word).
function wordBackward(c, big) {
    if (c.dec() === -1)
        return false;
    while (c.cls(big) === 0) {
        if (c.onEmptyLine())
            return true;
        if (c.dec() === -1)
            return true;
    }
    if (c.skip(c.cls(big), false, big))
        return true;
    c.inc();
    return true;
}

// One press of `e`/`E` (end_word).
function wordEndForward(c, big) {
    const sclass = c.cls(big);
    if (c.inc() === -1)
        return false;
    if (c.cls(big) === sclass && sclass !== 0) {
        if (c.skip(sclass, true, big))
            return false;
    } else {
        while (c.cls(big) === 0) {
            if (c.inc() === -1)
                return false;
        }
        if (c.skip(c.cls(big), true, big))
            return false;
    }
    c.dec();
    return true;
}

// One press of `ge`/`gE` (bckend_word).
function wordEndBackward(c, big) {
    const sclass = c.cls(big);
    if (c.dec() === -1)
        return false;
    if (sclass !== 0) {
        while (c.cls(big) === sclass) {
            if (c.dec() === -1)
                return true;
        }
    }
    while (c.cls(big) === 0) {
        if (c.onEmptyLine())
            break;
        if (c.dec() === -1)
            break;
    }
    return true;
}

// How many presses of `step` from `from` land exactly on `to`, or
// undefined if they skip past it, stop moving or fail first.
function countPresses(lines, from, to, dir, step) {
    const c = new Cursor(lines, from);
    for (let n = 1; n <= MAX_HOPS; n++) {
        const before = {line: c.line, col: c.col};
        if (!step(c))
            return undefined;
        const here = {line: c.line, col: c.col};
        if (cmp(here, before) * dir <= 0)
            return undefined;
        const d = cmp(here, to) * dir;
        if (d === 0)
            return n;
        if (d > 0)
            return undefined;
    }
    return undefined;
}

// One press of `}` (dir 1) or `{` (dir -1), Vim's findpar(): skip to a
// non-empty line, then stop at the next empty one (or one starting with a
// form feed). Returns where the cursor lands.
function paragraphStep(lines, line, dir) {
    let curr = line;
    let didSkip = false;
    for (let first = true; ; first = false) {
        const text = lines[curr];
        if (text.length > 0)
            didSkip = true;
        if (!first && didSkip && (text.length === 0 || text[0] === '\f'))
            break;
        curr += dir;
        if (curr < 0 || curr >= lines.length) {
            curr -= dir;
            break;
        }
    }
    const last = lines.length - 1;
    const col = dir > 0 && curr === last ? Math.max(lines[curr].length - 1, 0) : 0;
    return {line: curr, col};
}

function paragraphHops(lines, start, end, dir) {
    let line = start.line;
    for (let n = 1; n <= MAX_HOPS; n++) {
        const p = paragraphStep(lines, line, dir);
        if ((p.line - line) * dir <= 0)
            return undefined;
        if (p.line === end.line)
            return end.col === null || p.col === end.col ? n : undefined;
        if ((p.line - end.line) * dir > 0)
            return undefined;
        line = p.line;
    }
    return undefined;
}

function isSurrogate(code) {
    return code >= 0xd800 && code <= 0xdfff;
}

// `f<c>` then `;` (or `F<c>`), where <c> is the character at the end.
function findHops(text, from, to, dir) {
    if (to >= text.length || isSurrogate(text.charCodeAt(to)))
        return undefined;
    const ch = text[to];
    let n = 0;
    for (let k = from + dir; k * dir <= to * dir; k += dir) {
        if (text[k] === ch)
            n++;
    }
    return n || undefined;
}

// `t<c>` then `;` (or `T<c>`), where <c> is just past the end. Like Vim
// without 'cpo-;', a `;` right before <c> skips to the next one.
function tillHops(text, from, to, dir) {
    const target = to + dir;
    if (target < 0 || target >= text.length || isSurrogate(text.charCodeAt(target)))
        return undefined;
    const ch = text[target];
    let p = from;
    for (let n = 1; n <= MAX_HOPS; n++) {
        let k = p + dir * (n === 1 ? 1 : 2);
        while (k >= 0 && k < text.length && text[k] !== ch)
            k += dir;
        if (k < 0 || k >= text.length)
            return undefined;
        p = k - dir;
        if (p === to)
            return n;
        if ((p - to) * dir > 0)
            return undefined;
    }
    return undefined;
}

function firstNonBlank(text) {
    const i = text.search(/[^ \t]/);
    return i === -1 ? Math.max(text.length - 1, 0) : i;
}

// Double-quoted spans on a line, as [start, end] index pairs, or null if
// the quotes don't pair up (Vim then can't tell either).
function quoteSpans(text) {
    const spans = [];
    let open = -1;
    for (let i = 0; i < text.length; i++) {
        if (text[i] === '\\') {
            i++;
        } else if (text[i] === '"') {
            if (open === -1) {
                open = i;
            } else {
                spans.push([open, i]);
                open = -1;
            }
        }
    }
    return open === -1 ? spans : null;
}

// Whether `%` from the start lands on the end. Like Vim, it uses the first
// bracket at or after the cursor on the line, and ignores brackets in
// double quotes and char literals such as '('. Gives up (false) on things
// `%` treats specially: C comments, preprocessor lines, unpaired quotes.
function bracketMatch(lines, start, end) {
    const text = lines[start.line];
    let k = start.col;
    while (k < text.length && !(text[k] in BRACKETS))
        k++;
    if (k >= text.length)
        return false;
    const between = text.slice(start.col, k + 1);
    if (between.includes('/*') || between.includes('*/') || /^\s*#/.test(text))
        return false;
    const open = text[k];
    const close = BRACKETS[open];
    const dir = OPENERS.includes(open) ? 1 : -1;
    const spans = quoteSpans(text);
    if (spans === null || spans.some(([a, b]) => k > a && k < b))
        return false;

    let depth = 0;
    let line = start.line;
    let col = k;
    let lineSpans = spans;
    for (;;) {
        const t = lines[line];
        const quoted = lineSpans.some(([a, b]) => col > a && col < b);
        const literal = t[col - 1] === '\'' && t[col + 1] === '\'';
        if (!quoted && !literal) {
            if (t[col] === open) {
                depth++;
            } else if (t[col] === close && --depth === 0) {
                return line === end.line && col === end.col;
            }
        }
        col += dir;
        while (col < 0 || col >= lines[line].length) {
            line += dir;
            if (line < 0 || line >= lines.length || (line - end.line) * dir > 0)
                return false;
            lineSpans = quoteSpans(lines[line]);
            if (lineSpans === null)
                return false;
            col = dir > 0 ? 0 : lines[line].length - 1;
        }
    }
}

// `*` (dir 1) or `#` (dir -1): how many whole-word occurrences of the
// keyword under or after the cursor lie between the start and the end,
// counting the end. No wrapping around the file.
function starHops(lines, start, end, dir, ignorecase) {
    const text = lines[start.line];
    let a = start.col;
    while (a < text.length && !isKeyword(text, a))
        a++;
    if (a >= text.length)
        return undefined;
    while (a > 0 && isKeyword(text, a - 1))
        a--;
    let b = a;
    while (isKeyword(text, b))
        b++;
    const fold = s => (ignorecase ? s.toLowerCase() : s);
    const word = fold(text.slice(a, b));
    const from = {line: start.line, col: a};

    let n = 0;
    for (let line = start.line; line >= 0 && line < lines.length; line += dir) {
        const t = lines[line];
        const hay = fold(t);
        // Lowercasing some characters changes the length, and the columns.
        if (hay.length !== t.length)
            throw UNKNOWN;
        const found = [];
        for (let i = hay.indexOf(word); i !== -1; i = hay.indexOf(word, i + 1)) {
            if (!isKeyword(t, i - 1) && !isKeyword(t, i + word.length))
                found.push(i);
        }
        if (dir < 0)
            found.reverse();
        for (const col of found) {
            const p = {line, col};
            if (cmp(p, from) * dir <= 0)
                continue;
            n++;
            const d = cmp(p, end) * dir;
            if (d === 0)
                return n;
            if (d > 0)
                return undefined;
        }
        if ((line - end.line) * dir >= 0)
            return undefined;
    }
    return undefined;
}

function screenPos(window, line) {
    const count = window.bottom - window.top + 1;
    if (line === window.top)
        return 'top';
    if (line === window.bottom)
        return 'bottom';
    // Vim's `M`: the first line at or past half the window's rows.
    if (line === window.top + Math.floor((count + 1) / 2) - 1)
        return 'middle';
    return undefined;
}

/**
 * Facts about a movement from `start` to `end`.
 *
 * @param {string[]} lines the buffer, one string per line
 * @param {{line: number, col: number|null}} start where the cursor was
 * @param {{line: number, col: number|null}} end where it rested; a null
 *   column (Visual Line mode) leaves out every within-line fact
 * @param {{window?: {top: number, bottom: number}|null, ignorecase?: boolean,
 *   relativeNumbers?: boolean}} opts the visible lines (one range, no folds),
 *   Vim's 'ignorecase' for `*`, and whether relative line numbers are shown
 * @returns {object} only the MotionFacts fields that are known
 */
function computeFacts(lines, start, end, opts = {}) {
    const facts = {};
    const set = (key, value) => {
        if (value !== undefined && value !== false)
            facts[key] = value;
    };
    const window = opts.window ?? null;
    if (window && window.bottom >= window.top)
        facts.window_lines = window.bottom - window.top + 1;
    set('relative_numbers', !!opts.relativeNumbers);

    const vdir = Math.sign(end.line - start.line);
    if (vdir !== 0) {
        set('paragraph_hops', attempt(() => paragraphHops(lines, start, end, vdir)));
        if (window && window.bottom >= window.top)
            set('screen', screenPos(window, end.line));
        set('file_start', end.line === 0);
        set('file_end', end.line === lines.length - 1);
    }

    if (start.col !== null && end.col !== null && cmp(start, end) !== 0) {
        const dir = Math.sign(cmp(end, start));
        set('bracket_match', attempt(() => bracketMatch(lines, start, end)));
        set('star_hops', attempt(() => starHops(lines, start, end, dir, !!opts.ignorecase)));
    }

    if (end.col === null || start.col === null)
        return facts;
    const text = lines[end.line];
    const from = Math.min(start.col, Math.max(text.length - 1, 0));
    const hdir = Math.sign(end.col - from);
    if (hdir === 0)
        return facts;
    const origin = {line: end.line, col: from};
    const presses = step => attempt(() => countPresses(lines, origin, end, hdir, step));
    if (hdir > 0) {
        set('word_hops', presses(c => wordForward(c, false)));
        set('word_end_hops', presses(c => wordEndForward(c, false)));
        set('bigword_hops', presses(c => wordForward(c, true)));
    } else {
        set('word_hops', presses(c => wordBackward(c, false)));
        set('word_end_hops', presses(c => wordEndBackward(c, false)));
        set('bigword_hops', presses(c => wordBackward(c, true)));
    }
    set('find_hops', findHops(text, from, end.col, hdir));
    set('till_hops', tillHops(text, from, end.col, hdir));
    set('line_start', end.col === 0);
    set('first_nonblank', end.col === firstNonBlank(text));
    set('line_end', end.col === Math.max(text.length - 1, 0));
    return facts;
}

// Text objects an operator's range was exactly, mirroring
// vim-plugin/autoload/flowbuddy.vim: char_objects and line_objects.

const PAIRS = [['(', ')'], ['[', ']'], ['{', '}'], ['<', '>']];
const QUOTES = ['"', "'", '`'];
const KEYWORD = /^[A-Za-z0-9_\u00C0-\u00FF]$/;

function isKeywordChar(c) {
    return KEYWORD.test(c);
}

function edgeOrSpace(c) {
    return c === '' || /^\s$/.test(c);
}

function balanced(text, open, close) {
    let depth = 0;
    for (const ch of text) {
        depth += ch === open ? 1 : ch === close ? -1 : 0;
        if (depth < 0)
            return false;
    }
    return depth === 0;
}

/**
 * Text objects a charwise range is exactly, best first.
 *
 * @param {string} text what the range covered (lines joined by "\n")
 * @param {string} before the character just before it, '' at a line start
 * @param {string} after the character just after it, '' at a line end
 */
function charObjects(text, before, after) {
    const found = [];
    if (!text)
        return found;
    const word = /^[A-Za-z0-9_\u00C0-\u00FF]+$/;
    if (word.test(text) && !isKeywordChar(before) && !isKeywordChar(after))
        found.push('iw');
    if (/^[A-Za-z0-9_\u00C0-\u00FF]+\s+$/.test(text) && !isKeywordChar(before) && !/^\s$/.test(after))
        found.push('aw');
    if (/^\S+$/.test(text) && edgeOrSpace(before) && edgeOrSpace(after))
        found.push('iW');
    if (/^\S+\s+$/.test(text) && edgeOrSpace(before) && !/^\s$/.test(after))
        found.push('aW');
    for (const [open, close] of PAIRS) {
        if (before === open && after === close && balanced(text, open, close))
            found.push(`i${open}`);
        if (text.length >= 2 && text[0] === open && text[text.length - 1] === close &&
            balanced(text.slice(1, -1), open, close))
            found.push(`a${open}`);
    }
    if (!text.includes('\n')) {
        for (const q of QUOTES) {
            if (before === q && after === q && !text.includes(q))
                found.push(`i${q}`);
            if (text.length >= 2 && text[0] === q && text[text.length - 1] === q &&
                !text.slice(1, -1).includes(q))
                found.push(`a${q}`);
        }
    }
    return found;
}

/**
 * Text objects whole lines are, given whether the lines around them are
 * blank (or missing).
 */
function lineObjects(lines, blankBefore, blankAfter) {
    const filled = lines.filter(l => /\S/.test(l)).length;
    if (!blankBefore || filled === 0)
        return [];
    if (filled === lines.length && blankAfter)
        return ['ip'];
    const textEnd = lines.findIndex(l => !/\S/.test(l));
    if (textEnd > 0 && filled === textEnd && !blankAfter)
        return ['ap'];
    return [];
}

// How long rests on a line count towards `revisits`.
const REST_HISTORY_MS = 600000;

/**
 * Count earlier rests on `line` in the last ten minutes, then remember
 * this one. `rests` stays in the extension; only the count is sent.
 *
 * @param {{line: number, ms: number}[]} rests updated in place
 */
function rested(rests, line, now) {
    while (rests.length && now - rests[0].ms >= REST_HISTORY_MS)
        rests.shift();
    const visits = rests.filter(r => r.line === line).length;
    rests.push({line, ms: now});
    if (rests.length > 100)
        rests.shift();
    return visits;
}

// Character classes for w and e within a line: 0 blank, 1 keyword, 2 other.
function lineClass(c) {
    return /^\s$/.test(c) ? 0 : isKeywordChar(c) ? 1 : 2;
}

/**
 * Facts about a charwise range on one line of `text`, from its first
 * character (index `first`) to its last (`last`), as an operator's motion
 * could have covered it, mirroring flowbuddy#range_facts in the Vim
 * plugin: e presses to the last character, w presses to the one after it
 * (past the line's end counts too), f and t presses, and whether it ran to
 * the end of the line. Numbers only.
 */
function rangeFacts(text, first, last) {
    const chars = [...text];
    const n = chars.length;
    if (first < 0 || last < first || last >= n)
        return null;
    const cls = chars.map(lineClass);
    const idx = [...Array(n).keys()];
    const starts = idx.filter(j => cls[j] !== 0 && (j === 0 || cls[j - 1] !== cls[j]));
    const ends = idx.filter(j => cls[j] !== 0 && (j === n - 1 || cls[j + 1] !== cls[j]));
    const facts = {};
    if (ends.includes(last)) {
        const k = ends.filter(j => j > first && j <= last).length;
        if (k)
            facts.word_end_hops = k;
    }
    const after = last + 1;
    let words = 0;
    if (after >= n)
        words = starts.filter(j => j > first).length + 1;
    else if (starts.includes(after))
        words = starts.filter(j => j > first && j <= after).length;
    const count = (from, to, ch) => idx.filter(j => j >= from && j <= to && chars[j] === ch).length;
    const k = count(first + 1, last, chars[last]);
    if (k)
        facts.find_hops = k;
    if (after < n) {
        const t = count(first + 2, after, chars[after]);
        if (t)
            facts.till_hops = t;
    }
    facts.line_end = after >= n;
    const out = {cols: last - first, facts};
    if (words)
        out.words = words;
    return out;
}

if (typeof module !== 'undefined') {
    module.exports = {
        computeFacts,
        rangeFacts,
        rested,
        charObjects,
        lineObjects,
        // For the tests.
        wordForward, wordBackward, wordEndForward, wordEndBackward, Cursor,
    };
}
