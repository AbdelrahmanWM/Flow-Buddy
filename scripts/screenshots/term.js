// A bare terminal window for the screenshots: runs the command given, in
// VTE (the widget GNOME's terminals use), with a plain dark palette.
//   gjs term.js COMMAND...
imports.gi.versions.Gtk = '3.0';
imports.gi.versions.Gdk = '3.0';
imports.gi.versions.Vte = '2.91';
const {Gdk, GLib, Gtk, Pango, Vte} = imports.gi;

Gtk.init(null);
const rgba = s => {
    const c = new Gdk.RGBA();
    c.parse(s);
    return c;
};
const win = new Gtk.Window({title: 'Terminal', default_width: 920, default_height: 560});
const term = new Vte.Terminal();
term.set_font(Pango.FontDescription.from_string('Monospace 12'));
term.set_colors(rgba('#d0d0d0'), rgba('#1e1e1e'), [
    '#1e1e1e', '#e05561', '#8cc265', '#d18f52', '#4aa5f0', '#c162de', '#42b3c2', '#d0d0d0',
    '#5c6370', '#ff616e', '#a5e075', '#f0a45d', '#4dc4ff', '#de73ff', '#4cd1e0', '#ffffff',
].map(rgba));
term.set_size(96, 26);
// Steady, so every frame shows it.
term.set_cursor_blink_mode(Vte.CursorBlinkMode.OFF);
win.add(term);
win.connect('destroy', () => Gtk.main_quit());
term.connect('child-exited', () => win.destroy());
term.spawn_async(Vte.PtyFlags.DEFAULT, GLib.get_home_dir(), ARGV, null,
    GLib.SpawnFlags.SEARCH_PATH, null, -1, null, null);
win.show_all();
Gtk.main();
