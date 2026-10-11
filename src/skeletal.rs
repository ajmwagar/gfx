//! Bounded native glTF skeletal sampling and CPU skinning into the shared mesh ABI.
//!
//! Hosts own clocks, GPU submission and materials. Samples clamp per channel;
//! no loop, automatic root-motion extraction, physics or IK is inferred.
use crate::mesh::Vertex;
use glam::{Mat4, Quat, Vec3};
use std::collections::BTreeSet;

/// Rejected input or pose; unsupported features never disappear silently.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Invalid glTF JSON/container metadata.
    #[error("glTF: {0}")]
    Gltf(#[from] gltf::Error),
    /// Unsupported, excessive or inconsistent source/pose data.
    #[error("skeletal: {0}")]
    Invalid(String),
}
fn require(value: bool, message: &str) -> Result<(), Error> {
    if value {
        Ok(())
    } else {
        Err(Error::Invalid(message.into()))
    }
}
#[allow(
    clippy::float_cmp,
    reason = "Affine homogeneous sentinel entries must be exact."
)]
fn affine(m: Mat4) -> bool {
    let a = m.to_cols_array();
    m.is_finite()
        && m.inverse().is_finite()
        && a[3] == 0.
        && a[7] == 0.
        && a[11] == 0.
        && a[15] == 1.
}
#[derive(Clone, Copy)]
struct Trs {
    t: Vec3,
    r: Quat,
    s: Vec3,
}
impl Trs {
    fn matrix(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.s, self.r, self.t)
    }
}
struct Node {
    name: String,
    parent: Option<usize>,
    rest: Mat4,
    trs: Option<Trs>,
}
struct Skin {
    joints: Vec<usize>,
    inverse: Vec<Mat4>,
}
struct Primitive {
    node: usize,
    skin: Option<usize>,
    positions: Vec<[f32; 3]>,
    normals: Option<Vec<[f32; 3]>>,
    texcoords: Option<Vec<[f32; 2]>>,
    source_tangents: Option<Vec<[f32; 4]>>,
    joints: Option<Vec<[u16; 4]>>,
    weights: Option<Vec<[f32; 4]>>,
    indices: Vec<u32>,
    color: [f32; 4],
}
enum Values {
    Translation(Vec<Vec3>),
    Rotation(Vec<Quat>),
    Scale(Vec<Vec3>),
}
struct Channel {
    node: usize,
    times: Vec<f32>,
    linear: bool,
    values: Values,
}
struct Clip {
    info: ClipInfo,
    channels: Vec<Channel>,
}

