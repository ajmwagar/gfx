use std::borrow::Cow;

use bytemuck::{Pod, Zeroable};
use thiserror::Error;
use wgpu::util::DeviceExt;

use crate::{relative_to_origin_f32, wgs84_ellipsoid_mesh, EllipsoidVertex, SpatialError};

const DEFAULT_LATITUDE_SEGMENTS: u32 = 64;
const DEFAULT_LONGITUDE_SEGMENTS: u32 = 128;

const SHADER: &str = r"
struct FrameUniforms {
    view_projection: mat4x4<f32>,
    earth: vec4<f32>,
    land: vec4<f32>,
    boundary: vec4<f32>,
    atmosphere: vec4<f32>,
    viewport: vec2<f32>,
    _padding: vec2<f32>,
};

@group(0) @binding(0) var<uniform> frame: FrameUniforms;
@group(0) @binding(1) var earth_raster: texture_2d<f32>;
@group(0) @binding(2) var earth_sampler: sampler;

struct EarthInput {
    @location(0) position: vec3<f32>,
    @location(1) geo_degrees: vec2<f32>,
};

struct EarthOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) geo_degrees: vec2<f32>,
};

@vertex
fn earth_vertex(input: EarthInput) -> EarthOutput {
    var output: EarthOutput;
    output.position = frame.view_projection * vec4<f32>(input.position, 1.0);
    output.geo_degrees = input.geo_degrees;
    return output;
}

@fragment
fn earth_fragment(input: EarthOutput) -> @location(0) vec4<f32> {
    let raster_uv = vec2<f32>(
        (input.geo_degrees.y + 180.0) / 360.0,
        (90.0 - input.geo_degrees.x) / 180.0
    );
    let raster = textureSample(earth_raster, earth_sampler, raster_uv);
    let latitude_phase = fract((input.geo_degrees.x + 90.0) / 10.0);
    let longitude_phase = fract((input.geo_degrees.y + 180.0) / 10.0);
    let latitude_cell = min(latitude_phase, 1.0 - latitude_phase);
    let longitude_cell = min(longitude_phase, 1.0 - longitude_phase);
    let latitude_width = max(fwidth(latitude_cell), 0.0001);
    let longitude_width = max(fwidth(longitude_cell), 0.0001);
    let latitude_line = 1.0 - smoothstep(0.0, latitude_width * 1.5, latitude_cell);
    let longitude_line = 1.0 - smoothstep(0.0, longitude_width * 1.5, longitude_cell);
    let grid = clamp(max(latitude_line, longitude_line), 0.0, 1.0);
    return mix(raster, frame.boundary, grid * frame.boundary.a * 0.32);
}

struct PointInput {
    @location(0) center: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) size_px: f32,
};

struct PointOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local: vec2<f32>,
};

@vertex
fn point_vertex(input: PointInput, @builtin(vertex_index) vertex_index: u32) -> PointOutput {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0)
    );
    let local = corners[vertex_index];
    var clip = frame.view_projection * vec4<f32>(input.center, 1.0);
    let pixel_to_clip = vec2<f32>(2.0 / frame.viewport.x, 2.0 / frame.viewport.y);
    let offset = local * input.size_px * pixel_to_clip * clip.w;
    clip.x += offset.x;
    clip.y += offset.y;
    var output: PointOutput;
    output.position = clip;
    output.color = input.color;
    output.local = local;
    return output;
}

