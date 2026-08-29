use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicUsize, Ordering},
};

use rand::{SeedableRng, rngs::StdRng};

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
fn bokeh_settings_validate_blur_and_edge_placement() {
    let mut invalid_blur = settings(&[BokehType::Twinkle], &[], 50, 50, 50);
    invalid_blur.blur = 101;
    assert!(matches!(
        invalid_blur.validate(),
        Err(RenderError::InvalidBlur(101))
    ));

    let invalid_edge_placement =
        settings(&[BokehType::Edge], &[BokehPlacement::Center], 50, 50, 50);
    assert!(matches!(
        invalid_edge_placement.validate(),
        Err(RenderError::InvalidBokehEdgePlacement)
    ));
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
fn size_controls_scale_without_changing_primitive_count() {
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

    assert_eq!(small_twinkles.len(), large_twinkles.len());
    assert!(small_maximum < large_maximum);
}

#[test]
fn density_controls_counts_without_changing_size_distribution() {
    assert_eq!(twinkle_count(0), 0);
    assert!(twinkle_count(10) < twinkle_count(25));
    assert!(twinkle_count(25) < twinkle_count(50));
    assert!(twinkle_count(50) < twinkle_count(75));
    assert!(twinkle_count(75) < twinkle_count(100));
    assert!(damage_count(50) > twinkle_count(50));
    assert!(edge_count(25) < edge_count(100));

    let mut first_rng = StdRng::seed_from_u64(76);
    let first = sample_object_scale(80, 25, &mut first_rng);
    let mut second_rng = StdRng::seed_from_u64(76);
    let second = sample_object_scale(80, 25, &mut second_rng);
    assert_eq!(first, second);
}

#[test]
fn density_zero_produces_an_empty_scalar_render() {
    let mut rng = StdRng::seed_from_u64(77);
    let image = render_image(
        &settings(&[BokehType::Twinkle, BokehType::Edge], &[], 0, 50, 50),
        &mut rng,
    );

    assert!(image.pixels().all(|pixel| pixel.0 == [255, 255, 255, 0]));
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
    twinkle.rasterize(&mut effects, 100, 100, BlurParameters::new(100, 100, 100));

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
    twinkle.rasterize(&mut effects, 120, 80, BlurParameters::new(0, 120, 80));
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
    twinkle.rasterize(&mut sharp, 120, 80, BlurParameters::new(0, 120, 80));
    let mut soft = vec![0.0; 120 * 80];
    twinkle.rasterize(&mut soft, 120, 80, BlurParameters::new(100, 120, 80));

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
    twinkle.rasterize(&mut effects, 100, 60, BlurParameters::new(100, 100, 60));

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

#[test]
fn damage_segments_have_varied_deterministic_geometry() {
    let settings = settings(&[BokehType::Damage], &[BokehPlacement::Right], 50, 40, 40);
    let mut first_rng = StdRng::seed_from_u64(78);
    let first = generate_damage_segments(&settings, 640, 400, &mut first_rng);
    let mut second_rng = StdRng::seed_from_u64(78);
    let second = generate_damage_segments(&settings, 640, 400, &mut second_rng);

    assert_eq!(first, second);
    assert!(first.len() > 4);
    assert!(first.iter().any(|segment| {
        (segment.half_width - first[0].half_width).abs() > 0.01
            || (segment.half_height - first[0].half_height).abs() > 0.01
    }));
}

#[test]
fn damage_boundaries_are_soft_and_not_binary_rectangles() {
    let segment = DamageSegment {
        center_x: 60.0,
        center_y: 40.0,
        half_width: 25.0,
        half_height: 14.0,
        sin_angle: 0.0,
        cos_angle: 1.0,
        intensity: 0.7,
        softness: 6.0,
        deformation: 2.0,
        x_frequency: 0.7,
        y_frequency: 0.9,
        x_phase: 0.2,
        y_phase: 1.1,
        fragment_frequency: 0.6,
        fragment_phase: 0.4,
    };
    let mut effects = vec![0.0; 120 * 80];
    segment.rasterize(&mut effects, 120, 80, BlurParameters::new(100, 120, 80));
    let levels: BTreeSet<_> = effects
        .iter()
        .filter(|effect| **effect > 0.0)
        .map(|effect| (effect * 10_000.0).round() as i32)
        .collect();

    assert!(effect_at(&effects, 120, 60, 40) > 0.2);
    assert_eq!(effect_at(&effects, 120, 110, 40), 0.0);
    assert!(
        levels.len() > 16,
        "damage should contain soft intermediate values"
    );
}

#[test]
fn damage_blur_changes_only_the_boundary_feather() {
    let segment = DamageSegment {
        center_x: 60.0,
        center_y: 40.0,
        half_width: 25.0,
        half_height: 14.0,
        sin_angle: 0.0,
        cos_angle: 1.0,
        intensity: 0.7,
        softness: 6.0,
        deformation: 0.0,
        x_frequency: 0.7,
        y_frequency: 0.9,
        x_phase: 0.2,
        y_phase: 1.1,
        fragment_frequency: 0.6,
        fragment_phase: 0.4,
    };
    let mut sharp = vec![0.0; 120 * 80];
    segment.rasterize(&mut sharp, 120, 80, BlurParameters::new(0, 120, 80));
    let mut soft = vec![0.0; 120 * 80];
    segment.rasterize(&mut soft, 120, 80, BlurParameters::new(100, 120, 80));

    assert_close(
        effect_at(&sharp, 120, 60, 40),
        effect_at(&soft, 120, 60, 40),
    );
    assert_eq!(effect_at(&sharp, 120, 88, 40), 0.0);
    assert!(effect_at(&soft, 120, 88, 40) > 0.0);
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
    assert!(
        first
            .pixels()
            .all(|pixel| pixel[0] == 255 && pixel[1] == 255 && pixel[2] == 255)
    );
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
