use bytemuck::{Pod, Zeroable};
use thiserror::Error;
use wgpu::util::DeviceExt;

use crate::{scene::GpuPrimitive, Color, Scene, Theme, ThemeError};

const INITIAL_INSTANCE_CAPACITY: usize = 256;

/// Logical-pixel viewport rendered into the target texture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// Width in logical pixels.
    pub width: f32,
    /// Height in logical pixels.
    pub height: f32,
    /// Output scale factor used for antialiasing and future text rendering.
    pub scale_factor: f32,
    /// Monotonic application time in seconds.
    pub time_seconds: f32,
}

impl Viewport {
    /// Constructs a viewport at scale factor 1.
    pub const fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            scale_factor: 1.0,
            time_seconds: 0.0,
        }
    }

    fn valid(self) -> bool {
        [
            self.width,
            self.height,
            self.scale_factor,
            self.time_seconds,
        ]
        .into_iter()
        .all(f32::is_finite)
            && self.width > 0.0
            && self.height > 0.0
            && self.scale_factor > 0.0
    }
}

/// Immutable renderer configuration.
#[derive(Debug, Clone, Copy)]
pub struct RendererConfig {
    /// Format of every target passed to this renderer.
    pub target_format: wgpu::TextureFormat,
    /// Target sample count.
    pub sample_count: u32,
    /// Initial primitive capacity before geometric growth.
    pub initial_capacity: usize,
}

impl RendererConfig {
    /// Constructs a single-sampled renderer configuration.
    pub const fn new(target_format: wgpu::TextureFormat) -> Self {
        Self {
            target_format,
            sample_count: 1,
            initial_capacity: INITIAL_INSTANCE_CAPACITY,
        }
    }
}

/// Per-frame render behavior.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderOptions {
    /// Clear the target before drawing. `None` preserves existing contents.
    pub clear: Option<Color>,
}

/// Caller-owned GPU state for one encoded frame.
///
/// Keeping these resources borrowed makes `fpl-gfx` composable with existing
/// render graphs: the host retains scheduling, submission, and target ownership.
pub struct RenderContext<'a> {
    /// Device used to grow retained GPU resources when required.
    pub device: &'a wgpu::Device,
    /// Queue used for small per-frame uploads.
    pub queue: &'a wgpu::Queue,
    /// Command encoder that receives this renderer's pass.
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// Texture view receiving the rendered scene.
    pub target: &'a wgpu::TextureView,
}

/// Observable renderer work for profiling and regression tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RendererStats {
    /// Primitives submitted in the last frame.
    pub primitives: usize,
    /// Draw calls submitted in the last frame.
    pub draw_calls: u32,
    /// Instance bytes uploaded in the last frame.
    pub uploaded_bytes: usize,
    /// Current retained GPU instance capacity.
    pub instance_capacity: usize,
}

/// A retained renderer for one target format.
pub struct Renderer {
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    staging: Vec<GpuPrimitive>,
    stats: RendererStats,
}

