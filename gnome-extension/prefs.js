// Flow Buddy's settings window: the basics first, the rest folded away
// under Advanced, and the terminal's tools as buttons.
//
// Everything goes through the `flowbuddy` command (`config list --json`,
// `config set`, the tools' `--json`), which checks every value, so this
// window holds no settings logic of its own.

import Adw from 'gi://Adw';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Gtk from 'gi://Gtk';

import {ExtensionPreferences} from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';

import * as model from './settings-model.js';

// Run `flowbuddy args…`: its output, and whether it succeeded.
function run(cli, args) {
    return new Promise((resolve, reject) => {
        let proc;
        try {
            proc = Gio.Subprocess.new([cli, ...args],
                Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE);
        } catch (e) {
            reject(e);
            return;
        }
        proc.communicate_utf8_async(null, null, (p, res) => {
            try {
                const [, out, err] = p.communicate_utf8_finish(res);
                resolve({ok: p.get_successful(), out: out ?? '', err: err ?? ''});
            } catch (e) {
                reject(e);
            }
        });
    });
}

function findCli() {
    return model.cliPath(GLib.find_program_in_path('flowbuddy'),
        path => GLib.file_test(path, GLib.FileTest.IS_EXECUTABLE), GLib.get_home_dir());
}

export default class FlowBuddyPrefs extends ExtensionPreferences {
    async fillPreferencesWindow(window) {
        this._window = window;
        window.set_default_size(720, 760);
        window.set_search_enabled(true);
        this._cli = findCli();
        if (!this._cli) {
            window.add(this._missingPage('Flow Buddy isn\'t installed',
                'The settings need the flowbuddy command. Install it with scripts/install.sh from the Flow Buddy folder, then open this window again.'));
            return;
        }
        let settings;
        try {
            const r = await run(this._cli, ['config', 'list', '--json']);
            if (!r.ok)
                throw new Error(r.err.trim() || 'flowbuddy config list failed');
            settings = JSON.parse(r.out).settings;
        } catch (e) {
            window.add(this._missingPage('Couldn\'t read the settings', String(e.message ?? e)));
            return;
        }
        const {basic, groups, rules} = model.arrange(settings);
        window.add(this._generalPage(basic));
        window.add(this._advancedPage(groups));
        window.add(this._rulesPage(rules));
        window.add(this._toolsPage());
    }

    _missingPage(title, text) {
        const page = new Adw.PreferencesPage({title: 'Flow Buddy', icon_name: 'dialog-warning-symbolic'});
        const group = new Adw.PreferencesGroup({title, description: text});
        page.add(group);
        return page;
    }

    _toast(text) {
        this._window.add_toast(new Adw.Toast({title: text, timeout: 4}));
    }

    _generalPage(basic) {
        const page = new Adw.PreferencesPage({title: 'General', icon_name: 'preferences-system-symbolic'});
        const group = new Adw.PreferencesGroup({
            title: 'The basics',
            description: 'Changes apply right away. Everything else is under Advanced.',
        });
        for (const s of basic)
            group.add(this._row(s));
        page.add(group);
        return page;
    }

    _advancedPage(groups) {
        const page = new Adw.PreferencesPage({title: 'Advanced', icon_name: 'applications-engineering-symbolic'});
        const group = new Adw.PreferencesGroup({
            title: 'Advanced',
            description: 'Open a section to see its settings. The same settings are in `flowbuddy config list`.',
        });
        for (const [name, items] of groups) {
            const expander = new Adw.ExpanderRow({title: name, subtitle: `${items.length} settings`});
            for (const s of items)
                expander.add_row(this._row(s));
            group.add(expander);
        }
        page.add(group);
        return page;
    }

    _rulesPage(rules) {
        const page = new Adw.PreferencesPage({title: 'Rules', icon_name: 'view-list-symbolic'});
        const group = new Adw.PreferencesGroup({
            title: 'Rules',
            description: 'Each rule spots one habit. Turn one off, or make it wait for more repeats before a tip.',
        });
        for (const r of rules) {
            const expander = new Adw.ExpanderRow({title: r.title, subtitle: r.description ?? r.id});
            if (r.enabled)
                expander.add_row(this._row(r.enabled, 'Gives tips'));
            if (r.threshold && r.default_threshold !== null)
                expander.add_row(this._row(r.threshold, 'Repeats before a tip'));
            group.add(expander);
        }
        page.add(group);
        return page;
    }

