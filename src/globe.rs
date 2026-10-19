use std::borrow::Cow;

use bytemuck::{Pod, Zeroable};
use thiserror::Error;
use wgpu::util::DeviceExt;

use crate::{
    relative_to_origin_f32, wgs84_ellipsoid_mesh, wgs84_surface_patch, EllipsoidVertex,
    SpatialError,
};

const DEFAULT_LATITUDE_SEGMENTS: u32 = 64;
const DEFAULT_LONGITUDE_SEGMENTS: u32 = 128;

const SHADER: &str = r"
struct FrameUniforms {
    view_projection: mat4x4<f32>,
    earth: vec4<f32>,
    land: vec4<f32>,
    boundary: vec4<f32>,
    atmosphere: vec4<f32>,
    raster_bounds: vec4<f32>,
    detail_bounds: vec4<f32>,
    viewport: vec2<f32>,
    _padding: vec2<f32>,
};

@group(0) @binding(0) var<uniform> frame: FrameUniforms;
@group(0) @binding(1) var earth_raster: texture_2d<f32>;
@group(0) @binding(2) var earth_sampler: sampler;
@group(0) @binding(3) var detail_raster: texture_2d<f32>;

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
    let longitude = input.geo_degrees.y;
    let latitude = input.geo_degrees.x;
    let inside_raster = longitude >= frame.raster_bounds.x
        && longitude <= frame.raster_bounds.z
        && latitude >= frame.raster_bounds.y
        && latitude <= frame.raster_bounds.w;
    let raster_uv = vec2<f32>(
        (longitude - frame.raster_bounds.x) / (frame.raster_bounds.z - frame.raster_bounds.x),
        (frame.raster_bounds.w - latitude) / (frame.raster_bounds.w - frame.raster_bounds.y)
    );
    var surface = frame.land;
    if inside_raster {
        surface = textureSample(earth_raster, earth_sampler, raster_uv);
    }
    let bounds = frame.detail_bounds;
    let detail_inside = bounds.x < bounds.z && longitude >= bounds.x && longitude <= bounds.z
        && latitude >= bounds.y && latitude <= bounds.w;
    if detail_inside {
        let mercator_lat = log(tan(0.78539816339 + radians(latitude) * 0.5));
        let mercator_north = log(tan(0.78539816339 + radians(bounds.w) * 0.5));
        let mercator_south = log(tan(0.78539816339 + radians(bounds.y) * 0.5));
        let uv = vec2<f32>((longitude - bounds.x) / (bounds.z - bounds.x),
            (mercator_north - mercator_lat) / (mercator_north - mercator_south));
        surface = textureSample(detail_raster, earth_sampler, uv);
    }
    let latitude_phase = fract((input.geo_degrees.x + 90.0) / 10.0);
    let longitude_phase = fract((input.geo_degrees.y + 180.0) / 10.0);
    let latitude_cell = min(latitude_phase, 1.0 - latitude_phase);
    let longitude_cell = min(longitude_phase, 1.0 - longitude_phase);
    let latitude_width = max(fwidth(latitude_cell), 0.0001);
    let longitude_width = max(fwidth(longitude_cell), 0.0001);
    let latitude_line = 1.0 - smoothstep(0.0, latitude_width * 1.5, latitude_cell);
    let longitude_line = 1.0 - smoothstep(0.0, longitude_width * 1.5, longitude_cell);
    let grid = clamp(max(latitude_line, longitude_line), 0.0, 1.0);
    return mix(surface, frame.boundary, grid * frame.boundary.a * 0.32);
}

struct PointInput {
    @location(0) center: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) size_px: f32,
    @location(3) tangent: vec3<f32>,
    @location(4) glyph: f32,
};

struct PointOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local: vec2<f32>,
    @location(2) @interpolate(flat) glyph: f32,
};

