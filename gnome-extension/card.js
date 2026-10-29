// A nudge card at the top centre of the screen, or, kept, in the top-right
// corner (or wherever you drag it by its title) until you close it (its ×
// button, or the menu).
//
// It never takes keyboard focus, so it can't interrupt typing, and clicks
// pass through it to the window below: only a kept card's title row (to
// drag it) and its × take a click.
// A shown card fades out after the nudge's ttl. What the commands do is in
// the top-bar menu, and a kept card lists them too.

import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {toMarkup} from './markup.js';
import {onScreen} from './placement.js';

const FADE_MS = 250;
const GAP_PX = 12;

export class Card {
    // `kept`: stays until hidden, with the explanation and a close button
    // that calls `onClose`. `position` and `onMoved` remember where it was
    // dragged to: `[x, y]`, or null for the corner.
    constructor({kept = false, onClose = null, position = null, onMoved = null} = {}) {
        this._kept = kept;
        this._position = position;
        this._onMoved = onMoved;
        this._drag = null;
        this._up = false;
        this._hideId = 0;
        this._actor = new St.BoxLayout({
            style_class: kept ? 'flowbuddy-card flowbuddy-card-kept' : 'flowbuddy-card',
            vertical: true,
            reactive: false,
            can_focus: false,
            opacity: 0,
            visible: false,
        });
        this._title = new St.Label({style_class: 'flowbuddy-card-title', x_expand: true});
        this._text = new St.Label({style_class: 'flowbuddy-card-text'});
        this._text.clutter_text.line_wrap = true;
        this._explain = new St.Label({style_class: 'flowbuddy-card-explain', visible: kept});
        this._explain.clutter_text.line_wrap = true;
        if (kept) {
            // The title row is the handle to drag it by.
            const header = new St.BoxLayout({x_expand: true, reactive: true, track_hover: true});
            this._dragBy(header);
            const close = new St.Button({
                style_class: 'flowbuddy-card-close',
                child: new St.Icon({icon_name: 'window-close-symbolic', icon_size: 14}),
                can_focus: false,
                y_align: Clutter.ActorAlign.START,
                accessible_name: 'Stop keeping this tip on screen',
            });
            close.connect('clicked', () => onClose?.());
            header.add_child(this._title);
            header.add_child(close);
            this._actor.add_child(header);
        } else {
            this._actor.add_child(this._title);
        }
        this._actor.add_child(this._text);
        this._actor.add_child(this._explain);
        Main.layoutManager.uiGroup.add_child(this._actor);
    }

    get shown() {
        return this._up;
    }

    show(nudge) {
        this._up = true;
        this._title.text = nudge.title;
        this._text.clutter_text.set_markup(toMarkup(nudge.text));
        if (this._kept)
            this._explain.clutter_text.set_markup((nudge.explain ?? []).map(toMarkup).join('\n'));
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
        this._hideId = 0;
        if (this._kept)
            return;
        this._hideId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, nudge.ttl_ms, () => {
            this._hideId = 0;
            this.hide();
            return GLib.SOURCE_REMOVE;
        });
    }

    hide() {
        this._up = false;
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

    // Where it was dragged to, or null for the corner.
    get position() {
        return this._position;
    }

    // Back to the corner.
    resetPosition() {
        this._position = null;
        this._onMoved?.(null);
        if (this._up)
            this._place();
    }

    _dragBy(handle) {
        handle.connect('button-press-event', (_a, event) => {
            if (event.get_button() !== 1)
                return Clutter.EVENT_PROPAGATE;
            const [px, py] = event.get_coords();
            const [x, y] = this._actor.get_position();
            this._drag = {dx: px - x, dy: py - y, grab: global.stage.grab(handle)};
            return Clutter.EVENT_STOP;
        });
        handle.connect('motion-event', (_a, event) => {
            if (!this._drag)
                return Clutter.EVENT_PROPAGATE;
            const [px, py] = event.get_coords();
            this._actor.set_position(Math.round(px - this._drag.dx), Math.round(py - this._drag.dy));
            return Clutter.EVENT_STOP;
        });
        handle.connect('button-release-event', () => {
            if (!this._drag)
                return Clutter.EVENT_PROPAGATE;
            this._drag.grab.dismiss();
            this._drag = null;
            const [x, y] = this._actor.get_position();
            this._position = [x, y];
            this._onMoved?.(this._position);
            return Clutter.EVENT_STOP;
        });
    }

    _place() {
        const monitor = Main.layoutManager.primaryMonitor;
        if (!monitor)
            return;
        const [, width] = this._actor.get_preferred_width(-1);
        if (this._kept && this._position) {
            // Where you left it, pulled back on screen if the monitors
            // changed since.
            const [, height] = this._actor.get_preferred_height(width);
            const [x, y] = onScreen(this._position, [width, height], Main.layoutManager.monitors);
            this._actor.set_position(x, y);
            return;
        }
        const top = Main.panel.visible ? Main.panel.height : 0;
        const x = this._kept
            ? monitor.x + monitor.width - width - GAP_PX
            : Math.floor(monitor.x + (monitor.width - width) / 2);
        this._actor.set_position(x, monitor.y + top + GAP_PX);
    }

    destroy() {
        this._drag?.grab.dismiss();
        this._drag = null;
        if (this._hideId)
            GLib.source_remove(this._hideId);
        this._hideId = 0;
        this._actor.destroy();
    }
}
