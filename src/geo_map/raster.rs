//! Geographic placement of decoded XYZ raster tiles on a linear-latitude map.
//! No fetching, credentials, provider selection or image decoding lives here.
use super::MapError;
use crate::{geo::Extent, Rect};
use serde::{Deserialize, Serialize};

/// Verified raster asset reference. The host resolves a digest locally; this
/// contract never accepts arbitrary network URLs or provider credentials.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    /// XYZ zoom, at most 20.
    pub zoom: u8,
    /// Horizontal XYZ index.
    pub x: u32,
    /// North-to-south XYZ index.
    pub y: u32,
    /// Actual decoded PNG pixel width, exactly 256 for this bounded layer.
    pub width: u32,
    /// Actual decoded PNG pixel height, exactly 256 for this bounded layer.
    pub height: u32,
    /// Lowercase SHA256 of the compressed PNG bytes, checked by the asset host.
    pub sha256: String,
    /// Exact compressed PNG byte size, checked before decoding by the host.
    pub byte_size: u64,
}
impl Asset {
    /// Placement metadata derived from this asset, never separately configured.
    pub fn tile(&self) -> Tile {
        Tile {
            zoom: self.zoom,
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }
}
/// Optional current-view basemap, retaining its provider's visible attribution.
/// Image bytes remain outside telemetry and are delivered by the asset owner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Basemap {
    /// Required visible attribution, at most 256 Unicode characters.
    pub attribution: String,
    /// At most 64 unique XYZ assets; aggregate decoded pixels remain bounded.
    pub tiles: Vec<Asset>,
}
impl Basemap {
    /// Validate all references before opening or decoding any image bytes.
    /// # Errors
    /// Rejects invalid IDs, dimensions, digests, text or resource limits.
    pub fn validate(&self) -> Result<(), MapError> {
        if self.attribution.trim().is_empty()
            || self.attribution.chars().count() > 256
            || self.attribution.chars().any(char::is_control)
            || self.tiles.is_empty()
            || self.tiles.len() > 64
        {
            return Err(MapError);
        }
        let mut bytes = 0u64;
        for (index, asset) in self.tiles.iter().enumerate() {
            if !asset.tile().valid()
                || asset.width != 256
                || asset.height != 256
                || asset.sha256.len() != 64
                || !asset
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || !(33..=1_048_576).contains(&asset.byte_size)
                || self.tiles[..index]
                    .iter()
                    .any(|other| (other.zoom, other.x, other.y) == (asset.zoom, asset.x, asset.y))
            {
                return Err(MapError);
            }
            bytes += asset.byte_size;
            if bytes > 16 * 1024 * 1024 {
                return Err(MapError);
            }
        }
        Ok(())
    }
}

/// One validated Web Mercator XYZ image, whose pixels remain caller-owned.
#[derive(Debug, Clone, Copy)]
pub struct Tile {
    /// XYZ zoom, bounded to 20.
    pub zoom: u8,
    /// Horizontal XYZ index.
    pub x: u32,
    /// Vertical XYZ index, north to south.
    pub y: u32,
    /// Decoded pixel width, at most 1024.
    pub width: u32,
    /// Decoded pixel height, at most 1024.
    pub height: u32,
}
impl Tile {
    fn valid(self) -> bool {
        self.zoom <= 20
            && self.x < (1u32 << self.zoom)
            && self.y < (1u32 << self.zoom)
            && (1..=1024).contains(&self.width)
            && (1..=1024).contains(&self.height)
    }
}
/// A row-reprojected texture mesh. Clip drawing to the caller's plot rectangle.
/// Each source pixel-row edge has two vertices; implicit UVs run (0..1,0..1).
/// Android can pass this directly to `Canvas.drawBitmapMesh` with mesh width 1.
#[derive(Debug, Clone)]
pub struct Placement {
    /// Index of the image in the supplied tile list.
    pub tile_index: usize,
    /// Number of horizontal mesh cells; always one because longitude is linear.
    pub columns: u32,
    /// Number of vertical mesh cells, equal to the decoded pixel height.
    pub rows: u32,
    /// Row-major left/right positions including both terminal pixel-row edges.
    pub vertices: Vec<[f32; 2]>,
}

