// Tips in VS Code's status bar, where you're working, rather than on the
// desktop. The daemon sends a tip here when the keys came from this
// window; each one gets one answer back: seen, got it or not helpful.

'use strict';

const MAX_STATUS = 80;

// The tip's text without the markers around commands (`di(`, `` a`b ``).
function plain(text) {
    return (text ?? '').replace(/``\s?(.+?)\s?``|`([^`]+)`/g, (_m, a, b) => a ?? b);
}

// What the status bar shows: the title and as much of the text as fits.
function statusText(nudge, max = MAX_STATUS) {
    let text = `${nudge.title ?? 'Flow Buddy'}: ${plain(nudge.text)}`;
    if (text.length > max) {
        // At a space, unless that loses too much.
        const cut = text.slice(0, max - 1);
        const space = cut.lastIndexOf(' ');
        text = `${(space > max / 2 ? cut.slice(0, space) : cut).trimEnd()}…`;
    }
    return `$(lightbulb) ${text}`;
}

// The hover: the whole tip, what its commands do, and the two answers.
function tooltip(nudge) {
    const lines = [`**${nudge.title ?? 'Flow Buddy'}**`, '', nudge.text ?? ''];
    if (nudge.explain?.length)
        lines.push('', ...nudge.explain.map(e => `- ${e}`));
    lines.push('', '[Got it](command:flowbuddy.gotIt) · [Not helpful](command:flowbuddy.notHelpful)');
    return lines.join('\n');
}

// The tip on show and its one answer. `send` sends to the daemon; `view`
// is {show(nudge), hide()}; timers are passed in for the tests.
class Tips {
    constructor(send, view, timers = {setTimeout, clearTimeout}) {
        this._send = send;
        this._view = view;
        this._timers = timers;
        this._tip = null;
        this._timer = null;
    }

    get current() {
        return this._tip;
    }

    show(nudge) {
        if (typeof nudge?.nudge_id !== 'string')
            return;
        this.answer('seen');
        this._tip = nudge;
        this._view.show(nudge);
        this._timer = this._timers.setTimeout(() => this.answer('seen'), nudge.ttl_ms ?? 6000);
    }

    // Keep it up while you read it.
    hold() {
        this._timers.clearTimeout(this._timer);
        this._timer = null;
    }

    // Answer the tip on show, once, and take it down. False if there's none.
    answer(action) {
        if (!this._tip)
            return false;
        this._timers.clearTimeout(this._timer);
        this._timer = null;
        this._send({type: 'nudge_response', nudge_id: this._tip.nudge_id, action});
        this._tip = null;
        this._view.hide();
        return true;
    }
}

// How a tip shows: the desktop card (the default: tips don't come here at
// all), a notification at the bottom right with its answers, or only the
// status bar. For the last two the status bar keeps it, highlighted, for
// its time.
function style(vscode) {
    return vscode.workspace.getConfiguration('flowbuddy').get('tips', 'desktop');
}

// The notification's text: the title and the tip, without command marks.
function notificationText(nudge) {
    return `Flow Buddy · ${nudge.title ?? 'Tip'}: ${plain(nudge.text)}`;
}

// The status-bar item and the commands, in VS Code.
function register(vscode, link, subscriptions) {
    const item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 1000);
    item.name = 'Flow Buddy tip';
    item.command = 'flowbuddy.tipActions';
    item.backgroundColor = new vscode.ThemeColor('statusBarItem.warningBackground');
    subscriptions.push(item);
    let tips = null;
    // The answers on a notification; "What does it do?" shows the
    // explanation and its example, with the answers again.
    const ask = async (tip, text) => {
        const buttons = ['Got it', 'Not helpful'];
        if (tip.explain?.length && text === notificationText(tip))
            buttons.push('What does it do?');
        const pick = await vscode.window.showInformationMessage(text, ...buttons);
        if (tips.current !== tip)
            return;
        if (pick === 'Got it')
            tips.answer('dismiss');
        else if (pick === 'Not helpful')
            tips.answer('false_positive');
        else if (pick === 'What does it do?') {
            tips.hold();
            await ask(tip, `${plain(tip.text)} ${tip.explain.map(plain).join(' ')}`);
        }
    };
    tips = new Tips(msg => link.send(msg), {
        show(nudge) {
            item.text = statusText(nudge);
            const md = new vscode.MarkdownString(tooltip(nudge));
            md.isTrusted = {enabledCommands: ['flowbuddy.gotIt', 'flowbuddy.notHelpful']};
            item.tooltip = md;
            item.show();
            // "desktop" with a daemon that has no desktop: here instead.
            const how = style(vscode);
            if (how === 'notification' || (how === 'desktop' && link.editorOnly))
                ask(nudge, notificationText(nudge));
        },
        hide() {
            item.hide();
        },
    });
    const none = () => vscode.window.showInformationMessage('Flow Buddy: no tip to answer');
    const commands = {
        'flowbuddy.gotIt': () => tips.answer('dismiss') || none(),
        'flowbuddy.notHelpful': () => tips.answer('false_positive') || none(),
        // A click on the tip: the whole of it, with the answers.
        'flowbuddy.tipActions': async () => {
            const tip = tips.current;
            if (!tip)
                return;
            tips.hold();
            await ask(tip, `${plain(tip.text)} ${(tip.explain ?? []).map(plain).join(' ')}`.trim());
            if (tips.current === tip)
                tips.answer('seen');
        },
    };
    for (const [id, fn] of Object.entries(commands))
        subscriptions.push(vscode.commands.registerCommand(id, fn));
    return tips;
}

module.exports = {plain, statusText, tooltip, notificationText, style, Tips, register};
