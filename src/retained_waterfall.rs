//! Retained scalar waterfall shading; hosts own ring storage and row uploads.

/// Original Canvas waterfall source, preserving its palette and gamma.
pub const SHADER: &str = include_str!("shaders/retained_waterfall.wgsl");
/// Binding-one byte count, derived from the mixed scalar ABI.
pub const UNIFORM_BYTES: u64 = core::mem::size_of::<Uniforms>() as u64;

/// Mixed float/integer ABI; dimensions and ring indices stay integers.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Uniforms {
    /// Destination pixel dimensions.
    pub viewport: [f32; 2],
    /// Scalar texture width and ring height.
    pub texture_size: [u32; 2],
    /// Physical row containing the newest observation.
    pub head: u32,
    /// Number of observed rows, excluding uninitialized history.
    pub filled: u32,
    /// Normalized start of waterfall within the destination.
    pub waterfall_top: f32,
    /// Reserved zero padding.
    pub padding: f32,
}
impl Uniforms {
    /// Exact upload words, preserving integer values without float conversion.
    pub fn words(self) -> [u32; 8] {
        [
            self.viewport[0].to_bits(),
            self.viewport[1].to_bits(),
            self.texture_size[0],
            self.texture_size[1],
            self.head,
            self.filled,
            self.waterfall_top.to_bits(),
            self.padding.to_bits(),
        ]
    }
    /// Safe native-endian upload without pointer casts.
    pub fn to_bytes(self) -> [u8; 32] {
        let mut bytes = [0; 32];
        for (word, slot) in self
            .words()
            .into_iter()
            .zip(bytes.as_chunks_mut::<4>().0.iter_mut())
        {
            slot.copy_from_slice(&word.to_ne_bytes());
        }
        bytes
    }
}

/// GPU construction shared by retained hosts; no texture allocation or updates.
#[cfg(feature = "wgpu")]
pub struct Pipeline {
    /// Fullscreen triangle with the original alpha blend state.
    pub render_pipeline: wgpu::RenderPipeline,
    /// Nonfilterable scalar texture and mixed uniform bindings.
    pub bind_group_layout: wgpu::BindGroupLayout,
}
#[cfg(feature = "wgpu")]
impl Pipeline {
    /// Construct a pipeline for a caller-owned render target.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("GFX retained waterfall layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES),
                    },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("GFX retained waterfall pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("GFX retained waterfall shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("GFX retained waterfall pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Self {
            render_pipeline,
            bind_group_layout,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_and_mixed_abi_match_original() {
        use sha2::{Digest, Sha256};
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
            .find(|(_, t)| t.name.as_deref() == Some("Uniforms"))
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
        let value = Uniforms {
            viewport: [1920., 1080.],
            texture_size: [128, 360],
            head: 7,
            filled: 42,
            waterfall_top: 0.39,
            padding: 0.,
        };
        let words = value.words();
        assert_eq!(&words[2..6], &[128, 360, 7, 42]);
        assert_eq!(
            value.to_bytes(),
            words.map(u32::to_ne_bytes).concat().as_slice()
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(SHADER)),
            "9d2b4983dba271908d7fca41e139c436789859d134cb51fd90fd6d0718d07eef"
        );
    }
}
