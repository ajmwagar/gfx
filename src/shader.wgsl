struct FrameUniform {
    viewport: vec4<f32>,
    colors: array<vec4<f32>, 16>,
    material: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> frame: FrameUniform;

struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @location(0) rect: vec4<f32>,
    @location(1) shape: vec4<f32>,
    @location(2) style: vec4<u32>,
    @location(3) material: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) shape: vec4<f32>,
    @location(3) @interpolate(flat) style: vec4<u32>,
    @location(4) material: vec4<f32>,
    @location(5) screen: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let local = corners[input.vertex_index];
    let pixel = input.rect.xy + local * input.rect.zw;
    let clip = vec2(
        pixel.x / frame.viewport.x * 2.0 - 1.0,
        1.0 - pixel.y / frame.viewport.y * 2.0,
    );
    var output: VertexOutput;
    output.position = vec4(clip, 0.0, 1.0);
    output.local = local;
    output.size = input.rect.zw;
    output.shape = input.shape;
    output.style = input.style;
    output.material = input.material;
    output.screen = pixel;
    return output;
}

fn saturate(value: f32) -> f32 {
    return clamp(value, 0.0, 1.0);
}

fn rounded_box_distance(point: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let bounded_radius = min(radius, min(half_size.x, half_size.y));
    let q = abs(point) - half_size + vec2(bounded_radius);
    return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - bounded_radius;
}

fn line_distance(point: vec2<f32>, start: vec2<f32>, end: vec2<f32>) -> f32 {
    let pa = point - start;
    let ba = end - start;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 0.0001), 0.0, 1.0);
    return length(pa - ba * h);
}

fn hash21(point: vec2<f32>) -> f32 {
    let p = fract(point * vec2(123.34, 456.21));
    return fract((p.x + p.y) * (p.x + 45.32));
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let point = (input.local - vec2(0.5)) * input.size;
    let half_size = input.size * 0.5;
    let kind = input.style.w;
    var distance = rounded_box_distance(point, half_size, input.shape.x);
    if kind == 1u || kind == 3u || kind == 4u || kind == 7u {
        distance = length(point) - min(half_size.x, half_size.y);
    }
    if kind == 7u {
        let hole = min(half_size.x, half_size.y) * input.shape.x;
        distance = max(distance, hole - length(point));
    }
    if kind == 5u {
        let direction = vec2(cos(input.shape.w), sin(input.shape.w));
        distance = line_distance(
            point,
            -direction * input.shape.z,
            direction * input.shape.z,
        ) - input.shape.x;
    }
    if kind == 6u {
        let radius = max(min(half_size.x, half_size.y) - input.shape.x * 0.5, 0.0);
        let ring_distance = abs(length(point) - radius) - input.shape.x * 0.5;
        let angle = atan2(point.y, point.x);
        let sweep = min(abs(input.shape.w), 6.28318530718);
        let relative = ((angle - input.shape.z) % 6.28318530718 + 6.28318530718) % 6.28318530718;
        let directed = select(6.28318530718 - relative, relative, input.shape.w >= 0.0);
        let angular_distance = max(directed - sweep, 0.0) * max(radius, 1.0);
        distance = max(ring_distance, angular_distance);
    }
    let antialias = max(fwidth(distance), 0.65 / frame.viewport.z);
    let coverage = 1.0 - smoothstep(-antialias, antialias, distance);
    if coverage <= 0.0 {
        discard;
    }

    let fill = frame.colors[input.style.x];
    let outline = frame.colors[input.style.y];
    let accent = frame.colors[input.style.z];
    let outline_coverage = 1.0 - smoothstep(
        input.shape.y - antialias,
        input.shape.y + antialias,
        abs(distance),
    );
    let gloss = saturate(input.material.x + frame.material.x);
    let grain = max(input.material.y, frame.material.y);
    let top_light = saturate(0.5 - point.y / max(input.size.y, 1.0));
    var color = fill;
    // Flat means the authored color, not an implicit 14% lighting penalty.
    // Preserve the established material response whenever gloss is enabled.
    if gloss > 0.0 {
        color = vec4(color.rgb * (0.86 + gloss * top_light * 0.30), color.a);
    }
    color = vec4(
        color.rgb + (hash21(floor(input.screen * frame.viewport.z)) - 0.5) * grain,
        color.a,
    );
    color = mix(color, outline, outline_coverage * outline.a);

    if kind == 2u {
        let track = fill;
        let filled = step(1.0 - saturate(input.shape.z), input.local.y);
        let hot = smoothstep(0.82, 1.0, input.shape.z);
        let meter = mix(accent, frame.colors[12u], hot);
        color = mix(track, meter, filled);
    }

    if kind == 3u {
        let value_angle = mix(-2.35, 2.35, saturate(input.shape.z)) + input.shape.w;
        let radius = min(half_size.x, half_size.y);
        let marker_start = vec2(sin(value_angle), -cos(value_angle)) * radius * 0.48;
        let marker_end = vec2(sin(value_angle), -cos(value_angle)) * radius * 0.82;
        let marker = 1.0 - smoothstep(1.0, 2.2, line_distance(point, marker_start, marker_end));
        color = mix(color, accent, marker * accent.a);
    }

    if kind == 4u {
        let lamp_active = saturate(input.shape.z);
        let radius = min(half_size.x, half_size.y);
        let glow = pow(saturate(1.0 - length(point) / max(radius, 0.001)), 2.0);
        color = mix(color, accent, lamp_active * 0.82);
        color = vec4(
            color.rgb + accent.rgb * glow * lamp_active * max(input.material.z, frame.material.z),
            color.a,
        );
    }

    if kind == 7u {
        let radius = max(min(half_size.x, half_size.y), 0.001);
        let radial = length(point) / radius;
        let angle = atan2(point.y, point.x);
        // Derivative-filtered grooves: fade fine rings before they can shimmer.
        let frequency = min(radius * frame.viewport.z * 0.22, 110.0);
        let phase = radial * frequency;
        let resolved = 1.0 - smoothstep(0.25, 0.5, fwidth(phase));
        let groove = cos(phase * 6.28318530718) * resolved;
        let reflection = pow(abs(cos(angle + 0.85)), 18.0);
        let runout = smoothstep(0.33, 0.38, radial) * (1.0 - smoothstep(0.94, 0.98, radial));
        let wax = fill.rgb * 0.40 + vec3(0.008 + groove * 0.004 * runout + reflection * 0.12 * gloss);
        let paper = accent.rgb * (0.94 + cos(angle - input.shape.w) * 0.04);
        let label = 1.0 - smoothstep(input.shape.z - antialias / radius, input.shape.z + antialias / radius, radial);
        color = vec4(mix(wax, paper, label), fill.a);
    }

    color.a *= coverage;
    return vec4(max(color.rgb, vec3(0.0)), color.a);
}
