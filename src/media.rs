//! Cover-first media shelf geometry. Hosts own artwork, text, catalog and playback.
use crate::{Color, Point, Primitive, Rect, Scene, ThemeRole};

/// Bounded maximum cards in a single shelf viewport.
pub const MAX_CARDS: usize = 24;
/// Separation between a shelf and its selected-record detail panel.
pub const DETAIL_GAP:f32=24.0;

/// One record-crate slot. Labels and cover textures remain host-owned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordCard {
    /// Selection/hit-test bounds, including space for a partially withdrawn disc.
    pub bounds: Rect,
    /// Square paper sleeve placement.
    pub sleeve: Rect,
    /// Disc placement, behind the sleeve.
    pub record: Rect,
}

/// Full-width, bounded crate grid shared by flat and spatial hosts.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordCrate {
    /// Visible slots in catalog order.
    pub cards: Vec<RecordCard>,
    /// Column count for deterministic remote navigation.
    pub columns: usize,
    /// Maximum slots that fit this viewport; hosts page using this value.
    pub capacity: usize,
}

impl RecordCrate {
    /// Derive square sleeves and reserved disc space; never fabricate overflow rows.
    pub fn layout(bounds: Rect, requested: usize) -> Option<Self> {
        if !bounds.x.is_finite()
            || !bounds.y.is_finite()
            || !bounds.width.is_finite()
            || !bounds.height.is_finite()
            || !bounds.right().is_finite()
            || !bounds.bottom().is_finite()
            || !(320.0..=16_384.0).contains(&bounds.width)
            || !(260.0..=16_384.0).contains(&bounds.height)
            || requested > MAX_CARDS
        {
            return None;
        }
        let gap = 28.0;
        let columns = ((bounds.width + gap) / 240.0).floor().max(1.0) as usize;
        let width = (bounds.width - gap * (columns - 1) as f32) / columns as f32;
        let sleeve = (width * 0.68).min(210.0);
        let height = sleeve + 76.0;
        let rows = ((bounds.height + gap) / (height + gap)).floor() as usize;
        let capacity = (rows * columns).min(MAX_CARDS);
        if capacity == 0 {
            return None;
        }
        let cards = (0..requested.min(capacity))
            .map(|index| {
                let x = bounds.x + (index % columns) as f32 * (width + gap);
                let y = bounds.y + (index / columns) as f32 * (height + gap);
                RecordCard {
                    bounds: Rect::new(x, y, width, height),
                    sleeve: Rect::new(x, y + 8.0, sleeve, sleeve),
                    record: Rect::new(x + width - sleeve, y, sleeve, sleeve),
                }
            })
            .collect();
        Some(Self {
            cards,
            columns,
            capacity,
        })
    }

    /// Procedural material layers. `reveal` is caller-owned motion, not playback.
    pub fn append_card(
        card: RecordCard,
        scene: &mut Scene,
        reveal: f32,
        rotation: f32,
        label: ThemeRole,
    ) -> bool {
        if !reveal.is_finite()
            || !rotation.is_finite()
            || !card.bounds.is_valid()
            || !card.sleeve.is_valid()
            || !card.record.is_valid()
        {
            return false;
        }
        let reveal = reveal.clamp(0.0, 1.0);
        let record = Rect::new(
            card.sleeve.x + (card.record.x - card.sleeve.x) * (0.22 + reveal * 0.78),
            card.record.y,
            card.record.width,
            card.record.height,
        );
        scene.push(Primitive::disc(
            Rect::new(record.x + 3.0, record.y + 5.0, record.width, record.height),
            ThemeRole::SurfaceRecessed,
        ));
        scene.push(Primitive::vinyl(record, label, rotation));
        scene.push(Primitive::rounded_rect(
            Rect::new(
                card.sleeve.x + 3.0,
                card.sleeve.y + 5.0,
                card.sleeve.width,
                card.sleeve.height,
            ),
            3.0,
            ThemeRole::SurfaceRecessed,
        ));
        scene.push(
            Primitive::rounded_rect(card.sleeve, 2.0, ThemeRole::SurfaceRaised)
                .with_gloss(0.16)
                .with_grain(0.002),
        );
        true
    }
}

/// Lightweight vector-host fallback for the vinyl material. The first three
/// radii are the outer disc, label and hole; remaining radii are resolved grooves.
/// GPU hosts should use [`Primitive::vinyl`] instead of tessellating these rings.
pub fn vinyl_radii(diameter: f32) -> Option<Vec<f32>> {
    if !diameter.is_finite() || !(32.0..=4096.0).contains(&diameter) {
        return None;
    }
    let radius = diameter * 0.5;
    let material = Primitive::vinyl(
        Rect::new(0.0, 0.0, diameter, diameter),
        ThemeRole::Primary,
        0.0,
    );
    let count = (radius * 0.22).floor().clamp(8.0, 64.0) as usize;
    let mut radii = Vec::with_capacity(count + 3);
    radii.extend([radius, radius * material.value, radius * material.radius]);
    radii.extend((0..count).map(|i| radius * (0.38 + 0.55 * i as f32 / count as f32)));
    Some(radii)
}

