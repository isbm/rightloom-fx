use std::{
    fs,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicUsize, Ordering},
};

use image::RgbaImage;
use rand::{SeedableRng, rngs::StdRng};

use super::*;
use crate::render::{ExportPolicy, RenderError, RenderSettings, Resolution, RgbColor};

static TEST_OUTPUT_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TestOutputDir(PathBuf);

impl TestOutputDir {
    fn new() -> Self {
        let number = TEST_OUTPUT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!("rightloom-fx-burn-test-{}-{number}", process::id()));
        fs::create_dir_all(&path).expect("temporary output directory should be created");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestOutputDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn settings(density: u8, size: u8, blur: u8, lightness: u8, saturation: u8) -> BurnSettings {
    BurnSettings {
        render: RenderSettings {
            resolution: Resolution::new(480, 320).expect("test resolution should be valid"),
            density,
            amount: 1,
            outdir: "unused".into(),
            export_policy: ExportPolicy::default(),
        },
        size,
        blur,
        lightness,
        saturation,
    }
}

fn scene_with_seed(settings: &BurnSettings, seed: u64) -> BurnScene {
    let width = settings.render.resolution.width();
    let height = settings.render.resolution.height();
    let mut rng = StdRng::seed_from_u64(seed);
    generate_scene(settings, width, height, &mut rng)
}

fn render_with_seed(settings: &BurnSettings, seed: u64) -> RgbaImage {
    let mut rng = StdRng::seed_from_u64(seed);
    render_image(settings, &mut rng)
}

fn scene_coverage(scene: &BurnScene, width: u32, height: u32) -> f32 {
    let mut occupancy = BurnOccupancy::new(width, height);
    for field in &scene.light_fields {
        occupancy.mark_field(field);
    }
    occupancy.coverage()
}

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.001,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn burn_settings_validate_percentages() {
    let mut invalid = settings(50, 101, 70, 80, 85);
    assert!(matches!(
        invalid.validate(),
        Err(RenderError::InvalidSize(101))
    ));

    invalid.size = 70;
    invalid.blur = 101;
    assert!(matches!(
        invalid.validate(),
        Err(RenderError::InvalidBlur(101))
    ));

    invalid.blur = 70;
    invalid.lightness = 101;
    assert!(matches!(
        invalid.validate(),
        Err(RenderError::InvalidLightness(101))
    ));

    invalid.lightness = 80;
    invalid.saturation = 101;
    assert!(matches!(
        invalid.validate(),
        Err(RenderError::InvalidSaturation(101))
    ));
}

#[test]
fn generated_field_parameters_are_seeded_deterministic() {
    let settings = settings(50, 70, 70, 80, 85);
    let first = scene_with_seed(&settings, 31);
    let second = scene_with_seed(&settings, 31);

    assert_eq!(first.light_fields, second.light_fields);
    assert_eq!(first.burn_fields, second.burn_fields);
    assert_eq!(first.initial_light_count, second.initial_light_count);
    assert_eq!(first.coverage, second.coverage);
    assert!(
        first
            .light_fields
            .iter()
            .all(|field| field.kind == FieldKind::Light)
    );
    assert!(
        first
            .burn_fields
            .iter()
            .all(|field| field.kind == FieldKind::Burn)
    );
}

#[test]
fn seeded_rendering_is_deterministic() {
    let settings = settings(50, 70, 70, 80, 85);

    assert_eq!(
        render_with_seed(&settings, 31),
        render_with_seed(&settings, 31)
    );
}

#[test]
fn size_increases_characteristic_field_extent() {
    let short = characteristic_radius(10, 1_000);
    let broad = characteristic_radius(90, 1_000);

    assert!(broad > short);
    assert!(broad > 1_000.0);
    assert_eq!(characteristic_radius(100, 1_000), 1_500.0);
}

#[test]
fn blur_changes_transition_softness_without_changing_field_geometry() {
    let sharp = settings(50, 70, 0, 80, 85);
    let soft = settings(50, 70, 100, 80, 85);
    let sharp_scene = scene_with_seed(&sharp, 32);
    let soft_scene = scene_with_seed(&soft, 32);
    let field = &sharp_scene.light_fields[0];
    let distance = field.radius_x * field.boundary.scale(0.0) * 1.10;
    let x = field.center_x + distance * field.cos_angle;
    let y = field.center_y + distance * field.sin_angle;

    assert_eq!(sharp_scene.light_fields, soft_scene.light_fields);
    assert_eq!(sharp_scene.burn_fields, soft_scene.burn_fields);
    assert!(
        field.contribution(x, y, field_softness(100)) > field.contribution(x, y, field_softness(0))
    );
}

#[test]
fn generated_light_fields_can_originate_outside_the_canvas() {
    let settings = settings(50, 70, 70, 80, 85);
    let scene = scene_with_seed(&settings, 33);
    let width = settings.render.resolution.width() as f32;
    let height = settings.render.resolution.height() as f32;

    assert!(scene.light_fields.iter().any(|field| {
        field.center_x < 0.0
            || field.center_x > width
            || field.center_y < 0.0
            || field.center_y > height
    }));
}

#[test]
fn macro_deformation_stays_low_frequency_and_bounded() {
    let scene = scene_with_seed(&settings(50, 70, 70, 80, 85), 34);

    for field in scene.light_fields.iter().chain(&scene.burn_fields) {
        assert!((2..=6).contains(&field.boundary.values.len()));
        assert!(
            field
                .boundary
                .values
                .iter()
                .all(|value| value.abs() <= 0.18)
        );
        for index in 0..=360 {
            let scale = field
                .boundary
                .scale(index as f32 / 360.0 * std::f32::consts::TAU);
            assert!((0.82..=1.18).contains(&scale));
        }
    }
}

#[test]
fn palette_contains_warm_transition_cool_and_burn_families() {
    assert!(
        WARM_TO_COOL_CHAIN
            .iter()
            .any(|color| color.red > color.blue * 3.0)
    );
    assert!(
        WARM_TO_COOL_CHAIN
            .iter()
            .any(|color| color.green > color.red * 0.7 && color.green > color.blue * 0.7)
    );
    assert!(
        COOL_TO_WARM_CHAIN
            .iter()
            .any(|color| color.blue > color.red * 2.0)
    );
    assert!(BURN_PALETTE.iter().all(|color| color.red > color.green));
}

#[test]
fn saturation_zero_produces_grayscale_and_full_saturation_retains_color() {
    let grayscale = settings(50, 70, 70, 80, 0);
    let colorful = settings(50, 70, 70, 80, 100);
    let grayscale_image = render_with_seed(&grayscale, 35);
    let colorful_image = render_with_seed(&colorful, 35);

    assert!(
        grayscale_image
            .pixels()
            .all(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2])
    );
    assert!(
        colorful_image
            .pixels()
            .any(|pixel| { pixel[3] > 0 && (pixel[0] != pixel[1] || pixel[1] != pixel[2]) })
    );
    for (grayscale_pixel, colorful_pixel) in grayscale_image.pixels().zip(colorful_image.pixels()) {
        assert_eq!(grayscale_pixel[3], colorful_pixel[3]);
    }
}

