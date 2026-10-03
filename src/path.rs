//! Renderer-neutral vector path commands.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// One command in a retained vector path.
///
/// Coordinates use the consumer's logical coordinate space. Arc angles are in
/// radians; zero points right and positive values rotate clockwise in the
/// usual screen coordinate system.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PathCommand {
    /// Move to `(x, y)` without drawing.
    MoveTo(f32, f32),
    /// Draw a line to `(x, y)`.
    LineTo(f32, f32),
    /// Draw an arc from `start` to `end` around `(center_x, center_y)`.
    ArcTo(f32, f32, f32, f32, f32),
    /// Close the current sub-path.
    Close,
}

impl PathCommand {
    /// Validates numeric data before it crosses a renderer or process boundary.
    ///
    /// # Errors
    ///
    /// Returns [`PathError`] for non-finite coordinates or a non-positive arc
    /// radius.
    pub fn validate(self) -> Result<(), PathError> {
        match self {
            Self::MoveTo(x, y) | Self::LineTo(x, y) => {
                finite([x, y]).then_some(()).ok_or(PathError::NonFinite)
            }
            Self::ArcTo(center_x, center_y, radius, start, end) => {
                if !finite([center_x, center_y, radius, start, end]) {
                    Err(PathError::NonFinite)
                } else if radius <= 0.0 {
                    Err(PathError::ArcRadius)
                } else {
                    Ok(())
                }
            }
            Self::Close => Ok(()),
        }
    }
}

/// A retained sequence of vector commands.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VectorPath {
    commands: Vec<PathCommand>,
}

impl VectorPath {
    /// Constructs an empty path.
    pub const fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    /// Constructs an empty path with retained command capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            commands: Vec::with_capacity(capacity),
        }
    }

    /// Appends one command.
    pub fn push(&mut self, command: PathCommand) {
        self.commands.push(command);
    }

    /// Removes every command without releasing capacity.
    pub fn clear(&mut self) {
        self.commands.clear();
    }

    /// Returns the commands in path order.
    pub fn commands(&self) -> &[PathCommand] {
        &self.commands
    }

    /// Returns the number of commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Returns whether the path has no commands.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Returns retained command capacity for allocation diagnostics.
    pub fn capacity(&self) -> usize {
        self.commands.capacity()
    }

    /// Validates every command in path order.
    ///
    /// # Errors
    ///
    /// Returns the first invalid command error.
    pub fn validate(&self) -> Result<(), PathError> {
        self.commands
            .iter()
            .copied()
            .try_for_each(PathCommand::validate)
    }
}

impl From<Vec<PathCommand>> for VectorPath {
    fn from(commands: Vec<PathCommand>) -> Self {
        Self { commands }
    }
}

impl From<VectorPath> for Vec<PathCommand> {
    fn from(path: VectorPath) -> Self {
        path.commands
    }
}

/// Invalid vector path data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PathError {
    /// At least one coordinate or angle is non-finite.
    #[error("path coordinates and angles must be finite")]
    NonFinite,
    /// An arc has a zero or negative radius.
    #[error("arc radius must be positive")]
    ArcRadius,
}

fn finite<const N: usize>(values: [f32; N]) -> bool {
    values.into_iter().all(f32::is_finite)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_paths_validate_and_reuse_storage() {
        let mut path = VectorPath::with_capacity(3);
        path.push(PathCommand::MoveTo(0.0, 0.0));
        path.push(PathCommand::LineTo(2.0, 3.0));
        path.push(PathCommand::Close);
        assert_eq!(path.validate(), Ok(()));
        let capacity = path.capacity();
        path.clear();
        assert_eq!(path.capacity(), capacity);
    }

    #[test]
    fn malformed_arcs_fail_loudly() {
        assert_eq!(
            PathCommand::ArcTo(0.0, 0.0, 0.0, 0.0, 1.0).validate(),
            Err(PathError::ArcRadius)
        );
        assert_eq!(
            PathCommand::LineTo(f32::NAN, 0.0).validate(),
            Err(PathError::NonFinite)
        );
    }

    #[test]
    fn path_json_round_trips() {
        let path = VectorPath::from(vec![
            PathCommand::MoveTo(1.0, 2.0),
            PathCommand::ArcTo(3.0, 4.0, 5.0, 0.0, 1.0),
        ]);
        let json = serde_json::to_string(&path).unwrap();
        assert_eq!(serde_json::from_str::<VectorPath>(&json).unwrap(), path);
    }
}
