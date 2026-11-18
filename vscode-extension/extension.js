// Flow Buddy companion for VS Code.
//
// Tells the local flowbuddyd daemon whether a text editor has focus (so
// typing in the terminal or a chat panel isn't coached as Vim), vscodevim's
// real mode, how the cursor moved in Normal and Visual mode, and which text
// objects a delete or change covered. Only flags, numbers and names are
// sent, never text, file names or positions.

'use strict';

const {DaemonLink} = require('./daemon');
const {FocusReporter} = require('./focus');
const {ModeTracker} = require('./mode');
const {MotionTracker} = require('./motion');
const {OperatorTracker} = require('./operator');
const {UiReporter} = require('./ui');
const {MappingReporter} = require('./mappings');
const {version} = require('./package.json');
const tools = require('./tools');
const tips = require('./tips');

let link = null;

// With no daemon and no service to run one (an editor-only install),
// start one that reads no keyboard. It runs on its own, serves the next
// window too, and exits ten minutes after the last editor closes. Off
// with the setting flowbuddy.startDaemon.
function startDaemon() {
    const vscode = require('vscode');
    const fs = require('fs');
    const os = require('os');
    const exists = p => {
        try {
            fs.accessSync(p, fs.constants.X_OK);
            return true;
        } catch {
            return false;
        }
    };
    const config = process.env.XDG_CONFIG_HOME || `${os.homedir()}/.config`;
    const socket = process.env.FLOWBUDDY_SOCKET || `/run/user/${process.getuid()}/flowbuddy.sock`;
    const bin = tools.daemonToStart({
        enabled: vscode.workspace.getConfiguration('flowbuddy').get('startDaemon', true),
        socketExists: fs.existsSync(socket),
        serviceExists: fs.existsSync(`${config}/systemd/user/flowbuddyd.service`),
        pathVar: process.env.PATH,
        home: os.homedir(),
        exists,
    });
    if (!bin)
        return false;
    try {
        require('child_process').spawn(bin, ['--editor-only', '--exit-when-idle'],
            {detached: true, stdio: 'ignore'}).unref();
        return true;
    } catch (e) {
        console.warn(`flowbuddy: couldn't start ${bin}: ${e}`);
        return false;
    }
}

function activate(context) {
    let focus = null;
    let modes = null;
    let motions = null;
    let operators = null;
    let mappings = null;
    let ui = null;
    let shown = null;
    link = new DaemonLink({
        version,
        onConnected: () => {
            focus?.resend();
            modes?.resend();
            mappings?.resend();
        },
        onNudge: nudge => shown?.show(nudge),
        // The desktop shows them, unless the daemon has no desktop to use.
        showsTips: () => link.editorOnly || tips.style(require('vscode')) !== 'desktop',
        startDaemon,
    });
    context.subscriptions.push(require('vscode').workspace.onDidChangeConfiguration(e => {
        if (e.affectsConfiguration('flowbuddy.tips'))
            link.hello();
    }));
    shown = tips.register(require('vscode'), link, context.subscriptions);
    focus = new FocusReporter(link);
    modes = new ModeTracker(link, (previous, mode) => {
        motions?.modeChanged();
        operators?.modeChanged(previous, mode);
        ui?.modeChanged(previous, mode);
    });
    motions = new MotionTracker(link, modes);
    operators = new OperatorTracker(link, modes);
    focus.enable(context.subscriptions);
    modes.enable(context.subscriptions);
    motions.enable(context.subscriptions);
    operators.enable(context.subscriptions);
    ui = new UiReporter(link, modes);
    ui.enable(context.subscriptions);
    mappings = new MappingReporter(link);
    mappings.enable(context.subscriptions);
    link.start();
    registerTools(context.subscriptions);
}

// Settings and tools in the Command Palette, through the flowbuddy command.
function registerTools(subscriptions) {
    const vscode = require('vscode');
    const fs = require('fs');
    const os = require('os');
    const {execFile} = require('child_process');
    const exists = path => {
        try {
            fs.accessSync(path, fs.constants.X_OK);
            return true;
        } catch {
            return false;
        }
    };
    const run = args => new Promise((resolve, reject) => {
        const cli = tools.cliPath(process.env.PATH, os.homedir(), exists);
        if (!cli) {
            reject(new Error('the flowbuddy command isn\'t installed: run scripts/install.sh from the Flow Buddy folder'));
            return;
        }
        execFile(cli, args, {timeout: 30000}, (error, out, err) => {
            // An exit code is an answer; anything else (not found, timed
            // out) is a failure to run.
            if (error && typeof error.code !== 'number') {
                reject(error);
                return;
            }
            resolve({ok: !error, out: out ?? '', err: err ?? ''});
        });
    });
    tools.register(vscode, run, subscriptions, () => tools.cliPath(process.env.PATH, os.homedir(), exists));
}

function deactivate() {
    link?.stop();
    link = null;
}

module.exports = {activate, deactivate};
