// Recolor the authored blue status strip, preserving the surrounding material.
uniform float weaponHeat;
uniform vec2 weaponStripUvBounds;
vec4 lovrmain() {
  Surface s = getDefaultSurface();
  vec3 c = s.baseColor.rgb;
  float blue = smoothstep(.08, .20, c.b - max(c.r, c.g * .8));
  float stripPosition = clamp((UV.x - weaponStripUvBounds.x) /
    max(.0001, weaponStripUvBounds.y - weaponStripUvBounds.x), 0., 1.);
  float frontier = smoothstep(1. - weaponHeat - .015, 1. - weaponHeat + .015, stripPosition);
  float heated = step(.001, weaponHeat) * frontier;
  return vec4(mix(c, vec3(1., .08, .025), blue * heated), s.baseColor.a);
}
