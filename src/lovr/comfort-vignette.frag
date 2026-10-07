// Angular, stereo-compatible comfort mask. One overlay draw, no scene texture.
uniform vec3 comfortCenter;
uniform vec3 comfortForward;
uniform float comfortProgress;
vec4 lovrmain() {
  float phase = clamp(comfortProgress, 0.0, 1.0);
  vec3 direction = normalize(PositionWorld - comfortCenter);
  float forward = dot(direction, normalize(comfortForward));
  float radius = sqrt(max(0.0, 1.0 - forward * forward)) / max(0.05, forward);
  float inner = mix(1.1, 0.22, phase);
  float outer = inner + 0.42;
  float edge = smoothstep(inner, outer, radius) * phase;
  // The center becomes completely opaque before the host changes world pose.
  float blackout = smoothstep(0.45, 1.0, phase);
  float opacity = blackout + (1.0 - blackout) * edge;
  return vec4(0.0, 0.0, 0.0, opacity);
}
