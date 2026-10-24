// Flow Buddy GNOME Shell extension.
//
// Tells the local flowbuddyd which app is focused. Talks only to the
// daemon's Unix socket; nothing leaves the machine.

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {DaemonLink} from './daemon.js';
import {FocusReporter} from './focus.js';

export default class FlowBuddyExtension extends Extension {
    enable() {
        this._link = new DaemonLink({
            version: this.metadata['version-name'] ?? '0',
            onMessage: msg => this._onMessage(msg),
            // Resend the current focus whenever the daemon (re)connects.
            onConnected: () => this._focus.report(),
        });
        this._focus = new FocusReporter(this._link);
        this._focus.enable();
        this._link.start();
    }

    disable() {
        this._focus?.disable();
        this._link?.stop();
        this._focus = null;
        this._link = null;
    }

    _onMessage(_msg) {
    }
}
