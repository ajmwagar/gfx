//! Geographic geometry without feeds, application semantics or GPU ownership.
//! WGS84 positions remain f64; hosts retain the resulting GPU scene/buffers.
use crate::{geodetic_to_ecef, Primitive, Rect, Scene, ThemeRole};
use serde::{Deserialize, Serialize};

/// Invalid coordinates, geographic extent, render bounds or resource limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid geographic position, frame, extent or marker batch")]
pub struct GeoError;

/// WGS84 geodetic coordinates. Height is ellipsoidal metres, not MSL/geoid height.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Position {
    /// Latitude in degrees, including the poles.
    pub latitude_deg: f64,
    /// Longitude in degrees; callers explicitly normalize before admission.
    pub longitude_deg: f64,
    /// Ellipsoidal height in metres; an unknown altitude must not become zero.
    pub ellipsoidal_height_m: f64,
}
impl Position {
    /// Validate a physical position before coordinate conversion.
    /// # Errors
    /// Rejects nonfinite or out-of-domain angles and unusable heights.
    pub fn validate(self) -> Result<(), GeoError> {
        if !self.latitude_deg.is_finite()
            || !(-90.0..=90.0).contains(&self.latitude_deg)
            || !self.longitude_deg.is_finite()
            || !(-180.0..=180.0).contains(&self.longitude_deg)
            || !self.ellipsoidal_height_m.is_finite()
            || self.ellipsoidal_height_m.abs() > 1e9
        {
            return Err(GeoError);
        }
        Ok(())
    }
    /// Canonical ECEF metres, suitable for the existing retained `GlobeRenderer`.
    /// # Errors
    /// Rejects invalid positions; no silent coordinate clamping.
    pub fn ecef_m(self) -> Result<[f64; 3], GeoError> {
        self.validate()?;
        Ok(geodetic_to_ecef(
            self.latitude_deg,
            self.longitude_deg,
            self.ellipsoidal_height_m,
        ))
    }
}

/// Stable tangent frame. ENU order is exactly East, North, Up, in metres.
/// Hosts explicitly adapt that order to Bevy/LÖVR/wGPU axes at their boundary.
#[derive(Debug, Clone, Copy)]
pub struct LocalFrame {
    origin_ecef_m: [f64; 3],
    basis: [[f64; 3]; 3],
}
impl LocalFrame {
    /// Build one reusable WGS84 local frame, including antimeridian/polar sites.
    /// # Errors
    /// Rejects invalid origins.
    pub fn new(origin: Position) -> Result<Self, GeoError> {
        let origin_ecef_m = origin.ecef_m()?;
        let (sp, cp) = origin.latitude_deg.to_radians().sin_cos();
        let (sl, cl) = origin.longitude_deg.to_radians().sin_cos();
        Ok(Self {
            origin_ecef_m,
            basis: [
                [-sl, cl, 0.0],
                [-sp * cl, -sp * sl, cp],
                [cp * cl, cp * sl, sp],
            ],
        })
    }
    /// Convert with ECEF subtraction in f64 before rotating into the tangent frame.
    /// # Errors
    /// Rejects invalid source coordinates.
    pub fn enu_m(self, position: Position) -> Result<[f64; 3], GeoError> {
        let ecef = position.ecef_m()?;
        let delta = std::array::from_fn::<_, 3, _>(|i| ecef[i] - self.origin_ecef_m[i]);
        Ok(self
            .basis
            .map(|row| row.iter().zip(delta).map(|(a, b)| a * b).sum()))
    }
}

