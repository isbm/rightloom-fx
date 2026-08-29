use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicUsize, Ordering},
};

use image::RgbaImage;
use rand::{Rng, SeedableRng, rngs::StdRng};

use super::{
    COVERAGE_TAIL_THRESHOLD, CoarseField, DEFAULT_CONTRAST, DensityField, DensityStructure,
    FieldBounds, MAX_TIDE_RELATIVE_MODULATION, SECOND_TIDE_LINE_PROBABILITY, Stain, StainSettings,
    TideMark, accumulate_scalar_effect, bounded_tide_contribution, choose_anchor,
    contrast_adjusted_density, contrast_gain, density_base_alpha, finishing_blur_radius_cells,
    generate_images_with_rng, generate_images_with_structure_contribution, lightness_luma,
    render_image, render_image_with_structure_contribution, scalar_effects_to_image, smoothstep,
    soft_outer_coverage, softness_normalized, stain_count, tide_presence_probability,
};
use crate::render::{
    ExportPolicy, RenderError, RenderSettings, Resolution, RgbColor, blend_gray_pixel,
};

static TEST_OUTPUT_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TestOutputDir(PathBuf);

impl TestOutputDir {
    fn new() -> Self {
        let number = TEST_OUTPUT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "rightloom-fx-stain-test-{}-{number}",
                process::id()
            ));
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

fn settings(density: u8, blur: u8, lightness: u8) -> StainSettings {
    settings_with_contrast(density, blur, lightness, DEFAULT_CONTRAST)
}

fn settings_with_contrast(density: u8, blur: u8, lightness: u8, contrast: u8) -> StainSettings {
    settings_with_resolution_and_contrast(640, 400, density, blur, lightness, contrast)
}

fn settings_with_resolution(
    width: u32,
    height: u32,
    density: u8,
    blur: u8,
    lightness: u8,
) -> StainSettings {
    settings_with_resolution_and_contrast(width, height, density, blur, lightness, DEFAULT_CONTRAST)
}

fn settings_with_resolution_and_contrast(
    width: u32,
    height: u32,
    density: u8,
    blur: u8,
    lightness: u8,
    contrast: u8,
) -> StainSettings {
    StainSettings {
        render: RenderSettings {
            resolution: Resolution::new(width, height).expect("test resolution should be valid"),
            density,
            amount: 1,
            outdir: "unused".into(),
            export_policy: ExportPolicy::default(),
        },
        blur,
        lightness,
        contrast,
    }
}

fn structure_diagnostic_settings(outdir_name: &str) -> StainSettings {
    StainSettings {
        render: RenderSettings {
            resolution: Resolution::from_aspect_ratio("3:2x6000")
                .expect("diagnostic resolution should be valid"),
            density: 100,
            amount: 2,
            outdir: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(".tmp")
                .join(outdir_name),
            export_policy: ExportPolicy::default(),
        },
        blur: 50,
        lightness: 100,
        contrast: DEFAULT_CONTRAST,
    }
}

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.001,
        "expected {expected}, got {actual}"
    );
}

fn alpha_statistics(image: &RgbaImage) -> (f64, u8) {
    let alpha_sum: u64 = image.pixels().map(|pixel| u64::from(pixel[3])).sum();
    let maximum = image
        .pixels()
        .map(|pixel| pixel[3])
        .max()
        .expect("test image should have pixels");

    (alpha_sum as f64 / image.pixels().len() as f64, maximum)
}

fn rasterized_effects(stain: &Stain, width: u32, height: u32, blur: u8) -> Vec<f32> {
    let mut effects = vec![0.0; width as usize * height as usize];
    stain.rasterize(&mut effects, width, height, blur);
    effects
}

fn exported_stain(export_policy: ExportPolicy) -> (RgbaImage, RgbaImage) {
    const SEED: u64 = 34;

    let source_settings = settings(45, 50, 50);
    let mut source_rng = StdRng::seed_from_u64(SEED);
    let source = render_image(&source_settings, &mut source_rng);
    let output = TestOutputDir::new();
    let mut export_settings = source_settings;
    export_settings.render.outdir = output.path().to_path_buf();
    export_settings.render.export_policy = export_policy;
    let mut export_rng = StdRng::seed_from_u64(SEED);

    generate_images_with_rng(&export_settings, &mut export_rng)
        .expect("stain output should write successfully");
    let exported = image::open(output.path().join("stain-0001.png"))
        .expect("stain output should be readable")
        .to_rgba8();

    (source, exported)
}

#[test]
fn scalar_effect_union_preserves_an_existing_effect_for_zero_contribution() {
    assert_close(accumulate_scalar_effect(0.37, 0.0), 0.37);
}

#[test]
fn scalar_effect_union_saturates_for_a_fully_opaque_contribution() {
    assert_close(accumulate_scalar_effect(0.37, 1.0), 1.0);
}

#[test]
fn scalar_effect_union_matches_the_expected_combined_opacity() {
    assert_close(accumulate_scalar_effect(0.2, 0.4), 0.52);
}

#[test]
fn scalar_effect_union_is_order_independent() {
    let contributions = [0.12, 0.38, 0.67, 0.25];
    let forward = contributions
        .into_iter()
        .fold(0.0, accumulate_scalar_effect);
    let reverse = contributions
        .into_iter()
        .rev()
        .fold(0.0, accumulate_scalar_effect);

    assert_close(forward, reverse);
}

#[test]
fn scalar_effect_union_stays_within_the_unit_interval() {
    let mut effect = 0.0;
    for contribution in [0.05, 0.25, 0.75, 1.0] {
        effect = accumulate_scalar_effect(effect, contribution);
        assert!((0.0..=1.0).contains(&effect));
    }
}