#[test]
fn lightness_changes_rgb_without_changing_alpha() {
    let dim = settings(50, 70, 70, 20, 85);
    let bright = settings(50, 70, 70, 90, 85);
    let dim_image = render_with_seed(&dim, 36);
    let bright_image = render_with_seed(&bright, 36);

    for (dim_pixel, bright_pixel) in dim_image.pixels().zip(bright_image.pixels()) {
        assert_eq!(dim_pixel[3], bright_pixel[3]);
    }
    assert!(
        bright_image
            .pixels()
            .zip(dim_image.pixels())
            .any(|(bright_pixel, dim_pixel)| bright_pixel[0] > dim_pixel[0])
    );
}

#[test]
fn screen_accumulation_and_burn_attenuation_remain_bounded() {
    let mut color = [0.0; 3];
    for contribution in [0.20, 0.45, 0.80, 1.0] {
        screen_accumulate(&mut color, AMBER, contribution);
        assert!(color.iter().all(|channel| (0.0..=1.0).contains(channel)));
    }

    for strength in [0.0, 0.25, 0.75, 1.0] {
        for contribution in [0.0, 0.25, 0.75, 1.0] {
            assert!((0.0..=1.0).contains(&burn_attenuation(strength, contribution)));
        }
    }
}

#[test]
fn burn_fields_darken_existing_exposure() {
    let field = BurnField {
        kind: FieldKind::Burn,
        color: DARK_RED,
        center_x: 32.0,
        center_y: 32.0,
        radius_x: 24.0,
        radius_y: 24.0,
        sin_angle: 0.0,
        cos_angle: 1.0,
        intensity: 0.8,
        burn_strength: 0.8,
        boundary: BoundaryProfile {
            values: vec![0.0, 0.0],
        },
    };
    let mut exposure = vec![[0.8, 0.6, 0.4]; 64 * 64];
    let mut alpha = vec![0.5; 64 * 64];
    let before = exposure[32 * 64 + 32];

    field.rasterize_burn(&mut exposure, &mut alpha, 64, 64, 70);

    assert!(rgb_luminance(exposure[32 * 64 + 32]) < rgb_luminance(before));
}

