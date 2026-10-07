//! Symbolic, theme-role attention instruments. Acquisition and freshness remain host-owned.
use crate::{signals::ViewError, topology::Label, Primitive, Rect, Scene, ThemeRole};
use serde::{Deserialize, Serialize};

/// Five mutually exclusive, caller-observed attention buckets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attention {
    /// Response requested explicitly by the owner.
    pub input: u16,
    /// Approval requested explicitly by the owner.
    pub approval: u16,
    /// Working sessions.
    pub active: u16,
    /// Idle sessions.
    pub idle: u16,
    /// Unobserved or failed sessions.
    pub unknown: u16,
    /// False suppresses every numeric claim, including positive wait counts.
    pub fresh: bool,
}
impl Attention {
    fn cells(&self, b: Rect) -> Result<Vec<Rect>, ViewError> {
        if !b.is_valid()
            || !b.right().is_finite()
            || !b.bottom().is_finite()
            || b.width < 2.0
            || b.height < 2.0
            || b.width > 16384.0
            || b.height > 16384.0
            || b.x.abs() > 1_000_000.0
            || b.y.abs() > 1_000_000.0
            || [
                self.input,
                self.approval,
                self.active,
                self.idle,
                self.unknown,
            ]
            .iter()
            .map(|v| u32::from(*v))
            .sum::<u32>()
                > 32768
        {
            return Err(ViewError);
        }
        Ok((0_u8..5)
            .map(|i| {
                let (col, row) = if i < 2 { (i, 0_u8) } else { (i - 2, 1_u8) };
                let columns = if row == 0 { 2.0 } else { 3.0 };
                Rect::new(
                    b.x + f32::from(col) * b.width / columns,
                    b.y + f32::from(row) * b.height * 0.5,
                    b.width / columns,
                    b.height * 0.5,
                )
            })
            .collect())
    }
    fn values(&self) -> [u16; 5] {
        [
            self.input,
            self.approval,
            self.active,
            self.idle,
            self.unknown,
        ]
    }
    fn role(&self, i: usize) -> ThemeRole {
        if !self.fresh || self.values()[i] == 0 {
            return ThemeRole::TextMuted;
        }
        match i {
            0 => ThemeRole::Primary,
            1 => ThemeRole::Warning,
            2 => ThemeRole::Success,
            _ => ThemeRole::TextMuted,
        }
    }
    /// Shared symbol geometry, bounded to fewer than fifty line instances.
    /// # Errors
    /// Rejects invalid bounds or excessive counts before modifying the scene.
    pub fn append(&self, scene: &mut Scene, bounds: Rect) -> Result<(), ViewError> {
        let cells = self.cells(bounds)?;
        let paths: &[&[[f32; 2]]] = &[
            &[
                [-1., -0.7],
                [1., -0.7],
                [1., 0.5],
                [0., 0.5],
                [-0.6, 1.],
                [-0.6, 0.5],
                [-1., 0.5],
                [-1., -0.7],
            ],
            &[
                [-0.9, -0.7],
                [0., -1.],
                [0.9, -0.7],
                [0.8, 0.3],
                [0., 1.],
                [-0.8, 0.3],
                [-0.9, -0.7],
            ],
            &[
                [0.2, -1.],
                [-0.7, 0.1],
                [0., 0.1],
                [-0.2, 1.],
                [0.7, -0.1],
                [0., -0.1],
                [0.2, -1.],
            ],
            &[[-0.7, 0.], [0.7, 0.]],
            &[
                [-0.8, -0.5],
                [-1., 0.],
                [-0.7, 0.7],
                [0., 1.],
                [0.7, 0.7],
                [1., 0.],
                [0.8, -0.5],
            ],
        ];
        for (i, c) in cells.iter().enumerate() {
            let r = c.width.min(c.height) * 0.21;
            let x = c.x + c.width * 0.5;
            let y = c.y + c.height * 0.34;
            for p in paths[i].windows(2) {
                scene.push(Primitive::line(
                    [x + p[0][0] * r, y + p[0][1] * r],
                    [x + p[1][0] * r, y + p[1][1] * r],
                    (r * 0.09).max(0.01),
                    self.role(i),
                ));
            }
        }
        Ok(())
    }
    /// Labels use the same layout as the symbols; hosts own glyph rendering.
    /// # Errors
    /// Rejects invalid geometry or counts.
    pub fn labels(&self, bounds: Rect) -> Result<Vec<Label>, ViewError> {
        let names = ["Response", "Approval", "Working", "Resting", "Unknown"];
        Ok(self
            .cells(bounds)?
            .iter()
            .enumerate()
            .flat_map(|(i, c)| {
                let count = if self.fresh {
                    self.values()[i].to_string()
                } else {
                    "—".into()
                };
                [
                    (count, 0.62, ThemeRole::Text),
                    (names[i].into(), 0.82, self.role(i)),
                ]
                .map(|(text, y, role)| Label {
                    text,
                    bounds: Rect::new(
                        c.x + c.width * 0.12,
                        c.y + c.height * y,
                        c.width * 0.76,
                        c.height * 0.16,
                    ),
                    role,
                })
            })
            .collect())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn symbolic_frame_is_bounded_and_stale_counts_are_hidden() {
        let mut a = Attention {
            input: 2,
            approval: 1,
            active: 3,
            idle: 4,
            unknown: 1,
            fresh: true,
        };
        let b = Rect::new(0., 0., 640., 480.);
        let mut scene = Scene::new();
        a.append(&mut scene, b).unwrap();
        assert!(scene.validate());
        assert!(scene.len() < 50);
        assert_eq!(a.labels(b).unwrap()[0].text, "2");
        a.fresh = false;
        assert_eq!(a.labels(b).unwrap()[0].text, "—");
        let n = scene.len();
        assert!(a.append(&mut scene, Rect::new(0., 0., 0., 4.)).is_err());
        assert_eq!(scene.len(), n);
    }
    #[test]
    fn portable_frame_rejects_invalid_counts_and_unknown_fields() {
        let frame = serde_json::json!({"view":"attention","panel":{"input":0,"approval":1,"active":2,"idle":3,"unknown":4,"fresh":true}});
        let parsed: crate::signals::SignalFrame = serde_json::from_value(frame.clone()).unwrap();
        parsed.validate().unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), frame);
        let mut bad = frame.clone();
        bad["panel"]["input"] = serde_json::json!(-1);
        assert!(serde_json::from_value::<crate::signals::SignalFrame>(bad).is_err());
        let mut bad = frame;
        bad["panel"]["input"] = serde_json::json!(32768);
        assert!(serde_json::from_value::<crate::signals::SignalFrame>(bad)
            .unwrap()
            .validate()
            .is_err());
    }
}
