
struct Uniforms {
    viewport: vec2<f32>,
    texture_size: vec2<u32>,
    head: u32,
    filled: u32,
    waterfall_top: f32,
    _pad: f32,
};
@group(0) @binding(0) var field: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u: Uniforms;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};
@vertex fn vs_main(@builtin(vertex_index) i: u32) -> VertexOut {
    var p = array<vec2<f32>, 3>(vec2<f32>(-1.0,-1.0), vec2<f32>(3.0,-1.0), vec2<f32>(-1.0,3.0));
    var t = array<vec2<f32>, 3>(vec2<f32>(0.0,1.0), vec2<f32>(2.0,1.0), vec2<f32>(0.0,-1.0));
    var out: VertexOut;
    out.position = vec4(p[i], 0.0, 1.0);
    out.uv = t[i];
    return out;
}

fn palette(v: f32) -> vec3<f32> {
    // RF floors occupy much of the normalized lower range. A gentle gamma
    // reveals that structure as traditional deep blue without moving peaks or
    // inventing samples; hot signals still reach yellow, red and white.
    let x = pow(clamp(v, 0.0, 1.0), 0.72) * 5.0;
    if x < 1.0 { return mix(vec3(0.002,0.006,0.035), vec3(0.0,0.02,0.24), x); }
    if x < 2.0 { return mix(vec3(0.0,0.02,0.24), vec3(0.0,0.72,0.96), x-1.0); }
    if x < 3.0 { return mix(vec3(0.0,0.72,0.96), vec3(0.98,0.92,0.05), x-2.0); }
    if x < 4.0 { return mix(vec3(0.98,0.92,0.05), vec3(0.96,0.08,0.01), x-3.0); }
    return mix(vec3(0.96,0.08,0.01), vec3(1.0), x-4.0);
}

@fragment fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    if in.uv.y < u.waterfall_top { return vec4(0.0); }
    let local_y = (in.uv.y - u.waterfall_top) / max(1.0 - u.waterfall_top, 0.001);
    let logical_y = min(u32(local_y * f32(u.texture_size.y)), u.texture_size.y - 1u);
    // Unobserved history is opaque and visibly blue-black, but remains
    // distinct from real low-energy samples. It fills naturally with time.
    if logical_y >= u.filled { return vec4(0.002, 0.006, 0.025, 1.0); }
    let physical_y = (u.head + logical_y) % u.texture_size.y;
    let x = min(u32(clamp(in.uv.x, 0.0, 0.999999) * f32(u.texture_size.x)), u.texture_size.x - 1u);
    return vec4(palette(textureLoad(field, vec2<i32>(i32(x), i32(physical_y)), 0).r), 1.0);
}
