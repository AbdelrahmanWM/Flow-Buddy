// Spike probe for #54: which VS Code events tell us the editor has focus,
// what vscodevim's mode is, and how the cursor moved.
//
// Writes one JSON line per event to $XDG_RUNTIME_DIR/flowbuddy-probe.jsonl
// (or /run/user/<uid>/ under snap). Only kinds, numbers and URI schemes are
// logged: never document text, file names or paths.

const vscode = require('vscode');
const fs = require('fs');
const os = require('os');
const path = require('path');

const t0 = Date.now();
let out;

function log(event, data = {}) {
    out.write(JSON.stringify({ms: Date.now() - t0, at: Date.now(), event, ...data}) + '\n');
}

const STYLES = Object.fromEntries(
    Object.entries(vscode.TextEditorCursorStyle).map(([k, v]) => [v, k]));
const KINDS = Object.fromEntries(
    Object.entries(vscode.TextEditorSelectionChangeKind).map(([k, v]) => [v, k]));

function tabKind(tab) {
    const i = tab?.input;
    if (!i) return 'none';
    if (i instanceof vscode.TabInputText) return 'text';
    if (i instanceof vscode.TabInputTextDiff) return 'diff';
    if (i instanceof vscode.TabInputTerminal) return 'terminal';
    if (i instanceof vscode.TabInputWebview) return `webview:${i.viewType}`;
    if (i instanceof vscode.TabInputCustom) return `custom:${i.viewType}`;
    if (i instanceof vscode.TabInputNotebook) return 'notebook';
    return 'other';
}

function activate(context) {
    const uid = process.getuid();
    const dir = fs.existsSync(`/run/user/${uid}`) ? `/run/user/${uid}` : os.tmpdir();
    out = fs.createWriteStream(path.join(dir, 'flowbuddy-probe.jsonl'), {flags: 'a', mode: 0o600});
    log('start', {
        xdg_runtime_dir: process.env.XDG_RUNTIME_DIR ?? null,
        snap: process.env.SNAP_NAME ?? null,
        vscode: vscode.version,
    });

    const vim = vscode.extensions.getExtension('vscodevim.vim');
    log('vscodevim', {
        installed: !!vim,
        active: vim?.isActive ?? false,
        exports: vim?.isActive ? Object.keys(vim.exports ?? {}) : null,
    });

    const sub = d => context.subscriptions.push(d);
    let last = null;

    sub(vscode.window.onDidChangeWindowState(s => log('window', {focused: s.focused, active: s.active})));
    sub(vscode.window.onDidChangeActiveTextEditor(e => log('active_editor', {
        present: !!e, scheme: e?.document.uri.scheme ?? null,
        cursor: e ? STYLES[e.options.cursorStyle] : null,
    })));
    sub(vscode.window.onDidChangeTextEditorSelection(e => {
        const a = e.selections[0].active;
        const d = last && last.editor === e.textEditor
            ? {lines: a.line - last.line, cols: a.character - last.col} : {};
        last = {editor: e.textEditor, line: a.line, col: a.character};
        log('selection', {
            kind: KINDS[e.kind] ?? null, n: e.selections.length,
            empty: e.selections[0].isEmpty, scheme: e.textEditor.document.uri.scheme, ...d,
        });
    }));
    sub(vscode.window.onDidChangeTextEditorOptions(e => log('options', {
        cursor: STYLES[e.options.cursorStyle], scheme: e.textEditor.document.uri.scheme,
    })));
    sub(vscode.window.onDidChangeTextEditorVisibleRanges(e => log('visible_ranges', {
        scheme: e.textEditor.document.uri.scheme,
    })));
    sub(vscode.workspace.onDidChangeTextDocument(e => {
        if (e.contentChanges.length)
            log('doc_change', {scheme: e.document.uri.scheme, changes: e.contentChanges.length});
    }));
    sub(vscode.window.onDidChangeActiveTerminal(t => log('active_terminal', {present: !!t})));
    sub(vscode.window.onDidChangeTerminalState(() => log('terminal_state')));
    if (vscode.window.onDidStartTerminalShellExecution)
        sub(vscode.window.onDidStartTerminalShellExecution(() => log('terminal_exec')));
    sub(vscode.window.tabGroups.onDidChangeTabs(() => log('tabs', {
        active: tabKind(vscode.window.tabGroups.activeTabGroup.activeTab),
    })));
    sub(vscode.window.tabGroups.onDidChangeTabGroups(() => log('tab_groups')));
    sub({dispose: () => out.end()});
}

module.exports = {activate, deactivate() {}};
