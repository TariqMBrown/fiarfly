//! Display-only pseudo-color lookup tables (LUTs), à la ImageJ.
//!
//! Pure presentation: takes a normalized intensity in `[0, 1]` and returns an
//! RGB triple. Lives in the GUI crate so `fiarfly-core` stays display-free.
//! The perceptually-uniform maps (Viridis, Inferno) are coarse anchor-point
//! approximations of the matplotlib originals — fine for on-screen preview.

/// Available pseudo-color lookup tables for fluorescence display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Colormap {
    /// Plain grayscale (the classic raw-fluorescence look).
    #[default]
    Grays,
    /// ImageJ-style "Fire": black → purple → red → orange → yellow → white.
    Fire,
    /// Cool blue/cyan ramp.
    Ice,
    /// Black → green (matches GCaMP / green emission).
    Green,
    /// Black → magenta.
    Magenta,
    /// Perceptually-uniform viridis (matplotlib).
    Viridis,
    /// Perceptually-uniform inferno (matplotlib).
    Inferno,
}

impl Colormap {
    /// Every variant, in menu order.
    pub const ALL: [Colormap; 7] = [
        Colormap::Grays,
        Colormap::Fire,
        Colormap::Ice,
        Colormap::Green,
        Colormap::Magenta,
        Colormap::Viridis,
        Colormap::Inferno,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Colormap::Grays => "Grays",
            Colormap::Fire => "Fire",
            Colormap::Ice => "Ice",
            Colormap::Green => "Green",
            Colormap::Magenta => "Magenta",
            Colormap::Viridis => "Viridis",
            Colormap::Inferno => "Inferno",
        }
    }

    /// Map a normalized intensity `t` (clamped to `[0, 1]`) to an RGB triple.
    pub fn rgb(self, t: f32) -> [u8; 3] {
        let t = t.clamp(0.0, 1.0);
        match self {
            Colormap::Grays => {
                let g = (t * 255.0) as u8;
                [g, g, g]
            }
            Colormap::Green => [0, (t * 255.0) as u8, 0],
            Colormap::Magenta => {
                let v = (t * 255.0) as u8;
                [v, 0, v]
            }
            Colormap::Fire => interp(t, &FIRE),
            Colormap::Ice => interp(t, &ICE),
            Colormap::Viridis => interp(t, &VIRIDIS),
            Colormap::Inferno => interp(t, &INFERNO),
        }
    }

    /// Build a flat RGB pixel buffer (row-major, 3 bytes/pixel) from already
    /// 0–255 normalized grayscale values — ready for `ColorImage::from_rgb`.
    pub fn map_u8(self, gray: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(gray.len() * 3);
        for &g in gray {
            out.extend_from_slice(&self.rgb(g as f32 / 255.0));
        }
        out
    }
}

/// Linearly interpolate over evenly-spaced RGB anchor points.
fn interp(t: f32, anchors: &[[u8; 3]]) -> [u8; 3] {
    let n = anchors.len();
    debug_assert!(n >= 2);
    let scaled = t * (n - 1) as f32;
    let i = (scaled.floor() as usize).min(n - 2);
    let f = scaled - i as f32;
    let a = anchors[i];
    let b = anchors[i + 1];
    [
        lerp(a[0], b[0], f),
        lerp(a[1], b[1], f),
        lerp(a[2], b[2], f),
    ]
}

fn lerp(a: u8, b: u8, f: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * f).round().clamp(0.0, 255.0) as u8
}

// ImageJ-flavored "Fire": black → purple → red → orange → yellow → white.
const FIRE: [[u8; 3]; 7] = [
    [0, 0, 0],
    [40, 0, 120],
    [140, 0, 100],
    [210, 40, 0],
    [245, 140, 0],
    [255, 225, 50],
    [255, 255, 255],
];

// Cool blue/cyan ramp.
const ICE: [[u8; 3]; 5] = [
    [0, 0, 40],
    [0, 90, 160],
    [0, 170, 210],
    [140, 220, 235],
    [255, 255, 255],
];

// Coarse viridis approximation (matplotlib).
const VIRIDIS: [[u8; 3]; 9] = [
    [68, 1, 84],
    [72, 40, 120],
    [62, 73, 137],
    [49, 104, 142],
    [38, 130, 142],
    [31, 158, 137],
    [53, 183, 121],
    [110, 206, 88],
    [253, 231, 37],
];

// Coarse inferno approximation (matplotlib).
const INFERNO: [[u8; 3]; 9] = [
    [0, 0, 4],
    [31, 12, 72],
    [85, 15, 109],
    [136, 34, 106],
    [186, 54, 85],
    [227, 89, 51],
    [249, 140, 10],
    [249, 201, 50],
    [252, 255, 164],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grays_endpoints() {
        assert_eq!(Colormap::Grays.rgb(0.0), [0, 0, 0]);
        assert_eq!(Colormap::Grays.rgb(1.0), [255, 255, 255]);
    }

    #[test]
    fn out_of_range_is_clamped() {
        assert_eq!(Colormap::Viridis.rgb(-5.0), VIRIDIS[0]);
        assert_eq!(Colormap::Viridis.rgb(5.0), *VIRIDIS.last().unwrap());
    }

    #[test]
    fn anchor_endpoints_exact() {
        assert_eq!(Colormap::Fire.rgb(0.0), FIRE[0]);
        assert_eq!(Colormap::Fire.rgb(1.0), *FIRE.last().unwrap());
        assert_eq!(Colormap::Inferno.rgb(1.0), *INFERNO.last().unwrap());
    }

    #[test]
    fn map_u8_has_three_bytes_per_pixel() {
        let out = Colormap::Green.map_u8(&[0, 128, 255]);
        assert_eq!(out.len(), 9);
        assert_eq!(&out[3..6], &[0, 128, 0]);
    }
}
