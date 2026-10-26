// Report UI events for Flow Buddy's VS Code pack, by name only: a file
// opened in a new tab (never which file). The daemon adds whether a click
// or the shortcut came just before.

'use strict';

const vscode = require('vscode');

class UiReporter {
    constructor(link) {
        this._link = link;
    }

    enable(subscriptions) {
        subscriptions.push(vscode.window.tabGroups.onDidChangeTabs(e => {
            if (e.opened.some(tab => tab.input instanceof vscode.TabInputText))
                this._link.send({type: 'editor_event', event: 'file_opened'});
        }));
    }
}

module.exports = {UiReporter};
