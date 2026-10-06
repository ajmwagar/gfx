//! Bounded signal-flow geometry. Hosts own acquisition, fonts and interaction.
use crate::{Point, Primitive, Rect, Scene, ThemeRole};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Maximum visible nodes in one topology view.
pub const MAX_NODES: usize = 64;
/// Maximum visible connections in one topology view.
pub const MAX_EDGES: usize = 128;

/// Observed or desired state; configured is deliberately not connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// No observation is available.
    Unknown,
    /// Desired route only; not verified live.
    Configured,
    /// Connection has been observed.
    Connected,
    /// Caller observed recent signal activity.
    Active,
    /// Valid observation with no recent activity.
    Idle,
    /// Connection has failed or is absent.
    Fault,
}
impl State {
    /// Semantic paint color; injected themes choose actual colors.
    pub const fn role(self) -> ThemeRole {
        match self {
            Self::Unknown | Self::Configured | Self::Idle => ThemeRole::TextMuted,
            Self::Connected => ThemeRole::Secondary,
            Self::Active => ThemeRole::Success,
            Self::Fault => ThemeRole::Warning,
        }
    }
    /// Short, truthful legend for hosts that render text.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Configured => "Configured",
            Self::Connected => "Connected",
            Self::Active => "Active",
            Self::Idle => "Idle",
            Self::Fault => "Not connected",
        }
    }
}

/// One source, processor, bus or sink. Lane numbers are ordering, not pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    /// Unique stable identity.
    pub id: String,
    /// Human-facing name, at most 96 characters.
    pub label: String,
    /// Left-to-right lane ordering, 0..=7.
    pub lane: u8,
    /// Caller-owned observation.
    pub state: State,
    /// Optional normalized meter value; never inferred from connection state.
    pub level: Option<f32>,
}
/// Directed cable between declared nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    /// Source node identity.
    pub source: String,
    /// Destination node identity.
    pub target: String,
    /// Caller-owned observation.
    pub state: State,
}
/// Portable graph snapshot, reusable beyond DAWs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    /// Sources, processors, buses and sinks.
    pub nodes: Vec<Node>,
    /// Directed connections.
    pub edges: Vec<Edge>,
}
/// Shared label placement; host paints glyphs using its existing text engine.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Label {
    /// Visible text.
    pub text: String,
    /// Available text rectangle.
    pub bounds: Rect,
    /// Semantic text color.
    pub role: ThemeRole,
}
/// Invalid graph, geometry or resource bound. No partial scenes are appended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid topology geometry, identity, level or connection")]
pub struct TopologyError;

impl Graph {
    fn layout(&self, bounds: Rect) -> Result<BTreeMap<&str, Rect>, TopologyError> {
        if !bounds.is_valid()
            || bounds.width > 16384.0
            || bounds.height > 16384.0
            || bounds.x.abs() > 1_000_000.0
            || bounds.y.abs() > 1_000_000.0
            || bounds.width < 2.0
            || bounds.height < 2.0
            || !bounds.right().is_finite()
            || !bounds.bottom().is_finite()
            || self.nodes.len() > MAX_NODES
            || self.edges.len() > MAX_EDGES
        {
            return Err(TopologyError);
        }
        let mut lanes = BTreeMap::<u8, Vec<&Node>>::new();
        let mut ids = BTreeSet::new();
        for node in &self.nodes {
            if node.id.is_empty()
                || node.id.len() > 128
                || !ids.insert(node.id.as_str())
                || node.label.is_empty()
                || node.label.chars().count() > 96
                || node.lane > 7
                || node
                    .level
                    .is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
            {
                return Err(TopologyError);
            }
            lanes.entry(node.lane).or_default().push(node);
        }
        for edge in &self.edges {
            if !ids.contains(edge.source.as_str()) || !ids.contains(edge.target.as_str()) {
                return Err(TopologyError);
            }
        }
        let columns = count(lanes.len().max(1));
        let column = bounds.width / columns;
        let mut result = BTreeMap::new();
        for (lane_index, nodes) in lanes.values_mut().enumerate() {
            nodes.sort_by(|a, b| a.id.cmp(&b.id));
            let row = bounds.height / count(nodes.len());
            for (i, node) in nodes.iter().enumerate() {
                let height = (row * 0.76).min(bounds.height * 0.12);
                result.insert(
                    node.id.as_str(),
                    Rect::new(
                        bounds.x + (count(lane_index) + 0.10) * column,
                        bounds.y + count(i) * row + (row - height) * 0.5,
                        column * 0.80,
                        height,
                    ),
                );
            }
        }
        Ok(result)
    }

