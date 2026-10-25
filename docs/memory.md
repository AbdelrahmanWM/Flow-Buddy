# The mistake memory

Flow Buddy remembers which inefficient habits you have and how each one is changing over time. Coaching uses this memory: it stops nudging habits you've mastered and nudges again when one comes back. A future AI layer will reason over it too, which is why its format is stable and versioned.

It holds **aggregates and abstract shapes only**. See [privacy.md](privacy.md).

## What is stored

All of it lives in `~/.local/share/flowbuddy/flowbuddy.db` (SQLite).

| Table | One row per | Holds |
|---|---|---|
| `daily_stats` | day × context × rule | hits, nudges, dismissed, not helpful, adoptions |
| `activity` | day × context | minutes you were typing in that context |
| `patterns` | rule × context | first/last seen, totals, learning stage and since when |
| `episodes` | finding | hour bucket, rule, context, mode, shape |

A **shape** is an abstract description of one occurrence, e.g. `j×7`, `x×5`, `BS×12` or `:w then :q`. Shapes are built in code from fixed strings and numbers (`Shape::repeat("j", 7)`). There is no way for typed text to become a shape.

**Retention:** episodes are deleted after 90 days (`[memory] episode_retention_days`). Pattern records and daily stats are kept until you delete them.

## Learning stages

```
new ──nudged──▶ nudged ──▶ improving ──▶ mastered
                  ▲            │  ▲          │
                  └── regressed ◀─┴──────────┘
```

Stages are recomputed every hour from the last 7 and 14 days.

| Move | When (defaults, `[learning]` in config.toml) |
|---|---|
| new → nudged | the first nudge was shown |
| → improving | in the last 7 days, the better way was used ≥ 50% of the time (at least 4 events) |
| → mastered | over 14 days, ≥ 85% better way (at least 10 events); **or**, while improving, no hits at all in 14 days with at least 2 h of activity |
| improving → regressed | ≥ 5 hits in 7 days and under 30% better way |
| mastered → regressed | ≥ 5 hits in 7 days and under 50% better way |

Mastered patterns are never nudged. Regressed ones get the highest priority in balanced mode.

## Commands

```sh
flowbuddy memory show                    # every pattern, stage and history
flowbuddy memory export                  # the JSON below
flowbuddy memory forget vim.repeat-jk    # delete one rule's history
flowbuddy memory forget --all            # delete everything
```

## Export format, version 1

`flowbuddy memory export` prints:

```json
{
  "format": "flowbuddy.memory",
  "version": 1,
  "generated_at": 1761328800,
  "summary": {
    "patterns": 2,
    "mastered": 1,
    "improving": 0,
    "regressed": 0,
    "tracking_since": 1760000000
  },
  "patterns": [
    {
      "rule": "vim.repeat-jk",
      "context": "vscode-vim",
      "description": "Pressing j or k many times instead of a count or a bigger motion",
      "stage": "nudged",
      "stage_since": 1761300000,
      "first_seen": 1760000000,
      "last_seen": 1761320000,
      "total": { "hits": 42, "adoptions": 17 },
      "last_7_days": { "hits": 9, "adoptions": 8 },
      "nudges": 6,
      "dismissed": 1,
      "false_positives": 0,
      "recent_shapes": ["j×7", "j×12", "k×6"]
    }
  ]
}
```

| Field | Meaning |
|---|---|
| `format`, `version` | Always `"flowbuddy.memory"` and `1` for this layout. Incompatible changes bump `version`. |
| `generated_at`, `*_seen`, `stage_since`, `tracking_since` | Unix seconds |
| `context` | `vscode-vim`, `terminal-vim` or `bash-vi` |
| `stage` | `new`, `nudged`, `improving`, `mastered` or `regressed` |
| `total`, `last_7_days` | `hits`: the habit happened; `adoptions`: the better way was used instead |
| `false_positives` | nudges you marked as not helpful |
| `recent_shapes` | the 5 most recent shapes, newest first |

**Ordering:** patterns are listed most relevant first, so a consumer can stop reading early. Regressed come first, then nudged, new, improving and mastered. Within a stage, the most hits in the last 7 days come first.

**Size budget:** at most 20 patterns and 5 shapes each. A typical export is a few KB, small enough to pass to a model as context.

The format is pinned by a golden-file test (`crates/store/tests/golden_export.json`).

## What will never be in the memory or the export

- Typed text of any kind: Insert-mode text, shell commands, `:` commands, searches
- Raw key sequences
- File names, paths, window titles, command arguments
- Anything from apps other than the supported contexts

If a future feature sends the export off the machine (e.g. to an AI model), it will be opt-in and off by default, and it will be documented in [privacy.md](privacy.md) before it ships.
