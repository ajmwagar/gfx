//! Retained video shading contract. Hosts own decoding, freshness and textures.

/// WGSL for contain fit, explicit alpha modes and host-supplied stale state.
pub const SHADER: &str = include_str!("shaders/retained_video.wgsl");

/// Binding-two uniform byte count; derived from the shared host ABI.
pub const UNIFORM_BYTES: u64 = core::mem::size_of::<Uniforms>() as u64;

/// Shared GPU state for retained video; the caller owns textures and scheduling.
#[cfg(feature = "wgpu")]
pub struct Pipeline {
    /// Fullscreen triangle with straight-alpha blending into the caller's target.
    pub render_pipeline: wgpu::RenderPipeline,
    /// Frame, filtering sampler, shared uniforms and separate alpha bindings.
    pub bind_group_layout: wgpu::BindGroupLayout,
    /// Linear filtering with clamped image boundaries.
    pub sampler: wgpu::Sampler,
}

#[cfg(feature = "wgpu")]
impl Pipeline {
    /// Construct the production video pipeline for a caller-owned target format.
    ///
    /// Texture color-space interpretation belongs to the source texture format;
    /// the render target format controls output encoding. The shader returns
    /// straight color, including when the source is premultiplied.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("GFX retained video layout"),
            entries: &[
                texture_entry(0),
                texture_entry(3),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
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
            label: Some("GFX retained video pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("GFX retained video shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("GFX retained video pipeline"),
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("GFX retained video linear sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            min_filter: wgpu::FilterMode::Linear,
            mag_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            render_pipeline,
            bind_group_layout,
            sampler,
        }
    }
}

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
