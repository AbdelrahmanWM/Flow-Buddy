// What the settings window shows, worked out from `flowbuddy config list
// --json` and the tools' JSON: no GTK here, so it can be tested on its own.
// Every change goes through the `flowbuddy` command, which checks it.

// Where `flowbuddy` may be when it isn't on the session's PATH.
export const CLI_PLACES = ['.local/bin/flowbuddy', '.cargo/bin/flowbuddy'];

// The command to run: on the PATH, else where the installer puts it.
export function cliPath(onPath, exists, home) {
    if (onPath)
        return onPath;
    for (const place of CLI_PLACES) {
        const path = `${home}/${place}`;
        if (exists(path))
            return path;
    }
    return null;
}

// Short names for the settings shown first; the rest are named from
// their key.
const LABELS = {
    'coaching.default_mode': 'Coaching mode',
    'coaching.when': 'When tips appear',
    'coaching.quiet_hours': 'Quiet hours',
    'coaching.nudge_seconds': 'Seconds a tip stays',
    'learn.daily': 'Something new every day',
    'learn.level': 'Your level',
    'learn.challenges': 'Challenges after lessons',
    'learn.holdout': 'Tips held back to measure',
    'text.shapes': 'Exact tips from the text',
    'text.ai_snippets': 'Snippets for your AI',
    'text.cloud': 'Snippets to a cloud AI',
    'typing.accuracy': 'Typing accuracy',
    'record.keep_days': 'Days of recording kept',
};

// `coaching.quiet_hours` → "Quiet hours".
export function label(setting) {
    if (LABELS[setting.key])
        return LABELS[setting.key];
    const name = setting.key.split('.').pop().replace(/_/g, ' ');
    return name.charAt(0).toUpperCase() + name.slice(1);
}

// The basics, the other groups (in order), and the rules with their two
// settings each.
export function arrange(settings) {
    const basic = [];
    const groups = [];
    const rules = new Map();
    for (const s of settings) {
        if (s.rule) {
            const r = rules.get(s.rule.id) ?? {...s.rule, enabled: null, threshold: null};
            r[s.key.endsWith('.enabled') ? 'enabled' : 'threshold'] = s;
            rules.set(s.rule.id, r);
            continue;
        }
        if (s.basic) {
            basic.push(s);
            continue;
        }
        let g = groups.find(([name]) => name === s.group);
        if (!g) {
            g = [s.group, []];
            groups.push(g);
        }
        g[1].push(s);
    }
    return {basic, groups, rules: [...rules.values()]};
}

// Which control: a switch, a number, a choice or a text entry.
export function rowKind(setting) {
    switch (setting.kind.type) {
    case 'bool':
        return 'switch';
    case 'int':
    case 'float':
        return 'spin';
    case 'choice':
        return 'combo';
    default:
        return 'entry';
    }
}

// The value as the control holds it, from the setting (or its default).
export function current(setting) {
    // A rule not set in the file: on, at its own threshold.
    if (setting.rule && setting.value === null)
        return setting.key.endsWith('.enabled') ? true : (setting.rule.default_threshold ?? 1);
    const v = setting.value ?? setting.default;
    switch (rowKind(setting)) {
    case 'switch':
        return v === true;
    case 'spin':
        return typeof v === 'number' ? v : (setting.kind.min ?? 0);
    case 'combo':
        // A choice not set yet: the first entry, "Default".
        return setting.value === null ? -1 : setting.kind.choices.indexOf(v);
    default:
        return Array.isArray(v) ? v.join(', ') : (v ?? '');
    }
}

// Choices as shown, with "Default" first for one that may be left unset.
export function choiceLabels(setting) {
    const words = setting.kind.choices.map(c => c.replace(/-/g, ' '));
    return setting.default === null ? ['Default', ...words] : words;
}

// The arguments to `flowbuddy` for the control's new value: `config set`,
// or `config reset` for an empty entry or "Default".
export function command(setting, ui) {
    const key = setting.key;
    switch (rowKind(setting)) {
    case 'switch':
        return ['config', 'set', key, ui ? 'true' : 'false'];
    case 'spin':
        return ['config', 'set', key, setting.kind.type === 'int' ? String(Math.round(ui)) : String(ui)];
    case 'combo': {
        // `ui` is the index in choiceLabels.
        const offset = setting.default === null ? 1 : 0;
        if (ui < offset)
            return ['config', 'reset', key];
        return ['config', 'set', key, setting.kind.choices[ui - offset]];
    }
    default: {
        const text = String(ui).trim();
        return text === '' ? ['config', 'reset', key] : ['config', 'set', key, text];
    }
    }
}

// Whether to say what this means before doing it: a privacy notice, and
// the change turns the setting on.
export function needsConfirm(setting, ui) {
    if (!setting.notice)
        return false;
    switch (rowKind(setting)) {
    case 'switch':
        return ui === true;
    case 'spin':
        return ui > 0;
    default:
        return String(ui).trim() !== '';
    }
}

// Steps for a number control.
export function spinSteps(setting) {
    const {min, max, type} = setting.kind;
    if (type === 'int')
        return {min, max, step: 1, digits: 0};
    return {min, max, step: (max - min) <= 1 ? 0.05 : 0.5, digits: 2};
}

// Tool results, as lines of text for the window.

export function doctorLines(json) {
    return json.checks.map(c => c.ok ? `✓ ${c.what}` : `✗ ${c.what}\n    ${c.fix ?? ''}`);
}

export function progressLines(json) {
    if (json.nothing_yet)
        return ['Nothing recorded yet: is the daemon running?'];
    const lines = [];
    if (json.latest)
        lines.push(`Latest: ${json.latest.text} (${json.latest.learned ? 'learned' : 'not used yet'})`, '');
    const names = {1: 'Basics', 2: 'Bigger moves', 3: 'Power tools', 4: 'Advanced'};
    for (const level of [1, 2, 3, 4]) {
        const items = json.items.filter(i => i.level === level);
        const known = items.filter(i => i.state === 'known').length;
        const ready = items.filter(i => i.state === 'ready').length;
        lines.push(`${names[level]}: you use ${known} of ${items.length}; ${ready} ready to learn`);
    }
    const next = json.items.find(i => i.state === 'ready');
    if (next)
        lines.push('', `Ready next: ${next.keys}: ${next.what}`);
    return lines;
}

export function coverageLines(json) {
    const lines = ['Vim\'s own command index, by mode:', ''];
    for (const [mode, t] of Object.entries(json.modes)) {
        const covered = t.coached + t.taught;
        lines.push(`${mode}: ${covered} taught (${t.coached} coached), ${t.starting} where you start, ${t.skipped} left out, ${t.missing} missing`);
    }
    return lines;
}

// Summary and the learning report are text already.
export function textLines(json) {
    if (json.nothing_yet)
        return ['Nothing recorded yet: is the daemon running?'];
    return json.text.trimEnd().split('\n');
}