/// A bounded latitude/longitude window, optionally crossing the antimeridian.
/// This is a linear geographic chart/table layout, NOT Web Mercator or ENU.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extent {
    /// Southern latitude edge.
    pub south_deg: f64,
    /// Northern latitude edge.
    pub north_deg: f64,
    /// Western longitude edge; east < west explicitly crosses the date line.
    pub west_deg: f64,
    /// Eastern longitude edge; -180..180 denotes a full-world window.
    pub east_deg: f64,
}
impl Extent {
    fn span(self) -> f64 {
        let delta = self.east_deg - self.west_deg;
        if delta >= 360.0 {
            360.0
        } else {
            delta.rem_euclid(360.0)
        }
    }
    /// Validate before projecting or drawing a complete batch.
    /// # Errors
    /// Rejects nonfinite, empty or invalid windows.
    pub fn validate(self) -> Result<(), GeoError> {
        if ![self.south_deg, self.north_deg, self.west_deg, self.east_deg]
            .iter()
            .all(|v| v.is_finite())
            || self.south_deg < -90.0
            || self.north_deg > 90.0
            || self.north_deg <= self.south_deg
            || !(-180.0..=180.0).contains(&self.west_deg)
            || !(-180.0..=180.0).contains(&self.east_deg)
            || self.span() <= 0.0
        {
            return Err(GeoError);
        }
        Ok(())
    }
    /// Normalized X eastward, Y southward; outside positions remain outside.
    /// No height is needed for this two-dimensional layout.
    /// # Errors
    /// Rejects invalid extent or angles.
    pub fn normalized(self, latitude_deg: f64, longitude_deg: f64) -> Result<[f64; 2], GeoError> {
        self.validate()?;
        if !latitude_deg.is_finite()
            || !(-90.0..=90.0).contains(&latitude_deg)
            || !longitude_deg.is_finite()
            || !(-180.0..=180.0).contains(&longitude_deg)
        {
            return Err(GeoError);
        }
        let span = self.span();
        let x = if span >= 360.0 {
            (longitude_deg - self.west_deg) / span
        } else {
            let center = self.west_deg + span * 0.5;
            0.5 + ((longitude_deg - center + 180.0).rem_euclid(360.0) - 180.0) / span
        };
        let y = (self.north_deg - latitude_deg) / (self.north_deg - self.south_deg);
        if !x.is_finite() || !y.is_finite() {
            return Err(GeoError);
        }
        Ok([x, y])
    }
    /// Table-local X/Z (east/right, south/forward), centered at zero.
    /// Preserves the linear layout used by existing baked Holodeck terrain.
    /// # Errors
    /// Rejects invalid coordinates or nonpositive/nonfinite dimensions.
    pub fn table_xz(
        self,
        latitude_deg: f64,
        longitude_deg: f64,
        width: f64,
        depth: f64,
    ) -> Result<[f64; 2], GeoError> {
        if !width.is_finite() || !depth.is_finite() || width <= 0.0 || depth <= 0.0 {
            return Err(GeoError);
        }
        let [x, y] = self.normalized(latitude_deg, longitude_deg)?;
        let result = [(x - 0.5) * width, (y - 0.5) * depth];
        if !result.iter().all(|v| v.is_finite()) {
            return Err(GeoError);
        }
        Ok(result)
    }
}

