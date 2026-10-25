// Report the focused window's app id and PID to the daemon, and that a
// click happened.
//
// Window titles are deliberately not sent: they often contain file names
// and the daemon doesn't need them. Clicks are sent without position or
// target; touchpad taps count, which evdev can't see.

import Clutter from 'gi://Clutter';
import Shell from 'gi://Shell';

function appId(win) {
    const app = Shell.WindowTracker.get_default().get_window_app(win);
    const id = app?.get_id() ?? win.get_gtk_application_id() ?? win.get_wm_class();
    return id ? id.replace(/\.desktop$/, '') : null;
}

export class FocusReporter {
    constructor(link) {
        this._link = link;
        this._signalId = 0;
        this._clickId = 0;
    }

    enable() {
        this._signalId = global.display.connect('notify::focus-window', () => this.report());
        this._clickId = global.stage.connect('captured-event', (_stage, event) => {
            const type = event.type();
            if (type === Clutter.EventType.BUTTON_PRESS || type === Clutter.EventType.TOUCH_BEGIN)
                this._link.send({type: 'pointer_click'});
            return Clutter.EVENT_PROPAGATE;
        });
    }

    disable() {
        if (this._signalId)
            global.display.disconnect(this._signalId);
        if (this._clickId)
            global.stage.disconnect(this._clickId);
        this._signalId = 0;
        this._clickId = 0;
    }

    report() {
        const win = global.display.focus_window;
        const pid = win?.get_pid() ?? -1;
        this._link.send({
            type: 'focus',
            app_id: win ? appId(win) : null,
            pid: pid > 0 ? pid : null,
        });
    }
}
