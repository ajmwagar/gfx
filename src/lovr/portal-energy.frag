// Flowing energy lip with a transparent open center, tinted by the host.
uniform float portalTime;
uniform vec3 portalTint;
uniform float portalOpacity;
vec4 lovrmain() {
  vec2 p = UV * 2.0 - 1.0;
  float r = length(p);
  float a = atan(p.y, p.x);
  float t = mod(portalTime, 4096.0);
  float ripple = sin(a * 17.0 - t * 4.5) * .012 + sin(a * 29.0 + t * 3.0) * .006;
  float edge = abs(r - .68 - ripple);
  float core = exp(-edge * edge * 5000.0);
  float halo = exp(-edge * edge * 260.0);
  float flow = .55 + .45 * pow(.5 + .5 * sin(a * 9.0 - t * 7.0), 3.0);
  float inner = smoothstep(.48, .64, r) * (1.0 - smoothstep(.64, .68, r));
  float energy = core * 1.3 + halo * flow * .65 + inner * flow * .18;
  vec3 color = mix(portalTint, vec3(.85, .96, 1.0), core * .65);
  return vec4(gammaToLinear(color) * energy, clamp(energy * portalOpacity, 0.0, 1.0));
}
