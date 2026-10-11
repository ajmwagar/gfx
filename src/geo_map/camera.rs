//! Bounded regional camera math. A camera never modifies owner observations.
use super::MapError;
use crate::geo::Extent;

/// Retained display offset and zoom relative to the source viewport.
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    zoom: f64,
    east: f64,
    north: f64,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            east: 0.0,
            north: 0.0,
        }
    }
}
impl Camera {
    /// Current regional view. Dateline-crossing/global sources are not supported.
    /// # Errors
    /// Rejects invalid or non-regional extents.
    pub fn extent(&self, source: Extent) -> Result<Extent, MapError> {
        source.validate().map_err(|_| MapError)?;
        if source.east_deg <= source.west_deg {
            return Err(MapError);
        }
        let width = ((source.east_deg - source.west_deg) / self.zoom).min(359.0);
        let height = ((source.north_deg - source.south_deg) / self.zoom).min(170.0);
        let x = ((source.west_deg + source.east_deg) * 0.5 + self.east)
            .clamp(-180.0 + width * 0.5, 180.0 - width * 0.5);
        let y = ((source.south_deg + source.north_deg) * 0.5 + self.north)
            .clamp(-85.0 + height * 0.5, 85.0 - height * 0.5);
        Ok(Extent {
            west_deg: x - width * 0.5,
            east_deg: x + width * 0.5,
            south_deg: y - height * 0.5,
            north_deg: y + height * 0.5,
        })
    }
    /// Drag displacement as a fraction of plot width/height.
    /// # Errors
    /// Rejects nonfinite input without changing state.
    pub fn pan(&mut self, source: Extent, dx: f64, dy: f64) -> Result<(), MapError> {
        if !dx.is_finite() || !dy.is_finite() {
            return Err(MapError);
        }
        let view = self.extent(source)?;
        self.east -= dx * (view.east_deg - view.west_deg);
        self.north += dy * (view.north_deg - view.south_deg);
        self.clamp(source)
    }
    /// Pointer-anchored zoom, bounded to 0.5–16×. Anchor is normalized plot XY.
    /// # Errors
    /// Rejects invalid factors/anchors without changing state.
    pub fn zoom_at(
        &mut self,
        source: Extent,
        factor: f64,
        anchor: [f64; 2],
    ) -> Result<(), MapError> {
        if !factor.is_finite()
            || factor <= 0.0
            || anchor
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(MapError);
        }
        let before = self.extent(source)?;
        let x = before.west_deg + anchor[0] * (before.east_deg - before.west_deg);
        let y = before.north_deg - anchor[1] * (before.north_deg - before.south_deg);
        self.zoom = (self.zoom * factor).clamp(0.5, 16.0);
        let after = self.extent(source)?;
        self.east += x - (after.west_deg + anchor[0] * (after.east_deg - after.west_deg));
        self.north += y - (after.north_deg - anchor[1] * (after.north_deg - after.south_deg));
        self.clamp(source)
    }
    fn clamp(&mut self, source: Extent) -> Result<(), MapError> {
        let view = self.extent(source)?;
        self.east = (view.west_deg + view.east_deg - source.west_deg - source.east_deg) * 0.5;
        self.north = (view.north_deg + view.south_deg - source.north_deg - source.south_deg) * 0.5;
        Ok(())
    }
}
