// Flow Buddy companion for VS Code.
//
// Tells the local flowbuddyd daemon whether a text editor has focus (so
// typing in the terminal or a chat panel isn't coached as Vim), vscodevim's
// real mode, how the cursor moved in Normal and Visual mode, and which text
// objects a delete or change covered. Only flags, numbers and names are
// sent, never text, file names or positions.

'use strict';

const {DaemonLink, socketPath} = require('./daemon');
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
// This extension's own copies of flowbuddyd and flowbuddy, in the
// Marketplace's Linux and macOS builds: <extension>/bin.
let bundled = null;

const INSTALL_URL = 'https://github.com/AbdelrahmanWM/Flow-Buddy#install';

function executable(path) {
    const fs = require('fs');
    try {
        fs.accessSync(path, fs.constants.X_OK);
        return true;
    } catch {
        return false;
    }
}

// The bundled programs, made runnable: unpacking a .vsix may drop the
// executable bit. Only files inside this extension's own folder.
function prepareBundled(extensionPath) {
    const fs = require('fs');
    const dir = require('path').join(extensionPath, 'bin');
    if (!fs.existsSync(dir))
        return null;
    for (const name of ['flowbuddyd', 'flowbuddy']) {
        const p = `${dir}/${name}`;
        try {
            if (fs.existsSync(p) && !executable(p))
                fs.chmodSync(p, 0o755);
        } catch (e) {
            console.warn(`flowbuddy: couldn't make ${p} runnable: ${e}`);
        }
    }
    return dir;
}

function daemonFacts() {
    const fs = require('fs');
    const os = require('os');
    const config = process.env.XDG_CONFIG_HOME || `${os.homedir()}/.config`;
    return {
        socketExists: fs.existsSync(socketPath()),
        serviceExists: fs.existsSync(`${config}/systemd/user/flowbuddyd.service`),
        pathVar: process.env.PATH,
        home: os.homedir(),
        exists: executable,
        bundled,
    };
}

// Nothing to talk to and nothing to start, a few seconds in: say why the
// extension does nothing, with a way to fix it, until told not to.
function checkDaemon(context) {
    const vscode = require('vscode');
    if (link?.connected || context.globalState.get('flowbuddy.missingDaemonHidden'))
        return;
    if (!tools.missingDaemon(daemonFacts()))
        return;
    const where = process.platform === 'linux' || process.platform === 'darwin'
        ? 'Flow Buddy needs its daemon, which isn\'t on this computer.'
        : 'Flow Buddy runs on Linux and macOS; it can\'t coach here yet.';
    vscode.window.showInformationMessage(where, 'How to install', 'Don\'t show again').then(choice => {
        if (choice === 'How to install')
            vscode.env.openExternal(vscode.Uri.parse(INSTALL_URL));
        else if (choice === 'Don\'t show again')
            context.globalState.update('flowbuddy.missingDaemonHidden', true);
    });
}

// With no daemon and no service to run one (an editor-only install),
// start one that reads no keyboard. It runs on its own, serves the next
// window too, and exits ten minutes after the last editor closes. Off
// with the setting flowbuddy.startDaemon.
function startDaemon() {
    const vscode = require('vscode');
    const bin = tools.daemonToStart({
        enabled: vscode.workspace.getConfiguration('flowbuddy').get('startDaemon', true),
        ...daemonFacts(),
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
    bundled = prepareBundled(context.extensionPath);
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
    const check = setTimeout(() => checkDaemon(context), 5000);
    context.subscriptions.push({dispose: () => clearTimeout(check)});
}

// Settings and tools in the Command Palette, through the flowbuddy command.
function registerTools(subscriptions) {
    const vscode = require('vscode');
    const os = require('os');
    const {execFile} = require('child_process');
    const cli = () => tools.cliPath(process.env.PATH, os.homedir(), executable, bundled);
    const run = args => new Promise((resolve, reject) => {
        const path = cli();
        if (!path) {
            reject(new Error(`the flowbuddy command isn't installed: see ${INSTALL_URL}`));
            return;
        }
        execFile(path, args, {timeout: 30000}, (error, out, err) => {
            // An exit code is an answer; anything else (not found, timed
            // out) is a failure to run.
            if (error && typeof error.code !== 'number') {
                reject(error);
                return;
            }
            resolve({ok: !error, out: out ?? '', err: err ?? ''});
        });
    });
    tools.register(vscode, run, subscriptions, cli);
}

function deactivate() {
    link?.stop();
    link = null;
}

module.exports = {activate, deactivate};
