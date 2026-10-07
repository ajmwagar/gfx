//! Solver-independent analysis fields bound to exact ordered mesh vertices.
//! Solvers remain with the producer; gfx owns validation, normalization and presentation.
// Exact equality binds vertices and detects truly constant ranges.
#![allow(clippy::float_cmp)]
use crate::mesh::Colormap;
use serde::{Deserialize, Serialize};

/// Maximum vertex count accepted for a field, matching the mesh host bound.
pub const MAX_FIELD_VERTICES: usize = 10_000_000;

/// A scalar quantity painted onto the mesh, one value per vertex.
///
/// The renderer only ever sees the resulting array, which is the point: a cheap
/// closed form today and a batch FEA or CFD solve later produce the same thing,
/// so swapping the source changes nothing downstream. Values are in the field's
/// own unit; `range` is what gets mapped across the colormap, and anything
/// outside it clamps rather than wrapping, because a hotspot that wrapped to
/// "cold" would be a lie about the part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    /// What is being shown, e.g. "thermal" or "von mises".
    pub name: String,
    /// The unit the values carry, e.g. "C" or `MPa`. Displayed, never converted.
    #[serde(default)]
    pub unit: String,
    /// Value range mapped across the colormap, low to high.
    pub range: [f32; 2],
    /// Palette used for display; does not change physical values.
    #[serde(default)]
    pub colormap: Colormap,
    /// Imported samples or explicitly illustrative source.
    pub source: FieldSource,
}

/// How the per-vertex values are obtained.
///
/// Closed forms are evaluated while the mesh is baked, off the render loop, so
/// painting costs nothing per frame. A solved field will arrive as sampled data
/// and join this enum without the shader or the vertex format changing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FieldSource {
    /// Solver/closed-form results bound to the exact ordered STL vertices.
    /// No nearest-neighbor substitution or extrapolation is performed.
    VertexSamples {
        /// Exact ordered model-space vertex coordinates.
        positions: Vec<[f32; 3]>,
        /// One finite value per bound vertex, in declared units.
        values: Vec<f32>,
        /// Producer method, assumptions and boundary conditions.
        provenance: AnalysisProvenance,
    },
    /// Steady-state heat from a point, falling off with distance.
    ///
    /// `ambient + power / (1 + falloff * r^2)` — not physics, but the right
    /// shape for "this boss runs hot" at no cost.
    PointHeat {
        /// Model-space origin of the illustrative effect.
        at: [f32; 3],
        /// Nonnegative illustrative amplitude; not calibrated watts.
        power: f32,
        /// Illustrative baseline.
        #[serde(default = "default_ambient")]
        ambient: f32,
        /// Nonnegative illustrative spatial falloff.
        #[serde(default = "default_falloff")]
        falloff: f32,
    },
    /// Linear ramp along one axis, for a thermal gradient or a bending stress.
    Gradient {
        /// Coordinate axis, without implicit geometry normalization.
        axis: Axis,
        /// Value at coordinate zero.
        from: f32,
        /// Value at coordinate one.
        to: f32,
    },
    /// Distance from a point, scaled — a stand-in for stress concentration
    /// around a feature until a solver says otherwise.
    Radial {
        /// Illustrative origin in model coordinates.
        at: [f32; 3],
        /// Multiplier applied to distance.
        scale: f32,
    },
}

/// Numerical method declared by the producer; not certification by the renderer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisMethod {
    /// Producer declares a finite-element result.
    FiniteElement,
    /// Producer declares an analytical result.
    ClosedForm,
    /// Producer declares measured values.
    Measurement,
}

/// Auditable context travels with every imported analysis field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisProvenance {
    /// Method claimed by the producer.
    pub method: AnalysisMethod,
    /// Identifies the producer implementation.
    pub solver: String,
    /// Mesh/model dimension and bounded assumptions, e.g. 1D conduction.
    pub model: String,
    /// Explicit loading, constraints and boundary assumptions.
    pub boundary_conditions: String,
}

fn default_ambient() -> f32 {
    20.0
}

fn default_falloff() -> f32 {
    1.0
}

/// A coordinate axis in the producer model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// Model X.
    X,
    /// Model Y.
    Y,
    /// Model Z.
    Z,
}

