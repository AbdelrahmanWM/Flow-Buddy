// vscodevim's real mode, read from the cursor style it sets.
//
// vscodevim exports no API and its status bar can't be read, but it sets
// the editor's cursor style on every mode change: the user's
// `vim.cursorStylePerMode.<mode>` if set, else a fixed style per mode
// (ModeHandler.updateView in vscodevim 1.32). Where two modes share a
// style, the selection tells Visual apart from the rest; anything still
// ambiguous is reported as `other` rather than guessed.

'use strict';

const vscode = require('vscode');

const VISUAL = ['visual', 'visual_line', 'visual_block'];

// Our mode, vscodevim's key in vim.cursorStylePerMode (its Mode name in
// lower case) and the style it falls back to. Insert falls back to
// `editor.cursorStyle` (null here).
const VIM_MODES = [
    ['normal', 'normal', 'block'],
    ['insert', 'insert', null],
    ['visual', 'visual', 'line-thin'],
    ['visual_line', 'visualline', 'line-thin'],
    ['visual_block', 'visualblock', 'line-thin'],
    ['replace', 'replace', 'underline'],
    ['cmdline', 'searchinprogressmode', 'underline-thin'],
    ['cmdline', 'commandlineinprogress', 'underline-thin'],
    ['op_pending', 'operatorpendingmode', 'underline-thin'],
];

function styleOf(name) {
    const S = vscode.TextEditorCursorStyle;
    return {
        'line': S.Line, 'block': S.Block, 'underline': S.Underline,
        'line-thin': S.LineThin, 'block-outline': S.BlockOutline,
        'underline-thin': S.UnderlineThin,
    }[name];
}

class ModeTracker {
    constructor(link, onChange) {
        this._link = link;
        this._onChange = onChange;
        this._mode = null;
        this._sent = null;
        this._loadConfig();
    }

    get mode() {
        return this._mode;
    }

    enable(subscriptions) {
        const w = vscode.window;
        const sub = d => subscriptions.push(d);
        sub(w.onDidChangeTextEditorOptions(e => {
            if (e.textEditor === w.activeTextEditor)
                this.update();
        }));
        sub(w.onDidChangeTextEditorSelection(e => {
            if (e.textEditor === w.activeTextEditor)
                this.update();
        }));
        sub(w.onDidChangeActiveTextEditor(() => this.update()));
        sub(vscode.extensions.onDidChange(() => this._loadConfig()));
        sub(vscode.workspace.onDidChangeConfiguration(e => {
            if (e.affectsConfiguration('vim') || e.affectsConfiguration('editor.cursorStyle'))
                this._loadConfig();
        }));
        this.update();
    }

    resend() {
        this._sent = null;
        this._send();
    }

    update() {
        const editor = vscode.window.activeTextEditor;
        if (!editor)
            return;
        const mode = this._vim ? this._detect(editor) : 'other';
        if (mode === this._mode)
            return;
        const previous = this._mode;
        this._mode = mode;
        this._onChange?.(previous, mode);
        this._send();
    }

    _send() {
        if (this._mode === null || this._mode === this._sent)
            return;
        this._sent = this._mode;
        this._link.send({type: 'editor_mode', mode: this._mode});
    }

    _loadConfig() {
        const vim = vscode.workspace.getConfiguration('vim');
        this._vim = !!vscode.extensions.getExtension('vscodevim.vim') &&
            !vim.get('disableExtension', false);
        const perMode = vim.get('cursorStylePerMode') ?? {};
        const editorStyle = vscode.workspace.getConfiguration('editor').get('cursorStyle', 'line');
        this._styles = VIM_MODES.map(([mode, key, fallback]) => {
            // vscodevim ignores unknown names and falls back, like this.
            const style = styleOf(perMode[key]) ?? styleOf(fallback ?? editorStyle) ??
                vscode.TextEditorCursorStyle.Block;
            return [mode, style];
        });
        this.update();
    }

    _detect(editor) {
        const style = editor.options.cursorStyle;
        let modes = [...new Set(this._styles.filter(([, s]) => s === style).map(([m]) => m))];
        // vscodevim always selects something in Visual modes (except on an
        // empty line) and never in Normal mode.
        const selected = !editor.selections.every(s => s.isEmpty);
        const emptyLine = editor.document.lineAt(editor.selection.active.line).text.length === 0;
        modes = modes.filter(m => {
            if (VISUAL.includes(m))
                return selected || emptyLine;
            if (m === 'normal')
                return !selected;
            return true;
        });
        if (modes.length === 1)
            return modes[0];
        if (modes.length === 0 || !modes.every(m => VISUAL.includes(m)))
            return 'other';
        return this._visualKind(editor, modes);
    }

    // Visual, Visual Line and Visual Block share a style by default. Tell
    // them apart by the selection's shape, as vscodevim draws it.
    _visualKind(editor, modes) {
        const sels = editor.selections;
        if (sels.length > 1) {
            const lines = sels.map(s => s.start.line).sort((a, b) => a - b);
            const block = sels.every(s => s.isSingleLine) &&
                lines.every((l, i) => i === 0 || l === lines[i - 1] + 1);
            return block && modes.includes('visual_block') ? 'visual_block' : 'other';
        }
        const s = sels[0];
        if (s.isEmpty)
            return modes.includes(this._mode) ? this._mode : 'other';
        const wholeLines = s.start.character === 0 &&
            s.end.character === editor.document.lineAt(s.end.line).text.length;
        if (wholeLines && modes.includes('visual_line'))
            return 'visual_line';
        if (!wholeLines && modes.includes('visual'))
            return 'visual';
        return 'other';
    }
}

module.exports = {ModeTracker};
