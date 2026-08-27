use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::{Context, Result};

const APP: &str = "annotate-linux";

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

/// `$XDG_CONFIG_HOME/annotate-linux` (default `~/.config/annotate-linux`)
pub fn config_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home().join(".config"))
        .join(APP)
}

/// `$XDG_RUNTIME_DIR/annotate-linux`, created 0700. Private scratch space:
/// exported screen content must not land in a world-readable /tmp, where
/// another local user can read it or pre-plant the path as a symlink.
pub fn runtime_dir() -> Result<PathBuf> {
    let base = std::env::var("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
    let dir = PathBuf::from(base).join(APP);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("securing {}", dir.display()))?;
    Ok(dir)
}

/// `$XDG_STATE_HOME/annotate-linux` (default `~/.local/state/annotate-linux`)
pub fn state_dir() -> PathBuf {
    std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home().join(".local/state"))
        .join(APP)
}
