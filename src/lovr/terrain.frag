// Baked vertex colors and coast geometry remain caller-owned. One bounded
// directional-light evaluation makes relief readable without screen textures.
vec4 lovrmain() {
  Surface s = getDefaultSurface();
  finalizeSurface(s);
  vec3 relief = getLighting(s, normalize(vec3(-.4, .8, .6)), vec4(1., .96, .9, 1.), 1.);
  return vec4(tonemap(s.baseColor.rgb * .72 + relief * .45), s.baseColor.a);
}
