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

## Then

1. Add the constructor to the pack's `all()` so the engine runs it.
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
