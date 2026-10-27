// Tell the daemon about vscodevim key bindings that matter for following
// the mode: two-key Insert-mode bindings to Esc (`jk`) and single Normal-mode
// keys bound to something else. Only the keys are sent.

'use strict';

const vscode = require('vscode');

function bindings(config, name) {
    return [...(config.get(name) ?? []), ...(config.get(`${name}NonRecursive`) ?? [])];
}

// Plain keys only: vscodevim writes special keys as <...>.
const plain = key => typeof key === 'string' && key.length === 1;

function mappings() {
    const vim = vscode.workspace.getConfiguration('vim');
    const escapes = bindings(vim, 'insertModeKeyBindings')
        .filter(b => Array.isArray(b.before) && b.before.length === 2 && b.before.every(plain) &&
            Array.isArray(b.after) && b.after.length === 1 && /^<esc>$/i.test(b.after[0]))
        .map(b => b.before.join(''));
    const normal = bindings(vim, 'normalModeKeyBindings')
        .filter(b => Array.isArray(b.before) && b.before.length === 1 && plain(b.before[0]))
        .map(b => b.before[0]);
    return {insert_escapes: escapes.slice(0, 16), normal: [...new Set(normal)].slice(0, 64)};
}

class MappingReporter {
    constructor(link) {
        this._link = link;
    }

    enable(subscriptions) {
        subscriptions.push(vscode.workspace.onDidChangeConfiguration(e => {
            if (e.affectsConfiguration('vim'))
                this.resend();
        }));
    }

    resend() {
        this._link.send({type: 'editor_mappings', ...mappings()});
    }
}

module.exports = {MappingReporter};
