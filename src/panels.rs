//! Bounded editorial and task panels. Hosts own fonts, state, freshness and I/O.
use crate::{
    attention::Attention, signals::ViewError, topology::Label, Primitive, Rect, Scene, ThemeRole,
};
use serde::{Deserialize, Serialize};

fn valid(bounds: Rect) -> Result<(), ViewError> {
    if !bounds.is_valid()
        || bounds.width < 2.0
        || bounds.height < 2.0
        || bounds.width > 16384.0
        || bounds.height > 16384.0
        || bounds.x.abs() > 1_000_000.0
        || bounds.y.abs() > 1_000_000.0
        || !bounds.right().is_finite()
        || !bounds.bottom().is_finite()
    {
        return Err(ViewError);
    }
    Ok(())
}
fn text_valid(text: &str, maximum: usize) -> bool {
    text.len() <= maximum && !text.chars().any(|c| c.is_control() && c != '\n')
}
fn label(text: String, bounds: Rect, role: ThemeRole) -> Label {
    Label { text, bounds, role }
}

// Conservative fallback for hosts without text measurement; hosts still clip/ellipsize.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn wrapped(text: &str, bounds: Rect, maximum: usize) -> (Vec<String>, bool) {
    let columns = (bounds.width / (bounds.height * 0.8).clamp(10.0, 48.0) / 0.62)
        .floor()
        .clamp(8.0, 160.0) as usize;
    let mut result = Vec::new();
    let mut line = String::new();
    for paragraph in text.split('\n') {
        for word in paragraph.split_whitespace() {
            for chunk in word.chars().collect::<Vec<_>>().chunks(columns) {
                let chunk: String = chunk.iter().collect();
                if !line.is_empty() && line.chars().count() + chunk.chars().count() + 1 > columns {
                    result.push(std::mem::take(&mut line));
                }
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(&chunk);
            }
        }
        if !line.is_empty() {
            result.push(std::mem::take(&mut line));
        }
    }
    let more = result.len() > maximum;
    result.truncate(maximum);
    (result, more)
}

/// A bounded, owner-authored editorial brief; not an inferred summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Brief {
    /// Short source or section heading.
    pub kicker: String,
    /// Main title, up to 160 UTF-8 bytes.
    pub headline: String,
    /// Verbatim excerpt; wrapping remains bounded.
    pub body: String,
    /// Source/freshness note, never a debug identifier.
    pub footer: String,
}
impl Brief {
    fn validate(&self, bounds: Rect) -> Result<(), ViewError> {
        valid(bounds)?;
        if !text_valid(&self.kicker, 80)
            || !text_valid(&self.headline, 160)
            || !text_valid(&self.body, 4000)
            || !text_valid(&self.footer, 160)
        {
            return Err(ViewError);
        }
        Ok(())
    }
    /// Append subtle source accent without owning text or window rendering.
    /// # Errors
    /// Rejects invalid geometry or over-budget text before scene mutation.
    pub fn append(&self, scene: &mut Scene, bounds: Rect) -> Result<(), ViewError> {
        self.validate(bounds)?;
        scene.push(Primitive::rounded_rect(
            Rect::new(
                bounds.x,
                bounds.y,
                bounds.width * 0.13,
                bounds.height * 0.006,
            ),
            0.0,
            ThemeRole::Secondary,
        ));
        Ok(())
    }
    /// Shared editorial hierarchy and bounded line placement.
    /// # Errors
    /// Rejects invalid geometry or over-budget text.
    pub fn labels(&self, b: Rect) -> Result<Vec<Label>, ViewError> {
        self.validate(b)?;
        let mut result = vec![label(
            self.kicker.clone(),
            Rect::new(b.x, b.y + b.height * 0.03, b.width, b.height * 0.035),
            ThemeRole::Secondary,
        )];
        let head = Rect::new(b.x, b.y + b.height * 0.11, b.width, b.height * 0.08);
        let (titles, _) = wrapped(&self.headline, head, 2);
        let mut y = head.y;
        for title in titles {
            result.push(label(
                title,
                Rect::new(b.x, y, b.width, head.height),
                ThemeRole::Text,
            ));
            y += head.height;
        }
        let line = Rect::new(b.x, b.y + b.height * 0.33, b.width, b.height * 0.044);
        let (lines, more) = wrapped(&self.body, line, 12);
        y = line.y;
        for text in lines {
            result.push(label(
                text,
                Rect::new(b.x, y, b.width, line.height),
                ThemeRole::Text,
            ));
            y += line.height * 1.12;
        }
        result.push(label(
            if more {
                format!("Excerpt · {}", self.footer)
            } else {
                self.footer.clone()
            },
            Rect::new(b.x, b.y + b.height * 0.95, b.width, b.height * 0.035),
            ThemeRole::TextMuted,
        ));
        Ok(result)
    }
}

