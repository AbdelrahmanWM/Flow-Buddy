// Runs $SHOT_DIR/script, one command a line, in the throwaway headless
// session scripts/screenshots/shoot.sh starts, then writes $SHOT_DIR/done:
//   wait MS          pause
//   shot NAME        the whole screen as NAME.png
//   type TEXT        type TEXT on a virtual keyboard
//   key KEYSYM       press one key: Return, Escape…
//   menu             open or close Flow Buddy's top-bar menu
//   prefs            open Flow Buddy's settings window
//   run COMMAND      start a program
//   daemon LINE      pass LINE to the stand-in daemon ($SHOT_DIR/daemon.cmd)
//   frames NAME N MS N screenshots, MS apart: NAME-000.png…
//   typeframes NAME TEXT   type TEXT, a screenshot after each key

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Shell from 'gi://Shell';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

Gio._promisify(Shell.Screenshot.prototype, 'screenshot', 'screenshot_finish');

const FLOWBUDDY = 'flowbuddy@abdelrahmanwm.github.io';
const sleep = ms => new Promise(r => GLib.timeout_add(GLib.PRIORITY_DEFAULT, ms, () => {
    r();
    return GLib.SOURCE_REMOVE;
}));

export default class Shot extends Extension {
    enable() {
        this._dir = GLib.getenv('SHOT_DIR');
        const seat = Clutter.get_default_backend().get_default_seat();
        this._keyboard = seat.create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE);
        // After the shell has settled.
        Main.layoutManager.connectObject('startup-complete', () => this._run(), this);
        if (!Main.layoutManager._startingUp)
            this._run();
    }

    disable() {
        Main.layoutManager.disconnectObject(this);
        this._keyboard = null;
    }

    _log(text) {
        const f = Gio.File.new_for_path(`${this._dir}/log`);
        const s = f.append_to(Gio.FileCreateFlags.NONE, null);
        s.write_all(`${text}\n`, null);
        s.close(null);
    }

    async _shot(name) {
        const file = Gio.File.new_for_path(`${this._dir}/${name}.png`);
        const stream = file.replace(null, false, Gio.FileCreateFlags.NONE, null);
        await new Shell.Screenshot().screenshot(false, stream);
        stream.close(null);
    }

    _press(keyval) {
        const t = GLib.get_monotonic_time();
        this._keyboard.notify_keyval(t, keyval, Clutter.KeyState.PRESSED);
        this._keyboard.notify_keyval(t + 1000, keyval, Clutter.KeyState.RELEASED);
    }

    async _run() {
        if (this._started)
            return;
        this._started = true;
        Main.overview.hide();
        const [, bytes] = GLib.file_get_contents(`${this._dir}/script`);
        const lines = new TextDecoder().decode(bytes).split('\n').filter(l => l.trim() && !l.startsWith('#'));
        for (const line of lines) {
            const [cmd, ...rest] = line.split(' ');
            const arg = rest.join(' ');
            this._log(line);
            try {
                if (cmd === 'wait') {
                    await sleep(Number(arg));
                } else if (cmd === 'shot') {
                    await this._shot(arg);
                } else if (cmd === 'frames') {
                    const [name, n, ms] = rest;
                    for (let i = 0; i < Number(n); i++) {
                        await this._shot(`${name}-${String(i).padStart(3, '0')}`);
                        await sleep(Number(ms));
                    }
                } else if (cmd === 'typeframes') {
                    // Type TEXT, with a screenshot after each key.
                    const [name, text] = rest;
                    let i = 0;
                    for (const ch of text) {
                        this._press(Clutter.unicode_to_keysym(ch.codePointAt(0)));
                        await sleep(140);
                        await this._shot(`${name}-${String(i++).padStart(3, '0')}`);
                    }
                } else if (cmd === 'type') {
                    for (const ch of arg) {
                        this._press(Clutter.unicode_to_keysym(ch.codePointAt(0)));
                        await sleep(90);
                    }
                } else if (cmd === 'key') {
                    this._press(Clutter[`KEY_${arg}`]);
                } else if (cmd === 'menu') {
                    Main.panel.statusArea[FLOWBUDDY]?.menu.toggle();
                } else if (cmd === 'prefs') {
                    Main.extensionManager.openExtensionPrefs(FLOWBUDDY, '', {});
                } else if (cmd === 'run') {
                    const command = arg.replaceAll('SHOT_HERE', GLib.getenv('SHOT_HERE'));
                    GLib.spawn_async(null, ['bash', '-c', `${command} >> "$SHOT_DIR/run.log" 2>&1`],
                        null, GLib.SpawnFlags.SEARCH_PATH, null);
                } else if (cmd === 'daemon') {
                    const f = Gio.File.new_for_path(`${this._dir}/daemon.cmd`);
                    const s = f.append_to(Gio.FileCreateFlags.NONE, null);
                    s.write_all(`${arg}\n`, null);
                    s.close(null);
                }
            } catch (e) {
                this._log(`error: ${e}`);
            }
        }
        GLib.file_set_contents(`${this._dir}/done`, 'done');
    }
}