    /// Append node cards, ports, curved cables and optional meter rails.
    /// Work is bounded by `MAX_NODES + 16 * MAX_EDGES`.
    ///
    /// # Errors
    /// Rejects invalid data before changing the caller's scene.
    pub fn append(&self, scene: &mut Scene, bounds: Rect) -> Result<(), TopologyError> {
        let positions = self.layout(bounds)?;
        for edge in &self.edges {
            let a = positions[edge.source.as_str()];
            let b = positions[edge.target.as_str()];
            let start = Point::new(a.right(), a.center().y);
            let end = Point::new(b.x, b.center().y);
            let bend = (end.x - start.x).abs() * 0.5;
            let c1 = Point::new(start.x + bend, start.y);
            let c2 = Point::new(end.x - bend, end.y);
            let mut previous = start;
            for step in 1_u8..=16 {
                let t = f32::from(step) / 16.0;
                let u = 1.0 - t;
                let next = Point::new(
                    u * u * u * start.x
                        + 3.0 * u * u * t * c1.x
                        + 3.0 * u * t * t * c2.x
                        + t * t * t * end.x,
                    u * u * u * start.y
                        + 3.0 * u * u * t * c1.y
                        + 3.0 * u * t * t * c2.y
                        + t * t * t * end.y,
                );
                if edge.state != State::Configured || step % 2 == 0 {
                    scene.push(Primitive::line(
                        previous.into(),
                        next.into(),
                        2.0,
                        edge.state.role(),
                    ));
                }
                previous = next;
            }
        }
        for node in &self.nodes {
            let rect = positions[node.id.as_str()];
            scene.push(Primitive::rounded_rect(
                rect,
                rect.height.min(12.0) * 0.5,
                ThemeRole::SurfaceRaised,
            ));
            let rail = Rect::new(rect.x, rect.y, rect.width.min(3.0), rect.height);
            scene.push(Primitive::rounded_rect(rail, 1.0, node.state.role()));
            for x in [rect.x, rect.right()] {
                let size = rect.height.min(8.0);
                scene.push(Primitive::rounded_rect(
                    Rect::new(x - size * 0.5, rect.center().y - size * 0.5, size, size),
                    size * 0.5,
                    node.state.role(),
                ));
            }
            if let Some(level) = node.level {
                let width = rect.width * 0.85;
                let y = rect.bottom() - rect.height * 0.12;
                scene.push(Primitive::rounded_rect(
                    Rect::new(rect.x + rect.width * 0.075, y, width, 2.0),
                    1.0,
                    ThemeRole::Outline,
                ));
                scene.push(Primitive::rounded_rect(
                    Rect::new(rect.x + rect.width * 0.075, y, width * level, 2.0),
                    1.0,
                    ThemeRole::Success,
                ));
            }
        }
        Ok(())
    }

    /// Label placement and node-state captions, deterministic under input reorder.
    ///
    /// # Errors
    /// Uses the same validation as geometry generation.
    pub fn labels(&self, bounds: Rect) -> Result<Vec<Label>, TopologyError> {
        let positions = self.layout(bounds)?;
        let mut labels = vec![];
        for node in &self.nodes {
            let rect = positions[node.id.as_str()];
            let x = rect.x + rect.width * 0.075;
            let width = rect.width * 0.85;
            labels.push(Label {
                text: node.label.clone(),
                bounds: Rect::new(x, rect.y + rect.height * 0.12, width, rect.height * 0.35),
                role: ThemeRole::Text,
            });
            labels.push(Label {
                text: node.state.label().into(),
                bounds: Rect::new(x, rect.y + rect.height * 0.5, width, rect.height * 0.25),
                role: node.state.role(),
            });
        }
        Ok(labels)
    }
}
fn count(value: usize) -> f32 {
    f32::from(u16::try_from(value).expect("bounded topology count"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn graph() -> Graph {
        Graph {
            nodes: vec![
                Node {
                    id: "keys".into(),
                    label: "Keys".into(),
                    lane: 0,
                    state: State::Connected,
                    level: None,
                },
                Node {
                    id: "synth".into(),
                    label: "Synth".into(),
                    lane: 1,
                    state: State::Active,
                    level: Some(0.4),
                },
            ],
            edges: vec![Edge {
                source: "keys".into(),
                target: "synth".into(),
                state: State::Configured,
            }],
        }
    }
    #[test]
    fn bounded_valid_geometry_and_input_independent_layout() {
        let mut graph = graph();
        let bounds = Rect::new(0.0, 0.0, 800.0, 400.0);
        let before: BTreeMap<String, Rect> = graph
            .layout(bounds)
            .unwrap()
            .into_iter()
            .map(|(id, rect)| (id.to_owned(), rect))
            .collect();
        graph.nodes.reverse();
        let after: BTreeMap<String, Rect> = graph
            .layout(bounds)
            .unwrap()
            .into_iter()
            .map(|(id, rect)| (id.to_owned(), rect))
            .collect();
        assert_eq!(before, after);
        let mut scene = Scene::new();
        graph.append(&mut scene, bounds).unwrap();
        assert!(scene.validate());
        assert_eq!(graph.labels(bounds).unwrap().len(), 4);
        assert!(scene.len() < 40);
    }
    #[test]
    fn invalid_graph_never_partially_appends() {
        let mut graph = graph();
        let mut scene = Scene::new();
        graph.edges[0].target = "missing".into();
        assert!(graph
            .append(&mut scene, Rect::new(0.0, 0.0, 800.0, 400.0))
            .is_err());
        assert!(scene.is_empty());
        graph.edges.clear();
        graph.nodes[0].level = Some(f32::NAN);
        assert!(graph
            .append(&mut scene, Rect::new(0.0, 0.0, 800.0, 400.0))
            .is_err());
        assert!(scene.is_empty());
    }
    #[test]
    fn resource_limits_and_extreme_bounds_fail_before_append() {
        let mut graph = graph();
        let mut scene = Scene::new();
        assert!(graph
            .append(&mut scene, Rect::new(0.0, 0.0, f32::MAX, 100.0))
            .is_err());
        graph.nodes = (0..=MAX_NODES)
            .map(|i| Node {
                id: i.to_string(),
                label: "Source".into(),
                lane: 0,
                state: State::Unknown,
                level: None,
            })
            .collect();
        assert!(graph
            .append(&mut scene, Rect::new(0.0, 0.0, 800.0, 400.0))
            .is_err());
        assert!(scene.is_empty());
    }
}
