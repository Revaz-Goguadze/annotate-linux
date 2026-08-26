//! The shm ↔ cairo bridge. The only `unsafe` in the crate lives here,
//! behind a scoped helper that keeps the cairo surface from outliving the
//! borrowed canvas.

use anyhow::{Result, ensure};

/// Run `f` with a cairo context drawing into `canvas` (ARGB32 premultiplied,
/// which is byte-identical to wl_shm Argb8888 on little-endian).
///
/// SAFETY contract: `canvas` is the SlotPool mmap slice for exactly
/// (`w_px` × `h_px` × 4) bytes and outlives `f`. The ImageSurface is flushed
/// and dropped before returning, so cairo's raw pointer never escapes the
/// borrow. `f` must not touch the pool.
pub fn with_cairo<R>(
    canvas: &mut [u8],
    w_px: i32,
    h_px: i32,
    f: impl FnOnce(&cairo::Context) -> R,
) -> Result<R> {
    let stride = cairo::Format::ARgb32.stride_for_width(w_px as u32)?;
    ensure!(stride == w_px * 4, "unexpected cairo stride {stride} for width {w_px}");
    ensure!(
        canvas.len() >= (stride as usize) * (h_px as usize),
        "canvas too small: {} < {}",
        canvas.len(),
        stride * h_px
    );
    let surf = unsafe {
        cairo::ImageSurface::create_for_data_unsafe(
            canvas.as_mut_ptr(),
            cairo::Format::ARgb32,
            w_px,
            h_px,
            stride,
        )?
    };
    let r = {
        let cr = cairo::Context::new(&surf)?;
        f(&cr)
    };
    surf.flush();
    drop(surf);
    Ok(r)
}

/// wl_shm Argb8888 matches cairo ARgb32 only on little-endian.
pub fn assert_pixel_format_compatible() {
    assert!(
        cfg!(target_endian = "little"),
        "big-endian targets need a pixel format conversion between cairo ARgb32 and wl_shm Argb8888"
    );
}