impl Field {
    /// Evaluate the field at one point, in the field's own unit.
    pub fn sample(&self, position: [f32; 3]) -> f32 {
        match &self.source {
            FieldSource::VertexSamples {
                positions, values, ..
            } => positions
                .iter()
                .position(|p| *p == position)
                .and_then(|i| values.get(i))
                .copied()
                .unwrap_or(f32::NAN),
            FieldSource::PointHeat {
                at,
                power,
                ambient,
                falloff,
            } => {
                let squared = (0..3).map(|i| (position[i] - at[i]).powi(2)).sum::<f32>();
                ambient + power / (1.0 + falloff.max(0.0) * squared)
            }
            FieldSource::Gradient { axis, from, to } => {
                let value = position[axis.index()];
                from + (to - from) * value
            }
            FieldSource::Radial { at, scale } => {
                let squared = (0..3).map(|i| (position[i] - at[i]).powi(2)).sum::<f32>();
                squared.sqrt() * scale
            }
        }
    }

    /// Validate finite data, metadata and exact geometry binding before GPU upload.
    ///
    /// # Errors
    /// Rejects invalid metadata, positions, values, ranges and illustrative parameters.
    pub fn validate_vertices(&self, positions: &[[f32; 3]]) -> Result<(), FieldError> {
        if self.name.trim().is_empty() {
            return Err(FieldError::MissingName);
        }
        if !self.range.iter().all(|v| v.is_finite()) || self.range[1] < self.range[0] {
            return Err(FieldError::InvalidRange);
        }
        if positions.len() > MAX_FIELD_VERTICES
            || positions.is_empty()
            || positions.iter().flatten().any(|v| !v.is_finite())
        {
            return Err(FieldError::InvalidPositions);
        }
        match &self.source {
            FieldSource::VertexSamples {
                positions: bound,
                values,
                provenance,
            } => {
                if bound != positions || values.len() != positions.len() {
                    return Err(FieldError::GeometryMismatch);
                }
                if self.unit.trim().is_empty() {
                    return Err(FieldError::MissingUnits);
                }
                if [
                    &provenance.solver,
                    &provenance.model,
                    &provenance.boundary_conditions,
                ]
                .iter()
                .any(|s| s.trim().is_empty())
                {
                    return Err(FieldError::MissingProvenance);
                }
                if values.iter().any(|v| !v.is_finite()) {
                    return Err(FieldError::NonFiniteValues);
                }
            }
            FieldSource::PointHeat {
                at,
                power,
                ambient,
                falloff,
            } => {
                if at
                    .iter()
                    .chain([power, ambient, falloff])
                    .any(|v| !v.is_finite())
                    || *power < 0.
                    || *falloff < 0.
                {
                    return Err(FieldError::InvalidIllustration);
                }
            }
            FieldSource::Gradient { from, to, .. } => {
                if !from.is_finite() || !to.is_finite() {
                    return Err(FieldError::InvalidIllustration);
                }
            }
            FieldSource::Radial { at, scale } => {
                if at.iter().chain([scale]).any(|v| !v.is_finite()) {
                    return Err(FieldError::InvalidIllustration);
                }
            }
        }
        if !matches!(self.source, FieldSource::VertexSamples { .. })
            && positions.iter().any(|p| !self.sample(*p).is_finite())
        {
            return Err(FieldError::NonFiniteValues);
        }
        Ok(())
    }

    /// Validated scalar buffer in GPU order. This is display normalization, not a solve.
    ///
    /// # Errors
    /// Returns the same validation errors as [`Self::validate_vertices`].
    pub fn normalized_values(&self, positions: &[[f32; 3]]) -> Result<Vec<f32>, FieldError> {
        self.validate_vertices(positions)?;
        Ok(positions
            .iter()
            .enumerate()
            .map(|(i, p)| self.normalize(self.sample_vertex(i, *p)))
            .collect())
    }

    /// Metadata for a solved/measured field. `None` explicitly identifies an illustration.
    pub fn provenance(&self) -> Option<&AnalysisProvenance> {
        match &self.source {
            FieldSource::VertexSamples { provenance, .. } => Some(provenance),
            _ => None,
        }
    }

