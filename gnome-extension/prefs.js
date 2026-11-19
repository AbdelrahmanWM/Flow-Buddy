// Flow Buddy's settings window: the basics first, the rest folded away
// under Advanced, and the terminal's tools as buttons.
//
// Everything goes through the `flowbuddy` command (`config list --json`,
// `config set`, the tools' `--json`), which checks every value, so this
// window holds no settings logic of its own.

import Adw from 'gi://Adw';
import Gdk from 'gi://Gdk';
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

const SKILL_CSS = `
.flowbuddy-tile { padding: 4px 10px; border-radius: 6px; font-family: monospace; }
.flowbuddy-known { background: #2ec27e; color: #000; }
.flowbuddy-slipping { background: #f6d32d; color: #000; }
.flowbuddy-learning { background: #99c1f1; color: #000; }
.flowbuddy-ready { border: 1px solid alpha(currentColor, 0.6); }
.flowbuddy-later { opacity: 0.45; border: 1px dashed alpha(currentColor, 0.4); }
`;

export default class FlowBuddyPrefs extends ExtensionPreferences {
    async fillPreferencesWindow(window) {
        this._window = window;
        // Nothing of this window outlives it (extensions.gnome.org's review
        // asks for it): the skill tiles' CSS and every field go when it closes.
        window.connect('close-request', () => {
            if (this._skillCss)
                Gtk.StyleContext.remove_provider_for_display(Gdk.Display.get_default(), this._skillCss);
            this._skillCss = null;
            this._window = null;
            this._cli = null;
            this._pages = null;
            this._list = null;
            this._reverting = null;
            return false;
        });
        window.set_default_size(720, 760);
        window.set_search_enabled(true);
        this._cli = findCli();
        if (!this._cli) {
            this._pages = [];
            this._addPage(this._missingPage('Flow Buddy isn\'t installed',
                'The settings need the flowbuddy command. Install it with scripts/install.sh from the Flow Buddy folder, then open this window again.'));
            return;
        }
        await this._load();
        // "Take the tour" in the top-bar menu opened this window for it.
        const gs = this.getSettings();
        if (gs.get_boolean('open-tour')) {
            gs.set_boolean('open-tour', false);
            this._tour();
        }
    }

    // Read the settings and build the pages; again after a profile.
    async _load() {
        const window = this._window;
        for (const page of this._pages ?? [])
            window.remove(page);
        this._pages = [];
        let list;
        try {
            const r = await run(this._cli, ['config', 'list', '--json']);
            if (!r.ok)
                throw new Error(r.err.trim() || 'flowbuddy config list failed');
            list = JSON.parse(r.out);
        } catch (e) {
            this._addPage(this._missingPage('Couldn\'t read the settings', String(e.message ?? e)));
            return;
        }
        this._list = list;
        const {basic, groups, rules} = model.arrange(list.settings);
        this._addPage(this._generalPage(basic, list.profiles ?? [], list.settings));
        this._addPage(this._advancedPage(groups));
        this._addPage(this._rulesPage(rules));
        this._addPage(this._skillsPage());
        this._addPage(this._toolsPage());
    }

    _addPage(page) {
        // A command can finish after the window has closed.
        if (!this._window)
            return;
        this._window.add(page);
        this._pages.push(page);
    }

    _missingPage(title, text) {
        const page = new Adw.PreferencesPage({title: 'Flow Buddy', icon_name: 'dialog-warning-symbolic'});
        const group = new Adw.PreferencesGroup({title, description: text});
        page.add(group);
        return page;
    }

    _toast(text) {
        this._window?.add_toast(new Adw.Toast({title: text, timeout: 4}));
    }

    _generalPage(basic, profiles, settings) {
        const page = new Adw.PreferencesPage({title: 'General', icon_name: 'preferences-system-symbolic'});
        if (profiles.length)
            page.add(this._profilesGroup(profiles, settings));
        const group = new Adw.PreferencesGroup({
            title: 'The basics',
            description: 'Changes apply right away. Everything else is under Advanced.',
        });
        for (const s of basic)
            group.add(this._row(s));
        page.add(group);
        return page;
    }

