use crate::{Point, Rect};

const HALF: f32 = 1.0;
const BEVEL: f32 = 0.66;

/// Semantic part of a projected orientation cube.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrientationRegionKind {
    /// One of six principal faces.
    Face,
    /// One of twelve beveled edges.
    Edge,
    /// One of eight cut corners.
    Corner,
}

/// One region in the toolkit-independent three-dimensional cube model.
#[derive(Debug, Clone, PartialEq)]
pub struct OrientationRegion {
    /// Named orientation selected by this region.
    pub orientation: &'static str,
    /// Visual and semantic region kind.
    pub kind: OrientationRegionKind,
    /// Three-dimensional cube vertices.
    pub vertices: Vec<[f32; 3]>,
}

/// One projected region in painter order.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedOrientationRegion {
    /// Named orientation selected by this region.
    pub orientation: &'static str,
    /// Visual and semantic region kind.
    pub kind: OrientationRegionKind,
    /// Screen-space polygon.
    pub points: Vec<Point>,
    /// Average camera-space depth used for painter ordering.
    pub depth: f32,
}

impl ProjectedOrientationRegion {
    /// Polygon centroid, useful for a face label or test target.
    pub fn centroid(&self) -> Point {
        let (x, y) = self
            .points
            .iter()
            .fold((0.0, 0.0), |(x, y), point| (x + point.x, y + point.y));
        let count = small_count(self.points.len());
        Point::new(x / count, y / count)
    }

    /// Whether a face label points toward the viewer enough to remain legible.
    pub fn label_visible(&self, yaw_degrees: f32, pitch_degrees: f32) -> bool {
        if self.kind != OrientationRegionKind::Face {
            return false;
        }
        let normal = match self.orientation {
            "left" => [-1.0, 0.0, 0.0],
            "right" => [1.0, 0.0, 0.0],
            "bottom" => [0.0, -1.0, 0.0],
            "top" => [0.0, 1.0, 0.0],
            "back" => [0.0, 0.0, -1.0],
            "front" => [0.0, 0.0, 1.0],
            _ => return false,
        };
        rotate(normal, yaw_degrees, pitch_degrees)[2] > 0.16
    }

    fn contains(&self, point: Point) -> bool {
        let mut inside = false;
        let mut previous = self.points.last().copied().unwrap_or_default();
        for current in self.points.iter().copied() {
            if ((current.y > point.y) != (previous.y > point.y))
                && point.x
                    < (previous.x - current.x) * (point.y - current.y) / (previous.y - current.y)
                        + current.x
            {
                inside = !inside;
            }
            previous = current;
        }
        inside
    }
}

/// Pickable, projected orientation cube geometry.
#[derive(Debug, Clone)]
pub struct OrientationCube {
    regions: Vec<OrientationRegion>,
}

impl Default for OrientationCube {
    fn default() -> Self {
        Self::new()
    }
}

impl OrientationCube {
    /// Builds the canonical six-face, twelve-edge, eight-corner cube.
    pub fn new() -> Self {
        Self { regions: regions() }
    }

    /// Canonical three-dimensional regions.
    pub fn regions(&self) -> &[OrientationRegion] {
        &self.regions
    }

    /// Projects every region into the supplied bounds, back to front.
    pub fn project(
        &self,
        bounds: Rect,
        yaw_degrees: f32,
        pitch_degrees: f32,
    ) -> Vec<ProjectedOrientationRegion> {
        let radius = bounds.width.min(bounds.height) * 0.36;
        let center = Point::new(
            bounds.x + bounds.width * 0.5,
            bounds.y + bounds.height * 0.5,
        );
        let mut projected = self
            .regions
            .iter()
            .map(|region| {
                let rotated = region
                    .vertices
                    .iter()
                    .copied()
                    .map(|vertex| rotate(vertex, yaw_degrees, pitch_degrees))
                    .collect::<Vec<_>>();
                let depth =
                    rotated.iter().map(|point| point[2]).sum::<f32>() / small_count(rotated.len());
                let points = rotated
                    .into_iter()
                    .map(|point| {
                        Point::new(center.x + point[0] * radius, center.y - point[1] * radius)
                    })
                    .collect();
                ProjectedOrientationRegion {
                    orientation: region.orientation,
                    kind: region.kind,
                    points,
                    depth,
                }
            })
            .collect::<Vec<_>>();
        projected.sort_by(|left, right| left.depth.total_cmp(&right.depth));
        projected
    }

    /// Returns the front-most projected region containing `point`.
    pub fn pick<'a>(
        &self,
        projected: &'a [ProjectedOrientationRegion],
        point: Point,
    ) -> Option<&'a ProjectedOrientationRegion> {
        projected.iter().rev().find(|region| region.contains(point))
    }
}

fn small_count(len: usize) -> f32 {
    // Orientation polygons are canonical triangles and quads. Saturating here
    // keeps this helper total if that internal invariant changes.
    f32::from(u16::try_from(len.max(1)).unwrap_or(u16::MAX))
}

fn rotate([x, y, z]: [f32; 3], yaw_degrees: f32, pitch_degrees: f32) -> [f32; 3] {
    let (sin_yaw, cos_yaw) = yaw_degrees.to_radians().sin_cos();
    let (sin_pitch, cos_pitch) = pitch_degrees.to_radians().sin_cos();
    let rotated_x = cos_yaw * x - sin_yaw * z;
    let rotated_z = sin_yaw * x + cos_yaw * z;
    [
        rotated_x,
        cos_pitch * y - sin_pitch * rotated_z,
        sin_pitch * y + cos_pitch * rotated_z,
    ]
}

