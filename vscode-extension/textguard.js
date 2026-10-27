// What the daemon allows with your text ([text] in its config), and the
// files never looked at, mirroring crates/core/src/text.rs. Paths are only
// checked here; they never leave the extension.

'use strict';

// `*` is any run of characters, `?` one.
function wildcard(pattern, text) {
    const re = pattern.replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*').replace(/\?/g, '.');
    return new RegExp(`^${re}$`).test(text);
}

// No `/` in the pattern matches the file name; otherwise the end of the path.
function globMatch(pattern, path) {
    if (!pattern.includes('/'))
        return wildcard(pattern, path.slice(path.lastIndexOf('/') + 1));
    const p = pattern.replace(/^\/+/, '');
    return wildcard(`*/${p}`, path) || wildcard(p, path);
}

// Off until the daemon says otherwise.
const OFF = {shapes: false, snippets: false, exclude: [], projects: []};

// Whether a document's text may be looked at for shapes, or snippets too:
// a file on disk (not an untitled buffer, output panel or terminal), not
// excluded, and for snippets inside one of your projects.
function textAllowed(config, scheme, path, snippets) {
    const c = config ?? OFF;
    if (!(snippets ? c.snippets : c.shapes))
        return false;
    if (scheme !== 'file' || !path?.startsWith('/'))
        return false;
    if ((c.exclude ?? []).some(p => globMatch(p, path)))
        return false;
    if (snippets)
        return (c.projects ?? []).some(p => p && path.startsWith(`${p}/`));
    return true;
}

const SNIPPETS = 20;
const SNIPPET_MS = 30 * 60 * 1000;

// With [text] ai_snippets on: lines around recent moves and edits, for
// `flowbuddy coach`. In memory only, and only handed over when the daemon
// asks for the moments of recent tips.
class SnippetRing {
    constructor() {
        this._kept = [];
    }

    // Up to five lines around `line` of `lines`, each cut to 200 characters.
    add(atMs, filetype, lines, line) {
        // By index: `lines` may be a lazy view of the document.
        const kept = [];
        for (let i = Math.max(0, line - 2); i <= line + 2 && i < lines.length; i++)
            kept.push([...lines[i]].slice(0, 200).join(''));
        this._kept.push({at_ms: atMs, filetype, lines: kept});
        this._kept = this._kept.filter(s => atMs - s.at_ms <= SNIPPET_MS).slice(-SNIPPETS);
    }

    clear() {
        this._kept = [];
    }

    // The kept snippet closest to each of `times`, within 3 s.
    pick(times) {
        const out = [];
        for (const t of times) {
            let best = null;
            for (const s of this._kept) {
                const d = Math.abs(s.at_ms - t);
                if (d <= 3000 && (!best || d < Math.abs(best.at_ms - t)))
                    best = s;
            }
            if (best && !out.includes(best))
                out.push(best);
        }
        return out;
    }
}

if (typeof module !== 'undefined')
    module.exports = {globMatch, textAllowed, OFF, SnippetRing};
