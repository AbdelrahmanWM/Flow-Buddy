# Writing rules

A rule spots one inefficient habit and knows what the better way looks like. Rules live in `crates/core/src/rules/`, grouped into packs (`vim.rs`, `bash.rs`). This guide walks through adding one.

## What a rule sees

Rules never see raw keys. The mode tracker (`crates/core/src/vim.rs`) turns keys into **steps**:

```rust
pub struct Step {
    pub before: VimMode,        // Normal, Insert, Visual…, Cmdline, OpPending
    pub after: VimMode,
    pub confident: bool,        // the engine only passes confident steps
    pub event: Option<VimEvent>,
    pub repeat: bool,           // key auto-repeat
    pub at_ms: u64,
}

pub enum VimEvent {
    Command(Command),           // a complete Normal/Visual command: j, 5j, dd, ciw, fx
    Insert(InsertKey),          // Text, Backspace, CtrlW, CtrlU, CtrlR, CtrlO, Arrow, Enter, Other
    Cmdline { kind, text },     // a finished `:` command or `/` search
    Escape,
}
```

- In Insert mode a rule learns *that* text was typed (`InsertKey::Text`), never *what* was typed.
- Operator commands such as `dw` complete from `OpPending`, so match on `before` being Normal **or** OpPending (see `command()` in `vim.rs`).
- A step with `event: None` is a half-typed command, such as the `d` of `dd`.

## The `Rule` trait

```rust
pub trait Rule: Send {
    fn id(&self) -> &'static str;              // "pack.name", stable forever
    fn description(&self) -> &'static str;     // one line, shown in `flowbuddy rules`
    fn contexts(&self) -> &'static [ContextKind];
    fn threshold(&self) -> Option<u32> { None }
    fn set_threshold(&mut self, _n: u32) {}
    fn on_step(&mut self, step: &Step, out: &mut Vec<RuleEvent>);
    fn tick(&mut self, _now_ms: u64, _out: &mut Vec<RuleEvent>) {}
    fn reset(&mut self);
}
```

A rule emits two kinds of event:

- `RuleEvent::Finding(Finding { rule_id, title, text, shape, mode, at_ms })` when it spots the habit.
- `RuleEvent::Adoption { rule_id, at_ms }` when the better way was used. **Every rule should detect adoption.** That is how improvement is measured and how learning stages move.

## The easy way: `RepeatRule`

Most habits have the form "the same small thing many times in a row". For those, fill in a `RepeatSpec`. This is the `x` rule:

```rust
pub fn repeat_x() -> RepeatRule {
    RepeatRule::new(RepeatSpec {
        id: "vim.repeat-x",
        description: "Deleting character by character with x",
        title: "Delete by word",
        contexts: VIM,
        threshold: 4,
        // What counts as one unit of the streak (None = not part of it).
        unit: |s| uncounted(s, &['x']).map(|_| "x"),
        // What the better way looks like.
        adopted: |s| command(s).is_some_and(|c| matches!(c.head(), Some('D' | 'C')) /* … */),
        // The suggestion; only static text and numbers.
        message: |_, n| format!("You pressed x {n} times. Try dw, de or D."),
    })
}
```

`RepeatRule` handles everything else for you:
- it counts streaks
- it breaks them on any other event or on a gap longer than 1.5 s
- it reports when a streak ends, including after a pause (`tick`)
- it builds the shape (`x×5`)

For anything else, implement `Rule` directly. `WriteThenQuit` in `vim.rs` is a small example.

## Motion rules: when an editor reports

With the Vim plugin or the VS Code companion running, the editor reports each movement with facts about where it ended (`Motion` and `MotionFacts` in `crates/core/src/editor.rs`). The daemon adds how many keys you pressed for it, and rules get `on_motion(&MotionEvent)` with both.

`crates/core/src/motion.rs` turns the facts into the cheapest exact plan:
- the best way to the end line (a count, `{`/`}`, `H/M/L`, `gg/G`);
- plus the best way along it (`w/b/e/W/B`, `f/t` with `;` or a count, `0 ^ $`);
- or a single motion to the whole position (`%`, `*`/`#`, Ctrl-o).

There is one `MotionRule` per kind of plan (`motion.paragraph`, `motion.find`, …). It nudges when the plan saves at least its threshold in keys (default 3), and counts an adoption when you already took the short way.

To teach it a new motion:
1. Add the fact to `MotionFacts`, and compute it in both plugins: `vim-plugin/autoload/flowbuddy.vim` and `vscode-extension/facts.js`.
2. Turn the fact into a `Part` or `Plan` in `motion.rs`, with its key cost.
3. Add a table test in `motion.rs`.

Rules that only guess from keys and that a motion rule does better, such as `vim.repeat-jk`, are marked `.replaced_by_editor()`. They step aside while an editor reports motions, so one habit gets one nudge.

## The skill tree

Rules and the daily "something new" share one tree: the items in `crates/core/src/curriculum.rs`. Each item can need others first (`REQUIRES`, e.g. `ci(` needs `ciw`), and each rule names the items its advice teaches (`RULE_SKILLS`, main one first). A tip is only shown when you're ready for its main skill, meaning you use it already or use everything it builds on. Motion plans and text objects you're not ready for give way to the next best one. A test fails if a rule is missing from `RULE_SKILLS`.

When several rules fire on the same keys in one go, the engine keeps one tip: the first rule in `rules::PRECEDENCE`, which lists the rules whose advice covers more of the gesture. The runner-up is added to it ("Also: …") if both are short. List a new rule there if it covers more of a gesture than an existing rule does.

## Then

1. Add the constructor to the pack's `all()` so the engine runs it, and the rule to `RULE_SKILLS`.
2. Write tests next to the rule with the helpers in `rules::testing`:

   ```rust
   let ev = run(Box::new(repeat_x()), Flavor::Vim, VimMode::Normal, "xxxxx<Esc>");
   assert_eq!(findings(&ev)[0].shape.to_string(), "x×5");
   ```

   Keys use Vim notation: `<Esc>`, `<CR>`, `<BS>`, `<C-w>`, `<Up>`. Every rule needs:
   - a positive test (the habit is found)
   - a negative test (short streaks, the right mode, arguments like `fj`)
   - an adoption test
3. Run `cargo test` and `flowbuddy rules` to see it listed.

## Privacy checklist

The memory stores what rules emit, so these points are enforced by review:

- [ ] `Shape` is built with `Shape::repeat(token, n)` or `Shape::fixed(text)` from `'static` strings. Never build it from typed input. The types already make this hard, so don't work around them.
- [ ] `Finding.text` is built from static text and numbers only.
- [ ] The rule doesn't keep text from `VimEvent::Cmdline` or the keys of commands beyond what it needs to match. Nothing a rule holds should outlive `reset()`.
- [ ] Tests use hand-written key sequences, never recordings of real typing.
- [ ] `findings_never_contain_typed_text` in `rules/vim.rs` still passes. Add your pack to a similar test.
