# Native skeletal playback

The optional `skeletal` feature owns bounded GLB animation sampling and CPU linear blend skinning. It uses `glam` for standard transforms and emits the existing `mesh::Vertex` ABI. Default builds do not gain glTF dependencies; headless clients can disable default features and select only `skeletal`.

`skeletal::Asset::from_glb(bytes)` validates an embedded GLB, including binary ranges, hierarchy, skin references and rest-pose deformation. `clips()` reports names and time ranges; `node_names()` preserves node-index order. `sample(Some(clip), seconds)` returns world matrices and posed meshes; `sample(None, seconds)` returns the rest pose. Samples have no mutable playback state. Hosts supply clocks, looping and draw submission.

STEP and LINEAR translation/scale channels are supported; LINEAR rotations use shortest-path quaternion interpolation. Each channel clamps independently. Joint palettes are world-joint × inverse-bind; skinned mesh-node transforms are ignored as required by [glTF 2.0](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html). Provided normals use inverse transpose of each blended transform. Missing normals are generated from the deformed triangle faces. Singular transforms and cancelled normals fail explicitly.

The reader supports indexed triangle meshes, one UV set and one four-influence skin set. It retains UVs and base color for callers. It rejects morphs, sparse accessors, extra influence sets, CUBICSPLINE, unknown required extensions, nonfinite values, invalid references and nodes outside the explicit default scene. Budgets include 128 MiB GLB / 16 MiB JSON, 4,096 nodes / depth 64, 64 skins or clips, 256 joints per skin, 300,000 instanced vertices, 900,000 indices, 768 channels per clip and 98,304 total channel keys. No source-format extraction or automatic rigging occurs here.

With `wgpu`, `mesh::Renderer` uploads bounded posed meshes into the shared mesh shader, using two-sided triangles and a single-sample Depth32Float attachment. Callers supply a compatible color target, depth attachment and rigid model matrix. This renderer clears both attachments each call. It does not reconstruct source textures or PBR materials.

## Offscreen runbook

```sh
cargo check --features skeletal
cargo test --features skeletal --lib
cargo run --features skeletal --example skeletal_offscreen -- \
  /path/to/model.glb /tmp/new-pose-capture 0 0 0.5 0.9999 1 1.5 2
```

The example renders up to 64 frames using one fixed XZ camera fitted to the entire sampled sequence. It writes transparent 256px PNGs and a JSON receipt binding GLB, shader, adapter, camera, positions, normals, indices and image hashes. It uses an illustrative cyan material. The output directory must be new; GPU rendering completes before output creation. A filesystem failure can leave a partial directory and returns an error.

Reversal retains an authored two-joint COLLADA fixture, Metal capture and independent scalar pose/pixel verifier in `crates/reversal-assets/tests/evidence/gfx-skeletal/` and `examples/verify_gfx_replay.rs`. Six frames check root translation, STEP boundaries, mixed weights, inverse binds, seams and a fixed-camera silhouette. Separate tests check supplied normals under nonuniform deformation and quaternion interpolation. This qualifies the bounded authored path; commercial/exporter assets, Canvas integration and UE retargeting have separate qualification tasks.

Retargeting will use explicit bone correspondence, rest-pose orientation corrections, root-motion scale and helper-bone policies. Matching names alone will not establish interoperability or foot contact preservation. This module currently plays native rigs; it does not retarget or load `.uasset` files.
