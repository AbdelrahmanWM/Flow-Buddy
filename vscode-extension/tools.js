// Flow Buddy's settings and tools in the Command Palette, for people not
// on GNOME. Everything runs the `flowbuddy` command, which checks every
// value: the same as `flowbuddy config set` in a terminal.

const PLACES = ['.local/bin/flowbuddy', '.cargo/bin/flowbuddy'];

// The `flowbuddy` command: on the PATH, else where the installer puts it.
function cliPath(pathVar, home, exists) {
    for (const dir of (pathVar || '').split(':').filter(Boolean)) {
        if (exists(`${dir}/flowbuddy`))
            return `${dir}/flowbuddy`;
    }
    for (const place of PLACES) {
        if (exists(`${home}/${place}`))
            return `${home}/${place}`;
    }
    return null;
}

// A value as you'd type it.
function shown(v) {
    if (v === null || v === undefined)
        return '';
    return Array.isArray(v) ? v.join(', ') : String(v);
}

// Quick-pick items for settings: what it is, its value, what it does.
function settingItems(settings) {
    return settings.map(s => ({
        label: s.rule ? `${s.rule.title}: ${s.key.endsWith('.enabled') ? 'gives tips' : 'repeats before a tip'}` : s.key,
        description: shown(s.value ?? s.default) || '(not set)',
        detail: s.what,
        setting: s,
    }));
}

// The values to pick from, for settings that have a few; null means type
// one. Each is [label, arguments to flowbuddy].
function choices(s) {
    const set = v => ['config', 'set', s.key, v];
    switch (s.kind.type) {
    case 'bool':
        return [['On', set('true')], ['Off', set('false')]];
    case 'choice': {
        const out = s.kind.choices.map(c => [c, set(c)]);
        if (s.default === null)
            out.unshift(['Default', ['config', 'reset', s.key]]);
        return out;
    }
    default:
        return null;
    }
}

// The arguments for a typed value: empty puts the default back.
function typed(s, text) {
    const t = text.trim();
    return t === '' ? ['config', 'reset', s.key] : ['config', 'set', s.key, t];
}

// Whether to say what this means first: it has a privacy notice and the
// change turns it on.
function needsConfirm(s, args) {
    if (!s.notice || args[1] !== 'set')
        return false;
    return !['false', '0', 'none', 'off'].includes(args[3].toLowerCase());
}

function doctorText(json) {
    return json.checks.map(c => c.ok ? `✓ ${c.what}` : `✗ ${c.what}\n    ${c.fix ?? ''}`).join('\n');
}

function progressText(json) {
    if (json.nothing_yet)
        return 'Nothing recorded yet: is the daemon running?';
    const lines = [];
    if (json.latest)
        lines.push(`Latest: ${json.latest.text}`, '');
    for (const level of [1, 2, 3, 4]) {
        const items = json.items.filter(i => i.level === level);
        const known = items.filter(i => i.state === 'known').length;
        const ready = items.filter(i => i.state === 'ready');
        lines.push(`Level ${level}: you use ${known} of ${items.length}${ready.length ? `; ready: ${ready.map(i => i.keys).join(', ')}` : ''}`);
    }
    return lines.join('\n');
}

// The Command Palette's commands. `vscode` and `run` (run flowbuddy with
// arguments, resolving to {ok, out, err}) are passed in.
function register(vscode, run, subscriptions) {
    const output = vscode.window.createOutputChannel('Flow Buddy');
    subscriptions.push(output);
    const say = (title, text) => {
        output.clear();
        output.appendLine(title);
        output.appendLine('');
        output.appendLine(text);
        output.show(true);
    };
    const failed = e => vscode.window.showErrorMessage(`Flow Buddy: ${e.message ?? e}`);

    async function apply(s, args) {
        if (needsConfirm(s, args)) {
            const yes = await vscode.window.showWarningMessage(s.notice, {modal: true}, 'Turn on');
            if (yes !== 'Turn on')
                return;
        }
        const r = await run(args);
        if (r.ok)
            vscode.window.showInformationMessage(r.out.trim().split('\n').pop());
        else
            vscode.window.showErrorMessage(r.err.trim().replace(/^flowbuddy: /, ''));
    }

    async function change(s) {
        const options = choices(s);
        if (options) {
            const pick = await vscode.window.showQuickPick(options.map(([label]) => label), {title: s.key, placeHolder: s.what});
            const chosen = options.find(([label]) => label === pick);
            if (chosen)
                await apply(s, chosen[1]);
            return;
        }
        const text = await vscode.window.showInputBox({
            title: s.key,
            prompt: `${s.what}${s.kind.type === 'list' ? ' (comma-separated)' : ''}. Empty puts the default back.`,
            value: shown(s.value),
        });
        if (text !== undefined)
            await apply(s, typed(s, text));
    }

    async function settings() {
        const r = await run(['config', 'list', '--json']);
        if (!r.ok)
            throw new Error(r.err.trim());
        const all = JSON.parse(r.out).settings;
        const advanced = {label: '$(settings-gear) Advanced settings…', detail: 'Everything else, and each rule'};
        let pick = await vscode.window.showQuickPick([...settingItems(all.filter(s => s.basic)), advanced],
            {title: 'Flow Buddy settings', matchOnDetail: true});
        if (pick === advanced) {
            pick = await vscode.window.showQuickPick(settingItems(all.filter(s => !s.basic)),
                {title: 'Flow Buddy: advanced settings', matchOnDetail: true, matchOnDescription: true});
        }
        if (pick?.setting)
            await change(pick.setting);
    }

    const tool = (title, args, toText) => async () => {
        const r = await run(args);
        // `doctor` fails when a check does, and still says which.
        say(title, toText(args.includes('--json') && r.out ? JSON.parse(r.out) : null, r));
    };

    const commands = {
        'flowbuddy.settings': settings,
        'flowbuddy.checkSetup': tool('Check setup', ['doctor', '--json'], doctorText),
        'flowbuddy.progress': tool('Progress', ['learn', '--json'], progressText),
        'flowbuddy.today': tool('Today', ['summary', '--json'], j => j.nothing_yet ? 'Nothing recorded yet.' : j.text),
        'flowbuddy.recentTips': tool('Recent tips', ['tips'], (_j, r) => r.out),
    };
    for (const [id, fn] of Object.entries(commands))
        subscriptions.push(vscode.commands.registerCommand(id, () => fn().catch(failed)));
}

module.exports = {cliPath, settingItems, choices, typed, needsConfirm, doctorText, progressText, register};