#[test]
fn scalar_effects_encode_as_white_with_the_accumulated_alpha() {
    let image = scalar_effects_to_image(&[0.0, 0.5, 1.0], 3, 1);

    assert_eq!(image.get_pixel(0, 0).0, [255, 255, 255, 0]);
    assert_eq!(image.get_pixel(1, 0).0, [255, 255, 255, 128]);
    assert_eq!(image.get_pixel(2, 0).0, [255, 255, 255, 255]);
}

#[test]
fn scalar_effects_avoid_repeated_source_over_rgb_renormalization() {
    let first_alpha = 128.0 / 255.0;
    let second_alpha = 128.0 / 255.0;
    let scalar_effect = accumulate_scalar_effect(
        accumulate_scalar_effect(0.0, first_alpha * 128.0 / 255.0),
        second_alpha,
    );
    let scalar = scalar_effects_to_image(&[scalar_effect], 1, 1);
    let mut source_over = RgbaImage::new(1, 1);

    blend_gray_pixel(&mut source_over, 0, 0, 128, 128);
    blend_gray_pixel(&mut source_over, 0, 0, 255, 128);

    let scalar_pixel = scalar.get_pixel(0, 0);
    assert_eq!(
        [scalar_pixel[0], scalar_pixel[1], scalar_pixel[2]],
        [255, 255, 255]
    );
    let source_over_pixel = source_over.get_pixel(0, 0);
    assert_ne!(
        [
            source_over_pixel[0],
            source_over_pixel[1],
            source_over_pixel[2]
        ],
        [255, 255, 255]
    );
    assert_ne!(scalar, source_over);
}

#[test]
fn default_stain_export_flattens_scalar_effects_onto_black() {
    let (source, exported) = exported_stain(ExportPolicy::default());

    for (source_pixel, exported_pixel) in source.pixels().zip(exported.pixels()) {
        for channel in 0..3 {
            let expected =
                ((u32::from(source_pixel[channel]) * u32::from(source_pixel[3]) + 127) / 255) as u8;
            assert_eq!(exported_pixel[channel], expected);
        }
        assert_eq!(exported_pixel[3], 255);
    }
}

#[test]
fn alpha_stain_export_preserves_transparency_and_source_pixels() {
    let (source, exported) = exported_stain(ExportPolicy::PreserveAlpha);

    assert_eq!(exported, source);
    assert!(exported.pixels().any(|pixel| pixel[3] == 0));
    assert!(exported.pixels().any(|pixel| pixel[3] > 0));
}

#[test]
fn background_stain_export_uses_the_requested_background_color() {
    let background = [17, 43, 89];
    let (source, exported) = exported_stain(ExportPolicy::Flatten(RgbColor::new(
        background[0],
        background[1],
        background[2],
    )));

    for (source_pixel, exported_pixel) in source.pixels().zip(exported.pixels()) {
        for channel in 0..3 {
            let alpha = u32::from(source_pixel[3]);
            let expected = ((u32::from(source_pixel[channel]) * alpha
                + u32::from(background[channel]) * (255 - alpha)
                + 127)
                / 255) as u8;
            assert_eq!(exported_pixel[channel], expected);
        }
        assert_eq!(exported_pixel[3], 255);
    }
}

fn tide_bounds() -> FieldBounds {
    FieldBounds {
        min_x: 0.0,
        max_x: 640.0,
        min_y: 0.0,
        max_y: 400.0,
    }
}

fn tide_presence_sequence(seed: u64, density: u8) -> Vec<bool> {
    let mut rng = StdRng::seed_from_u64(seed);

    (0..96)
        .map(|_| TideMark::new(tide_bounds(), 100.0, 0.1, density, &mut rng).is_some())
        .collect()
}

fn seeded_tide_mark(density: u8) -> TideMark {
    for seed in 0..1_000 {
        let mut rng = StdRng::seed_from_u64(seed);
        if let Some(tide) = TideMark::new(tide_bounds(), 100.0, 0.1, density, &mut rng) {
            return tide;
        }
    }

    panic!("a seeded tide mark should be found");
}

fn seeded_stain(seed: u64, density: u8, structures_enabled: bool) -> Stain {
    seeded_stain_with_contrast(seed, density, structures_enabled, DEFAULT_CONTRAST)
}

fn seeded_stain_with_contrast(
    seed: u64,
    density: u8,
    structures_enabled: bool,
    contrast: u8,
) -> Stain {
    let mut rng = StdRng::seed_from_u64(seed);

    Stain::new(
        (320.0, 200.0),
        100.0,
        density,
        density as f32 / 100.0,
        100,
        structures_enabled,
        &mut rng,
    )
    .with_contrast_gain(contrast_gain(contrast))
}

fn sampled_density_structures() -> Vec<DensityStructure> {
    let mut structures = Vec::new();

    for seed in 0..128 {
        structures.extend(seeded_stain(seed, 100, true).structures);
    }

    structures
}

fn first_seed_with_tide() -> u64 {
    for seed in 0..1_000 {
        if seeded_stain(seed, 100, true).tide.is_some() {
            return seed;
        }
    }

    panic!("a seeded high-density stain should contain a tide mark");
}

fn seeded_stain_with_tide() -> Stain {
    seeded_stain(first_seed_with_tide(), 100, true)
}

fn normalized_alpha_bounds(image: &RgbaImage, minimum_alpha: u8) -> (f32, f32, f32, f32) {
    let mut min_x = image.width();
    let mut max_x = 0;
    let mut min_y = image.height();
    let mut max_y = 0;

    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel[3] < minimum_alpha {
            continue;
        }

        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }

    assert!(min_x <= max_x && min_y <= max_y, "stain should be visible");
    (
        min_x as f32 / image.width() as f32,
        max_x as f32 / image.width() as f32,
        min_y as f32 / image.height() as f32,
        max_y as f32 / image.height() as f32,
    )
}

#[test]
fn density_alpha_calibration_points_match() {
    for (density, expected) in [
        (0, 0.0),
        (5, 18.0),
        (10, 35.0),
        (25, 75.0),
        (50, 145.0),
        (70, 195.0),
        (85, 225.0),
        (100, 250.0),
    ] {
        assert_close(density_base_alpha(density), expected);
    }
}

