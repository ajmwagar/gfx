use std::time::Duration;

use bytemuck::{Pod, Zeroable};
use thiserror::Error;

const UNIFORM_BYTES: u64 = std::mem::size_of::<FrameUniforms>() as u64;
const DEFAULT_FRESH_AFTER: Duration = Duration::from_secs(3);
const DEFAULT_STALE_AFTER: Duration = Duration::from_secs(20);

const SHADER: &str = r"
struct Uniforms {
    viewport: vec2<f32>,
    source: vec2<f32>,
    staleness: f32,
    _padding: vec3<f32>,
};

@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var frame_sampler: sampler;
@group(0) @binding(2) var<uniform> u: Uniforms;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0),
    );
    var out: VertexOut;
    out.position = vec4<f32>(positions[index], 0.0, 1.0);
    out.uv = uvs[index];
    return out;
}

@fragment
fn fs_main(input: VertexOut) -> @location(0) vec4<f32> {
    let viewport_aspect = u.viewport.x / max(u.viewport.y, 1.0);
    let source_aspect = u.source.x / max(u.source.y, 1.0);
    var uv = input.uv;
    if viewport_aspect > source_aspect {
        let width = source_aspect / viewport_aspect;
        if abs(uv.x - 0.5) > width * 0.5 { return vec4<f32>(0.0); }
        uv.x = (uv.x - 0.5) / width + 0.5;
    } else {
        let height = viewport_aspect / source_aspect;
        if abs(uv.y - 0.5) > height * 0.5 { return vec4<f32>(0.0); }
        uv.y = (uv.y - 0.5) / height + 0.5;
    }
    let frame_color = textureSample(frame, frame_sampler, uv);
    let band = step(0.5, fract((input.position.x + input.position.y) / 24.0));
    let dimmed = frame_color.rgb * (1.0 - 0.45 * u.staleness);
    let barred = dimmed * (1.0 - 0.35 * u.staleness * band);
    return vec4<f32>(barred, frame_color.a);
}
";

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct FrameUniforms {
    viewport: [f32; 2],
    source: [f32; 2],
    staleness: f32,
    padding: [f32; 3],
}

/// Per-frame state for presenting an image with aspect-preserving containment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FramePresentation {
    /// Destination size in logical or physical pixels.
    pub viewport: [f32; 2],
    /// Source image size in pixels.
    pub source: [f32; 2],
    /// Feed staleness from live `0` to fully stale `1`.
    pub staleness: f32,
}

impl FramePresentation {
    /// Constructs presentation state with an explicit staleness value.
    pub const fn new(viewport: [f32; 2], source: [f32; 2], staleness: f32) -> Self {
        Self {
            viewport,
            source,
            staleness,
        }
    }

    /// Constructs presentation state using the default live-feed freshness curve.
    pub fn for_age(viewport: [f32; 2], source: [f32; 2], age: Duration) -> Self {
        Self::new(
            viewport,
            source,
            Self::staleness_for_age(age, DEFAULT_FRESH_AFTER, DEFAULT_STALE_AFTER),
        )
    }

    /// Maps an age onto a monotonic `0..=1` freshness curve.
    pub fn staleness_for_age(age: Duration, fresh_after: Duration, stale_after: Duration) -> f32 {
        let fresh = fresh_after.as_secs_f32();
        let stale = stale_after.as_secs_f32();
        if !fresh.is_finite() || !stale.is_finite() || stale <= fresh {
            return 1.0;
        }
        ((age.as_secs_f32() - fresh) / (stale - fresh)).clamp(0.0, 1.0)
    }

    fn uniforms(self) -> Result<FrameUniforms, FramePresentationError> {
        if self
            .viewport
            .into_iter()
            .chain(self.source)
            .chain([self.staleness])
            .any(|value| !value.is_finite())
            || self.viewport.contains(&0.0)
            || self.source.contains(&0.0)
            || !(0.0..=1.0).contains(&self.staleness)
        {
            return Err(FramePresentationError::InvalidPresentation);
        }
        Ok(FrameUniforms {
            viewport: self.viewport,
            source: self.source,
            staleness: self.staleness,
            padding: [0.0; 3],
        })
    }
}

/// Immutable retained-frame pipeline configuration.
#[derive(Debug, Clone, Copy)]
pub struct FramePresenterConfig {
    /// Format of the caller-owned output target.
    pub target_format: wgpu::TextureFormat,
}

impl FramePresenterConfig {
    /// Creates configuration for one output format.
    pub const fn new(target_format: wgpu::TextureFormat) -> Self {
        Self { target_format }
    }
}

/// Failure to validate or update a retained frame.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum FramePresentationError {
    /// Source dimensions are zero or overflow addressable byte counts.
    #[error("frame dimensions are invalid")]
    InvalidDimensions,
    /// Presentation dimensions or staleness are invalid.
    #[error("frame presentation state is invalid")]
    InvalidPresentation,
    /// RGBA payload length does not match the retained texture extent.
    #[error("RGBA payload length does not match the retained texture")]
    InvalidPayload,
}

/// One retained source texture and its presentation bindings.
pub struct FrameTexture {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    width: u32,
    height: u32,
}

impl FrameTexture {
    /// Width of the retained source in pixels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height of the retained source in pixels.
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Source dimensions in pixels.
    pub const fn size(&self) -> [u32; 2] {
        [self.width, self.height]
    }