@vertex
fn point_vertex(input: PointInput, @builtin(vertex_index) vertex_index: u32) -> PointOutput {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0)
    );
    let local = corners[vertex_index];
    if input.glyph > 2.5 && input.glyph < 3.5 {
        let start = frame.view_projection * vec4<f32>(input.center - input.tangent, 1.0);
        let end = frame.view_projection * vec4<f32>(input.center + input.tangent, 1.0);
        var line: PointOutput;
        line.color = input.color;
        line.local = local;
        line.glyph = input.glyph;
        line.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        if start.w <= 0.0 || end.w <= 0.0 { return line; }
        let delta = (end.xy / end.w - start.xy / start.w) * frame.viewport;
        let normal = vec2<f32>(-delta.y, delta.x) / max(length(delta), 0.00001);
        var clip = mix(start, end, (local.x + 1.0) * 0.5);
        let offset = normal * local.y * input.size_px * 2.0 / frame.viewport * clip.w;
        clip.x += offset.x;
        clip.y += offset.y;
        line.position = clip;
        return line;
    }
    var clip = frame.view_projection * vec4<f32>(input.center, 1.0);
    let pixel_to_clip = vec2<f32>(2.0 / frame.viewport.x, 2.0 / frame.viewport.y);
    var direction = vec2<f32>(0.0, 1.0);
    if length(input.tangent) > 0.0 {
        let tip = frame.view_projection * vec4<f32>(input.center + input.tangent * 1000.0, 1.0);
        let delta = (tip.xy / tip.w - clip.xy / clip.w) * frame.viewport;
        if length(delta) > 0.0001 { direction = normalize(delta); }
    }
    let rotated = vec2<f32>(local.x * direction.y + local.y * direction.x,
        -local.x * direction.x + local.y * direction.y);
    let offset = rotated * input.size_px * pixel_to_clip * clip.w;
    clip.x += offset.x;
    clip.y += offset.y;
    var output: PointOutput;
    output.position = clip;
    output.color = input.color;
    output.local = local;
    output.glyph = input.glyph;
    return output;
}