/// Original clip range in seconds; nonzero starts are retained.
#[derive(Debug, Clone)]
pub struct ClipInfo {
    /// Source name, or a stable generated name.
    pub name: String,
    /// Minimum key time among channels.
    pub start: f32,
    /// Maximum key time among channels.
    pub end: f32,
}
/// One world-space triangle primitive ready for the shared mesh renderer.
pub struct MeshFrame {
    /// Source mesh-instance node index.
    pub node: usize,
    /// World-space positions and unit normals; field is zero.
    pub vertices: Vec<Vertex>,
    /// Source triangle indices, unchanged.
    pub indices: Vec<u32>,
    /// Source UV set zero, if present. Texture sampling remains host-owned.
    pub texcoords: Option<Vec<[f32; 2]>>,
    /// Original mesh-local glTF tangent xyz and handedness w, unchanged.
    /// Not transformed or skinned; the current shared mesh shader does not use them.
    pub source_tangents: Option<Vec<[f32; 4]>>,
    /// Linear glTF base color factor; a host must convert for the sRGB mesh uniform.
    pub color: [f32; 4],
}
/// Deterministic snapshot; sampling is independent of call order.
pub struct Frame {
    /// Requested source time in seconds.
    pub seconds: f32,
    /// Selected clip, or None for the rest pose.
    pub clip: Option<usize>,
    /// All selected-scene node world matrices, in glTF node order.
    pub world: Vec<[[f32; 4]; 4]>,
    /// CPU-skinned/static world-space mesh primitives.
    pub meshes: Vec<MeshFrame>,
}
/// Immutable selected-scene geometry, hierarchy, skins and animation channels.
///
/// Supports embedded GLB triangle geometry with up to four influences, STEP
/// and LINEAR TRS channels. Does not load URIs, run scripts or reconstruct PBR.
pub struct Asset {
    nodes: Vec<Node>,
    order: Vec<usize>,
    skins: Vec<Skin>,
    primitives: Vec<Primitive>,
    clips: Vec<Clip>,
}
impl Asset {
    /// Load and validate a self-contained GLB with an explicit default scene.
    ///
    /// # Errors
    /// Rejects malformed references, binary ranges, cycles, unknown required
    /// extensions, morphs, sparse data, extra influence sets and cubic curves.
    pub fn from_glb(bytes: &[u8]) -> Result<Self, Error> {
        require(
            bytes.len() <= 128 * 1024 * 1024 && bytes.starts_with(b"glTF"),
            "expected GLB up to 128 MiB",
        )?;
        let container = gltf::Glb::from_slice(bytes).map_err(|e| Error::Invalid(e.to_string()))?;
        require(
            container.json.len() <= 16 * 1024 * 1024,
            "GLB JSON exceeds 16 MiB",
        )?;
        let source = gltf::Gltf::from_slice(bytes)?;
        let bin = source
            .blob
            .as_deref()
            .ok_or_else(|| Error::Invalid("missing binary buffer".into()))?;
        validate_binary(&source, bin)?;
        let (nodes, order) = load_nodes(&source)?;
        let skins = load_skins(&source, &nodes, bin)?;
        let primitives = load_primitives(&source, &skins, bin)?;
        let clips = load_clips(&source, &nodes, bin)?;
        let asset = Self {
            nodes,
            order,
            skins,
            primitives,
            clips,
        };
        asset.sample(None, 0.)?;
        Ok(asset)
    }
    /// Inspect source clip names and time ranges without starting playback.
    pub fn clips(&self) -> impl ExactSizeIterator<Item = &ClipInfo> {
        self.clips.iter().map(|c| &c.info)
    }
    /// Node names in source index order, including non-joint hierarchy parents.
    pub fn node_names(&self) -> impl ExactSizeIterator<Item = &str> {
        self.nodes.iter().map(|n| n.name.as_str())
    }
    /// Sample a clip or rest pose; all meshes are returned in world coordinates.
    ///
    /// # Errors
    /// Rejects invalid clip/time, singular normal transforms, degenerate faces
    /// and numeric overflow in the sampled hierarchy or deformed positions.
    pub fn sample(&self, clip: Option<usize>, seconds: f32) -> Result<Frame, Error> {
        require(
            seconds.is_finite() && seconds.abs() <= 1e6,
            "invalid sample time",
        )?;
        let world = self.sample_world(clip, seconds)?;
        let meshes = self.skin_meshes(&world)?;
        Ok(Frame {
            seconds,
            clip,
            world: world.iter().map(Mat4::to_cols_array_2d).collect(),
            meshes,
        })
    }
    fn sample_world(&self, clip: Option<usize>, seconds: f32) -> Result<Vec<Mat4>, Error> {
        let mut local = self.nodes.iter().map(|n| n.rest).collect::<Vec<_>>();
        if let Some(index) = clip {
            let clip = self
                .clips
                .get(index)
                .ok_or_else(|| Error::Invalid("clip index out of bounds".into()))?;
            let mut trs = self.nodes.iter().map(|n| n.trs).collect::<Vec<_>>();
            for channel in &clip.channels {
                let i = channel
                    .times
                    .partition_point(|&t| t <= seconds)
                    .saturating_sub(1);
                let j = (i + 1).min(channel.times.len() - 1);
                let f = if channel.linear && i != j {
                    ((seconds - channel.times[i]) / (channel.times[j] - channel.times[i]))
                        .clamp(0., 1.)
                } else {
                    0.
                };
                let target = trs[channel.node].as_mut().unwrap();
                match &channel.values {
                    Values::Translation(v) => target.t = v[i].lerp(v[j], f),
                    Values::Scale(v) => target.s = v[i].lerp(v[j], f),
                    Values::Rotation(v) => target.r = v[i].slerp(v[j], f).normalize(),
                }
            }
            for channel in &clip.channels {
                local[channel.node] = trs[channel.node].unwrap().matrix();
            }
        }
        let mut world = vec![Mat4::IDENTITY; self.nodes.len()];
        for &i in &self.order {
            world[i] = self.nodes[i]
                .parent
                .map_or(local[i], |p| world[p] * local[i]);
            require(
                affine(world[i]),
                "sampled hierarchy overflows or is singular",
            )?;
        }
        Ok(world)
    }
    fn skin_meshes(&self, world: &[Mat4]) -> Result<Vec<MeshFrame>, Error> {
        let palettes = self
            .skins
            .iter()
            .map(|s| {
                s.joints
                    .iter()
                    .zip(&s.inverse)
                    .map(|(&j, &m)| world[j] * m)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let mut meshes = Vec::new();
        for p in &self.primitives {
            let mut vertices = Vec::new();
            for (i, &position) in p.positions.iter().enumerate() {
                let transform = if let Some(skin) = p.skin {
                    let js = p.joints.as_ref().unwrap()[i];
                    let mut ws = p.weights.as_ref().unwrap()[i];
                    let sum = ws.iter().sum::<f32>();
                    for w in &mut ws {
                        *w /= sum;
                    }
                    let mut blend = Mat4::ZERO;
                    for (j, w) in js.into_iter().zip(ws) {
                        if w > 0. {
                            blend += palettes[skin][j as usize] * w;
                        }
                    }
                    blend
                } else {
                    world[p.node]
                };
                let position = transform.transform_point3(Vec3::from_array(position));
                require(
                    transform.is_finite() && position.is_finite(),
                    "skinned position overflow",
                )?;
                let normal = if let Some(normals) = &p.normals {
                    let normal = transform
                        .inverse()
                        .transpose()
                        .transform_vector3(Vec3::from_array(normals[i]));
                    require(
                        normal.is_finite()
                            && normal.length_squared().is_finite()
                            && normal.length_squared() > 1e-20,
                        "singular/invalid normal transform",
                    )?;
                    normal.normalize().to_array()
                } else {
                    [0.; 3]
                };
                vertices.push(Vertex {
                    position: position.to_array(),
                    normal,
                    field: 0.,
                });
            }
            if p.normals.is_none() {
                generated_normals(&mut vertices, &p.indices)?;
            }
            meshes.push(MeshFrame {
                node: p.node,
                vertices,
                indices: p.indices.clone(),
                texcoords: p.texcoords.clone(),
                source_tangents: p.source_tangents.clone(),
                color: p.color,
            });
        }
        Ok(meshes)
    }
}
fn validate_binary(source: &gltf::Gltf, bin: &[u8]) -> Result<(), Error> {
    require(
        source.buffers().count() == 1
            && source.buffers().all(|b| {
                matches!(b.source(), gltf::buffer::Source::Bin) && b.length() <= bin.len()
            }),
        "only one embedded binary buffer supported",
    )?;
    require(
        source
            .extensions_required()
            .all(|e| e == "KHR_materials_unlit"),
        "unknown required extension",
    )?;
    for view in source.views() {
        require(
            view.offset()
                .checked_add(view.length())
                .is_some_and(|n| n <= bin.len()),
            "buffer view exceeds binary payload",
        )?;
    }
    for accessor in source.accessors() {
        require(
            accessor.sparse().is_none() && accessor.count() <= 1_000_000 && accessor.count() > 0,
            "sparse/empty/excessive accessor",
        )?;
        let view = accessor
            .view()
            .ok_or_else(|| Error::Invalid("accessor has no buffer view".into()))?;
        let stride = view.stride().unwrap_or(accessor.size());
        require(
            stride >= accessor.size(),
            "accessor stride smaller than element",
        )?;
        let end = (accessor.count() - 1)
            .checked_mul(stride)
            .and_then(|n| n.checked_add(accessor.offset()))
            .and_then(|n| n.checked_add(accessor.size()));
        require(
            end.is_some_and(|n| n <= view.length()),
            "accessor exceeds buffer view",
        )?;
    }
    require(
        source.nodes().count() <= 4096
            && source.skins().count() <= 64
            && source.animations().count() <= 64,
        "node/skin/clip budget exceeded",
    )?;
    Ok(())
}

fn load_nodes(source: &gltf::Gltf) -> Result<(Vec<Node>, Vec<usize>), Error> {
    fn visit(
        node: &gltf::Node<'_>,
        depth: usize,
        selected: &mut BTreeSet<usize>,
        order: &mut Vec<usize>,
    ) -> Result<(), Error> {
        require(
            depth <= 64 && selected.insert(node.index()),
            "cyclic/shared/deep scene hierarchy",
        )?;
        order.push(node.index());
        for child in node.children() {
            visit(&child, depth + 1, selected, order)?;
        }
        Ok(())
    }
    let mut nodes = Vec::new();
    for node in source.nodes() {
        let rest = Mat4::from_cols_array_2d(&node.transform().matrix());
        require(affine(rest), "invalid rest matrix")?;
        let trs = match node.transform() {
            gltf::scene::Transform::Decomposed {
                translation,
                rotation,
                scale,
            } => {
                let r = checked_quat(rotation)?;
                Some(Trs {
                    t: Vec3::from_array(translation),
                    r,
                    s: Vec3::from_array(scale),
                })
            }
            gltf::scene::Transform::Matrix { .. } => None,
        };
        nodes.push(Node {
            name: node
                .name()
                .map_or_else(|| format!("node-{}", node.index()), str::to_owned),
            parent: None,
            rest,
            trs,
        });
    }
    for node in source.nodes() {
        for child in node.children() {
            require(
                nodes[child.index()].parent.is_none(),
                "multiple node parents",
            )?;
            nodes[child.index()].parent = Some(node.index());
        }
    }
    let scene = source
        .default_scene()
        .ok_or_else(|| Error::Invalid("explicit default scene required".into()))?;
    let mut order = Vec::new();
    let mut selected = BTreeSet::new();
    for root in scene.nodes() {
        require(
            nodes[root.index()].parent.is_none(),
            "scene root has a parent",
        )?;
        visit(&root, 0, &mut selected, &mut order)?;
    }
    require(
        selected.len() == nodes.len(),
        "profile requires all nodes in selected scene",
    )?;
    Ok((nodes, order))
}

fn load_skins(source: &gltf::Gltf, nodes: &[Node], bin: &[u8]) -> Result<Vec<Skin>, Error> {
    let mut skins = Vec::new();
    for skin in source.skins() {
        let joints = skin.joints().map(|n| n.index()).collect::<Vec<_>>();
        require(
            !joints.is_empty()
                && joints.len() <= 256
                && joints.iter().collect::<BTreeSet<_>>().len() == joints.len(),
            "invalid skin joint count/duplicates",
        )?;
        let inverse = skin
            .reader(|_| Some(bin))
            .read_inverse_bind_matrices()
            .map_or_else(
                || vec![Mat4::IDENTITY; joints.len()],
                |v| v.map(|m| Mat4::from_cols_array_2d(&m)).collect(),
            );
        require(
            inverse.len() == joints.len() && inverse.iter().all(|&m| affine(m)),
            "invalid inverse bind matrices",
        )?;
        // All joints must share a scene root; optional skeleton must contain them.
        let ancestor = |mut node: usize, target: usize| loop {
            if node == target {
                return true;
            }
            match nodes[node].parent {
                Some(p) => node = p,
                None => return false,
            }
        };
        let mut common = joints[0];
        while !joints.iter().all(|&j| ancestor(j, common)) {
            common = nodes[common]
                .parent
                .ok_or_else(|| Error::Invalid("skin joints have no common root".into()))?;
        }
        if let Some(root) = skin.skeleton() {
            require(
                joints.iter().all(|&j| ancestor(j, root.index())),
                "skeleton is not joint ancestor",
            )?;
        }
        skins.push(Skin { joints, inverse });
    }
    Ok(skins)
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep all per-primitive bounds and attribute compatibility checks in one ordered validation pass."
)]
#[allow(
    clippy::float_cmp,
    reason = "glTF tangent handedness is the discrete sentinel -1 or +1."
)]
fn load_primitives(
    source: &gltf::Gltf,
    skins: &[Skin],
    bin: &[u8],
) -> Result<Vec<Primitive>, Error> {
    let mut primitives = Vec::new();
    let mut vertex_count = 0;
    let mut index_count = 0;
    for node in source.nodes() {
        if let Some(mesh) = node.mesh() {
            require(mesh.weights().is_none(), "morph mesh weights unsupported")?;
            for p in mesh.primitives() {
                require(
                    p.mode() == gltf::mesh::Mode::Triangles && p.morph_targets().next().is_none(),
                    "only unmorphed triangles supported",
                )?;
                for (semantic, _) in p.attributes() {
                    require(
                        matches!(
                            semantic,
                            gltf::Semantic::Positions
                                | gltf::Semantic::Normals
                                | gltf::Semantic::Tangents
                                | gltf::Semantic::TexCoords(0)
                                | gltf::Semantic::Joints(0)
                                | gltf::Semantic::Weights(0)
                        ),
                        "unsupported vertex attribute/set",
                    )?;
                }
                let reader = p.reader(|_| Some(bin));
                let positions = reader
                    .read_positions()
                    .ok_or_else(|| Error::Invalid("missing positions".into()))?
                    .collect::<Vec<_>>();
                vertex_count += positions.len();
                require(
                    !positions.is_empty()
                        && vertex_count <= 300_000
                        && positions
                            .iter()
                            .flatten()
                            .all(|v| v.is_finite() && v.abs() <= 1e12),
                    "position budget/numeric failure",
                )?;
                let normals = reader.read_normals().map(Iterator::collect::<Vec<_>>);
                if let Some(normals) = &normals {
                    require(
                        normals.len() == positions.len()
                            && normals.iter().all(|&n| {
                                Vec3::from_array(n).is_finite()
                                    && Vec3::from_array(n).length_squared() > 1e-20
                            }),
                        "invalid normals",
                    )?;
                }
                let texcoords = reader
                    .read_tex_coords(0)
                    .map(|v| v.into_f32().collect::<Vec<_>>());
                if let Some(uv) = &texcoords {
                    require(
                        uv.len() == positions.len() && uv.iter().flatten().all(|v| v.is_finite()),
                        "invalid UVs",
                    )?;
                }
                let source_tangents = if let Some(accessor) = p.get(&gltf::Semantic::Tangents) {
                    require(
                        accessor.data_type() == gltf::accessor::DataType::F32
                            && accessor.dimensions() == gltf::accessor::Dimensions::Vec4
                            && !accessor.normalized()
                            && accessor.count() == positions.len(),
                        "invalid tangent accessor type/count",
                    )?;
                    let tangents = reader
                        .read_tangents()
                        .ok_or_else(|| Error::Invalid("missing tangent data".into()))?
                        .collect::<Vec<_>>();
                    require(
                        tangents.len() == positions.len()
                            && tangents.iter().all(|t| {
                                let direction = Vec3::new(t[0], t[1], t[2]);
                                direction.is_finite()
                                    && (direction.length_squared() - 1.).abs() <= 1e-3
                                    && (t[3] == -1. || t[3] == 1.)
                            }),
                        "invalid tangent direction/handedness",
                    )?;
                    Some(tangents)
                } else {
                    None
                };
                let indices = reader.read_indices().map_or_else(
                    || (0..u32::try_from(positions.len()).expect("bounded vertices")).collect(),
                    |v| v.into_u32().collect::<Vec<_>>(),
                );
                index_count += indices.len();
                require(
                    !indices.is_empty()
                        && indices.len() % 3 == 0
                        && index_count <= 900_000
                        && indices.iter().all(|&i| (i as usize) < positions.len()),
                    "invalid triangle indices/budget",
                )?;
                let joints = reader
                    .read_joints(0)
                    .map(|v| v.into_u16().collect::<Vec<_>>());
                let weights = reader
                    .read_weights(0)
                    .map(|v| v.into_f32().collect::<Vec<_>>());
                let skin = node.skin().map(|s| s.index());
                match (skin, &joints, &weights) {
                    (Some(s), Some(js), Some(ws)) => {
                        require(
                            js.len() == positions.len() && ws.len() == positions.len(),
                            "skin attribute count mismatch",
                        )?;
                        for (j, w) in js.iter().zip(ws) {
                            let mut seen = BTreeSet::new();
                            require(
                                j.iter().all(|&v| (v as usize) < skins[s].joints.len())
                                    && w.iter().all(|v| v.is_finite() && *v >= 0. && *v <= 1.)
                                    && (w.iter().sum::<f32>() - 1.).abs() <= 1e-5,
                                "invalid joint/weight values or sum",
                            )?;
                            require(
                                j.iter()
                                    .zip(w)
                                    .filter(|(_, w)| **w > 0.)
                                    .all(|(&j, _)| seen.insert(j)),
                                "duplicate weighted joint",
                            )?;
                        }
                    }
                    (None, None, None) => {}
                    _ => {
                        return Err(Error::Invalid(
                            "skin/JOINTS_0/WEIGHTS_0 must appear together".into(),
                        ))
                    }
                }
                let color = p.material().pbr_metallic_roughness().base_color_factor();
                require(
                    color
                        .iter()
                        .all(|c| c.is_finite() && (0. ..=1.).contains(c)),
                    "invalid material color",
                )?;
                primitives.push(Primitive {
                    node: node.index(),
                    skin,
                    positions,
                    normals,
                    texcoords,
                    source_tangents,
                    joints,
                    weights,
                    indices,
                    color,
                });
            }
        }
    }
    require(
        !primitives.is_empty() && primitives.len() <= 4096,
        "empty/excessive primitive set",
    )?;
    Ok(primitives)
}

