#extension GL_OES_EGL_image_external : require
precision mediump float;
uniform samplerExternalOES uFrame;
uniform int uRegionCount;
uniform vec4 uBounds[8];
uniform vec3 uColor[8];
uniform vec2 uRamp[8];
varying vec2 vTextureUv;
varying vec2 vSourceUv;
void main() {
    vec3 rgb = texture2D(uFrame, vTextureUv).rgb;
    float alpha = 1.0;
    for (int i = 0; i < 8; ++i) {
        if (i < uRegionCount) {
            vec4 r = uBounds[i];
            if (vSourceUv.x >= r.x && vSourceUv.y >= r.y &&
                vSourceUv.x < r.x + r.z && vSourceUv.y < r.y + r.w) {
                vec3 delta = abs(rgb - uColor[i]);
                float distance = max(max(delta.r, delta.g), delta.b);
                alpha = min(alpha, smoothstep(uRamp[i].x, uRamp[i].x + uRamp[i].y, distance));
            }
        }
    }
    gl_FragColor = vec4(rgb * alpha, alpha);
}
