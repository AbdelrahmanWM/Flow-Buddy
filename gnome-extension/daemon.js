// Connection to flowbuddyd over its Unix socket (IPC protocol v0).
//
// Reconnects with backoff when the daemon restarts. All I/O is async so
// GNOME Shell's main loop never blocks on the daemon.

import GLib from 'gi://GLib';
import Gio from 'gi://Gio';

const PROTOCOL_VERSION = 0;
const MAX_BACKOFF_S = 30;

export class DaemonLink {
    constructor({version, onMessage, onConnected, onDisconnected}) {
        this._version = version;
        this._onMessage = onMessage;
        this._onConnected = onConnected;
        this._onDisconnected = onDisconnected;
        this._backoff = 1;
        this._queue = [];
        this._writing = false;
        this._conn = null;
        this._timeoutId = 0;
        this._cancellable = null;
    }

    get connected() {
        return this._conn !== null;
    }

    start() {
        this._connect();
    }

    stop() {
        if (this._timeoutId) {
            GLib.source_remove(this._timeoutId);
            this._timeoutId = 0;
        }
        this._cancellable?.cancel();
        this._close();
    }

    send(msg) {
        if (!this._conn)
            return;
        const line = `${JSON.stringify({v: PROTOCOL_VERSION, ...msg})}\n`;
        this._queue.push(new GLib.Bytes(new TextEncoder().encode(line)));
        this._flush();
    }

    _socketPath() {
        return GLib.getenv('FLOWBUDDY_SOCKET') ??
            GLib.build_filenamev([GLib.get_user_runtime_dir(), 'flowbuddy.sock']);
    }

    _connect() {
        this._cancellable = new Gio.Cancellable();
        const client = new Gio.SocketClient();
        const address = Gio.UnixSocketAddress.new(this._socketPath());
        client.connect_async(address, this._cancellable, (c, res) => {
            try {
                this._conn = c.connect_finish(res);
            } catch (e) {
                if (!e.matches(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                    this._scheduleReconnect();
                return;
            }
            this._backoff = 1;
            this._input = new Gio.DataInputStream({
                base_stream: this._conn.get_input_stream(),
            });
            this._output = this._conn.get_output_stream();
            this.send({
                type: 'hello',
                client: 'gnome-extension',
                client_version: this._version,
            });
            this._read();
            this._onConnected?.();
        });
    }

    _scheduleReconnect() {
        if (this._timeoutId)
            return;
        const delay = this._backoff;
        this._backoff = Math.min(this._backoff * 2, MAX_BACKOFF_S);
        this._timeoutId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, delay, () => {
            this._timeoutId = 0;
            this._connect();
            return GLib.SOURCE_REMOVE;
        });
    }

    _read() {
        this._input.read_line_async(GLib.PRIORITY_DEFAULT, this._cancellable, (stream, res) => {
            let line;
            try {
                [line] = stream.read_line_finish_utf8(res);
            } catch (e) {
                if (!e.matches(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                    this._lost();
                return;
            }
            if (line === null) {
                this._lost();
                return;
            }
            try {
                this._onMessage(JSON.parse(line));
            } catch (e) {
                console.warn(`flowbuddy: bad message from daemon: ${e}`);
            }
            this._read();
        });
    }

    // Gio streams allow one pending write at a time, so writes are queued.
    _flush() {
        if (this._writing || this._queue.length === 0 || !this._output)
            return;
        this._writing = true;
        const bytes = this._queue.shift();
        this._output.write_bytes_async(bytes, GLib.PRIORITY_DEFAULT, this._cancellable, (s, res) => {
            this._writing = false;
            try {
                s.write_bytes_finish(res);
            } catch (e) {
                if (!e.matches(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                    this._lost();
                return;
            }
            this._flush();
        });
    }

    _lost() {
        this._close();
        this._onDisconnected?.();
        this._scheduleReconnect();
    }

    _close() {
        this._queue = [];
        this._writing = false;
        this._input = null;
        this._output = null;
        try {
            this._conn?.close(null);
        } catch {
            // Already closed.
        }
        this._conn = null;
    }
}