@fragment
fn point_fragment(input: PointOutput) -> @location(0) vec4<f32> {
    if input.glyph >= 100.0 {
        let count = u32(input.glyph - 100.0);
        var digits = 1u;
        if count >= 10u { digits = 2u; }
        if count >= 100u { digits = 3u; }
        if count >= 1000u { digits = 4u; }
        if count >= 10000u { digits = 5u; }
        if count >= 100000u { digits = 6u; }
        let powers = array<u32, 6>(1u, 10u, 100u, 1000u, 10000u, 100000u);
        let masks = array<u32, 10>(63u, 6u, 91u, 79u, 102u, 109u, 125u, 7u, 127u, 111u);
        let segments = array<vec4<f32>, 7>(
            vec4<f32>(-0.32, 0.5, 0.32, 0.5), vec4<f32>(0.32, 0.5, 0.32, 0.0),
            vec4<f32>(0.32, 0.0, 0.32, -0.5), vec4<f32>(-0.32, -0.5, 0.32, -0.5),
            vec4<f32>(-0.32, 0.0, -0.32, -0.5), vec4<f32>(-0.32, 0.5, -0.32, 0.0),
            vec4<f32>(-0.32, 0.0, 0.32, 0.0));
        let cell = 1.28 / f32(digits);
        var distance = 10.0;
        for (var digit_index = 0u; digit_index < digits; digit_index += 1u) {
            let digit = (count / powers[digits - 1u - digit_index]) % 10u;
            let center = vec2<f32>(-0.64 + cell * (f32(digit_index) + 0.5), 0.0);
            for (var segment = 0u; segment < 7u; segment += 1u) {
                if (masks[digit] & (1u << segment)) != 0u {
                    let a = center + segments[segment].xy * vec2<f32>(cell * 1.1, 0.68);
                    let b = center + segments[segment].zw * vec2<f32>(cell * 1.1, 0.68);
                    let ab = b - a;
                    let t = clamp(dot(input.local - a, ab) / max(dot(ab, ab), 0.000001), 0.0, 1.0);
                    distance = min(distance, length(input.local - a - ab * t));
                }
            }
        }
        let box_distance = length(max(abs(input.local) - vec2<f32>(0.78, 0.49), vec2<f32>(0.0))) - 0.12;
        if box_distance > 0.05 { discard; }
        let border = 1.0 - smoothstep(0.015, 0.055, abs(box_distance));
        let number = 1.0 - smoothstep(0.025, 0.060, distance);
        let color = mix(frame.earth.rgb, input.color.rgb, max(border, number));
        return vec4<f32>(color, input.color.a * 0.95 * (1.0 - smoothstep(0.0, 0.05, box_distance)));
    }
    if input.glyph > 2.5 && input.glyph < 3.5 {
        let edge = 1.0 - smoothstep(0.6, 1.0, abs(input.local.y));
        return vec4<f32>(input.color.rgb, input.color.a * edge);
    }
    if input.glyph > 0.5 {
        var hull = array<vec2<f32>, 15>(
            vec2<f32>(0.0, 0.88), vec2<f32>(-0.28, 0.40),
            vec2<f32>(-0.28, -0.72), vec2<f32>(0.28, -0.72),
            vec2<f32>(0.28, 0.40), vec2<f32>(0.0, 0.88),
            vec2<f32>(0.0), vec2<f32>(0.0), vec2<f32>(0.0),
            vec2<f32>(0.0), vec2<f32>(0.0), vec2<f32>(0.0),
            vec2<f32>(0.0), vec2<f32>(0.0), vec2<f32>(0.0));
        var count = 6u;
        if input.glyph > 1.5 {
            hull = array<vec2<f32>, 15>(
                vec2<f32>(0.0, 0.9), vec2<f32>(-0.12, 0.2),
                vec2<f32>(-0.82, -0.18), vec2<f32>(-0.82, -0.35),
                vec2<f32>(-0.12, -0.12), vec2<f32>(-0.12, -0.62),
                vec2<f32>(-0.36, -0.82), vec2<f32>(0.36, -0.82),
                vec2<f32>(0.12, -0.62), vec2<f32>(0.12, -0.12),
                vec2<f32>(0.82, -0.35), vec2<f32>(0.82, -0.18),
                vec2<f32>(0.12, 0.2), vec2<f32>(0.0, 0.9), vec2<f32>(0.0, 0.9));
            count = 14u;
        }
        var distance = 10.0;
        for (var i = 1u; i < count; i += 1u) {
            let a = hull[i - 1u];
            let ab = hull[i] - a;
            let along = clamp(dot(input.local - a, ab) / max(dot(ab, ab), 0.000001), 0.0, 1.0);
            distance = min(distance, length(input.local - a - ab * along));
        }
        let core = 1.0 - smoothstep(0.045, 0.105, distance);
        let glow = (1.0 - smoothstep(0.04, 0.23, distance)) * 0.20;
        let alpha = max(core, glow) * input.color.a;
        if alpha < 0.01 { discard; }
        return vec4<f32>(input.color.rgb, alpha);
    }
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
    raster_bounds: [f32; 4],
    detail_bounds: [f32; 4],
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
    tangent: [f32; 3],
    glyph: f32,
}

/// Recognizable outline shape for a globe billboard.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GlobeMarkerKind {
    /// The existing soft circular observation point.
    #[default]
    Observation,
    /// A narrow hull with a pointed bow.
    Vessel,
    /// Fixed-wing aircraft silhouette.
    Aircraft,
    /// A flat numeric traffic badge. Count must be in `1..=999999`.
    Cluster(u32),
}

/// Optional shape and physical heading for a retained globe point.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GlobeMarkerStyle {
    /// Shape; interpretation of source domains remains the caller's responsibility.
    pub kind: GlobeMarkerKind,
    /// Unit ECEF tangent direction. Zero means an unoriented, upright symbol.
    pub tangent_ecef: [f64; 3],
}

/// A depth-tested geographic line segment in canonical ECEF metres.
///
/// Callers tessellate long routes along their supplied surface, rather than
/// raising endpoints into an airborne arc. Width remains readable at any zoom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobeLineSegment {
    /// Canonical starting position.
    pub start_ecef_m: [f64; 3],
    /// Canonical ending position.
    pub end_ecef_m: [f64; 3],
    /// Linear RGBA stroke color.
    pub color: [f32; 4],
    /// Full physical-pixel stroke width.
    pub width_px: f32,
}

