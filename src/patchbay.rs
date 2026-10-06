//! Paired-jack annotations. Wiring, patch state and fonts belong to the host.
use crate::{Primitive, Rect, Scene, ThemeRole, topology::Label};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Declared wiring mode, not proof of current physical continuity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Both jacks break the default connection.
    Normal,
    /// Top jack taps; bottom jack breaks the default connection.
    HalfNormal,
    /// No default connection.
    Thru,
}

/// A column of two jacks, numbered by the caller's physical map.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pair {
    /// One-based physical column identifier, unique within the strip.
    pub column: u8,
    /// Caller-owned label for the upper jack, up to 96 characters.
    pub top: String,
    /// Caller-owned label for the lower jack, up to 96 characters.
    pub bottom: String,
    /// Declared normalization; does not sense inserted patch cables.
    pub mode: Mode,
}

/// A bounded, reusable patchbay strip, often used beside a signal-flow graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patchbay {
    /// At most 48 pairs, laid out in ascending column order.
    pub pairs: Vec<Pair>,
}

impl Patchbay {
    fn validate(&self, bounds: Rect) -> Result<(), crate::topology::TopologyError> {
        let mut columns = BTreeSet::new();
        if !bounds.is_valid()
            || bounds.width < 2.0
            || bounds.height < 2.0
            || bounds.width > 16384.0
            || bounds.height > 16384.0
            || bounds.x.abs() > 1_000_000.0
            || bounds.y.abs() > 1_000_000.0
            || self.pairs.len() > 48
            || self.pairs.iter().any(|pair| {
                pair.column == 0
                    || !columns.insert(pair.column)
                    || [&pair.top, &pair.bottom]
                        .iter()
                        .any(|label| label.chars().count() > 96)
            })
        {
            return Err(crate::topology::TopologyError);
        }
        Ok(())
    }

    /// Geometry and text share one layout, with no window, GPU or studio dependency.
    /// Invalid input leaves the destination scene unchanged.
    ///
    /// # Errors
    /// Rejects invalid bounds, duplicate columns or excessive labels/pairs.
    pub fn append(
        &self,
        scene: &mut Scene,
        bounds: Rect,
    ) -> Result<(), crate::topology::TopologyError> {
        self.validate(bounds)?;
        let width = bounds.width / count(self.pairs.len().max(1))?;
        let mut pairs: Vec<_> = self.pairs.iter().collect();
        pairs.sort_by_key(|pair| pair.column);
        for (i, pair) in pairs.into_iter().enumerate() {
            let x = bounds.x + (count(i)? + 0.5) * width;
            let radius = (width * 0.12).min(bounds.height * 0.05);
            let top = bounds.y + bounds.height * 0.32;
            let bottom = bounds.y + bounds.height * 0.68;
            if pair.mode != Mode::Thru {
                scene.push(Primitive::line(
                    [x, top],
                    [x, bottom],
                    1.5,
                    ThemeRole::TextMuted,
                ));
            }
            for y in [top, bottom] {
                scene.push(Primitive::rounded_rect(
                    Rect::new(x - radius, y - radius, radius * 2.0, radius * 2.0),
                    radius,
                    ThemeRole::Outline,
                ));
                scene.push(Primitive::rounded_rect(
                    Rect::new(x - radius * 0.5, y - radius * 0.5, radius, radius),
                    radius * 0.5,
                    ThemeRole::Surface,
                ));
            }
        }
        Ok(())
    }

    /// Labels include the declaration mode so half-normal is not mistaken for thru.
    ///
    /// # Errors
    /// Uses the same validation and ordering as geometry generation.
    pub fn labels(&self, bounds: Rect) -> Result<Vec<Label>, crate::topology::TopologyError> {
        self.validate(bounds)?;
        let width = bounds.width / count(self.pairs.len().max(1))?;
        let mut pairs: Vec<_> = self.pairs.iter().collect();
        pairs.sort_by_key(|pair| pair.column);
        let mut labels = vec![];
        for (i, pair) in pairs.into_iter().enumerate() {
            let x = bounds.x + count(i)? * width;
            for (y, text) in [
                (0.04, format!("{} · {}", pair.column, pair.top)),
                (
                    0.44,
                    match pair.mode {
                        Mode::Normal => "Normal",
                        Mode::HalfNormal => "Half-normal",
                        Mode::Thru => "Thru",
                    }
                    .into(),
                ),
                (0.80, pair.bottom.clone()),
            ] {
                labels.push(Label {
                    text,
                    bounds: Rect::new(
                        x + width * 0.05,
                        bounds.y + bounds.height * y,
                        width * 0.9,
                        bounds.height * 0.14,
                    ),
                    role: ThemeRole::TextMuted,
                });
            }
        }
        Ok(labels)
    }
}

fn count(value: usize) -> Result<f32, crate::topology::TopologyError> {
    u8::try_from(value)
        .map(f32::from)
        .map_err(|_| crate::topology::TopologyError)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paired_jacks_and_declared_mode_are_bounded() {
        let bay = Patchbay {
            pairs: vec![Pair {
                column: 1,
                top: "Synth".into(),
                bottom: "Input 1".into(),
                mode: Mode::HalfNormal,
            }],
        };
        let bounds = Rect::new(0.0, 0.0, 200.0, 100.0);
        let mut scene = Scene::new();
        bay.append(&mut scene, bounds).unwrap();
        assert!(scene.validate());
        assert_eq!(scene.len(), 5);
        assert_eq!(bay.labels(bounds).unwrap()[1].text, "Half-normal");
    }
    #[test]
    fn duplicate_columns_fail_before_scene_mutation() {
        let pair = Pair {
            column: 1,
            top: "".into(),
            bottom: "".into(),
            mode: Mode::Thru,
        };
        let bay = Patchbay {
            pairs: vec![pair.clone(), pair],
        };
        let mut scene = Scene::new();
        assert!(
            bay.append(&mut scene, Rect::new(0.0, 0.0, 200.0, 100.0))
                .is_err()
        );
        assert!(scene.is_empty());
    }
}