    /// Bind group used by [`FramePresenter::render_pipeline`].
    ///
    /// This narrow escape hatch lets toolkit render-pass adapters bind shared
    /// resources without forcing incompatible lifetime relationships through
    /// a helper method.
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }
}

/// Reusable WGPU pipeline for retained RGBA or externally imported frames.
pub struct FramePresenter {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl FramePresenter {
    /// Creates the retained frame pipeline.
    pub fn new(device: &wgpu::Device, config: FramePresenterConfig) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fpl-gfx retained frame layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
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
            label: Some("fpl-gfx retained frame pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fpl-gfx retained frame shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fpl-gfx retained frame pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.target_format,
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
            label: Some("fpl-gfx retained frame linear sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            min_filter: wgpu::FilterMode::Linear,
            mag_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            bind_group_layout,
            sampler,
        }
    }

    /// Allocates a retained RGBA8 sRGB source texture.
    ///
    /// # Errors
    ///
    /// Returns [`FramePresentationError::InvalidDimensions`] for zero or
    /// overflowing dimensions.
    pub fn create_rgba8_texture(
        &self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> Result<FrameTexture, FramePresentationError> {
        validate_dimensions(width, height)?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fpl-gfx retained RGBA frame"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.bind_texture(device, texture, width, height)
    }

    /// Retains an externally imported texture behind the same presentation pipeline.
    ///
    /// The caller remains responsible for external-memory lifetime and
    /// synchronization before queue submission.
    ///
    /// # Errors
    ///
    /// Returns [`FramePresentationError::InvalidDimensions`] for zero or
    /// overflowing dimensions.
    pub fn bind_external_texture(
        &self,
        device: &wgpu::Device,
        texture: wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<FrameTexture, FramePresentationError> {
        validate_dimensions(width, height)?;
        self.bind_texture(device, texture, width, height)
    }

    fn bind_texture(
        &self,
        device: &wgpu::Device,
        texture: wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<FrameTexture, FramePresentationError> {
        validate_dimensions(width, height)?;
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fpl-gfx retained frame uniforms"),
            size: UNIFORM_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fpl-gfx retained frame bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        Ok(FrameTexture {
            texture,
            bind_group,
            uniform,
            width,
            height,
        })
    }

    /// Uploads one tightly packed RGBA8 frame.
    ///
    /// # Errors
    ///
    /// Returns [`FramePresentationError::InvalidPayload`] when byte length
    /// does not match the retained extent.
    pub fn upload_rgba8(
        &self,
        queue: &wgpu::Queue,
        frame: &FrameTexture,
        bytes: &[u8],
    ) -> Result<(), FramePresentationError> {
        let expected = frame.width as usize * frame.height as usize * 4;
        if bytes.len() != expected {
            return Err(FramePresentationError::InvalidPayload);
        }
        queue.write_texture(
            frame.texture.as_image_copy(),
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(frame.width * 4),
                rows_per_image: Some(frame.height),
            },
            frame.texture.size(),
        );
        Ok(())
    }

    /// Uploads presentation state for one retained frame.
    ///
    /// # Errors
    ///
    /// Returns [`FramePresentationError::InvalidPresentation`] when values are
    /// zero, non-finite, or outside the supported staleness range.
    pub fn prepare(
        &self,
        queue: &wgpu::Queue,
        frame: &FrameTexture,
        presentation: FramePresentation,
    ) -> Result<(), FramePresentationError> {
        let uniforms = presentation.uniforms()?;
        queue.write_buffer(&frame.uniform, 0, bytemuck::bytes_of(&uniforms));
        Ok(())
    }

    /// Render pipeline for toolkit-owned render passes.
    pub fn render_pipeline(&self) -> &wgpu::RenderPipeline {
        &self.pipeline
    }

    /// Draws one prepared frame into an existing render pass.
    pub fn draw<'pass>(
        &'pass self,
        frame: &'pass FrameTexture,
        render_pass: &mut wgpu::RenderPass<'pass>,
    ) {
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &frame.bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), FramePresentationError> {
    width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|_| width > 0 && height > 0)
        .ok_or(FramePresentationError::InvalidDimensions)
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_curve_is_monotonic_and_bounded() {
        let values = (0..30)
            .map(|seconds| {
                FramePresentation::staleness_for_age(
                    Duration::from_secs(seconds),
                    DEFAULT_FRESH_AFTER,
                    DEFAULT_STALE_AFTER,
                )
            })
            .collect::<Vec<_>>();
        // Endpoints are exact contract values, not approximate measurements.
        assert_eq!(values[0].to_bits(), 0.0_f32.to_bits());
        assert_eq!(values.last().unwrap().to_bits(), 1.0_f32.to_bits());
        assert!(values.windows(2).all(|pair| pair[1] >= pair[0]));
    }

    #[test]
    fn frame_uniform_layout_matches_wgsl() {
        assert_eq!(UNIFORM_BYTES, 32);
    }

    #[test]
    fn invalid_presentation_fails_before_gpu_work() {
        let invalid = FramePresentation::new([0.0, 1.0], [640.0, 480.0], 0.0);
        assert_eq!(
            invalid.uniforms().unwrap_err(),
            FramePresentationError::InvalidPresentation,
        );
    }

    #[test]
    fn frame_shader_validates() {
        let module = naga::front::wgsl::parse_str(SHADER).expect("frame shader parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("frame shader validates");
    }
}
