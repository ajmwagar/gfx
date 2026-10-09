// Emissive bolt layers. The host owns bounded geometry and additive blending.
uniform vec3 laserTint;
uniform float laserCore;
uniform float laserOpacity;
vec4 lovrmain() {
  vec3 color = mix(laserTint, vec3(1.0), clamp(laserCore, 0.0, 1.0));
  return vec4(gammaToLinear(color), clamp(laserOpacity, 0.0, 1.0));
}