fn regions() -> Vec<OrientationRegion> {
    let mut result = Vec::with_capacity(26);
    for (axis, negative, positive) in [
        (0, "left", "right"),
        (1, "bottom", "top"),
        (2, "back", "front"),
    ] {
        for (sign, orientation) in [(-1.0, negative), (1.0, positive)] {
            let others = (0..3)
                .filter(|candidate| *candidate != axis)
                .collect::<Vec<_>>();
            let vertices = [
                (-BEVEL, -BEVEL),
                (BEVEL, -BEVEL),
                (BEVEL, BEVEL),
                (-BEVEL, BEVEL),
            ]
            .into_iter()
            .map(|(first, second)| {
                let mut vertex = [0.0; 3];
                vertex[axis] = sign * HALF;
                vertex[others[0]] = first;
                vertex[others[1]] = second;
                vertex
            })
            .collect();
            result.push(OrientationRegion {
                orientation,
                kind: OrientationRegionKind::Face,
                vertices,
            });
        }
    }
    for free in 0..3 {
        let fixed = (0..3).filter(|axis| *axis != free).collect::<Vec<_>>();
        for sign_a in [-1.0, 1.0] {
            for sign_b in [-1.0, 1.0] {
                let vertices = [
                    (-BEVEL, HALF, BEVEL),
                    (BEVEL, HALF, BEVEL),
                    (BEVEL, BEVEL, HALF),
                    (-BEVEL, BEVEL, HALF),
                ]
                .into_iter()
                .map(|(along, outer_a, outer_b)| {
                    let mut vertex = [0.0; 3];
                    vertex[free] = along;
                    vertex[fixed[0]] = sign_a * outer_a;
                    vertex[fixed[1]] = sign_b * outer_b;
                    vertex
                })
                .collect();
                result.push(OrientationRegion {
                    orientation: edge_orientation(free, sign_a, sign_b),
                    kind: OrientationRegionKind::Edge,
                    vertices,
                });
            }
        }
    }
    for sign_x in [-1.0, 1.0] {
        for sign_y in [-1.0, 1.0] {
            for sign_z in [-1.0, 1.0] {
                result.push(OrientationRegion {
                    orientation: corner_orientation(sign_x, sign_y, sign_z),
                    kind: OrientationRegionKind::Corner,
                    vertices: vec![
                        [sign_x * HALF, sign_y * BEVEL, sign_z * BEVEL],
                        [sign_x * BEVEL, sign_y * HALF, sign_z * BEVEL],
                        [sign_x * BEVEL, sign_y * BEVEL, sign_z * HALF],
                    ],
                });
            }
        }
    }
    result
}

fn edge_orientation(free: usize, first: f32, second: f32) -> &'static str {
    match (free, first > 0.0, second > 0.0) {
        (0, false, false) => "bottom_back",
        (0, false, true) => "bottom_front",
        (0, true, false) => "top_back",
        (0, true, true) => "top_front",
        (1, false, false) => "back_left",
        (1, false, true) => "front_left",
        (1, true, false) => "back_right",
        (1, true, true) => "front_right",
        (2, false, false) => "bottom_left",
        (2, false, true) => "bottom_right",
        (2, true, false) => "top_left",
        (2, true, true) => "top_right",
        _ => unreachable!("axis is generated in 0..3"),
    }
}

fn corner_orientation(x: f32, y: f32, z: f32) -> &'static str {
    match (x > 0.0, y > 0.0, z > 0.0) {
        (true, true, true) => "iso",
        (false, true, true) => "iso_front_left",
        (true, true, false) => "iso_back_right",
        (false, true, false) => "iso_rear",
        (true, false, true) => "iso_bottom",
        (false, false, true) => "iso_bottom_left",
        (true, false, false) => "iso_bottom_back_right",
        (false, false, false) => "iso_bottom_rear",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::named_orientation;

    #[test]
    fn cube_has_all_pickable_cad_regions() {
        let cube = OrientationCube::new();
        assert_eq!(cube.regions().len(), 26);
        for region in cube.regions() {
            assert!(named_orientation(region.orientation).is_some());
        }
    }

    #[test]
    fn center_pick_tracks_camera_orientation() {
        let cube = OrientationCube::new();
        let bounds = Rect::new(0.0, 0.0, 132.0, 132.0);
        let center = Point::new(66.0, 66.0);
        for (yaw, expected) in [(0.0, "front"), (90.0, "right"), (-90.0, "left")] {
            let projected = cube.project(bounds, yaw, 0.0);
            assert_eq!(
                cube.pick(&projected, center)
                    .map(|region| region.orientation),
                Some(expected),
            );
        }
    }

    #[test]
    fn labels_only_appear_on_front_facing_faces() {
        let cube = OrientationCube::new();
        let projected = cube.project(Rect::new(0.0, 0.0, 132.0, 132.0), 0.0, 0.0);
        let front = projected
            .iter()
            .find(|region| region.orientation == "front")
            .unwrap();
        let back = projected
            .iter()
            .find(|region| region.orientation == "back")
            .unwrap();
        assert!(front.label_visible(0.0, 0.0));
        assert!(!back.label_visible(0.0, 0.0));
    }
}