#[test]
fn density_alpha_interpolates_between_calibration_points() {
    for (density, expected) in [
        (2, 7.2),
        (7, 24.8),
        (15, 48.333_332),
        (40, 117.0),
        (60, 170.0),
        (77, 209.0),
        (92, 236.666_67),
    ] {
        assert_close(density_base_alpha(density), expected);
    }
}

#[test]
fn density_alpha_mapping_is_monotonic() {
    for density in 0..100 {
        assert!(
            density_base_alpha(density) <= density_base_alpha(density + 1),
            "density {density}"
        );
    }
}

#[test]
fn tide_presence_probability_uses_the_calibrated_monotonic_curve() {
    for (density, expected) in [
        (0, 0.0),
        (10, 0.05),
        (25, 0.08),
        (50, 0.12),
        (75, 0.16),
        (100, 0.20),
    ] {
        assert!(
            (tide_presence_probability(density) - expected).abs() < 0.000_001,
            "density {density}"
        );
    }

    for density in 0..100 {
        assert!(
            tide_presence_probability(density) <= tide_presence_probability(density + 1),
            "density {density}"
        );
    }
}

#[test]
fn tide_creation_is_deterministic_with_a_seeded_rng() {
    assert_eq!(
        tide_presence_sequence(27, 100),
        tide_presence_sequence(27, 100)
    );
}

#[test]
fn second_tide_line_is_disabled() {
    assert_eq!(SECOND_TIDE_LINE_PROBABILITY, 0.0);
    assert!(seeded_tide_mark(10).second_line.is_none());
    assert!(seeded_tide_mark(100).second_line.is_none());
}

#[test]
fn tide_marks_have_bounded_modulation() {
    let tide = seeded_tide_mark(100);
    assert!((0.02..=0.10).contains(&tide.strength));

    let mut maximum_density = 0.0_f32;
    for y in (0..=400).step_by(8) {
        for x in (0..=640).step_by(8) {
            maximum_density = maximum_density.max(tide.density_at(tide.center, x as f32, y as f32));
        }
    }
    assert!(maximum_density <= tide.strength + f32::EPSILON);

    assert_close(
        bounded_tide_contribution(0.10, 0.20),
        0.20 * MAX_TIDE_RELATIVE_MODULATION,
    );
    assert_close(bounded_tide_contribution(0.10, 1.35), 0.10);
}

#[test]
fn tide_construction_inside_seeded_stains_is_deterministic() {
    let seed = first_seed_with_tide();
    let stain = seeded_stain(seed, 100, true);
    let repeated_stain = seeded_stain(seed, 100, true);
    let tide = stain.tide.as_ref().expect("tide should exist");
    let repeated_tide = repeated_stain.tide.as_ref().expect("tide should exist");
    assert_eq!(tide.center, repeated_tide.center);
    assert_eq!(tide.width, repeated_tide.width);
    assert_eq!(tide.strength, repeated_tide.strength);
    assert_eq!(tide.presence_threshold, repeated_tide.presence_threshold);
    for (x, y) in [(160.0, 100.0), (320.0, 200.0), (480.0, 300.0)] {
        assert_eq!(
            tide.density_at(tide.center, x, y),
            repeated_tide.density_at(repeated_tide.center, x, y)
        );
    }
}

#[test]
fn normal_stains_exclude_tide_contribution() {
    let stain = seeded_stain_with_tide();
    let mut observed_tide_density = false;

    for y in (0..400).step_by(4) {
        for x in (0..640).step_by(4) {
            let world_x = x as f32 + 0.5;
            let world_y = y as f32 + 0.5;
            let local_density = stain.local_density_at(world_x, world_y);
            let tide_density = stain.tide_contribution_at(
                stain.warped_shape_at(world_x, world_y),
                world_x,
                world_y,
                local_density,
            );
            observed_tide_density |= tide_density > 0.0;
            let directional_density = stain.directional.as_ref().map_or(1.0, |directional| {
                directional.multiplier_at(world_x, world_y)
            });
            assert_close(
                stain.optical_density_at(stain.warped_shape_at(world_x, world_y), world_x, world_y),
                local_density * directional_density,
            );
        }
    }

    assert!(
        observed_tide_density,
        "seeded stain should retain a TideMark for RNG stability"
    );
}

#[test]
fn high_density_stain_keeps_a_broad_cloudy_body() {
    let mut rng = StdRng::seed_from_u64(28);
    let image = render_image(&settings(100, 50, 100), &mut rng);
    let strong_pixels = image.pixels().filter(|pixel| pixel[3] >= 64).count();
    let visible_alphas: BTreeSet<_> = image
        .pixels()
        .filter(|pixel| pixel[3] >= 16)
        .map(|pixel| pixel[3])
        .collect();

    assert!(
        strong_pixels > image.pixels().len() / 100,
        "high density should retain a broad strong body"
    );
    assert!(
        visible_alphas.len() > 16,
        "high density should retain cloudy internal variation"
    );
}

#[test]
fn lightness_calibration_points_match() {
    for (lightness, expected) in [
        (0, 0.0),
        (10, 30.0),
        (25, 100.0),
        (50, 190.0),
        (70, 230.0),
        (80, 245.0),
        (100, 255.0),
    ] {
        assert_close(lightness_luma(lightness), expected);
    }
}

#[test]
fn lightness_interpolates_between_calibration_points() {
    for (lightness, expected) in [
        (5, 15.0),
        (17, 62.666_668),
        (30, 118.0),
        (60, 210.0),
        (75, 237.5),
        (90, 250.0),
    ] {
        assert_close(lightness_luma(lightness), expected);
    }
}

