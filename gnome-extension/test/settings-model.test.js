// Run with: gjs -m gnome-extension/test/settings-model.test.js
import GLib from 'gi://GLib';
import System from 'system';

const here = GLib.path_get_dirname(GLib.path_get_dirname(System.programPath));
const m = await import(`file://${here}/settings-model.js`);

let failed = 0;
const check = (name, got, want) => {
    const g = JSON.stringify(got);
    const w = JSON.stringify(want);
    if (g !== w) {
        failed++;
        print(`FAIL ${name}\n  got  ${g}\n  want ${w}`);
    }
};

const setting = (key, kind, extra = {}) => ({
    key, kind, group: 'Coaching', basic: false, what: '', notice: null,
    value: null, default: null, ...extra,
});
const daily = setting('learn.daily', {type: 'bool'}, {basic: true, group: 'Learning', value: true, default: true});
const holdout = setting('learn.holdout', {type: 'float', min: 0, max: 0.5}, {group: 'Learning', value: 0.1, default: 0.1});
const style = setting('coaching.nudge_style', {type: 'choice', choices: ['bar', 'card']});
const level = setting('learn.level', {type: 'choice', choices: ['beginner', 'intermediate', 'advanced']},
    {basic: true, value: 'beginner', default: 'beginner'});
const quiet = setting('coaching.quiet_hours', {type: 'time-range'}, {basic: true});
const terms = setting('apps.terminals', {type: 'list'}, {group: 'Keyboard and apps', value: ['kitty', 'foot']});
const shapes = setting('text.shapes', {type: 'bool'}, {group: 'Privacy and text', notice: 'reads text', value: false, default: false});
const jk = setting('rules.vim.repeat-jk.threshold', {type: 'int', min: 1, max: 100},
    {group: 'Rules', rule: {id: 'vim.repeat-jk', title: 'Move further at once', default_threshold: 5}});
const jkOn = setting('rules.vim.repeat-jk.enabled', {type: 'bool'},
    {group: 'Rules', rule: {id: 'vim.repeat-jk', title: 'Move further at once', default_threshold: 5}});

// Finding the command.
check('on the PATH', m.cliPath('/usr/bin/flowbuddy', () => false, '/home/a'), '/usr/bin/flowbuddy');
check('where the installer puts it', m.cliPath(null, p => p === '/home/a/.local/bin/flowbuddy', '/home/a'),
    '/home/a/.local/bin/flowbuddy');
check('nowhere', m.cliPath(null, () => false, '/home/a'), null);

// Arranging.
const a = m.arrange([daily, holdout, style, level, quiet, terms, shapes, jk, jkOn]);
check('basics first', a.basic.map(s => s.key), ['learn.daily', 'learn.level', 'coaching.quiet_hours']);
check('then groups in order', a.groups.map(([g]) => g), ['Learning', 'Coaching', 'Keyboard and apps', 'Privacy and text']);
check('rules with both settings', a.rules.map(r => [r.id, r.enabled.key, r.threshold.key]),
    [['vim.repeat-jk', 'rules.vim.repeat-jk.enabled', 'rules.vim.repeat-jk.threshold']]);
check('labels', [m.label(quiet), m.label(terms)], ['Quiet hours', 'Terminals']);

