// Flow Buddy GNOME Shell extension.
//
// Tells the local flowbuddyd which app is focused and shows its tips in
// the top bar. Talks only to the daemon's Unix socket; nothing leaves the
// machine.

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {DaemonLink} from './daemon.js';
import {FocusReporter} from './focus.js';
import {Indicator} from './indicator.js';

export default class FlowBuddyExtension extends Extension {
    enable() {
        this._link = new DaemonLink({
            version: this.metadata['version-name'] ?? '0',
            onMessage: msg => this._onMessage(msg),
            onConnected: () => {
                this._indicator.setConnected(true);
                // Resend the current focus whenever the daemon (re)connects.
                this._focus.report();
            },
            onDisconnected: () => this._indicator.setConnected(false),
        });
        this._indicator = new Indicator(this._link);
        Main.panel.addToStatusArea(this.uuid, this._indicator);
        this._focus = new FocusReporter(this._link);
        this._focus.enable();
        this._link.start();
    }

    disable() {
        this._focus?.disable();
        this._link?.stop();
        this._indicator?.destroy();
        this._focus = null;
        this._link = null;
        this._indicator = null;
    }

    _onMessage(msg) {
        switch (msg.type) {
        case 'welcome':
            this._indicator.setMode(msg.mode, null);
            break;
        case 'mode_changed':
            this._indicator.setMode(msg.mode, msg.paused_until);
            break;
        case 'nudge':
            this._indicator.showNudge(msg);
            break;
        }
    }
}