#[test]
fn lightness_mapping_is_monotonic() {
    for lightness in 0..100 {
        assert!(
            lightness_luma(lightness) <= lightness_luma(lightness + 1),
            "lightness {lightness}"
        );
    }
}

#[test]
fn rendered_image_has_requested_dimensions() {
    let mut rng = StdRng::seed_from_u64(10);
    let image = render_image(&settings_with_resolution(960, 640, 30, 50, 10), &mut rng);

    assert_eq!(image.dimensions(), (960, 640));
}

#[test]
fn seeded_rendering_is_deterministic() {
    let mut first_rng = StdRng::seed_from_u64(22);
    let first = render_image(&settings(45, 80, 50), &mut first_rng);
    let mut second_rng = StdRng::seed_from_u64(22);
    let second = render_image(&settings(45, 80, 50), &mut second_rng);

    assert_eq!(first, second);
}

#[test]
fn default_contrast_is_neutral() {
    assert_eq!(DEFAULT_CONTRAST, 50);
    assert_close(contrast_gain(DEFAULT_CONTRAST), 1.0);
}

#[test]
fn contrast_gain_uses_the_requested_mapping() {
    assert_close(contrast_gain(0), 0.5);
    assert_close(contrast_gain(50), 1.0);
    assert_close(contrast_gain(100), 2.0);
}

#[test]
fn valid_contrast_values_are_accepted() {
    for contrast in [0, 50, 100] {
        assert!(
            settings_with_contrast(45, 50, 70, contrast)
                .validate()
                .is_ok()
        );
    }
}

#[test]
fn neutral_contrast_preserves_unadjusted_internal_density() {
    let stain = seeded_stain(30, 70, true);

    for (x, y) in [(160.0, 100.0), (320.0, 200.0), (480.0, 300.0)] {
        let shape = stain.warped_shape_at(x, y);
        assert_close(
            stain.optical_density_at(shape, x, y),
            stain.unadjusted_optical_density_at(shape, x, y),
        );
    }
}

#[test]
fn higher_contrast_increases_distance_from_the_internal_midpoint() {
    const MIDPOINT: f32 = 0.775;

    for density in [0.4, 1.15] {
        let flat = contrast_adjusted_density(density, contrast_gain(0));
        let neutral = contrast_adjusted_density(density, contrast_gain(50));
        let strong = contrast_adjusted_density(density, contrast_gain(100));

        assert!((flat - MIDPOINT).abs() < (neutral - MIDPOINT).abs());
        assert!((strong - MIDPOINT).abs() > (neutral - MIDPOINT).abs());
    }
}

#[test]
fn contrast_preserves_stain_geometry_and_rng_sequence() {
    let mut low_rng = StdRng::seed_from_u64(31);
    let low = Stain::new((320.0, 200.0), 100.0, 70, 0.70, 70, true, &mut low_rng)
        .with_contrast_gain(contrast_gain(0));
    let low_next = low_rng.random::<u64>();
    let mut high_rng = StdRng::seed_from_u64(31);
    let high = Stain::new((320.0, 200.0), 100.0, 70, 0.70, 70, true, &mut high_rng)
        .with_contrast_gain(contrast_gain(100));
    let high_next = high_rng.random::<u64>();

    assert_eq!(low_next, high_next);
    assert_eq!(low.lobes.len(), high.lobes.len());
    assert_eq!(low.min_x, high.min_x);
    assert_eq!(low.max_x, high.max_x);
    assert_eq!(low.min_y, high.min_y);
    assert_eq!(low.max_y, high.max_y);
    assert_eq!(low.characteristic_size, high.characteristic_size);
    assert_eq!(low.outline_strength, high.outline_strength);
    assert_eq!(low.feather, high.feather);
    assert_eq!(low.alpha, high.alpha);
    assert_eq!(low.shade, high.shade);

    for (low_lobe, high_lobe) in low.lobes.iter().zip(&high.lobes) {
        assert_eq!(low_lobe.center_x, high_lobe.center_x);
        assert_eq!(low_lobe.center_y, high_lobe.center_y);
        assert_eq!(low_lobe.radius_x, high_lobe.radius_x);
        assert_eq!(low_lobe.radius_y, high_lobe.radius_y);
        assert_eq!(low_lobe.sin_angle, high_lobe.sin_angle);
        assert_eq!(low_lobe.cos_angle, high_lobe.cos_angle);
    }

    let softness = softness_normalized(50);
    for (x, y) in [(160.0, 100.0), (320.0, 200.0), (480.0, 300.0)] {
        assert_eq!(low.warped_shape_at(x, y), high.warped_shape_at(x, y));
        assert_eq!(
            low.soft_outer_coverage_at(x, y, softness),
            high.soft_outer_coverage_at(x, y, softness)
        );
        assert_eq!(low.local_density_at(x, y), high.local_density_at(x, y));
    }

    let mut low_render_rng = StdRng::seed_from_u64(32);
    let low_render = render_image(&settings_with_contrast(70, 50, 70, 0), &mut low_render_rng);
    let low_render_next = low_render_rng.random::<u64>();
    let mut high_render_rng = StdRng::seed_from_u64(32);
    let high_render = render_image(
        &settings_with_contrast(70, 50, 70, 100),
        &mut high_render_rng,
    );
    let high_render_next = high_render_rng.random::<u64>();

    assert_eq!(low_render.dimensions(), high_render.dimensions());
    assert_eq!(low_render_next, high_render_next);
    assert_ne!(low_render, high_render);
}

#[test]
fn density_structure_strengths_are_non_negative() {
    let structures = sampled_density_structures();

    assert!(
        !structures.is_empty(),
        "seeded samples should include structures"
    );
    assert!(structures.iter().all(|structure| structure.strength >= 0.0));
}

#[test]
fn density_structure_strengths_are_bounded() {
    let structures = sampled_density_structures();

    assert!(
        structures
            .iter()
            .all(|structure| (0.03..=0.14).contains(&structure.strength))
    );
}