/// Derive a restrained accent from a host-decoded thumbnail (at most 64×64 RGBA pixels).
/// Ignores transparent, near-black and near-white pixels; returns no fabricated color
/// for monochrome artwork. Decode/downsample/cache belongs to the host, not GFX.
pub fn artwork_accent(rgba: &[u8]) -> Option<Color> {
    if rgba.is_empty() || rgba.len() % 4 != 0 || rgba.len() > 64 * 64 * 4 {
        return None;
    }
    let mut bins = [[0u32; 4]; 512];
    for pixel in rgba.chunks_exact(4) {
        let [r, g, b, a] = [pixel[0], pixel[1], pixel[2], pixel[3]];
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        if a < 192 || max < 48 || min > 224 || max - min < 24 {
            continue;
        }
        let index = ((r as usize >> 5) << 6) | ((g as usize >> 5) << 3) | (b as usize >> 5);
        let bin = &mut bins[index];
        bin[0] += r as u32;
        bin[1] += g as u32;
        bin[2] += b as u32;
        bin[3] += 1;
    }
    let bin = bins
        .iter()
        .filter(|bin| bin[3] > 0)
        .max_by_key(|bin| bin[3])?;
    Some(Color::from_srgb8(
        (bin[0] / bin[3]) as u8,
        (bin[1] / bin[3]) as u8,
        (bin[2] / bin[3]) as u8,
        255,
    ))
}

/// Host paint anchors; the artwork rectangle is a square, not a stretched thumbnail.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverCard {
    /// Hit-test and focus bounds.
    pub bounds: Rect,
    /// Host-owned decoded artwork placement.
    pub artwork: Rect,
    /// Title baseline anchor.
    pub title: Point,
    /// Artist baseline anchor.
    pub artist: Point,
}

/// A bounded cover grid with large TV-safe cards and a reserved detail pane.
#[derive(Debug, Clone, PartialEq)]
pub struct Shelf {
    /// Visible cards, in source order.
    pub cards: Vec<CoverCard>,
    /// Space for selected album and tracks; no overlap with the cover grid.
    pub detail: Rect,
}

impl Shelf {
    /// Derive geometry from host bounds. Invalid or cramped viewports fail explicitly.
    pub fn layout(bounds: Rect, count: usize) -> Option<Self> {
        if !bounds.x.is_finite()
            || !bounds.y.is_finite()
            || !bounds.width.is_finite()
            || !bounds.height.is_finite()
            || !bounds.right().is_finite()
            || !bounds.bottom().is_finite()
            || bounds.width > 16_384.0
            || bounds.height > 16_384.0
            || bounds.width < 640.0
            || bounds.height < 320.0
            || count > MAX_CARDS
        {
            return None;
        }
        let gap = DETAIL_GAP;
        let detail_width = (bounds.width * 0.30).max(220.0);
        let grid_width = bounds.width - detail_width - gap;
        let max_columns = ((grid_width + gap) / 204.0).floor().max(1.0) as usize;
        let (columns, card_width) = (1..=max_columns).find_map(|columns| {
            let width = (grid_width - gap * (columns - 1) as f32) / columns as f32;
            let rows = ((bounds.height + gap) / (width + 80.0 + gap)).floor() as usize;
            (count <= columns * rows).then_some((columns, width))
        })?;
        let cards = (0..count)
            .map(|index| {
                let x = bounds.x + (index % columns) as f32 * (card_width + gap);
                let y = bounds.y + (index / columns) as f32 * (card_width + 80.0 + gap);
                CoverCard {
                    bounds: Rect::new(x, y, card_width, card_width + 80.0),
                    artwork: Rect::new(x, y, card_width, card_width),
                    title: Point::new(x + 12.0, y + card_width + 30.0),
                    artist: Point::new(x + 12.0, y + card_width + 58.0),
                }
            })
            .collect();
        Some(Self {
            cards,
            detail: Rect::new(
                bounds.right() - detail_width,
                bounds.y,
                detail_width,
                bounds.height,
            ),
        })
    }

