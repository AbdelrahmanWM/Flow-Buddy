# JSON from the command line

Add `--json` to these commands for output a program can read. The settings window is built on it, and so can anything else be. Every object has a `version` (1 now); fields may be added, but not renamed or removed within a version.

Where there's no database yet (the daemon hasn't run), commands that read it print `{"version": 1, "nothing_yet": true}`.

## `flowbuddy config list --json`

Every setting, then every rule's two settings.

```json
{"version": 1, "settings": [
  {"key": "learn.holdout", "group": "Learning", "basic": false,
   "what": "The share of tips held back at random…",
   "kind": {"type": "float", "min": 0.0, "max": 0.5},
   "value": 0.1, "default": 0.1, "notice": null},
  {"key": "rules.vim.repeat-jk.threshold", "group": "Rules", …,
   "rule": {"id": "vim.repeat-jk", "title": "…", "description": "…", "default_threshold": 5}}
]}
```

`kind.type` is one of `bool`, `int` and `float` (with `min` and `max`), `choice` (with `choices`), `time` (`HH:MM`), `time-range` (`HH:MM-HH:MM`), `text` and `list`. `value` is `null` when not set. `more` is a longer explanation: when you'd change it, and what it's easily confused with (`flowbuddy config explain <key>` prints it). `notice`, when there is one, says what turning the setting on means for your privacy or data: show it before turning it on. Change a setting with `flowbuddy config set <key> <value>` (lists comma-separated); it exits non-zero, with what's allowed, if the value is wrong.

## `flowbuddy doctor --json`

`{"version": 1, "ok": false, "checks": [{"ok": true, "what": "…", "fix": null}, …]}`. `fix` says what to do when `ok` is false. It exits 1 when any check fails, as without `--json`.

## `flowbuddy learn --json`

`{"version": 1, "latest": {"id", "text", "learned"} or null, "items": [{"id", "level", "keys", "what", "state", "tile", "uses", "days_idle", "half_life_days", "mastery", "recall"}, …]}`. `state` is `known`, `ready` (one step up from what you know) or `later`. `tile` is how the skill map colours it: `known`, `slipping` (known, but under even odds you'd recall it today), `learning` (used, not known yet), `ready` or `later`. `days_idle` is days since you last used it, or `null`. `half_life_days` is how many days the chance of recalling it takes to halve, fitted to your use (`null` without evidence). `mastery` and `recall` are percentages, or `null` without evidence.

## `flowbuddy learn --coverage --json`

`{"version": 1, "modes": {"normal": {"commands", "coached", "taught", "starting", "skipped", "missing"}, …}, "missing": [{"mode", "keys", "what"}, …]}`.

## `flowbuddy rules --json`

`{"version": 1, "rules": [{"id", "description", "enabled", "muted", "threshold", "raised"}, …]}`, from the running daemon.

## `flowbuddy summary --json`, `flowbuddy learn --report --json`

These are written to be read: `{"version": 1, "text": "…"}`, the same text as without `--json`.
