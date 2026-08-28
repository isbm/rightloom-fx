use std::collections::BTreeSet;

use image::RgbaImage;
use rand::{Rng, SeedableRng, rngs::StdRng};

use super::{
    CoarseField, DensityField, FieldBounds, MAX_TIDE_RELATIVE_MODULATION,
    SECOND_TIDE_LINE_PROBABILITY, Stain, StainSettings, TideMark, bounded_tide_contribution,
    density_base_alpha, generate_images_with_rng, lightness_luma, render_image, smoothstep,
    tide_presence_probability,
};
use crate::render::{ExportPolicy, RenderError, RenderSettings, Resolution};

fn settings(density: u8, blur: u8, lightness: u8) -> StainSettings {
    settings_with_resolution(640, 400, density, blur, lightness)
}

fn settings_with_resolution(
    width: u32,
    height: u32,
    density: u8,
    blur: u8,
    lightness: u8,
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

fn seeded_stain_with_tide() -> Stain {
    for seed in 0..1_000 {
        let mut rng = StdRng::seed_from_u64(seed);
        let stain = Stain::new((320.0, 200.0), 100.0, 100, 1.0, 100, &mut rng);
        if stain.tide.is_some() {
            return stain;
        }
    }

    panic!("a seeded high-density stain should contain a tide mark");
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

    assert_close(bounded_tide_contribution(0.10, 0.20), 0.02);
    assert_close(bounded_tide_contribution(0.10, 1.35), 0.10);
}

#[test]
fn high_density_stain_keeps_a_broad_body_without_tide_dominance() {
    let stain = seeded_stain_with_tide();
    let mut maximum_relative_tide = 0.0_f32;

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
            let relative_tide = tide_density / local_density;
            maximum_relative_tide = maximum_relative_tide.max(relative_tide);

            assert!(
                relative_tide <= MAX_TIDE_RELATIVE_MODULATION + f32::EPSILON,
                "tide contribution should remain bounded relative to the body"
            );
        }
    }

    assert!(maximum_relative_tide <= MAX_TIDE_RELATIVE_MODULATION + f32::EPSILON);

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
fn seeded_stain_preserves_normalized_macro_structure_across_resolutions() {
    let mut small_rng = StdRng::seed_from_u64(23);
    let small_stain = Stain::new((400.0, 300.0), 100.0, 45, 0.45, 50, &mut small_rng);
    let mut large_rng = StdRng::seed_from_u64(23);
    let large_stain = Stain::new((800.0, 600.0), 200.0, 45, 0.45, 50, &mut large_rng);

    assert_eq!(small_stain.lobes.len(), large_stain.lobes.len());
    for (small_lobe, large_lobe) in small_stain.lobes.iter().zip(&large_stain.lobes) {
        assert!((small_lobe.center_x / 800.0 - large_lobe.center_x / 1600.0).abs() < 0.0001);
        assert!((small_lobe.center_y / 600.0 - large_lobe.center_y / 1200.0).abs() < 0.0001);
        assert!((small_lobe.radius_x / 800.0 - large_lobe.radius_x / 1600.0).abs() < 0.0001);
        assert!((small_lobe.radius_y / 600.0 - large_lobe.radius_y / 1200.0).abs() < 0.0001);
    }

    let mut small_image = RgbaImage::new(800, 600);
    small_stain.rasterize(&mut small_image, 80);
    let mut large_image = RgbaImage::new(1600, 1200);
    large_stain.rasterize(&mut large_image, 80);

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
    let stain = Stain::new((320.0, 200.0), 100.0, 45, 0.45, 50, &mut rng);
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

        assert!(image.pixels().all(|pixel| pixel[3] == 0));
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
            .filter(|pixel| pixel[3] > 0)
            .all(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2])
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

    let composited_lumas: BTreeSet<_> = image
        .pixels()
        .filter(|pixel| pixel[3] >= 8)
        .map(|pixel| {
            let alpha = u16::from(pixel[3]);
            ((u16::from(pixel[0]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
        })
        .collect();
    assert!(
        *composited_lumas.last().expect("stains should be visible")
            - *composited_lumas.first().expect("stains should be visible")
            >= 8,
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
    assert!(alpha_sum(&blur_100) * 100 <= alpha_sum(&hard) * 140);
}

#[test]
fn diffused_alpha_uses_only_the_blurred_outer_mask() {
    let mut rng = StdRng::seed_from_u64(17);
    let stain = Stain::new((320.0, 200.0), 100.0, 45, 0.45, 10, &mut rng);
    let mask = stain.diffused_outer_mask(50);
    let mut image = RgbaImage::new(640, 400);
    stain.rasterize_diffused(&mut image, 50);

    let mut sample = None;
    'rows: for y in 0..image.height() {
        for x in 0..image.width() {
            let world_x = x as f32 + 0.5;
            let world_y = y as f32 + 0.5;
            let raw_coverage = smoothstep(
                -stain.feather,
                stain.feather,
                stain.warped_shape_at(world_x, world_y),
            );
            let blurred_coverage = mask.sample(world_x, world_y).clamp(0.0, 1.0);
            if raw_coverage <= blurred_coverage + 0.25 {
                continue;
            }

            let expected_alpha = (stain.alpha
                * blurred_coverage
                * stain
                    .optical_density_at(stain.warped_shape_at(world_x, world_y), world_x, world_y)
                    .max(0.0))
            .round()
            .clamp(0.0, 255.0) as u8;
            if expected_alpha > 0 {
                sample = Some((x, y, expected_alpha));
                break 'rows;
            }
        }
    }

    let (x, y, expected_alpha) = sample.expect("blurred mask should soften a hard-edge pixel");
    assert_eq!(image.get_pixel(x, y)[3], expected_alpha);
}

#[test]
fn invalid_lightness_is_rejected_before_output_is_created() {
    let mut rng = StdRng::seed_from_u64(18);
    let error = generate_images_with_rng(&settings(30, 50, 101), &mut rng)
        .expect_err("out-of-range lightness should fail validation");

    assert!(matches!(error, RenderError::InvalidLightness(101)));
}

#[test]
fn lightness_never_changes_alpha_geometry_or_blur() {
    let mut low_rng = StdRng::seed_from_u64(19);
    let low = render_image(&settings(45, 80, 10), &mut low_rng);
    let mut mid_rng = StdRng::seed_from_u64(19);
    let mid = render_image(&settings(45, 80, 50), &mut mid_rng);
    let mut high_rng = StdRng::seed_from_u64(19);
    let high = render_image(&settings(45, 80, 100), &mut high_rng);

    assert_eq!(low.dimensions(), mid.dimensions());
    assert_eq!(mid.dimensions(), high.dimensions());

    for ((low_pixel, mid_pixel), high_pixel) in low.pixels().zip(mid.pixels()).zip(high.pixels()) {
        assert_eq!(low_pixel[3], mid_pixel[3]);
        assert_eq!(mid_pixel[3], high_pixel[3]);
    }
}

#[test]
fn average_luma_increases_with_lightness() {
    let average_luma = |lightness: u8| {
        let mut rng = StdRng::seed_from_u64(20);
        let image = render_image(&settings(45, 50, lightness), &mut rng);
        let visible: Vec<_> = image.pixels().filter(|pixel| pixel[3] > 0).collect();
        visible.iter().map(|pixel| u64::from(pixel[0])).sum::<u64>() as f64 / visible.len() as f64
    };

    let lumas: Vec<_> = [10, 25, 50, 75, 100]
        .into_iter()
        .map(average_luma)
        .collect();

    for window in lumas.windows(2) {
        assert!(window[0] < window[1], "lumas should increase: {lumas:?}");
    }
}

#[test]
fn lightness_keeps_internal_luma_variation() {
    let mut rng = StdRng::seed_from_u64(21);
    let image = render_image(&settings(45, 50, 50), &mut rng);
    let visible_lumas: BTreeSet<_> = image
        .pixels()
        .filter(|pixel| pixel[3] > 0)
        .map(|pixel| pixel[0])
        .collect();

    assert!(
        visible_lumas.len() > 1,
        "internal cloudy variation should remain"
    );
}