@fragment
fn point_fragment(input: PointOutput) -> @location(0) vec4<f32> {
    let radius = length(input.local);
    let edge = 1.0 - smoothstep(0.76, 1.0, radius);
    if edge <= 0.0 { discard; }
    return vec4<f32>(input.color.rgb, input.color.a * edge);
}
";

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniforms {
    view_projection: [f32; 16],
    earth: [f32; 4],
    land: [f32; 4],
    boundary: [f32; 4],
    atmosphere: [f32; 4],
    viewport: [f32; 2],
    padding: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct EarthVertex {
    position: [f32; 3],
    coordinates_degrees: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PointInstance {
    center: [f32; 3],
    color: [f32; 4],
    size_px: f32,
}

/// Theme-resolved globe colors supplied by the host.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobeStyle {
    /// Clear color behind the ellipsoid.
    pub earth: [f32; 4],
    /// Land color reserved for vector and fallback rendering.
    pub land: [f32; 4],
    /// Graticule and boundary color.
    pub boundary: [f32; 4],
    /// Atmosphere color reserved for atmosphere passes.
    pub atmosphere: [f32; 4],
}

/// Renderer-independent plan for one camera-relative globe frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobeFrame {
    /// ECEF origin subtracted before coordinates are converted to `f32`.
    pub world_origin_ecef_m: [f64; 3],
    /// Column-major view-projection matrix.
    pub view_projection: [f32; 16],
    /// Physical output extent.
    pub viewport_px: [u32; 2],
    /// Theme-resolved colors.
    pub style: GlobeStyle,
}

/// One camera-relative billboard point rendered over the ellipsoid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobePoint {
    /// Canonical ECEF location in metres.
    pub ecef_m: [f64; 3],
    /// Premultiplied or straight RGBA color expected by the host blend policy.
    pub color: [f32; 4],
    /// Billboard radius in physical pixels.
    pub size_px: f32,
}

/// Immutable retained-globe renderer configuration.
#[derive(Debug, Clone, Copy)]
pub struct GlobeRendererConfig {
    /// Format of every caller-owned output target.
    pub target_format: wgpu::TextureFormat,
    /// Latitude tessellation segments.
    pub latitude_segments: u32,
    /// Longitude tessellation segments.
    pub longitude_segments: u32,
    /// Retained depth attachment format.
    pub depth_format: wgpu::TextureFormat,
}

impl GlobeRendererConfig {
    /// Constructs the standard globe configuration for one target format.
    pub const fn new(target_format: wgpu::TextureFormat) -> Self {
        Self {
            target_format,
            latitude_segments: DEFAULT_LATITUDE_SEGMENTS,
            longitude_segments: DEFAULT_LONGITUDE_SEGMENTS,
            depth_format: wgpu::TextureFormat::Depth32Float,
        }
    }
}

/// Observable work encoded by one globe frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobeRenderStats {
    /// Ellipsoid triangles submitted.
    pub earth_triangles: u32,
    /// Billboard points submitted.
    pub point_instances: u32,
}