#[test]
fn density_structure_feathers_are_bounded() {
    let structures = sampled_density_structures();

    assert!(
        structures
            .iter()
            .all(|structure| (0.28..=0.55).contains(&structure.feather))
    );
}

#[test]
fn density_structure_outline_strengths_are_bounded() {
    let structures = sampled_density_structures();

    assert!(
        structures
            .iter()
            .all(|structure| (0.02..=0.07).contains(&structure.outline_strength))
    );
}

#[test]
fn density_structure_count_never_exceeds_two() {
    for density in [25, 69, 70, 100] {
        for seed in 0..128 {
            assert!(
                seeded_stain(seed, density, true).structures.len() <= 2,
                "density {density}, seed {seed}"
            );
        }
    }
}

#[test]
fn seeded_density_structure_generation_is_deterministic() {
    let mut observed_structure = false;

    for seed in 0..64 {
        let first = seeded_stain(seed, 100, true);
        let second = seeded_stain(seed, 100, true);
        assert_eq!(
            first.structures.len(),
            second.structures.len(),
            "seed {seed}"
        );

        for (first_structure, second_structure) in first.structures.iter().zip(&second.structures) {
            observed_structure = true;
            assert_eq!(first_structure.strength, second_structure.strength);
            assert_eq!(first_structure.feather, second_structure.feather);
            assert_eq!(
                first_structure.outline_strength,
                second_structure.outline_strength
            );
            for (x, y) in [(160.0, 100.0), (320.0, 200.0), (480.0, 300.0)] {
                assert_eq!(
                    first_structure.density_at(x, y),
                    second_structure.density_at(x, y),
                    "seed {seed}, point ({x}, {y})"
                );
            }
        }
    }

    assert!(
        observed_structure,
        "seeded samples should include structures"
    );
}

#[test]
fn density_structures_never_reduce_local_density() {
    const EPSILON: f32 = 0.000_001;

    for seed in 0..64 {
        let with_structures = seeded_stain(seed, 100, true);
        let without_structures = seeded_stain(seed, 100, false);
        assert_eq!(
            with_structures.structures.len(),
            without_structures.structures.len(),
            "seed {seed}"
        );

        for y in 0..=8 {
            for x in 0..=8 {
                let sample_x = with_structures.min_x
                    + (with_structures.max_x - with_structures.min_x) * x as f32 / 8.0;
                let sample_y = with_structures.min_y
                    + (with_structures.max_y - with_structures.min_y) * y as f32 / 8.0;
                assert!(
                    with_structures.local_density_at(sample_x, sample_y) + EPSILON
                        >= without_structures.local_density_at(sample_x, sample_y),
                    "seed {seed}, point ({sample_x}, {sample_y})"
                );
            }
        }
    }
}

#[test]
fn structure_contribution_switch_preserves_rng_sequence() {
    let settings = settings(100, 50, 100);
    let mut enabled_rng = StdRng::seed_from_u64(29);
    let enabled = render_image_with_structure_contribution(&settings, true, &mut enabled_rng);
    let enabled_next = enabled_rng.random::<u64>();
    let mut disabled_rng = StdRng::seed_from_u64(29);
    let disabled = render_image_with_structure_contribution(&settings, false, &mut disabled_rng);
    let disabled_next = disabled_rng.random::<u64>();

    assert_eq!(enabled.dimensions(), disabled.dimensions());
    assert_ne!(
        enabled, disabled,
        "structures should affect the enabled render"
    );
    assert_eq!(
        enabled_next, disabled_next,
        "the diagnostic switch must not alter RNG use"
    );
}

#[test]
#[ignore = "writes seeded DensityStructure comparison PNGs under .tmp"]
fn writes_seeded_structure_contribution_diagnostics() {
    const SEED: u64 = 29;

    for (outdir_name, structures_enabled) in
        [("structure-enabled", true), ("structure-disabled", false)]
    {
        let settings = structure_diagnostic_settings(outdir_name);
        let mut rng = StdRng::seed_from_u64(SEED);
        generate_images_with_structure_contribution(&settings, structures_enabled, &mut rng)
            .expect("diagnostic images should write successfully");
    }
}

#[test]
#[ignore = "writes seeded single-stain and composite coverage PNGs under .tmp"]
fn writes_seeded_continuous_coverage_component_diagnostics() {
    const WIDTH: u32 = 1000;
    const HEIGHT: u32 = 667;
    const DENSITY: u8 = 100;
    const SEED: u64 = 41;

    let outdir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".tmp")
        .join("continuous-coverage-components");
    fs::create_dir_all(&outdir).expect("diagnostic output directory should be created");

    let density_scale = f32::from(DENSITY) / 100.0;
    let smallest_dimension = WIDTH.min(HEIGHT) as f32;
    let mut rng = StdRng::seed_from_u64(SEED);
    let mut anchors = Vec::new();
    let mut composite_effects = vec![0.0; WIDTH as usize * HEIGHT as usize];

    for index in 0..stain_count(DENSITY, &mut rng) {
        let base_radius = smallest_dimension
            * (0.04 + 0.12 * density_scale.sqrt())
            * rng.random_range(0.78..1.18);
        let anchor = choose_anchor(
            WIDTH,
            HEIGHT,
            base_radius,
            &anchors,
            density_scale,
            &mut rng,
        );
        let stain = Stain::new(
            anchor,
            base_radius,
            DENSITY,
            density_scale,
            100,
            true,
            &mut rng,
        );
        let component_effects = rasterized_effects(&stain, WIDTH, HEIGHT, 50);
        scalar_effects_to_image(&component_effects, WIDTH, HEIGHT)
            .save(outdir.join(format!("component-{index:02}.png")))
            .expect("component diagnostic should write");
        stain.rasterize(&mut composite_effects, WIDTH, HEIGHT, 50);
        anchors.push(anchor);
    }

    scalar_effects_to_image(&composite_effects, WIDTH, HEIGHT)
        .save(outdir.join("composite.png"))
        .expect("composite diagnostic should write");
}

