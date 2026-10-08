attribute vec2 aPosition;
attribute vec2 aUv;
uniform mat4 uTextureMatrix;
uniform mediump float uPackedAlpha;
varying mediump vec2 vTextureUv;
varying mediump vec2 vAlphaUv;
varying mediump vec2 vSourceUv;
void main() {
    gl_Position = vec4(aPosition, 0.0, 1.0);
    vec2 colorUv = vec2(aUv.x * (1.0 - 0.5 * uPackedAlpha), aUv.y);
    vTextureUv = (uTextureMatrix * vec4(colorUv, 0.0, 1.0)).xy;
    vAlphaUv = (uTextureMatrix * vec4(colorUv + vec2(0.5, 0.0), 0.0, 1.0)).xy;
    vSourceUv = vec2(aUv.x, 1.0 - aUv.y);
}
