attribute vec2 aPosition;
attribute vec2 aUv;
uniform mat4 uTextureMatrix;
varying vec2 vTextureUv;
varying vec2 vSourceUv;
void main() {
    gl_Position = vec4(aPosition, 0.0, 1.0);
    vTextureUv = (uTextureMatrix * vec4(aUv, 0.0, 1.0)).xy;
    vSourceUv = vec2(aUv.x, 1.0 - aUv.y);
}