#[test]
fn seeded_stain_preserves_normalized_macro_structure_across_resolutions() {
    let mut small_rng = StdRng::seed_from_u64(23);
    let small_stain = Stain::new((400.0, 300.0), 100.0, 45, 0.45, 50, true, &mut small_rng);
    let mut large_rng = StdRng::seed_from_u64(23);
    let large_stain = Stain::new((800.0, 600.0), 200.0, 45, 0.45, 50, true, &mut large_rng);

    assert_eq!(small_stain.lobes.len(), large_stain.lobes.len());
    for (small_lobe, large_lobe) in small_stain.lobes.iter().zip(&large_stain.lobes) {
        assert!((small_lobe.center_x / 800.0 - large_lobe.center_x / 1600.0).abs() < 0.0001);
        assert!((small_lobe.center_y / 600.0 - large_lobe.center_y / 1200.0).abs() < 0.0001);
        assert!((small_lobe.radius_x / 800.0 - large_lobe.radius_x / 1600.0).abs() < 0.0001);
        assert!((small_lobe.radius_y / 600.0 - large_lobe.radius_y / 1200.0).abs() < 0.0001);
    }

    let small_effects = rasterized_effects(&small_stain, 800, 600, 80);
    let small_image = scalar_effects_to_image(&small_effects, 800, 600);
    let large_effects = rasterized_effects(&large_stain, 1600, 1200, 80);
    let large_image = scalar_effects_to_image(&large_effects, 1600, 1200);

    let small_bounds = normalized_alpha_bounds(&small_image, 4);
    let large_bounds = normalized_alpha_bounds(&large_image, 4);
    for (small, large) in [
        small_bounds.0,
        small_bounds.1,
        small_bounds.2,
        small_bounds.3,
    ]
    .into_iter()
    .zip([
        large_bounds.0,
        large_bounds.1,
        large_bounds.2,
        large_bounds.3,
    ]) {
        assert!((small - large).abs() < 0.025);
    }
}

#[test]
fn internal_density_has_many_distinct_final_resolution_values() {
    let mut rng = StdRng::seed_from_u64(24);
    let stain = Stain::new((320.0, 200.0), 100.0, 45, 0.45, 50, true, &mut rng);
    let lobe = stain.lobes[0];
    let mut densities = BTreeSet::new();

    for offset_y in -24..=24 {
        for offset_x in -24..=24 {
            let x = lobe.center_x + offset_x as f32 + 0.5;
            let y = lobe.center_y + offset_y as f32 + 0.5;
            let shape = stain.warped_shape_at(x, y);
            if shape > 0.0 {
                densities
                    .insert((stain.optical_density_at(shape, x, y) * 100_000.0).round() as i32);
            }
        }
    }

    assert!(
        densities.len() > 64,
        "final-resolution density should retain broad continuous variation"
    );
}

#[test]
fn density_field_does_not_repeat_flat_control_cells() {
    let width = 6;
    let height = 6;
    let mut values = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let is_flat_center = (2..=3).contains(&x) && (2..=3).contains(&y);
            values.push(if is_flat_center {
                0.0
            } else {
                (x as f32 - 2.5) * 0.4 + (y as f32 - 2.5) * 0.2
            });
        }
    }
    let field = DensityField {
        control: CoarseField {
            values,
            width,
            height,
            origin_x: 0.0,
            origin_y: 0.0,
            cell_size: 16.0,
        },
    };
    let mut samples = BTreeSet::new();

    for y in 32..48 {
        for x in 32..48 {
            samples.insert(
                (field.sample(x as f32 + 0.5, y as f32 + 0.5) * 1_000_000.0).round() as i32,
            );
        }
    }

    assert!(
        samples.len() > 64,
        "density should vary inside a control cell instead of creating a repeated rectangle"
    );
}

#[test]
fn density_sampling_does_not_consume_rng() {
    let mut sampled_rng = StdRng::seed_from_u64(25);
    let sampled_field = DensityField::new(0.0, 256.0, 0.0, 256.0, 16.0, &mut sampled_rng);
    for y in 0..64 {
        for x in 0..64 {
            let _ = sampled_field.sample(x as f32 + 0.5, y as f32 + 0.5);
        }
    }
    let sampled_next = sampled_rng.random::<u64>();

    let mut untouched_rng = StdRng::seed_from_u64(25);
    let _untouched_field = DensityField::new(0.0, 256.0, 0.0, 256.0, 16.0, &mut untouched_rng);
    let untouched_next = untouched_rng.random::<u64>();

    assert_eq!(sampled_next, untouched_next);
}

#[test]
fn zero_density_is_transparent() {
    for (seed, blur) in [(11, 0), (12, 100)] {
        let mut rng = StdRng::seed_from_u64(seed);
        let image = render_image(&settings(0, blur, 10), &mut rng);

        assert!(image.pixels().all(|pixel| pixel.0 == [255, 255, 255, 0]));
    }
}

#[test]
fn nonzero_density_modifies_monochrome_pixels() {
    let mut rng = StdRng::seed_from_u64(12);
    let image = render_image(&settings(30, 50, 10), &mut rng);

    assert!(image.pixels().any(|pixel| pixel[3] > 0));
    assert!(
        image
            .pixels()
            .all(|pixel| pixel[0] == 255 && pixel[1] == 255 && pixel[2] == 255)
    );
}

#[test]
fn density_70_has_substantially_higher_mean_and_peak_alpha_than_density_25() {
    let mut lower_density_rng = StdRng::seed_from_u64(26);
    let lower_density = render_image(&settings(25, 50, 80), &mut lower_density_rng);
    let mut higher_density_rng = StdRng::seed_from_u64(26);
    let higher_density = render_image(&settings(70, 50, 80), &mut higher_density_rng);

    let (lower_mean, lower_peak) = alpha_statistics(&lower_density);
    let (higher_mean, higher_peak) = alpha_statistics(&higher_density);

    assert!(
        higher_mean > lower_mean * 1.5,
        "density 70 should have substantially higher mean alpha ({higher_mean} vs {lower_mean})"
    );
    assert!(
        higher_peak > lower_peak,
        "density 70 should have a higher peak alpha ({higher_peak} vs {lower_peak})"
    );
}

