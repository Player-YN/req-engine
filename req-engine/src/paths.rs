//! Portable home-directory resolution for REQ_ENGINE_HOME.

use std::path::PathBuf;

/// Default data home: `%USERPROFILE%\.req-engine` on Windows, `~/.req-engine` elsewhere.
pub fn default_home() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".req-engine")
}

/// Resolve engine home from `REQ_ENGINE_HOME` or the platform default.
pub fn resolve_home() -> PathBuf {
    std::env::var_os("REQ_ENGINE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(default_home)
}

pub fn db_path(home: &std::path::Path) -> PathBuf {
    home.join("req-engine.sqlite")
}

pub fn tokens_path(home: &std::path::Path) -> PathBuf {
    home.join("tokens.txt")
}

pub fn pair_codes_path(home: &std::path::Path) -> PathBuf {
    home.join("pair-codes.json")
}