// Controls and the commands they run.
check('kinds', [daily, holdout, style, quiet].map(m.rowKind), ['switch', 'spin', 'combo', 'entry']);
check('current', [daily, holdout, style, terms, level].map(m.current), [true, 0.1, -1, 'kitty, foot', 0]);
check('a rule not set: on, at its threshold', [m.current(jkOn), m.current(jk)], [true, 5]);
check('unset choice offers Default', m.choiceLabels(style), ['Default', 'bar', 'card']);
check('switch', m.command(daily, false), ['config', 'set', 'learn.daily', 'false']);
check('float', m.command(holdout, 0.05), ['config', 'set', 'learn.holdout', '0.05']);
check('int rounded', m.command(jk, 7.6), ['config', 'set', 'rules.vim.repeat-jk.threshold', '8']);
check('choice', m.command(style, 2), ['config', 'set', 'coaching.nudge_style', 'card']);
check('choice back to default', m.command(style, 0), ['config', 'reset', 'coaching.nudge_style']);
check('choice with a default', m.command(level, 1), ['config', 'set', 'learn.level', 'intermediate']);
check('entry', m.command(quiet, ' 22:00-08:00 '), ['config', 'set', 'coaching.quiet_hours', '22:00-08:00']);
check('empty entry resets', m.command(quiet, ''), ['config', 'reset', 'coaching.quiet_hours']);
check('ask before turning text on', [m.needsConfirm(shapes, true), m.needsConfirm(shapes, false), m.needsConfirm(daily, true)],
    [true, false, false]);
check('steps', [m.spinSteps(holdout).step, m.spinSteps(jk).digits], [0.05, 0]);

// Profiles.
check('profile changes', m.profileChanges(
    {changes: [{key: 'coaching.quiet_hours', now: '', then: '22:00-08:00'}, {key: 'learn.daily', now: 'true', then: 'false'}]},
    [quiet, daily]),
['Quiet hours: default → 22:00-08:00', 'Something new every day: true → false']);

// Tool results.
check('doctor', m.doctorLines({checks: [{ok: true, what: 'daemon'}, {ok: false, what: 'plugin', fix: 'install it'}]}),
    ['✓ daemon', '✗ plugin\n    install it']);
const progress = m.progressLines({
    latest: null,
    items: [
        {level: 1, state: 'known', keys: 'w', what: 'words'},
        {level: 1, state: 'ready', keys: 'e', what: 'word ends'},
        {level: 2, state: 'later', keys: '}', what: 'paragraphs'},
    ],
});
check('progress', progress[0], 'Basics: you use 1 of 2; 1 ready to learn');
check('progress next', progress[progress.length - 1], 'Ready next: e: word ends');
check('nothing yet', m.textLines({nothing_yet: true}).length, 1);

// A drill in a terminal.
const found = names => n => (names.includes(n) ? `/usr/bin/${n}` : null);
check('desktop terminal', m.inTerminal(['fb', 'practice'], found(['xdg-terminal-exec', 'ptyxis'])), ['xdg-terminal-exec', 'fb', 'practice']);
check('ptyxis', m.inTerminal(['fb', 'practice'], found(['ptyxis'])), ['ptyxis', '--', 'fb', 'practice']);
check('no terminal', m.inTerminal(['fb'], found([])), null);
check('new labels', [m.label({key: 'coaching.show_in'}), m.label({key: 'learn.review_at'})],
    ['Where tips show', 'Review a skill when recall drops below (%)']);

// The skill map.
const groups = m.skillGroups({items: [
    {level: 1, keys: 'w', what: 'words', tile: 'known', uses: 40, days_idle: 1, recall: 90},
    {level: 1, keys: 'e', what: 'word ends', tile: 'ready', uses: 0, days_idle: null, recall: null},
    {level: 2, keys: '}', what: 'paragraphs', state: 'later', uses: 0},
]});
check('map groups', groups.map(g => [g.name, g.known, g.tiles.map(t => t.tile)]),
    [['Basics', 1, ['known', 'ready']], ['Bigger moves', 0, ['later']]]);
check('tile tooltip', groups[0].tiles[0].tooltip, 'w: words\nUsed 40 times, last yesterday\nAbout 90% likely you\'d remember it today');
check('unused tooltip', groups[0].tiles[1].tooltip, 'e: word ends\nNot used yet');
check('map: nothing yet', m.skillGroups({nothing_yet: true}), []);

print(failed ? `${failed} failed` : 'settings model ok');
if (failed)
    System.exit(1);
