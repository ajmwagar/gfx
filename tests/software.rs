//! Deterministic software-raster fixture at the bounded 480x640 extent.
#![cfg(not(feature = "wgpu"))]
use fpl_gfx::software::{self, MAX_HEIGHT, MAX_WIDTH};
use fpl_gfx::{Primitive, Rect, Scene, Theme, ThemeRole};
use sha2::{Digest, Sha256};

/// A fixed 480x640 scene that is stable across runs so the emitted PNG and its
/// SHA-256 digest can be pinned in CI.
fn fixture_scene() -> Scene {
    let mut scene = Scene::with_capacity(4);
    scene.push(
        Primitive::rounded_rect(Rect::new(0.0, 0.0, 240.0, 120.0), 8.0, ThemeRole::Surface)
            .with_outline(ThemeRole::Outline, 0.0),
    );
    scene.push(Primitive::disc(
        Rect::new(60.0, 200.0, 160.0, 160.0),
        ThemeRole::Primary,
    ));
    scene.push(
        Primitive::rounded_rect(
            Rect::new(300.0, 400.0, 150.0, 200.0),
            0.0,
            ThemeRole::Danger,
        )
        .with_outline(ThemeRole::Outline, 0.0),
    );
    scene.push(Primitive::disc(
        Rect::new(-40.0, 500.0, 120.0, 120.0),
        ThemeRole::Success,
    ));
    scene
}

#[test]
fn fixture_is_bounded_and_deterministic() {
    let image = software::render(
        &fixture_scene(),
        &Theme::studio_dark(),
        MAX_WIDTH,
        MAX_HEIGHT,
    )
    .unwrap();
    assert_eq!((image.width, image.height), (MAX_WIDTH, MAX_HEIGHT));

    let mut hasher = Sha256::new();
    hasher.update(&image.pixels);
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();

    // Pin the fixture so any change to the rasterizer or theme is caught.
    assert_eq!(
        hex, "bcc00f14d8233e098822854956bde7997533c4660ed88d2345938ed31409a4b6",
        "software raster fixture changed; regenerate the expected digest",
    );

    let png = image.to_bgr24().unwrap();
    assert_eq!(png.len(), (MAX_WIDTH * MAX_HEIGHT * 3) as usize);
}

#[test]
fn writes_stable_png_asset() {
    let image = software::render(
        &fixture_scene(),
        &Theme::studio_dark(),
        MAX_WIDTH,
        MAX_HEIGHT,
    )
    .unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("software_fixture.png");
    let file = std::fs::File::create(&path).unwrap();
    let mut w = std::io::BufWriter::new(file);
    let mut enc = png::Encoder::new(&mut w, image.width, image.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&image.pixels)
        .unwrap();
    drop(w);
    assert!(path.exists());
    assert!(std::fs::metadata(&path).unwrap().len() > 0);
}
