//! Bounded count mosaics and structural layer glyphs. Hosts own labels and data.
// Counts remain exact integers; only bounded screen coordinates/final fill fractions use floats.
#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
use crate::{Primitive, Rect, Scene, ThemeRole};

/// Maximum cells in one mosaic.
pub const MAX_CELLS: usize = 4096;
/// Maximum independent count categories.
pub const MAX_CATEGORIES: usize = 16;

/// A count category colored by the host's semantic palette.
#[derive(Clone, Copy, Debug)]
pub struct CountBand {
    /// Exact count, retained as an integer until the final partial-cell fill.
    pub count: u64,
    /// Semantic fill role.
    pub role: ThemeRole,
}

/// Invalid input; no scene primitives are appended on failure.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid data view: {0}")]
pub struct DataViewError(pub &'static str);

/// Exact mosaic scale metadata, suitable for a host-provided legend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MosaicStats {
    /// Total represented items.
    pub count: u64,
    /// Count represented by a completely filled cell.
    pub items_per_cell: u64,
    /// Occupied cell slots, including partial cells at category boundaries.
    pub occupied_cells: usize,
}

/// Choose a common scale for multiple mosaics with at most `categories` bands.
/// Each band receives its own fractional final cell; reserve their rounding slots.
///
/// # Errors
/// Rejects unsupported capacity or category counts.
pub fn common_scale(
    max_count: u64,
    capacity: usize,
    categories: usize,
) -> Result<u64, DataViewError> {
    if capacity > MAX_CELLS
        || categories == 0
        || categories > MAX_CATEGORIES
        || capacity <= categories
    {
        return Err(DataViewError("capacity/categories"));
    }
    Ok(max_count.div_ceil((capacity - categories) as u64).max(1))
}

/// Append a count mosaic. Equal scales make the colored areas comparable.
/// Partial cells use proportional width; empty capacity remains visibly recessed.
/// Labels, units, identities and admission status belong to the caller.
///
/// # Errors
/// Rejects invalid bounds, scales, overflowing counts and insufficient cell capacity.
pub fn append_mosaic(
    scene: &mut Scene,
    bounds: Rect,
    columns: usize,
    rows: usize,
    items_per_cell: u64,
    bands: &[CountBand],
) -> Result<MosaicStats, DataViewError> {
    let capacity = columns
        .checked_mul(rows)
        .ok_or(DataViewError("capacity overflow"))?;
    if !bounds.is_valid()
        || bounds.width <= 0.0
        || bounds.height <= 0.0
        || columns == 0
        || rows == 0
        || capacity > MAX_CELLS
        || items_per_cell == 0
        || bands.is_empty()
        || bands.len() > MAX_CATEGORIES
    {
        return Err(DataViewError("bounds/scale/bands"));
    }
    let count = bands
        .iter()
        .try_fold(0u64, |sum, band| sum.checked_add(band.count))
        .ok_or(DataViewError("count overflow"))?;
    let occupied = bands
        .iter()
        .try_fold(0u64, |sum, band| {
            sum.checked_add(band.count.div_ceil(items_per_cell))
        })
        .ok_or(DataViewError("cell overflow"))?;
    if occupied > capacity as u64 {
        return Err(DataViewError("counts exceed capacity"));
    }
    let cw = bounds.width / columns as f32;
    let ch = bounds.height / rows as f32;
    if cw < 2.0 || ch < 2.0 {
        return Err(DataViewError("cells smaller than two pixels"));
    }
    let cell = |index: usize| {
        Rect::new(
            bounds.x + (index % columns) as f32 * cw,
            bounds.y + (index / columns) as f32 * ch,
            cw - 1.0,
            ch - 1.0,
        )
    };
    for i in 0..capacity {
        scene.push(
            Primitive::rounded_rect(cell(i), 0.0, ThemeRole::SurfaceRecessed)
                .with_outline(ThemeRole::Outline, 0.0),
        );
    }
    let mut index = 0;
    for band in bands {
        let mut remaining = band.count;
        while remaining > 0 {
            let represented = remaining.min(items_per_cell);
            let mut rect = cell(index);
            rect.width *= (represented as f64 / items_per_cell as f64) as f32;
            scene.push(
                Primitive::rounded_rect(rect, 0.0, band.role).with_outline(ThemeRole::Outline, 0.0),
            );
            index += 1;
            remaining -= represented;
        }
    }
    Ok(MosaicStats {
        count,
        items_per_cell,
        occupied_cells: index,
    })
}

