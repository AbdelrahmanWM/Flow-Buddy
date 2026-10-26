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

if (typeof module !== 'undefined')
    module.exports = {globMatch, textAllowed, OFF};
