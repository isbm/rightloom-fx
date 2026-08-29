use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicUsize, Ordering},
};

use rand::{Rng, SeedableRng, rngs::StdRng};

use super::*;
use crate::render::{ExportPolicy, Resolution, RgbColor};

static TEST_OUTPUT_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TestOutputDir(PathBuf);

impl TestOutputDir {
    fn new() -> Self {
        let number = TEST_OUTPUT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "rightloom-fx-bokeh-test-{}-{number}",
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

fn settings(
    types: &[BokehType],
    placements: &[BokehPlacement],
    density: u8,
    size: u8,
    uniform: u8,
) -> BokehSettings {
    BokehSettings {
        render: RenderSettings {
            resolution: Resolution::new(160, 100).expect("test resolution should be valid"),
            density,
            amount: 1,
            outdir: "unused".into(),
            export_policy: ExportPolicy::default(),
        },
        types: types.to_vec(),
        placements: placements.to_vec(),
        blur: 100,
        lightness: 70,
        deform: 0,
        size,
        uniform,
    }
}

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.001,
        "expected {expected}, got {actual}"
    );
}

fn effect_at(effects: &[f32], width: u32, x: u32, y: u32) -> f32 {
    effects[y as usize * width as usize + x as usize]
}

fn measured_twinkle_coverage(twinkles: &[Twinkle], width: u32, height: u32, deform: u8) -> f32 {
    let mut occupancy = TwinkleOccupancy::new(width, height);
    for twinkle in twinkles {
        occupancy.mark_twinkle(twinkle, deform);
    }
    occupancy.coverage()
}

fn coverage_settings(density: u8, size: u8, uniform: u8) -> BokehSettings {
    let mut settings = settings(&[BokehType::Twinkle], &[], density, size, uniform);
    settings.render.resolution =
        Resolution::new(480, 320).expect("test resolution should be valid");
    settings.blur = 30;
    settings.lightness = 60;
    settings.deform = 0;
    settings
}

fn manual_density_settings(density: u8) -> BokehSettings {
    let mut settings = settings(&[BokehType::Twinkle], &[], density, 40, 60);
    settings.render.resolution =
        Resolution::new(1_000, 667).expect("test resolution should be valid");
    settings.blur = 30;
    settings.lightness = 60;
    settings.deform = 0;
    settings
}

fn sample_scales(uniform: u8) -> Vec<f32> {
    let mut rng = StdRng::seed_from_u64(71);
    (0..2_048)
        .map(|_| sample_object_scale(100, uniform, &mut rng))
        .collect()
}

fn scale_variance(scales: &[f32]) -> f32 {
    let mean = scales.iter().sum::<f32>() / scales.len() as f32;
    scales
        .iter()
        .map(|scale| (scale - mean).powi(2))
        .sum::<f32>()
        / scales.len() as f32
}

fn export_bokeh(export_policy: ExportPolicy) -> (RgbaImage, RgbaImage) {
    const SEED: u64 = 72;

    let source_settings = settings(
        &[BokehType::Twinkle, BokehType::Edge, BokehType::Damage],
        &[BokehPlacement::Left],
        50,
        60,
        35,
    );
    let mut source_rng = StdRng::seed_from_u64(SEED);
    let source = render_image(&source_settings, &mut source_rng);
    let output = TestOutputDir::new();
    let mut output_settings = source_settings;
    output_settings.render.outdir = output.path().to_path_buf();
    output_settings.render.export_policy = export_policy;
    let mut output_rng = StdRng::seed_from_u64(SEED);

    generate_images_with_rng(&output_settings, &mut output_rng)
        .expect("bokeh output should write successfully");
    let exported = image::open(output.path().join("bokeh-0001.png"))
        .expect("bokeh output should be readable")
        .to_rgba8();

    (source, exported)
}

#[test]
fn bokeh_type_and_placement_names_are_stable() {
    assert_eq!(BokehType::names(), ["twinkle", "edge", "damage"]);
    assert_eq!(
        BokehPlacement::names(),
        ["center", "left", "right", "top", "bottom"]
    );
}

#[test]
fn placement_lists_deduplicate_values() {
    assert_eq!(
        BokehPlacement::parse_list("c,left,c,l"),
        Ok(vec![BokehPlacement::Center, BokehPlacement::Left])
    );
}

#[test]
fn bokeh_settings_require_a_type_before_output_is_created() {
    let output = TestOutputDir::new();
    let mut settings = settings(&[], &[], 50, 50, 50);
    settings.render.outdir = output.path().to_path_buf();
    let mut rng = StdRng::seed_from_u64(1);

    let error = generate_images_with_rng(&settings, &mut rng)
        .expect_err("bokeh generation without a type should fail");
    assert!(matches!(error, RenderError::NoBokehTypes));
    assert!(
        fs::read_dir(output.path())
            .expect("test directory should be readable")
            .next()
            .is_none()
    );
}

#[test]
fn bokeh_settings_validate_size_and_uniformity() {
    let mut invalid_size = settings(&[BokehType::Twinkle], &[], 50, 101, 50);
    assert!(matches!(
        invalid_size.validate(),
        Err(RenderError::InvalidSize(101))
    ));

    invalid_size.size = 50;
    invalid_size.uniform = 101;
    assert!(matches!(
        invalid_size.validate(),
        Err(RenderError::InvalidUniform(101))
    ));
}

#[test]
fn bokeh_settings_validate_blur_lightness_and_effect_placements() {
    let mut invalid_blur = settings(&[BokehType::Twinkle], &[], 50, 50, 50);
    invalid_blur.blur = 101;
    assert!(matches!(
        invalid_blur.validate(),
        Err(RenderError::InvalidBlur(101))
    ));

    let invalid_edge_placement =
        settings(&[BokehType::Edge], &[BokehPlacement::Center], 50, 50, 50);
    let error = invalid_edge_placement
        .validate()
        .expect_err("center-only edge placement should fail");
    assert!(matches!(error, RenderError::InvalidBokehEdgePlacement));
    assert_eq!(
        error.to_string(),
        "center placement is not available for edge bokeh"
    );

    let invalid_damage_placement =
        settings(&[BokehType::Damage], &[BokehPlacement::Center], 50, 50, 50);
    let error = invalid_damage_placement
        .validate()
        .expect_err("center-only damage placement should fail");
    assert!(matches!(error, RenderError::InvalidBokehDamagePlacement));
    assert_eq!(
        error.to_string(),
        "center placement is not available for damage bokeh"
    );

    let mut invalid_lightness = settings(&[BokehType::Twinkle], &[], 50, 50, 50);
    invalid_lightness.lightness = 101;
    assert!(matches!(
        invalid_lightness.validate(),
        Err(RenderError::InvalidLightness(101))
    ));

    let mut invalid_deform = settings(&[BokehType::Twinkle], &[], 50, 50, 50);
    invalid_deform.deform = 101;
    assert!(matches!(
        invalid_deform.validate(),
        Err(RenderError::InvalidDeform(101))
    ));

    let mixed_placements = settings(
        &[BokehType::Twinkle, BokehType::Edge, BokehType::Damage],
        &[BokehPlacement::Center, BokehPlacement::Left],
        50,
        50,
        50,
    );
    assert!(mixed_placements.validate().is_ok());
}