/// Append sequential structural layer blocks with representative glyphs per block.
/// Edges express adjacency only, never weights, activations or dense connectivity.
///
/// # Errors
/// Rejects invalid bounds, mismatched layer roles or excessive/undersized glyphs.
pub fn append_layers(
    scene: &mut Scene,
    bounds: Rect,
    glyphs: &[usize],
    roles: &[ThemeRole],
) -> Result<(), DataViewError> {
    if !bounds.is_valid()
        || bounds.width <= 0.0
        || bounds.height <= 0.0
        || glyphs.is_empty()
        || glyphs.len() > 32
        || roles.len() != glyphs.len()
        || glyphs.iter().any(|n| *n == 0 || *n > 16)
    {
        return Err(DataViewError("layer bounds/count"));
    }
    let step = bounds.width / glyphs.len() as f32;
    let d = (step * 0.3)
        .min(bounds.height / (glyphs.iter().copied().max().unwrap_or(0) + 1) as f32)
        .min(24.0);
    if d < 2.0 {
        return Err(DataViewError("layer glyphs smaller than two pixels"));
    }
    for i in 1..glyphs.len() {
        scene.push(Primitive::line(
            [
                bounds.x + (i as f32 - 0.5) * step,
                bounds.y + bounds.height * 0.5,
            ],
            [
                bounds.x + (i as f32 + 0.5) * step,
                bounds.y + bounds.height * 0.5,
            ],
            2.0,
            ThemeRole::Outline,
        ));
    }
    for (i, (&n, &role)) in glyphs.iter().zip(roles).enumerate() {
        let x = bounds.x + (i as f32 + 0.5) * step;
        scene.push(Primitive::rounded_rect(
            Rect::new(x - step * 0.4, bounds.y, step * 0.8, bounds.height),
            8.0,
            ThemeRole::Surface,
        ));
        for j in 0..n {
            let y = bounds.y + (j as f32 + 0.5) * bounds.height / n as f32;
            scene.push(Primitive::disc(
                Rect::new(x - d * 0.5, y - d * 0.5, d, d),
                role,
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn huge_integer_counts_do_not_overflow_scale() {
        let scale = common_scale(u64::MAX, 64, 2).unwrap();
        let mut scene = Scene::default();
        let stats = append_mosaic(
            &mut scene,
            Rect::new(0.0, 0.0, 160.0, 160.0),
            8,
            8,
            scale,
            &[CountBand {
                count: u64::MAX,
                role: ThemeRole::Primary,
            }],
        )
        .unwrap();
        assert_eq!(stats.count, u64::MAX);
        assert!(stats.occupied_cells <= 64);
    }
    #[test]
    fn split_partial_cells_preserve_exact_count_and_area() {
        let mut scene = Scene::default();
        let stats = append_mosaic(
            &mut scene,
            Rect::new(0.0, 0.0, 40.0, 40.0),
            4,
            4,
            10,
            &[
                CountBand {
                    count: 11,
                    role: ThemeRole::Primary,
                },
                CountBand {
                    count: 9,
                    role: ThemeRole::Success,
                },
            ],
        )
        .unwrap();
        assert_eq!(stats.count, 20);
        assert_eq!(stats.occupied_cells, 3);
    }
    #[test]
    fn refusal_is_atomic() {
        let mut scene = Scene::default();
        assert!(append_mosaic(
            &mut scene,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            1,
            1,
            1,
            &[CountBand {
                count: 2,
                role: ThemeRole::Primary
            }]
        )
        .is_err());
        assert!(scene.is_empty());
        assert!(append_layers(
            &mut scene,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            &[0],
            &[ThemeRole::Primary]
        )
        .is_err());
        assert!(scene.is_empty());
    }
}
