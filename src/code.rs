//! Bounded code/disassembly geometry with caller-owned source observations.
//! No decoding, CPU execution tracking, theme palettes or transport lives here.

use crate::{topology::Label, view_appearance::ViewAppearance, Primitive, Rect, Scene};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Maximum retained rows in one code snapshot.
pub const MAX_ROWS: usize = 128;
/// Maximum visible rows; remaining rows are explicitly reported in the footer.
pub const MAX_VISIBLE_ROWS: usize = 32;

/// An owner-decoded instruction, preserving text rather than guessing semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeRow {
    /// Owner address-space notation; never an inferred execution address.
    pub address: String,
    /// Optional owner-observed encoded bytes, not reconstructed from the mnemonic.
    #[serde(default)]
    pub bytes: Option<String>,
    /// Owner-decoded mnemonic or other code token.
    pub mnemonic: String,
    /// Owner-decoded operand text. Empty means no operands were supplied.
    pub operands: String,
    /// Optional owner annotation; never automatically invented.
    #[serde(default)]
    pub comment: Option<String>,
}

/// A code viewport. Observation timestamps and state remain in the host contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeGrid {
    /// Short program/function title.
    pub title: String,
    /// Caller-authored source identity caption, not a connection assertion.
    pub source: String,
    /// Caller-authored freshness/state caption; this module owns no clock.
    pub freshness: String,
    /// Address-ordered rows exactly as supplied by the owner.
    pub rows: Vec<CodeRow>,
    /// Inspection selection only. When supplied it must identify a retained row.
    #[serde(default)]
    pub selected_address: Option<String>,
    /// First retained row to show, bounded by the snapshot row count.
    #[serde(default)]
    pub first_row: usize,
}

/// Semantic code text zone. Hosts paint code columns with a monospace font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeColumn {
    /// Header/program identity.
    Title,
    /// Owner address.
    Address,
    /// Optional bytes.
    Bytes,
    /// Mnemonic.
    Mnemonic,
    /// Operands.
    Operands,
    /// Optional annotation.
    Comment,
    /// Source, freshness and viewport coverage.
    Footer,
}

/// Shared text rectangle plus a host font/column semantic.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CodeLabel {
    /// Column identity; all instruction columns require monospace host painting.
    pub column: CodeColumn,
    /// Shared text geometry and theme role.
    pub label: Label,
}

/// Shared row geometry suitable for painting and host-owned hit testing.
#[derive(Debug, Clone, PartialEq)]
pub struct RowGeometry {
    /// Index in the retained snapshot, not a CPU instruction count.
    pub index: usize,
    /// Whole row bounds.
    pub bounds: Rect,
    /// Address text bounds.
    pub address: Rect,
    /// Bytes column, present only when some retained row contains observed bytes.
    pub bytes: Option<Rect>,
    /// Mnemonic text bounds.
    pub mnemonic: Rect,
    /// Operand text bounds.
    pub operands: Rect,
    /// Comment column, present only when some retained row contains an annotation.
    pub comment: Option<Rect>,
    /// Whether this row is the inspection selection.
    pub selected: bool,
}

/// Deterministic layout independent of material/skin selection.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeGeometry {
    /// Header bounds.
    pub title: Rect,
    /// Footer bounds.
    pub footer: Rect,
    /// Visible rows with stable snapshot indices.
    pub rows: Vec<RowGeometry>,
    /// Retained rows before this viewport.
    pub omitted_before: usize,
    /// Retained rows after this viewport.
    pub omitted_after: usize,
}

/// Invalid source rows, selection, bounds or appearance. No partial scene mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid code grid rows, selection, geometry or appearance")]
pub struct CodeError;

fn text(value: &str, maximum: usize, nonempty: bool) -> bool {
    value.len() <= maximum
        && (!nonempty || !value.trim().is_empty())
        && !value.chars().any(char::is_control)
}

fn count(value: usize) -> f32 {
    f32::from(u16::try_from(value).expect("bounded code row count"))
}

