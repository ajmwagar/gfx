//! Shared LÖVR shader sources. The host compiles once with LÖVR's shader prelude,
//! owns passes/textures, and supplies uniforms; GFX owns shading policy.
//! Available without wGPU so an embedded host does not create a second device.

/// Stable version of this source catalog and its uniform contracts.
pub const ABI_VERSION: u32 = 1;

/// Borrowed immutable sources, compiled and cached by the host renderer.
#[derive(Debug, Clone, Copy)]
pub struct ShaderSource {
    /// LÖVR vertex source or built-in vertex identifier.
    pub vertex: &'static str,
    /// Fragment source using the LÖVR shader prelude.
    pub fragment: &'static str,
}

/// Look up a bounded named shader; unknown names are rejected by the host.
/// `model` retains tint/metalness/roughness/useAuthored uniforms. `terrain`
/// retains authored material and vertex colors without additional uniforms.
/// `splats` retains the SplatData storage buffer and instance-index contract.
pub fn shader(name: &str) -> Option<ShaderSource> {
    Some(match name {
        "model" => ShaderSource {
            vertex: "unlit",
            fragment: include_str!("lovr/model.frag"),
        },
        "terrain" => ShaderSource {
            vertex: "unlit",
            fragment: include_str!("lovr/terrain.frag"),
        },
        "splats" => ShaderSource {
            vertex: include_str!("lovr/splats-aniso.vert"),
            fragment: include_str!("lovr/splats-aniso.frag"),
        },
        "portal-clear" => ShaderSource {
            vertex: "unlit",
            fragment: include_str!("lovr/portal-clear.frag"),
        },
        "portal-cache" => ShaderSource {
            vertex: "unlit",
            fragment: include_str!("lovr/portal-cache.frag"),
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_source_catalog_preserves_host_contracts() {
        assert_eq!(ABI_VERSION, 1);
        assert!(shader("unknown").is_none());
        let model = shader("model").unwrap();
        for uniform in ["tint", "metalness", "roughness", "useAuthored"] {
            assert!(model.fragment.contains(uniform));
        }
        let terrain = shader("terrain").unwrap();
        assert_eq!(terrain.vertex, "unlit");
        assert!(terrain.fragment.contains("getDefaultSurface()"));
        assert!(!terrain.fragment.contains("uniform texture"));
        let splats = shader("splats").unwrap();
        assert!(splats.vertex.contains("SplatData"));
        assert!(splats.fragment.contains("gaussianUv"));
    }
}
