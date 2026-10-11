
struct Uniforms {
    viewport: vec2<f32>,
    source: vec2<f32>,
    // 0 while the producer is writing frames, rising to 1 once it has stopped
    // for long enough that nothing on screen should be read as current.
    staleness: f32,
    // 1 only while a complete, matching alpha generation has a live lease.
    has_alpha: f32,
    // Scalars, not a vec3: a vec3 here aligns the struct to 16 and grows it to
    // 48 bytes against a 32-byte binding, which wgpu rejects outright.
    premultiplied: f32,
    native_alpha: f32,
};

@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var frame_sampler: sampler;
@group(0) @binding(2) var<uniform> u: Uniforms;
@group(0) @binding(3) var alpha_plane: texture_2d<f32>;

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
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let viewport_aspect = u.viewport.x / max(u.viewport.y, 1.0);
    let source_aspect = u.source.x / max(u.source.y, 1.0);
    var uv = in.uv;
    var content_size = u.viewport;
    if viewport_aspect > source_aspect {
        let width = source_aspect / viewport_aspect;
        if abs(uv.x - 0.5) > width * 0.5 { return vec4<f32>(0.0); }
        uv.x = (uv.x - 0.5) / width + 0.5;
        content_size.x = u.viewport.y * source_aspect;
    } else {
        let height = viewport_aspect / source_aspect;
        if abs(uv.y - 0.5) > height * 0.5 { return vec4<f32>(0.0); }
        uv.y = (uv.y - 0.5) / height + 0.5;
        content_size.y = u.viewport.x / source_aspect;
    }
    let frame_color = textureSample(frame, frame_sampler, uv);
    // H.264 has no alpha channel. A macOS window's transparent outer corners
    // therefore arrive as black pixels unless the compositor restores the
    // window silhouette. Clip at the fitted content boundary (not the widget
    // boundary) with a resolution-independent, softly antialiased radius.
    // The same restrained treatment makes camera/video surfaces sit cleanly
    // in Canvas without baking chrome into any producer.
    let radius = clamp(min(content_size.x, content_size.y) * 0.06, 8.0, 30.0);
    let point = uv * content_size;
    let q = abs(point - content_size * 0.5) - (content_size * 0.5 - vec2<f32>(radius));
    let distance = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
    let geometric_mask = 1.0 - smoothstep(-1.25, 1.25, distance);
    let exact_mask = textureSample(alpha_plane, frame_sampler, uv).r;
    let mask = select(select(geometric_mask, exact_mask, u.has_alpha > 0.5), 1.0, u.native_alpha > 0.5);
    // A camera that has stopped keeps its last frame on the texture forever, so
    // a dead feed has to be marked. NOT by draining colour: these cameras
    // switch to infrared at night and produce legitimately black-and-white
    // video, so a greyscale picture is normal exactly when it matters most.
    // Diagonal banding cannot come out of a camera, so it cannot be mistaken
    // for the scene. Computed in framebuffer space to keep the bands an even
    // width whatever size the surface is.
    let band = step(0.5, fract((in.position.x + in.position.y) / 24.0));
    let rgb = select(frame_color.rgb, frame_color.rgb / max(frame_color.a, 0.00001), u.premultiplied > 0.5);
    let dimmed = rgb * (1.0 - 0.45 * u.staleness);
    let barred = dimmed * (1.0 - 0.35 * u.staleness * band);
    return vec4<f32>(barred, frame_color.a * mask);
}
