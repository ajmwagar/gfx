//! Shared shell chrome. Hosts own labels, hit testing and input, not visual states.
use crate::{Primitive, Rect, Scene, ShapeTheme, ThemeRole};

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
        Primitive::rounded_rect(bounds, 0.0, ThemeRole::SurfaceRecessed)
            .with_outline(ThemeRole::Outline, 0.0),
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Theme;
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
