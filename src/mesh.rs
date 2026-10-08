//! Shared mesh rendering ABI and hologram/quantitative scalar shader.
//!
//! Hosts own geometry, solvers, GPU pipelines and targets. Matrices use column
//! major storage; `model` must be rigid (rotation/translation, no nonuniform
//! scaling). Shader output is linear RGB for an sRGB render attachment.
use serde::{Deserialize, Serialize};

/// Reusable WGSL; entry points `vs_main` and `fs_main`, uniform binding 0/0.
pub const SHADER: &str = include_str!("mesh.wgsl");

/// Packed mesh vertex: locations 0 position, 1 normal, 2 normalized scalar.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "wgpu", derive(bytemuck::Pod, bytemuck::Zeroable))]
pub struct Vertex {
    /// Model-space position.
    pub position: [f32; 3],
    /// Model-space unit normal.
    pub normal: [f32; 3],
    /// Normalized scalar in 0..=1; host validates finite values.
    pub field: f32,
}

#[cfg(feature = "wgpu")]
impl Vertex {
    /// Shared GPU layout; callers must not duplicate byte offsets.
    pub const fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32];
        wgpu::VertexBufferLayout {
            array_stride: core::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRIBUTES,
        }
    }
}

/// Uniform ABI matching [`SHADER`].
#[repr(C)]
#[derive(Clone, Copy)]
#[cfg_attr(feature = "wgpu", derive(bytemuck::Pod, bytemuck::Zeroable))]
pub struct Uniforms {
    /// Projection * view * model.
    pub mvp: [[f32; 4]; 4],
    /// Rigid model transform.
    pub model: [[f32; 4]; 4],
    /// Theme color in sRGB, alpha ignored.
    pub color: [f32; 4],
    /// Time seconds, glow, hologram opacity (quantitative mode is opaque), wireframe toggle.
    pub params: [f32; 4],
    /// Palette toggle, palette index, illustrative lighting toggle, reserved zero.
    /// Lighting must remain zero for quantitative fields whose colors encode values.
    pub field_params: [f32; 4],
    /// World-space eye position; last component reserved zero.
    pub eye: [f32; 4],
}

/// Ordered scalar palettes, approximated with six sRGB stops in WGSL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Colormap {
    /// Dark purple through red to yellow.
    #[default]
    Inferno,
    /// Purple through green to yellow.
    Viridis,
    /// Dark purple through pink to cream.
    Magma,
}
impl Colormap {
    /// Shader palette selector.
    pub const fn shader_index(self) -> f32 {
        match self {
            Self::Inferno => 0.0,
            Self::Viridis => 1.0,
            Self::Magma => 2.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shader_validates_and_matches_host_abi() {
        let module = naga::front::wgsl::parse_str(SHADER).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("WGSL validates");
        let ty = module
            .types
            .iter()
            .find(|(_, ty)| ty.name.as_deref() == Some("Uniforms"))
            .unwrap()
            .1;
        let naga::TypeInner::Struct { span, .. } = ty.inner else {
            panic!("uniform struct")
        };
        assert_eq!(span as usize, core::mem::size_of::<Uniforms>());
        assert_eq!(core::mem::size_of::<Vertex>(), 28);
    }
    #[test]
    fn palettes_round_trip_stable_names() {
        for (palette, name, index) in [
            (Colormap::Inferno, "inferno", 0.0),
            (Colormap::Viridis, "viridis", 1.0),
            (Colormap::Magma, "magma", 2.0),
        ] {
            assert_eq!(serde_json::to_value(palette).unwrap(), name);
            assert_eq!(palette.shader_index(), index);
        }
    }
}
