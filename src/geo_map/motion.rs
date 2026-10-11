//! Observation-driven geographic motion. Never predicts or extrapolates contacts.
use super::{Map, MapError, Path, Point};
use std::collections::{BTreeMap, VecDeque};

/// Pure presentation timer for automatic cycling; manual navigation defers it.
#[derive(Debug, Clone)]
pub struct Kiosk {
    interval_ms: u64,
    manual_pause_ms: u64,
    next_ms: Option<u64>,
}
impl Default for Kiosk {
    fn default() -> Self {
        Self {
            interval_ms: 15_000,
            manual_pause_ms: 60_000,
            next_ms: None,
        }
    }
}
impl Kiosk {
    /// Configure a bounded interval and manual pause.
    /// # Errors
    /// Zero or excessive intervals are rejected.
    pub fn new(interval_ms: u64, manual_pause_ms: u64) -> Result<Self, MapError> {
        if !(1_000..=300_000).contains(&interval_ms)
            || !(1_000..=600_000).contains(&manual_pause_ms)
        {
            return Err(MapError);
        }
        Ok(Self {
            interval_ms,
            manual_pause_ms,
            next_ms: None,
        })
    }
    /// Whether a contact should advance now. Initial focus is immediately due;
    /// an absent selection never bypasses an explicit manual pause.
    pub fn due(&self, now_ms: u64, _selected: Option<&str>) -> bool {
        self.next_ms.is_none_or(|deadline| now_ms >= deadline)
    }
    /// Defer automatic cycling after any manual remote interaction.
    pub fn manual(&mut self, now_ms: u64) {
        self.next_ms = Some(now_ms.saturating_add(self.manual_pause_ms));
    }
    /// Start the next dwell only after a successful selection advance.
    pub fn mark_advanced(&mut self, now_ms: u64) {
        self.next_ms = Some(now_ms.saturating_add(self.interval_ms));
    }
}

