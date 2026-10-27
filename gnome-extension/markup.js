// Tip text marks commands with backticks ("Try `Ctrl-d` then `5j`"), the
// Markdown way: a run of backticks opens, the next run of the same length
// closes, and one space just inside is dropped (`` `a `` for a command with
// a backtick in it). Turn that into Pango markup with the commands in a
// monospace font. Mirrors crates/core/src/glossary.rs `segments`.

import GLib from 'gi://GLib';

function closing(body, n) {
    let from = 0;
    for (;;) {
        const at = body.indexOf('`', from);
        if (at < 0)
            return -1;
        let run = 0;
        while (body[at + run] === '`')
            run++;
        if (run === n)
            return at;
        from = at + run;
    }
}

export function segments(text) {
    const out = [];
    let rest = text;
    for (;;) {
        const open = rest.indexOf('`');
        if (open < 0)
            break;
        let n = 0;
        while (rest[open + n] === '`')
            n++;
        const body = rest.slice(open + n);
        const close = closing(body, n);
        if (close < 0)
            break;
        if (open > 0)
            out.push({code: false, text: rest.slice(0, open)});
        let inner = body.slice(0, close);
        if (inner.length > 2 && inner.startsWith(' ') && inner.endsWith(' '))
            inner = inner.slice(1, -1);
        out.push({code: true, text: inner});
        rest = body.slice(close + n);
    }
    if (rest)
        out.push({code: false, text: rest});
    return out;
}

const escape = s => GLib.markup_escape_text(s, -1);

export function toMarkup(text) {
    return segments(text)
        .map(s => s.code ? `<tt><b>${escape(s.text)}</b></tt>` : escape(s.text))
        .join('');
}

export function plain(text) {
    return segments(text).map(s => s.text).join('');
}