    /// Min/max and clipping counts for an honest legend on the selected geometry.
    ///
    /// # Errors
    /// Rejects invalid field data or geometry binding.
    pub fn summary(&self, positions: &[[f32; 3]]) -> Result<FieldSummary, FieldError> {
        self.validate_vertices(positions)?;
        let mut summary = FieldSummary {
            sample_count: positions.len(),
            minimum: f32::INFINITY,
            maximum: f32::NEG_INFINITY,
            below_range: 0,
            above_range: 0,
        };
        for (i, p) in positions.iter().enumerate() {
            let value = self.sample_vertex(i, *p);
            summary.minimum = summary.minimum.min(value);
            summary.maximum = summary.maximum.max(value);
            summary.below_range += usize::from(value < self.range[0]);
            summary.above_range += usize::from(value > self.range[1]);
        }
        Ok(summary)
    }

    /// Compare two scalar results at the same ordered vertices and in the same units.
    /// Does not infer physical equivalence or perform unit conversion.
    ///
    /// # Errors
    /// Rejects mismatched quantity/units, invalid fields and geometry bindings.
    pub fn compare(
        &self,
        reference: &Self,
        positions: &[[f32; 3]],
    ) -> Result<FieldComparison, FieldError> {
        self.validate_vertices(positions)?;
        reference.validate_vertices(positions)?;
        if self.name != reference.name || self.unit.trim().is_empty() || self.unit != reference.unit
        {
            return Err(FieldError::QuantityMismatch);
        }
        let mut maximum_absolute_error = 0.0_f64;
        let mut squares = 0.0;
        for (i, p) in positions.iter().enumerate() {
            let error =
                f64::from(self.sample_vertex(i, *p)) - f64::from(reference.sample_vertex(i, *p));
            maximum_absolute_error = maximum_absolute_error.max(error.abs());
            squares += error * error;
        }
        Ok(FieldComparison {
            sample_count: positions.len(),
            maximum_absolute_error,
            rms_error: (squares
                / f64::from(
                    u32::try_from(positions.len()).map_err(|_| FieldError::InvalidPositions)?,
                ))
            .sqrt(),
        })
    }

    /// Sample by validated vertex index without an expensive spatial search.
    pub fn sample_vertex(&self, index: usize, position: [f32; 3]) -> f32 {
        match &self.source {
            FieldSource::VertexSamples {
                positions, values, ..
            } => {
                if positions.get(index) != Some(&position) {
                    return f32::NAN;
                }
                values.get(index).copied().unwrap_or(f32::NAN)
            }
            _ => self.sample(position),
        }
    }

    /// Where a value sits across the colormap, clamped to 0..=1.
    ///
    /// A degenerate range collapses to the low end rather than dividing by
    /// zero: a flat field should read as uniformly cold, not as NaN.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "Palette coordinates are bounded to 0..=1 after f64 arithmetic"
    )]
    pub fn normalize(&self, value: f32) -> f32 {
        let [low, high] = self.range.map(f64::from);
        if low == high {
            return 0.0;
        }
        ((f64::from(value) - low) / (high - low)).clamp(0.0, 1.0) as f32
    }
}

impl Axis {
    /// Coordinate index in model space.
    pub const fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }
}

