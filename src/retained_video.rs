//! Retained video shading contract. Hosts own decoding, freshness and textures.

/// WGSL for contain fit, explicit alpha modes and host-supplied stale state.
pub const SHADER: &str = include_str!("shaders/retained_video.wgsl");

/// Binding-two uniform byte count; derived from the shared host ABI.
pub const UNIFORM_BYTES: u64 = core::mem::size_of::<Uniforms>() as u64;

/// Uniform ABI matching the retained-video WGSL, with native float byte order.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Uniforms {
    /// Destination size in pixels.
    pub viewport: [f32; 2],
    /// Decoded source size in pixels.
    pub source: [f32; 2],
    /// Host-computed stale presentation amount, zero through one.
    pub staleness: f32,
    /// Matching separate alpha plane available, zero or one.
    pub has_alpha: f32,
    /// Source color is premultiplied, zero or one.
    pub premultiplied: f32,
    /// Source supplies native alpha, zero or one.
    pub native_alpha: f32,
}

impl Uniforms {
    /// Safe upload bytes with no host-side pointer cast or padding assumptions.
    pub fn to_bytes(self) -> [u8; 32] {
        let values = [
            self.viewport[0],
            self.viewport[1],
            self.source[0],
            self.source[1],
            self.staleness,
            self.has_alpha,
            self.premultiplied,
            self.native_alpha,
        ];
        let mut bytes = [0; 32];
        for (value, slot) in values
            .into_iter()
            .zip(bytes.as_chunks_mut::<4>().0.iter_mut())
        {
            slot.copy_from_slice(&value.to_ne_bytes());
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shader_and_every_uniform_offset_match() {
        let module = naga::front::wgsl::parse_str(SHADER).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
        let ty = module
            .types
            .iter()
            .find(|(_, ty)| ty.name.as_deref() == Some("Uniforms"))
            .unwrap()
            .1;
        let naga::TypeInner::Struct { members, span } = &ty.inner else {
            panic!("uniform struct")
        };
        assert_eq!(u64::from(*span), UNIFORM_BYTES);
        assert_eq!(
            members.iter().map(|m| m.offset).collect::<Vec<_>>(),
            [0, 8, 16, 20, 24, 28]
        );
        let uniforms = Uniforms {
            viewport: [1., 2.],
            source: [3., 4.],
            staleness: 5.,
            has_alpha: 6.,
            premultiplied: 7.,
            native_alpha: 8.,
        };
        let decoded = uniforms
            .to_bytes()
            .chunks_exact(4)
            .map(|b| f32::from_ne_bytes(b.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(decoded, [1., 2., 3., 4., 5., 6., 7., 8.]);
    }

    #[test]
    fn moved_source_is_byte_identical_to_canvas_capture() {
        use sha2::{Digest, Sha256};
        assert_eq!(
            format!("{:x}", Sha256::digest(SHADER.as_bytes())),
            "17c6d45e346eef957405c84a9beed44c4e1b46b02f7ce94326d02c6e228feb06"
        );
    }
}
