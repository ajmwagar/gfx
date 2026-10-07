// Project a 3D Gaussian covariance into a screen-space ellipse for each eye.
struct Splat {
  vec4 center;
  vec4 covA;  // xx, xy, xz, yy
  vec4 covB;  // yz, zz, reserved, reserved
  vec4 color;
};
readonly buffer SplatData { Splat splats[]; };

out vec2 gaussianUv;
out vec4 gaussianColor;

vec4 lovrmain() {
  Splat splat = splats[InstanceIndex];
  vec4 view = ViewFromLocal * vec4(splat.center.xyz, 1.0);
  if (-view.z < .08) {
    gaussianUv = vec2(100.);
    gaussianColor = vec4(0.);
    return vec4(2., 2., 2., 1.);
  }
  float depth = max(-view.z, .02);
  mat3 covariance = mat3(
    vec3(splat.covA.x, splat.covA.y, splat.covA.z),
    vec3(splat.covA.y, splat.covA.w, splat.covB.x),
    vec3(splat.covA.z, splat.covB.x, splat.covB.y)
  );
  mat3 worldToView = mat3(ViewFromLocal);
  mat3 viewCovariance = worldToView * covariance * transpose(worldToView);
  float fx = abs(Projection[0][0]) * Resolution.x * .5;
  float fy = abs(Projection[1][1]) * Resolution.y * .5;
  vec3 dx = vec3(fx / depth, 0., fx * view.x / (depth * depth));
  vec3 dy = vec3(0., fy / depth, fy * view.y / (depth * depth));
  float a = dot(dx, viewCovariance * dx) + .25;
  float b = dot(dx, viewCovariance * dy);
  float c = dot(dy, viewCovariance * dy) + .25;
  float l00 = sqrt(max(a, .01));
  float l10 = b / l00;
  float l11 = sqrt(max(c - l10 * l10, .01));
  vec2 corner = VertexPosition.xy;
  // Use the ellipse's axis-aligned bounds so skew does not enlarge its quad.
  // Keep the Gaussian coordinates tied to the clipped pixel offsets: capping
  // a large footprint should crop it, not squeeze the whole splat into 256px.
  vec2 halfSize = min(2.5 * vec2(sqrt(a), sqrt(c)), vec2(256.));
  vec2 offsetPixels = corner * halfSize;
  vec4 clip = Projection * view;
  clip.xy += vec2(offsetPixels.x / Resolution.x,
    offsetPixels.y / Resolution.y) * (2. * clip.w);
  gaussianUv = vec2(offsetPixels.x / l00,
    (offsetPixels.y - l10 * offsetPixels.x / l00) / l11);
  gaussianColor = splat.color;
  return clip;
}
