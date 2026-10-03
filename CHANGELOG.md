# Changelog

All notable changes to this project will be documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Portable `Point`, `Size`, and `Rect` geometry.
- Validated retained vector-path commands.
- Renderer-neutral IOSurface and DMA-BUF descriptor/lease contract.
- GPU-free contract builds through `--no-default-features`.
- Toolkit-independent orbit cameras, named CAD orientations, and pickable view-cube geometry.
- Retained WGPU frame presentation for decoded and external textures.
- WGS84/ECEF mesh primitives and a caller-targeted retained globe renderer.

## [0.1.0] - 2026-10-03

### Added

- Caller-owned wGPU rendering contract.
- One-draw instanced primitive renderer.
- Rounded panels, discs, meters, knobs, and lamps.
- Versioned semantic themes with linear-light colors.
- Stable CPU/GPU buffer growth and renderer statistics.
- Offscreen example, shader validation, tests, and scene benchmark.

[Unreleased]: https://github.com/ajmwagar/gfx/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/ajmwagar/gfx/releases/tag/v0.1.0
