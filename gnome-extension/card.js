// A nudge card at the top centre of the screen.
//
// It is never reactive, so clicks pass through to the window below, and it
// never takes keyboard focus: it can't interrupt typing. It fades out after
// the nudge's ttl. What the commands do is in the top-bar menu.

import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {toMarkup} from './markup.js';

const FADE_MS = 250;
const GAP_PX = 12;

export class Card {
    constructor() {
        this._hideId = 0;
        this._actor = new St.BoxLayout({
            style_class: 'flowbuddy-card',
            vertical: true,
            reactive: false,
            can_focus: false,
            opacity: 0,
            visible: false,
        });
        this._title = new St.Label({style_class: 'flowbuddy-card-title'});
        this._text = new St.Label({style_class: 'flowbuddy-card-text'});
        this._text.clutter_text.line_wrap = true;
        this._actor.add_child(this._title);
        this._actor.add_child(this._text);
        Main.layoutManager.uiGroup.add_child(this._actor);
    }

    show(nudge) {
        this._title.text = nudge.title;
        this._text.clutter_text.set_markup(toMarkup(nudge.text));
        this._place();
        this._actor.remove_all_transitions();
        this._actor.visible = true;
        this._actor.ease({
            opacity: 255,
            duration: FADE_MS,
            mode: Clutter.AnimationMode.EASE_OUT_QUAD,
        });
        if (this._hideId)
            GLib.source_remove(this._hideId);
        this._hideId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, nudge.ttl_ms, () => {
            this._hideId = 0;
            this.hide();
            return GLib.SOURCE_REMOVE;
        });
    }

    hide() {
        if (this._hideId) {
            GLib.source_remove(this._hideId);
            this._hideId = 0;
        }
        this._actor.ease({
            opacity: 0,
            duration: FADE_MS,
            mode: Clutter.AnimationMode.EASE_IN_QUAD,
            onComplete: () => {
                this._actor.visible = false;
            },
        });
    }

    _place() {
        const monitor = Main.layoutManager.primaryMonitor;
        if (!monitor)
            return;
        const [, width] = this._actor.get_preferred_width(-1);
        const top = Main.panel.visible ? Main.panel.height : 0;
        this._actor.set_position(
            Math.floor(monitor.x + (monitor.width - width) / 2),
            monitor.y + top + GAP_PX);
    }

    destroy() {
        if (this._hideId)
            GLib.source_remove(this._hideId);
        this._hideId = 0;
        this._actor.destroy();
    }
}
