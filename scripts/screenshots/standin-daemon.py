#!/usr/bin/env python3
"""A stand-in for flowbuddyd, for screenshots: it speaks the IPC protocol
on $XDG_RUNTIME_DIR/flowbuddy.sock and sends the tips it's told to, from
lines appended to $SHOT_DIR/daemon.cmd. It reads no keyboard.

    card      the paragraph tip, as a card on the desktop
    editor    the same tip, to the editor plugin (Vim's popup)
    inside    the "delete inside" tip, as a card

The tips are word for word what the real daemon sends for these habits.
"""

import json
import os
import socket
import threading
import time

RUNTIME = os.environ["XDG_RUNTIME_DIR"]
COMMANDS = os.path.join(os.environ["SHOT_DIR"], "daemon.cmd")

PARAGRAPH = {
    "rule_id": "motion.paragraph",
    "title": "Jump by paragraph",
    "text": "That took 7 keys. `}` gets there in 1, or `7j` in 2.",
    "explain": [
        "`}`: down to the next blank line (end of the paragraph)",
        "`7`: a count: do the next command that many times",
        "`j`: down one line",
    ],
}
INSIDE = {
    "rule_id": "motion.text-object",
    "title": "Delete inside",
    "text": "That took 6 keys. `di(` does it from anywhere inside, in 3.",
    "explain": [
        "`d`: delete",
        "`i(`: inside the ( ) brackets, from anywhere in them",
    ],
}

clients = {}  # socket -> hello
lock = threading.Lock()
seq = 0


def send(conn, msg):
    try:
        conn.sendall((json.dumps({"v": 0, **msg}) + "\n").encode())
    except OSError:
        pass


def nudge(tip, style="card", in_editor=False, ttl_ms=60000):
    global seq
    seq += 1
    return {
        "type": "nudge",
        "nudge_id": f"n-shot-{seq}",
        "ttl_ms": ttl_ms,
        "style": style,
        "in_editor": in_editor,
        **tip,
    }


def to(kind, msg):
    with lock:
        for conn, hello in clients.items():
            if hello.get("client") == kind:
                send(conn, msg)


def serve(conn):
    buf = b""
    while True:
        data = conn.recv(65536)
        if not data:
            break
        buf += data
        while b"\n" in buf:
            line, buf = buf.split(b"\n", 1)
            try:
                msg = json.loads(line)
            except ValueError:
                continue
            if msg.get("type") == "hello":
                with lock:
                    clients[conn] = msg
                send(conn, {"type": "welcome", "daemon_version": "0.1.0", "mode": "active"})
            elif msg.get("type") == "command":
                name = msg.get("name")
                data = {"shown": "card"} if name == "sample_tip" else []
                if name == "sample_tip":
                    to("gnome-extension", nudge(PARAGRAPH))
                send(conn, {"type": "result", "id": msg.get("id"), "data": data})
    with lock:
        clients.pop(conn, None)


def commands():
    seen = 0
    while True:
        time.sleep(0.1)
        try:
            lines = open(COMMANDS).read().splitlines()
        except OSError:
            continue
        for cmd in lines[seen:]:
            if cmd == "card":
                to("gnome-extension", nudge(PARAGRAPH, ttl_ms=4000))
            elif cmd == "inside":
                to("gnome-extension", nudge(INSIDE))
            elif cmd == "editor":
                to("editor", nudge(PARAGRAPH))
                to("gnome-extension", nudge(PARAGRAPH, in_editor=True))
        seen = len(lines)


def main():
    path = os.path.join(RUNTIME, "flowbuddy.sock")
    if os.path.exists(path):
        os.unlink(path)
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(path)
    os.chmod(path, 0o600)
    server.listen()
    threading.Thread(target=commands, daemon=True).start()
    while True:
        conn, _ = server.accept()
        threading.Thread(target=serve, args=(conn,), daemon=True).start()


if __name__ == "__main__":
    main()
