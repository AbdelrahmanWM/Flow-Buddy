// Tests for tools.js. Run with gjs (no node needed):
//
//   gjs vscode-extension/test/tools.test.js

const GLib = imports.gi.GLib;
const System = imports.system;

function load() {
    const dir = GLib.path_get_dirname(GLib.path_get_dirname(System.programPath));
    const [, bytes] = GLib.file_get_contents(GLib.build_filenamev([dir, 'tools.js']));
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

const daily = {key: 'learn.daily', kind: {type: 'bool'}, basic: true, what: 'One a day', notice: null, value: true, default: true};
const style = {key: 'coaching.nudge_style', kind: {type: 'choice', choices: ['bar', 'card']}, what: '', notice: null, value: null, default: null};
const quiet = {key: 'coaching.quiet_hours', kind: {type: 'time-range'}, what: '', notice: null, value: null, default: null};
const shapes = {key: 'text.shapes', kind: {type: 'bool'}, what: '', notice: 'reads text', value: false, default: false};

check('PATH first', t.cliPath('/a:/b', '/h', p => p === '/b/flowbuddy'), '/b/flowbuddy');
check('then ~/.local/bin', t.cliPath('/a', '/h', p => p === '/h/.local/bin/flowbuddy'), '/h/.local/bin/flowbuddy');
check('nowhere', t.cliPath('', '/h', () => false), null);
check('items', t.settingItems([daily, quiet]).map(i => [i.label, i.description]),
    [['learn.daily', 'true'], ['coaching.quiet_hours', '(not set)']]);
check('bool choices', t.choices(daily).map(([l, a]) => [l, a.join(' ')]),
    [['On', 'config set learn.daily true'], ['Off', 'config set learn.daily false']]);
check('unset choice offers Default', t.choices(style)[0], ['Default', ['config', 'reset', 'coaching.nudge_style']]);
check('typed', t.typed(quiet, ' 22:00-08:00 '), ['config', 'set', 'coaching.quiet_hours', '22:00-08:00']);
check('empty resets', t.typed(quiet, ''), ['config', 'reset', 'coaching.quiet_hours']);
check('confirm turning text on', [t.needsConfirm(shapes, ['config', 'set', 'text.shapes', 'true']),
    t.needsConfirm(shapes, ['config', 'set', 'text.shapes', 'false'])], [true, false]);

// The commands, against a fake VS Code: pick a basic setting, then a value.
const ran = [];
const shownOut = [];
const commands = {};
const picks = ['learn.daily', 'Off'];
const vscode = {
    window: {
        createOutputChannel: () => ({clear() {}, appendLine: l => shownOut.push(l), show() {}, dispose() {}}),
        showQuickPick: async items => {
            const want = picks.shift();
            return items.find(i => (i.label ?? i) === want);
        },
        showInformationMessage: () => {},
        showErrorMessage: m => print(`error: ${m}`),
        showWarningMessage: async () => undefined,
        showInputBox: async () => undefined,
    },
    commands: {registerCommand: (id, fn) => (commands[id] = fn, {dispose() {}})},
};
const run = async args => {
    ran.push(args.join(' '));
    if (args[0] === 'config' && args[1] === 'list')
        return {ok: true, out: JSON.stringify({version: 1, settings: [daily, quiet]}), err: ''};
    if (args[0] === 'doctor')
        return {ok: false, out: JSON.stringify({version: 1, ok: false, checks: [{ok: false, what: 'plugin', fix: 'install it'}]}), err: ''};
    return {ok: true, out: 'learn.daily = false', err: ''};
};
t.register(vscode, run, []);
check('commands', Object.keys(commands).sort(),
    ['flowbuddy.checkSetup', 'flowbuddy.practice', 'flowbuddy.profile', 'flowbuddy.progress', 'flowbuddy.recentTips', 'flowbuddy.settings', 'flowbuddy.skillMap', 'flowbuddy.today', 'flowbuddy.tour']);

let done = false;
(async () => {
    await commands['flowbuddy.settings']();
    await new Promise(r => GLib.timeout_add(GLib.PRIORITY_DEFAULT, 10, () => (r(), false)));
    check('settings ran config set', ran, ['config list --json', 'config set learn.daily false']);
    await commands['flowbuddy.checkSetup']();
    await new Promise(r => GLib.timeout_add(GLib.PRIORITY_DEFAULT, 10, () => (r(), false)));
    check('doctor shown even when it fails', shownOut.includes('✗ plugin\n    install it'), true);
    done = true;
})().catch(e => {
    failed++;
    print(`FAIL ${e}`);
    done = true;
});
const loop = new GLib.MainLoop(null, false);
GLib.timeout_add(GLib.PRIORITY_DEFAULT, 20, () => (done ? (loop.quit(), false) : true));
loop.run();

const html = t.mapHtml({items: [
    {level: 1, keys: 'w', what: 'words', tile: 'known', uses: 3, days_idle: 2},
    {level: 1, keys: '<C-d>', what: 'down "half"', tile: 'ready', uses: 0},
]});
check('map tile', html.includes('<span class="tile known" title="w: words\nUsed 3 times, last 2 days ago">w</span>'), true);
check('map escapes', html.includes('&lt;C-d&gt;') && html.includes('&quot;half&quot;') && !html.includes('<C-d>'), true);
check('map counts', html.includes('Basics <small>you use 1 of 2</small>'), true);
check('map: no scripts', /<script/i.test(html) || !html.includes("default-src 'none'"), false);
check('drills: the CLI\'s names', t.DRILLS.map(d => d.args.join('')).join(' '), ' vertical horizontal delete inside');
const has = (...paths) => p => paths.includes(p);
const start = over => t.daemonToStart({enabled: true, socketExists: false, serviceExists: false,
    pathVar: '/usr/bin:/opt/bin', home: '/home/me', exists: has('/opt/bin/flowbuddyd', '/home/me/.local/bin/flowbuddyd'), ...over});
check('daemon: on the PATH first', start({}), '/opt/bin/flowbuddyd');
check('daemon: else where the installer puts it', start({pathVar: '/usr/bin'}), '/home/me/.local/bin/flowbuddyd');
check('daemon: not when turned off', start({enabled: false}), null);
check('daemon: not when a socket is there', start({socketExists: true}), null);
check('daemon: not when the service exists', start({serviceExists: true}), null);
check('daemon: not when it isn\'t installed', start({exists: has()}), null);
check('daemon: the bundled one last', start({pathVar: '/usr/bin', exists: has('/ext/bin/flowbuddyd'), bundled: '/ext/bin'}), '/ext/bin/flowbuddyd');
check('daemon: an installed one before the bundled one', start({bundled: '/ext/bin', exists: has('/ext/bin/flowbuddyd', '/home/me/.local/bin/flowbuddyd')}), '/home/me/.local/bin/flowbuddyd');
check('cli: the bundled one', t.cliPath('/usr/bin', '/h', has('/ext/bin/flowbuddy'), '/ext/bin'), '/ext/bin/flowbuddy');
check('cli: ~/.cargo/bin', t.cliPath('', '/h', has('/h/.cargo/bin/flowbuddy')), '/h/.cargo/bin/flowbuddy');
check('daemon: ~/.cargo/bin', t.findProgram('flowbuddyd', '', '/h', has('/h/.cargo/bin/flowbuddyd')), '/h/.cargo/bin/flowbuddyd');
const missing = over => t.missingDaemon({socketExists: false, serviceExists: false, pathVar: '/usr/bin', home: '/h', exists: has(), ...over});
check('missing: nothing anywhere', missing({}), true);
check('missing: not with a socket', missing({socketExists: true}), false);
check('missing: not with the service', missing({serviceExists: true}), false);
check('missing: not with a bundled daemon', missing({bundled: '/ext/bin', exists: has('/ext/bin/flowbuddyd')}), false);
const sock = over => t.socketPath({env: {}, uid: 1000, home: '/home/me', isDir: () => false, ...over});
check('socket: FLOWBUDDY_SOCKET', sock({env: {FLOWBUDDY_SOCKET: '/x.sock', XDG_RUNTIME_DIR: '/r'}}), '/x.sock');
check('socket: XDG_RUNTIME_DIR', sock({env: {XDG_RUNTIME_DIR: '/r'}}), '/r/flowbuddy.sock');
check('socket: /run/user when it exists', sock({isDir: p => p === '/run/user/1000'}), '/run/user/1000/flowbuddy.sock');
check('socket: else the state folder (macOS)', sock({}), '/home/me/.local/state/flowbuddy/flowbuddy.sock');
check('socket: XDG_STATE_HOME', sock({env: {XDG_STATE_HOME: '/s'}}), '/s/flowbuddy/flowbuddy.sock');
check('map: nothing yet', t.mapHtml({nothing_yet: true}).includes('Nothing recorded yet'), true);

print(`${passed} passed, ${failed} failed`);
if (failed)
    System.exit(1);