    // A control for one setting, applying changes through `flowbuddy`.
    _row(s, title = model.label(s)) {
        const subtitle = s.what;
        const ui = model.current(s);
        let row;
        let get;
        let put;
        switch (model.rowKind(s)) {
        case 'switch':
            row = new Adw.SwitchRow({title, subtitle, active: ui});
            get = () => row.active;
            put = v => (row.active = v);
            row.connect('notify::active', () => this._changed(s, get(), () => put(!get())));
            break;
        case 'spin': {
            const {min, max, step, digits} = model.spinSteps(s);
            row = Adw.SpinRow.new_with_range(min, max, step);
            row.set({title, subtitle, digits, value: ui});
            get = () => row.value;
            put = v => (row.value = v);
            let last = ui;
            let timer = 0;
            // A held arrow changes the value many times: apply the last.
            row.connect('notify::value', () => {
                if (this._reverting)
                    return;
                if (timer)
                    GLib.source_remove(timer);
                timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 500, () => {
                    timer = 0;
                    const before = last;
                    last = get();
                    this._changed(s, last, () => {
                        last = before;
                        put(before);
                    });
                    return GLib.SOURCE_REMOVE;
                });
            });
            break;
        }
        case 'combo': {
            const labels = model.choiceLabels(s);
            const offset = labels.length - s.kind.choices.length;
            row = new Adw.ComboRow({title, subtitle, model: Gtk.StringList.new(labels)});
            row.selected = ui < 0 ? 0 : ui + offset;
            let last = row.selected;
            get = () => row.selected;
            put = v => (row.selected = v);
            row.connect('notify::selected', () => {
                if (this._reverting)
                    return;
                const before = last;
                last = get();
                this._changed(s, last, () => {
                    last = before;
                    put(before);
                });
            });
            break;
        }
        default:
            row = new Adw.EntryRow({title, text: ui, show_apply_button: true, tooltip_text: subtitle});
            get = () => row.text;
            put = v => (row.text = v);
            row.connect('apply', () => {
                const before = String(model.current(s));
                this._changed(s, get(), () => put(before));
            });
        }
        if (s.more)
            this._addInfo(row, title, s.more);
        return row;
    }

    // An ⓘ beside a setting: what it does, at more length.
    _addInfo(row, title, more) {
        const info = new Gtk.Button({
            icon_name: 'help-about-symbolic',
            valign: Gtk.Align.CENTER,
            tooltip_text: 'More about this setting',
        });
        info.add_css_class('flat');
        info.connect('clicked', () => {
            const dialog = new Adw.AlertDialog({heading: title, body: more});
            dialog.add_response('close', 'Close');
            dialog.present(this._window);
        });
        row.add_suffix(info);
    }

    // Apply a change, asking first when it needs it; undo the control if
    // it's refused.
    async _changed(s, ui, revert) {
        if (this._reverting)
            return;
        const undo = () => {
            this._reverting = true;
            revert();
            this._reverting = false;
        };
        if (model.needsConfirm(s, ui) && !await this._confirm(model.label(s), s.notice, 'Turn on'))
            return undo();
        try {
            const r = await run(this._cli, model.command(s, ui));
            if (!r.ok) {
                undo();
                this._toast(r.err.trim().replace(/^flowbuddy: /, '') || 'That didn\'t work');
            }
        } catch (e) {
            undo();
            this._toast(String(e.message ?? e));
        }
    }

    _confirm(heading, body, yes) {
        return new Promise(resolve => {
            const dialog = new Adw.AlertDialog({heading, body});
            dialog.add_response('cancel', 'Cancel');
            dialog.add_response('yes', yes);
            dialog.set_response_appearance('yes', Adw.ResponseAppearance.SUGGESTED);
            dialog.connect('response', (_d, id) => resolve(id === 'yes'));
            dialog.present(this._window);
        });
    }

    _toolsPage() {
        const page = new Adw.PreferencesPage({title: 'Tools', icon_name: 'utilities-terminal-symbolic'});
        const look = new Adw.PreferencesGroup({
            title: 'See how you\'re doing',
            description: 'The same as the terminal\'s flowbuddy commands, shown here.',
        });
        const tools = [
            ['Check setup', 'Whether everything Flow Buddy needs is in place, and how to fix what isn\'t',
                ['doctor', '--json'], j => model.doctorLines(j)],
            ['Progress', 'What you use, and what you\'re ready to learn next',
                ['learn', '--json'], j => model.progressLines(j)],
            ['Today', 'Your habits today, against before', ['summary', '--json'], j => model.textLines(j)],
            ['This week', 'The last seven days', ['summary', '--week', '--json'], j => model.textLines(j)],
            ['Whether tips help', 'Tips shown against tips held back, and how well you know each skill',
                ['learn', '--report', '--json'], j => model.textLines(j)],
            ['How much of Vim', 'How much of Vim\'s command index is taught', ['learn', '--coverage', '--json'],
                j => model.coverageLines(j)],
        ];
        for (const [title, subtitle, args, lines] of tools)
            look.add(this._toolRow(title, subtitle, 'Show', () => this._show(title, args, lines)));
        page.add(look);

        const data = new Adw.PreferencesGroup({
            title: 'Your data',
            description: 'Everything Flow Buddy remembers stays on this machine.',
        });
        data.add(this._toolRow('Export', 'Save what Flow Buddy remembers as a JSON file', 'Save…',
            () => this._export()));
        data.add(this._toolRow('Forget everything', 'Delete every count and pattern Flow Buddy has kept', 'Forget…',
            () => this._forget(), true));
        page.add(data);
        return page;
    }

    _toolRow(title, subtitle, action, onClick, destructive = false) {
        const row = new Adw.ActionRow({title, subtitle});
        const button = new Gtk.Button({label: action, valign: Gtk.Align.CENTER});
        if (destructive)
            button.add_css_class('destructive-action');
        button.connect('clicked', onClick);
        row.add_suffix(button);
        row.activatable_widget = button;
        return row;
    }

    async _show(title, args, toLines) {
        let text;
        try {
            const r = await run(this._cli, args);
            // `doctor` fails when a check does, and still says which.
            text = toLines(JSON.parse(r.out)).join('\n');
        } catch (e) {
            text = `Couldn't run flowbuddy ${args.join(' ')}: ${e.message ?? e}`;
        }
        const view = new Gtk.TextView({
            editable: false,
            cursor_visible: false,
            monospace: true,
            wrap_mode: Gtk.WrapMode.WORD_CHAR,
            top_margin: 8, bottom_margin: 8, left_margin: 8, right_margin: 8,
        });
        view.buffer.text = text;
        const scroll = new Gtk.ScrolledWindow({child: view, min_content_height: 360, min_content_width: 560});
        const dialog = new Adw.AlertDialog({heading: title, extra_child: scroll});
        dialog.add_response('close', 'Close');
        dialog.present(this._window);
    }

    async _export() {
        const chooser = new Gtk.FileDialog({initial_name: 'flowbuddy-memory.json'});
        let file;
        try {
            file = await new Promise((resolve, reject) => chooser.save(this._window, null, (d, res) => {
                try {
                    resolve(d.save_finish(res));
                } catch (e) {
                    reject(e);
                }
            }));
        } catch {
            return; // Cancelled.
        }
        try {
            const r = await run(this._cli, ['memory', 'export']);
            if (!r.ok)
                throw new Error(r.err.trim());
            file.replace_contents(new TextEncoder().encode(r.out), null, false,
                Gio.FileCreateFlags.REPLACE_DESTINATION, null);
            this._toast(`Saved to ${file.get_basename()}`);
        } catch (e) {
            this._toast(`Couldn't export: ${e.message ?? e}`);
        }
    }

    async _forget() {
        if (!await this._confirm('Forget everything?',
            'Every count, pattern and lesson Flow Buddy has kept is deleted, and it starts learning about you again. This can\'t be undone.',
            'Forget everything'))
            return;
        const r = await run(this._cli, ['memory', 'forget', '--all', '--yes']).catch(e => ({ok: false, err: String(e)}));
        this._toast(r.ok ? 'Forgotten.' : `Couldn't: ${r.err.trim()}`);
    }
}
