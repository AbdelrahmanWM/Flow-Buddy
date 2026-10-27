// Flow Buddy GNOME Shell extension.
//
// Tells the local flowbuddyd which app is focused and shows its tips in
// the top bar. Talks only to the daemon's Unix socket; nothing leaves the
// machine.

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import {getInputSourceManager} from 'resource:///org/gnome/shell/ui/status/keyboard.js';

import {DaemonLink} from './daemon.js';
import {FocusReporter} from './focus.js';
import {Card} from './card.js';
import {Indicator} from './indicator.js';

// Our request for recent tips; its answer comes back with this id.
const RECENT_ID = 1;

export default class FlowBuddyExtension extends Extension {
    enable() {
        this._link = new DaemonLink({
            version: this.metadata['version-name'] ?? '0',
            onMessage: msg => this._onMessage(msg),
            onConnected: () => {
                this._indicator.setConnected(true);
                // Resend the current focus whenever the daemon (re)connects.
                this._focus.report();
                this._reportSource();
                // The menu's recent tips, from before this session too.
                this._link.send({type: 'command', id: RECENT_ID, name: 'recent_tips', args: {limit: 8}});
            },
            onDisconnected: () => this._indicator.setConnected(false),
        });
        this._indicator = new Indicator(this._link);
        Main.panel.addToStatusArea(this.uuid, this._indicator);
        this._card = new Card();
        this._focus = new FocusReporter(this._link);
        this._focus.enable();
        // Follow keyboard layout switches, so keys are read right.
        this._sources = getInputSourceManager();
        this._sourceId = this._sources.connect('current-source-changed', () => this._reportSource());
        this._link.start();
    }

    // xkb sources have ids like "de+nodeadkeys"; input methods (IBus) have
    // no layout of their own.
    _reportSource() {
        const source = this._sources?.currentSource;
        if (!source)
            return;
        const [layout, variant] = source.type === 'xkb' ? source.id.split('+') : [null, null];
        this._link.send({type: 'input_source', layout, variant: variant ?? null});
    }

    disable() {
        if (this._sourceId)
            this._sources.disconnect(this._sourceId);
        this._sources = null;
        this._sourceId = 0;
        this._focus?.disable();
        this._link?.stop();
        this._indicator?.destroy();
        this._card?.destroy();
        this._card = null;
        this._focus = null;
        this._link = null;
        this._indicator = null;
    }

    _onMessage(msg) {
        switch (msg.type) {
        case 'welcome':
            this._indicator.setMode(msg.mode, null);
            break;
        case 'result':
            if (msg.id === RECENT_ID && Array.isArray(msg.data))
                this._indicator.setRecent(msg.data);
            break;
        case 'mode_changed':
            this._indicator.setMode(msg.mode, msg.paused_until);
            break;
        case 'nudge':
            // `style` is newer than protocol v0; treat it as missing = bar.
            if (msg.style === 'card') {
                this._card.show(msg);
                this._indicator.showNudge(msg, false);
            } else {
                this._indicator.showNudge(msg, true);
            }
            break;
        }
    }
}
