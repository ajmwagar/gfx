use thiserror::Error;

/// WGS84 semi-major axis in metres.
pub const WGS84_SEMI_MAJOR_M: f64 = 6_378_137.0;
/// WGS84 semi-minor axis in metres.
pub const WGS84_SEMI_MINOR_M: f64 = 6_356_752.314_245;

/// Failure to construct portable spatial geometry.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum SpatialError {
    /// Segment counts must be non-zero and fit in a `u32` index buffer.
    #[error("ellipsoid segment counts are invalid")]
    InvalidSegments,
}

/// One canonical ellipsoid vertex before camera-relative conversion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EllipsoidVertex {
    /// Earth-centred, Earth-fixed position in metres.
    pub ecef_m: [f64; 3],
    /// Latitude and longitude in degrees.
    pub coordinates_degrees: [f32; 2],
}

/// Indexed ellipsoid mesh retained in double-precision world coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct EllipsoidMesh {
    /// Canonical vertices.
    pub vertices: Vec<EllipsoidVertex>,
    /// Counter-clockwise triangle indices.
    pub indices: Vec<u32>,
}

/// Converts geodetic WGS84 coordinates to Earth-centred, Earth-fixed metres.
///
/// This implements EPSG Guidance Note 7-2 section 2.2.1.
pub fn geodetic_to_ecef(
    latitude_degrees: f64,
    longitude_degrees: f64,
    altitude_m: f64,
) -> [f64; 3] {
    let latitude = latitude_degrees.to_radians();
    let longitude = longitude_degrees.to_radians();
    let eccentricity_squared =
        1.0 - (WGS84_SEMI_MINOR_M * WGS84_SEMI_MINOR_M) / (WGS84_SEMI_MAJOR_M * WGS84_SEMI_MAJOR_M);
    let prime_vertical =
        WGS84_SEMI_MAJOR_M / (1.0 - eccentricity_squared * latitude.sin().powi(2)).sqrt();
    [
        (prime_vertical + altitude_m) * latitude.cos() * longitude.cos(),
        (prime_vertical + altitude_m) * latitude.cos() * longitude.sin(),
        (prime_vertical * (1.0 - eccentricity_squared) + altitude_m) * latitude.sin(),
    ]
}

/// Converts a double-precision world position into camera-relative `f32` coordinates.
pub fn relative_to_origin_f32(position: [f64; 3], origin: [f64; 3]) -> [f32; 3] {
    // This conversion is the deliberate precision boundary: subtracting the
    // nearby camera origin in f64 keeps the remaining GPU coordinates precise.
    #[allow(clippy::cast_possible_truncation)]
    [
        (position[0] - origin[0]) as f32,
        (position[1] - origin[1]) as f32,
        (position[2] - origin[2]) as f32,
    ]
}

/// Generates an indexed WGS84 ellipsoid with a duplicated longitude seam.
///
/// # Errors
///
/// Returns [`SpatialError::InvalidSegments`] when either segment count is zero
/// or the resulting mesh cannot be indexed by `u32`.
pub fn wgs84_ellipsoid_mesh(
    latitude_segments: u32,
    longitude_segments: u32,
) -> Result<EllipsoidMesh, SpatialError> {
    let vertex_count = latitude_segments
        .checked_add(1)
        .and_then(|latitude| {
            longitude_segments
                .checked_add(1)
                .and_then(|longitude| latitude.checked_mul(longitude))
        })
        .filter(|_| latitude_segments > 0 && longitude_segments > 0)
        .ok_or(SpatialError::InvalidSegments)?;
    let index_count = latitude_segments
        .checked_mul(longitude_segments)
        .and_then(|quads| quads.checked_mul(6))
        .ok_or(SpatialError::InvalidSegments)?;
    let mut vertices = Vec::with_capacity(vertex_count as usize);
    for latitude_index in 0..=latitude_segments {
        let latitude = -90.0 + 180.0 * f64::from(latitude_index) / f64::from(latitude_segments);
        for longitude_index in 0..=longitude_segments {
            let longitude =
                -180.0 + 360.0 * f64::from(longitude_index) / f64::from(longitude_segments);
            vertices.push(EllipsoidVertex {
                ecef_m: geodetic_to_ecef(latitude, longitude, 0.0),
                // Tessellation coordinates are bounded to latitude/longitude
                // ranges and are used only for raster UV generation.
                #[allow(clippy::cast_possible_truncation)]
                coordinates_degrees: [latitude as f32, longitude as f32],
            });
        }
    }
    let mut indices = Vec::with_capacity(index_count as usize);
    let stride = longitude_segments + 1;
    for latitude in 0..latitude_segments {
        for longitude in 0..longitude_segments {
            let northwest = latitude * stride + longitude;
            let southwest = (latitude + 1) * stride + longitude;
            indices.extend_from_slice(&[
                northwest,
                northwest + 1,
                southwest,
                northwest + 1,
                southwest + 1,
                southwest,
            ]);
        }
    }
    Ok(EllipsoidMesh { vertices, indices })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ellipsoid_is_closed_and_indexed() {
        let mesh = wgs84_ellipsoid_mesh(8, 16).unwrap();
        assert_eq!(mesh.vertices.len(), 9 * 17);
        assert_eq!(mesh.indices.len(), 8 * 16 * 6);
        assert!(mesh
            .indices
            .iter()
            .all(|index| (*index as usize) < mesh.vertices.len()));
        for latitude in 0..=8 {
            let west = mesh.vertices[latitude * 17].ecef_m;
            let east = mesh.vertices[latitude * 17 + 16].ecef_m;
            for axis in 0..3 {
                assert!((west[axis] - east[axis]).abs() < 0.000_001);
            }
        }
    }

    #[test]
    fn camera_relative_conversion_preserves_local_precision() {
        let origin = geodetic_to_ecef(47.6, -122.3, 0.0);
        let elevated = geodetic_to_ecef(47.6, -122.3, 25.0);
        let relative = relative_to_origin_f32(elevated, origin);
        let distance = relative
            .into_iter()
            .map(|axis| axis * axis)
            .sum::<f32>()
            .sqrt();
        assert!((distance - 25.0).abs() < 0.01);
    }

    #[test]
    fn rejects_degenerate_meshes() {
        assert_eq!(
            wgs84_ellipsoid_mesh(0, 8).unwrap_err(),
            SpatialError::InvalidSegments,
        );
    }
}
