struct View { extent: vec4<f32>, plot: vec4<f32>, tile: vec4<f32> };
@group(0) @binding(0) var<uniform> v: View;
@group(0) @binding(1) var pixels: texture_2d<f32>;
@group(0) @binding(2) var sampling: sampler;
struct Out { @builtin(position) position: vec4<f32>, @location(0) geo: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) index: u32) -> Out {
    let corners = array<vec2<f32>,6>(vec2(0.,0.),vec2(0.,1.),vec2(1.,0.),vec2(1.,0.),vec2(0.,1.),vec2(1.,1.));
    let corner = corners[index];
    let longitude = (v.tile.x + corner.x) / v.tile.z * 360. - 180.;
    let mercator = 3.14159265359 * (1. - 2. * (v.tile.y + corner.y) / v.tile.z);
    let latitude = atan(sinh(mercator)) * 57.2957795131;
    let point = vec2((longitude-v.extent.x)/(v.extent.y-v.extent.x), (v.extent.w-latitude)/(v.extent.w-v.extent.z));
    let screen = v.plot.xy + point * v.plot.zw;
    var out: Out;
    out.position = vec4(screen.x*2.-1., 1.-screen.y*2., 0.,1.);
    out.geo = vec2(longitude,latitude); return out;
}
@fragment fn fs(input: Out) -> @location(0) vec4<f32> {
    let latitude = input.geo.y * 0.0174532925199;
    let uv = vec2((input.geo.x+180.)/360.*v.tile.z-v.tile.x, (1.-log(tan(0.785398163397+latitude*.5))/3.14159265359)*.5*v.tile.z-v.tile.y);
    return textureSample(pixels,sampling,clamp(uv,vec2(0.),vec2(1.)));
}
