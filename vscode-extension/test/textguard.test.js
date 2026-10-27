// Tests for textguard.js, mirroring crates/core/src/text.rs. Run with gjs:
//
//   gjs vscode-extension/test/textguard.test.js

const GLib = imports.gi.GLib;
const System = imports.system;

const dir = GLib.path_get_dirname(GLib.path_get_dirname(System.programPath));
const [, bytes] = GLib.file_get_contents(GLib.build_filenamev([dir, 'textguard.js']));
const module = {exports: {}};
new Function('module', new TextDecoder().decode(bytes))(module);
const {globMatch, textAllowed, SnippetRing} = module.exports;

let failed = 0;
let passed = 0;
function ok(name, value) {
    if (value) {
        passed++;
    } else {
        failed++;
        print(`FAIL ${name}`);
    }
}

ok('*.pem', globMatch('*.pem', '/a/b/c.pem'));
ok('*.pem not a dir', !globMatch('*.pem', '/a/b.pem/c.txt'));
ok('.ssh/*', globMatch('.ssh/*', '/home/me/.ssh/known_hosts'));
ok('.ssh/* not ssh', !globMatch('.ssh/*', '/home/me/ssh/known_hosts'));
ok('.env.*', globMatch('.env.*', '/x/.env.local'));
ok('? is one', globMatch('?.txt', '/x/a.txt') && !globMatch('?.txt', '/x/ab.txt'));

const on = {shapes: true, snippets: true, exclude: ['.env', '*.pem'], projects: ['/home/me/app']};
ok('off by default', !textAllowed(null, 'file', '/home/me/app/a.js', false));
ok('shapes', textAllowed(on, 'file', '/home/me/app/a.js', false));
ok('snippets in a project', textAllowed(on, 'file', '/home/me/app/a.js', true));
ok('not outside it', !textAllowed(on, 'file', '/tmp/a.js', true));
ok('not a lookalike', !textAllowed(on, 'file', '/home/me/application/a.js', true));
ok('excluded', !textAllowed(on, 'file', '/home/me/app/.env', false));
ok('untitled', !textAllowed(on, 'untitled', 'Untitled-1', false));
ok('output panel', !textAllowed(on, 'output', '/x', false));
ok('snippets off', !textAllowed({...on, snippets: false}, 'file', '/home/me/app/a.js', true));

{
    const ring = new SnippetRing();
    const lines = ['a', 'b', 'c', 'd', 'e', 'f'];
    ring.add(1000, 'python', lines, 3);
    ring.add(9000, 'python', lines, 0);
    const got = ring.pick([1500, 50000]);
    ok('one snippet near 1500', got.length === 1);
    ok('five lines around it', JSON.stringify(got[0].lines) === JSON.stringify(['b', 'c', 'd', 'e', 'f']));
    ok('at the top', JSON.stringify(ring.pick([9000])[0].lines) === JSON.stringify(['a', 'b', 'c']));
    ring.clear();
    ok('cleared', ring.pick([1000]).length === 0);
    for (let i = 0; i < 30; i++)
        ring.add(i * 5000, null, lines, 0);
    ok('at most 20 kept', ring.pick([0]).length === 0 && ring.pick([29 * 5000]).length === 1);
}

print(`${passed} passed, ${failed} failed`);
if (failed)
    System.exit(1);
