// Selection follows the authored mesh, including its alpha cutouts and pose.
// No bounding geometry or second screen-space render is required.
vec4 lovrmain() {
  Surface s = getDefaultSurface();
  // Unlit imported meshes can lack usable normals. Selection must preserve
  // their textures without passing those normals into a lighting calculation.
  vec3 authored = s.baseColor.rgb;
  return vec4(mix(authored, vec3(.16, .94, .77), .58), s.baseColor.a);
}
