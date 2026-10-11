//! Shared shell chrome. Hosts own labels, hit testing and input, not visual states.
use crate::{Primitive, Rect, Scene, ShapeTheme, ThemeRole};

/// Shared logical shell row height; platform hosts apply their own display scale.
pub const BAR_HEIGHT: u32 = 44;
/// Remote/keyboard shell controls occupy the entire logical row.
pub const BAR_CONTROL_HEIGHT: u32 = BAR_HEIGHT;
/// Compact shell label size in logical pixels, not physical display pixels.
pub const BAR_TEXT_SIZE: u32 = 14;
/// Outer shell row padding in logical pixels.
pub const BAR_PADDING: u32 = 8;

/// Orthogonal retained selection and transient interaction state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ControlState {
    /// Current workspace or active action.
    pub selected: bool,
    /// Keyboard, remote or pointer focus.
    pub focused: bool,
    /// Pointer/remote activation in progress.
    pub pressed: bool,
    /// Disabled controls never receive focus/activation emphasis.
    pub disabled: bool,
}

/// Whether an idle control has a face or blends into the shell bar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ControlAppearance {
    /// Persistent outlined control.
    #[default]
    Standard,
    /// Idle controls retain their hit target without visual clutter.
    Quiet,
}

/// Complete visual description shared by GPU and native adapters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlVisual {
    /// Canonical face geometry and color roles.
    pub face: Primitive,
    /// Label color role; disabled and idle labels remain legible but subordinate.
    pub ink: ThemeRole,
    /// Face opacity, independent of label opacity.
    pub face_opacity: f32,
}

/// Resolve visual hierarchy without owning input, layout or palette colors.
pub fn control_visual(
    bounds: Rect,
    shapes: ShapeTheme,
    state: ControlState,
    appearance: ControlAppearance,
) -> Option<ControlVisual> {
    let mut face = control_face(bounds, shapes, state)?;
    let engaged = !state.disabled && (state.selected || state.focused || state.pressed);
    if !state.disabled && state.pressed {
        face.fill = ThemeRole::SurfaceRecessed;
    }
    let hidden = appearance == ControlAppearance::Quiet && !engaged;
    if hidden {
        face.outline_width = 0.0;
    }
    Some(ControlVisual {
        face,
        ink: if engaged {
            ThemeRole::Text
        } else {
            ThemeRole::TextMuted
        },
        face_opacity: if hidden {
            0.0
        } else if state.disabled {
            0.45
        } else {
            1.0
        },
    })
}

/// Canonical control face for wGPU scenes and native primitive visitors.
/// Returns `false` without mutating the scene for invalid bounds or shape tokens.
pub fn control(scene: &mut Scene, bounds: Rect, shapes: ShapeTheme, state: ControlState) -> bool {
    let Some(face) = control_face(bounds, shapes, state) else {
        return false;
    };
    scene.push(face);
    true
}

/// Allocation-free control description for native styling adapters.
/// Returns `None` for invalid geometry or shape tokens.
pub fn control_face(bounds: Rect, shapes: ShapeTheme, state: ControlState) -> Option<Primitive> {
    if !bounds.is_valid()
        || bounds.width <= 0.0
        || bounds.height <= 0.0
        || bounds.width > 16384.0
        || bounds.height > 16384.0
        || !shapes.radius_small.is_finite()
        || shapes.radius_small < 0.0
        || !shapes.outline_width.is_finite()
        || shapes.outline_width < 0.0
    {
        return None;
    }
    let emphasis = !state.disabled && (state.focused || state.pressed);
    let active = !state.disabled && state.selected;
    Some(
        Primitive::rounded_rect(
            bounds,
            shapes
                .radius_small
                .min(bounds.width.min(bounds.height) * 0.5),
            if emphasis || active {
                ThemeRole::SurfaceRaised
            } else {
                ThemeRole::Surface
            },
        )
        .with_outline(
            if emphasis || active {
                ThemeRole::Primary
            } else {
                ThemeRole::Outline
            },
            shapes.outline_width * if emphasis { 2.0 } else { 1.0 },
        ),
    )
}

