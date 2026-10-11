in vec2 gaussianUv;
in vec4 gaussianColor;

vec4 lovrmain() {
  // Pass alpha fades nearby live detail into the owner's baked distant scenery.
  float opacity = gaussianColor.a * Color.a * exp(-.5 * dot(gaussianUv, gaussianUv));
  if (opacity < .01) discard;
  return vec4(gaussianColor.rgb, opacity);
}