impl GlobeMarkerStyle {
    fn glyph(self) -> f32 {
        match self.kind {
            GlobeMarkerKind::Observation => 0.0,
            GlobeMarkerKind::Vessel => 1.0,
            GlobeMarkerKind::Aircraft => 2.0,
            GlobeMarkerKind::Cluster(count) => 100.0 + count as f32,
        }
    }
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
    /// Regional raster bounds must be finite, ordered WGS84 degrees.
    #[error("globe regional raster bounds are invalid")]
    InvalidRasterBounds,
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

struct SurfacePatch {
    canonical: Vec<EllipsoidVertex>,
    relative: Vec<EarthVertex>,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
}

impl SurfacePatch {
    fn new(device: &wgpu::Device, bounds: [f64; 4]) -> Result<Self, GlobeRenderError> {
        let mesh = wgs84_surface_patch(bounds, 32, 32)?;
        let relative = vec![EarthVertex::zeroed(); mesh.vertices.len()];
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fpl-gfx detailed surface patch"),
            size: (relative.len() * std::mem::size_of::<EarthVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("fpl-gfx surface patch indices"),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        Ok(Self {
            canonical: mesh.vertices,
            relative,
            vertex_buffer,
            index_buffer,
            index_count: mesh.indices.len() as u32,
        })
    }
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
    detail_texture: wgpu::Texture,
    raster_sampler: wgpu::Sampler,
    raster_bounds: [f32; 4],
    detail_bounds: [f32; 4],
    surface_patch: Option<SurfacePatch>,
    uniform_buffer: wgpu::Buffer,
    earth_vertex_buffer: wgpu::Buffer,
    earth_index_buffer: wgpu::Buffer,
    point_instance_buffer: wgpu::Buffer,
    earth_canonical: Vec<EllipsoidVertex>,
    earth_relative: Vec<EarthVertex>,
    earth_index_count: u32,
    points: Vec<GlobePoint>,
    lines: Vec<GlobeLineSegment>,
    marker_styles: Vec<GlobeMarkerStyle>,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let detail_texture = raster_texture(device, 1, 1);
        let raster_texture = raster_texture(device, 1, 1);
        let raster_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fpl-gfx globe raster sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
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
            &detail_texture,
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
            detail_texture,
            raster_sampler,
            raster_bounds: [-180.0, -90.0, 180.0, 90.0],
            detail_bounds: [0.0; 4],
            surface_patch: None,
            uniform_buffer,
            earth_vertex_buffer,
            earth_index_buffer,
            point_instance_buffer,
            earth_canonical: mesh.vertices,
            earth_relative,
            earth_index_count,
            points: Vec::new(),
            lines: Vec::new(),
            marker_styles: Vec::new(),
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
        self.set_raster_region_rgba8(
            device,
            queue,
            width,
            height,
            [-180.0, -90.0, 180.0, 90.0],
            rgba,
        )
    }

    /// Replaces imagery covering one non-antimeridian WGS84 region.
    ///
    /// Bounds are `[west, south, east, north]` in degrees. Pixels are expected
    /// in north-up order, matching ordinary WMS and image conventions.
    pub fn set_raster_region_rgba8(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        bounds_degrees: [f64; 4],
        rgba: &[u8],
    ) -> Result<(), GlobeRenderError> {
        validate_raster_bounds(bounds_degrees)?;
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
            &self.detail_texture,
        );
        self.raster_texture = texture;
        self.raster_bounds = bounds_degrees.map(|value| value as f32);
        Ok(())
    }

