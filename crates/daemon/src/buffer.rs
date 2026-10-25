//! The only place recent keys are kept.
//!
//! Privacy guarantees enforced here (see docs/privacy.md):
//! - bounded, in memory only: no `Serialize`, no `Debug`, no `Display`;
//! - only filled while a supported context is focused;
//! - cleared on context change, pause, mode `off` and shutdown.

use std::collections::VecDeque;

use flowbuddy_core::context::ContextKind;
use flowbuddy_core::key::KeyStroke;

pub const CAPACITY: usize = 64;

/// Why the buffer was cleared; useful for tests and debug logs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClearReason {
    ContextChanged,
    Paused,
    ModeOff,
    Shutdown,
}

pub struct KeyBuffer {
    keys: VecDeque<KeyStroke>,
}

impl Default for KeyBuffer {
    fn default() -> Self {
        KeyBuffer {
            keys: VecDeque::with_capacity(CAPACITY),
        }
    }
}

impl KeyBuffer {
    /// Push a key if the context is supported. Returns whether it was kept.
    pub fn push(&mut self, stroke: KeyStroke, context: ContextKind) -> bool {
        if !context.is_supported() {
            return false;
        }
        if self.keys.len() == CAPACITY {
            self.keys.pop_front();
        }
        self.keys.push_back(stroke);
        true
    }

    pub fn clear(&mut self, reason: ClearReason) {
        if !self.keys.is_empty() {
            tracing::debug!(?reason, "key buffer cleared");
        }
        self.keys.clear();
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Most recent keys, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &KeyStroke> {
        self.keys.iter()
    }
}

impl Drop for KeyBuffer {
    fn drop(&mut self) {
        self.clear(ClearReason::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowbuddy_core::key::Key;

    fn stroke(c: char) -> KeyStroke {
        KeyStroke {
            key: Key::ch(c),
            repeat: false,
            at_ms: 0,
        }
    }

    #[test]
    fn unsupported_context_keeps_nothing() {
        let mut b = KeyBuffer::default();
        assert!(!b.push(stroke('p'), ContextKind::Unsupported));
        assert!(b.is_empty());
    }

    #[test]
    fn bounded() {
        let mut b = KeyBuffer::default();
        for _ in 0..CAPACITY * 2 {
            b.push(stroke('j'), ContextKind::VsCodeVim);
        }
        assert_eq!(b.len(), CAPACITY);
    }

    #[test]
    fn cleared_for_every_reason() {
        for reason in [
            ClearReason::ContextChanged,
            ClearReason::Paused,
            ClearReason::ModeOff,
            ClearReason::Shutdown,
        ] {
            let mut b = KeyBuffer::default();
            b.push(stroke('j'), ContextKind::TerminalVim);
            b.clear(reason);
            assert!(b.is_empty(), "{reason:?}");
        }
    }

    /// The buffer type must never be printable or serializable.
    #[test]
    fn no_debug_display_or_serde_on_buffer() {
        let src = include_str!("buffer.rs");
        let def = src.split("pub struct KeyBuffer").next().unwrap();
        let attrs = def.lines().rev().take(3).collect::<Vec<_>>().join("\n");
        assert!(
            !attrs.contains("derive"),
            "KeyBuffer must not derive traits"
        );
        let forbidden = [
            "Debug for KeyBuffer",
            "Display for KeyBuffer",
            "Serialize for KeyBuffer",
        ];
        for f in forbidden {
            assert!(!src.contains(&format!("impl {f}")), "{f}");
        }
    }
}