impl CodeGrid {
    /// Validate data independently of renderer availability.
    ///
    /// # Errors
    /// Rejects over-budget/control text, duplicate addresses and absent selections.
    pub fn validate(&self) -> Result<(), CodeError> {
        if !text(&self.title, 160, true)
            || !text(&self.source, 128, true)
            || !text(&self.freshness, 128, true)
            || self.rows.len() > MAX_ROWS
            || (!self.rows.is_empty() && self.first_row >= self.rows.len())
            || (self.rows.is_empty() && self.first_row != 0)
        {
            return Err(CodeError);
        }
        let mut addresses = BTreeSet::new();
        for row in &self.rows {
            if !text(&row.address, 64, true)
                || !addresses.insert(row.address.as_str())
                || !text(&row.mnemonic, 64, true)
                || !text(&row.operands, 512, false)
                || row
                    .bytes
                    .as_ref()
                    .is_some_and(|value| !text(value, 128, true))
                || row
                    .comment
                    .as_ref()
                    .is_some_and(|value| !text(value, 512, true))
            {
                return Err(CodeError);
            }
        }
        if self
            .selected_address
            .as_ref()
            .is_some_and(|address| !addresses.contains(address.as_str()))
        {
            return Err(CodeError);
        }
        Ok(())
    }

    /// Derive column and row rectangles once for painting and hit targets.
    ///
    /// # Errors
    /// Rejects invalid data or bounds outside 240..=16384 by 128..=16384 pixels.
    pub fn geometry(&self, bounds: Rect) -> Result<CodeGeometry, CodeError> {
        self.validate()?;
        if !bounds.is_valid()
            || !(240.0..=16384.0).contains(&bounds.width)
            || !(128.0..=16384.0).contains(&bounds.height)
            || bounds.x.abs() > 1_000_000.0
            || bounds.y.abs() > 1_000_000.0
            || !bounds.right().is_finite()
            || !bounds.bottom().is_finite()
        {
            return Err(CodeError);
        }
        let x = bounds.x + 12.0;
        let width = bounds.width - 24.0;
        let title = Rect::new(x, bounds.y + 12.0, width, 24.0);
        let footer = Rect::new(x, bounds.bottom() - 28.0, width, 16.0);
        let body_y = title.bottom() + 12.0;
        let body_height = footer.y - body_y - 8.0;
        let capacity = (1..=MAX_VISIBLE_ROWS)
            .rfind(|n| count(*n) * 18.0 <= body_height)
            .unwrap_or(1);
        let row_height = (body_height / count(capacity)).min(28.0);
        let visible = self.rows.len().saturating_sub(self.first_row).min(capacity);
        let has_bytes = self.rows.iter().any(|row| row.bytes.is_some());
        let has_comments = self.rows.iter().any(|row| row.comment.is_some());
        let address_width = width * 0.20;
        let bytes_width = if has_bytes { width * 0.18 } else { 0.0 };
        let mnemonic_width = width * 0.14;
        let comment_width = if has_comments { width * 0.23 } else { 0.0 };
        let operand_width = width - address_width - bytes_width - mnemonic_width - comment_width;
        let mut rows = Vec::with_capacity(visible);
        for offset in 0..visible {
            let index = self.first_row + offset;
            let y = body_y + count(offset) * row_height;
            let zone = |left: f32, size: f32| Rect::new(left + 3.0, y, size - 6.0, row_height);
            let bytes_x = x + address_width;
            let mnemonic_x = bytes_x + bytes_width;
            let operands_x = mnemonic_x + mnemonic_width;
            rows.push(RowGeometry {
                index,
                bounds: Rect::new(x, y, width, row_height),
                address: zone(x, address_width),
                bytes: has_bytes.then(|| zone(bytes_x, bytes_width)),
                mnemonic: zone(mnemonic_x, mnemonic_width),
                operands: zone(operands_x, operand_width),
                comment: has_comments.then(|| zone(operands_x + operand_width, comment_width)),
                selected: self.selected_address.as_deref()
                    == Some(self.rows[index].address.as_str()),
            });
        }
        Ok(CodeGeometry {
            title,
            footer,
            rows,
            omitted_before: self.first_row,
            omitted_after: self.rows.len() - self.first_row - visible,
        })
    }

    /// Append the clean shared geometry.
    ///
    /// # Errors
    /// Rejects invalid data before changing the caller's scene.
    pub fn append(&self, scene: &mut Scene, bounds: Rect) -> Result<(), CodeError> {
        self.append_with_appearance(scene, bounds, &ViewAppearance::clean())
    }