#[test]
fn placements_are_filtered_per_effect_without_rejecting_mixed_requests() {
    let settings = settings(
        &[BokehType::Twinkle, BokehType::Edge, BokehType::Damage],
        &[
            BokehPlacement::Center,
            BokehPlacement::Left,
            BokehPlacement::Right,
        ],
        50,
        50,
        50,
    );

    assert!(settings.validate().is_ok());
    assert_eq!(
        settings_for_type(&settings, BokehType::Twinkle).placements,
        vec![
            BokehPlacement::Center,
            BokehPlacement::Left,
            BokehPlacement::Right,
        ]
    );
    assert_eq!(
        settings_for_type(&settings, BokehType::Edge).placements,
        vec![BokehPlacement::Left, BokehPlacement::Right]
    );
    assert_eq!(
        settings_for_type(&settings, BokehType::Damage).placements,
        vec![BokehPlacement::Left, BokehPlacement::Right]
    );
}

#[test]
fn maximum_scale_uses_a_tiny_floor_and_requested_percentage() {
    assert_close(maximum_scale(0), 0.02);
    assert_close(maximum_scale(50), 0.50);
    assert_close(maximum_scale(100), 1.0);
}

#[test]
fn uniform_100_produces_a_narrow_size_range() {
    let scales = sample_scales(100);
    let minimum = scales.iter().copied().fold(f32::INFINITY, f32::min);
    let maximum = scales.iter().copied().fold(f32::NEG_INFINITY, f32::max);

    assert!(minimum >= 0.97);
    assert!(maximum <= 1.03);
}

#[test]
fn uniform_zero_produces_a_much_wider_size_range() {
    let low_uniformity = sample_scales(0);
    let high_uniformity = sample_scales(100);
    let low_range = low_uniformity
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max)
        - low_uniformity.iter().copied().fold(f32::INFINITY, f32::min);
    let high_range = high_uniformity
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max)
        - high_uniformity
            .iter()
            .copied()
            .fold(f32::INFINITY, f32::min);

    assert!(low_range > high_range * 10.0);
}

#[test]
fn size_100_uniform_1_contains_tiny_and_near_maximum_scales() {
    let mut rng = StdRng::seed_from_u64(73);
    let scales: Vec<_> = (0..4_096)
        .map(|_| sample_object_scale(100, 1, &mut rng))
        .collect();
    let minimum = scales.iter().copied().fold(f32::INFINITY, f32::min);
    let maximum = scales.iter().copied().fold(f32::NEG_INFINITY, f32::max);

    assert!(minimum < 0.03, "expected tiny scales, got {minimum}");
    assert!(
        maximum > 0.94,
        "expected near-maximum scales, got {maximum}"
    );
}

#[test]
fn increasing_uniformity_monotonically_reduces_size_variance() {
    let variances: Vec<_> = [0, 25, 50, 75, 100]
        .into_iter()
        .map(|uniform| scale_variance(&sample_scales(uniform)))
        .collect();

    for pair in variances.windows(2) {
        assert!(pair[0] > pair[1], "variance should decrease: {variances:?}");
    }
}

#[test]
fn size_100_allows_a_twinkle_near_the_longest_image_side() {
    let mut settings = settings(&[BokehType::Twinkle], &[], 50, 100, 100);
    settings.render.resolution = Resolution::new(1_000, 667).expect("resolution should be valid");
    let mut rng = StdRng::seed_from_u64(74);
    let twinkles = generate_twinkles(
        &settings,
        settings.render.resolution.width(),
        settings.render.resolution.height(),
        &mut rng,
    );
    let largest_diameter = twinkles
        .iter()
        .map(|twinkle| twinkle.radius_x * 2.0)
        .fold(0.0, f32::max);

    assert!((940.0..=1_040.0).contains(&largest_diameter));
}

#[test]
fn size_controls_twinkle_scale() {
    let mut small = settings(&[BokehType::Twinkle], &[], 75, 50, 100);
    small.render.resolution = Resolution::new(1_000, 667).expect("resolution should be valid");
    let mut large = small.clone();
    large.size = 100;
    let mut small_rng = StdRng::seed_from_u64(75);
    let small_twinkles = generate_twinkles(&small, 1_000, 667, &mut small_rng);
    let mut large_rng = StdRng::seed_from_u64(75);
    let large_twinkles = generate_twinkles(&large, 1_000, 667, &mut large_rng);
    let small_maximum = small_twinkles
        .iter()
        .map(|twinkle| twinkle.radius_x)
        .fold(0.0, f32::max);
    let large_maximum = large_twinkles
        .iter()
        .map(|twinkle| twinkle.radius_x)
        .fold(0.0, f32::max);

    assert!(small_maximum < large_maximum);
}

#[test]
fn damage_density_count_and_scale_sampling_are_unchanged() {
    assert_eq!(damage_count(0), 0);
    assert!(damage_count(10) < damage_count(25));
    assert!(damage_count(25) < damage_count(50));
    assert!(damage_count(50) < damage_count(75));
    assert!(damage_count(75) < damage_count(100));
    assert_eq!(damage_count(50), 18);
    assert!(edge_count(25) < edge_count(100));

    let mut first_rng = StdRng::seed_from_u64(76);
    let first = sample_object_scale(80, 25, &mut first_rng);
    let mut second_rng = StdRng::seed_from_u64(76);
    let second = sample_object_scale(80, 25, &mut second_rng);
    assert_eq!(first, second);
}

