// Run with: gjs -m gnome-extension/test/placement.test.js
import GLib from 'gi://GLib';
import System from 'system';

const here = GLib.path_get_dirname(GLib.path_get_dirname(System.programPath));
const {onScreen} = await import(`file://${here}/placement.js`);

let failed = 0;
const check = (name, got, want) => {
    if (JSON.stringify(got) !== JSON.stringify(want)) {
        failed++;
        print(`FAIL ${name}\n  got  ${JSON.stringify(got)}\n  want ${JSON.stringify(want)}`);
    }
};

const one = [{x: 0, y: 0, width: 1920, height: 1080}];
const two = [...one, {x: 1920, y: 0, width: 1280, height: 1024}];
check('left where it was', onScreen([300, 200], [400, 120], one), [300, 200]);
check('kept inside the edge', onScreen([1800, 1050], [400, 120], one), [1512, 952]);
check('on the second monitor', onScreen([2500, 400], [400, 120], two), [2500, 400]);
check('second monitor gone: pulled back', onScreen([2500, 400], [400, 120], one), [1512, 400]);
check('no monitors: as it was', onScreen([5, 5], [10, 10], []), [5, 5]);

print(failed ? `${failed} failed` : 'placement ok');
if (failed)
    System.exit(1);
