// Report UI events for Flow Buddy's VS Code pack, by name only: a file
// opened in a new tab, or another tab made active (never which file). The
// daemon adds whether a click or the shortcut came just before.

'use strict';

const vscode = require('vscode');

const VISUAL = ['visual', 'visual_line', 'visual_block'];
// Making the same selection again within this long is reselecting.
const RESELECT_MS = 60000;

class UiReporter {
    constructor(link, modes) {
        this._link = link;
        this._modes = modes;
        this._active = null;
        // The Visual selection being made, and the last one finished, as
        // {document, ranges, at}. Positions stay here.
        this._selection = null;
        this._lastSelection = null;
    }

    // Called by the mode tracker: leaving Visual mode finishes a selection;
    // the same one again soon after is what `gv` does.
    modeChanged(previous, mode) {
        if (!VISUAL.includes(previous) || VISUAL.includes(mode) || !this._selection)
            return;
        const sel = this._selection;
        this._selection = null;
        const last = this._lastSelection;
        if (last && last.document === sel.document && last.ranges === sel.ranges &&
            Date.now() - last.at <= RESELECT_MS)
            this._link.send({type: 'editor_event', event: 'reselected'});
        this._lastSelection = {...sel, at: Date.now()};
    }

    // While in Visual mode: the selection as it stands.
    _selectionChanged(editor) {
        this._modes.update();
        if (!VISUAL.includes(this._modes.mode))
            return;
        const ranges = editor.selections
            .map(s => `${s.start.line}:${s.start.character}-${s.end.line}:${s.end.character}`)
            .join(',');
        this._selection = {document: editor.document, ranges};
    }

    enable(subscriptions) {
        subscriptions.push(vscode.window.onDidChangeTextEditorSelection(e => this._selectionChanged(e.textEditor)));
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
