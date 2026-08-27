//! Shared TOML file handling for config.toml and state.toml: a missing
//! file is not an error, and writes go through tmp + rename so a crash
//! mid-write cannot leave a truncated file behind.

use std::path::Path;

use anyhow::{Context, Result};

/// The file's contents, or `None` when it does not exist.
pub fn read_opt(path: &Path) -> std::io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Create the parent directory and write `contents` atomically.
pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let dir = path.parent().context("path has no parent directory")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, contents).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("renaming into {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_reads_as_none() {
        assert_eq!(read_opt(Path::new("/nonexistent/annotate-test/x.toml")).unwrap(), None);
    }

    #[test]
    fn atomic_write_roundtrips_and_leaves_no_tmp() {
        let dir = std::env::temp_dir().join(format!("annotate-toml-{}", std::process::id()));
        let path = dir.join("state.toml");
        write_atomic(&path, "width = 4.0\n").unwrap();
        assert_eq!(read_opt(&path).unwrap().as_deref(), Some("width = 4.0\n"));
        assert!(!path.with_extension("toml.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
