//! Render a scene to a PNG file (transparent background unless a board is
//! active) at full device resolution.

use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use anyhow::{Context, Result};

use crate::model::scene::Scene;
use crate::render::board::{self, BoardKind};
use crate::render::objects::paint_object;

pub fn export_png(
    path: &Path,
    logical: (u32, u32),
    scale: f64,
    board: BoardKind,
    board_opacity: f64,
    scene: &Scene,
) -> Result<()> {
    let bw = (logical.0 as f64 * scale).round() as i32;
    let bh = (logical.1 as f64 * scale).round() as i32;
    let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, bw, bh)
        .map_err(|e| anyhow::anyhow!("creating {bw}x{bh} surface: {e}"))?;
    {
        let cr = cairo::Context::new(&surf)?;
        cr.scale(scale, scale);
        board::paint(&cr, board, board_opacity);
        for obj in &scene.objects {
            paint_object(&cr, obj, 1.0);
        }
    }
    // Screen captures are private. OpenOptions.mode is only used when the
    // path is created; an existing world-readable file would keep those
    // bits across truncate. fchmod the opened fd so both cases are 0600.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restricting permissions on {}", path.display()))?;
    surf.write_to_png(&mut file)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_tightens_existing_world_readable_file_to_0600() {
        let path = std::env::temp_dir().join(format!(
            "annotate-export-perm-{}-{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&path, b"old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );

        let result = export_png(&path, (8, 8), 1.0, BoardKind::None, 1.0, &Scene::new());
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let _ = std::fs::remove_file(&path);
        result.unwrap();
        assert_eq!(mode, 0o600);
    }
}
