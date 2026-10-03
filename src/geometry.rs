//! Portable geometry shared by layout engines and renderers.

use serde::{Deserialize, Serialize};

/// A point in two-dimensional logical space.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: f32,
    /// Vertical coordinate.
    pub y: f32,
}

impl Point {
    /// Constructs a point.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Returns whether both coordinates are finite.
    pub const fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl From<[f32; 2]> for Point {
    fn from([x, y]: [f32; 2]) -> Self {
        Self { x, y }
    }
}

impl From<Point> for [f32; 2] {
    fn from(point: Point) -> Self {
        [point.x, point.y]
    }
}

/// A two-dimensional logical extent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Size {
    /// Horizontal extent.
    pub width: f32,
    /// Vertical extent.
    pub height: f32,
}

impl Size {
    /// Constructs a size.
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    /// Returns whether both extents are finite and non-negative.
    pub const fn is_valid(self) -> bool {
        self.width.is_finite() && self.height.is_finite() && self.width >= 0.0 && self.height >= 0.0
    }
}

/// An axis-aligned rectangle in logical space.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

impl Rect {
    /// Constructs a rectangle.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns the right edge.
    pub const fn right(self) -> f32 {
        self.x + self.width
    }

    /// Returns the bottom edge.
    pub const fn bottom(self) -> f32 {
        self.y + self.height
    }

    /// Returns the rectangle center.
    pub fn center(self) -> Point {
        Point::new(self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    /// Returns whether this rectangle overlaps another with positive area.
    pub const fn overlaps(&self, other: &Self) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }

    /// Returns whether a point lies on or inside this rectangle.
    pub const fn contains(&self, point: Point) -> bool {
        point.x >= self.x
            && point.x <= self.right()
            && point.y >= self.y
            && point.y <= self.bottom()
    }

    /// Returns whether a point lies on or inside this rectangle.
    ///
    /// This spelling is retained for compatibility with layout-engine APIs.
    pub const fn contains_point(&self, point: Point) -> bool {
        self.contains(point)
    }

    /// Returns whether coordinates are finite and extents are positive.
    pub const fn is_valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_queries_share_edge_semantics() {
        let rect = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert!(rect.contains(Point::new(40.0, 60.0)));
        assert!(rect.overlaps(&Rect::new(39.0, 59.0, 2.0, 2.0)));
        assert!(!rect.overlaps(&Rect::new(40.0, 60.0, 2.0, 2.0)));
        assert_eq!(rect.center(), Point::new(25.0, 40.0));
    }

    #[test]
    fn invalid_geometry_is_observable() {
        assert!(!Point::new(f32::NAN, 0.0).is_finite());
        assert!(!Size::new(-1.0, 2.0).is_valid());
        assert!(!Rect::new(0.0, 0.0, 0.0, 1.0).is_valid());
    }
}