#[test]
fn twinkle_count_estimate_uses_the_poisson_coverage_formula() {
    let canvas_area = 1_000.0 * 667.0;
    let expected_area = 12_500.0;
    let targets = [0.05, 0.25, 0.50, 0.75, FULL_TWINKLE_COVERAGE_TARGET];
    let estimates: Vec<_> = targets
        .into_iter()
        .map(|target| estimated_twinkle_count(canvas_area, target, expected_area))
        .collect();

    assert_eq!(estimated_twinkle_count(canvas_area, 0.0, expected_area), 0);
    assert!(estimates.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(
        estimated_twinkle_count(canvas_area, 0.50, expected_area)
            > estimated_twinkle_count(canvas_area, 0.50, expected_area * 2.0)
    );
    assert_eq!(twinkle_target_coverage(0), 0.0);
    assert_close(twinkle_target_coverage(100), 0.995);
    assert!(estimates.last().is_some_and(|count| *count < usize::MAX));

    let expected = (-((1.0 - 0.50_f32).ln()) * canvas_area / expected_area).ceil() as usize;
    assert_eq!(
        estimated_twinkle_count(canvas_area, 0.50, expected_area),
        expected
    );
}

#[test]
fn expected_twinkle_area_uses_size_uniformity_and_no_render_rng() {
    let small = coverage_settings(50, 10, 60);
    let large = coverage_settings(50, 90, 60);
    let mut broad = coverage_settings(50, 100, 1);
    let mut narrow = broad.clone();
    narrow.uniform = 100;
    broad.deform = 100;
    narrow.deform = 100;

    assert!(
        expected_twinkle_body_area(&small, 480, 320) < expected_twinkle_body_area(&large, 480, 320)
    );
    assert!(
        expected_twinkle_body_area(&broad, 480, 320)
            < expected_twinkle_body_area(&narrow, 480, 320)
    );

    let mut expected_rng = StdRng::seed_from_u64(121);
    let next_value = expected_rng.random::<u64>();
    let mut actual_rng = StdRng::seed_from_u64(121);
    let _ = expected_twinkle_body_area(&large, 480, 320);
    assert_eq!(actual_rng.random::<u64>(), next_value);
}

#[test]
fn occupancy_tracks_the_union_of_overlapping_twinkle_bodies() {
    let twinkle = geometric_twinkle();
    let single = measured_twinkle_coverage(&[twinkle], 120, 80, 0);
    let overlap = measured_twinkle_coverage(&[twinkle, twinkle], 120, 80, 0);

    assert!(single > 0.0);
    assert_eq!(overlap, single);
}

#[test]
fn occupancy_grid_keeps_the_canvas_aspect_ratio() {
    let occupancy = TwinkleOccupancy::new(1_000, 667);

    assert_eq!((occupancy.grid_width, occupancy.grid_height), (192, 128));
}

#[test]
fn occupancy_uses_the_exact_circular_body_when_deform_is_zero() {
    let twinkle = geometric_twinkle();

    assert!(twinkle.contains_body(twinkle.center_x + 12.0, twinkle.center_y + 16.0, 0));
    assert!(!twinkle.contains_body(twinkle.center_x + 12.1, twinkle.center_y + 16.0, 0));
    assert!(
        measured_twinkle_coverage(&[twinkle], 120, 80, 100)
            > measured_twinkle_coverage(&[twinkle], 120, 80, 0)
    );
}

#[test]
fn blur_and_lightness_do_not_change_twinkle_body_occupancy() {
    let sharp = coverage_settings(50, 20, 100);
    let mut soft = sharp.clone();
    soft.blur = 100;
    let mut bright = sharp.clone();
    bright.lightness = 100;
    let width = sharp.render.resolution.width();
    let height = sharp.render.resolution.height();

    let mut sharp_rng = StdRng::seed_from_u64(122);
    let sharp_generation = generate_twinkles_with_coverage(&sharp, width, height, &mut sharp_rng)
        .expect("sharp twinkles should reach their target");
    let mut soft_rng = StdRng::seed_from_u64(122);
    let soft_generation = generate_twinkles_with_coverage(&soft, width, height, &mut soft_rng)
        .expect("soft twinkles should reach their target");
    let mut bright_rng = StdRng::seed_from_u64(122);
    let bright_generation =
        generate_twinkles_with_coverage(&bright, width, height, &mut bright_rng)
            .expect("bright twinkles should reach their target");

    assert_eq!(sharp_generation.twinkles, soft_generation.twinkles);
    assert_eq!(sharp_generation.twinkles, bright_generation.twinkles);
    assert_eq!(
        sharp_generation.initial_count,
        soft_generation.initial_count
    );
    assert_eq!(
        sharp_generation.initial_count,
        bright_generation.initial_count
    );
    assert_eq!(sharp_generation.coverage, soft_generation.coverage);
    assert_eq!(sharp_generation.coverage, bright_generation.coverage);
}

#[test]
fn twinkle_occupancy_and_primitives_are_seeded_deterministic() {
    let settings = coverage_settings(75, 20, 100);
    let width = settings.render.resolution.width();
    let height = settings.render.resolution.height();
    let mut first_rng = StdRng::seed_from_u64(123);
    let first = generate_twinkles_with_coverage(&settings, width, height, &mut first_rng)
        .expect("first generation should reach its target");
    let mut second_rng = StdRng::seed_from_u64(123);
    let second = generate_twinkles_with_coverage(&settings, width, height, &mut second_rng)
        .expect("second generation should reach its target");

    assert_eq!(first.twinkles, second.twinkles);
    assert_eq!(first.initial_count, second.initial_count);
    assert_eq!(first.coverage, second.coverage);
}

#[test]
fn twinkle_density_reaches_monotonic_body_coverage_targets() {
    let mut coverages = Vec::new();

    for density in [5, 25, 50, 75, 100] {
        let settings = manual_density_settings(density);
        let width = settings.render.resolution.width();
        let height = settings.render.resolution.height();
        let mut rng = StdRng::seed_from_u64(128);
        let generation = generate_twinkles_with_coverage(&settings, width, height, &mut rng)
            .expect("twinkles should reach their target");
        let measured =
            measured_twinkle_coverage(&generation.twinkles, width, height, settings.deform);
        let target = twinkle_target_coverage(density);

        assert_eq!(measured, generation.coverage);
        assert!(measured >= target, "density {density} measured {measured}");
        if density < 100 {
            assert!(
                measured <= target + TWINKLE_COVERAGE_TOLERANCE,
                "density {density} overshot: {measured}"
            );
        } else {
            assert!(measured >= 0.995);
        }
        coverages.push(measured);
    }

    assert!(coverages.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn density_is_independent_of_twinkle_size() {
    let small = coverage_settings(75, 10, 100);
    let large = coverage_settings(75, 90, 60);
    let width = small.render.resolution.width();
    let height = small.render.resolution.height();
    let mut small_rng = StdRng::seed_from_u64(125);
    let small_generation = generate_twinkles_with_coverage(&small, width, height, &mut small_rng)
        .expect("small twinkles should reach their target");
    let mut large_rng = StdRng::seed_from_u64(125);
    let large_generation = generate_twinkles_with_coverage(&large, width, height, &mut large_rng)
        .expect("large twinkles should reach their target");

    assert!(small_generation.twinkles.len() > large_generation.twinkles.len());
    for (label, generation) in [("small", &small_generation), ("large", &large_generation)] {
        assert!(generation.coverage >= 0.75, "{label} coverage was too low");
        assert!(generation.coverage <= 0.77, "{label} coverage was too high");
    }
}

#[test]
fn center_placement_remains_a_bias_while_full_density_reaches_the_canvas() {
    let mut settings = coverage_settings(100, 10, 100);
    settings.placements = vec![BokehPlacement::Center];
    let width = settings.render.resolution.width();
    let height = settings.render.resolution.height();
    let mut rng = StdRng::seed_from_u64(126);
    let generation = generate_twinkles_with_coverage(&settings, width, height, &mut rng)
        .expect("center-biased twinkles should reach their target");

    assert!(generation.coverage >= 0.995);
}

#[test]
fn twinkle_placement_is_biased_without_becoming_a_clipping_zone() {
    let mut rng = StdRng::seed_from_u64(127);
    let positions: Vec<_> = (0..512)
        .map(|_| twinkle_placement_position(Some(BokehPlacement::Left), 1_000, 667, &mut rng))
        .collect();
    let mean_x = positions.iter().map(|position| position.0).sum::<f32>() / positions.len() as f32;

    assert!(mean_x < 350.0);
    assert!(positions.iter().any(|position| position.0 > 800.0));
}

#[test]
fn zero_density_generates_no_twinkles() {
    let settings = coverage_settings(0, 40, 60);
    let mut rng = StdRng::seed_from_u64(129);
    let generation = generate_twinkles_with_coverage(&settings, 480, 320, &mut rng)
        .expect("zero density should be valid");

    assert!(generation.twinkles.is_empty());
    assert_eq!(generation.initial_count, 0);
    assert_eq!(generation.coverage, 0.0);
}

#[test]
#[ignore = "writes seeded twinkle-density diagnostics under .tmp"]
fn writes_seeded_twinkle_density_diagnostics() {
    const SEED: u64 = 128;
    const WIDTH: u32 = 1_000;
    const HEIGHT: u32 = 667;

    for density in [5, 25, 50, 75, 100] {
        let mut settings = manual_density_settings(density);
        settings.render.outdir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!("density-seeded-{density:02}"));

        let mut measurement_rng = StdRng::seed_from_u64(SEED);
        let generation =
            generate_twinkles_with_coverage(&settings, WIDTH, HEIGHT, &mut measurement_rng)
                .expect("density diagnostic should reach its target");
        println!(
            "density {density:3}: initial {:4}, final {:4}, coverage {:.3}%",
            generation.initial_count,
            generation.twinkles.len(),
            generation.coverage * 100.0
        );

        let mut output_rng = StdRng::seed_from_u64(SEED);
        generate_images_with_rng(&settings, &mut output_rng)
            .expect("density diagnostic should write successfully");
    }

    for size in [10, 90] {
        let settings = settings(&[BokehType::Twinkle], &[], 75, size, 60);
        let mut rng = StdRng::seed_from_u64(SEED);
        let generation = generate_twinkles_with_coverage(&settings, WIDTH, HEIGHT, &mut rng)
            .expect("size diagnostic should reach its target");
        println!(
            "size {size:2}, density 75: initial {:4}, final {:4}, coverage {:.3}%",
            generation.initial_count,
            generation.twinkles.len(),
            generation.coverage * 100.0
        );
    }
}

#[test]
fn density_zero_produces_an_empty_scalar_render() {
    let mut rng = StdRng::seed_from_u64(77);
    let image = render_image(
        &settings(&[BokehType::Twinkle, BokehType::Edge], &[], 0, 50, 50),
        &mut rng,
    );

    let luma = bokeh_luma(70);
    assert!(image.pixels().all(|pixel| pixel.0 == [luma, luma, luma, 0]));
}

#[test]
fn bokeh_lightness_uses_the_required_grayscale_anchors() {
    assert_eq!(bokeh_luma(0), 0);
    assert_eq!(bokeh_luma(25), 64);
    assert_eq!(bokeh_luma(50), 128);
    assert_eq!(bokeh_luma(75), 191);
    assert_eq!(bokeh_luma(100), 255);
}

#[test]
fn lightness_changes_rgb_without_changing_seeded_alpha_or_geometry() {
    let mut dim = settings(
        &[BokehType::Twinkle, BokehType::Edge, BokehType::Damage],
        &[BokehPlacement::Center, BokehPlacement::Left],
        50,
        70,
        25,
    );
    dim.lightness = 20;
    let mut bright = dim.clone();
    bright.lightness = 80;

    let mut dim_rng = StdRng::seed_from_u64(94);
    let dim_image = render_image(&dim, &mut dim_rng);
    let mut bright_rng = StdRng::seed_from_u64(94);
    let bright_image = render_image(&bright, &mut bright_rng);

    for (dim_pixel, bright_pixel) in dim_image.pixels().zip(bright_image.pixels()) {
        assert_eq!(dim_pixel[3], bright_pixel[3]);
        assert_eq!(&dim_pixel.0[..3], &[bokeh_luma(20); 3]);
        assert_eq!(&bright_pixel.0[..3], &[bokeh_luma(80); 3]);
    }

    let mut dim_rng = StdRng::seed_from_u64(95);
    let mut bright_rng = StdRng::seed_from_u64(95);
    assert_eq!(
        generate_twinkles(&dim, 1_000, 667, &mut dim_rng),
        generate_twinkles(&bright, 1_000, 667, &mut bright_rng)
    );
    let mut dim_rng = StdRng::seed_from_u64(96);
    let mut bright_rng = StdRng::seed_from_u64(96);
    let dim_edge = settings_for_type(&dim, BokehType::Edge);
    let bright_edge = settings_for_type(&bright, BokehType::Edge);
    assert_eq!(
        generate_edge_exposures(&dim_edge, 1_000, 667, &mut dim_rng),
        generate_edge_exposures(&bright_edge, 1_000, 667, &mut bright_rng)
    );
    let mut dim_rng = StdRng::seed_from_u64(97);
    let mut bright_rng = StdRng::seed_from_u64(97);
    let dim_damage = settings_for_type(&dim, BokehType::Damage);
    let bright_damage = settings_for_type(&bright, BokehType::Damage);
    assert_eq!(
        generate_damage_segments(&dim_damage, 1_000, 667, &mut dim_rng),
        generate_damage_segments(&bright_damage, 1_000, 667, &mut bright_rng)
    );
}

fn geometric_twinkle() -> Twinkle {
    Twinkle {
        center_x: 60.0,
        center_y: 40.0,
        radius_x: 20.0,
        radius_y: 26.0,
        sin_angle: 0.6,
        cos_angle: 0.8,
        intensity: 0.6,
        deformation: 0.06,
        deformation_frequency: 2.0,
        deformation_phase: 0.0,
        glow_phase: 0.3,
    }
}

#[test]
fn deform_zero_is_a_perfect_circle_without_angular_modulation() {
    let twinkle = geometric_twinkle();
    let angles = [
        0.0,
        std::f32::consts::FRAC_PI_6,
        std::f32::consts::FRAC_PI_4,
        std::f32::consts::FRAC_PI_2,
        std::f32::consts::PI,
    ];

    assert_eq!(twinkle.effective_radius_y(0) / twinkle.radius_x, 1.0);
    for angle in angles {
        assert_eq!(twinkle.radial_adjustment(angle, 0), 1.0);
        assert_close(twinkle.boundary_radius(angle, 0), twinkle.radius_x);
    }
}

#[test]
fn deform_blends_the_existing_ellipse_and_angular_amplitude() {
    let mut ellipse = geometric_twinkle();
    ellipse.deformation = 0.0;
    let angle = std::f32::consts::FRAC_PI_2;
    let base_radius = ellipse.boundary_radius(angle, 0);
    let half_radius = ellipse.boundary_radius(angle, 50);
    let full_radius = ellipse.boundary_radius(angle, 100);

    assert_eq!(ellipse.effective_radius_y(100), ellipse.radius_y);
    assert_close(half_radius - base_radius, (full_radius - base_radius) * 0.5);

    let mut wobble = geometric_twinkle();
    wobble.radius_y = wobble.radius_x;
    let angle = std::f32::consts::FRAC_PI_4;
    let expected_full = 1.0
        + wobble.deformation
            * (wobble.deformation_frequency * angle + wobble.deformation_phase).sin();

    assert_close(wobble.radial_adjustment(angle, 100), expected_full);
    assert_close(
        wobble.boundary_radius(angle, 50) - wobble.boundary_radius(angle, 0),
        (wobble.boundary_radius(angle, 100) - wobble.boundary_radius(angle, 0)) * 0.5,
    );
}

#[test]
fn deformation_is_bounded_and_keeps_every_effective_radius_positive() {
    let twinkle = geometric_twinkle();

    for deform in [0, 25, 50, 75, 100] {
        for index in 0..=360 {
            let angle = index as f32 / 360.0 * std::f32::consts::TAU;
            let adjustment = twinkle.radial_adjustment(angle, deform);

            assert!(adjustment > 0.0);
            assert!(adjustment >= 1.0 - twinkle.deformation);
            assert!(adjustment <= 1.0 + twinkle.deformation);
            assert!(twinkle.boundary_radius(angle, deform) > 0.0);
        }
    }
}

#[test]
fn deform_uses_its_body_geometry_without_affecting_other_effect_generators() {
    let mut circular = settings(
        &[BokehType::Twinkle, BokehType::Edge, BokehType::Damage],
        &[BokehPlacement::Left],
        60,
        70,
        80,
    );
    circular.blur = 35;
    circular.lightness = 60;
    let mut organic = circular.clone();
    organic.deform = 100;

    let mut circular_rng = StdRng::seed_from_u64(108);
    let circular_twinkles =
        generate_twinkles_with_coverage(&circular, 1_000, 667, &mut circular_rng)
            .expect("circular twinkles should reach their target");
    let mut organic_rng = StdRng::seed_from_u64(108);
    let organic_twinkles = generate_twinkles_with_coverage(&organic, 1_000, 667, &mut organic_rng)
        .expect("organic twinkles should reach their target");
    assert!(circular_twinkles.coverage >= twinkle_target_coverage(60));
    assert!(organic_twinkles.coverage >= twinkle_target_coverage(60));

    let mut circular_edge_rng = StdRng::seed_from_u64(108);
    let mut organic_edge_rng = StdRng::seed_from_u64(108);
    let circular_edges = generate_edge_exposures(&circular, 1_000, 667, &mut circular_edge_rng);
    let organic_edges = generate_edge_exposures(&organic, 1_000, 667, &mut organic_edge_rng);
    assert_eq!(circular_edges, organic_edges);
    let mut circular_damage_rng = StdRng::seed_from_u64(109);
    let mut organic_damage_rng = StdRng::seed_from_u64(109);
    let circular_damage = generate_damage_segments(&circular, 1_000, 667, &mut circular_damage_rng);
    let organic_damage = generate_damage_segments(&organic, 1_000, 667, &mut organic_damage_rng);
    assert_eq!(circular_damage, organic_damage);

    let mut circular_rng = StdRng::seed_from_u64(109);
    let circular_image = render_image(&circular, &mut circular_rng);
    let mut organic_rng = StdRng::seed_from_u64(109);
    let organic_image = render_image(&organic, &mut organic_rng);
    let luma = bokeh_luma(circular.lightness);
    for (circular_pixel, organic_pixel) in circular_image.pixels().zip(organic_image.pixels()) {
        assert_eq!(&circular_pixel.0[..3], &[luma; 3]);
        assert_eq!(&organic_pixel.0[..3], &[luma; 3]);
    }
    assert_eq!(circular.blur, organic.blur);
    assert_eq!(circular.lightness, organic.lightness);
}

#[test]
#[ignore = "writes a seeded deform progression under .tmp"]
fn writes_seeded_twinkle_deform_progression() {
    const SEED: u64 = 110;

    for deform in [0, 25, 50, 75, 100] {
        let mut settings = settings(&[BokehType::Twinkle], &[BokehPlacement::Center], 60, 70, 80);
        settings.render.resolution = Resolution::new(1_000, 667).expect("resolution is valid");
        settings.render.amount = 4;
        settings.render.outdir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!("twinkle-deform-seeded-{deform}"));
        settings.blur = 35;
        settings.lightness = 60;
        settings.deform = deform;

        let mut rng = StdRng::seed_from_u64(SEED);
        generate_images_with_rng(&settings, &mut rng)
            .expect("seeded deform diagnostic should write successfully");
    }
}

#[test]
fn twinkle_center_is_stronger_than_its_distant_falloff() {
    let twinkle = Twinkle {
        center_x: 50.0,
        center_y: 50.0,
        radius_x: 20.0,
        radius_y: 20.0,
        sin_angle: 0.0,
        cos_angle: 1.0,
        intensity: 0.7,
        deformation: 0.0,
        deformation_frequency: 2.0,
        deformation_phase: 0.0,
        glow_phase: 0.0,
    };
    let mut effects = vec![0.0; 100 * 100];
    twinkle.rasterize(
        &mut effects,
        100,
        100,
        BlurParameters::new(100, 100, 100),
        0,
    );

    assert!(effect_at(&effects, 100, 50, 50) > effect_at(&effects, 100, 70, 50));
    assert!(effect_at(&effects, 100, 70, 50) > effect_at(&effects, 100, 90, 50));
}

#[test]
fn twinkle_uses_a_smooth_pancake_profile_with_a_brighter_rim() {
    let twinkle = Twinkle {
        center_x: 60.0,
        center_y: 40.0,
        radius_x: 24.0,
        radius_y: 24.0,
        sin_angle: 0.0,
        cos_angle: 1.0,
        intensity: 0.6,
        deformation: 0.0,
        deformation_frequency: 2.0,
        deformation_phase: 0.0,
        glow_phase: 0.0,
    };
    let mut effects = vec![0.0; 120 * 80];
    twinkle.rasterize(&mut effects, 120, 80, BlurParameters::new(0, 120, 80), 0);
    let samples: Vec<_> = (60..=96).map(|x| effect_at(&effects, 120, x, 40)).collect();
    let quantized: BTreeSet<_> = samples
        .iter()
        .map(|value| (value * 10_000.0).round() as i32)
        .collect();

    assert!(effect_at(&effects, 120, 78, 40) > effect_at(&effects, 120, 60, 40));
    assert!(
        quantized.len() > 12,
        "pancake edge should remain antialiased"
    );
}

#[test]
fn twinkle_blur_changes_only_its_outer_transition() {
    let twinkle = Twinkle {
        center_x: 60.0,
        center_y: 40.0,
        radius_x: 24.0,
        radius_y: 24.0,
        sin_angle: 0.0,
        cos_angle: 1.0,
        intensity: 0.6,
        deformation: 0.0,
        deformation_frequency: 2.0,
        deformation_phase: 0.0,
        glow_phase: 0.0,
    };
    let mut sharp = vec![0.0; 120 * 80];
    twinkle.rasterize(&mut sharp, 120, 80, BlurParameters::new(0, 120, 80), 0);
    let mut soft = vec![0.0; 120 * 80];
    twinkle.rasterize(&mut soft, 120, 80, BlurParameters::new(100, 120, 80), 0);

    assert_close(
        effect_at(&sharp, 120, 60, 40),
        effect_at(&soft, 120, 60, 40),
    );
    assert_close(
        effect_at(&sharp, 120, 70, 40),
        effect_at(&soft, 120, 70, 40),
    );
    assert_eq!(effect_at(&sharp, 120, 90, 40), 0.0);
    assert!(effect_at(&soft, 120, 90, 40) > 0.0);
}

#[test]
fn twinkle_overlap_accumulation_stays_in_the_unit_interval() {
    let mut effect = 0.0;
    for contribution in [0.2, 0.4, 0.7, 1.0] {
        effect = accumulate_scalar_effect(effect, contribution);
        assert!((0.0..=1.0).contains(&effect));
    }
}

#[test]
fn partial_off_frame_twinkles_render_inside_the_image() {
    let twinkle = Twinkle {
        center_x: -8.0,
        center_y: 30.0,
        radius_x: 32.0,
        radius_y: 20.0,
        sin_angle: 0.0,
        cos_angle: 1.0,
        intensity: 0.6,
        deformation: 0.0,
        deformation_frequency: 2.0,
        deformation_phase: 0.0,
        glow_phase: 0.0,
    };
    let mut effects = vec![0.0; 100 * 60];
    twinkle.rasterize(&mut effects, 100, 60, BlurParameters::new(100, 100, 60), 0);

    assert!(effect_at(&effects, 100, 0, 30) > 0.0);
    assert!(effects.iter().any(|effect| *effect > 0.0));
}

fn edge_exposure(direction: EdgeDirection) -> EdgeExposure {
    EdgeExposure {
        direction,
        penetration: 28.0,
        intensity: 0.6,
        broad_profile: EdgeProfile {
            values: vec![0.0, 0.0],
        },
        torn_profile: EdgeProfile {
            values: vec![0.0, 0.0],
        },
        brightness_profile: EdgeProfile {
            values: vec![0.0, 0.0],
        },
        broad_depth_variation: 0.15,
        brightness_variation: 0.15,
        bright_center: 0.5,
        bright_spread: 0.2,
        bright_strength: 0.4,
    }
}

#[test]
fn each_edge_direction_is_strongest_near_its_selected_side() {
    for (direction, near, far) in [
        (EdgeDirection::Left, (1, 50), (90, 50)),
        (EdgeDirection::Right, (98, 50), (10, 50)),
        (EdgeDirection::Top, (50, 1), (50, 90)),
        (EdgeDirection::Bottom, (50, 98), (50, 10)),
    ] {
        let mut effects = vec![0.0; 100 * 100];
        edge_exposure(direction).rasterize(
            &mut effects,
            100,
            100,
            BlurParameters::new(100, 100, 100),
        );
        assert!(
            effect_at(&effects, 100, near.0, near.1) > effect_at(&effects, 100, far.0, far.1),
            "{direction:?} should be strongest near its chosen side"
        );
    }
}

#[test]
fn edge_profiles_use_seeded_nonperiodic_control_points() {
    let settings = settings(&[BokehType::Edge], &[BokehPlacement::Left], 70, 70, 50);
    let mut rng = StdRng::seed_from_u64(82);
    let exposures = generate_edge_exposures(&settings, 1_000, 667, &mut rng);

    assert!(!exposures.is_empty());
    for exposure in exposures {
        assert!((3..=7).contains(&(exposure.broad_profile.values.len() - 1)));
        assert!((8..=20).contains(&(exposure.torn_profile.values.len() - 1)));
        assert_eq!(exposure.broad_depth_variation, 0.15);
        assert!(
            exposure
                .broad_profile
                .values
                .iter()
                .all(|value| (-1.0..=1.0).contains(value))
        );
        assert!(
            exposure
                .torn_profile
                .values
                .iter()
                .all(|value| value.abs() <= 0.18)
        );
    }
}

#[test]
fn torn_profiles_keep_strong_excursions_local_and_bounded() {
    let mut rng = StdRng::seed_from_u64(93);
    let profile = EdgeProfile::random_torn(100, &mut rng);

    assert!(profile.values.iter().all(|value| value.abs() <= 0.18));
    assert!(profile.values.iter().any(|value| value.abs() >= 0.15));
}

#[test]
fn edge_blur_uses_the_required_softness_range_and_preserves_inner_brightness() {
    let edge = EdgeExposure {
        penetration: 240.0,
        ..edge_exposure(EdgeDirection::Left)
    };
    let sharp = BlurParameters::new(0, 1_000, 667);
    let soft = BlurParameters::new(100, 1_000, 667);

    assert_close(sharp.edge_softness, 2.0);
    assert_close(soft.edge_softness, 80.0);
    assert_close(
        edge.contribution(0.5, edge.penetration, 1.0, sharp),
        edge.contribution(0.5, edge.penetration, 1.0, soft),
    );
    assert_eq!(
        edge.contribution(edge.penetration + 4.0, edge.penetration, 1.0, sharp),
        0.0
    );
    assert!(edge.contribution(edge.penetration + 4.0, edge.penetration, 1.0, soft) > 0.0);
    assert!(soft_rectangle_coverage(0.0, sharp.edge_softness) > 0.0);
    assert!(soft_rectangle_coverage(0.0, sharp.edge_softness) < 1.0);
}

#[test]
fn edge_fade_is_monotonic_before_low_frequency_modulation() {
    let edge = edge_exposure(EdgeDirection::Left);
    let values: Vec<_> = [0.0, 8.0, 16.0, 24.0, 32.0]
        .into_iter()
        .map(|distance| {
            edge.contribution(
                distance,
                edge.penetration,
                1.0,
                BlurParameters::new(100, 100, 100),
            )
        })
        .collect();

    assert!(values.windows(2).all(|pair| pair[0] > pair[1]));
}

#[test]
fn edge_modulation_changes_smoothly_without_hard_stripes() {
    let edge = EdgeExposure {
        broad_profile: EdgeProfile {
            values: vec![-0.7, 0.45, -0.2, 0.85],
        },
        torn_profile: EdgeProfile {
            values: vec![0.06, -0.08, 0.03, -0.04, 0.07, -0.02, 0.05, -0.06, 0.02],
        },
        ..edge_exposure(EdgeDirection::Left)
    };
    let depths: Vec<_> = (0..1_000)
        .map(|index| edge.modulation(index as f32 / 1_000.0).0)
        .collect();
    let greatest_step = depths
        .windows(2)
        .map(|pair| (pair[0] - pair[1]).abs())
        .fold(0.0, f32::max);

    assert!(
        greatest_step < 0.2,
        "edge modulation should remain low frequency"
    );
}

#[test]
fn blur_does_not_change_seeded_primitive_geometry() {
    let mut sharp = settings(
        &[BokehType::Twinkle, BokehType::Edge, BokehType::Damage],
        &[BokehPlacement::Left],
        70,
        65,
        35,
    );
    sharp.blur = 0;
    let mut soft = sharp.clone();
    soft.blur = 100;

    let mut sharp_rng = StdRng::seed_from_u64(83);
    let mut soft_rng = StdRng::seed_from_u64(83);
    assert_eq!(
        generate_twinkles(&sharp, 1_000, 667, &mut sharp_rng),
        generate_twinkles(&soft, 1_000, 667, &mut soft_rng)
    );

    let mut sharp_rng = StdRng::seed_from_u64(84);
    let mut soft_rng = StdRng::seed_from_u64(84);
    assert_eq!(
        generate_edge_exposures(&sharp, 1_000, 667, &mut sharp_rng),
        generate_edge_exposures(&soft, 1_000, 667, &mut soft_rng)
    );

    let mut sharp_rng = StdRng::seed_from_u64(85);
    let mut soft_rng = StdRng::seed_from_u64(85);
    assert_eq!(
        generate_damage_segments(&sharp, 1_000, 667, &mut sharp_rng),
        generate_damage_segments(&soft, 1_000, 667, &mut soft_rng)
    );
}

fn damage_segment(edge: DamageEdge) -> DamageSegment {
    DamageSegment {
        edge,
        along_center: 40.0,
        along_half_length: 22.0,
        penetration: 24.0,
        intensity: 0.7,
        base_softness: 1.0,
        irregularity: DamageProfile {
            values: vec![0.0, 0.0, 0.0],
        },
    }
}

#[test]
fn damage_blur_uses_the_requested_boundary_softness_range() {
    let segment = damage_segment(DamageEdge::Left);
    let short_dimension = 24.0;

    assert_close(
        segment.boundary_softness(short_dimension, BlurParameters::new(0, 120, 80)),
        1.0,
    );
    assert_close(
        segment.boundary_softness(short_dimension, BlurParameters::new(100, 120, 80)),
        0.20 * short_dimension,
    );
}

#[test]
fn generated_damage_intersects_each_requested_frame_edge() {
    for (index, (placement, edge)) in [
        (BokehPlacement::Left, DamageEdge::Left),
        (BokehPlacement::Right, DamageEdge::Right),
        (BokehPlacement::Top, DamageEdge::Top),
        (BokehPlacement::Bottom, DamageEdge::Bottom),
    ]
    .into_iter()
    .enumerate()
    {
        let settings = settings(&[BokehType::Damage], &[placement], 50, 50, 30);
        let mut rng = StdRng::seed_from_u64(98 + index as u64);
        let segments = generate_damage_segments(&settings, 640, 400, &mut rng);

        assert!(!segments.is_empty());
        for segment in segments {
            assert_eq!(segment.edge, edge);
            assert!(segment.intersects_frame(640, 400));

            let mut effects = vec![0.0; 640 * 400];
            segment.rasterize(&mut effects, 640, 400, BlurParameters::new(0, 640, 400));
            let touches_frame = match edge {
                DamageEdge::Left => (0..400).any(|y| effect_at(&effects, 640, 0, y) > 0.0),
                DamageEdge::Right => (0..400).any(|y| effect_at(&effects, 640, 639, y) > 0.0),
                DamageEdge::Top => (0..640).any(|x| effect_at(&effects, 640, x, 0) > 0.0),
                DamageEdge::Bottom => (0..640).any(|x| effect_at(&effects, 640, x, 399) > 0.0),
            };
            assert!(touches_frame, "{edge:?} damage should touch the frame");
        }
    }
}

#[test]
fn damage_ignores_center_when_valid_frame_edges_are_also_requested() {
    let settings = settings(
        &[BokehType::Damage],
        &[
            BokehPlacement::Center,
            BokehPlacement::Left,
            BokehPlacement::Right,
        ],
        50,
        50,
        30,
    );
    let filtered = settings_for_type(&settings, BokehType::Damage);
    let mut rng = StdRng::seed_from_u64(102);
    let segments = generate_damage_segments(&filtered, 640, 400, &mut rng);

    assert_eq!(
        filtered.placements,
        [BokehPlacement::Left, BokehPlacement::Right]
    );
    assert!(segments.iter().all(|segment| {
        matches!(segment.edge, DamageEdge::Left | DamageEdge::Right)
            && segment.intersects_frame(640, 400)
    }));
}

#[test]
fn unplaced_damage_randomizes_across_all_physical_edges() {
    let mut rng = StdRng::seed_from_u64(107);
    let mut seen = [false; 4];

    for _ in 0..256 {
        match select_damage_edge(&[], &mut rng) {
            DamageEdge::Left => seen[0] = true,
            DamageEdge::Right => seen[1] = true,
            DamageEdge::Top => seen[2] = true,
            DamageEdge::Bottom => seen[3] = true,
        }
    }

    assert!(seen.into_iter().all(|edge| edge));
}

#[test]
fn damage_profiles_are_bounded_nonperiodic_and_smoothly_interpolated() {
    let mut rng = StdRng::seed_from_u64(103);
    let profile = DamageProfile::random(8, &mut rng);
    let samples: Vec<_> = (0..=1_000)
        .map(|index| profile.sample(index as f32 / 1_000.0))
        .collect();
    let greatest_step = samples
        .windows(2)
        .map(|pair| (pair[0] - pair[1]).abs())
        .fold(0.0, f32::max);

    assert_eq!(profile.values.len(), 8);
    assert!(profile.values.iter().all(|value| value.abs() <= 0.18));
    assert!(samples.iter().all(|value| value.abs() <= 0.18));
    assert!(
        greatest_step < 0.01,
        "damage profile should not form hard steps"
    );
}

#[test]
fn damage_geometry_is_deterministic_and_varied() {
    let settings = settings(&[BokehType::Damage], &[BokehPlacement::Right], 50, 40, 40);
    let mut first_rng = StdRng::seed_from_u64(104);
    let first = generate_damage_segments(&settings, 640, 400, &mut first_rng);
    let mut second_rng = StdRng::seed_from_u64(104);
    let second = generate_damage_segments(&settings, 640, 400, &mut second_rng);

    assert_eq!(first, second);
    assert!(first.len() > 4);
    assert!(first.iter().any(|segment| {
        (segment.penetration - first[0].penetration).abs() > 0.01
            || (segment.along_half_length - first[0].along_half_length).abs() > 0.01
    }));
}

#[test]
fn damage_size_and_uniformity_control_only_segment_scale() {
    let mut small = settings(&[BokehType::Damage], &[BokehPlacement::Right], 100, 25, 50);
    let mut large = small.clone();
    large.size = 100;
    let mut small_rng = StdRng::seed_from_u64(105);
    let small_segments = generate_damage_segments(&small, 1_000, 667, &mut small_rng);
    let mut large_rng = StdRng::seed_from_u64(105);
    let large_segments = generate_damage_segments(&large, 1_000, 667, &mut large_rng);

    assert_eq!(small_segments.len(), large_segments.len());
    assert!(
        small_segments
            .iter()
            .map(|segment| segment.penetration)
            .fold(0.0, f32::max)
            < large_segments
                .iter()
                .map(|segment| segment.penetration)
                .fold(0.0, f32::max)
    );

    small.uniform = 0;
    let mut uniform = small.clone();
    uniform.uniform = 100;
    let mut low_uniform_rng = StdRng::seed_from_u64(106);
    let low_uniform = generate_damage_segments(&small, 1_000, 667, &mut low_uniform_rng);
    let mut high_uniform_rng = StdRng::seed_from_u64(106);
    let high_uniform = generate_damage_segments(&uniform, 1_000, 667, &mut high_uniform_rng);
    let minimum_penetration = |segments: &[DamageSegment]| {
        segments
            .iter()
            .map(|segment| segment.penetration)
            .fold(f32::INFINITY, f32::min)
    };
    let average_penetration = |segments: &[DamageSegment]| {
        segments
            .iter()
            .map(|segment| segment.penetration)
            .sum::<f32>()
            / segments.len() as f32
    };

    assert_eq!(low_uniform.len(), high_uniform.len());
    assert!(minimum_penetration(&low_uniform) < minimum_penetration(&high_uniform));
    assert!(average_penetration(&low_uniform) < average_penetration(&high_uniform));
    assert!(
        low_uniform
            .iter()
            .all(|segment| segment.edge == DamageEdge::Right)
    );
    assert!(
        high_uniform
            .iter()
            .all(|segment| segment.edge == DamageEdge::Right)
    );
}

#[test]
fn damage_interior_is_uniform_and_boundaries_are_feathered() {
    let segment = damage_segment(DamageEdge::Left);
    let mut effects = vec![0.0; 120 * 80];
    segment.rasterize(&mut effects, 120, 80, BlurParameters::new(100, 120, 80));
    let levels: BTreeSet<_> = effects
        .iter()
        .filter(|effect| **effect > 0.0)
        .map(|effect| (effect * 10_000.0).round() as i32)
        .collect();

    assert_close(
        effect_at(&effects, 120, 5, 30),
        effect_at(&effects, 120, 5, 50),
    );
    assert!(effect_at(&effects, 120, 0, 40) > 0.2);
    assert_eq!(effect_at(&effects, 120, 90, 40), 0.0);
    assert!(
        levels.len() > 16,
        "damage should retain feathered boundaries"
    );
}

#[test]
fn damage_blur_changes_only_the_boundary_feather() {
    let segment = damage_segment(DamageEdge::Left);
    let mut sharp = vec![0.0; 120 * 80];
    segment.rasterize(&mut sharp, 120, 80, BlurParameters::new(0, 120, 80));
    let mut soft = vec![0.0; 120 * 80];
    segment.rasterize(&mut soft, 120, 80, BlurParameters::new(100, 120, 80));

    assert_close(effect_at(&sharp, 120, 5, 40), effect_at(&soft, 120, 5, 40));
    assert_eq!(effect_at(&sharp, 120, 27, 40), 0.0);
    assert!(effect_at(&soft, 120, 27, 40) > 0.0);
}

#[test]
fn unplaced_artifacts_cover_the_full_frame_without_a_center_bias() {
    let mut rng = StdRng::seed_from_u64(79);
    let positions: Vec<_> = (0..512)
        .map(|_| placement_position(None, 1_000, 600, &mut rng))
        .collect();
    let mean_x = positions.iter().map(|position| position.0).sum::<f32>() / positions.len() as f32;
    let mean_y = positions.iter().map(|position| position.1).sum::<f32>() / positions.len() as f32;

    assert!((440.0..=560.0).contains(&mean_x));
    assert!((260.0..=340.0).contains(&mean_y));
    assert!(positions.iter().any(|position| position.0 < 150.0));
    assert!(positions.iter().any(|position| position.0 > 850.0));
}

#[test]
fn scalar_accumulation_has_the_required_union_behavior() {
    assert_close(accumulate_scalar_effect(0.4, 0.0), 0.4);
    assert_close(accumulate_scalar_effect(0.4, 1.0), 1.0);
    assert_close(accumulate_scalar_effect(0.2, 0.4), 0.52);

    let forward = [0.12, 0.38, 0.67]
        .into_iter()
        .fold(0.0, accumulate_scalar_effect);
    let reverse = [0.67, 0.38, 0.12]
        .into_iter()
        .fold(0.0, accumulate_scalar_effect);
    assert_close(forward, reverse);
    assert!((0.0..=1.0).contains(&forward));
}

#[test]
fn bokeh_rendering_is_seeded_deterministic_and_monochrome() {
    let settings = settings(
        &[BokehType::Twinkle, BokehType::Edge, BokehType::Damage],
        &[BokehPlacement::Left],
        50,
        70,
        25,
    );
    let mut first_rng = StdRng::seed_from_u64(80);
    let first = render_image(&settings, &mut first_rng);
    let mut second_rng = StdRng::seed_from_u64(80);
    let second = render_image(&settings, &mut second_rng);

    assert_eq!(first, second);
    assert!(first.pixels().any(|pixel| pixel[3] > 0));
    assert!(first.pixels().all(|pixel| {
        pixel[0] == bokeh_luma(settings.lightness)
            && pixel[1] == bokeh_luma(settings.lightness)
            && pixel[2] == bokeh_luma(settings.lightness)
    }));
}

#[test]
fn default_bokeh_export_flattens_through_the_shared_black_exporter() {
    let (source, exported) = export_bokeh(ExportPolicy::default());

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
fn alpha_bokeh_export_preserves_generated_alpha() {
    let (source, exported) = export_bokeh(ExportPolicy::PreserveAlpha);

    assert_eq!(exported, source);
    assert!(exported.pixels().any(|pixel| pixel[3] == 0));
    assert!(exported.pixels().any(|pixel| pixel[3] > 0));
}

#[test]
fn background_bokeh_export_uses_the_shared_selected_background() {
    let background = [17, 43, 89];
    let (source, exported) = export_bokeh(ExportPolicy::Flatten(RgbColor::new(
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

#[test]
fn bokeh_sequence_continues_after_existing_bokeh_files_and_ignores_other_effects() {
    let output = TestOutputDir::new();
    for number in [1, 2, 7] {
        fs::write(
            output.path().join(format!("bokeh-{number:04}.png")),
            b"existing output",
        )
        .expect("existing bokeh output should be created");
    }
    fs::write(output.path().join("stain-9999.png"), b"unrelated output")
        .expect("unrelated output should be created");
    let mut settings = settings(&[BokehType::Twinkle], &[], 25, 50, 50);
    settings.render.outdir = output.path().to_path_buf();
    let mut first_rng = StdRng::seed_from_u64(81);
    generate_images_with_rng(&settings, &mut first_rng).expect("first bokeh output should write");
    let mut second_rng = StdRng::seed_from_u64(82);
    generate_images_with_rng(&settings, &mut second_rng).expect("second bokeh output should write");

    assert!(output.path().join("bokeh-0008.png").is_file());
    assert!(output.path().join("bokeh-0009.png").is_file());
    assert_eq!(
        fs::read(output.path().join("bokeh-0007.png")).expect("existing output should remain"),
        b"existing output"
    );
}