    /// Retained card chrome; labels/bitmaps are rendered by the host.
    pub fn append(&self, scene: &mut Scene, focused: Option<usize>, playing: Option<usize>) {
        for (index, card) in self.cards.iter().enumerate() {
            let selected = focused == Some(index);
            scene.push(
                Primitive::rounded_rect(card.bounds, 12.0, ThemeRole::SurfaceRaised).with_outline(
                    if selected {
                        ThemeRole::Primary
                    } else {
                        ThemeRole::Outline
                    },
                    if selected { 3.0 } else { 1.0 },
                ),
            );
            scene.push(Primitive::rounded_rect(
                card.artwork,
                10.0,
                ThemeRole::SurfaceRecessed,
            ));
            if playing == Some(index) {
                scene.push(Primitive::lamp(
                    Rect::new(
                        card.artwork.right() - 28.0,
                        card.artwork.y + 12.0,
                        16.0,
                        16.0,
                    ),
                    1.0,
                    ThemeRole::Success,
                ));
            }
        }
    }

    /// Pointer selection shares the exact painted geometry.
    pub fn hit(&self, point: Point) -> Option<usize> {
        self.cards.iter().position(|card| {
            point.x >= card.bounds.x
                && point.x < card.bounds.right()
                && point.y >= card.bounds.y
                && point.y < card.bounds.bottom()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crate_is_bounded_and_sleeves_are_square() {
        for bounds in [
            Rect::new(0.0, 0.0, 960.0, 540.0),
            Rect::new(20.0, 30.0, 1848.0, 900.0),
        ] {
            let grid = RecordCrate::layout(bounds, 24).unwrap();
            assert!(grid.cards.len() <= grid.capacity && grid.capacity <= MAX_CARDS);
            for card in grid.cards {
                assert_eq!(card.sleeve.width, card.sleeve.height);
                assert!(card.bounds.right() <= bounds.right());
                assert!(card.bounds.bottom() <= bounds.bottom());
            }
        }
        assert!(RecordCrate::layout(Rect::new(0.0, 0.0, f32::NAN, 900.0), 2).is_none());
        assert!(RecordCrate::layout(Rect::new(0.0, 0.0, 960.0, 540.0), 25).is_none());
    }
    #[test]
    fn vector_record_fallback_is_bounded_at_all_scales() {
        for size in [32.0, 128.0, 420.0, 4096.0] {
            let radii = vinyl_radii(size).unwrap();
            assert!(radii.len() <= 67);
            assert!(radii
                .iter()
                .all(|r| r.is_finite() && *r > 0.0 && *r <= size * 0.5));
        }
        assert!(vinyl_radii(f32::NAN).is_none());
        assert!(vinyl_radii(8192.0).is_none());
    }
    #[test]
    fn record_material_is_bounded_and_invalid_motion_is_atomic() {
        let card = RecordCrate::layout(Rect::new(0.0, 0.0, 960.0, 540.0), 1)
            .unwrap()
            .cards[0];
        let mut scene = Scene::default();
        assert!(!RecordCrate::append_card(
            card,
            &mut scene,
            f32::NAN,
            0.0,
            ThemeRole::Primary
        ));
        assert!(scene.is_empty());
        assert!(RecordCrate::append_card(
            card,
            &mut scene,
            1.0,
            0.5,
            ThemeRole::Primary
        ));
        assert!(scene.validate());
        assert_eq!(scene.len(), 4);
    }
    #[test]
    fn cards_are_square_and_detail_does_not_overlap() {
        let shelf = Shelf::layout(Rect::new(30.0, 40.0, 1920.0, 1080.0), 8).unwrap();
        for (index, card) in shelf.cards.iter().enumerate() {
            assert_eq!(card.artwork.width, card.artwork.height);
            assert!(card.bounds.right() < shelf.detail.x);
            assert_eq!(shelf.hit(card.title), Some(index));
        }
    }
    #[test]
    fn bad_viewports_and_overflow_are_rejected() {
        assert!(Shelf::layout(Rect::new(0.0, 0.0, f32::NAN, 1080.0), 1).is_none());
        assert!(Shelf::layout(Rect::new(0.0, 0.0, 1920.0, 1080.0), MAX_CARDS + 1).is_none());
        assert!(Shelf::layout(Rect::new(0.0, 0.0, 640.0, 320.0), 24).is_none());
        assert!(Shelf::layout(Rect::new(0.0, 0.0, f32::MAX, f32::MAX), 1).is_none());
    }
    #[test]
    fn accent_ignores_backdrop_and_rejects_unbounded_input() {
        let samples = [
            0, 0, 0, 255, 255, 255, 255, 255, 180, 50, 80, 255, 180, 50, 80, 255,
        ];
        assert_eq!(
            artwork_accent(&samples),
            Some(Color::from_srgb8(180, 50, 80, 255))
        );
        assert!(artwork_accent(&[100, 100, 100, 255]).is_none());
        assert!(artwork_accent(&[180, 50, 80, 0]).is_none());
        assert!(artwork_accent(&[0; 64 * 64 * 4 + 4]).is_none());
        assert!(artwork_accent(&[1, 2, 3]).is_none());
    }
}