#[test]
fn generated_alpha_has_broad_density_variation() {
    let mut rng = StdRng::seed_from_u64(13);
    let image = render_image(&settings(45, 0, 100), &mut rng);
    let alphas: BTreeSet<_> = image
        .pixels()
        .filter(|pixel| pixel[3] > 0)
        .map(|pixel| pixel[3])
        .collect();
    let visible_alphas: BTreeSet<_> = alphas.iter().copied().filter(|alpha| *alpha >= 8).collect();

    assert!(alphas.len() > 8, "stains should have varied opacity");
    assert!(
        *visible_alphas.last().expect("stains should be visible")
            - *visible_alphas.first().expect("stains should be visible")
            >= 8,
        "visible stain regions should have broad density variation"
    );

    assert!(
        visible_alphas.len() > 8,
        "high-lightness stains should retain visible internal variation"
    );
}

#[test]
fn low_density_keeps_most_of_the_canvas_transparent() {
    let mut rng = StdRng::seed_from_u64(14);
    let image = render_image(&settings(5, 50, 10), &mut rng);
    let transparent = image.pixels().filter(|pixel| pixel[3] == 0).count();

    assert!(transparent * 100 / image.pixels().len() >= 70);
}

#[test]
fn invalid_blur_is_rejected_before_output_is_created() {
    let mut rng = StdRng::seed_from_u64(15);
    let error = generate_images_with_rng(&settings(30, 101, 10), &mut rng)
        .expect_err("out-of-range blur should fail validation");

    assert!(matches!(error, RenderError::InvalidBlur(101)));
}

#[test]
fn increasing_blur_broadens_the_transition() {
    let mut hard_rng = StdRng::seed_from_u64(16);
    let hard = render_image(&settings(45, 0, 10), &mut hard_rng);
    let mut blur_25_rng = StdRng::seed_from_u64(16);
    let blur_25 = render_image(&settings(45, 25, 10), &mut blur_25_rng);
    let mut blur_50_rng = StdRng::seed_from_u64(16);
    let blur_50 = render_image(&settings(45, 50, 10), &mut blur_50_rng);
    let mut blur_75_rng = StdRng::seed_from_u64(16);
    let blur_75 = render_image(&settings(45, 75, 10), &mut blur_75_rng);
    let mut blur_100_rng = StdRng::seed_from_u64(16);
    let blur_100 = render_image(&settings(45, 100, 10), &mut blur_100_rng);

    let footprint = |image: &image::RgbaImage| image.pixels().filter(|pixel| pixel[3] > 0).count();
    let low_alpha = |image: &image::RgbaImage| {
        image
            .pixels()
            .filter(|pixel| (1..=24).contains(&pixel[3]))
            .count()
    };
    let alpha_sum =
        |image: &image::RgbaImage| image.pixels().map(|pixel| u64::from(pixel[3])).sum::<u64>();

    assert!(footprint(&blur_25) > footprint(&hard));
    assert!(footprint(&blur_50) > footprint(&blur_25));
    assert!(footprint(&blur_75) > footprint(&blur_50));
    assert!(footprint(&blur_100) > footprint(&blur_75));
    assert!(low_alpha(&blur_100) > low_alpha(&hard));
    assert!(alpha_sum(&blur_100) * 100 >= alpha_sum(&hard) * 60);
}

#[test]
fn blur_zero_uses_the_existing_hard_rasterization_path() {
    let stain = seeded_stain(17, 45, true);
    let mut dispatched = vec![0.0; 640 * 400];
    let mut hard = vec![0.0; 640 * 400];

    stain.rasterize(&mut dispatched, 640, 400, 0);
    stain.rasterize_hard(&mut hard, 640, 400);

    assert_eq!(dispatched, hard);
}

#[test]
fn soft_outer_coverage_is_continuous_at_the_boundary() {
    let softness = softness_normalized(50);
    let boundary = soft_outer_coverage(0.0, softness);
    let just_inside = soft_outer_coverage(0.001, softness);
    let just_outside = soft_outer_coverage(-0.001, softness);

    assert_close(boundary, 1.0);
    assert_close(just_inside, boundary);
    assert!(
        just_outside > 0.999,
        "coverage should remain significant outside"
    );
    assert!(just_outside <= boundary);
}

#[test]
fn soft_outer_coverage_has_a_monotonic_gaussian_tail() {
    let softness = softness_normalized(50);
    let near = soft_outer_coverage(-softness * 0.25, softness);
    let middle = soft_outer_coverage(-softness, softness);
    let far = soft_outer_coverage(-softness * 4.0, softness);

    assert!(near > middle && middle > far && far > 0.0);
    assert!(far < COVERAGE_TAIL_THRESHOLD);
}

#[test]
fn larger_blur_widens_the_same_continuous_outer_tail() {
    let outside_shape = -0.20;
    let blur_25 = soft_outer_coverage(outside_shape, softness_normalized(25));
    let blur_50 = soft_outer_coverage(outside_shape, softness_normalized(50));
    let blur_100 = soft_outer_coverage(outside_shape, softness_normalized(100));

    assert!(blur_100 > blur_50 && blur_50 > blur_25);
}

