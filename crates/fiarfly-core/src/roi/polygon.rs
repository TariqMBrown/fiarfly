//! Polygon ROI type and RoiSet container.

use super::RoiMask;
use crate::FiarflyError;
use serde::{Deserialize, Serialize};

/// A single closed polygon ROI in image-pixel coordinates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Roi {
    pub id: String,
    pub label: String,
    pub group: Option<String>,
    pub color: [u8; 3],
    /// Polygon vertices as [x, y] in image pixels (closed — first ≠ last).
    pub vertices: Vec<[f32; 2]>,
}

impl Roi {
    /// Rasterize polygon to a pixel list using the scanline fill algorithm.
    pub fn to_mask(&self, height: usize, width: usize) -> RoiMask {
        let mut mask = RoiMask::new(&self.id);
        let verts = &self.vertices;
        let n = verts.len();
        if n < 3 {
            return mask;
        }

        // Bounding box (clamped to image).
        let min_y = verts
            .iter()
            .map(|v| v[1])
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as usize;
        let max_y = verts
            .iter()
            .map(|v| v[1])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(height as f32) as usize;

        for y in min_y..max_y.min(height) {
            let yf = y as f32 + 0.5; // sample at pixel-row center
            let mut xs: Vec<f32> = Vec::new();

            for i in 0..n {
                let j = (i + 1) % n;
                let (y0, y1) = (verts[i][1], verts[j][1]);
                let (x0, x1) = (verts[i][0], verts[j][0]);

                if (y0 <= yf && yf < y1) || (y1 <= yf && yf < y0) {
                    let t = (yf - y0) / (y1 - y0);
                    xs.push(x0 + t * (x1 - x0));
                }
            }

            xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            for pair in xs.chunks_exact(2) {
                let x_start = pair[0].floor().max(0.0) as usize;
                let x_end = pair[1].ceil().min(width as f32) as usize;
                for x in x_start..x_end.min(width) {
                    mask.pixels.push([y, x]);
                }
            }
        }
        mask
    }

    /// Neuropil annular ring: pixels at distance in [inner, outer] from the
    /// ROI centroid, excluding any pixel belonging to `exclude`.
    pub fn neuropil_mask(
        &self,
        height: usize,
        width: usize,
        inner_radius: f32,
        outer_radius: f32,
        exclude: &[&RoiMask],
    ) -> RoiMask {
        let [cx, cy] = self.centroid();
        let excluded: std::collections::HashSet<[usize; 2]> = exclude
            .iter()
            .flat_map(|m| m.pixels.iter().copied())
            .collect();

        let y0 = (cy - outer_radius).floor().max(0.0) as usize;
        let y1 = ((cy + outer_radius).ceil() as usize).min(height);
        let x0 = (cx - outer_radius).floor().max(0.0) as usize;
        let x1 = ((cx + outer_radius).ceil() as usize).min(width);

        let mut mask = RoiMask::new(format!("{}_neuropil", self.id));
        for r in y0..y1 {
            for c in x0..x1 {
                let dy = r as f32 - cy;
                let dx = c as f32 - cx;
                let dist = (dy * dy + dx * dx).sqrt();
                if dist >= inner_radius && dist <= outer_radius && !excluded.contains(&[r, c]) {
                    mask.pixels.push([r, c]);
                }
            }
        }
        mask
    }

    /// Ray-casting point-in-polygon test.
    pub fn contains_point(&self, x: f32, y: f32) -> bool {
        let verts = &self.vertices;
        let n = verts.len();
        if n < 3 {
            return false;
        }
        let mut inside = false;
        let mut j = n - 1;
        for i in 0..n {
            let (xi, yi) = (verts[i][0], verts[i][1]);
            let (xj, yj) = (verts[j][0], verts[j][1]);
            if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi) {
                inside = !inside;
            }
            j = i;
        }
        inside
    }

    /// Centroid of the polygon vertices [x, y].
    pub fn centroid(&self) -> [f32; 2] {
        let n = self.vertices.len() as f32;
        let x = self.vertices.iter().map(|v| v[0]).sum::<f32>() / n;
        let y = self.vertices.iter().map(|v| v[1]).sum::<f32>() / n;
        [x, y]
    }
}

/// A named collection of ROIs — the "mask set".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoiSet {
    pub name: String,
    pub image_shape: [usize; 2], // [height, width]
    pub rois: Vec<Roi>,
}

impl RoiSet {
    pub fn new(name: impl Into<String>, height: usize, width: usize) -> Self {
        Self {
            name: name.into(),
            image_shape: [height, width],
            rois: Vec::new(),
        }
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), FiarflyError> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load(path: &std::path::Path) -> Result<Self, FiarflyError> {
        let json = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }

    pub fn add_roi(&mut self, mut roi: Roi) -> &Roi {
        roi.id = format!("roi_{:03}", self.rois.len() + 1);
        self.rois.push(roi);
        self.rois.last().unwrap()
    }

    pub fn get(&self, id: &str) -> Option<&Roi> {
        self.rois.iter().find(|r| r.id == id)
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    fn square_roi(x0: f32, y0: f32, size: f32) -> Roi {
        Roi {
            id: "test".into(),
            label: "test".into(),
            group: None,
            color: [255, 0, 0],
            vertices: vec![
                [x0, y0],
                [x0 + size, y0],
                [x0 + size, y0 + size],
                [x0, y0 + size],
            ],
        }
    }

    #[test]
    fn rasterize_square_pixel_count() {
        let roi = square_roi(4.0, 4.0, 8.0); // 8×8 square
        let mask = roi.to_mask(32, 32);
        assert_eq!(mask.pixels.len(), 64, "8×8 square should have 64 pixels");
    }

    #[test]
    fn rasterize_clamps_to_image_bounds() {
        let roi = square_roi(28.0, 28.0, 10.0); // hangs off a 32×32 image
        let mask = roi.to_mask(32, 32);
        for &[r, c] in &mask.pixels {
            assert!(r < 32 && c < 32, "pixel ({r},{c}) out of bounds");
        }
    }

    #[test]
    fn point_in_polygon() {
        let roi = square_roi(0.0, 0.0, 10.0);
        assert!(roi.contains_point(5.0, 5.0));
        assert!(!roi.contains_point(15.0, 5.0));
        assert!(!roi.contains_point(5.0, 15.0));
    }

    #[test]
    fn roi_set_round_trip() {
        let mut set = RoiSet::new("test", 64, 64);
        set.add_roi(Roi {
            id: String::new(),
            label: "Cell 1".into(),
            group: Some("L2/3".into()),
            color: [255, 0, 0],
            vertices: vec![[10.0, 10.0], [20.0, 10.0], [20.0, 20.0], [10.0, 20.0]],
        });
        let json = serde_json::to_string(&set).unwrap();
        let loaded: RoiSet = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.rois.len(), 1);
        assert_eq!(loaded.rois[0].label, "Cell 1");
        assert_eq!(loaded.rois[0].id, "roi_001");
    }
}
