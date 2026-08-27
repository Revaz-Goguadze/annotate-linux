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
    use crate::model::geom::Point;
    use crate::model::object::{Object, ObjectKind, Style};
    use crate::util::color::Rgba;

    fn scene_with_a_stroke() -> Scene {
        let mut scene = Scene::new();
        let id = scene.alloc_id();
        scene.objects.push(Object::new(
            id,
            ObjectKind::Line { a: Point::new(10.0, 10.0), b: Point::new(90.0, 90.0) },
            Style { stroke: Rgba::new(1.0, 0.0, 0.0, 1.0), width: 6.0, group_alpha: 1.0 },
        ));
        scene
    }

    /// PNG signature + IHDR width/height, straight out of the file bytes.
    fn png_size(bytes: &[u8]) -> (u32, u32) {
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
        assert_eq!(&bytes[12..16], b"IHDR");
        let be = |i: usize| u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap());
        (be(16), be(20))
    }

    fn tmp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("annotate-export-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("out.png")
    }

    #[test]
    fn writes_a_png_at_device_resolution() {
        let path = tmp_path("scaled");
        export_png(&path, (100, 60), 1.6, BoardKind::None, 1.0, &scene_with_a_stroke()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(png_size(&bytes), (160, 96), "logical size times the scale");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn scene_without_a_board_exports_transparent_background() {
        let path = tmp_path("transparent");
        export_png(&path, (40, 40), 1.0, BoardKind::None, 1.0, &Scene::new()).unwrap();
        let empty = std::fs::metadata(&path).unwrap().len();

        export_png(&path, (40, 40), 1.0, BoardKind::None, 1.0, &scene_with_a_stroke()).unwrap();
        let drawn = std::fs::metadata(&path).unwrap().len();
        assert!(drawn > empty, "a stroke must add pixel data ({drawn} vs {empty} bytes)");

        export_png(&path, (40, 40), 1.0, BoardKind::White, 1.0, &Scene::new()).unwrap();
        let board = std::fs::metadata(&path).unwrap().len();
        assert!(board > empty, "a board fills the frame ({board} vs {empty} bytes)");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn unwritable_path_reports_the_target() {
        let err = export_png(
            Path::new("/nonexistent-dir-for-tests/out.png"),
            (10, 10),
            1.0,
            BoardKind::None,
            1.0,
            &Scene::new(),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("/nonexistent-dir-for-tests/out.png"),
            "error should name the file: {err}"
        );
    }

    #[test]
    fn zero_sized_export_fails_cleanly() {
        let path = tmp_path("empty-surface");
        let err = export_png(&path, (0, 0), 1.0, BoardKind::None, 1.0, &Scene::new()).unwrap_err();
        assert!(
            err.to_string().contains(&path.display().to_string()),
            "error should name the file: {err}"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

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
