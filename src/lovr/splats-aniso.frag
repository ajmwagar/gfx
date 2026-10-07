in vec2 gaussianUv;
in vec4 gaussianColor;

vec4 lovrmain() {
  float opacity = gaussianColor.a * exp(-.5 * dot(gaussianUv, gaussianUv));
  if (opacity < .01) discard;
  return vec4(gaussianColor.rgb, opacity);
}