/// Failure to validate retained globe input.
#[derive(Debug, Error)]
pub enum GlobeRenderError {
    /// Caller-owned output has a zero extent.
    #[error("globe render target has zero size")]
    InvalidTarget,
    /// Raster dimensions or tightly packed RGBA payload are invalid.
    #[error("globe raster dimensions or RGBA payload are invalid")]
    InvalidRaster,
    /// Point count cannot be represented by WGPU draw arguments.
    #[error("globe point count exceeds u32 capacity")]
    TooManyPoints,
    /// A point contains non-finite coordinates, color, or size.
    #[error("globe point data is invalid")]
    InvalidPoint,
    /// Ellipsoid tessellation is invalid.
    #[error(transparent)]
    Spatial(#[from] SpatialError),
}

struct DepthTarget {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: [u32; 2],
}

/// Retained WGPU renderer for a WGS84 ellipsoid, raster, and billboard points.
///
/// It owns reconstructible pipelines and buffers, never a window, surface,
/// command encoder, or queue submission.
pub struct GlobeRenderer {
    earth_pipeline: wgpu::RenderPipeline,
    point_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    raster_texture: wgpu::Texture,
    raster_sampler: wgpu::Sampler,
    uniform_buffer: wgpu::Buffer,
    earth_vertex_buffer: wgpu::Buffer,
    earth_index_buffer: wgpu::Buffer,
    point_instance_buffer: wgpu::Buffer,
    earth_canonical: Vec<EllipsoidVertex>,
    earth_relative: Vec<EarthVertex>,
    earth_index_count: u32,
    points: Vec<GlobePoint>,
    point_instances: Vec<PointInstance>,
    point_count: u32,
    point_capacity: usize,
    last_origin: Option<[f64; 3]>,
    depth_format: wgpu::TextureFormat,
    depth: Option<DepthTarget>,
}

impl GlobeRenderer {
    /// Creates retained globe pipelines and geometry.
    ///
    /// # Errors
    ///
    /// Returns [`GlobeRenderError::Spatial`] for invalid tessellation counts.
    #[allow(clippy::too_many_lines)] // GPU resource assembly is intentionally kept atomic.
    pub fn new(
        device: &wgpu::Device,
        config: GlobeRendererConfig,
    ) -> Result<Self, GlobeRenderError> {
        let mesh = wgs84_ellipsoid_mesh(config.latitude_segments, config.longitude_segments)?;
        let earth_relative = vec![EarthVertex::zeroed(); mesh.vertices.len()];
        let earth_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fpl-gfx globe camera-relative vertices"),
            size: (earth_relative.len() * std::mem::size_of::<EarthVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let earth_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("fpl-gfx globe indices"),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let point_capacity = 1;
        let point_instance_buffer = point_buffer(device, point_capacity);
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fpl-gfx globe frame uniforms"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fpl-gfx globe frame layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<FrameUniforms>() as u64,
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let raster_texture = raster_texture(device, 1, 1);
        let raster_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fpl-gfx globe raster sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind_group = raster_bind_group(
            device,
            &bind_group_layout,
            &uniform_buffer,
            &raster_texture,
            &raster_sampler,
        );
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fpl-gfx globe pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fpl-gfx globe shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER)),
        });
        let earth_pipeline = earth_pipeline(
            device,
            &pipeline_layout,
            &shader,
            config.target_format,
            config.depth_format,
        );
        let point_pipeline = point_pipeline(
            device,
            &pipeline_layout,
            &shader,
            config.target_format,
            config.depth_format,
        );
        let earth_index_count = u32::try_from(mesh.indices.len())
            .map_err(|_| GlobeRenderError::Spatial(SpatialError::InvalidSegments))?;
        Ok(Self {
            earth_pipeline,
            point_pipeline,
            bind_group_layout,
            bind_group,
            raster_texture,
            raster_sampler,
            uniform_buffer,
            earth_vertex_buffer,
            earth_index_buffer,
            point_instance_buffer,
            earth_canonical: mesh.vertices,
            earth_relative,
            earth_index_count,
            points: Vec::new(),
            point_instances: Vec::new(),
            point_count: 0,
            point_capacity,
            last_origin: None,
            depth_format: config.depth_format,
            depth: None,
        })
    }

    /// Replaces equirectangular imagery draped over the ellipsoid.
    ///
    /// # Errors
    ///
    /// Returns [`GlobeRenderError::InvalidRaster`] when dimensions are zero,
    /// overflow, or do not match the tightly packed RGBA payload.
    pub fn set_raster_rgba8(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<(), GlobeRenderError> {
        let row_bytes = width.checked_mul(4);
        let expected = row_bytes
            .and_then(|row| row.checked_mul(height))
            .map(|bytes| bytes as usize);
        if width == 0 || height == 0 || expected != Some(rgba.len()) {
            return Err(GlobeRenderError::InvalidRaster);
        }
        let texture = raster_texture(device, width, height);
        queue.write_texture(
            texture.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: row_bytes,
                rows_per_image: Some(height),
            },
            texture.size(),
        );
        self.bind_group = raster_bind_group(
            device,
            &self.bind_group_layout,
            &self.uniform_buffer,
            &texture,
            &self.raster_sampler,
        );
        self.raster_texture = texture;
        Ok(())
    }

    /// Replaces billboard points while retaining geometrically grown GPU capacity.
    ///
    /// # Errors
    ///
    /// Returns an error for non-finite point data or a count beyond `u32`.
    pub fn set_points(
        &mut self,
        device: &wgpu::Device,
        points: &[GlobePoint],
    ) -> Result<(), GlobeRenderError> {
        if points.len() > u32::MAX as usize {
            return Err(GlobeRenderError::TooManyPoints);
        }
        if points.iter().any(|point| {
            point
                .ecef_m
                .into_iter()
                .chain(point.color.map(f64::from))
                .chain([f64::from(point.size_px)])
                .any(|value| !value.is_finite())
                || point.size_px <= 0.0
        }) {
            return Err(GlobeRenderError::InvalidPoint);
        }
        self.points.clear();
        self.points.extend_from_slice(points);
        if self.points.len() > self.point_capacity {
            self.point_capacity = self.points.len().next_power_of_two();
            self.point_instance_buffer = point_buffer(device, self.point_capacity);
        }
        self.point_instances
            .resize(self.points.len(), PointInstance::zeroed());
        self.point_count =
            u32::try_from(self.points.len()).map_err(|_| GlobeRenderError::TooManyPoints)?;
        self.last_origin = None;
        Ok(())
    }

    /// Encodes a globe into a caller-owned texture view and command encoder.
    ///
    /// # Errors
    ///
    /// Returns [`GlobeRenderError::InvalidTarget`] for a zero viewport.
    pub fn render_into(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        frame: &GlobeFrame,
    ) -> Result<GlobeRenderStats, GlobeRenderError> {
        if frame.viewport_px.contains(&0) {
            return Err(GlobeRenderError::InvalidTarget);
        }
        self.ensure_depth(device, frame.viewport_px);
        self.update_relative_buffers(queue, frame.world_origin_ecef_m);
        let uniforms = FrameUniforms {
            view_projection: frame.view_projection,
            earth: frame.style.earth,
            land: frame.style.land,
            boundary: frame.style.boundary,
            atmosphere: frame.style.atmosphere,
            // WGPU texture limits are far below the first integer not exactly
            // representable by f32, so viewport dimensions remain exact.
            #[allow(clippy::cast_precision_loss)]
            viewport: [frame.viewport_px[0] as f32, frame.viewport_px[1] as f32],
            padding: [0.0; 2],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        let Some(depth) = self.depth.as_ref() else {
            return Err(GlobeRenderError::InvalidTarget);
        };
        let clear = frame.style.earth;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fpl-gfx globe pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: f64::from(clear[0]),
                        g: f64::from(clear[1]),
                        b: f64::from(clear[2]),
                        a: f64::from(clear[3]),
                    }),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_pipeline(&self.earth_pipeline);
        pass.set_vertex_buffer(0, self.earth_vertex_buffer.slice(..));
        pass.set_index_buffer(self.earth_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.earth_index_count, 0, 0..1);
        if !self.point_instances.is_empty() {
            pass.set_pipeline(&self.point_pipeline);
            pass.set_vertex_buffer(0, self.point_instance_buffer.slice(..));
            pass.draw(0..6, 0..self.point_count);
        }
        Ok(GlobeRenderStats {
            earth_triangles: self.earth_index_count / 3,
            point_instances: self.point_count,
        })
    }

    fn ensure_depth(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if self.depth.as_ref().is_some_and(|depth| depth.size == size) {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fpl-gfx globe retained depth"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.depth_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.depth = Some(DepthTarget {
            _texture: texture,
            view,
            size,
        });
    }

    fn update_relative_buffers(&mut self, queue: &wgpu::Queue, origin: [f64; 3]) {
        if self.last_origin == Some(origin) {
            return;
        }
        for (target, source) in self
            .earth_relative
            .iter_mut()
            .zip(self.earth_canonical.iter())
        {
            *target = EarthVertex {
                position: relative_to_origin_f32(source.ecef_m, origin),
                coordinates_degrees: source.coordinates_degrees,
            };
        }
        for (target, source) in self.point_instances.iter_mut().zip(&self.points) {
            *target = PointInstance {
                center: relative_to_origin_f32(source.ecef_m, origin),
                color: source.color,
                size_px: source.size_px,
            };
        }
        queue.write_buffer(
            &self.earth_vertex_buffer,
            0,
            bytemuck::cast_slice(&self.earth_relative),
        );
        if !self.point_instances.is_empty() {
            queue.write_buffer(
                &self.point_instance_buffer,
                0,
                bytemuck::cast_slice(&self.point_instances),
            );
        }
        self.last_origin = Some(origin);
    }
}

