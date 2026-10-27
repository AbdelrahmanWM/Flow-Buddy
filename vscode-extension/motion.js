// Cursor movements in Normal and Visual mode, sent as `motion` messages.
//
// A movement starts at the first cursor move and ends when the cursor has
// rested for REST_MS or the mode changes. Its facts are computed by
// facts.js from the text, and only numbers and flags are sent: never the
// text, the file or where in it the cursor is.

'use strict';

const vscode = require('vscode');
const {computeFacts, rested} = require('./facts');
const {textAllowed} = require('./textguard');

const REST_MS = 1000;
// Visual Block has a selection per line and no single cursor to follow.
const MOTION_MODES = ['normal', 'visual', 'visual_line'];

// Where vscodevim's cursor is, from the selection it draws (see
// ModeHandler.updateView). A null column means it can't be known.
function cursorOf(editor, mode) {
    const s = editor.selection;
    const a = s.active;
    if (mode === 'visual_line')
        return {line: a.line, col: null};
    if (mode === 'visual' && s.anchor.isBefore(a)) {
        // Forward selections end just past the cursor.
        if (a.character > 0)
            return {line: a.line, col: a.character - 1};
        const empty = editor.document.lineAt(a.line).text.length === 0;
        return {line: a.line, col: empty ? 0 : null};
    }
    return {line: a.line, col: a.character};
}

// The first visible line, or null when it can't be told.
function topLine(editor) {
    const ranges = editor.visibleRanges;
    return ranges.length ? ranges[0].start.line : null;
}

function samePos(a, b) {
    return a.line === b.line && a.col === b.col;
}

// The document as an array of lines, read lazily: facts.js only looks at
// the lines it needs.
function linesOf(document) {
    return new Proxy([], {
        get(_target, key) {
            if (key === 'length')
                return document.lineCount;
            const i = typeof key === 'string' ? Number(key) : NaN;
            return Number.isInteger(i) && i >= 0 && i < document.lineCount
                ? document.lineAt(i).text : undefined;
        },
    });
}

class MotionTracker {
    constructor(link, modeTracker) {
        this._link = link;
        this._modes = modeTracker;
        this._pos = null;
        this._posMode = null;
        this._editor = null;
        this._version = null;
        this._segment = null;
        this._restId = null;
        this._top = null;
        this._rests = [];
        // Where the last edit was, for `g;`: the document and line only.
        this._lastChange = null;
    }

    enable(subscriptions) {
        const w = vscode.window;
        const sub = d => subscriptions.push(d);
        sub(w.onDidChangeTextEditorSelection(e => this._onSelection(e)));
        sub(w.onDidChangeActiveTextEditor(() => this._abort()));
        sub(vscode.workspace.onDidChangeTextDocument(e => {
            if (!e.contentChanges.length)
                return;
            const last = e.contentChanges[e.contentChanges.length - 1];
            this._lastChange = {document: e.document, line: last.range.start.line};
            // An edit, not a movement.
            if (e.document === this._editor?.document)
                this._abort();
        }));
        sub({dispose: () => this._abort()});
    }

    // Called by the mode tracker: a mode change ends the movement.
    modeChanged() {
        this._finish();
        this._pos = null;
    }

    _onSelection(e) {
        const editor = e.textEditor;
        if (editor !== vscode.window.activeTextEditor)
            return;
        this._modes.update();
        const mode = this._modes.mode;
        if (editor !== this._editor || mode !== this._posMode) {
            this._finish();
            this._editor = editor;
            this._posMode = mode;
            this._pos = null;
        }
        if (!MOTION_MODES.includes(mode)) {
            this._pos = null;
            return;
        }
        const pos = cursorOf(editor, mode);
        const version = editor.document.version;
        // A cursor moved by an edit isn't a motion, and neither are clicks.
        if (version !== this._version) {
            this._abort();
            this._pos = pos;
            this._version = version;
            return;
        }
        if (e.kind === vscode.TextEditorSelectionChangeKind.Mouse) {
            this._finish();
            this._pos = pos;
            return;
        }
        const from = this._pos;
        const top = this._top;
        this._pos = pos;
        this._top = topLine(editor);
        if (!from || samePos(from, pos))
            return;
        const now = Date.now();
        this._segment ??= {mode, start: from, startMs: now, moves: 0, top};
        this._segment.moves++;
        this._segment.end = pos;
        this._segment.endMs = now;
        clearTimeout(this._restId);
        this._restId = setTimeout(() => this._finish(), REST_MS);
    }

    _abort() {
        clearTimeout(this._restId);
        this._restId = null;
        this._segment = null;
        this._pos = null;
    }

    _finish() {
        clearTimeout(this._restId);
        this._restId = null;
        const seg = this._segment;
        this._segment = null;
        const editor = this._editor;
        if (!seg || !editor || editor.document.isClosed || samePos(seg.start, seg.end))
            return;
        const known = seg.start.col !== null && seg.end.col !== null;
        const ranges = editor.visibleRanges;
        const window = ranges.length === 1
            ? {top: ranges[0].start.line, bottom: ranges[0].end.line} : null;
        let facts = {};
        try {
            facts = computeFacts(linesOf(editor.document), seg.start, seg.end, {
                window,
                ignorecase: vscode.workspace.getConfiguration('vim').get('ignorecase', true),
                relativeNumbers: editor.options.lineNumbers === vscode.TextEditorLineNumbersStyle.Relative,
            });
        } catch (e) {
            console.warn(`flowbuddy: motion facts failed: ${e}`);
        }
        const top = topLine(editor);
        if (seg.top !== null && top !== null)
            facts.view_scrolled = top - seg.top;
        const change = this._lastChange;
        if (change && change.document === editor.document && change.line === seg.end.line
            && seg.start.line !== seg.end.line)
            facts.last_change = true;
        const visits = rested(this._rests, seg.end.line, Date.now());
        if (visits)
            facts.revisits = visits;
        const doc = editor.document;
        if (textAllowed(this._link.editorConfig, doc.uri.scheme, doc.uri.fsPath, true))
            this._link.snippets.add(seg.endMs, doc.languageId, linesOf(doc), seg.end.line);
        this._link.send({
            type: 'motion',
            start_ms: seg.startMs,
            end_ms: seg.endMs,
            moves: seg.moves,
            mode: seg.mode,
            lines: seg.end.line - seg.start.line,
            cols: known ? seg.end.col - seg.start.col : 0,
            facts,
        });
    }
}

module.exports = {MotionTracker};
