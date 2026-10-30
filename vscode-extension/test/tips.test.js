// Tests for tips.js. Run with gjs (no node needed):
//
//   gjs vscode-extension/test/tips.test.js

const GLib = imports.gi.GLib;
const System = imports.system;

function load() {
    const dir = GLib.path_get_dirname(GLib.path_get_dirname(System.programPath));
    const [, bytes] = GLib.file_get_contents(GLib.build_filenamev([dir, 'tips.js']));
    const module = {exports: {}};
    new Function('module', new TextDecoder().decode(bytes))(module);
    return module.exports;
}

const t = load();
let passed = 0;
let failed = 0;
function check(name, got, want) {
    const g = JSON.stringify(got);
    const w = JSON.stringify(want);
    if (g === w) {
        passed++;
    } else {
        failed++;
        print(`FAIL ${name}\n  got  ${g}\n  want ${w}`);
    }
}

const nudge = {nudge_id: 'n-1', title: 'Delete inside', text: 'That took 6 keys. `di(` does it from anywhere inside, in 3.',
    explain: ['`di(` delete inside the brackets'], ttl_ms: 6000};

check('plain', t.plain('`di(` and `` a`b ``'), 'di( and a`b');
check('status', t.statusText(nudge), '$(lightbulb) Delete inside: That took 6 keys. di( does it from anywhere inside, in 3.');
check('status cut', t.statusText(nudge, 30), '$(lightbulb) Delete inside: That took 6…');
check('tooltip', t.tooltip(nudge).split('\n'), ['**Delete inside**', '', nudge.text, '',
    '- `di(` delete inside the brackets', '', '[Got it](command:flowbuddy.gotIt) · [Not helpful](command:flowbuddy.notHelpful)']);

// One answer each: the next tip makes the last one seen; a timeout too.
const sent = [];
const views = [];
let pending = null;
const timers = {setTimeout: fn => (pending = fn), clearTimeout: () => (pending = null)};
const tips = new t.Tips(m => sent.push(`${m.nudge_id} ${m.action}`), {show: n => views.push(`show ${n.nudge_id}`), hide: () => views.push('hide')}, timers);
tips.show(nudge);
tips.show({...nudge, nudge_id: 'n-2'});
check('next tip: the last one seen', sent, ['n-1 seen']);
check('got it', tips.answer('dismiss'), true);
check('answered once', tips.answer('dismiss'), false);
check('answers', sent, ['n-1 seen', 'n-2 dismiss']);
tips.show({...nudge, nudge_id: 'n-3'});
pending();
check('timed out: seen', sent[sent.length - 1], 'n-3 seen');
check('views', views, ['show n-1', 'hide', 'show n-2', 'hide', 'show n-3', 'hide']);
tips.show({nudge_id: 7});
check('a bad tip is ignored', tips.current, null);
tips.show({...nudge, nudge_id: 'n-4'});
tips.hold();
check('held: no timeout', pending, null);

print(`${passed} passed, ${failed} failed`);
if (failed)
    System.exit(1);