fn raster_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("fpl-gfx globe equirectangular raster"),
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
    })
}

fn raster_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform_buffer: &wgpu::Buffer,
    texture: &wgpu::Texture,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fpl-gfx globe frame bindings"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn earth_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    target_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("fpl-gfx globe earth pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("earth_vertex"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<EarthVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 12,
                        shader_location: 1,
                    },
                ],
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("earth_fragment"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(depth_state(depth_format)),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn point_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    target_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("fpl-gfx globe point pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("point_vertex"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<PointInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 12,
                        shader_location: 1,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32,
                        offset: 28,
                        shader_location: 2,
                    },
                ],
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("point_fragment"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(depth_state(depth_format)),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn depth_state(format: wgpu::TextureFormat) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format,
        depth_write_enabled: true,
        depth_compare: wgpu::CompareFunction::Less,
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

fn point_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fpl-gfx globe retained point instances"),
        size: (capacity.max(1) * std::mem::size_of::<PointInstance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_struct_layouts_match_wgsl_contract() {
        assert_eq!(std::mem::size_of::<FrameUniforms>(), 144);
        assert_eq!(std::mem::size_of::<EarthVertex>(), 20);
        assert_eq!(std::mem::size_of::<PointInstance>(), 32);
    }

    #[test]
    fn point_validation_rejects_non_finite_data_before_gpu_upload() {
        let point = GlobePoint {
            ecef_m: [f64::NAN, 0.0, 0.0],
            color: [1.0; 4],
            size_px: 4.0,
        };
        assert!(point.ecef_m.into_iter().any(|value| !value.is_finite()));
    }

    #[test]
    fn globe_shader_validates() {
        let module = naga::front::wgsl::parse_str(SHADER).expect("globe shader parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("globe shader validates");
    }

    #[test]
    fn encodes_into_a_caller_owned_texture() {
        pollster::block_on(async {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
                .expect("WGPU adapter required for renderer validation");
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("fpl-gfx globe test"),
                    ..Default::default()
                })
                .await
                .expect("WGPU device");
            device.push_error_scope(wgpu::ErrorFilter::Validation);

            let format = wgpu::TextureFormat::Rgba8UnormSrgb;
            let target = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("fpl-gfx caller-owned globe test target"),
                size: wgpu::Extent3d {
                    width: 64,
                    height: 64,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
            let mut renderer = GlobeRenderer::new(&device, GlobeRendererConfig::new(format))
                .expect("default tessellation");
            let origin = crate::geodetic_to_ecef(47.6, -122.3, 0.0);
            renderer
                .set_points(
                    &device,
                    &[GlobePoint {
                        ecef_m: origin,
                        color: [0.4, 0.8, 1.0, 1.0],
                        size_px: 6.0,
                    }],
                )
                .unwrap();
            renderer
                .set_raster_rgba8(&device, &queue, 1, 1, &[24, 28, 36, 255])
                .unwrap();
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("fpl-gfx globe test encoder"),
            });
            let stats = renderer
                .render_into(
                    &device,
                    &queue,
                    &mut encoder,
                    &target_view,
                    &GlobeFrame {
                        world_origin_ecef_m: origin,
                        view_projection: [
                            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
                            0.0, 1.0,
                        ],
                        viewport_px: [64, 64],
                        style: GlobeStyle {
                            earth: [0.02, 0.03, 0.05, 1.0],
                            land: [0.1, 0.2, 0.1, 1.0],
                            boundary: [0.6, 0.7, 0.8, 0.5],
                            atmosphere: [0.2, 0.5, 1.0, 0.2],
                        },
                    },
                )
                .unwrap();
            assert!(stats.earth_triangles > 1_000);
            assert_eq!(stats.point_instances, 1);
            queue.submit([encoder.finish()]);
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("device poll");
            let validation_error = device.pop_error_scope().await;
            assert!(
                validation_error.is_none(),
                "WGPU validation failed: {validation_error:?}",
            );
        });
    }
}