#[test]
fn continuous_coverage_uses_a_quality_bounded_field_and_mild_smoothing() {
    let stain = seeded_stain(17, 45, true);
    let softness = softness_normalized(50);
    let softness_pixels = stain.characteristic_size * softness;
    let expected_cell_size = (stain.characteristic_size / 256.0).clamp(1.0, 8.0);
    let field = stain.diffused_outer_coverage(50);
    let radius = finishing_blur_radius_cells(softness_pixels, expected_cell_size);
    let lobe = stain.lobes[0];

    assert_close(field.cell_size, expected_cell_size);
    assert!((1..=6).contains(&radius));
    assert!(
        field.sample(lobe.center_x, lobe.center_y) > 0.95,
        "one finishing pass should preserve the stain core"
    );
}

#[test]
fn diffused_rasterization_uses_the_continuous_outer_coverage_field() {
    let stain = seeded_stain(17, 45, true);
    let field = stain.diffused_outer_coverage(50);
    let mut effects = vec![0.0; 640 * 400];
    stain.rasterize_diffused(&mut effects, 640, 400, 50);

    let mut sample = None;
    'rows: for y in 0..400 {
        for x in 0..640 {
            let world_x = x as f32 + 0.5;
            let world_y = y as f32 + 0.5;
            let warped_shape = stain.warped_shape_at(world_x, world_y);
            let coverage = field.sample(world_x, world_y).clamp(0.0, 1.0);
            if warped_shape > -stain.feather || coverage <= COVERAGE_TAIL_THRESHOLD {
                continue;
            }

            let expected_effect = ((stain.alpha / 255.0)
                * coverage
                * stain.optical_density_at(warped_shape, world_x, world_y))
            .clamp(0.0, 1.0)
                * f32::from(stain.shade)
                / 255.0;
            if expected_effect > 0.0 {
                sample = Some((x, y, warped_shape, expected_effect));
                break 'rows;
            }
        }
    }

    let (x, y, warped_shape, expected_effect) =
        sample.expect("continuous coverage should remain outside the old hard mask");
    assert_eq!(smoothstep(-stain.feather, stain.feather, warped_shape), 0.0);
    assert_close(effects[y as usize * 640 + x as usize], expected_effect);
}

#[test]
fn invalid_lightness_is_rejected_before_output_is_created() {
    let mut rng = StdRng::seed_from_u64(18);
    let error = generate_images_with_rng(&settings(30, 50, 101), &mut rng)
        .expect_err("out-of-range lightness should fail validation");

    assert!(matches!(error, RenderError::InvalidLightness(101)));
}

#[test]
fn invalid_contrast_is_rejected_before_output_is_created() {
    let mut rng = StdRng::seed_from_u64(33);
    let error = generate_images_with_rng(&settings_with_contrast(30, 50, 50, 101), &mut rng)
        .expect_err("out-of-range contrast should fail validation");

    assert!(matches!(error, RenderError::InvalidContrast(101)));
}

#[test]
fn lightness_preserves_stain_geometry_and_coverage() {
    let mut low_rng = StdRng::seed_from_u64(19);
    let low = Stain::new((320.0, 200.0), 100.0, 45, 0.45, 10, true, &mut low_rng);
    let low_next = low_rng.random::<u64>();
    let mut mid_rng = StdRng::seed_from_u64(19);
    let mid = Stain::new((320.0, 200.0), 100.0, 45, 0.45, 50, true, &mut mid_rng);
    let mid_next = mid_rng.random::<u64>();
    let mut high_rng = StdRng::seed_from_u64(19);
    let high = Stain::new((320.0, 200.0), 100.0, 45, 0.45, 100, true, &mut high_rng);
    let high_next = high_rng.random::<u64>();

    assert_eq!(low_next, mid_next);
    assert_eq!(mid_next, high_next);
    assert_eq!(low.lobes.len(), mid.lobes.len());
    assert_eq!(mid.lobes.len(), high.lobes.len());
    assert_eq!(low.min_x, mid.min_x);
    assert_eq!(mid.min_x, high.min_x);
    assert_eq!(low.max_x, mid.max_x);
    assert_eq!(mid.max_x, high.max_x);
    assert_eq!(low.min_y, mid.min_y);
    assert_eq!(mid.min_y, high.min_y);
    assert_eq!(low.max_y, mid.max_y);
    assert_eq!(mid.max_y, high.max_y);

    let softness = softness_normalized(80);
    for (x, y) in [(160.0, 100.0), (320.0, 200.0), (480.0, 300.0)] {
        assert_eq!(low.warped_shape_at(x, y), mid.warped_shape_at(x, y));
        assert_eq!(mid.warped_shape_at(x, y), high.warped_shape_at(x, y));
        assert_eq!(
            low.soft_outer_coverage_at(x, y, softness),
            mid.soft_outer_coverage_at(x, y, softness)
        );
        assert_eq!(
            mid.soft_outer_coverage_at(x, y, softness),
            high.soft_outer_coverage_at(x, y, softness)
        );
    }
}

#[test]
fn average_effect_increases_with_lightness() {
    let average_effect = |lightness: u8| {
        let mut rng = StdRng::seed_from_u64(20);
        let image = render_image(&settings(45, 50, lightness), &mut rng);
        let visible: Vec<_> = image.pixels().filter(|pixel| pixel[3] > 0).collect();
        visible.iter().map(|pixel| u64::from(pixel[3])).sum::<u64>() as f64 / visible.len() as f64
    };

    let effects: Vec<_> = [10, 25, 50, 75, 100]
        .into_iter()
        .map(average_effect)
        .collect();

    for window in effects.windows(2) {
        assert!(
            window[0] < window[1],
            "effects should increase: {effects:?}"
        );
    }
}

#[test]
fn lightness_keeps_internal_effect_variation() {
    let mut rng = StdRng::seed_from_u64(21);
    let image = render_image(&settings(45, 50, 50), &mut rng);
    let visible_effects: BTreeSet<_> = image
        .pixels()
        .filter(|pixel| pixel[3] > 0)
        .map(|pixel| pixel[3])
        .collect();

    assert!(
        visible_effects.len() > 1,
        "internal cloudy variation should remain"
    );
}
