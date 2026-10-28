// Top-bar indicator: shows nudges briefly and offers quick controls.
//
// A nudge appears as a short label next to the icon (or as a card, see
// card.js) and fades after its ttl. A dot stays on the icon until the menu
// is opened. The menu holds the full tip and lets you dismiss it, mute
// that rule, mark it as not helpful, keep it on screen while you practise,
// look back at recent tips, switch mode or pause.

import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import St from 'gi://St';
import Clutter from 'gi://Clutter';

import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

import {toMarkup} from './markup.js';
import {Card} from './card.js';

// Recent tips listed in the menu.
const RECENT = 8;

// One line of a tip for the recent list: no markers, cut short.
function short(text, max = 70) {
    const plain = text.replace(/`+ ?([^`]*?) ?`+/g, '$1');
    return plain.length > max ? `${plain.slice(0, max - 1)}…` : plain;
}

const MODES = [
    ['off', 'Off'],
    ['passive', 'Passive: summaries only'],
    ['balanced', 'Balanced: a few tips now and then'],
    ['active', 'Active: tips as you work'],
];

export const Indicator = GObject.registerClass(
class FlowBuddyIndicator extends PanelMenu.Button {
    _init(link, openSettings) {
        super._init(0.0, 'Flow Buddy');
        this._link = link;
        this._nudge = null;
        // The tip shown in the menu: the latest nudge, or one picked from
        // the recent list.
        this._shownTip = null;
        this._recent = [];
        this._kept = new Card({kept: true, onClose: () => this._stopKeeping()});
        this._keptTip = null;
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
        this._dot = new St.Widget({
            style_class: 'flowbuddy-unread',
            y_align: Clutter.ActorAlign.CENTER,
            visible: false,
        });
        box.add_child(this._icon);
        box.add_child(this._dot);
        box.add_child(this._label);
        this.add_child(box);
        this.menu.connect('open-state-changed', (_menu, open) => {
            if (open)
                this._dot.visible = false;
        });

        this._tip = new PopupMenu.PopupMenuItem('No tips yet', {reactive: false});
        this._tip.label.clutter_text.line_wrap = true;
        this.menu.addMenuItem(this._tip);

        // What the tip's commands do: collapsed until you want it.
        this._explain = new PopupMenu.PopupSubMenuMenuItem('What does it do?');
        this._explain.visible = false;
        this.menu.addMenuItem(this._explain);

        this._keep = new PopupMenu.PopupMenuItem('Keep on screen');
        this._keep.connect('activate', () => this._toggleKeep());
        this._keep.visible = false;
        this.menu.addMenuItem(this._keep);
        // Whatever tip the menu shows, a kept one can always go.
        this._unkeep = new PopupMenu.PopupMenuItem('Stop keeping it on screen');
        this._unkeep.connect('activate', () => this._stopKeeping());
        this._unkeep.visible = false;
        this.menu.addMenuItem(this._unkeep);

        this._gotIt = this._action('Got it', 'dismiss');
        this._mute = this._action('Stop this tip', 'mute_rule');
        this._notHelpful = this._action('Not helpful', 'false_positive');
        // On the daily lesson instead of "Not helpful".
        this._knowIt = this._action('I know this', 'know_it');
        this._another = this._action('Show me another', 'another');
        this._setNudgeActions(false);

        this._recentMenu = new PopupMenu.PopupSubMenuMenuItem('Recent tips');
        this._recentMenu.visible = false;
        this.menu.addMenuItem(this._recentMenu);

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

        // Every setting, and the terminal's tools, in a window.
        const settings = new PopupMenu.PopupMenuItem('Settings…');
        settings.connect('activate', () => openSettings?.());
        this.menu.addMenuItem(settings);

        this.setConnected(false);
    }

    // Tips the daemon remembers, newest first (on connect).
    setRecent(tips) {
        this._recent = tips.slice(0, RECENT);
        this._fillRecent();
    }

    _fillRecent() {
        this._recentMenu.menu.removeAll();
        for (const tip of this._recent) {
            const item = new PopupMenu.PopupMenuItem(`${tip.title}: ${short(tip.text)}`);
            item.connect('activate', () => this._showTip(tip));
            this._recentMenu.menu.addMenuItem(item);
        }
        this._recentMenu.visible = this._recent.length > 0;
    }

    // Put a tip in the menu (and in the kept card, if one is kept).
    _showTip(tip) {
        this._shownTip = tip;
        this._tip.label.clutter_text.set_markup(toMarkup(tip.text));
        this._setExplanation(tip.explain ?? []);
        this._setNudgeActions(this._nudge !== null && tip === this._nudge);
        this._updateKeep();
    }

    // Keep the tip in the menu on screen, swap the kept one for it, or
    // stop keeping it.
    _toggleKeep() {
        if (this._kept.shown && (this._keptTip === this._shownTip || !this._shownTip)) {
            this._stopKeeping();
            return;
        } else if (this._shownTip) {
            this._kept.show(this._shownTip);
            this._keptTip = this._shownTip;
        }
        this._updateKeep();
    }

    _stopKeeping() {
        this._kept.hide();
        this._keptTip = null;
        this._updateKeep();
    }

    _updateKeep() {
        const kept = this._kept.shown;
        // Keep the tip in the menu, or swap the kept one for it.
        this._keep.visible = this._shownTip !== null && this._keptTip !== this._shownTip;
        this._keep.label.text = kept ? 'Keep this one on screen instead' : 'Keep on screen';
        this._unkeep.visible = kept;
    }

    _setExplanation(lines) {
        this._explain.menu.removeAll();
        this._explain.setSubmenuShown(false);
        for (const line of lines) {
            const item = new PopupMenu.PopupMenuItem('', {reactive: false});
            item.label.clutter_text.line_wrap = true;
            item.label.clutter_text.set_markup(toMarkup(line));
            this._explain.menu.addMenuItem(item);
        }
        this._explain.visible = lines.length > 0;
    }

    _action(label, action) {
        const item = new PopupMenu.PopupMenuItem(label);
        item.connect('activate', () => this._respond(action));
        this.menu.addMenuItem(item);
        return item;
    }

    _setNudgeActions(visible) {
        const lesson = visible && this._nudge?.rule_id === 'learn.daily';
        for (const item of [this._gotIt, this._mute])
            item.visible = visible;
        this._notHelpful.visible = visible && !lesson;
        this._knowIt.visible = lesson;
        this._another.visible = lesson;
    }

    _command(name, args) {
        this._link.send(args ? {type: 'command', name, args} : {type: 'command', name});
    }

    // For the shortcuts.
    keepTip() {
        this._toggleKeep();
    }

    respond(action) {
        this._respond(action);
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

    // `inBar`: also show the title next to the icon for the nudge's ttl.
    showNudge(nudge, inBar) {
        if (this._nudge)
            this._link.send({type: 'nudge_response', nudge_id: this._nudge.nudge_id, action: 'seen'});
        this._nudge = nudge;
        // A kept tip stays as it is: you're still practising it.
        this._shownTip = nudge;
        this._tip.label.clutter_text.set_markup(toMarkup(nudge.text));
        this._setExplanation(nudge.explain ?? []);
        this._setNudgeActions(true);
        this._updateKeep();
        this._recent = [nudge, ...this._recent.filter(t => t.text !== nudge.text)].slice(0, RECENT);
        this._fillRecent();
        this._dot.visible = !this.menu.isOpen;
        if (!inBar)
            return;
        this._label.text = ` ${nudge.title}`;
        this._label.visible = true;
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
        this._dot.visible = false;
        this._setNudgeActions(false);
        this._shownTip = null;
        this._updateKeep();
        this._tip.label.text = 'No tips right now';
        this._setExplanation([]);
    }

    destroy() {
        if (this._hideId)
            GLib.source_remove(this._hideId);
        this._hideId = 0;
        this._kept.destroy();
        super.destroy();
    }
});
