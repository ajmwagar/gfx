//! Shared mesh rendering ABI and hologram/quantitative scalar shader.
//!
//! Hosts own geometry, solvers, GPU resources and targets. Shared pipeline
//! construction is available through `Pipelines`. Matrices use column
//! major storage; `model` must be rigid (rotation/translation, no nonuniform
//! scaling). Shader output is linear RGB for an sRGB render attachment.
#[cfg(feature = "wgpu")]
pub use crate::mesh_renderer::{Draw, Pipelines, Renderer};
use serde::{Deserialize, Serialize};

/// Face finish independent of an optional edge overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Finish {
    /// Preserve historical face shading and edge-toggle behavior.
    #[default]
    Legacy,
    /// Animated hologram lighting, unaffected by the edge toggle.
    Hologram,
    /// Existing bright unlit shading, without scan or rim modulation.
    Flat,
}

impl Finish {
    /// Whether this is the serialization-compatible default.
    pub const fn is_legacy(&self) -> bool {
        matches!(self, Self::Legacy)
    }

    /// Separate fill/edge uniforms without changing geometry or scalar fields.
    pub fn uniforms(self, mut base: Uniforms, edges: bool) -> (Uniforms, Option<Uniforms>) {
        base.params[3] = match self {
            Self::Legacy => {
                if edges {
                    1.
                } else {
                    0.
                }
            }
            Self::Hologram => 0.,
            Self::Flat => 1.,
        };
        let mut edge = base;
        if self != Self::Legacy {
            edge.params[3] = 1.;
        }
        (base, edges.then_some(edge))
    }
}

/// Reusable WGSL; entry points `vs_main` and `fs_main`, uniform binding 0/0.
pub const SHADER: &str = include_str!("mesh.wgsl");

/// Canonical box wire geometry for displaying the same supplied bounds that
/// analysis checked. Hosts transform these endpoints into their model frame.
/// # Errors
/// Rejects nonfinite bounds or a zero/negative extent on any axis.
pub fn wire_box_edges(bounds: [[f32; 3]; 2]) -> Result<[[[f32; 3]; 2]; 12], &'static str> {
    if bounds.iter().flatten().any(|v| !v.is_finite())
        || (0..3).any(|i| bounds[0][i] >= bounds[1][i])
    {
        return Err("Invalid box display bounds");
    }
    let points: [[f32; 3]; 8] = std::array::from_fn(|corner| {
        std::array::from_fn(|axis| bounds[(corner >> axis) & 1][axis])
    });
    let mut edges = [[[0.; 3]; 2]; 12];
    let mut next = 0;
    for corner in 0..8 {
        for axis in 0..3 {
            if corner & (1 << axis) == 0 {
                edges[next] = [points[corner], points[corner | (1 << axis)]];
                next += 1;
            }
        }
    }
    Ok(edges)
}

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
    /// Time, glow, hologram opacity (quantitative mode is opaque), bright unlit pass toggle.
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
    #[allow(
        clippy::float_cmp,
        reason = "Discrete shader selectors must match exactly."
    )]
    fn finishes_separate_edges_and_preserve_quantitative_state() {
        let base = Uniforms {
            mvp: [[0.; 4]; 4],
            model: [[0.; 4]; 4],
            color: [0.2, 0.3, 0.4, 1.],
            params: [2., 1.15, 0.92, 0.],
            field_params: [1., 2., 0., 0.],
            eye: [0.; 4],
        };
        for finish in [Finish::Legacy, Finish::Hologram, Finish::Flat] {
            let (plain, absent) = finish.uniforms(base, false);
            assert!(absent.is_none());
            let (filled, edge) = finish.uniforms(base, true);
            let edge = edge.unwrap();
            if finish != Finish::Legacy {
                assert_eq!(plain.params, filled.params);
            }
            assert_eq!(edge.params[3], 1.);
            for uniform in [plain, filled, edge] {
                assert_eq!(uniform.field_params, base.field_params);
                assert_eq!(uniform.mvp, base.mvp);
                assert_eq!(uniform.model, base.model);
                assert_eq!(uniform.color, base.color);
                assert_eq!(&uniform.params[..3], &base.params[..3]);
            }
        }
        assert_eq!(Finish::Legacy.uniforms(base, true).0.params[3], 1.);
        assert_eq!(Finish::Hologram.uniforms(base, true).0.params[3], 0.);
        assert_eq!(Finish::Flat.uniforms(base, false).0.params[3], 1.);
    }
    #[test]
    fn wire_boxes_have_twelve_unique_axis_edges() {
        let edges = wire_box_edges([[0.; 3], [1.; 3]]).unwrap();
        assert_eq!(edges.len(), 12);
        for [a, b] in edges {
            assert_eq!((0..3).filter(|i| a[*i] != b[*i]).count(), 1);
            assert!(a.iter().chain(b.iter()).all(|v| *v == 0. || *v == 1.));
        }
        assert!(wire_box_edges([[0.; 3], [f32::NAN; 3]]).is_err());
        assert!(wire_box_edges([[1.; 3], [0.; 3]]).is_err());
    }
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