    /// Apply a finish without changing row identities, layout or selection.
    ///
    /// # Errors
    /// Rejects invalid data/appearance before changing the caller's scene.
    pub fn append_with_appearance(
        &self,
        scene: &mut Scene,
        bounds: Rect,
        appearance: &ViewAppearance,
    ) -> Result<(), CodeError> {
        appearance.validate().map_err(|_| CodeError)?;
        let geometry = self.geometry(bounds)?;
        scene.push(
            appearance.apply(
                Primitive::rounded_rect(bounds, 0.0, appearance.face)
                    .with_outline(appearance.outline, 1.0),
            ),
        );
        scene.push(appearance.apply(Primitive::rounded_rect(
            Rect::new(
                geometry.title.x,
                geometry.title.bottom() + 4.0,
                geometry.title.width,
                1.0,
            ),
            0.0,
            appearance.outline,
        )));
        for row in &geometry.rows {
            if row.selected {
                scene.push(appearance.apply(Primitive::rounded_rect(
                    row.bounds,
                    0.0,
                    appearance.selection,
                )));
                scene.push(appearance.apply(Primitive::rounded_rect(
                    Rect::new(row.bounds.x, row.bounds.y, 2.0, row.bounds.height),
                    0.0,
                    appearance.accent,
                )));
            }
        }
        Ok(())
    }

    /// Shared labels for legacy hosts. Prefer `text_runs` to retain font semantics.
    ///
    /// # Errors
    /// Uses the same validation as geometry generation.
    pub fn labels(&self, bounds: Rect) -> Result<Vec<Label>, CodeError> {
        Ok(self
            .text_runs(bounds, &ViewAppearance::clean())?
            .into_iter()
            .map(|run| run.label)
            .collect())
    }