/// Source identity, freshness and semantic category stay with the provider.
#[derive(Debug, Clone, Copy)]
pub struct Marker {
    /// Observed latitude, not an extrapolated track.
    pub latitude_deg: f64,
    /// Observed longitude.
    pub longitude_deg: f64,
    /// Host-selected semantic color; never a baked palette.
    pub role: ThemeRole,
}
/// Retain the returned instanced primitives using the existing GFX GPU renderer.
/// Hidden/outside markers are culled; selection uses the exact same projection.
#[derive(Debug, Clone, Copy)]
pub struct MarkerView {
    /// Explicit linear geographic window.
    pub extent: Extent,
    /// Caller-owned widget frame in logical pixels.
    pub bounds: Rect,
    /// Pixel radius independent of geographic units.
    pub radius_px: f32,
}
impl MarkerView {
    fn validate(self, markers: &[Marker]) -> Result<(), GeoError> {
        self.extent.validate()?;
        if markers.len() > 4096
            || !self.bounds.is_valid()
            || !self.bounds.right().is_finite()
            || !self.bounds.bottom().is_finite()
            || self.bounds.width > 16384.0
            || self.bounds.height > 16384.0
            || !self.radius_px.is_finite()
            || !(1.0..=64.0).contains(&self.radius_px)
            || self.bounds.width < self.radius_px * 2.0
            || self.bounds.height < self.radius_px * 2.0
        {
            return Err(GeoError);
        }
        for marker in markers {
            self.extent
                .normalized(marker.latitude_deg, marker.longitude_deg)?;
        }
        Ok(())
    }
    fn screen(self, marker: Marker) -> Result<Option<[f32; 2]>, GeoError> {
        let [x, y] = self
            .extent
            .normalized(marker.latitude_deg, marker.longitude_deg)?;
        if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
            return Ok(None);
        }
        #[allow(clippy::cast_possible_truncation)]
        let point = [
            self.bounds.x + x as f32 * self.bounds.width,
            self.bounds.y + y as f32 * self.bounds.height,
        ];
        // Painting and picking both exclude partial glyphs at the frame edge.
        let margin = self.radius_px + 0.75;
        if point[0] - margin < self.bounds.x
            || point[0] + margin > self.bounds.right()
            || point[1] - margin < self.bounds.y
            || point[1] + margin > self.bounds.bottom()
        {
            return Ok(None);
        }
        Ok(Some(point))
    }
    /// Append bounded diamond outlines using existing GPU line primitives.
    /// # Errors
    /// Invalid input leaves the caller's scene untouched.
    pub fn append(self, scene: &mut Scene, markers: &[Marker]) -> Result<(), GeoError> {
        self.validate(markers)?;
        for marker in markers {
            if let Some([x, y]) = self.screen(*marker)? {
                let r = self.radius_px;
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
    /// Return the nearest rendered marker's source-array index; ties retain order.
    /// Callers map that index to their stable feature ID, never a fabricated identity.
    /// # Errors
    /// Rejects invalid input, pointers and pick radius.
    pub fn pick(
        self,
        markers: &[Marker],
        pointer: [f32; 2],
        radius_px: f32,
    ) -> Result<Option<usize>, GeoError> {
        self.validate(markers)?;
        if !pointer.iter().all(|v| v.is_finite())
            || !radius_px.is_finite()
            || !(1.0..=128.0).contains(&radius_px)
        {
            return Err(GeoError);
        }
        if pointer[0] < self.bounds.x
            || pointer[0] > self.bounds.right()
            || pointer[1] < self.bounds.y
            || pointer[1] > self.bounds.bottom()
        {
            return Ok(None);
        }
        let mut nearest = None;
        let mut distance = radius_px * radius_px;
        for (index, marker) in markers.iter().enumerate() {
            if let Some([x, y]) = self.screen(*marker)? {
                let squared = (x - pointer[0]).powi(2) + (y - pointer[1]).powi(2);
                if squared <= distance && (nearest.is_none() || squared < distance) {
                    nearest = Some(index);
                    distance = squared;
                }
            }
        }
        Ok(nearest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn position(lat: f64, lon: f64, height: f64) -> Position {
        Position {
            latitude_deg: lat,
            longitude_deg: lon,
            ellipsoidal_height_m: height,
        }
    }
    #[test]
    fn enu_uses_canonical_axes_and_retains_submetre_precision() {
        let origin = position(47.6, -122.3, 0.0);
        let frame = LocalFrame::new(origin).unwrap();
        assert_eq!(frame.enu_m(origin).unwrap(), [0.0; 3]);
        let up = frame.enu_m(position(47.6, -122.3, 0.01)).unwrap();
        assert!(up[0].abs() < 1e-8 && up[1].abs() < 1e-8 && (up[2] - 0.01).abs() < 1e-8);
        assert!(frame.enu_m(position(47.60001, -122.3, 0.0)).unwrap()[1] > 1.0);
        assert!(frame.enu_m(position(47.6, -122.29999, 0.0)).unwrap()[0] > 0.7);
    }
    #[test]
    fn matches_published_proj_topocentric_reference() {
        // PROJ's geographic -> cartesian -> topocentric documented example.
        let frame = LocalFrame::new(position(55.0, 5.0, 200.0)).unwrap();
        let actual = frame.enu_m(position(53.80939444, 2.12955, 73.0)).unwrap();
        for (value, expected) in actual
            .into_iter()
            .zip([-189013.869, -128642.040, -4220.171])
        {
            assert!((value - expected).abs() < 0.01);
        }
    }
    #[test]
    fn date_line_and_poles_do_not_create_global_jumps() {
        let frame = LocalFrame::new(position(0.0, 179.999, 0.0)).unwrap();
        let enu = frame.enu_m(position(0.0, -179.999, 0.0)).unwrap();
        assert!((enu[0] - 222.639).abs() < 0.01);
        assert!(LocalFrame::new(position(90.0, 0.0, 0.0))
            .unwrap()
            .enu_m(position(89.999, 0.0, 0.0))
            .unwrap()
            .iter()
            .all(|v| v.is_finite()));
        let extent = Extent {
            south_deg: -10.0,
            north_deg: 10.0,
            west_deg: 170.0,
            east_deg: -170.0,
        };
        assert_eq!(extent.normalized(0.0, 180.0).unwrap(), [0.5, 0.5]);
        assert_eq!(extent.normalized(0.0, -180.0).unwrap(), [0.5, 0.5]);
        assert_eq!(extent.table_xz(0.0, 180.0, 2.0, 1.0).unwrap(), [0.0, 0.0]);
    }
    #[test]
    fn gpu_marker_geometry_and_picking_share_a_frame_and_fail_atomically() {
        let view = MarkerView {
            extent: Extent {
                south_deg: 0.0,
                north_deg: 2.0,
                west_deg: 0.0,
                east_deg: 2.0,
            },
            bounds: Rect::new(12.0, 24.0, 200.0, 100.0),
            radius_px: 5.0,
        };
        let marker = Marker {
            latitude_deg: 1.0,
            longitude_deg: 1.0,
            role: ThemeRole::Primary,
        };
        let mut scene = Scene::new();
        view.append(&mut scene, &[marker]).unwrap();
        assert_eq!(scene.len(), 4);
        assert_eq!(view.pick(&[marker], [112.0, 74.0], 10.0).unwrap(), Some(0));
        assert_eq!(view.pick(&[marker], [0.0, 0.0], 10.0).unwrap(), None);
        let invalid = Marker {
            latitude_deg: f64::NAN,
            ..marker
        };
        assert!(view.append(&mut scene, &[marker, invalid]).is_err());
        assert_eq!(scene.len(), 4);
        assert!(position(91.0, 0.0, 0.0).ecef_m().is_err());
    }
}
