//! Shared types and policy for Flow Buddy: events, context model, rules, coaching modes.

pub mod context;
pub mod key;

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
