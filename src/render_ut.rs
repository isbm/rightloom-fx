use image::{Rgba, RgbaImage};

use super::{ExportPolicy, RgbColor};

#[test]
fn default_export_policy_flattens_onto_black() {
    assert_eq!(
        ExportPolicy::default(),
        ExportPolicy::Flatten(RgbColor::BLACK)
    );

    let mut image = RgbaImage::from_pixel(1, 1, Rgba([160, 160, 160, 64]));

    ExportPolicy::default().apply_to(&mut image);

    assert_eq!(image.dimensions(), (1, 1));
    assert_eq!(image.get_pixel(0, 0).0, [40, 40, 40, 255]);
}

#[test]
fn transparent_pixels_become_the_background_color() {
    let mut image = RgbaImage::from_pixel(1, 1, Rgba([200, 100, 50, 0]));

    ExportPolicy::Flatten(RgbColor::new(12, 34, 56)).apply_to(&mut image);

    assert_eq!(image.get_pixel(0, 0).0, [12, 34, 56, 255]);
}

#[test]
fn opaque_pixels_keep_their_source_color() {
    let mut image = RgbaImage::from_pixel(1, 1, Rgba([17, 34, 51, 255]));

    ExportPolicy::Flatten(RgbColor::new(200, 180, 160)).apply_to(&mut image);

    assert_eq!(image.get_pixel(0, 0).0, [17, 34, 51, 255]);
}

#[test]
fn half_alpha_pixels_use_rounded_source_over_composition() {
    let mut image = RgbaImage::from_pixel(1, 1, Rgba([200, 100, 50, 128]));

    ExportPolicy::Flatten(RgbColor::new(20, 40, 60)).apply_to(&mut image);

    assert_eq!(image.get_pixel(0, 0).0, [110, 70, 55, 255]);
}

#[test]
fn preserve_alpha_leaves_pixels_and_dimensions_unchanged() {
    let mut image = RgbaImage::from_raw(2, 1, vec![12, 34, 56, 0, 160, 160, 160, 64])
        .expect("test image data should be valid");
    let expected = image.clone();

    ExportPolicy::PreserveAlpha.apply_to(&mut image);

    assert_eq!(image.dimensions(), (2, 1));
    assert_eq!(image, expected);
}