fn load_clips(source: &gltf::Gltf, nodes: &[Node], bin: &[u8]) -> Result<Vec<Clip>, Error> {
    let mut clips = Vec::new();
    let mut key_count = 0;
    for animation in source.animations() {
        let mut channels = Vec::new();
        let mut targets = BTreeSet::new();
        let mut start = f32::INFINITY;
        let mut end = 0f32;
        for channel in animation.channels() {
            require(channels.len() < 768, "channel budget exceeded")?;
            let node = channel.target().node().index();
            let property = channel.target().property();
            require(
                nodes[node].trs.is_some() && targets.insert((node, format!("{property:?}"))),
                "matrix node/duplicate animation target",
            )?;
            let linear = match channel.sampler().interpolation() {
                gltf::animation::Interpolation::Step => false,
                gltf::animation::Interpolation::Linear => true,
                gltf::animation::Interpolation::CubicSpline => {
                    return Err(Error::Invalid(
                        "CUBICSPLINE requires explicit baking".into(),
                    ))
                }
            };
            let reader = channel.reader(|_| Some(bin));
            let times = reader
                .read_inputs()
                .ok_or_else(|| Error::Invalid("missing key times".into()))?
                .collect::<Vec<_>>();
            key_count += times.len();
            require(
                !times.is_empty()
                    && times.len() <= 16_384
                    && key_count <= 98_304
                    && times
                        .iter()
                        .all(|t| t.is_finite() && (0. ..=86_400.).contains(t))
                    && times.windows(2).all(|w| w[0] < w[1]),
                "invalid/excessive key times",
            )?;
            start = start.min(times[0]);
            end = end.max(*times.last().unwrap());
            let output = reader
                .read_outputs()
                .ok_or_else(|| Error::Invalid("missing key values".into()))?;
            let values = match output {
                gltf::animation::util::ReadOutputs::Translations(v) => {
                    Values::Translation(checked_vectors(v, times.len())?)
                }
                gltf::animation::util::ReadOutputs::Scales(v) => {
                    Values::Scale(checked_vectors(v, times.len())?)
                }
                gltf::animation::util::ReadOutputs::Rotations(v) => {
                    let values = v
                        .into_f32()
                        .map(checked_quat)
                        .collect::<Result<Vec<_>, _>>()?;
                    require(values.len() == times.len(), "rotation key count mismatch")?;
                    Values::Rotation(values)
                }
                gltf::animation::util::ReadOutputs::MorphTargetWeights(_) => {
                    return Err(Error::Invalid("morph animation unsupported".into()))
                }
            };
            channels.push(Channel {
                node,
                times,
                linear,
                values,
            });
        }
        require(!channels.is_empty(), "empty animation clip")?;
        clips.push(Clip {
            info: ClipInfo {
                name: animation
                    .name()
                    .map_or_else(|| format!("clip-{}", animation.index()), str::to_owned),
                start,
                end,
            },
            channels,
        });
    }
    Ok(clips)
}