/// Continuous top-bar surface shared by every shell renderer.
pub fn bar(scene: &mut Scene, bounds: Rect) -> bool {
    if !bounds.is_valid() || bounds.width <= 0.0 || bounds.height <= 0.0 {
        return false;
    }
    scene.push(
        Primitive::rounded_rect(bounds, 0.0, ThemeRole::Surface)
            .with_outline(ThemeRole::Outline, 0.0),
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Theme;
    #[test]
    fn shell_bar_is_compact_without_shrinking_remote_targets() {
        assert_eq!(BAR_HEIGHT, 44);
        assert_eq!(BAR_CONTROL_HEIGHT, BAR_HEIGHT);
        assert!(BAR_TEXT_SIZE < BAR_CONTROL_HEIGHT / 2);
        assert!(f64::from(BAR_HEIGHT) / 540.0 < 0.09);
    }
    #[test]
    fn quiet_pressed_and_disabled_have_shared_visual_hierarchy() {
        let bounds = Rect::new(0.0, 0.0, 100.0, 32.0);
        let shapes = Theme::default().shapes;
        let idle = control_visual(
            bounds,
            shapes,
            ControlState::default(),
            ControlAppearance::Quiet,
        )
        .unwrap();
        assert_eq!(idle.face_opacity, 0.0);
        assert_eq!(idle.face.outline_width, 0.0);
        assert_eq!(idle.ink, ThemeRole::TextMuted);
        let pressed = control_visual(
            bounds,
            shapes,
            ControlState {
                pressed: true,
                ..Default::default()
            },
            ControlAppearance::Quiet,
        )
        .unwrap();
        assert_eq!(pressed.face.fill, ThemeRole::SurfaceRecessed);
        assert_eq!(pressed.ink, ThemeRole::Text);
        assert_eq!(pressed.face_opacity, 1.0);
        let disabled = control_visual(
            bounds,
            shapes,
            ControlState {
                disabled: true,
                selected: true,
                focused: true,
                pressed: true,
            },
            ControlAppearance::Standard,
        )
        .unwrap();
        assert_eq!(disabled.face.fill, ThemeRole::Surface);
        assert_eq!(disabled.ink, ThemeRole::TextMuted);
        assert_eq!(disabled.face_opacity, 0.45);
    }
    #[test]
    fn focus_selection_and_disabled_are_distinct() {
        let draw = |state| {
            let mut scene = Scene::default();
            assert!(control(
                &mut scene,
                Rect::new(0.0, 0.0, 100.0, 32.0),
                Theme::default().shapes,
                state
            ));
            let face = *scene.iter().next().unwrap();
            face
        };
        let idle = draw(ControlState::default());
        let selected = draw(ControlState {
            selected: true,
            ..Default::default()
        });
        let focus = draw(ControlState {
            focused: true,
            ..Default::default()
        });
        let disabled = draw(ControlState {
            disabled: true,
            focused: true,
            selected: true,
            pressed: true,
        });
        assert_eq!(idle, disabled);
        assert_eq!(selected.fill, ThemeRole::SurfaceRaised);
        assert_eq!(focus.outline_width, selected.outline_width * 2.0);
    }
    #[test]
    fn invalid_shapes_do_not_mutate_scene() {
        let mut scene = Scene::default();
        let mut shapes = Theme::default().shapes;
        shapes.radius_small = f32::NAN;
        assert!(!control(
            &mut scene,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            shapes,
            ControlState::default()
        ));
        assert_eq!(scene.iter().count(), 0);
    }
    #[test]
    fn native_face_and_scene_match_with_custom_shape_tokens() {
        let mut shapes = Theme::default().shapes;
        shapes.radius_small = 11.0;
        shapes.outline_width = 1.25;
        let bounds = Rect::new(0.0, 0.0, 120.0, 44.0);
        let state = ControlState {
            focused: true,
            ..Default::default()
        };
        let face = control_face(bounds, shapes, state).unwrap();
        assert_eq!(face.radius, 11.0);
        assert_eq!(face.outline_width, 2.5);
        let mut scene = Scene::default();
        assert!(control(&mut scene, bounds, shapes, state));
        assert_eq!(scene.iter().next(), Some(&face));
    }
}
