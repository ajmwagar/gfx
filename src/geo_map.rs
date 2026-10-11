//! Bounded regional geographic view: GPU line geometry and shared glyph placement.
//! Not a globe camera, tile downloader, feature catalog or GIS query engine.
use crate::{
    geo::Extent, geometry::clip_unit_segment, topology::Label, Primitive, Rect, Scene, ThemeRole,
};
use serde::{Deserialize, Serialize};

/// Retained, bounded observation history and delayed linear map motion.
pub mod motion;

/// Invalid source geometry or unusable view extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid geographic map frame or geometry")]
pub struct MapError;

/// Geographic point without an invented altitude or vertical datum.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    /// WGS84 latitude in degrees.
    pub latitude_deg: f64,
    /// WGS84 longitude in degrees.
    pub longitude_deg: f64,
}
/// Stable display identity; observation age and source semantics stay with owner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    /// Owner's stable identity, scoped by its source/domain adapter.
    pub id: String,
    /// Bounded caption, used only for selected or collision-free labels.
    pub label: String,
    /// WGS84 latitude.
    pub latitude_deg: f64,
    /// WGS84 longitude.
    pub longitude_deg: f64,
    /// Semantic theme color, never a hard-coded source palette.
    pub role: ThemeRole,
}
/// Observed line geometry. This does not silently fill polygon rings/holes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Path {
    /// Stable display identity.
    pub id: String,
    /// Theme role for boundaries, infrastructure or observed tracks.
    pub role: ThemeRole,
    /// Connected geographic positions. Segments follow the shortest longitude wrap.
    pub points: Vec<Point>,
}
/// Shared data-driven map widget contract, independent of Panopticon semantics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Map {
    /// Explicit linear latitude/longitude region (not Mercator or ENU).
    pub extent: Extent,
    /// Up to 512 admitted contacts; provider reports any reduction separately.
    pub markers: Vec<Marker>,
    /// Up to 128 paths and 8192 total source positions.
    pub paths: Vec<Path>,
    /// View selection; never a command to the data provider.
    #[serde(default)]
    pub selected: Option<String>,
}
fn text(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.chars().count() <= limit && !value.chars().any(char::is_control)
}
impl Map {
    /// Select a visible contact using deterministic geographic navigation.
    /// # Errors
    /// Rejects invalid revisions before changing selection.
    pub fn navigate(&mut self, action: crate::panels::TaskNavigation) -> Result<bool, MapError> {
        use crate::panels::TaskNavigation;
        self.validate()?;
        let visible: Vec<_> = self
            .markers
            .iter()
            .filter_map(|m| {
                let p = self
                    .extent
                    .normalized(m.latitude_deg, m.longitude_deg)
                    .ok()?;
                ((0.0..=1.0).contains(&p[0]) && (0.0..=1.0).contains(&p[1]))
                    .then_some((m.id.clone(), p))
            })
            .collect();
        if matches!(action, TaskNavigation::Back) {
            return Ok(self.selected.take().is_some());
        }
        if matches!(action, TaskNavigation::Next | TaskNavigation::Previous) {
            let mut ids: Vec<_> = visible.iter().map(|(id, _)| id.clone()).collect();
            ids.sort();
            if ids.is_empty() {
                return Ok(false);
            }
            let index = self
                .selected
                .as_ref()
                .and_then(|selected| ids.iter().position(|id| id == selected));
            let next = match (index, action) {
                (Some(index), TaskNavigation::Next) => (index + 1) % ids.len(),
                (Some(index), TaskNavigation::Previous) => (index + ids.len() - 1) % ids.len(),
                _ => 0,
            };
            self.selected = Some(ids[next].clone());
            return Ok(true);
        }
        let current = visible
            .iter()
            .find(|(id, _)| Some(id) == self.selected.as_ref());
        let Some((_, origin)) = current else {
            self.selected = visible.first().map(|(id, _)| id.clone());
            return Ok(self.selected.is_some());
        };
        let direction = match action {
            TaskNavigation::Left => [-1.0, 0.0],
            TaskNavigation::Right => [1.0, 0.0],
            TaskNavigation::Up => [0.0, -1.0],
            TaskNavigation::Down => [0.0, 1.0],
            _ => return Ok(true),
        };
        let candidate = visible
            .iter()
            .filter_map(|(id, p)| {
                let dx = p[0] - origin[0];
                let dy = p[1] - origin[1];
                let forward = dx * direction[0] + dy * direction[1];
                (forward > 0.0).then_some((
                    id,
                    dx * dx + dy * dy + (dx * direction[1] - dy * direction[0]).abs(),
                ))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(b.0)));
        if let Some((id, _)) = candidate {
            self.selected = Some(id.clone());
        }
        Ok(true)
    }
    /// Validate the complete revision before painting any graphics.
    /// # Errors
    /// Rejects invalid coordinates, identities, selection or resource limits.
    pub fn validate(&self) -> Result<(), MapError> {
        self.extent.validate().map_err(|_| MapError)?;
        if self.markers.len() > 512
            || self.paths.len() > 128
            || self.paths.iter().map(|p| p.points.len()).sum::<usize>() > 8192
        {
            return Err(MapError);
        }
        for (index, m) in self.markers.iter().enumerate() {
            if !text(&m.id, 128)
                || !text(&m.label, 48)
                || self.markers[..index].iter().any(|n| n.id == m.id)
            {
                return Err(MapError);
            }
            self.extent
                .normalized(m.latitude_deg, m.longitude_deg)
                .map_err(|_| MapError)?;
        }
        for (index, path) in self.paths.iter().enumerate() {
            if !text(&path.id, 128)
                || path.points.len() < 2
                || self.paths[..index].iter().any(|p| p.id == path.id)
            {
                return Err(MapError);
            }
            for point in &path.points {
                self.extent
                    .normalized(point.latitude_deg, point.longitude_deg)
                    .map_err(|_| MapError)?;
            }
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|id| !self.markers.iter().any(|m| &m.id == id))
        {
            return Err(MapError);
        }
        Ok(())
    }
    fn inner(&self, bounds: Rect) -> Result<Rect, MapError> {
        self.validate()?;
        if !bounds.is_valid()
            || !bounds.right().is_finite()
            || !bounds.bottom().is_finite()
            || bounds.width < 240.0
            || bounds.height < 180.0
            || bounds.width > 16384.0
            || bounds.height > 16384.0
        {
            return Err(MapError);
        }
        Ok(Rect::new(
            bounds.x + 56.0,
            bounds.y + 24.0,
            bounds.width - 70.0,
            bounds.height - 58.0,
        ))
    }
    fn project(&self, point: Point, inner: Rect) -> Result<Option<[f32; 2]>, MapError> {
        let [x, y] = self
            .extent
            .normalized(point.latitude_deg, point.longitude_deg)
            .map_err(|_| MapError)?;
        if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
            return Ok(None);
        }
        #[allow(clippy::cast_possible_truncation)]
        let screen = [
            inner.x + x as f32 * inner.width,
            inner.y + y as f32 * inner.height,
        ];
        Ok(Some(screen))
    }
    /// Append theme-driven grid, clipped geographic paths and contact glyphs.
    /// Geometry is cached/retained by the host and painted by its existing GPU path.
    /// # Errors
    /// Invalid inputs leave the caller's scene untouched.
    #[allow(clippy::cast_possible_truncation)]
    pub fn append(&self, scene: &mut Scene, bounds: Rect) -> Result<(), MapError> {
        let inner = self.inner(bounds)?;
        scene.push(Primitive::rounded_rect(
            bounds,
            0.0,
            ThemeRole::SurfaceRecessed,
        ));
        for i in 0..=4 {
            let x = inner.x + i as f32 * inner.width / 4.0;
            let y = inner.y + i as f32 * inner.height / 4.0;
            scene.push(Primitive::line(
                [x, inner.y],
                [x, inner.bottom()],
                0.5,
                ThemeRole::Outline,
            ));
            scene.push(Primitive::line(
                [inner.x, y],
                [inner.right(), y],
                0.5,
                ThemeRole::Outline,
            ));
        }
        let span = if self.extent.east_deg > self.extent.west_deg {
            self.extent.east_deg - self.extent.west_deg
        } else {
            360.0 + self.extent.east_deg - self.extent.west_deg
        };
        let wrap = 360.0 / span;
        let map = |p: [f64; 2]| {
            [
                inner.x + p[0] as f32 * inner.width,
                inner.y + p[1] as f32 * inner.height,
            ]
        };
        for path in &self.paths {
            for pair in path.points.windows(2) {
                let a = self
                    .extent
                    .normalized(pair[0].latitude_deg, pair[0].longitude_deg)
                    .map_err(|_| MapError)?;
                let mut b = self
                    .extent
                    .normalized(pair[1].latitude_deg, pair[1].longitude_deg)
                    .map_err(|_| MapError)?;
                let delta = (pair[1].longitude_deg - pair[0].longitude_deg + 180.0)
                    .rem_euclid(360.0)
                    - 180.0;
                b[0] = a[0] + delta / span;
                for shift in [-wrap, 0.0, wrap] {
                    if let Some((first, last)) =
                        clip_unit_segment([a[0] + shift, a[1]], [b[0] + shift, b[1]])
                    {
                        scene.push(Primitive::line(map(first), map(last), 1.0, path.role));
                    }
                }
            }
        }
        for marker in &self.markers {
            if let Some([x, y]) = self.project(
                Point {
                    latitude_deg: marker.latitude_deg,
                    longitude_deg: marker.longitude_deg,
                },
                inner,
            )? {
                let r = if self.selected.as_ref() == Some(&marker.id) {
                    5.0
                } else {
                    3.0
                };
                if x - r < inner.x
                    || x + r > inner.right()
                    || y - r < inner.y
                    || y + r > inner.bottom()
                {
                    continue;
                }
                for (a, b) in [
                    ([x, y - r], [x + r, y]),
                    ([x + r, y], [x, y + r]),
                    ([x, y + r], [x - r, y]),
                    ([x - r, y], [x, y - r]),
                ] {
                    scene.push(Primitive::line(a, b, 1.5, marker.role));
                }
            }
        }
        Ok(())
    }
    /// Geographic coordinate labels and bounded, collision-aware contact captions.
    /// # Errors
    /// Rejects invalid frames or widget bounds.
    #[allow(clippy::cast_possible_truncation)]
    pub fn labels(&self, bounds: Rect) -> Result<Vec<Label>, MapError> {
        let inner = self.inner(bounds)?;
        let mut labels = vec![Label {
            text: "Regional geographic view".into(),
            bounds: Rect::new(bounds.x + 8.0, bounds.y + 2.0, bounds.width - 16.0, 20.0),
            role: ThemeRole::Text,
        }];
        let span = if self.extent.east_deg > self.extent.west_deg {
            self.extent.east_deg - self.extent.west_deg
        } else {
            360.0 + self.extent.east_deg - self.extent.west_deg
        };
        for i in 0..=4 {
            let lat = self.extent.north_deg
                - (self.extent.north_deg - self.extent.south_deg) * f64::from(i) / 4.0;
            let lon = (self.extent.west_deg + span * f64::from(i) / 4.0 + 180.0).rem_euclid(360.0)
                - 180.0;
            labels.push(Label {
                text: format!("{:.1}°{}", lat.abs(), if lat < 0.0 { "S" } else { "N" }),
                bounds: Rect::new(
                    bounds.x + 2.0,
                    inner.y + i as f32 * inner.height / 4.0 - 8.0,
                    52.0,
                    16.0,
                ),
                role: ThemeRole::TextMuted,
            });
            labels.push(Label {
                text: format!("{:.1}°{}", lon.abs(), if lon < 0.0 { "W" } else { "E" }),
                bounds: Rect::new(
                    (inner.x + i as f32 * inner.width / 4.0 - 25.0)
                        .clamp(bounds.x, bounds.right() - 64.0),
                    inner.bottom() + 8.0,
                    64.0,
                    16.0,
                ),
                role: ThemeRole::TextMuted,
            });
        }
        let mut occupied: Vec<Rect> = Vec::new();
        let ordered = self
            .markers
            .iter()
            .filter(|m| self.selected.as_ref() == Some(&m.id))
            .chain(
                self.markers
                    .iter()
                    .filter(|m| self.selected.as_ref() != Some(&m.id)),
            );
        for marker in ordered {
            if occupied.len() >= 8 {
                break;
            }
            if let Some([x, y]) = self.project(
                Point {
                    latitude_deg: marker.latitude_deg,
                    longitude_deg: marker.longitude_deg,
                },
                inner,
            )? {
                let box_ = Rect::new(x + 7.0, y - 8.0, 96.0, 16.0);
                if box_.right() > inner.right()
                    || box_.y < inner.y
                    || box_.bottom() > inner.bottom()
                    || occupied.iter().any(|b| {
                        box_.x < b.right()
                            && box_.right() > b.x
                            && box_.y < b.bottom()
                            && box_.bottom() > b.y
                    })
                {
                    continue;
                }
                occupied.push(box_);
                labels.push(Label {
                    text: marker.label.clone(),
                    bounds: box_,
                    role: marker.role,
                });
            }
        }
        Ok(labels)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cycle_visits_coincident_contacts_in_sorted_order_and_wraps() {
        use crate::panels::TaskNavigation;
        let mut map = fixture();
        map.markers.clear();
        for id in ["c", "a", "b", "outside"] {
            map.markers.push(Marker {
                id: id.into(),
                label: id.into(),
                latitude_deg: if id == "outside" { 0.0 } else { 47.6 },
                longitude_deg: -122.3,
                role: ThemeRole::Primary,
            });
        }
        for expected in ["a", "b", "c", "a"] {
            map.navigate(TaskNavigation::Next).unwrap();
            assert_eq!(map.selected.as_deref(), Some(expected));
        }
        map.navigate(TaskNavigation::Previous).unwrap();
        assert_eq!(map.selected.as_deref(), Some("c"));
    }
    fn fixture() -> Map {
        Map {
            extent: Extent {
                south_deg: 45.0,
                north_deg: 50.0,
                west_deg: -126.0,
                east_deg: -120.0,
            },
            markers: vec![Marker {
                id: "adsb/1".into(),
                label: "FLIGHT".into(),
                latitude_deg: 47.6,
                longitude_deg: -122.3,
                role: ThemeRole::Primary,
            }],
            paths: vec![Path {
                id: "boundary".into(),
                role: ThemeRole::Outline,
                points: vec![
                    Point {
                        latitude_deg: 46.0,
                        longitude_deg: -130.0,
                    },
                    Point {
                        latitude_deg: 49.0,
                        longitude_deg: -115.0,
                    },
                ],
            }],
            selected: None,
        }
    }
    #[test]
    fn frame_roundtrips_and_labels_fit_tv_bounds() {
        let frame = fixture();
        let bounds = Rect::new(0.0, 0.0, 600.0, 400.0);
        assert_eq!(
            serde_json::from_value::<Map>(serde_json::to_value(&frame).unwrap()).unwrap(),
            frame
        );
        let labels = frame.labels(bounds).unwrap();
        assert!(labels.iter().any(|l| l.text == "FLIGHT"));
        assert!(labels.iter().all(|l| l.bounds.x >= 0.0
            && l.bounds.y >= 0.0
            && l.bounds.right() <= bounds.right()
            && l.bounds.bottom() <= bounds.bottom()));
        let mut scene = Scene::new();
        frame.append(&mut scene, bounds).unwrap();
        assert!(scene.len() > 10);
    }

    #[test]
    fn geographic_navigation_selects_directionally_and_clears() {
        use crate::panels::TaskNavigation;
        let mut map = fixture();
        let mut second = map.markers[0].clone();
        second.id = "adsb/2".into();
        second.longitude_deg += 1.0;
        map.markers.push(second);
        assert!(map.navigate(TaskNavigation::Focus).unwrap());
        assert_eq!(map.selected.as_deref(), Some("adsb/1"));
        map.navigate(TaskNavigation::Right).unwrap();
        assert_eq!(map.selected.as_deref(), Some("adsb/2"));
        map.navigate(TaskNavigation::Left).unwrap();
        assert_eq!(map.selected.as_deref(), Some("adsb/1"));
        assert!(map.navigate(TaskNavigation::Back).unwrap());
        assert!(!map.navigate(TaskNavigation::Back).unwrap());
        map.markers.clear();
        assert!(!map.navigate(TaskNavigation::Focus).unwrap());
    }
    #[test]
    fn invalid_revision_never_partially_mutates_geometry() {
        let mut frame = fixture();
        frame.markers.push(frame.markers[0].clone());
        let mut scene = Scene::new();
        assert!(frame
            .append(&mut scene, Rect::new(0.0, 0.0, 600.0, 400.0))
            .is_err());
        assert!(scene.is_empty());
        frame = fixture();
        frame.paths[0].points[1].latitude_deg = f64::NAN;
        assert!(frame.validate().is_err());
        frame = fixture();
        frame.selected = Some("unknown".into());
        assert!(frame.validate().is_err());
    }
    #[test]
    fn wrapped_paths_cross_the_date_line_without_drawing_across_world() {
        let mut frame = fixture();
        frame.extent = Extent {
            south_deg: -10.0,
            north_deg: 10.0,
            west_deg: -180.0,
            east_deg: 180.0,
        };
        frame.markers.clear();
        frame.paths[0].role = ThemeRole::Success;
        frame.paths[0].points = vec![
            Point {
                latitude_deg: 0.0,
                longitude_deg: 179.0,
            },
            Point {
                latitude_deg: 0.0,
                longitude_deg: -179.0,
            },
        ];
        let mut scene = Scene::new();
        frame
            .append(&mut scene, Rect::new(0.0, 0.0, 600.0, 400.0))
            .unwrap();
        let segments: Vec<_> = scene
            .iter()
            .filter(|p| p.fill == ThemeRole::Success)
            .collect();
        assert_eq!(segments.len(), 2);
        assert!(segments.iter().all(|p| p.value < 2.0));
    }
}