/// A field could not be safely presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FieldError {
    /// Empty display name.
    #[error("field needs a name")]
    MissingName,
    /// Non-finite or reversed limits.
    #[error("invalid field range")]
    InvalidRange,
    /// Empty or non-finite vertex data.
    #[error("analysis needs finite nonempty geometry")]
    InvalidPositions,
    /// Imported values belong to other geometry or ordering.
    #[error("analysis geometry binding mismatch")]
    GeometryMismatch,
    /// Quantitative data needs physical units.
    #[error("analysis field needs units")]
    MissingUnits,
    /// Solver/model/boundary conditions missing.
    #[error("analysis provenance is incomplete")]
    MissingProvenance,
    /// Samples or evaluated illustration overflowed.
    #[error("non-finite analysis values")]
    NonFiniteValues,
    /// Invalid parameters in a legacy illustrative source.
    #[error("invalid illustrative field parameters")]
    InvalidIllustration,
    /// Comparisons require the same quantity and units.
    #[error("analysis quantity or units mismatch")]
    QuantityMismatch,
}
/// Data bounds and clipped samples; range clipping never conceals these counts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FieldSummary {
    /// Number of geometry-bound samples.
    pub sample_count: usize,
    /// Observed minimum in the declared unit.
    pub minimum: f32,
    /// Observed maximum in the declared unit.
    pub maximum: f32,
    /// Samples below the palette range.
    pub below_range: usize,
    /// Samples above the palette range.
    pub above_range: usize,
}
/// Numeric visual comparison; no claim of solver validation or qualification.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FieldComparison {
    /// Number of paired vertices.
    pub sample_count: usize,
    /// Maximum absolute difference in the common field unit.
    pub maximum_absolute_error: f64,
    /// Root-mean-square difference in the common field unit.
    pub rms_error: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn field(values: Vec<f32>, range: [f32; 2]) -> Field {
        Field {
            name: "temperature".into(),
            unit: "C".into(),
            range,
            colormap: Colormap::Inferno,
            source: FieldSource::VertexSamples {
                positions: vec![[0., 0., 0.], [1., 0., 0.], [2., 0., 0.]],
                values,
                provenance: AnalysisProvenance {
                    method: AnalysisMethod::FiniteElement,
                    solver: "producer".into(),
                    model: "1D benchmark".into(),
                    boundary_conditions: "fixed ends".into(),
                },
            },
        }
    }
    const POSITIONS: [[f32; 3]; 3] = [[0., 0., 0.], [1., 0., 0.], [2., 0., 0.]];
    #[test]
    fn paired_fields_and_clipping_preserve_physical_values() {
        let a = field(vec![10., 30., 50.], [20., 40.]);
        assert_eq!(a.normalized_values(&POSITIONS).unwrap(), [0., 0.5, 1.]);
        let summary = a.summary(&POSITIONS).unwrap();
        assert_eq!(
            (
                summary.minimum,
                summary.maximum,
                summary.below_range,
                summary.above_range
            ),
            (10., 50., 1, 1)
        );
        let b = field(vec![11., 32., 52.], [20., 40.]);
        let error = a.compare(&b, &POSITIONS).unwrap();
        assert_eq!(error.maximum_absolute_error, 2.);
        assert_eq!(error.rms_error, 3.0_f64.sqrt());
        let mut different = b.clone();
        different.unit = "K".into();
        assert_eq!(
            a.compare(&different, &POSITIONS),
            Err(FieldError::QuantityMismatch)
        );
    }
    #[test]
    fn geometry_corruption_and_nonfinite_data_are_refused() {
        let a = field(vec![20., 30., 40.], [20., 40.]);
        let mut moved = POSITIONS;
        moved.swap(0, 1);
        assert_eq!(
            a.normalized_values(&moved),
            Err(FieldError::GeometryMismatch)
        );
        let mut corrupt = a.clone();
        if let FieldSource::VertexSamples { values, .. } = &mut corrupt.source {
            values[1] = f32::NAN;
        }
        assert_eq!(
            corrupt.normalized_values(&POSITIONS),
            Err(FieldError::NonFiniteValues)
        );
        assert_eq!(a.normalized_values(&[]), Err(FieldError::InvalidPositions));
        let mut infinite = POSITIONS;
        infinite[1][0] = f32::INFINITY;
        assert_eq!(
            a.normalized_values(&infinite),
            Err(FieldError::InvalidPositions)
        );
    }
    #[test]
    fn normalization_handles_full_float_range_and_sub_epsilon_spans() {
        let a = field(vec![-f32::MAX, 0., f32::MAX], [-f32::MAX, f32::MAX]);
        assert_eq!(a.normalized_values(&POSITIONS).unwrap(), [0., 0.5, 1.]);
        let b = field(vec![0., 1e-9, 2e-9], [0., 2e-9]);
        assert_eq!(b.normalized_values(&POSITIONS).unwrap(), [0., 0.5, 1.]);
        let flat = field(vec![20.; 3], [20., 20.]);
        assert_eq!(flat.normalized_values(&POSITIONS).unwrap(), [0.; 3]);
    }
    #[test]
    fn old_wire_format_and_illustration_identity_remain_explicit() {
        let json = r#"{"name":"illustrative","range":[0,1],"source":{"kind":"gradient","axis":"x","from":0,"to":1}}"#;
        let a: Field = serde_json::from_str(json).unwrap();
        assert!(a.provenance().is_none());
        assert_eq!(a.normalized_values(&POSITIONS).unwrap(), [0., 1., 1.]);
        assert_eq!(
            serde_json::from_value::<Field>(serde_json::to_value(&a).unwrap()).unwrap(),
            a
        );
        let mut invalid = a;
        invalid.source = FieldSource::PointHeat {
            at: [0.; 3],
            power: f32::INFINITY,
            ambient: 20.,
            falloff: 0.,
        };
        assert_eq!(
            invalid.normalized_values(&POSITIONS),
            Err(FieldError::InvalidIllustration)
        );
    }
}
