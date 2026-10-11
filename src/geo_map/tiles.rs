//! View intent and verified asset descriptors, never provider URLs or credentials.
use super::{raster::Basemap, MapError};
use crate::geo::Extent;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// One bounded actively viewed regional viewport.
pub struct Request {
    /// Version 1 of this transport-independent contract.
    pub schema_version: u32,
    /// Local viewer generation, monotonically advanced on camera changes.
    pub generation: u64,
    /// Requested geographic coverage, independent of owner observations.
    pub extent: Extent,
    /// Surface sampling width; the shared Map::inner derives the tile plot.
    pub width: u32,
    /// Surface sampling height, coupled to the retained image dimensions.
    pub height: u32,
}
impl Request {
    /// Validate admission before planning/fetching any tiles.
    /// # Errors
    /// Rejects invalid generation, oversized or non-regional views.
    pub fn validate(&self) -> Result<(), MapError> {
        self.extent.validate().map_err(|_| MapError)?;
        if self.schema_version != 1
            || self.generation == 0
            || self.width < 240
            || self.height < 180
            || self.width > 4096
            || self.height > 4096
            || u64::from(self.width) * u64::from(self.height) > 4_194_304
            || self.extent.south_deg < -85.
            || self.extent.north_deg > 85.
            || self.extent.west_deg >= self.extent.east_deg
        {
            return Err(MapError);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Retained tile result for exactly one request. Partial results are explicit.
pub struct Response {
    /// Exact originating request; stale generations must not replace newer views.
    pub request: Request,
    /// None when no tile is available. Previous imagery remains usable.
    pub basemap: Option<Basemap>,
    /// Number of current-view tiles not yet available.
    pub pending: u32,
    /// Cache hits this cycle, at most 64.
    pub cached: u32,
    /// Network acquisitions this cycle, at most 16.
    pub fetched: u32,
    /// Bounded diagnostics with no credential-bearing URL/body text.
    pub errors: Vec<String>,
}
impl Response {
    /// Validate a result against the latest local view request.
    /// # Errors
    /// Rejects stale, malformed or oversized results before image publication.
    pub fn validate_for(&self, request: &Request) -> Result<(), MapError> {
        self.request.validate()?;
        if &self.request != request
            || self.pending > 64
            || self.cached > 64
            || self.fetched > 16
            || self.errors.len() > 8
            || self
                .errors
                .iter()
                .any(|e| e.len() > 256 || e.chars().any(char::is_control))
        {
            return Err(MapError);
        }
        if let Some(map) = &self.basemap {
            map.validate()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn response_is_bound_to_generation_extent_and_sampling_size() {
        let request = Request {
            schema_version: 1,
            generation: 1,
            extent: Extent {
                west_deg: -123.,
                east_deg: -121.,
                south_deg: 47.,
                north_deg: 49.,
            },
            width: 1408,
            height: 881,
        };
        let response = Response {
            request: request.clone(),
            basemap: None,
            pending: 1,
            cached: 0,
            fetched: 0,
            errors: vec![],
        };
        assert!(response.validate_for(&request).is_ok());
        let mut changed = request.clone();
        changed.generation += 1;
        assert!(response.validate_for(&changed).is_err());
        changed = request.clone();
        changed.width += 1;
        assert!(response.validate_for(&changed).is_err());
        changed = request.clone();
        changed.extent.east_deg += 0.1;
        assert!(response.validate_for(&changed).is_err());
        let mut malformed = response;
        malformed.errors.push("secret\nbody".into());
        assert!(malformed.validate_for(&request).is_err());
    }
}