fn checked_quat(v: [f32; 4]) -> Result<Quat, Error> {
    let q = Quat::from_array(v);
    require(
        q.is_finite() && (q.length_squared() - 1.).abs() <= 1e-3,
        "invalid unit quaternion",
    )?;
    Ok(q.normalize())
}
fn checked_vectors(
    values: impl Iterator<Item = [f32; 3]>,
    count: usize,
) -> Result<Vec<Vec3>, Error> {
    let values = values.map(Vec3::from_array).collect::<Vec<_>>();
    require(
        values.len() == count
            && values
                .iter()
                .all(|v| v.is_finite() && v.abs().max_element() <= 1e12),
        "invalid vector key count/value",
    )?;
    Ok(values)
}
fn generated_normals(vertices: &mut [Vertex], indices: &[u32]) -> Result<(), Error> {
    let mut normals = vec![Vec3::ZERO; vertices.len()];
    for triangle in indices.as_chunks::<3>().0 {
        let a = Vec3::from_array(vertices[triangle[0] as usize].position);
        let b = Vec3::from_array(vertices[triangle[1] as usize].position);
        let c = Vec3::from_array(vertices[triangle[2] as usize].position);
        let n = (b - a).cross(c - a);
        require(
            n.is_finite() && n.length_squared().is_finite() && n.length_squared() > 1e-30,
            "degenerate deformed triangle",
        )?;
        for &i in triangle {
            normals[i as usize] += n;
        }
    }
    for (v, n) in vertices.iter_mut().zip(normals) {
        require(
            n.is_finite() && n.length_squared().is_finite() && n.length_squared() > 1e-30,
            "missing/cancelled generated normal",
        )?;
        v.normal = n.normalize().to_array();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tangent_fixture(tangent: [f32; 4], count: usize, semantic: &str) -> Vec<u8> {
        tangent_fixture_with_accessor(tangent, count, semantic, "VEC4", 5126, false)
    }

    fn tangent_fixture_with_accessor(
        tangent: [f32; 4],
        count: usize,
        semantic: &str,
        dimensions: &str,
        component_type: u32,
        normalized: bool,
    ) -> Vec<u8> {
        let mut bin = Vec::new();
        for position in [[0_f32, 0., 0.], [1., 0., 0.], [0., 1., 0.]] {
            for value in position {
                bin.extend_from_slice(&value.to_le_bytes());
            }
        }
        for _ in 0..count {
            for value in tangent {
                bin.extend_from_slice(&value.to_le_bytes());
            }
        }
        let document = serde_json::json!({
            "asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],
            "nodes":[{"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0,semantic:1}}]}],
            "buffers":[{"byteLength":bin.len()}],
            "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},
                {"buffer":0,"byteOffset":36,"byteLength":count*16}],
            "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]},
                {"bufferView":1,"componentType":component_type,"count":count,"type":dimensions,"normalized":normalized}]
        });
        let mut json = serde_json::to_vec(&document).unwrap();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        let mut bytes = b"glTF".to_vec();
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(
            &u32::try_from(28 + json.len() + bin.len())
                .unwrap()
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&u32::try_from(json.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(b"JSON");
        bytes.extend_from_slice(&json);
        bytes.extend_from_slice(&u32::try_from(bin.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(b"BIN\0");
        bytes.extend_from_slice(&bin);
        bytes
    }

    #[test]
    fn source_tangents_survive_rest_and_skinned_sampling_unchanged() {
        let tangent = [1., 0., 0., -1.];
        let static_asset = Asset::from_glb(&tangent_fixture(tangent, 3, "TANGENT")).unwrap();
        let frame = static_asset.sample(None, 0.).unwrap();
        assert_eq!(frame.meshes[0].source_tangents, Some(vec![tangent; 3]));
        let mut skinned = asset();
        skinned.primitives[0].source_tangents = Some(vec![tangent; 3]);
        assert_eq!(
            skinned.sample(Some(0), 3.).unwrap().meshes[0].source_tangents,
            Some(vec![tangent; 3])
        );
    }

    #[test]
    fn malformed_tangents_and_other_attributes_still_fail() {
        for (tangent, count, semantic) in [
            ([1., 0., 0., 1.], 2, "TANGENT"),
            ([1., 0., 0., 0.], 3, "TANGENT"),
            ([2., 0., 0., 1.], 3, "TANGENT"),
            ([f32::NAN, 0., 0., 1.], 3, "TANGENT"),
            ([1., 0., 0., f32::INFINITY], 3, "TANGENT"),
            ([1., 0., 0., 1.], 3, "COLOR_0"),
        ] {
            assert!(Asset::from_glb(&tangent_fixture(tangent, count, semantic)).is_err());
        }
        for (dimensions, component_type, normalized) in [
            ("VEC3", 5126, false),
            ("VEC4", 5123, false),
            ("VEC4", 5126, true),
        ] {
            assert!(Asset::from_glb(&tangent_fixture_with_accessor(
                [1., 0., 0., 1.],
                3,
                "TANGENT",
                dimensions,
                component_type,
                normalized
            ))
            .is_err());
        }
    }

    fn asset() -> Asset {
        let rest = Trs {
            t: Vec3::ZERO,
            r: Quat::IDENTITY,
            s: Vec3::ONE,
        };
        Asset {
            nodes: vec![
                Node {
                    name: "joint".into(),
                    parent: None,
                    rest: rest.matrix(),
                    trs: Some(rest),
                },
                Node {
                    name: "mesh".into(),
                    parent: None,
                    rest: Mat4::from_translation(Vec3::splat(99.)),
                    trs: None,
                },
            ],
            order: vec![0, 1],
            skins: vec![Skin {
                joints: vec![0],
                inverse: vec![Mat4::IDENTITY],
            }],
            primitives: vec![Primitive {
                node: 1,
                skin: Some(0),
                positions: vec![[1., 0., 0.], [2., 0., 0.], [1., 1., 0.]],
                normals: Some(vec![[0., 0., 1.]; 3]),
                texcoords: None,
                source_tangents: None,
                joints: Some(vec![[0; 4]; 3]),
                weights: Some(vec![[1., 0., 0., 0.]; 3]),
                indices: vec![0, 1, 2],
                color: [1.; 4],
            }],
            clips: vec![Clip {
                info: ClipInfo {
                    name: "turn".into(),
                    start: 1.,
                    end: 3.,
                },
                channels: vec![Channel {
                    node: 0,
                    times: vec![1., 3.],
                    linear: true,
                    values: Values::Rotation(vec![
                        Quat::IDENTITY,
                        -Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
                    ]),
                }],
            }],
        }
    }
    #[test]
    fn quaternion_short_path_clamping_and_ignored_mesh_transform() {
        let asset = asset();
        let point =
            |t| Vec3::from_array(asset.sample(Some(0), t).unwrap().meshes[0].vertices[0].position);
        assert!((point(0.) - Vec3::X).length() < 1e-6);
        assert!(
            (point(2.)
                - Vec3::new(
                    std::f32::consts::FRAC_1_SQRT_2,
                    std::f32::consts::FRAC_1_SQRT_2,
                    0.
                ))
            .length()
                < 1e-6
        );
        assert!((point(4.) - Vec3::Y).length() < 1e-6);
    }
    #[test]
    fn singular_scale_and_nonfinite_time_are_errors() {
        let mut asset = asset();
        asset.clips[0].channels[0].values = Values::Scale(vec![Vec3::ONE, -Vec3::ONE]);
        assert!(asset.sample(Some(0), 2.).is_err());
        assert!(asset.sample(None, f32::INFINITY).is_err());
    }
    #[test]
    fn nonuniform_normal_transform_matches_plane_normal() {
        let mut asset = asset();
        asset.nodes[0].rest = Mat4::from_scale(Vec3::new(2., 1., 1.));
        asset.primitives[0].normals = Some(vec![[1., 0., 1.]; 3]);
        let frame = asset.sample(None, 0.).unwrap();
        let normal = Vec3::from_array(frame.meshes[0].vertices[0].normal);
        assert!((normal - Vec3::new(0.5, 0., 1.).normalize()).length() < 1e-6);
    }
}
