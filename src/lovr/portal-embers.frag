// Angular motion and sparks stay inside the portal quad; host controls blending.
uniform float portalTime;
uniform vec3 portalTint;
uniform float portalOpacity;
vec4 lovrmain() {
  vec2 p = UV * 2.0 - 1.0;
  float r = length(p);
  float a = atan(p.y, p.x);
  float t = mod(portalTime, 4096.0);
  float turbulence = sin(a * 31.0 - t * 8.0) * .008 + sin(a * 57.0 + t * 13.0) * .006;
  float edge = abs(r - .68 - turbulence);
  float hot = exp(-edge * edge * 6500.0);
  float fire = exp(-edge * edge * 700.0) * (.6 + .4 * sin(a * 23.0 - t * 17.0));
  float sparks = 0.0;
  for (int i = 0; i < 16; i++) {
    float k = float(i);
    float age = fract(t * (.7 + fract(k * .37) * .6) + k * .618);
    float angle = k * 2.399963 + t * 1.6 + age * .35;
    vec2 center = vec2(cos(angle), sin(angle)) * (.68 + age * .24);
    vec2 tangent = vec2(-sin(angle), cos(angle));
    vec2 d = p - center;
    float along = dot(d, tangent);
    float across = dot(d, vec2(cos(angle), sin(angle)));
    sparks += exp(-across * across * 18000.0 - along * along * 700.0) * (1.0 - age);
  }
  float energy = hot + fire * .65 + sparks * 2.5;
  vec3 color = mix(portalTint, vec3(1.0, .94, .7), clamp(hot + sparks * .5, 0.0, 1.0));
  return vec4(gammaToLinear(color) * energy, clamp(energy * portalOpacity, 0.0, 1.0));
}
