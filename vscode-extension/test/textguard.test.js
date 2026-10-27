// Tests for textguard.js, mirroring crates/core/src/text.rs. Run with gjs:
//
//   gjs vscode-extension/test/textguard.test.js

const GLib = imports.gi.GLib;
const System = imports.system;

const dir = GLib.path_get_dirname(GLib.path_get_dirname(System.programPath));
const [, bytes] = GLib.file_get_contents(GLib.build_filenamev([dir, 'textguard.js']));
const module = {exports: {}};
new Function('module', new TextDecoder().decode(bytes))(module);
const {globMatch, textAllowed} = module.exports;

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

print(`${passed} passed, ${failed} failed`);
if (failed)
    System.exit(1);
