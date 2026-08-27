//! Render a scene to a PNG file (transparent background unless a board is
//! active) at full device resolution.

use std::os::unix::fs::OpenOptionsExt;
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
    // A screen annotation capture is private: 0600 on create, never the
    // umask default that leaves it world-readable.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    surf.write_to_png(&mut file)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
