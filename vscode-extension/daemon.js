// Connection to flowbuddyd over its Unix socket (IPC protocol v0).
//
// Mirrors gnome-extension/daemon.js: hello first, reconnect with backoff
// when the daemon restarts, and silently do nothing while it isn't there.

'use strict';

const net = require('net');
const {SnippetRing} = require('./textguard');

const PROTOCOL_VERSION = 0;
const MAX_BACKOFF_S = 30;

// $XDG_RUNTIME_DIR isn't trusted: the VS Code snap can rewrite it.
function socketPath() {
    return process.env.FLOWBUDDY_SOCKET || `/run/user/${process.getuid()}/flowbuddy.sock`;
}

class DaemonLink {
    constructor({version, onConnected}) {
        this._version = version;
        this._onConnected = onConnected;
        // [text] settings from the daemon; see textguard.js.
        this.editorConfig = null;
        this.snippets = new SnippetRing();
        this._backoff = 1;
        this._sock = null;
        this._ready = false;
        this._timeoutId = null;
        this._stopped = true;
    }

    get connected() {
        return this._ready;
    }

    start() {
        this._stopped = false;
        this._connect();
    }

    stop() {
        this._stopped = true;
        clearTimeout(this._timeoutId);
        this._timeoutId = null;
        this._sock?.destroy();
        this._sock = null;
        this._ready = false;
    }

    send(msg) {
        if (!this._ready)
            return;
        this._sock.write(`${JSON.stringify({v: PROTOCOL_VERSION, ...msg})}\n`);
    }

    _connect() {
        const sock = net.createConnection(socketPath());
        this._sock = sock;
        sock.setEncoding('utf8');
        let buffer = '';
        sock.on('connect', () => {
            this._backoff = 1;
            this._ready = true;
            this.send({
                type: 'hello',
                client: 'editor',
                client_version: this._version,
                editor: 'vscode',
                // The daemon walks up from here to the focused window's pid.
                pid: process.pid,
            });
            this._onConnected?.();
        });
        sock.on('data', chunk => {
            buffer += chunk;
            let i;
            while ((i = buffer.indexOf('\n')) !== -1) {
                this._handle(buffer.slice(0, i));
                buffer = buffer.slice(i + 1);
            }
        });
        // 'close' always follows, and reconnects.
        sock.on('error', () => {});
        sock.on('close', () => {
            if (this._sock !== sock)
                return;
            this._sock = null;
            this._ready = false;
            // Whatever was allowed, it's off until the daemon says so again.
            this.editorConfig = null;
            this.snippets.clear();
            if (!this._stopped)
                this._scheduleReconnect();
        });
    }

    _handle(line) {
        let msg;
        try {
            msg = JSON.parse(line);
        } catch (e) {
            console.warn(`flowbuddy: bad message from daemon: ${e}`);
            return;
        }
        if (msg.type === 'editor_config') {
            this.editorConfig = msg;
            if (!msg.snippets)
                this.snippets.clear();
        }
        if (msg.type === 'snippet_request') {
            const snippets = this.editorConfig?.snippets ? this.snippets.pick(msg.times ?? []) : [];
            this.send({type: 'snippets', id: msg.id, snippets});
        }
        // Errors mean this extension sent something wrong.
        if (msg.type === 'error')
            console.warn(`flowbuddy: daemon error ${msg.code}: ${msg.message}`);
    }

    _scheduleReconnect() {
        if (this._timeoutId)
            return;
        const delay = this._backoff;
        this._backoff = Math.min(this._backoff * 2, MAX_BACKOFF_S);
        this._timeoutId = setTimeout(() => {
            this._timeoutId = null;
            if (!this._stopped)
                this._connect();
        }, delay * 1000);
    }
}

module.exports = {DaemonLink};