    /// Updates Web Mercator detail over the retained global basemap.
    /// Bounds are west/south/east/north degrees; texels are north-up XYZ imagery.
    ///
    /// # Errors
    /// Returns an invalid-raster error for unordered bounds, polar extents,
    /// zero dimensions, overflow or a mismatched RGBA payload.
    pub fn set_mercator_detail_rgba8(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        bounds: [f64; 4],
        rgba: &[u8],
    ) -> Result<(), GlobeRenderError> {
        validate_raster_bounds(bounds)?;
        if bounds[1] < -85.051_129
            || bounds[3] > 85.051_129
            || width == 0
            || height == 0
            || width
                .checked_mul(height)
                .and_then(|pixels| pixels.checked_mul(4))
                .map(|bytes| bytes as usize)
                != Some(rgba.len())
        {
            return Err(GlobeRenderError::InvalidRaster);
        }
        let texture = raster_texture(device, width, height);
        queue.write_texture(
            texture.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            texture.size(),
        );
        self.bind_group = raster_bind_group(
            device,
            &self.bind_group_layout,
            &self.uniform_buffer,
            &self.raster_texture,
            &self.raster_sampler,
            &texture,
        );
        self.detail_texture = texture;
        self.detail_bounds = bounds.map(|value| value as f32);
        self.surface_patch = if bounds[2] - bounds[0] <= 20.0 && bounds[3] - bounds[1] <= 20.0 {
            Some(SurfacePatch::new(device, bounds)?)
        } else {
            None
        };
        self.last_origin = None;
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
        if points
            .len()
            .checked_add(self.lines.len())
            .is_none_or(|count| count > u32::MAX as usize)
        {
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
        self.marker_styles.clear();
        self.marker_styles
            .resize(points.len(), GlobeMarkerStyle::default());
        self.resize_instances(device)?;
        self.point_count =
            u32::try_from(self.points.len()).map_err(|_| GlobeRenderError::TooManyPoints)?;
        self.last_origin = None;
        Ok(())
    }

    /// Replaces retained geographic strokes without replacing observation points.
    ///
    /// # Errors
    /// Returns an error for non-finite data, non-positive widths or a combined
    /// line/point count beyond `u32`.
    pub fn set_lines(
        &mut self,
        device: &wgpu::Device,
        lines: &[GlobeLineSegment],
    ) -> Result<(), GlobeRenderError> {
        if lines
            .len()
            .checked_add(self.points.len())
            .is_none_or(|count| count > u32::MAX as usize)
        {
            return Err(GlobeRenderError::TooManyPoints);
        }
        if lines.iter().any(|line| {
            line.start_ecef_m
                .into_iter()
                .chain(line.end_ecef_m)
                .chain(line.color.map(f64::from))
                .chain([f64::from(line.width_px)])
                .any(|value| !value.is_finite())
                || line.width_px <= 0.0
        }) {
            return Err(GlobeRenderError::InvalidPoint);
        }
        self.lines.clear();
        self.lines.extend_from_slice(lines);
        self.resize_instances(device)?;
        self.last_origin = None;
        Ok(())
    }

    fn resize_instances(&mut self, device: &wgpu::Device) -> Result<(), GlobeRenderError> {
        let count = self
            .points
            .len()
            .checked_add(self.lines.len())
            .filter(|count| *count <= u32::MAX as usize)
            .ok_or(GlobeRenderError::TooManyPoints)?;
        if count > self.point_capacity {
            self.point_capacity = count.next_power_of_two();
            self.point_instance_buffer = point_buffer(device, self.point_capacity);
        }
        self.point_instances.resize(count, PointInstance::zeroed());
        Ok(())
    }

    /// Replaces styles for all retained points, preserving the existing point API.
    ///
    /// # Errors
    /// Returns an error if the count differs or a tangent is neither zero nor
    /// a finite unit vector.
    pub fn set_marker_styles(
        &mut self,
        styles: &[GlobeMarkerStyle],
    ) -> Result<(), GlobeRenderError> {
        if styles.len() != self.points.len()
            || styles.iter().any(|style| {
                if matches!(style.kind, GlobeMarkerKind::Cluster(0 | 1_000_000..)) {
                    return true;
                }
                let length = style.tangent_ecef[0]
                    .hypot(style.tangent_ecef[1])
                    .hypot(style.tangent_ecef[2]);
                !length.is_finite() || (length != 0.0 && (length - 1.0).abs() > 0.000_001)
            })
        {
            return Err(GlobeRenderError::InvalidPoint);
        }
        self.marker_styles.copy_from_slice(styles);
        self.last_origin = None;
        Ok(())
    }

    /// Replaces a bounded range of retained points without invalidating the
    /// static prefix. When the camera-relative origin is unchanged, the next
    /// frame uploads only this range to the instance buffer.
    pub fn update_points(
        &mut self,
        queue: &wgpu::Queue,
        start: usize,
        points: &[GlobePoint],
    ) -> Result<(), GlobeRenderError> {
        let Some(end) = start.checked_add(points.len()) else {
            return Err(GlobeRenderError::TooManyPoints);
        };
        if end > self.points.len()
            || points.iter().any(|point| {
                point
                    .ecef_m
                    .into_iter()
                    .chain(point.color.map(f64::from))
                    .chain([f64::from(point.size_px)])
                    .any(|value| !value.is_finite())
                    || point.size_px <= 0.0
            })
        {
            return Err(GlobeRenderError::InvalidPoint);
        }
        self.points[start..end].copy_from_slice(points);
        let Some(origin) = self.last_origin else {
            return Ok(());
        };
        for ((target, source), style) in self.point_instances[start..end]
            .iter_mut()
            .zip(points)
            .zip(&self.marker_styles[start..end])
        {
            *target = PointInstance {
                center: relative_to_origin_f32(source.ecef_m, origin),
                color: source.color,
                size_px: source.size_px,
                tangent: style.tangent_ecef.map(|value| value as f32),
                glyph: style.glyph(),
            };
        }
        if !points.is_empty() {
            queue.write_buffer(
                &self.point_instance_buffer,
                (start * std::mem::size_of::<PointInstance>()) as u64,
                bytemuck::cast_slice(&self.point_instances[start..end]),
            );
        }
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
            raster_bounds: self.raster_bounds,
            detail_bounds: self.detail_bounds,
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
        if let Some(patch) = &self.surface_patch {
            pass.set_vertex_buffer(0, patch.vertex_buffer.slice(..));
            pass.set_index_buffer(patch.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..patch.index_count, 0, 0..1);
        }
        if !self.point_instances.is_empty() {
            pass.set_pipeline(&self.point_pipeline);
            pass.set_vertex_buffer(0, self.point_instance_buffer.slice(..));
            pass.draw(0..6, 0..self.point_instances.len() as u32);
        }
        Ok(GlobeRenderStats {
            earth_triangles: (self.earth_index_count
                + self
                    .surface_patch
                    .as_ref()
                    .map_or(0, |patch| patch.index_count))
                / 3,
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
        for ((target, source), style) in self.point_instances[..self.points.len()]
            .iter_mut()
            .zip(&self.points)
            .zip(&self.marker_styles)
        {
            *target = PointInstance {
                center: relative_to_origin_f32(source.ecef_m, origin),
                color: source.color,
                size_px: source.size_px,
                tangent: style.tangent_ecef.map(|value| value as f32),
                glyph: style.glyph(),
            };
        }
        for (target, line) in self.point_instances[self.points.len()..]
            .iter_mut()
            .zip(&self.lines)
        {
            let center =
                std::array::from_fn(|axis| line.start_ecef_m[axis].midpoint(line.end_ecef_m[axis]));
            *target = PointInstance {
                center: relative_to_origin_f32(center, origin),
                color: line.color,
                size_px: line.width_px * 0.5,
                tangent: std::array::from_fn(|axis| {
                    ((line.end_ecef_m[axis] - line.start_ecef_m[axis]) * 0.5) as f32
                }),
                glyph: 3.0,
            };
        }
        queue.write_buffer(
            &self.earth_vertex_buffer,
            0,
            bytemuck::cast_slice(&self.earth_relative),
        );
        if let Some(patch) = self.surface_patch.as_mut() {
            for (target, source) in patch.relative.iter_mut().zip(&patch.canonical) {
                *target = EarthVertex {
                    position: relative_to_origin_f32(source.ecef_m, origin),
                    coordinates_degrees: source.coordinates_degrees,
                };
            }
            queue.write_buffer(
                &patch.vertex_buffer,
                0,
                bytemuck::cast_slice(&patch.relative),
            );
        }
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

fn validate_raster_bounds(bounds: [f64; 4]) -> Result<(), GlobeRenderError> {
    let [west, south, east, north] = bounds;
    if bounds.iter().any(|value| !value.is_finite())
        || !(-180.0..=180.0).contains(&west)
        || !(-180.0..=180.0).contains(&east)
        || !(-90.0..=90.0).contains(&south)
        || !(-90.0..=90.0).contains(&north)
        || west >= east
        || south >= north
    {
        return Err(GlobeRenderError::InvalidRasterBounds);
    }
    Ok(())
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
    detail: &wgpu::Texture,
) -> wgpu::BindGroup {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let detail_view = detail.create_view(&wgpu::TextureViewDescriptor::default());
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
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&detail_view),
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
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 32,
                        shader_location: 3,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32,
                        offset: 44,
                        shader_location: 4,
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
        depth_stencil: Some(wgpu::DepthStencilState {
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::LessEqual,
            bias: wgpu::DepthBiasState {
                constant: -2,
                slope_scale: 0.0,
                clamp: 0.0,
            },
            ..depth_state(depth_format)
        }),
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
        assert_eq!(std::mem::size_of::<FrameUniforms>(), 176);
        assert_eq!(std::mem::size_of::<EarthVertex>(), 20);
        assert_eq!(std::mem::size_of::<PointInstance>(), 48);
    }

    #[test]
    fn regional_raster_bounds_are_ordered_and_bounded() {
        assert!(validate_raster_bounds([-123.0, 47.0, -122.0, 48.0]).is_ok());
        assert!(validate_raster_bounds([-122.0, 47.0, -123.0, 48.0]).is_err());
        assert!(validate_raster_bounds([-123.0, 48.0, -122.0, 47.0]).is_err());
        assert!(validate_raster_bounds([-181.0, 47.0, -122.0, 48.0]).is_err());
        assert!(validate_raster_bounds([f64::NAN, 47.0, -122.0, 48.0]).is_err());
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
            assert!(renderer.set_marker_styles(&[]).is_err());
            assert!(renderer
                .set_marker_styles(&[GlobeMarkerStyle {
                    kind: GlobeMarkerKind::Aircraft,
                    tangent_ecef: [f64::NAN, 0.0, 0.0],
                }])
                .is_err());
            renderer
                .set_marker_styles(&[GlobeMarkerStyle {
                    kind: GlobeMarkerKind::Vessel,
                    tangent_ecef: [0.0, 1.0, 0.0],
                }])
                .unwrap();
            let line = GlobeLineSegment {
                start_ecef_m: origin,
                end_ecef_m: crate::geodetic_to_ecef(47.61, -122.31, 0.0),
                color: [0.1, 0.4, 0.6, 0.6],
                width_px: 2.0,
            };
            assert!(renderer
                .set_lines(
                    &device,
                    &[GlobeLineSegment {
                        width_px: 0.0,
                        ..line
                    }]
                )
                .is_err());
            renderer.set_lines(&device, &[line]).unwrap();
            renderer
                .set_mercator_detail_rgba8(
                    &device,
                    &queue,
                    1,
                    1,
                    [-123.0, 47.0, -122.0, 48.0],
                    &[20, 100, 40, 255],
                )
                .unwrap();
            assert_eq!(renderer.detail_bounds, [-123.0, 47.0, -122.0, 48.0]);
            assert!(renderer
                .set_mercator_detail_rgba8(&device, &queue, 1, 1, [-123.0, 47.0, -122.0, 48.0], &[])
                .is_err());
            // Updating the base texture must preserve the detail binding.
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
            assert_eq!(renderer.point_instances.len(), 2);
            assert_eq!(renderer.point_instances[1].glyph, 3.0);
            assert_eq!(
                renderer.surface_patch.as_ref().unwrap().canonical.len(),
                33 * 33
            );
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
