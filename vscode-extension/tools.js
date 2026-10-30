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
        lines.push(`Latest: ${json.latest.text}`, ...(json.latest.example ? [`Example: ${json.latest.example}`] : []), '');
    for (const level of [1, 2, 3, 4]) {
        const items = json.items.filter(i => i.level === level);
        const known = items.filter(i => i.state === 'known').length;
        const ready = items.filter(i => i.state === 'ready');
        lines.push(`Level ${level}: you use ${known} of ${items.length}${ready.length ? `; ready: ${ready.map(i => i.keys).join(', ')}` : ''}`);
    }
    return lines.join('\n');
}

const LEVELS = {1: 'Basics', 2: 'Bigger moves', 3: 'Power tools', 4: 'Advanced'};
const TILES = [
    ['known', 'You use it'],
    ['slipping', 'Slipping: not used lately'],
    ['learning', 'Learning'],
    ['ready', 'Ready to learn'],
    ['later', 'Later'],
];

function escapeHtml(s) {
    return String(s ?? '').replace(/[&<>"']/g, c => ({'&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'})[c]);
}

function tileTitle(i) {
    const used = i.uses ? `Used ${i.uses === 1 ? 'once' : `${i.uses} times`}${i.days_idle ? `, last ${i.days_idle === 1 ? 'yesterday' : `${i.days_idle} days ago`}` : ''}` : 'Not used yet';
    return `${i.keys}: ${i.what}\n${used}`;
}

// The skill map as a page: a tile per skill, coloured by where you are
// with it, from `learn --json`. Styles only, no scripts.
function mapHtml(json) {
    const tile = (cls, text, title) => `<span class="tile ${cls}"${title ? ` title="${escapeHtml(title)}"` : ''}>${escapeHtml(text)}</span>`;
    const body = [];
    body.push('<h1>Your Vim skills</h1>');
    body.push(`<p class="legend">${TILES.map(([cls, text]) => tile(cls, text)).join(' ')}</p>`);
    if (json.nothing_yet) {
        body.push('<p>Nothing recorded yet: is the daemon running?</p>');
    } else {
        for (const level of [1, 2, 3, 4]) {
            const items = json.items.filter(i => i.level === level);
            if (!items.length)
                continue;
            const state = i => i.tile ?? (i.state === 'known' ? 'known' : i.state === 'ready' ? 'ready' : 'later');
            const known = items.filter(i => state(i) === 'known').length;
            body.push(`<h2>${LEVELS[level]} <small>you use ${known} of ${items.length}</small></h2>`);
            body.push(`<p>${items.map(i => tile(state(i), i.keys, tileTitle(i))).join(' ')}</p>`);
        }
    }
    return `<!DOCTYPE html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline';">
<style>
body { font-family: var(--vscode-font-family); color: var(--vscode-foreground); padding: 0 16px; }
p { line-height: 2.2; }
small { opacity: .7; font-weight: normal; }
.tile { padding: 3px 8px; border-radius: 5px; font-family: var(--vscode-editor-font-family); white-space: nowrap; }
.known { background: #2ec27e; color: #000; }
.slipping { background: #f6d32d; color: #000; }
.learning { background: #99c1f1; color: #000; }
.ready { border: 1px solid currentColor; }
.later { opacity: .45; border: 1px dashed currentColor; }
</style></head><body>${body.join('\n')}</body></html>`;
}

// The Command Palette's commands. `vscode` and `run` (run flowbuddy with
// arguments, resolving to {ok, out, err}) are passed in.
// `cli`: where the flowbuddy command is, for what runs in a terminal.
function register(vscode, run, subscriptions, cli = () => null) {
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
            const pick = await vscode.window.showQuickPick(options.map(([label]) => label), {title: s.key, placeHolder: s.more ?? s.what});
            const chosen = options.find(([label]) => label === pick);
            if (chosen)
                await apply(s, chosen[1]);
            return;
        }
        const text = await vscode.window.showInputBox({
            title: s.key,
            prompt: `${s.more ?? s.what}${s.kind.type === 'list' ? ' (comma-separated)' : ''} Empty puts the default back.`,
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

    async function profile() {
        const r = await run(['config', 'list', '--json']);
        if (!r.ok)
            throw new Error(r.err.trim());
        const profiles = JSON.parse(r.out).profiles ?? [];
        const pick = await vscode.window.showQuickPick(profiles.map(p => ({
            label: p.name,
            description: p.current ? 'in use' : '',
            detail: p.what,
            profile: p,
        })), {title: 'Flow Buddy: start from a profile'});
        if (!pick || pick.profile.current)
            return;
        const changes = pick.profile.changes.map(c => `${c.key}: ${c.now || 'default'} → ${c.then}`).join('\n');
        const yes = await vscode.window.showInformationMessage(`Use ${pick.label}? It changes:\n${changes}`, {modal: true}, 'Use it');
        if (yes === 'Use it')
            await apply({notice: null}, ['config', 'profile', pick.profile.id]);
    }

    const tool = (title, args, toText) => async () => {
        const r = await run(args);
        // `doctor` fails when a check does, and still says which.
        say(title, toText(args.includes('--json') && r.out ? JSON.parse(r.out) : null, r));
    };

    const commands = {
        'flowbuddy.settings': settings,
        'flowbuddy.profile': profile,
        'flowbuddy.checkSetup': tool('Check setup', ['doctor', '--json'], doctorText),
        'flowbuddy.progress': tool('Progress', ['learn', '--json'], progressText),
        'flowbuddy.today': tool('Today', ['summary', '--json'], j => j.nothing_yet ? 'Nothing recorded yet.' : j.text),
        'flowbuddy.recentTips': tool('Recent tips', ['tips'], (_j, r) => r.out),
        // The drill runs in Vim, so in a terminal: vscodevim can't run it.
        'flowbuddy.practice': async () => {
            const path = cli();
            if (!path)
                throw new Error('the flowbuddy command isn\'t installed: run scripts/install.sh from the Flow Buddy folder');
            const term = vscode.window.createTerminal({name: 'Flow Buddy practice', shellPath: path, shellArgs: ['practice']});
            term.show();
        },
        'flowbuddy.skillMap': async () => {
            const r = await run(['learn', '--json']);
            if (!r.ok)
                throw new Error(r.err.trim());
            const panel = vscode.window.createWebviewPanel('flowbuddy.skillMap', 'Flow Buddy: skills',
                vscode.ViewColumn.Active, {enableScripts: false});
            panel.webview.html = mapHtml(JSON.parse(r.out));
        },
    };
    for (const [id, fn] of Object.entries(commands))
        subscriptions.push(vscode.commands.registerCommand(id, () => fn().catch(failed)));
}

module.exports = {cliPath, settingItems, choices, typed, needsConfirm, doctorText, progressText, mapHtml, register};
