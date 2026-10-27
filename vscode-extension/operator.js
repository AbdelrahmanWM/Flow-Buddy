// Report which text objects an operator's range was (the `operator`
// message), so the daemon can suggest `di(` over moving in and `dt)`.
//
// VS Code doesn't say what an edit removed, so while you're in Normal or
// Visual mode this keeps a copy of the active document's lines, and reads
// the removed range from it. The copy and the text never leave the
// extension: only the operator and the text objects' names are sent.
// Yanks change nothing in the document, so only deletes and changes are
// seen.

'use strict';

const vscode = require('vscode');
const {charObjects, lineObjects, rangeFacts} = require('./facts');
const {textAllowed} = require('./textguard');

const OPERATOR_MODES = ['normal', 'visual', 'visual_line', 'visual_block'];
// vscodevim may switch to Insert just before a change's edit arrives.
const LEFT_NORMAL_MS = 200;
// Then see which mode it settled in: Insert means `c`, otherwise `d`.
const SETTLE_MS = 150;
// Don't copy huge documents.
const MAX_LINES = 20000;

function linesOf(document) {
    const lines = [];
    for (let i = 0; i < document.lineCount; i++)
        lines.push(document.lineAt(i).text);
    return lines;
}

const blank = line => line === undefined || !/\S/.test(line);

class OperatorTracker {
    constructor(link, modeTracker) {
        this._link = link;
        this._modes = modeTracker;
        this._snapshot = null;
        this._document = null;
        this._leftNormalMs = 0;
    }

    enable(subscriptions) {
        const sub = d => subscriptions.push(d);
        sub(vscode.window.onDidChangeActiveTextEditor(() => this._snap()));
        sub(vscode.workspace.onDidChangeTextDocument(e => this._onChange(e)));
        this._snap();
    }

    // Called by the mode tracker.
    modeChanged(previous, mode) {
        if (OPERATOR_MODES.includes(previous) && !OPERATOR_MODES.includes(mode))
            this._leftNormalMs = Date.now();
        if (OPERATOR_MODES.includes(mode) && !this._snapshot)
            this._snap();
    }

    _snap() {
        const editor = vscode.window.activeTextEditor;
        const doc = editor?.document;
        this._document = doc ?? null;
        this._snapshot = doc && doc.lineCount <= MAX_LINES && OPERATOR_MODES.includes(this._modes.mode)
            ? linesOf(doc) : null;
    }

    _onChange(e) {
        if (e.document !== this._document || !e.contentChanges.length)
            return;
        const operating = OPERATOR_MODES.includes(this._modes.mode) ||
            Date.now() - this._leftNormalMs < LEFT_NORMAL_MS;
        const snapshot = this._snapshot;
        if (operating && snapshot && e.contentChanges.length === 1 && e.contentChanges[0].rangeLength > 0)
            this._report(snapshot, e.contentChanges[0].range);
        // Keep the copy only while it can be kept current cheaply.
        this._snapshot = null;
        if (OPERATOR_MODES.includes(this._modes.mode))
            this._snap();
    }

    _report(lines, range) {
        const {start, end} = range;
        let objects;
        let shape = null;
        if (start.character === 0 && end.character === 0 && end.line > start.line) {
            objects = lineObjects(lines.slice(start.line, end.line),
                blank(lines[start.line - 1]) || start.line === 0, blank(lines[end.line]));
        } else {
            const text = start.line === end.line
                ? lines[start.line].slice(start.character, end.character)
                : [lines[start.line].slice(start.character),
                    ...lines.slice(start.line + 1, end.line),
                    lines[end.line].slice(0, end.character)].join('\n');
            const before = lines[start.line].charAt(start.character - 1);
            const after = lines[end.line].charAt(end.character);
            objects = charObjects(text, before, after);
            // With [text] shapes on: which motions would have covered it.
            const doc = this._document;
            if (start.line === end.line && end.character > start.character &&
                textAllowed(this._link.editorConfig, doc?.uri.scheme, doc?.uri.fsPath, false)) {
                const line = lines[start.line];
                // Code points, as Vim counts characters.
                const first = [...line.slice(0, start.character)].length;
                const last = first + [...text].length - 1;
                shape = rangeFacts(line, first, last);
            }
        }
        if (!objects.length && !shape)
            return;
        const atMs = Date.now();
        const doc = this._document;
        if (doc && textAllowed(this._link.editorConfig, doc.uri.scheme, doc.uri.fsPath, true))
            this._link.snippets.add(atMs, doc.languageId, lines, start.line);
        setTimeout(() => {
            const op = this._modes.mode === 'insert' ? 'change' : 'delete';
            const msg = {type: 'operator', op, at_ms: atMs, objects};
            if (shape)
                msg.range = shape;
            this._link.send(msg);
        }, SETTLE_MS);
    }
}

module.exports = {OperatorTracker};
