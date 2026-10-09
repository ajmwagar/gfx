// UV carries room-local floor X/Z. Analytic ceiling-strip reflections avoid
// another scene render and stay bounded on mobile GPUs.
uniform vec3 floorEye;
uniform vec4 floorBounds;
uniform float floorTile;
vec4 lovrmain() {
  vec2 p = UV;
  vec2 tile = floor((p - floorBounds.xz) / floorTile);
  float parity = mod(tile.x + tile.y, 2.);
  vec3 base = mix(vec3(.62, .65, .68), vec3(.018, .023, .03), parity);
  vec2 grid = abs(fract((p - floorBounds.xz) / floorTile) - .5);
  float seam = smoothstep(.491, .499, max(grid.x, grid.y));
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
