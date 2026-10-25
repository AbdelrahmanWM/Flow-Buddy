// Top-bar indicator: shows nudges briefly and offers quick controls.
//
// A nudge appears as a short label next to the icon and fades after its
// ttl. The menu holds the full tip and lets you dismiss it, mute that
// rule, mark it as not helpful, switch mode or pause.

import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import St from 'gi://St';
import Clutter from 'gi://Clutter';

import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

const MODES = [
    ['off', 'Off'],
    ['passive', 'Passive: summaries only'],
    ['balanced', 'Balanced: a few tips now and then'],
    ['active', 'Active: tips as you work'],
];

export const Indicator = GObject.registerClass(
class FlowBuddyIndicator extends PanelMenu.Button {
    _init(link) {
        super._init(0.0, 'Flow Buddy');
        this._link = link;
        this._nudge = null;
        this._hideId = 0;

        const box = new St.BoxLayout({style_class: 'panel-status-menu-box'});
        this._icon = new St.Icon({
            icon_name: 'input-keyboard-symbolic',
            style_class: 'system-status-icon',
        });
        this._label = new St.Label({
            text: '',
            y_align: Clutter.ActorAlign.CENTER,
            visible: false,
        });
        box.add_child(this._icon);
        box.add_child(this._label);
        this.add_child(box);

        this._tip = new PopupMenu.PopupMenuItem('No tips yet', {reactive: false});
        this._tip.label.clutter_text.line_wrap = true;
        this.menu.addMenuItem(this._tip);

        this._gotIt = this._action('Got it', 'dismiss');
        this._mute = this._action('Stop this tip', 'mute_rule');
        this._notHelpful = this._action('Not helpful', 'false_positive');
        this._setNudgeActions(false);

        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this._modeItems = new Map();
        for (const [mode, label] of MODES) {
            const item = new PopupMenu.PopupMenuItem(label);
            item.connect('activate', () => this._command('set_mode', {mode}));
            this._modeItems.set(mode, item);
            this.menu.addMenuItem(item);
        }

        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this._pause = new PopupMenu.PopupMenuItem('Pause for 1 hour');
        this._pause.connect('activate', () => {
            if (this._paused)
                this._command('resume');
            else
                this._command('pause', {seconds: 3600});
        });
        this.menu.addMenuItem(this._pause);

        this.setConnected(false);
    }

    _action(label, action) {
        const item = new PopupMenu.PopupMenuItem(label);
        item.connect('activate', () => this._respond(action));
        this.menu.addMenuItem(item);
        return item;
    }

    _setNudgeActions(visible) {
        for (const item of [this._gotIt, this._mute, this._notHelpful])
            item.visible = visible;
    }

    _command(name, args) {
        this._link.send(args ? {type: 'command', name, args} : {type: 'command', name});
    }

    _respond(action) {
        if (!this._nudge)
            return;
        this._link.send({type: 'nudge_response', nudge_id: this._nudge.nudge_id, action});
        this._clearNudge();
    }

    setConnected(connected) {
        this._icon.opacity = connected ? 255 : 100;
        this.reactive = connected;
        if (!connected) {
            this._clearNudge();
            this._tip.label.text = 'Flow Buddy daemon is not running';
        }
    }

    setMode(mode, pausedUntil) {
        this._paused = pausedUntil !== null && pausedUntil !== undefined;
        for (const [m, item] of this._modeItems) {
            item.setOrnament(m === mode
                ? PopupMenu.Ornament.DOT : PopupMenu.Ornament.NONE);
        }
        this._pause.label.text = this._paused ? 'Resume' : 'Pause for 1 hour';
        this._icon.opacity = mode === 'off' || this._paused ? 120 : 255;
        if (!this._nudge)
            this._tip.label.text = this._paused ? 'Paused' : 'No tips right now';
    }

    showNudge(nudge) {
        if (this._nudge)
            this._link.send({type: 'nudge_response', nudge_id: this._nudge.nudge_id, action: 'seen'});
        this._nudge = nudge;
        this._label.text = ` ${nudge.title}`;
        this._label.visible = true;
        this._tip.label.text = nudge.text;
        this._setNudgeActions(true);
        if (this._hideId)
            GLib.source_remove(this._hideId);
        this._hideId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, nudge.ttl_ms, () => {
            this._hideId = 0;
            // The label goes away; the tip stays in the menu until handled.
            this._label.visible = false;
            return GLib.SOURCE_REMOVE;
        });
    }

    _clearNudge() {
        if (this._hideId) {
            GLib.source_remove(this._hideId);
            this._hideId = 0;
        }
        this._nudge = null;
        this._label.visible = false;
        this._setNudgeActions(false);
        this._tip.label.text = 'No tips right now';
    }

    destroy() {
        if (this._hideId)
            GLib.source_remove(this._hideId);
        this._hideId = 0;
        super.destroy();
    }
});
