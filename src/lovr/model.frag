uniform vec4 tint;
uniform float metalness;
uniform float roughness;
uniform bool useAuthored;
vec4 lovrmain() {
  Surface s = getDefaultSurface();
  if (!useAuthored) {
    s.baseColor = vec4(gammaToLinear(tint.rgb), tint.a);
    s.metalness = metalness;
    s.roughness = roughness;
  }
  finalizeSurface(s);
  vec3 light = getLighting(s, normalize(vec3(-.4, .8, .6)), vec4(3., 2.8, 2.6, 1.), 1.);
  light += getLighting(s, normalize(vec3(.6, .4, -.7)), vec4(1.4, 1.7, 2., 1.), 1.);
  // Neutral studio ambient keeps metal legible without loading an HDR panorama.
  light += s.baseColor.rgb * (.12 + .22 * s.metalness);
  return vec4(tonemap(light), 1.);
}
