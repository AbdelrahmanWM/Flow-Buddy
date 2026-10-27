// Report UI events for Flow Buddy's VS Code pack, by name only: a file
// opened in a new tab, or another tab made active (never which file). The
// daemon adds whether a click or the shortcut came just before.

'use strict';

const vscode = require('vscode');

class UiReporter {
    constructor(link) {
        this._link = link;
        this._active = null;
    }

    enable(subscriptions) {
        const first = vscode.window.tabGroups.activeTabGroup.activeTab;
        this._active = first?.input instanceof vscode.TabInputText ? first.input.uri.toString() : null;
        subscriptions.push(vscode.window.tabGroups.onDidChangeTabs(e => {
            if (e.opened.some(tab => tab.input instanceof vscode.TabInputText))
                this._link.send({type: 'editor_event', event: 'file_opened'});
            // `changed` also fires when the active tab is edited or saved:
            // only a different tab coming to the front is a switch. Its
            // uri stays here.
            const tab = vscode.window.tabGroups.activeTabGroup.activeTab;
            const uri = tab?.input instanceof vscode.TabInputText ? tab.input.uri.toString() : null;
            if (uri && uri !== this._active && !e.opened.length && e.changed.includes(tab))
                this._link.send({type: 'editor_event', event: 'tab_switched'});
            this._active = uri;
        }));
    }
}

module.exports = {UiReporter};
