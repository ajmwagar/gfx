// UV carries room-local floor X/Z. Analytic ceiling-strip reflections avoid
// another scene render and stay bounded on mobile GPUs.
uniform vec3 floorEye;
uniform vec4 floorBounds;
uniform float floorTile;
float checkerIntegral(float x) {
  return 1. - abs(mod(x, 2.) - 1.);
}
float checkerFilter(float x, float width) {
  width = max(width, .0001);
  return (checkerIntegral(x + width * .5) - checkerIntegral(x - width * .5)) / width;
}
vec4 lovrmain() {
  vec2 p = UV;
  vec2 tile = (p - floorBounds.xz) / floorTile;
  vec2 footprint = fwidth(tile);
  float parity = .5 - .5 * checkerFilter(tile.x, footprint.x) * checkerFilter(tile.y, footprint.y);
  vec3 base = mix(vec3(.62, .65, .68), vec3(.018, .023, .03), parity);
  vec2 grid = abs(fract((p - floorBounds.xz) / floorTile) - .5);
  float aa = max(footprint.x, footprint.y);
  float seam = smoothstep(.493 - aa * .5, .493 + max(.001, aa * .5), max(grid.x, grid.y));
  seam *= 1. - smoothstep(.1, .5, aa);
  base *= 1. - .35 * seam;
  float eyeHeight = max(.25, floorEye.y);
  vec2 reflected = p + (p - floorEye.xz) * (5. / eyeHeight);
  float stripX = min(abs(reflected.x - (floorBounds.x + 5.)), abs(reflected.x - (floorBounds.y - 5.)));
  float stripZ = abs(mod(reflected.y - floorBounds.z - 3. + 3., 6.) - 3.);
  float blur = .11 + .012 * length(p - floorEye.xz);
  float reflection = (1. - smoothstep(.07, .07 + blur, stripX)) * (1. - smoothstep(1.8, 2.1, stripZ));
  reflection *= step(floorBounds.z, reflected.y) * step(reflected.y, floorBounds.w);
  float grazing = 1. - eyeHeight / length(vec3(p.x - floorEye.x, eyeHeight, p.y - floorEye.z));
  vec3 color = base * (.65 + .15 * grazing) + vec3(.65, .78, .94) * reflection * (.35 + .55 * grazing);
  return vec4(color, 1.);
}