#[test]
fn density_targets_are_monotonic_and_full_density_covers_the_grid() {
    let mut coverages = Vec::new();

    for density in [25, 50, 75, 100] {
        let settings = settings(density, 70, 70, 80, 85);
        let scene = scene_with_seed(&settings, 37);
        let measured = scene_coverage(
            &scene,
            settings.render.resolution.width(),
            settings.render.resolution.height(),
        );

        assert_close(measured, scene.coverage);
        assert!(measured >= coverage_target(density));
        coverages.push(measured);
    }

    assert!(coverages.windows(2).all(|pair| pair[0] <= pair[1]));
    assert!(coverages.last().is_some_and(|coverage| *coverage >= 0.99));
}

#[test]
fn larger_fields_generally_need_fewer_lights_for_the_same_density() {
    let small = scene_with_seed(&settings(75, 10, 70, 80, 85), 38);
    let large = scene_with_seed(&settings(75, 90, 70, 80, 85), 38);

    assert!(small.initial_light_count > large.initial_light_count);
}

#[test]
fn zero_density_is_transparent() {
    let image = render_with_seed(&settings(0, 70, 70, 80, 85), 39);

    assert!(image.pixels().all(|pixel| pixel.0 == [0, 0, 0, 0]));
}

#[test]
fn burn_export_uses_shared_alpha_and_background_policies() {
    const SEED: u64 = 40;
    let source_settings = settings(50, 70, 70, 80, 85);
    let source = render_with_seed(&source_settings, SEED);
    let output = TestOutputDir::new();
    let mut export_settings = source_settings.clone();
    export_settings.render.outdir = output.path().to_path_buf();
    export_settings.render.export_policy = ExportPolicy::Flatten(RgbColor::new(12, 34, 56));
    let mut rng = StdRng::seed_from_u64(SEED);

    generate_images_with_rng(&export_settings, &mut rng).expect("burn output should write");
    let exported = image::open(output.path().join("burn-0001.png"))
        .expect("burn output should be readable")
        .to_rgba8();

    for (source_pixel, exported_pixel) in source.pixels().zip(exported.pixels()) {
        for (channel, background) in [(0, 12u8), (1, 34u8), (2, 56u8)] {
            let alpha = u32::from(source_pixel[3]);
            let expected = ((u32::from(source_pixel[channel]) * alpha
                + u32::from(background) * (255 - alpha)
                + 127)
                / 255) as u8;
            assert_eq!(exported_pixel[channel], expected);
        }
        assert_eq!(exported_pixel[3], 255);
    }
}

#[test]
#[ignore = "writes seeded burn diagnostics under .tmp"]
fn writes_seeded_burn_diagnostics() {
    const SEED: u64 = 41;

    for (name, density, size, blur, lightness, saturation) in [
        ("burn-seeded-default", 50, 70, 70, 80, 85),
        ("burn-seeded-density-100", 100, 80, 75, 85, 90),
        ("burn-seeded-blur-20", 60, 70, 20, 80, 90),
        ("burn-saturation-0", 50, 70, 70, 80, 0),
        ("burn-saturation-100", 50, 70, 70, 80, 100),
    ] {
        let mut settings = settings(density, size, blur, lightness, saturation);
        settings.render.resolution = Resolution::from_aspect_ratio("3:2x1000")
            .expect("diagnostic resolution should be valid");
        settings.render.outdir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(name);
        let scene = scene_with_seed(&settings, SEED);
        println!(
            "{name}: {} light, {} burn, {:.3}% coverage",
            scene.light_fields.len(),
            scene.burn_fields.len(),
            scene.coverage * 100.0
        );
        let mut rng = StdRng::seed_from_u64(SEED);
        generate_images_with_rng(&settings, &mut rng).expect("diagnostic burn should write");
    }
}