    // Profiles: ordinary settings, set together. The one your settings
    // match says so; none matching means they're your own.
    _profilesGroup(profiles, settings) {
        const inUse = profiles.find(p => p.current);
        const group = new Adw.PreferencesGroup({
            title: 'Start from a profile',
            description: inUse
                ? `Your settings match ${inUse.name}. Change any of them below: a profile only sets them.`
                : 'Your settings are your own. A profile sets a few of them for a way of working; change any afterwards.',
        });
        for (const p of profiles) {
            const row = new Adw.ActionRow({title: p.name, subtitle: p.what});
            if (p.current) {
                row.add_suffix(new Gtk.Label({label: 'In use', valign: Gtk.Align.CENTER, css_classes: ['dim-label']}));
            } else {
                const use = new Gtk.Button({label: 'Use…', valign: Gtk.Align.CENTER});
                use.connect('clicked', () => this._useProfile(p, settings));
                row.add_suffix(use);
                row.activatable_widget = use;
            }
            group.add(row);
        }
        return group;
    }

    async _useProfile(profile, settings) {
        const changes = model.profileChanges(profile, settings);
        if (!await this._confirm(`Use ${profile.name}?`,
            `This changes:\n\n${changes.join('\n')}\n\nYou can change any of them afterwards.`, 'Use it'))
            return;
        try {
            const r = await run(this._cli, ['config', 'profile', profile.id]);
            if (!r.ok)
                throw new Error(r.err.trim());
            this._toast(`${profile.name} it is.`);
            await this._load();
        } catch (e) {
            this._toast(`Couldn't: ${e.message ?? e}`);
        }
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
            const expander = new Adw.ExpanderRow({
                title: GLib.markup_escape_text(r.title, -1),
                subtitle: GLib.markup_escape_text(r.description ?? r.id, -1),
            });
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

    // Every skill as a tile, coloured by where you are with it.
    _skillsPage() {
        if (!this._skillCss) {
            this._skillCss = new Gtk.CssProvider();
            this._skillCss.load_from_string(SKILL_CSS);
            Gtk.StyleContext.add_provider_for_display(Gdk.Display.get_default(), this._skillCss,
                Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
        const page = new Adw.PreferencesPage({title: 'Skills', icon_name: 'starred-symbolic'});
        const legend = new Adw.PreferencesGroup({
            title: 'Your skills',
            description: 'Each Vim skill Flow Buddy teaches. Hover over one for what it does and when you last used it.',
        });
        legend.add(this._tiles(model.TILES.map(([tile, text]) => ({keys: text, tile, tooltip: ''}))));
        page.add(legend);
        run(this._cli, ['learn', '--json']).then(r => {
            const groups = model.skillGroups(JSON.parse(r.out));
            if (!groups.length) {
                legend.set_description('Nothing recorded yet: is the daemon running?');
                return;
            }
            for (const g of groups) {
                const group = new Adw.PreferencesGroup({title: g.name, description: `You use ${g.known} of ${g.tiles.length}`});
                group.add(this._tiles(g.tiles));
                page.add(group);
            }
        }).catch(e => legend.set_description(`Couldn't read your skills: ${e.message ?? e}`));
        return page;
    }

    _tiles(tiles) {
        const box = new Gtk.FlowBox({selection_mode: Gtk.SelectionMode.NONE, max_children_per_line: 12,
            column_spacing: 6, row_spacing: 6});
        for (const t of tiles) {
            box.append(new Gtk.Label({label: t.keys, tooltip_text: t.tooltip || null,
                css_classes: ['flowbuddy-tile', `flowbuddy-${t.tile}`]}));
        }
        return box;
    }

    _toolsPage() {
        const page = new Adw.PreferencesPage({title: 'Tools', icon_name: 'utilities-terminal-symbolic'});
        const start = new Adw.PreferencesGroup({
            title: 'New here?',
            description: 'How Flow Buddy works, in seven short parts, in a terminal.',
        });
        start.add(this._toolRow('Tour', 'What it watches and keeps, a sample tip and how to answer it, the settings that matter, and the learning tools',
            'Start', () => this._tour()));
        page.add(start);

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

        const practise = new Adw.PreferencesGroup({
            title: 'Practise',
            description: 'A two-minute drill on your weakest habit, in Vim. In Vim itself: :FlowBuddyPractice.',
        });
        practise.add(this._toolRow('Practice', 'Rounds like "go to the marked line" or "empty the brackets", in as few keys as you can',
            'Start', () => this._inTerminal('practice')));
        page.add(practise);

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

    // The tour, one part a page, from `flowbuddy tour --json`: the same
    // text as in the terminal, with the checks, the sample tip and the
    // settings live on their pages.
    async _tour() {
        this.getSettings().set_boolean('welcomed', true);
        let tour;
        try {
            const r = await run(this._cli, ['tour', '--json']);
            if (!r.ok)
                throw new Error(r.err.trim() || 'flowbuddy tour --json failed');
            tour = JSON.parse(r.out);
        } catch (e) {
            this._toast(`Couldn't start the tour: ${e.message ?? e}`);
            return;
        }
        const markup = t => model.tourMarkup(t, x => GLib.markup_escape_text(x, -1));
        const para = text => text.split('\n\n').filter(Boolean).map(p => new Gtk.Label({
            label: markup(p), use_markup: true, wrap: true, xalign: 0, selectable: false,
            width_chars: 50, max_width_chars: 70,
        }));
        const carousel = new Adw.Carousel({hexpand: true, vexpand: true, allow_scroll_wheel: false});
        const parts = tour.parts ?? [];
        parts.forEach((part, i) => {
            const box = new Gtk.Box({
                orientation: Gtk.Orientation.VERTICAL, spacing: 14,
                margin_top: 18, margin_bottom: 18, margin_start: 24, margin_end: 24,
            });
            box.append(new Gtk.Label({
                label: `${i + 1} of ${parts.length}`, xalign: 0, css_classes: ['dim-label', 'caption'],
            }));
            box.append(new Gtk.Label({label: part.title, xalign: 0, wrap: true, css_classes: ['title-2']}));
            const extra = new Gtk.Box({orientation: Gtk.Orientation.VERTICAL, spacing: 10});
            if (part.action === 'sample')
                box.append(this._tourSample());
            for (const l of para(part.text ?? ''))
                box.append(l);
            if (part.action === 'checks')
                box.append(this._tourChecks());
            if (part.action === 'settings') {
                const group = new Adw.PreferencesGroup();
                for (const s of model.tourSettings(tour.settings ?? [], this._list?.settings ?? []))
                    group.add(this._row(s));
                box.append(group);
            }
            if (part.rows?.length) {
                const group = new Adw.PreferencesGroup();
                for (const [what, about] of part.rows) {
                    group.add(new Adw.ActionRow({
                        title: GLib.markup_escape_text(what, -1), subtitle: GLib.markup_escape_text(about, -1),
                        css_classes: ['property'],
                    }));
                }
                box.append(group);
            }
            for (const l of para(part.after ?? ''))
                extra.append(l);
            box.append(extra);
            if (i === parts.length - 1) {
                const practise = new Gtk.Button({label: 'Try a drill now', halign: Gtk.Align.START, css_classes: ['suggested-action', 'pill']});
                practise.connect('clicked', () => this._inTerminal('practice'));
                box.append(practise);
            }
            carousel.append(new Gtk.ScrolledWindow({
                child: box, hscrollbar_policy: Gtk.PolicyType.NEVER, hexpand: true, vexpand: true,
            }));
        });

        const back = new Gtk.Button({label: 'Back', sensitive: false});
        const next = new Gtk.Button({label: 'Next', css_classes: ['suggested-action']});
        const dialog = new Adw.Dialog({title: 'Welcome to Flow Buddy', content_width: 620, content_height: 640});
        // The page it's on, or heading for: quick clicks mid-slide count.
        let page = 0;
        const go = i => {
            page = Math.max(0, Math.min(parts.length - 1, i));
            carousel.scroll_to(carousel.get_nth_page(page), true);
            update();
        };
        const update = () => {
            back.sensitive = page > 0;
            next.label = page === parts.length - 1 ? 'Done' : 'Next';
        };
        carousel.connect('page-changed', (_c, i) => {
            page = i;
            update();
        });
        back.connect('clicked', () => go(page - 1));
        next.connect('clicked', () => {
            if (page >= parts.length - 1)
                dialog.close();
            else
                go(page + 1);
        });
        const bottom = new Gtk.CenterBox({margin_top: 6, margin_bottom: 12, margin_start: 12, margin_end: 12});
        bottom.set_start_widget(back);
        bottom.set_center_widget(new Adw.CarouselIndicatorDots({carousel}));
        bottom.set_end_widget(next);
        const view = new Adw.ToolbarView({content: carousel});
        view.add_top_bar(new Adw.HeaderBar());
        view.add_bottom_bar(bottom);
        dialog.set_child(view);
        dialog.present(this._window);
    }

    // The doctor's checks, with how to fix what's missing.
    _tourChecks() {
        const group = new Adw.PreferencesGroup({description: 'Checking…'});
        run(this._cli, ['doctor', '--json']).then(r => {
            const j = JSON.parse(r.out);
            const bad = j.checks.filter(c => !c.ok);
            group.description = bad.length
                ? `${j.checks.length - bad.length} of ${j.checks.length} checks pass. To fix:`
                : `Yes: all ${j.checks.length} checks pass. Flow Buddy is watching.`;
            for (const c of j.checks) {
                const row = new Adw.ActionRow({
                    title: GLib.markup_escape_text(c.what, -1),
                    subtitle: c.ok ? '' : GLib.markup_escape_text(c.fix ?? '', -1),
                });
                row.add_prefix(new Gtk.Image({icon_name: c.ok ? 'emblem-ok-symbolic' : 'dialog-warning-symbolic'}));
                group.add(row);
            }
        }).catch(e => {
            group.description = `Couldn't run the checks: ${e.message ?? e}`;
        });
        return group;
    }

    // A button that shows the sample tip where tips show.
    _tourSample() {
        const box = new Gtk.Box({spacing: 12});
        const button = new Gtk.Button({label: 'Show a sample tip', css_classes: ['pill']});
        const said = new Gtk.Label({wrap: true, xalign: 0, hexpand: true, css_classes: ['dim-label']});
        button.connect('clicked', async () => {
            const r = await run(this._cli, ['tour', '--sample']).catch(e => ({ok: false, err: String(e)}));
            if (!r.ok) {
                said.label = 'The daemon isn\'t running, so there\'s nothing to show it: see the checks.';
                return;
            }
            said.label = JSON.parse(r.out).shown === 'card'
                ? 'It\'s on your screen now, under the top bar. Real tips look the same.'
                : 'It\'s in your notifications now. Real tips look the same.';
        });
        box.append(button);
        box.append(said);
        return box;
    }

    // `flowbuddy practice` (or another command) in a terminal: the drill
    // runs in Vim, the tour asks questions.
    _inTerminal(command) {
        const argv = model.inTerminal([this._cli, command], name => GLib.find_program_in_path(name));
        if (!argv) {
            this._toast(`No terminal found: run flowbuddy ${command} in one`);
            return;
        }
        try {
            Gio.Subprocess.new(argv, Gio.SubprocessFlags.NONE);
        } catch (e) {
            this._toast(`Couldn't open a terminal: ${e.message ?? e}`);
        }
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
