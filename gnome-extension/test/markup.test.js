// Run with: gjs -m gnome-extension/test/markup.test.js
import GLib from 'gi://GLib';
import System from 'system';

const here = GLib.path_get_dirname(GLib.path_get_dirname(System.programPath));
const {toMarkup, plain, segments, shortcutLabel} = await import(`file://${here}/markup.js`);

let failed = 0;
const check = (name, got, want) => {
    if (got !== want) {
        failed++;
        print(`FAIL ${name}\n  got  ${got}\n  want ${want}`);
    }
};

check('commands in monospace, the rest escaped',
    toMarkup('Try `Ctrl-d` then `5j` <x> & y'),
    'Try <tt><b>Ctrl-d</b></tt> then <tt><b>5j</b></tt> &lt;x&gt; &amp; y');
check('a command with a backtick in it', plain('Then `` `a `` jumps.'), 'Then `a jumps.');
check('an unclosed marker is text', plain('unclosed `x'), 'unclosed `x');
check('only a command', JSON.stringify(segments('`di(`')), '[{"code":true,"text":"di("}]');
check('markup in a command is escaped', toMarkup('`a<b>`'), '<tt><b>a&lt;b&gt;</b></tt>');

check('a shortcut', shortcutLabel('<Super><Alt>t'), 'Super+Alt+T');
check('another', shortcutLabel('<Primary><Shift>F5'), 'Ctrl+Shift+F5');
check('none', shortcutLabel(''), '');

print(failed ? `${failed} failed` : 'markup ok');
if (failed)
    System.exit(1);
