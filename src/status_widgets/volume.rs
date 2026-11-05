//! Read-only volume marks. Hosts own grants, observations and overlay expiry.
use super::{Availability, Mark};
use crate::ThemeRole;

/// Confirmed read-only volume observation; never a command target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VolumeView {
    /// Confirmed measured level only, not a requested target.
    pub level: Option<u8>,
    /// Confirmed mute state; None never means unmuted.
    pub muted: Option<bool>,
    /// Host-supplied freshness and permission state.
    pub availability: Availability,
}
impl VolumeView {
    /// Reject levels outside the bounded percentage domain.
    pub fn validate(self) -> Result<(), String> {
        if self.level.is_some_and(|level| level > 100) {
            return Err("volume level must be 0..100".into());
        }
        Ok(())
    }
}
/// Emit an honest volume overlay, suppressing all nonfresh numbers.
pub fn visit(view: VolumeView, mut emit: impl FnMut(Mark)) -> Result<(), String> {
    view.validate()?;
    emit(Mark::Rect { bounds: [0.0,0.0,1.0,1.0], role: ThemeRole::Surface });
    let fresh = view.availability == Availability::Fresh;
    emit(Mark::Text { at: [0.04,0.04], text: if fresh { view.level.map_or_else(|| "Volume: —".into(), |level| format!("Volume: {level}%")) } else { "Volume unavailable".into() }, role: ThemeRole::Text });
    emit(Mark::Text { at: [0.04,0.44], text: if fresh { match view.muted { Some(true) => "Muted", Some(false) => "Unmuted", None => "Mute unknown" } } else { match view.availability { Availability::Stale => "Stale", Availability::Denied => "Denied", _ => "Unknown" } }.into(), role: ThemeRole::TextMuted });
    super::visit_progress(if fresh {view.level.map(|l| l as f32 / 100.0)} else {None}, emit);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nonfresh_hides_all_values() {
        for availability in [Availability::Unknown,Availability::Stale,Availability::Denied] {
            let mut cached = Vec::new();
            visit(VolumeView { level: Some(77), muted: Some(true), availability }, |m| cached.push(m)).unwrap();
            let mut empty = Vec::new();
            visit(VolumeView { level: None, muted: None, availability }, |m| empty.push(m)).unwrap();
            assert_eq!(cached, empty);
        }
        assert!(VolumeView {level: Some(101), ..Default::default()}.validate().is_err());
    }
}