/// Explicit caller-observed task state; color is supplemented by shape and text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    /// Response requested.
    Input,
    /// Approval requested.
    Approval,
    /// Working.
    Active,
    /// Resting.
    Idle,
    /// Not known.
    Unknown,
}
impl TaskState {
    fn index(self) -> usize {
        match self {
            Self::Input => 0,
            Self::Approval => 1,
            Self::Active => 2,
            Self::Idle => 3,
            Self::Unknown => 4,
        }
    }
    fn role(self) -> ThemeRole {
        match self {
            Self::Input => ThemeRole::Primary,
            Self::Approval => ThemeRole::Warning,
            Self::Active => ThemeRole::Success,
            _ => ThemeRole::TextMuted,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Input => "Needs response",
            Self::Approval => "Needs approval",
            Self::Active => "Working",
            Self::Idle => "Resting",
            Self::Unknown => "Status unknown",
        }
    }
}
/// One task tile; no actionable authority is carried by these labels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    /// Project label.
    pub project: String,
    /// Owner-authored task label, not a transcript.
    pub title: String,
    /// Owner and optional first-observed wait time.
    pub meta: String,
    /// Explicit state.
    pub state: TaskState,
}
/// Up to four priority-selected task tiles. Selection is caller-owned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDeck {
    /// Caller-selected tasks.
    pub items: Vec<Task>,
    /// False overrides every tile's state and wait metadata.
    pub fresh: bool,
}
impl TaskDeck {
    fn cells(&self, b: Rect) -> Result<Vec<Rect>, ViewError> {
        valid(b)?;
        if self.items.len() > 4
            || self.items.iter().any(|i| {
                !text_valid(&i.project, 160)
                    || !text_valid(&i.title, 240)
                    || !text_valid(&i.meta, 240)
            })
        {
            return Err(ViewError);
        }
        let gap = b.width.min(b.height) * 0.035;
        Ok((0_u8..4)
            .map(|i| {
                Rect::new(
                    b.x + f32::from(i % 2) * (b.width + gap) * 0.5,
                    b.y + f32::from(i / 2) * (b.height + gap) * 0.5,
                    (b.width - gap) * 0.5,
                    (b.height - gap) * 0.5,
                )
            })
            .collect())
    }
    /// Append themed task wells and the canonical attention symbols.
    /// # Errors
    /// Rejects invalid geometry, text and cardinality atomically.
    pub fn append(&self, scene: &mut Scene, b: Rect) -> Result<(), ViewError> {
        let cells = self.cells(b)?;
        for (item, c) in self.items.iter().zip(cells) {
            let state = if self.fresh {
                item.state
            } else {
                TaskState::Unknown
            };
            scene.push(Primitive::rounded_rect(
                c,
                c.height * 0.045,
                ThemeRole::SurfaceRaised,
            ));
            scene.push(Primitive::rounded_rect(
                Rect::new(c.x, c.y + c.height * 0.17, c.width * 0.008, c.height * 0.66),
                c.width * 0.004,
                state.role(),
            ));
            let side = c.height * 0.18;
            Attention::append_symbol(
                scene,
                Rect::new(
                    c.x + c.width - side * 1.45,
                    c.y + c.height * 0.09,
                    side,
                    side,
                ),
                state.index(),
                state.role(),
            );
        }
        Ok(())
    }
    /// Project-first tile hierarchy; task and owner remain readable and secondary.
    /// # Errors
    /// Rejects invalid geometry or excessive text.
    pub fn labels(&self, b: Rect) -> Result<Vec<Label>, ViewError> {
        let cells = self.cells(b)?;
        let mut result = Vec::new();
        if self.items.is_empty() {
            return Ok(vec![label(
                "Waiting for owner telemetry".into(),
                Rect::new(b.x, b.y, b.width, b.height * 0.1),
                ThemeRole::TextMuted,
            )]);
        }
        for (item, c) in self.items.iter().zip(cells) {
            let state = if self.fresh {
                item.state
            } else {
                TaskState::Unknown
            };
            let x = c.x + c.width * 0.07;
            let width = c.width * 0.86;
            result.push(label(
                item.project.clone(),
                Rect::new(x, c.y + c.height * 0.10, c.width * 0.64, c.height * 0.16),
                ThemeRole::Text,
            ));
            result.push(label(
                state.label().into(),
                Rect::new(x, c.y + c.height * 0.32, width, c.height * 0.10),
                state.role(),
            ));
            let (lines, _) = wrapped(&item.title, Rect::new(x, 0., width, c.height * 0.12), 2);
            let mut y = c.y + c.height * 0.49;
            for text in lines {
                result.push(label(
                    text,
                    Rect::new(x, y, width, c.height * 0.12),
                    ThemeRole::Text,
                ));
                y += c.height * 0.14;
            }
            result.push(label(
                if self.fresh {
                    item.meta.clone()
                } else {
                    "Telemetry stale".into()
                },
                Rect::new(x, c.y + c.height * 0.85, width, c.height * 0.08),
                ThemeRole::TextMuted,
            ));
        }
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_deck_is_bounded_atomic_and_freshness_safe() {
        let item = Task {
            project: "Canvas".into(),
            title: "Shared primitives".into(),
            meta: "seen waiting 4m".into(),
            state: TaskState::Approval,
        };
        let mut deck = TaskDeck {
            items: vec![item; 4],
            fresh: true,
        };
        let b = Rect::new(0., 0., 1400., 800.);
        let mut scene = Scene::new();
        deck.append(&mut scene, b).unwrap();
        assert!(scene.validate() && scene.len() < 50);
        deck.fresh = false;
        let labels = deck.labels(b).unwrap();
        assert!(labels.iter().any(|l| l.text == "Status unknown"));
        assert!(!labels.iter().any(|l| l.text.contains("waiting 4m")));
        deck.items.push(deck.items[0].clone());
        let len = scene.len();
        assert!(deck.append(&mut scene, b).is_err());
        assert_eq!(scene.len(), len);
    }
    #[test]
    fn brief_wraps_unicode_and_marks_excerpt_without_overflow() {
        let brief = Brief {
            kicker: "PUBLIC REEL".into(),
            headline: "真实工作 · Canvas".into(),
            body: "bounded words ".repeat(180),
            footer: "Tardy".into(),
        };
        let b = Rect::new(0., 0., 1200., 900.);
        let labels = brief.labels(b).unwrap();
        assert!(labels.len() <= 16);
        assert!(labels.last().unwrap().text.contains("Excerpt"));
        assert!(labels
            .iter()
            .all(|l| l.bounds.bottom() <= b.bottom() + 0.01));
    }
}
