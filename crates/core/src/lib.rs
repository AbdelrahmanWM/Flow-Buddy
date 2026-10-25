//! Shared types and policy for Flow Buddy: events, context model, rules, coaching modes.

pub mod config;
pub mod context;
pub mod editor;
pub mod engine;
pub mod ipc;
pub mod key;
pub mod learning;
pub mod paths;
pub mod policy;
pub mod rule;
pub mod rules;
pub mod vim;

/// Crate version, shared by the daemon and CLI.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_set() {
        assert!(!VERSION.is_empty());
    }
}
