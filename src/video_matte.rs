//! Bounded source-space color mattes for decoded video. Hosts own decoding.
use serde::{Deserialize, Serialize};

/// Maximum region uniforms shared with the GLES shader.
pub const MAX_REGIONS: usize = 8;

/// Opt-in presentation contract; never guessed from a stream's pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct VideoMatte {
    /// Exact decoded width and height; reject mismatched media before keying.
    pub source_size: [u32; 2],
    /// Region coordinates use the untransformed source, top-left origin.
    pub regions: Vec<ColorKeyRegion>,
}

/// One conservative color key in a normalized source rectangle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ColorKeyRegion {
    /// Normalized x, y, width, height. PDF regions can remain entirely opaque.
    pub bounds: [f32; 4],
    /// Decoded sRGB key, in byte units.
    pub color: [u8; 3],
    /// Maximum-channel distance below which pixels are fully transparent.
    pub cutoff: f32,
    /// Distance beyond cutoff over which opacity smoothly returns to one.
    pub feather: f32,
}

impl VideoMatte {
    /// Validate resource bounds and reject malformed or excessively broad keys.
    ///
    /// # Errors
    /// Returns a concrete admission failure for invalid dimensions or regions.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.source_size.iter().any(|v| !(1..=8192).contains(v)) {
            return Err("Video matte source dimensions must be 1..8192");
        }
        if self.regions.is_empty() || self.regions.len() > MAX_REGIONS {
            return Err("Video matte requires 1..8 regions");
        }
        for region in &self.regions {
            let [x, y, w, h] = region.bounds;
            if !region.bounds.iter().all(|v| v.is_finite())
                || x < 0.0
                || y < 0.0
                || w <= 0.0
                || h <= 0.0
                || x + w > 1.0
                || y + h > 1.0
            {
                return Err("Video matte region must fit within the source");
            }
            if !region.cutoff.is_finite()
                || !region.feather.is_finite()
                || !(0.0..=0.08).contains(&region.cutoff)
                || !(0.001..=0.08).contains(&region.feather)
                || region.cutoff + region.feather > 0.1
            {
                return Err("Video matte cutoff/feather exceed conservative color bounds");
            }
        }
        Ok(())
    }

    /// CPU oracle for the shader, on normalized top-left source coordinates.
    /// Call `validate` once at admission. No opacity change outside a region.
    pub fn opacity(&self, uv: [f32; 2], rgb: [f32; 3]) -> f32 {
        self.regions.iter().fold(1.0, |alpha, region| {
            let [x, y, w, h] = region.bounds;
            if uv[0] < x || uv[1] < y || uv[0] >= x + w || uv[1] >= y + h {
                return alpha;
            }
            let distance = rgb
                .iter()
                .zip(region.color)
                .map(|(v, key)| (v - f32::from(key) / 255.0).abs())
                .fold(0.0_f32, f32::max);
            let t = ((distance - region.cutoff) / region.feather).clamp(0.0, 1.0);
            alpha.min(t * t * (3.0 - 2.0 * t))
        })
    }
}

/// GLES2 vertex shader, sharing canonical source UVs with the matte contract.
pub const GLES_VERTEX: &str = include_str!("shaders/video_matte.vert");
/// Android external-OES decoder texture; output is premultiplied RGBA.
pub const GLES_FRAGMENT: &str = include_str!("shaders/video_matte.frag");

#[cfg(test)]
mod tests {
    use super::*;
    fn matte() -> VideoMatte {
        VideoMatte {
            source_size: [1280, 960],
            regions: vec![ColorKeyRegion {
                bounds: [0.0, 0.0, 0.625, 1.0],
                color: [0, 0, 0],
                cutoff: 2.0 / 255.0,
                feather: 6.0 / 255.0,
            }],
        }
    }
    #[test]
    fn color_matte_preserves_pdf_ink_and_foreground() {
        let matte = matte();
        matte.validate().unwrap();
        assert_eq!(matte.opacity([0.1, 0.2], [0.0; 3]), 0.0);
        assert_eq!(matte.opacity([0.8, 0.2], [0.0; 3]), 1.0);
        assert_eq!(matte.opacity([0.1, 0.2], [0.1, 0.2, 0.5]), 1.0);
        assert_eq!(matte.opacity([0.1, 0.2], [1.0; 3]), 1.0);
        let half = matte.opacity([0.1, 0.2], [5.0 / 255.0; 3]);
        assert!((half - 0.5).abs() < 0.0001);
        assert_eq!(matte.opacity([0.625, 0.2], [0.0; 3]), 1.0);
    }
    #[test]
    fn rejects_unbounded_and_non_finite_keys() {
        let mut m = matte();
        m.regions[0].bounds[0] = f32::NAN;
        assert!(m.validate().is_err());
        let mut m = matte();
        m.regions[0].bounds[2] = 2.0;
        assert!(m.validate().is_err());
        let mut m = matte();
        m.regions[0].feather = 0.0;
        assert!(m.validate().is_err());
        let mut m = matte();
        m.regions[0].cutoff = 0.08;
        m.regions[0].feather = 0.08;
        assert!(m.validate().is_err());
        let mut m = matte();
        m.source_size[0] = 0;
        assert!(m.validate().is_err());
        let mut m = matte();
        m.regions = vec![m.regions[0].clone(); 9];
        assert!(m.validate().is_err());
    }
}
