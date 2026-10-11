//! Caller-owned targets and the shared mesh shader; no window or application clock.
use crate::{
    mesh::{Uniforms, Vertex, SHADER},
    RenderContext,
};
use wgpu::util::DeviceExt;

/// Borrowed triangle geometry and its shared shader uniforms.
pub struct Draw<'a> {
    /// World/model positions, unit normals and scalar values.
    pub vertices: &'a [Vertex],
    /// Triangle indices into the supplied vertices.
    pub indices: &'a [u32],
    /// Camera, rigid model transform and appearance.
    pub uniforms: Uniforms,
}
/// Shared mesh pipelines for hosts retaining their own buffers and render passes.
pub struct Pipelines {
    /// Two-sided filled triangles with depth writes and straight-alpha blending.
    pub triangles: wgpu::RenderPipeline,
    /// Indexed edge overlay with the same ABI and depth comparison.
    pub lines: wgpu::RenderPipeline,
    /// Binding zero accepts the shared mesh uniform buffer.
    pub layout: wgpu::BindGroupLayout,
}
impl Pipelines {
    /// Construct shared pipelines for a caller-owned color format.
    /// Depth targets passed to render must use `Depth32Float`, single sample.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gfx shared mesh"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gfx mesh uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gfx mesh layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let make_pipeline = |topology| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("gfx mesh pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Vertex::layout()],
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview: None,
                cache: None,
            })
        };
        let triangles = make_pipeline(wgpu::PrimitiveTopology::TriangleList);
        let lines = make_pipeline(wgpu::PrimitiveTopology::LineList);
        Self {
            triangles,
            lines,
            layout,
        }
    }
}

/// Shared two-sided triangle renderer for transient posed geometry.
pub struct Renderer {
    pipelines: Pipelines,
}
impl Renderer {
    /// Construct shared pipelines; retained hosts may use [`Pipelines`] directly.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self {
            pipelines: Pipelines::new(device, format),
        }
    }
    /// Validate, upload and encode mesh draws into caller-owned color/depth targets.
    ///
    /// Color clears to transparent black and depth to one. Buffers are per call;
    /// large retained-mesh hosts may continue using the same shader/ABI directly.
    /// # Errors
    /// Rejects nonfinite/invalid geometry, uniforms and more than 300k vertices
    /// or 900k indices, before opening a GPU pass.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "Match the caller-owned GFX RenderContext pass contract."
    )]
    pub fn render(
        &self,
        context: RenderContext<'_>,
        depth: &wgpu::TextureView,
        draws: &[Draw<'_>],
    ) -> Result<(), &'static str> {
        validate_draws(draws)?;
        let resources = draws
            .iter()
            .map(|draw| {
                let vertex = context
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("gfx posed vertices"),
                        contents: bytemuck::cast_slice(draw.vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
                let index = context
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("gfx mesh indices"),
                        contents: bytemuck::cast_slice(draw.indices),
                        usage: wgpu::BufferUsages::INDEX,
                    });
                let uniform =
                    context
                        .device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("gfx mesh uniform"),
                            contents: bytemuck::bytes_of(&draw.uniforms),
                            usage: wgpu::BufferUsages::UNIFORM,
                        });
                let bind = context
                    .device
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("gfx mesh bind"),
                        layout: &self.pipelines.layout,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: uniform.as_entire_binding(),
                        }],
                    });
                Ok((
                    vertex,
                    index,
                    bind,
                    u32::try_from(draw.indices.len()).map_err(|_| "index count overflow")?,
                ))
            })
            .collect::<Result<Vec<_>, &'static str>>()?;
        let mut pass = context
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gfx posed mesh pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: context.target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        pass.set_pipeline(&self.pipelines.triangles);
        for (vertex, index, bind, count) in &resources {
            pass.set_vertex_buffer(0, vertex.slice(..));
            pass.set_index_buffer(index.slice(..), wgpu::IndexFormat::Uint32);
            pass.set_bind_group(0, bind, &[]);
            pass.draw_indexed(0..*count, 0, 0..1);
        }
        Ok(())
    }
}

#[allow(
    clippy::float_cmp,
    reason = "Rigid homogeneous sentinel entries must be exact."
)]
fn validate_draws(draws: &[Draw<'_>]) -> Result<(), &'static str> {
    let mut vertices = 0usize;
    let mut indices = 0usize;
    for draw in draws {
        vertices = vertices
            .checked_add(draw.vertices.len())
            .ok_or("vertex budget overflow")?;
        indices = indices
            .checked_add(draw.indices.len())
            .ok_or("index budget overflow")?;
        if draw.vertices.is_empty()
            || draw.indices.is_empty()
            || draw.indices.len() % 3 != 0
            || vertices > 300_000
            || indices > 900_000
            || draw
                .indices
                .iter()
                .any(|&i| i as usize >= draw.vertices.len())
        {
            return Err("invalid mesh counts/indices");
        }
        if draw.vertices.iter().any(|v| {
            v.position.iter().chain(&v.normal).any(|x| !x.is_finite())
                || !v.field.is_finite()
                || !(0. ..=1.).contains(&v.field)
                || (v.normal.iter().map(|x| x * x).sum::<f32>() - 1.).abs() > 1e-4
        }) {
            return Err("invalid mesh vertex values");
        }
        let m = draw.uniforms.model;
        if m[0][3] != 0.
            || m[1][3] != 0.
            || m[2][3] != 0.
            || m[3][3] != 1.
            || (0..3).any(|i| {
                (0..3).any(|j| {
                    let dot = (0..3).map(|axis| m[i][axis] * m[j][axis]).sum::<f32>();
                    (dot - if i == j { 1. } else { 0. }).abs() > 1e-4
                })
            })
        {
            return Err("mesh model transform must be rigid");
        }
        if bytemuck::cast_slice::<Uniforms, f32>(&[draw.uniforms])
            .iter()
            .any(|x| !x.is_finite())
        {
            return Err("nonfinite mesh uniforms");
        }
    }
    Ok(())
}