/// Place at most 64 decoded tiles, with at most 4,194,304 aggregate pixels.
/// The optional geographic wrap produces at most three placements per tile.
/// Source row edges are reprojected from Mercator into true latitude; a tile
/// is never stretched linearly between only its north/south corners.
/// # Errors
/// Invalid geography, indices, image dimensions or budgets reject the whole plan.
#[allow(clippy::cast_possible_truncation)]
pub fn plan(extent: Extent, plot: Rect, tiles: &[Tile]) -> Result<Vec<Placement>, MapError> {
    extent.validate().map_err(|_| MapError)?;
    if !plot.is_valid()
        || !plot.right().is_finite()
        || !plot.bottom().is_finite()
        || plot.width > 16384.0
        || plot.height > 16384.0
        || tiles.len() > 64
    {
        return Err(MapError);
    }
    let mut pixels = 0u64;
    for tile in tiles {
        if !tile.valid() {
            return Err(MapError);
        }
        pixels += u64::from(tile.width) * u64::from(tile.height);
        if pixels > 4_194_304 {
            return Err(MapError);
        }
    }
    let delta = extent.east_deg - extent.west_deg;
    let longitude_span = if delta >= 360.0 {
        360.0
    } else {
        delta.rem_euclid(360.0)
    };
    let latitude_span = extent.north_deg - extent.south_deg;
    let mut result = Vec::new();
    for (tile_index, tile) in tiles.iter().enumerate() {
        let n = f64::from(1u32 << tile.zoom);
        let west = f64::from(tile.x) / n * 360.0 - 180.0;
        let span = 360.0 / n;
        let north = latitude(f64::from(tile.y) / n);
        let south = latitude(f64::from(tile.y + 1) / n);
        if north <= extent.south_deg || south >= extent.north_deg {
            continue;
        }
        for shift in [-360.0, 0.0, 360.0] {
            let relative_west = west + shift - extent.west_deg;
            if relative_west + span <= 0.0 || relative_west >= longitude_span {
                continue;
            }
            let left = f64::from(plot.x) + relative_west / longitude_span * f64::from(plot.width);
            let right = left + span / longitude_span * f64::from(plot.width);
            let mut vertices = Vec::with_capacity((tile.height as usize + 1) * 2);
            for row in 0..=tile.height {
                let v = f64::from(row) / f64::from(tile.height);
                let lat = latitude((f64::from(tile.y) + v) / n);
                let y = f64::from(plot.y)
                    + (extent.north_deg - lat) / latitude_span * f64::from(plot.height);
                // Unclipped edges may sit outside the viewport. Reject values
                // that cannot be represented by the renderer, never cast to infinity.
                if ![left, right, y]
                    .into_iter()
                    .all(|value| value.is_finite() && value.abs() <= f64::from(f32::MAX))
                {
                    return Err(MapError);
                }
                vertices.push([left as f32, y as f32]);
                vertices.push([right as f32, y as f32]);
            }
            result.push(Placement {
                tile_index,
                columns: 1,
                rows: tile.height,
                vertices,
            });
        }
    }
    Ok(result)
}
fn latitude(mercator_y: f64) -> f64 {
    (std::f64::consts::PI * (1.0 - 2.0 * mercator_y))
        .sinh()
        .atan()
        .to_degrees()
}

