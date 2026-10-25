// Whether a text editor has keyboard focus, as opposed to the terminal, a
// chat panel or another view in the same window.
//
// VS Code has no API for this, so editor activity sends `true` as a
// heartbeat (at most every HEARTBEAT_MS) and signs of focus elsewhere send
// `false`. Typing in a webview panel such as Claude Code's shows no sign at
// all; the daemon notices keys arriving without heartbeats. The heuristics
// and their limits are in README.md.

'use strict';

const vscode = require('vscode');

const HEARTBEAT_MS = 500;

// Editors that are panels rather than files: output, comment and commit
// message boxes, chat inputs.
const PANEL_SCHEMES = /^(output|comment|vscode-scm|vscode-chat|chat)/;

function isFileEditor(editor) {
    return !!editor && !PANEL_SCHEMES.test(editor.document.uri.scheme);
}

function isNonEditorTab(tab) {
    const input = tab?.input;
    return input instanceof vscode.TabInputTerminal ||
        input instanceof vscode.TabInputWebview ||
        input instanceof vscode.TabInputCustom;
}

class FocusReporter {
    constructor(link) {
        this._link = link;
        // What was last sent, and whether the editor had focus inside the
        // window when it was last known (restored when the window refocuses).
        this._sent = null;
        this._inside = isFileEditor(vscode.window.activeTextEditor);
        this._windowFocused = vscode.window.state.focused;
        this._lastBeat = 0;
        this._trailingId = null;
    }

    get focused() {
        return this._windowFocused && this._inside;
    }

    enable(subscriptions) {
        const w = vscode.window;
        const sub = d => subscriptions.push(d);

        sub(w.onDidChangeTextEditorSelection(e => {
            // No kind means the text changed under the cursor, which another
            // extension or an external write can do while you type elsewhere.
            if (e.kind !== undefined && e.textEditor === w.activeTextEditor)
                this._activity(e.textEditor);
        }));
        sub(vscode.workspace.onDidChangeTextDocument(e => {
            const editor = w.activeTextEditor;
            if (editor?.document === e.document && nearCursor(e, editor))
                this._activity(editor);
        }));
        sub(w.onDidChangeActiveTextEditor(editor => {
            if (editor)
                this._activity(editor);
            else
                this._left();
        }));
        sub(w.onDidChangeWindowState(s => {
            this._windowFocused = s.focused;
            if (s.focused && this._inside)
                this._beat();
            else if (!s.focused)
                this._report(false);
        }));
        sub(w.onDidChangeActiveTerminal(t => {
            if (t)
                this._left();
        }));
        sub(w.onDidChangeTerminalState(t => {
            if (t.state.isInteractedWith)
                this._left();
        }));
        if (w.onDidStartTerminalShellExecution)
            sub(w.onDidStartTerminalShellExecution(() => this._left()));
        sub(w.tabGroups.onDidChangeTabs(e => {
            const tab = w.tabGroups.activeTabGroup.activeTab;
            const touched = [...e.opened, ...e.changed].includes(tab);
            if (touched && tab.isActive && isNonEditorTab(tab))
                this._left();
        }));
        sub({dispose: () => clearTimeout(this._trailingId)});
    }

    // Send the current state again, e.g. after reconnecting.
    resend() {
        this._sent = null;
        this._report(this.focused);
    }

    _activity(editor) {
        if (!isFileEditor(editor)) {
            this._left();
            return;
        }
        this._inside = true;
        if (this._windowFocused)
            this._beat();
    }

    _left() {
        this._inside = false;
        this._report(false);
    }

    // Throttled: one beat now, and one more at the end of the interval if
    // there was activity in between, so the last activity is never more
    // than HEARTBEAT_MS older than the last beat.
    _beat() {
        const wait = this._lastBeat + HEARTBEAT_MS - Date.now();
        if (this._sent !== true || wait <= 0) {
            this._report(true);
            return;
        }
        this._trailingId ??= setTimeout(() => {
            this._trailingId = null;
            if (this.focused)
                this._report(true);
        }, wait);
    }

    _report(focused) {
        if (!focused) {
            clearTimeout(this._trailingId);
            this._trailingId = null;
            if (this._sent === false)
                return;
        } else {
            this._lastBeat = Date.now();
        }
        this._sent = focused;
        this._link.send({type: 'editor_focus', focused});
    }
}

// Typing changes the text where a cursor is; edits made elsewhere (by
// another extension, a formatter or a write from outside) usually don't.
function nearCursor(e, editor) {
    if (e.contentChanges.length === 0)
        return false;
    return e.contentChanges.every(c => editor.selections.some(s =>
        Math.abs(s.active.line - c.range.start.line) <= 1));
}

module.exports = {FocusReporter, isFileEditor};
