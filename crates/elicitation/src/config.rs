//! Minimal server configuration (spec §8): the state directory.
//!
//! Resolution order for `state_dir`:
//!  1. explicit constructor argument (used by tests),
//!  2. `ELICITATION_STATE_DIR` env var,
//!  3. `$XDG_DATA_HOME/elicitation` or `$HOME/.local/share/elicitation`,
//!  4. `./elicitation-state` as a last resort.

use std::path::PathBuf;

/// Environment variable overriding the state directory.
pub const STATE_DIR_ENV: &str = "ELICITATION_STATE_DIR";

/// Resolve the on-disk state directory from the environment (spec §8).
pub fn resolve_state_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(STATE_DIR_ENV) {
        return PathBuf::from(dir);
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(xdg).join("elicitation");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("elicitation");
    }
    PathBuf::from("elicitation-state")
}
