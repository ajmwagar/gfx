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
    /// One decoded frame packs straight RGB on the left and grayscale alpha on
    /// the right. Both planes share the decoder clock; dimensions are encoded.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub packed_alpha: bool,
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
        if self.packed_alpha && (self.source_size[0] % 2 != 0 || !self.regions.is_empty()) {
            return Err("Packed alpha requires even width and no color-key regions");
        }
        if (!self.packed_alpha && self.regions.is_empty()) || self.regions.len() > MAX_REGIONS {
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

    /// Logical display size after unpacking the two synchronized planes.
    pub fn display_size(&self) -> [u32; 2] {
        [
            self.source_size[0] / if self.packed_alpha { 2 } else { 1 },
            self.source_size[1],
        ]
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

/// Pack an sRGB render-target readback whose RGB was blended in linear light
/// over transparent black. Alpha is kept as linear grayscale in the right half.
/// Color is unpremultiplied in linear light, then encoded back to sRGB.
pub fn pack_render_rgba(size: [u32; 2], rgba: &[u8]) -> Result<Vec<u8>, &'static str> {
    pack_rgba(size, rgba, true)
}

/// Pack straight sRGB PNG/presentation pixels without render unpremultiplication.
pub fn pack_straight_rgba(size: [u32; 2], rgba: &[u8]) -> Result<Vec<u8>, &'static str> {
    pack_rgba(size, rgba, false)
}
fn pack_rgba(
    size: [u32; 2],
    rgba: &[u8],
    render_premultiplied: bool,
) -> Result<Vec<u8>, &'static str> {
    let [w, h] = size.map(|v| v as usize);
    if w == 0 || h == 0 || w > 4096 || h > 8192 || rgba.len() != w * h * 4 {
        return Err("Invalid RGBA frame dimensions or byte count");
    }
    let mut packed = vec![0; rgba.len() * 2];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let a = rgba[i + 3];
            let rgb = if render_premultiplied {
                let mut color = crate::Color::from_srgb8(rgba[i], rgba[i + 1], rgba[i + 2], a);
                if a != 0 {
                    color.r /= color.a;
                    color.g /= color.a;
                    color.b /= color.a;
                } else {
                    color = crate::Color::TRANSPARENT;
                }
                color.to_srgb8()
            } else {
                [rgba[i], rgba[i + 1], rgba[i + 2], a]
            };
            let c = (y * w * 2 + x) * 4;
            packed[c..c + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            let alpha = c + w * 4;
            packed[alpha..alpha + 4].copy_from_slice(&[a, a, a, 255]);
        }
    }
    Ok(packed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_gles_uniform_and_varying_precision_matches() {
        for declaration in [
            "uniform mediump float uPackedAlpha;",
            "varying mediump vec2 vTextureUv;",
            "varying mediump vec2 vAlphaUv;",
            "varying mediump vec2 vSourceUv;",
        ] {
            assert!(GLES_VERTEX.contains(declaration));
            assert!(GLES_FRAGMENT.contains(declaration));
        }
    }
    #[test]
    fn packing_preserves_black_geometry_and_independent_coverage() {
        let pixels = [0, 0, 0, 255, 0, 0, 0, 0, 188, 0, 0, 128];
        let packed = pack_render_rgba([3, 1], &pixels).unwrap();
        assert_eq!(&packed[0..8], &[0, 0, 0, 255, 0, 0, 0, 255]);
        assert_eq!(&packed[8..12], &[255, 0, 0, 255]);
        assert_eq!(
            &packed[12..],
            &[255, 255, 255, 255, 0, 0, 0, 255, 128, 128, 128, 255]
        );
        assert!(pack_render_rgba([3, 2], &pixels).is_err());
        assert_eq!(
            &pack_straight_rgba([1, 1], &[188, 0, 0, 128]).unwrap()[..4],
            &[188, 0, 0, 255]
        );
    }
    fn matte() -> VideoMatte {
        VideoMatte {
            source_size: [1280, 960],
            packed_alpha: false,
            regions: vec![ColorKeyRegion {
                bounds: [0.0, 0.0, 0.625, 1.0],
                color: [0, 0, 0],
                cutoff: 2.0 / 255.0,
                feather: 6.0 / 255.0,
            }],
        }
    }
    #[test]
    fn packed_planes_are_explicit_and_preserve_display_aspect() {
        let mut m = VideoMatte {
            source_size: [2560, 960],
            packed_alpha: true,
            regions: vec![],
        };
        m.validate().unwrap();
        assert_eq!(m.display_size(), [1280, 960]);
        m.source_size[0] = 2559;
        assert!(m.validate().is_err());
        m.source_size[0] = 2560;
        m.regions = matte().regions;
        assert!(m.validate().is_err());
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
