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
const {version} = require('./package.json');

let link = null;

function activate(context) {
    let focus = null;
    let modes = null;
    let motions = null;
    let operators = null;
    link = new DaemonLink({
        version,
        onConnected: () => {
            focus?.resend();
            modes?.resend();
        },
    });
    focus = new FocusReporter(link);
    modes = new ModeTracker(link, (previous, mode) => {
        motions?.modeChanged();
        operators?.modeChanged(previous, mode);
    });
    motions = new MotionTracker(link, modes);
    operators = new OperatorTracker(link, modes);
    focus.enable(context.subscriptions);
    modes.enable(context.subscriptions);
    motions.enable(context.subscriptions);
    operators.enable(context.subscriptions);
    new UiReporter(link).enable(context.subscriptions);
    link.start();
}

function deactivate() {
    link?.stop();
    link = null;
}

module.exports = {activate, deactivate};
