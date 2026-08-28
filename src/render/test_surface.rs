//! Test-only cairo scratch surface helpers: paint into an in-memory ARGB32
//! image, then assert on the pixels that came out.

use cairo::{Context, Format, ImageSurface};

pub struct Canvas {
    surface: ImageSurface,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Self {
        Self { surface: ImageSurface::create(Format::ARgb32, w, h).expect("scratch surface") }
    }

    /// Paint with a fresh context; the context is dropped (flushing the
    /// surface) before any pixel read.
    pub fn paint(&mut self, f: impl FnOnce(&Context)) -> &mut Self {
        let cr = Context::new(&self.surface).expect("scratch context");
        f(&cr);
        drop(cr);
        self.surface.flush();
        self
    }

    /// Premultiplied alpha of one pixel (0 = untouched).
    pub fn alpha_at(&mut self, x: i32, y: i32) -> u8 {
        let stride = self.surface.stride() as usize;
        let data = self.surface.data().expect("surface data");
        data[y as usize * stride + x as usize * 4 + 3]
    }

    /// One pixel as (r, g, b, a) bytes, un-premultiplied.
    pub fn rgba_at(&mut self, x: i32, y: i32) -> (u8, u8, u8, u8) {
        let stride = self.surface.stride() as usize;
        let data = self.surface.data().expect("surface data");
        let i = y as usize * stride + x as usize * 4;
        // ARgb32 is native-endian premultiplied BGRA on little-endian hosts.
        let (b, g, r, a) = (data[i], data[i + 1], data[i + 2], data[i + 3]);
        let un = |v: u8| if a == 0 { 0 } else { (v as u32 * 255 / a as u32).min(255) as u8 };
        (un(r), un(g), un(b), a)
    }

    /// Number of pixels with any coverage.
    pub fn ink(&mut self) -> usize {
        let stride = self.surface.stride() as usize;
        let (w, h) = (self.surface.width() as usize, self.surface.height() as usize);
        let data = self.surface.data().expect("surface data");
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|(x, y)| data[y * stride + x * 4 + 3] != 0)
            .count()
    }

    /// Number of pixels with coverage inside a rectangle.
    pub fn ink_in(&mut self, x0: i32, y0: i32, w: i32, h: i32) -> usize {
        let stride = self.surface.stride() as usize;
        let data = self.surface.data().expect("surface data");
        (y0..y0 + h)
            .flat_map(move |y| (x0..x0 + w).map(move |x| (x, y)))
            .filter(|(x, y)| data[*y as usize * stride + *x as usize * 4 + 3] != 0)
            .count()
    }
}
