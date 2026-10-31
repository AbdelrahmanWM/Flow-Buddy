// The settings window's tour, in a real GTK window drawn off screen
// (broadway): run by prefs-tour.sh, which sets up a sandboxed CLI.
import Adw from 'gi://Adw';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Gtk from 'gi://Gtk';
import System from 'system';

const dir = GLib.path_get_dirname(import.meta.url.replace('file://', ''));
const source = Gio.SettingsSchemaSource.new_from_directory(`${dir}/schemas`, null, false);
globalThis.fbSettings = new Gio.Settings({
    settings_schema: source.lookup('org.gnome.shell.extensions.flowbuddy', false),
    backend: Gio.memory_settings_backend_new(),
});
// As "Take the tour…" in the top-bar menu leaves it.
globalThis.fbSettings.set_boolean('open-tour', true);
const {default: Prefs} = await import(`file://${dir}/prefs.js`);

let failed = 0;
const check = (name, got, want) => {
    if (JSON.stringify(got) !== JSON.stringify(want)) {
        failed++;
        printerr(`FAIL ${name}\n  got  ${JSON.stringify(got)}\n  want ${JSON.stringify(want)}`);
    }
};
const all = (w, test, out = []) => {
    if (test(w))
        out.push(w);
    for (let c = w.get_first_child(); c; c = c.get_next_sibling())
        all(c, test, out);
    return out;
};
const button = (w, label) => all(w, x => x instanceof Gtk.Button && x.label === label)[0];
const wait = ms => new Promise(r => GLib.timeout_add(0, ms, () => {
    r();
    return GLib.SOURCE_REMOVE;
}));

const app = new Adw.Application({application_id: 'test.flowbuddy.prefs'});
app.connect('activate', () => {
    (async () => {
        try {
            const win = new Adw.PreferencesWindow({application: app});
            win.present();
            await new Prefs().fillPreferencesWindow(win);
            await wait(2000);
            const dialog = win.get_visible_dialog();
            check('the tour opened', dialog?.title, 'Welcome to Flow Buddy');
            check('once', [fbSettings.get_boolean('open-tour'), fbSettings.get_boolean('welcomed')], [false, true]);
            const carousel = all(dialog, x => x instanceof Adw.Carousel)[0];
            check('seven pages', carousel.get_n_pages(), 7);
            const titles = all(carousel, x => x instanceof Gtk.Label && x.css_classes.includes('title-2')).map(l => l.label);
            check('first title', titles[0], 'What Flow Buddy does');
            const rows = all(carousel.get_nth_page(4), x => x instanceof Adw.ActionRow).map(r => r.title);
            check('the five settings', rows.length, 5);
            const checks = all(carousel.get_nth_page(1), x => x instanceof Adw.ActionRow);
            check('the checks ran', checks.length > 3, true);
            check('the sample button', !!button(carousel.get_nth_page(2), 'Show a sample tip'), true);
            check('Back at the start', button(dialog, 'Back').sensitive, false);
            for (let i = 0; i < 6; i++) {
                button(dialog, 'Next').emit('clicked');
                await wait(150);
            }
            await wait(800);
            check('Done at the end', !!button(dialog, 'Done'), true);
            button(dialog, 'Back').emit('clicked');
            await wait(800);
            check('Back', Math.round(carousel.get_position()), 5);
            button(dialog, 'Next').emit('clicked');
            await wait(800);
            button(dialog, 'Done').emit('clicked');
            await wait(800);
            check('Done closes it', win.get_visible_dialog(), null);
        } catch (e) {
            failed++;
            printerr(`FAIL ${e}\n${e.stack}`);
        }
        app.quit();
    })();
});
await app.runAsync([]);
print(failed ? `${failed} failed` : 'prefs tour ok');
if (failed)
    System.exit(1);
