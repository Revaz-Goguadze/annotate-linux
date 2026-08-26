//! Per-slot damage bookkeeping for an N-buffered shm swapchain. A slot must
//! repaint everything that changed since *it* was last presented, not just
//! this frame's delta — each known slot keeps its own pending-rect ledger.
//! Pure and unit-tested; keys are opaque (`canvas.as_ptr() as usize`).

use std::collections::HashMap;

use crate::model::geom::Rect;

const MAX_RECTS: usize = 256;
const COVERAGE_ESCALATE: f64 = 0.6;
/// Ledger overflow marker: covers everything.
fn everything() -> Rect {
    Rect::new(f64::NEG_INFINITY / 2.0, f64::NEG_INFINITY / 2.0, f64::INFINITY, f64::INFINITY)
}

#[derive(Default)]
pub struct DamageTracker {
    ledgers: HashMap<usize, Vec<Rect>>,
}

impl DamageTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record damage for every known slot.
    pub fn record(&mut self, rect: Rect) {
        if rect.is_empty() {
            return;
        }
        for ledger in self.ledgers.values_mut() {
            if ledger.len() >= MAX_RECTS {
                ledger.clear();
                ledger.push(everything());
            } else {
                ledger.push(rect);
            }
        }
    }

    /// Everything changed (resize, scale change, clear, board toggle).
    pub fn invalidate_all(&mut self) {
        self.ledgers.clear();
    }

    /// Damage the slot keyed by `key` must repaint, clipped to `surface`.
    /// `None` = full repaint (unknown slot, overflow, or high coverage).
    /// Resets the slot's ledger — call exactly once per presented frame.
    pub fn take(&mut self, key: usize, surface: Rect) -> Option<Vec<Rect>> {
        let known = self.ledgers.contains_key(&key);
        let pending = self.ledgers.insert(key, Vec::new());
        if !known {
            return None;
        }
        let rects = merge(pending.unwrap_or_default());
        if rects.len() > MAX_RECTS {
            return None;
        }
        let covered: f64 = rects.iter().map(|r| r.area()).sum();
        if covered > COVERAGE_ESCALATE * surface.area() {
            return None;
        }
        let clipped: Vec<Rect> = rects
            .into_iter()
            .filter_map(|r| clip(r, surface))
            .collect();
        if clipped.iter().any(|r| r.x <= surface.x && r.y <= surface.y && r.w >= surface.w && r.h >= surface.h) {
            return None;
        }
        Some(clipped)
    }
}

fn clip(r: Rect, surface: Rect) -> Option<Rect> {
    let x0 = r.x.max(surface.x);
    let y0 = r.y.max(surface.y);
    let x1 = (r.x + r.w).min(surface.x + surface.w);
    let y1 = (r.y + r.h).min(surface.y + surface.h);
    let out = Rect::new(x0, y0, x1 - x0, y1 - y0);
    (!out.is_empty()).then_some(out)
}

/// Greedy union of rects whose 1px-inflated boxes intersect, to fixpoint.
pub fn merge(mut rects: Vec<Rect>) -> Vec<Rect> {
    loop {
        let mut merged_any = false;
        let mut out: Vec<Rect> = Vec::with_capacity(rects.len());
        'outer: for r in rects {
            for o in &mut out {
                if o.inflate(1.0).intersects(r.inflate(1.0)) {
                    *o = o.union(r);
                    merged_any = true;
                    continue 'outer;
                }
            }
            out.push(r);
        }
        rects = out;
        if !merged_any {
            return rects;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SURFACE: Rect = Rect { x: 0.0, y: 0.0, w: 1600.0, h: 1000.0 };

    #[test]
    fn unknown_slot_full_then_tracked() {
        let mut t = DamageTracker::new();
        assert_eq!(t.take(1, SURFACE), None, "first sight of a slot = full repaint");
        t.record(Rect::new(10.0, 10.0, 20.0, 20.0));
        let d = t.take(1, SURFACE).expect("known slot gets rects");
        assert_eq!(d, vec![Rect::new(10.0, 10.0, 20.0, 20.0)]);
        // taken → ledger reset
        assert_eq!(t.take(1, SURFACE), Some(vec![]));
    }

    #[test]
    fn slot_accumulates_across_missed_frames() {
        let mut t = DamageTracker::new();
        t.take(1, SURFACE);
        t.take(2, SURFACE);
        t.record(Rect::new(0.0, 0.0, 10.0, 10.0));
        // slot 1 presents and resets; slot 2 still owes this rect
        t.take(1, SURFACE);
        t.record(Rect::new(500.0, 500.0, 10.0, 10.0));
        let d2 = t.take(2, SURFACE).unwrap();
        assert_eq!(d2.len(), 2, "slot 2 repaints both rects: {d2:?}");
    }

    #[test]
    fn high_coverage_escalates_to_full() {
        let mut t = DamageTracker::new();
        t.take(1, SURFACE);
        t.record(Rect::new(0.0, 0.0, 1500.0, 900.0)); // ~84% of surface
        assert_eq!(t.take(1, SURFACE), None);
    }

    #[test]
    fn overflow_escalates_to_full() {
        let mut t = DamageTracker::new();
        t.take(1, SURFACE);
        for i in 0..400 {
            // far apart so merge can't collapse them
            let x = (i % 20) as f64 * 80.0;
            let y = (i / 20) as f64 * 50.0;
            t.record(Rect::new(x, y, 2.0, 2.0));
        }
        assert_eq!(t.take(1, SURFACE), None);
    }

    #[test]
    fn invalidate_all_forgets_slots() {
        let mut t = DamageTracker::new();
        t.take(1, SURFACE);
        t.invalidate_all();
        assert_eq!(t.take(1, SURFACE), None);
    }

    #[test]
    fn merge_unions_touching_rects() {
        let out = merge(vec![
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Rect::new(10.5, 0.0, 10.0, 10.0), // within 1px inflation
            Rect::new(500.0, 500.0, 5.0, 5.0),
        ]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], Rect::new(0.0, 0.0, 20.5, 10.0));
    }
}
