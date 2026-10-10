// Selection follows the authored mesh, including its alpha cutouts and pose.
// No bounding geometry or second screen-space render is required.
vec4 lovrmain() {
  Surface s = getDefaultSurface();
  finalizeSurface(s);
  vec3 light = getLighting(s, normalize(vec3(-.4, .8, .6)), vec4(1.), 1.);
  vec3 authored = tonemap(s.baseColor.rgb * .65 + light * .35);
  return vec4(mix(authored, vec3(.16, .94, .77), .58), s.baseColor.a);
}
