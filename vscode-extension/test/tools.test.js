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
    ['flowbuddy.checkSetup', 'flowbuddy.profile', 'flowbuddy.progress', 'flowbuddy.recentTips', 'flowbuddy.settings', 'flowbuddy.today']);

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

print(`${passed} passed, ${failed} failed`);
if (failed)
    System.exit(1);