/// Center-crop a regional extent to the plot's physical aspect ratio.
/// Longitude distance is corrected at the region's central latitude. All
/// overlays must use this same extent; independently fitting imagery is wrong.
/// # Errors
/// Rejects invalid bounds and polar regions without a usable regional scale.
#[allow(clippy::float_cmp)] // Exact full-world sentinel, not approximate geometry.
pub fn cropped_extent(extent: Extent, plot: Rect) -> Result<Extent, MapError> {
    extent.validate().map_err(|_| MapError)?;
    if !plot.is_valid() {
        return Err(MapError);
    }
    let longitude = (extent.east_deg - extent.west_deg).rem_euclid(360.0);
    let longitude = if longitude == 0.0 { 360.0 } else { longitude };
    let latitude = extent.north_deg - extent.south_deg;
    let center_lat = extent.north_deg.midpoint(extent.south_deg);
    let cos = center_lat.to_radians().cos();
    if cos <= 1e-6 {
        return Err(MapError);
    }
    let aspect = f64::from(plot.width) / f64::from(plot.height);
    let lat_span = latitude.min(longitude * cos / aspect);
    let lon_span = longitude.min(latitude * aspect / cos);
    let center_lon = extent.west_deg + longitude * 0.5;
    let wrap = |lon: f64| (lon + 180.0).rem_euclid(360.0) - 180.0;
    let result = Extent {
        south_deg: center_lat - lat_span * 0.5,
        north_deg: center_lat + lat_span * 0.5,
        west_deg: wrap(center_lon - lon_span * 0.5),
        east_deg: if lon_span == 360.0 {
            180.0
        } else {
            wrap(center_lon + lon_span * 0.5)
        },
    };
    result.validate().map_err(|_| MapError)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crop_preserves_regional_scale_for_wide_and_tall_windows() {
        let e = Extent {
            south_deg: 46.8,
            north_deg: 48.7,
            west_deg: -123.5,
            east_deg: -121.5,
        };
        for (w, h) in [(1200.0, 500.0), (500.0, 1200.0)] {
            let c = cropped_extent(e, Rect::new(0.0, 0.0, w, h)).unwrap();
            let scale = (c.east_deg - c.west_deg)
                * ((c.north_deg + c.south_deg) * 0.5).to_radians().cos()
                / (c.north_deg - c.south_deg);
            assert!((scale - f64::from(w) / f64::from(h)).abs() < 1e-10);
            assert!(c.west_deg >= e.west_deg && c.east_deg <= e.east_deg);
            assert!(c.south_deg >= e.south_deg && c.north_deg <= e.north_deg);
        }
    }
    #[test]
    fn basemap_is_bounded_digest_addressed_and_never_accepts_network_urls() {
        let asset = Asset {
            zoom: 9,
            x: 81,
            y: 178,
            width: 256,
            height: 256,
            sha256: "a".repeat(64),
            byte_size: 26028,
        };
        let mut basemap = Basemap {
            attribution: "© OpenStreetMap contributors".into(),
            tiles: vec![asset.clone()],
        };
        basemap.validate().unwrap();
        let mut json = serde_json::to_value(&basemap).unwrap();
        json["tiles"][0]["url"] = serde_json::json!("https://untrusted.example/tile.png");
        assert!(serde_json::from_value::<Basemap>(json).is_err());
        basemap.tiles.push(asset);
        assert!(basemap.validate().is_err());
        basemap.tiles.pop();
        basemap.tiles[0].sha256 = "../not-a-digest".into();
        assert!(basemap.validate().is_err());
        basemap.tiles[0].sha256 = "a".repeat(64);
        basemap.tiles[0].byte_size = 1_048_577;
        assert!(basemap.validate().is_err());
        basemap.tiles[0].byte_size = 26028;
        basemap.tiles[0].width = 1024;
        assert!(basemap.validate().is_err());
    }
    fn world() -> Extent {
        Extent {
            south_deg: -85.0,
            north_deg: 85.0,
            west_deg: -180.0,
            east_deg: 180.0,
        }
    }
    fn tile() -> Tile {
        Tile {
            zoom: 0,
            x: 0,
            y: 0,
            width: 256,
            height: 256,
        }
    }
    #[test]
    fn row_mesh_reprojects_mercator_not_stretched_corners() {
        let extent = Extent {
            south_deg: 0.0,
            north_deg: 85.0,
            west_deg: -180.0,
            east_deg: 180.0,
        };
        let mesh = plan(extent, Rect::new(0.0, 0.0, 360.0, 85.0), &[tile()]).unwrap();
        assert_eq!(mesh.len(), 1);
        let row = 64usize;
        let expected = 85.0 - latitude(0.25);
        assert!((f64::from(mesh[0].vertices[row * 2][1]) - expected).abs() < 1e-5);
        assert!((expected - 42.5).abs() > 10.0);
        assert_eq!(mesh[0].vertices.len(), 514);
    }
    #[test]
    fn longitude_wrap_and_source_row_order_match_map() {
        let extent = Extent {
            south_deg: -10.0,
            north_deg: 10.0,
            west_deg: 170.0,
            east_deg: -170.0,
        };
        let meshes = plan(extent, Rect::new(0.0, 0.0, 200.0, 100.0), &[tile()]).unwrap();
        assert_eq!(meshes.len(), 2);
        for mesh in meshes {
            assert!(mesh
                .vertices
                .windows(2)
                .step_by(2)
                .all(|pair| pair[0][0] < pair[1][0]));
            assert!(mesh
                .vertices
                .chunks(2)
                .map(|row| row[0][1])
                .collect::<Vec<_>>()
                .windows(2)
                .all(|pair| pair[0] < pair[1]));
        }
    }
    #[test]
    fn offscreen_invalid_and_memory_budget_are_explicit() {
        let extent = Extent {
            south_deg: 46.8,
            north_deg: 48.7,
            west_deg: -123.5,
            east_deg: -121.5,
        };
        let offscreen = Tile {
            zoom: 8,
            x: 0,
            y: 0,
            ..tile()
        };
        assert!(
            plan(extent, Rect::new(0.0, 0.0, 500.0, 400.0), &[offscreen])
                .unwrap()
                .is_empty()
        );
        assert!(plan(
            world(),
            Rect::new(0.0, 0.0, 500.0, 400.0),
            &[Tile { zoom: 21, ..tile() }]
        )
        .is_err());
        assert!(plan(
            world(),
            Rect::new(0.0, 0.0, 500.0, 400.0),
            &[Tile {
                width: 1024,
                height: 1024,
                ..tile()
            }; 5]
        )
        .is_err());
        assert!(plan(world(), Rect::new(0.0, 0.0, 500.0, 400.0), &[tile(); 65]).is_err());
    }
}
