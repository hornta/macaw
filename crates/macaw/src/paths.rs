//! Where Macaw keeps its files.

use std::path::PathBuf;

fn known(var: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// `%APPDATA%\Macaw\config.toml` (roams with the user profile).
pub fn config_file() -> PathBuf {
    known("APPDATA").join("Macaw").join("config.toml")
}

/// `%LOCALAPPDATA%\Macaw\macaw.log`.
pub fn log_file() -> PathBuf {
    known("LOCALAPPDATA").join("Macaw").join("macaw.log")
}