/// Host-selected resource and motion bounds; not part of the map wire schema.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// Delay behind the host clock, allowing two known observations to bracket it.
    pub delay_ms: u64,
    /// Larger temporal gaps start a new track instead of drawing inferred motion.
    pub max_gap_ms: u64,
    /// Larger great-circle position jumps start a new track.
    pub max_jump_km: f64,
    /// Maximum retained stable identities, at most the map contract's 512.
    pub max_markers: usize,
    /// Samples per stable identity; aggregate storage is capped at 16,384 samples.
    pub samples_per_marker: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            delay_ms: 5_000,
            max_gap_ms: 30_000,
            max_jump_km: 250.0,
            max_markers: 512,
            samples_per_marker: 16,
        }
    }
}
impl Config {
    fn validate(self) -> Result<(), MapError> {
        if self.delay_ms > 60_000
            || self.max_gap_ms == 0
            || self.max_gap_ms > 300_000
            || self.delay_ms >= self.max_gap_ms
            || !self.max_jump_km.is_finite()
            || self.max_jump_km <= 0.0
            || self.max_jump_km > 20_000.0
            || !(1..=512).contains(&self.max_markers)
            || !(2..=128).contains(&self.samples_per_marker)
            || self
                .max_markers
                .checked_mul(self.samples_per_marker)
                .is_none_or(|n| n > 16_384)
        {
            return Err(MapError);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy)]
struct Sample {
    at_ms: u64,
    point: Point,
}

/// Retained state for one surface. The caller may put it behind its own mutex.
/// No background clock, provider requests or global singleton are created.
#[derive(Debug)]
pub struct Motion {
    config: Config,
    map: Option<Map>,
    history: BTreeMap<String, VecDeque<Sample>>,
    received_ms: Option<u64>,
    frozen_ms: Option<u64>,
    receive_time_fallbacks: usize,
}
impl Motion {
    /// Construct independently bounded state for a single surface.
    /// # Errors
    /// Invalid configuration is rejected rather than silently widened.
    pub fn new(config: Config) -> Result<Self, MapError> {
        config.validate()?;
        Ok(Self {
            config,
            map: None,
            history: BTreeMap::new(),
            received_ms: None,
            frozen_ms: None,
            receive_time_fallbacks: 0,
        })
    }
    /// Admit a complete source revision and optional per-identity observation times.
    /// Missing timestamps explicitly use receive time, counted by `fallback_count`.
    /// Disconnection freezes the existing view; it does not replace it with zeros.
    /// # Errors
    /// Rejects invalid maps, excessive identities, future or reversed host clocks.
    pub fn ingest(
        &mut self,
        map: &Map,
        observed_ms: &BTreeMap<String, u64>,
        received_ms: u64,
        connected: bool,
    ) -> Result<(), MapError> {
        map.validate()?;
        if map.markers.len() > self.config.max_markers
            || self
                .received_ms
                .is_some_and(|previous| received_ms < previous)
            || map.markers.iter().any(|m| {
                observed_ms
                    .get(&m.id)
                    .is_some_and(|at| *at == 0 || *at > received_ms.saturating_add(1_000))
            })
        {
            return Err(MapError);
        }
        if !connected {
            // Freeze at the first disconnect, not each subsequent failed poll.
            self.frozen_ms.get_or_insert(received_ms);
            self.received_ms = Some(received_ms);
            return Ok(());
        }
        let resumed = self.frozen_ms.take().is_some();
        if resumed {
            self.history.clear();
        }
        self.history
            .retain(|id, _| map.markers.iter().any(|m| &m.id == id));
        self.receive_time_fallbacks = 0;
        for marker in &map.markers {
            let at_ms = if let Some(at) = observed_ms.get(&marker.id) {
                *at
            } else {
                self.receive_time_fallbacks += 1;
                received_ms
            };
            let point = Point {
                latitude_deg: marker.latitude_deg,
                longitude_deg: marker.longitude_deg,
            };
            let samples = self.history.entry(marker.id.clone()).or_default();
            if let Some(previous) = samples.back() {
                // Duplicate/out-of-order owner evidence cannot grow or rewind a track.
                if at_ms <= previous.at_ms {
                    continue;
                }
                if at_ms - previous.at_ms > self.config.max_gap_ms
                    || distance_km(previous.point, point) > self.config.max_jump_km
                {
                    samples.clear();
                }
            }
            samples.push_back(Sample { at_ms, point });
            while samples.len() > self.config.samples_per_marker {
                samples.pop_front();
            }
        }
        self.map = Some(map.clone());
        self.received_ms = Some(received_ms);
        Ok(())
    }
    /// Number of contacts in the last revision lacking owner observation time.
    pub fn fallback_count(&self) -> usize {
        self.receive_time_fallbacks
    }
    /// Actual retained sample count, useful for host diagnostics and bound tests.
    pub fn sample_count(&self) -> usize {
        self.history.values().map(VecDeque::len).sum()
    }
    /// Project known history at a deterministic clock, clamped to known endpoints.
    /// Trails consume only the remaining existing map path/point budget. Source
    /// paths, selection, extent and marker captions are preserved unchanged.
    pub fn render(&self, now_ms: u64) -> Option<Map> {
        let mut map = self.map.clone()?;
        let clock = self.frozen_ms.unwrap_or(now_ms);
        let target = clock.saturating_sub(self.config.delay_ms);
        let mut point_budget =
            8192usize.saturating_sub(map.paths.iter().map(|p| p.points.len()).sum());
        for (index, marker) in map.markers.iter_mut().enumerate() {
            let Some(samples) = self.history.get(&marker.id) else {
                continue;
            };
            let point = position(samples, target)?;
            marker.latitude_deg = point.latitude_deg;
            marker.longitude_deg = point.longitude_deg;
            if map.paths.len() >= 128 || point_budget < 2 {
                continue;
            }
            let mut points: Vec<_> = samples
                .iter()
                .filter(|sample| sample.at_ms <= target)
                .map(|sample| sample.point)
                .collect();
            if points.last().is_some_and(|last| *last != point) {
                points.push(point);
            }
            // Repeated stationary observations are known timing evidence, not
            // additional geographic movement or zero-length trail segments.
            points.dedup();
            if points.len() < 2 {
                continue;
            }
            if points.len() > point_budget {
                points.drain(..points.len() - point_budget);
            }
            let mut salt = 0usize;
            let mut id = format!("motion-track-{index}-{salt}");
            while map.paths.iter().any(|p| p.id == id) {
                salt += 1;
                id = format!("motion-track-{index}-{salt}");
            }
            point_budget -= points.len();
            map.paths.push(Path {
                id,
                role: marker.role,
                points,
            });
        }
        Some(map)
    }
}
fn position(samples: &VecDeque<Sample>, target: u64) -> Option<Point> {
    let first = samples.front()?;
    if target <= first.at_ms {
        return Some(first.point);
    }
    for (first, second) in samples.iter().zip(samples.iter().skip(1)) {
        if target <= second.at_ms {
            #[allow(clippy::cast_precision_loss)]
            let fraction = (target - first.at_ms) as f64 / (second.at_ms - first.at_ms) as f64;
            let delta = (second.point.longitude_deg - first.point.longitude_deg + 180.0)
                .rem_euclid(360.0)
                - 180.0;
            return Some(Point {
                latitude_deg: first.point.latitude_deg
                    + (second.point.latitude_deg - first.point.latitude_deg) * fraction,
                longitude_deg: (first.point.longitude_deg + delta * fraction + 180.0)
                    .rem_euclid(360.0)
                    - 180.0,
            });
        }
    }
    samples.back().map(|last| last.point)
}
fn distance_km(a: Point, b: Point) -> f64 {
    let latitude = (b.latitude_deg - a.latitude_deg).to_radians();
    let longitude = (b.longitude_deg - a.longitude_deg).to_radians();
    let haversine = (latitude / 2.0).sin().powi(2)
        + a.latitude_deg.to_radians().cos()
            * b.latitude_deg.to_radians().cos()
            * (longitude / 2.0).sin().powi(2);
    12_742.0 * haversine.clamp(0.0, 1.0).sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kiosk_dwell_and_manual_pause_are_deterministic() {
        let mut kiosk = Kiosk::default();
        assert!(kiosk.due(10_000, None));
        kiosk.mark_advanced(10_000);
        assert!(!kiosk.due(24_999, Some("plane")));
        assert!(kiosk.due(25_000, Some("plane")));
        kiosk.manual(20_000);
        assert!(!kiosk.due(79_999, None));
        assert!(kiosk.due(80_000, None));
        kiosk.mark_advanced(80_000);
        assert!(!kiosk.due(80_000, Some("plane")));
        assert!(Kiosk::new(0, 60_000).is_err());
    }
    use crate::{geo::Extent, geo_map::Marker, ThemeRole};
    fn map(longitude: f64) -> Map {
        Map {
            basemap: None,
            extent: Extent {
                south_deg: -10.0,
                north_deg: 10.0,
                west_deg: 170.0,
                east_deg: -170.0,
            },
            markers: vec![Marker {
                id: "plane".into(),
                label: "Real".into(),
                latitude_deg: 0.0,
                longitude_deg: longitude,
                role: ThemeRole::Primary,
                observed_at_unix_ms: None,
            }],
            paths: vec![],
            selected: None,
        }
    }
    fn state() -> Motion {
        Motion::new(Config {
            max_jump_km: 1000.0,
            ..Config::default()
        })
        .unwrap()
    }
    #[test]
    fn deterministic_delayed_interpolation_never_extrapolates() {
        let mut state = state();
        state
            .ingest(&map(179.0), &BTreeMap::new(), 10_000, true)
            .unwrap();
        state
            .ingest(&map(179.2), &BTreeMap::new(), 20_000, true)
            .unwrap();
        assert!((state.render(20_000).unwrap().markers[0].longitude_deg - 179.1).abs() < 1e-9);
        assert_eq!(
            state.render(500_000).unwrap().markers[0].longitude_deg,
            179.2
        );
        assert_eq!(state.render(1).unwrap().markers[0].longitude_deg, 179.0);
        assert_eq!(state.fallback_count(), 1);
    }
    #[test]
    fn owner_timestamps_win_and_longitude_uses_shortest_wrap() {
        let mut state = state();
        state
            .ingest(
                &map(179.0),
                &BTreeMap::from([("plane".into(), 10_000)]),
                11_000,
                true,
            )
            .unwrap();
        state
            .ingest(
                &map(-179.0),
                &BTreeMap::from([("plane".into(), 20_000)]),
                21_000,
                true,
            )
            .unwrap();
        assert_eq!(
            state.render(20_000).unwrap().markers[0].longitude_deg,
            -180.0
        );
        assert_eq!(state.fallback_count(), 0);
    }
    #[test]
    fn selection_only_revisions_and_duplicate_positions_never_fake_trails() {
        let mut state = state();
        state
            .ingest(&map(179.0), &BTreeMap::new(), 10_000, true)
            .unwrap();
        let mut selected = map(179.0);
        selected.selected = Some("plane".into());
        state
            .ingest(&selected, &BTreeMap::new(), 10_000, true)
            .unwrap();
        assert_eq!(state.sample_count(), 1);
        state
            .ingest(&selected, &BTreeMap::new(), 20_000, true)
            .unwrap();
        let rendered = state.render(40_000).unwrap();
        assert!(rendered.paths.is_empty());
        assert_eq!(rendered.selected.as_deref(), Some("plane"));
    }
    #[test]
    fn disconnect_freezes_and_reconnect_does_not_bridge_gap() {
        let mut state = state();
        state
            .ingest(&map(179.0), &BTreeMap::new(), 10_000, true)
            .unwrap();
        state
            .ingest(&map(179.2), &BTreeMap::new(), 20_000, true)
            .unwrap();
        state
            .ingest(&map(179.2), &BTreeMap::new(), 20_000, false)
            .unwrap();
        let frozen = state.render(20_000).unwrap();
        assert_eq!(state.render(90_000).unwrap(), frozen);
        state
            .ingest(&map(179.4), &BTreeMap::new(), 100_000, true)
            .unwrap();
        assert_eq!(state.sample_count(), 1);
        assert!(state.render(105_000).unwrap().paths.is_empty());
    }
    #[test]
    fn huge_temporal_and_spatial_gaps_are_not_smoothed() {
        let mut state = state();
        state
            .ingest(&map(179.0), &BTreeMap::new(), 10_000, true)
            .unwrap();
        state
            .ingest(&map(179.5), &BTreeMap::new(), 90_000, true)
            .unwrap();
        assert_eq!(state.sample_count(), 1);
        state
            .ingest(&map(160.0), &BTreeMap::new(), 95_000, true)
            .unwrap();
        assert_eq!(state.sample_count(), 1);
        assert_eq!(
            state.render(95_000).unwrap().markers[0].longitude_deg,
            160.0
        );
    }
    #[test]
    fn samples_and_paths_are_bounded_and_removed_identities_pruned() {
        let mut state = Motion::new(Config {
            samples_per_marker: 3,
            ..Config::default()
        })
        .unwrap();
        for i in 1u32..10 {
            state
                .ingest(
                    &map(179.0 + f64::from(i) * 0.01),
                    &BTreeMap::new(),
                    u64::from(i) * 5000,
                    true,
                )
                .unwrap();
        }
        assert_eq!(state.sample_count(), 3);
        let mut source = map(179.1);
        source.paths = (0..128)
            .map(|id| Path {
                id: format!("source-{id}"),
                role: ThemeRole::Outline,
                points: vec![
                    Point {
                        latitude_deg: 0.0,
                        longitude_deg: 179.0,
                    },
                    Point {
                        latitude_deg: 1.0,
                        longitude_deg: 179.0,
                    },
                ],
            })
            .collect();
        state
            .ingest(&source, &BTreeMap::new(), 50_000, true)
            .unwrap();
        let result = state.render(60_000).unwrap();
        assert_eq!(result.paths, source.paths);
        result.validate().unwrap();
        source.markers.clear();
        state
            .ingest(&source, &BTreeMap::new(), 55_000, true)
            .unwrap();
        assert_eq!(state.sample_count(), 0);
    }
    #[test]
    fn invalid_config_and_times_leave_history_unchanged() {
        assert!(Motion::new(Config {
            samples_per_marker: 128,
            ..Config::default()
        })
        .is_err());
        let mut state = state();
        state
            .ingest(&map(179.0), &BTreeMap::new(), 10_000, true)
            .unwrap();
        assert!(state
            .ingest(
                &map(179.2),
                &BTreeMap::from([("plane".into(), 30_000)]),
                20_000,
                true
            )
            .is_err());
        assert_eq!(state.sample_count(), 1);
        assert!(state
            .ingest(&map(179.2), &BTreeMap::new(), 9_000, true)
            .is_err());
    }
}