impl Renderer {
    /// Creates retained pipelines and buffers from a caller-owned device.
    pub fn new(device: &wgpu::Device, config: RendererConfig) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fpl-gfx primitive shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fpl-gfx frame layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("fpl-gfx frame uniforms"),
            contents: bytemuck::bytes_of(&FrameUniform::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fpl-gfx frame bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fpl-gfx pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fpl-gfx primitive pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[GpuPrimitive::layout()],
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
            multisample: wgpu::MultisampleState {
                count: config.sample_count.max(1),
                ..Default::default()
            },
            multiview: None,
            cache: None,
        });
        let instance_capacity = config.initial_capacity.max(1).next_power_of_two();
        let instance_buffer = create_instance_buffer(device, instance_capacity);
        Self {
            pipeline,
            uniform_buffer,
            bind_group,
            instance_buffer,
            instance_capacity,
            staging: Vec::with_capacity(instance_capacity),
            stats: RendererStats {
                instance_capacity,
                ..Default::default()
            },
        }
    }

    /// Renders a scene into a caller-owned target and command encoder.
    ///
    /// Primitives retain painter's order and are emitted as one instanced draw.
    /// The GPU buffer grows geometrically and never shrinks during renderer life.
    ///
    /// # Errors
    ///
    /// Returns an error when the viewport, theme, primitive data, or scene size
    /// cannot be represented safely by the renderer.
    pub fn render(
        &mut self,
        context: RenderContext<'_>,
        viewport: Viewport,
        theme: &Theme,
        scene: &Scene,
        options: RenderOptions,
    ) -> Result<RendererStats, RenderError> {
        let RenderContext {
            device,
            queue,
            encoder,
            target,
        } = context;
        if !viewport.valid() {
            return Err(RenderError::Viewport);
        }
        theme.validate()?;
        if scene
            .primitives()
            .iter()
            .copied()
            .any(|primitive| !primitive.valid())
        {
            return Err(RenderError::Primitive);
        }
        self.ensure_capacity(device, scene.len());
        let uniform = FrameUniform {
            viewport: [
                viewport.width,
                viewport.height,
                viewport.scale_factor,
                viewport.time_seconds,
            ],
            colors: theme.gpu_colors(),
            material: [
                theme.materials.gloss,
                theme.materials.grain,
                theme.materials.emission,
                0.0,
            ],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniform));

        self.staging.clear();
        self.staging.extend(
            scene
                .primitives()
                .iter()
                .copied()
                .map(crate::Primitive::gpu),
        );
        if !self.staging.is_empty() {
            queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&self.staging),
            );
        }
        let load = options.clear.map_or(wgpu::LoadOp::Load, |color| {
            wgpu::LoadOp::Clear(color.as_wgpu())
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fpl-gfx primitives"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        if !self.staging.is_empty() {
            pass.draw(
                0..6,
                0..u32::try_from(self.staging.len()).map_err(|_| RenderError::SceneSize)?,
            );
        }
        drop(pass);
        self.stats = RendererStats {
            primitives: self.staging.len(),
            draw_calls: u32::from(!self.staging.is_empty()),
            uploaded_bytes: std::mem::size_of_val(self.staging.as_slice()),
            instance_capacity: self.instance_capacity,
        };
        Ok(self.stats)
    }

    /// Returns statistics for the most recently encoded frame.
    pub const fn stats(&self) -> RendererStats {
        self.stats
    }

    fn ensure_capacity(&mut self, device: &wgpu::Device, required: usize) {
        if required <= self.instance_capacity {
            return;
        }
        self.instance_capacity = required.next_power_of_two();
        self.instance_buffer = create_instance_buffer(device, self.instance_capacity);
        if self.staging.capacity() < self.instance_capacity {
            self.staging
                .reserve_exact(self.instance_capacity.saturating_sub(self.staging.len()));
        }
    }
}

fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fpl-gfx primitive instances"),
        size: (capacity * std::mem::size_of::<GpuPrimitive>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct FrameUniform {
    viewport: [f32; 4],
    colors: [[f32; 4]; crate::theme::THEME_ROLE_COUNT],
    material: [f32; 4],
}

/// A rejected render request.
#[derive(Debug, Error)]
pub enum RenderError {
    /// The viewport contains zero, negative, or non-finite dimensions.
    #[error("viewport dimensions, scale, and time must be finite and positive")]
    Viewport,
    /// A primitive contains invalid bounds or non-finite style values.
    #[error("scene contains an invalid primitive")]
    Primitive,
    /// The scene cannot be addressed by wGPU's instance range.
    #[error("scene contains more than u32::MAX primitives")]
    SceneSize,
    /// The supplied theme is invalid.
    #[error(transparent)]
    Theme(#[from] ThemeError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_uniform_obeys_uniform_alignment() {
        assert_eq!(std::mem::size_of::<FrameUniform>() % 16, 0);
    }

    #[test]
    fn viewport_rejects_non_finite_time() {
        assert!(!Viewport {
            width: 100.0,
            height: 100.0,
            scale_factor: 1.0,
            time_seconds: f32::NAN,
        }
        .valid());
    }

    #[test]
    fn embedded_shader_parses_and_validates() {
        let module = naga::front::wgsl::parse_str(include_str!("shader.wgsl"))
            .expect("primitive shader must parse");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("primitive shader must validate");
    }
}
