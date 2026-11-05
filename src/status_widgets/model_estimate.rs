//! Independently permissioned model estimate; never measured battery SOC.
use super::{Availability, Mark};
use crate::ThemeRole;

/// Host-retained eligible model output. Fresh is not an accuracy claim.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModelEstimateView {
    /// Estimated percent; never copied into measured BatteryView.
    pub percent: Option<u8>,
    /// Host-owned eligibility/expiry/permission classification.
    pub availability: Availability,
}
impl ModelEstimateView {
    /// Validate before atomically accepting an update.
    pub fn validate(self) -> Result<(), String> {
        if self.percent.is_some_and(|p| p > 100) {
            return Err("estimated percent must be 0..100".into());
        }
        Ok(())
    }
}
/// Emits a bounded model-only card. Every numeric output says Estimated.
pub fn visit(view: ModelEstimateView, mut emit: impl FnMut(Mark)) -> Result<(), String> {
    view.validate()?;
    emit(Mark::Rect {
        bounds: [0.0, 0.0, 1.0, 1.0],
        role: ThemeRole::Surface,
    });
    let text = match (view.availability, view.percent) {
        (Availability::Fresh, Some(p)) => format!("Estimated {p}%"),
        (Availability::Fresh, None) => "Estimated SOC unavailable".into(),
        (Availability::Unknown, _) => "Estimated unknown".into(),
        (Availability::Stale, _) => "Estimated stale".into(),
        (Availability::Denied, _) => "Estimated denied".into(),
    };
    emit(Mark::Text {
        at: [0.04, 0.05],
        text,
        role: ThemeRole::Text,
    });
    emit(Mark::Text {
        at: [0.04, 0.52],
        text: "Model estimate".into(),
        role: ThemeRole::TextMuted,
    });
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn label_and_nonfresh_suppression() {
        for availability in [
            Availability::Fresh,
            Availability::Unknown,
            Availability::Stale,
            Availability::Denied,
        ] {
            let mut marks = Vec::new();
            visit(
                ModelEstimateView {
                    percent: Some(57),
                    availability,
                },
                |m| marks.push(m),
            )
            .unwrap();
            let text = format!("{marks:?}");
            assert_eq!(text.contains("57"), availability == Availability::Fresh);
            assert!(
                marks
                    .iter()
                    .any(|m| matches!(m,Mark::Text{text,..} if text=="Model estimate"))
            );
            if availability == Availability::Fresh {
                assert!(text.contains("Estimated 57%"));
            }
        }
        let mut none_marks = Vec::new();
        visit(
            ModelEstimateView {
                percent: None,
                availability: Availability::Fresh,
            },
            |m| none_marks.push(m),
        )
        .unwrap();
        assert!(
            none_marks
                .iter()
                .any(|m| matches!(m,Mark::Text{text,..} if text=="Estimated SOC unavailable"))
        );
        assert!(
            ModelEstimateView {
                percent: Some(101),
                availability: Availability::Fresh
            }
            .validate()
            .is_err()
        );
    }
}
