uniform texture2DArray cachedImage;
uniform mat4 cachedClip0;
uniform mat4 cachedClip1;
vec4 lovrmain() {
  mat4 capture = ViewIndex == 0 ? cachedClip0 : cachedClip1;
  vec4 p = capture * vec4(PositionWorld, 1.0);
  if (p.w <= 0.0) discard;
  vec2 uv = p.xy / p.w * .5 + .5;
  if (any(lessThan(uv, vec2(0.0))) || any(greaterThan(uv, vec2(1.0)))) discard;
  return getPixel(cachedImage, uv, float(ViewIndex));
}