    /// Exact source text with fixed columns; hosts clip text and paint code as monospace.
    ///
    /// # Errors
    /// Rejects invalid data, geometry or appearance.
    pub fn text_runs(
        &self,
        bounds: Rect,
        appearance: &ViewAppearance,
    ) -> Result<Vec<CodeLabel>, CodeError> {
        appearance.validate().map_err(|_| CodeError)?;
        let geometry = self.geometry(bounds)?;
        let mut labels = Vec::new();
        let mut push = |column, value: &str, bounds, role| {
            labels.push(CodeLabel {
                column,
                label: Label {
                    text: value.to_owned(),
                    bounds,
                    role,
                },
            });
        };
        push(
            CodeColumn::Title,
            &self.title,
            geometry.title,
            appearance.ink,
        );
        for zone in &geometry.rows {
            let row = &self.rows[zone.index];
            push(
                CodeColumn::Address,
                &row.address,
                zone.address,
                appearance.muted,
            );
            if let (Some(value), Some(bounds)) = (&row.bytes, zone.bytes) {
                push(CodeColumn::Bytes, value, bounds, appearance.muted);
            }
            push(
                CodeColumn::Mnemonic,
                &row.mnemonic,
                zone.mnemonic,
                appearance.ink,
            );
            if !row.operands.is_empty() {
                push(
                    CodeColumn::Operands,
                    &row.operands,
                    zone.operands,
                    appearance.ink,
                );
            }
            if let (Some(value), Some(bounds)) = (&row.comment, zone.comment) {
                push(CodeColumn::Comment, value, bounds, appearance.muted);
            }
        }
        let coverage = if self.rows.is_empty() {
            "No observed instructions".to_owned()
        } else {
            format!(
                "Rows {}–{} / {}",
                self.first_row + 1,
                self.first_row + geometry.rows.len(),
                self.rows.len()
            )
        };
        let outside = self.selected_address.as_ref().is_some_and(|address| {
            !geometry
                .rows
                .iter()
                .any(|zone| &self.rows[zone.index].address == address)
        });
        let footer = format!(
            "{} · {} · {}{} · Selection, not execution",
            self.source,
            self.freshness,
            coverage,
            if outside {
                " · Selection outside viewport"
            } else {
                ""
            }
        );
        push(
            CodeColumn::Footer,
            &footer,
            geometry.footer,
            appearance.muted,
        );
        Ok(labels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> CodeGrid {
        CodeGrid {
            title: "Fixture only".into(),
            source: "test owner".into(),
            freshness: "recorded fixture".into(),
            rows: vec![CodeRow {
                address: "ram:00401000".into(),
                bytes: Some("90".into()),
                mnemonic: "NOP".into(),
                operands: String::new(),
                comment: None,
            }],
            selected_address: Some("ram:00401000".into()),
            first_row: 0,
        }
    }

    #[test]
    fn deterministic_columns_preserve_owner_text_and_selection() {
        let grid = grid();
        let bounds = Rect::new(10.0, 20.0, 800.0, 600.0);
        assert_eq!(
            grid.geometry(bounds).unwrap(),
            grid.geometry(bounds).unwrap()
        );
        let geometry = grid.geometry(bounds).unwrap();
        assert!(geometry.rows[0].selected);
        assert!(geometry.rows[0].bytes.is_some());
        assert!(geometry.rows[0].comment.is_none());
        let runs = grid.text_runs(bounds, &ViewAppearance::clean()).unwrap();
        assert!(runs
            .iter()
            .any(|run| run.column == CodeColumn::Address && run.label.text == "ram:00401000"));
        assert!(runs
            .iter()
            .any(|run| run.column == CodeColumn::Mnemonic && run.label.text == "NOP"));
        assert!(runs
            .last()
            .unwrap()
            .label
            .text
            .contains("Selection, not execution"));
    }

    #[test]
    fn finish_does_not_change_geometry_or_identity() {
        let grid = grid();
        let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
        let mut clean = Scene::new();
        let mut terminal = Scene::new();
        grid.append(&mut clean, bounds).unwrap();
        grid.append_with_appearance(&mut terminal, bounds, &ViewAppearance::terminal())
            .unwrap();
        assert_eq!(clean.len(), terminal.len());
        assert!(clean.validate() && terminal.validate());
        for (a, b) in clean.iter().zip(terminal.iter()) {
            assert_eq!(a.rect, b.rect);
            assert_eq!(a.kind, b.kind);
            assert_eq!(a.value, b.value);
        }
        let clean_labels = grid.text_runs(bounds, &ViewAppearance::clean()).unwrap();
        let terminal_labels = grid.text_runs(bounds, &ViewAppearance::terminal()).unwrap();
        for (a, b) in clean_labels.iter().zip(terminal_labels.iter()) {
            assert_eq!(a.column, b.column);
            assert_eq!(a.label.text, b.label.text);
            assert_eq!(a.label.bounds, b.label.bounds);
        }
    }

    #[test]
    fn invalid_inputs_leave_existing_scene_unchanged() {
        let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
        let mut scene = Scene::new();
        scene.push(Primitive::rounded_rect(
            bounds,
            0.0,
            crate::ThemeRole::Surface,
        ));
        let mut invalid = grid();
        invalid.rows.push(invalid.rows[0].clone());
        assert!(invalid.append(&mut scene, bounds).is_err());
        assert_eq!(scene.len(), 1);
        invalid = grid();
        invalid.selected_address = Some("missing".into());
        assert!(invalid.append(&mut scene, bounds).is_err());
        assert_eq!(scene.len(), 1);
        invalid = grid();
        invalid.rows[0].operands = "injected\nrow".into();
        assert!(invalid.append(&mut scene, bounds).is_err());
        assert_eq!(scene.len(), 1);
        assert!(grid()
            .append(&mut scene, Rect::new(0.0, 0.0, f32::NAN, 600.0))
            .is_err());
        assert_eq!(scene.len(), 1);
        let mut appearance = ViewAppearance::clean();
        appearance.material.emission = f32::INFINITY;
        assert!(grid()
            .append_with_appearance(&mut scene, bounds, &appearance)
            .is_err());
        assert_eq!(scene.len(), 1);
    }

    #[test]
    fn viewport_reports_omitted_rows_and_offscreen_selection() {
        let mut grid = grid();
        grid.rows = (0..MAX_ROWS)
            .map(|index| CodeRow {
                address: format!("ram:{index:08x}"),
                bytes: None,
                mnemonic: "fixture".into(),
                operands: String::new(),
                comment: None,
            })
            .collect();
        grid.first_row = 50;
        grid.selected_address = Some("ram:00000000".into());
        let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
        let geometry = grid.geometry(bounds).unwrap();
        assert_eq!(geometry.omitted_before, 50);
        assert!(geometry.omitted_after > 0);
        assert!(geometry.rows.len() <= MAX_VISIBLE_ROWS);
        assert!(grid
            .labels(bounds)
            .unwrap()
            .last()
            .unwrap()
            .text
            .contains("Selection outside viewport"));
        grid.rows.push(grid.rows[0].clone());
        assert!(grid.validate().is_err());
    }

    #[test]
    fn empty_source_rows_remain_missing_not_generated() {
        let mut grid = grid();
        grid.rows.clear();
        grid.selected_address = None;
        let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
        assert!(grid.geometry(bounds).unwrap().rows.is_empty());
        assert!(grid
            .labels(bounds)
            .unwrap()
            .last()
            .unwrap()
            .text
            .contains("No observed instructions"));
        grid.first_row = 1;
        assert!(grid.validate().is_err());
    }
}
