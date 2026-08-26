use std::path::PathBuf;

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

/// `$XDG_STATE_HOME/annotate-linux` (default `~/.local/state/annotate-linux`)
pub fn state_dir() -> PathBuf {
    std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home().join(".local/state"))
        .join(APP)
}
