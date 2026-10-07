struct Uniforms {
    mvp: mat4x4<f32>,
    model: mat4x4<f32>,
    color: vec4<f32>,
    params: vec4<f32>,       // time, glow, opacity, line_mode
    field_params: vec4<f32>, // painted, colormap index, unused, unused
    eye: vec4<f32>, // world-space eye position
};
@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // Already normalised to 0..=1 on the CPU, so the shader never needs the
    // field's unit or range.
    @location(2) field: f32,
};
struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) world_position: vec3<f32>,
    @location(2) field: f32,
};

@vertex fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip = uniforms.mvp * vec4<f32>(input.position, 1.0);
    output.world_position = (uniforms.model * vec4<f32>(input.position, 1.0)).xyz;
    output.world_normal = normalize((uniforms.model * vec4<f32>(input.normal, 0.0)).xyz);
    // Interpolated across the triangle, which is the whole trick: three vertex
    // samples become a smooth gradient over the face for free.
    output.field = input.field;
    return output;
}


// Perceptually ordered ramps, as six stops interpolated in sRGB.
//
// Stops rather than a polynomial fit because they are readable and checkable
// against the reference ramps. All three rise monotonically in lightness so two
// points can be ranked by eye; a rainbow ramp is deliberately absent, since its
// banding reads as structure that is not in the data.
fn ramp(t: f32, c0: vec3<f32>, c1: vec3<f32>, c2: vec3<f32>,
        c3: vec3<f32>, c4: vec3<f32>, c5: vec3<f32>) -> vec3<f32> {
    let x = clamp(t, 0.0, 1.0) * 5.0;
    let i = floor(x);
    let f = x - i;
    if (i < 1.0) { return mix(c0, c1, f); }
    if (i < 2.0) { return mix(c1, c2, f); }
    if (i < 3.0) { return mix(c2, c3, f); }
    if (i < 4.0) { return mix(c3, c4, f); }
    return mix(c4, c5, f);
}

fn colormap(t: f32, which: f32) -> vec3<f32> {
    if (which < 0.5) {
        // inferno
        return ramp(t,
            vec3<f32>(0.001, 0.000, 0.014), vec3<f32>(0.259, 0.039, 0.408),
            vec3<f32>(0.576, 0.149, 0.404), vec3<f32>(0.867, 0.318, 0.227),
            vec3<f32>(0.988, 0.647, 0.039), vec3<f32>(0.988, 1.000, 0.644));
    }
    if (which < 1.5) {
        // viridis
        return ramp(t,
            vec3<f32>(0.267, 0.005, 0.329), vec3<f32>(0.283, 0.141, 0.458),
            vec3<f32>(0.254, 0.265, 0.530), vec3<f32>(0.164, 0.471, 0.558),
            vec3<f32>(0.478, 0.821, 0.318), vec3<f32>(0.993, 0.906, 0.144));
    }
    // magma
    return ramp(t,
        vec3<f32>(0.001, 0.000, 0.014), vec3<f32>(0.232, 0.059, 0.437),
        vec3<f32>(0.550, 0.161, 0.505), vec3<f32>(0.868, 0.288, 0.409),
        vec3<f32>(0.996, 0.624, 0.427), vec3<f32>(0.987, 0.991, 0.750));
}

// Palette stops and theme colors are sRGB; the target performs linear -> sRGB.
fn linear_rgb(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)),
                  c / 12.92, c <= vec3<f32>(0.04045));
}

@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let view = normalize(uniforms.eye.xyz - input.world_position);
    let normal = normalize(input.world_normal);
    let rim = pow(1.0 - abs(dot(normal, view)), 2.2);
    let light = normalize(vec3<f32>(0.4, 0.8, 0.6));
    let diffuse = abs(dot(normal, light));
    // Subtle modulation preserves the silhouette instead of painting dark bands.
    let scan = 0.97 + 0.03 * sin(input.world_position.y * 40.0 - uniforms.params.x * 2.0);
    let energy = (0.35 + 0.45 * diffuse + 0.25 * rim * uniforms.params.y) * scan;
    var base = linear_rgb(uniforms.color.rgb) * energy;
    if (uniforms.field_params.x > 0.5) {
        // Quantitative mode is unlit: identical scalar values stay identical
        // at every orientation/time and match the palette legend.
        base = linear_rgb(colormap(input.field, uniforms.field_params.y));
    } else if (uniforms.params.w > 0.5) {
        base = linear_rgb(uniforms.color.rgb) * 1.1;
    }
    return vec4<f32>(base, select(uniforms.params.z, 1.0, uniforms.field_params.x > 0.5));
}
