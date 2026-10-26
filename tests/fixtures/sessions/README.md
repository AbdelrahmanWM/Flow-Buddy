# Recorded sessions

Each `*.jsonl` is a session the rules are checked against, and the `*.expect.json` next to it says how many findings per rule are right. `cargo test -p flowbuddy-core --test sessions` replays them all and fails on any finding that isn't expected (a false positive) or expected finding that's gone (a miss).

To add one of your own:

```sh
flowbuddy record start
# ...work for a while...
flowbuddy record stop                         # prints the file
flowbuddy record check FILE > counts.json     # what the rules find now
```

Look through the counts, keep the ones that are right, and save them as `NAME.expect.json` next to `NAME.jsonl` here.

A recording has what the rules see and nothing typed: commands such as `5j` or `ciw`, Insert-mode keys only as their kind (`text`, `backspace`), `x` (or `1`) for the character after `f`, `t` or `r`, no search patterns, and `:` commands with every letter and digit after the name replaced (`:%s/x/x/x`). Read it before you share it.
