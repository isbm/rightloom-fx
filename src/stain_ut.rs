use std::collections::BTreeSet;

use image::RgbaImage;
use rand::{SeedableRng, rngs::StdRng};

use super::{Stain, StainSettings, generate_images_with_rng, render_image, smoothstep};
use crate::render::{ExportPolicy, RenderError, RenderSettings, Resolution};

fn settings(density: u8, blur: u8, strength: u8) -> StainSettings {
    StainSettings {
        render: RenderSettings {
            resolution: Resolution::new(640, 400).expect("test resolution should be valid"),
            density,
            amount: 1,
            outdir: "unused".into(),
            export_policy: ExportPolicy::default(),
        },
        blur,
        strength,
    }
}

#[test]
fn rendered_image_has_requested_dimensions() {
    let mut rng = StdRng::seed_from_u64(10);
    let image = render_image(&settings(30, 50, 50), &mut rng);

    assert_eq!(image.dimensions(), (640, 400));
}

#[test]
fn zero_density_is_transparent() {
    for (seed, blur) in [(11, 0), (12, 100)] {
        let mut rng = StdRng::seed_from_u64(seed);
        let image = render_image(&settings(0, blur, 50), &mut rng);

        assert!(image.pixels().all(|pixel| pixel[3] == 0));
    }
}

#[test]
fn nonzero_density_modifies_monochrome_pixels() {
    let mut rng = StdRng::seed_from_u64(12);
    let image = render_image(&settings(30, 50, 50), &mut rng);

    assert!(image.pixels().any(|pixel| pixel[3] > 0));
    assert!(
        image
            .pixels()
            .filter(|pixel| pixel[3] > 0)
            .all(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2])
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
        "high-strength stains should retain visible internal variation"
    );
}

#[test]
fn low_density_keeps_most_of_the_canvas_transparent() {
    let mut rng = StdRng::seed_from_u64(14);
    let image = render_image(&settings(5, 50, 50), &mut rng);
    let transparent = image.pixels().filter(|pixel| pixel[3] == 0).count();

    assert!(transparent * 100 / image.pixels().len() >= 70);
}

#[test]
fn invalid_blur_is_rejected_before_output_is_created() {
    let mut rng = StdRng::seed_from_u64(15);
    let error = generate_images_with_rng(&settings(30, 101, 50), &mut rng)
        .expect_err("out-of-range blur should fail validation");

    assert!(matches!(error, RenderError::InvalidBlur(101)));
}

#[test]
fn increasing_blur_broadens_the_transition() {
    let mut hard_rng = StdRng::seed_from_u64(16);
    let hard = render_image(&settings(45, 0, 50), &mut hard_rng);
    let mut blur_25_rng = StdRng::seed_from_u64(16);
    let blur_25 = render_image(&settings(45, 25, 50), &mut blur_25_rng);
    let mut blur_50_rng = StdRng::seed_from_u64(16);
    let blur_50 = render_image(&settings(45, 50, 50), &mut blur_50_rng);
    let mut blur_75_rng = StdRng::seed_from_u64(16);
    let blur_75 = render_image(&settings(45, 75, 50), &mut blur_75_rng);
    let mut blur_100_rng = StdRng::seed_from_u64(16);
    let blur_100 = render_image(&settings(45, 100, 50), &mut blur_100_rng);

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
    let stain = Stain::new((320.0, 200.0), 100.0, 0.45, 50, &mut rng);
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
fn invalid_strength_is_rejected_before_output_is_created() {
    let mut rng = StdRng::seed_from_u64(18);
    let error = generate_images_with_rng(&settings(30, 50, 101), &mut rng)
        .expect_err("out-of-range strength should fail validation");

    assert!(matches!(error, RenderError::InvalidStrength(101)));
}

#[test]
fn strength_changes_luma_without_changing_alpha_or_dimensions() {
    let mut light_rng = StdRng::seed_from_u64(19);
    let light = render_image(&settings(45, 75, 0), &mut light_rng);
    let mut medium_rng = StdRng::seed_from_u64(19);
    let medium = render_image(&settings(45, 75, 50), &mut medium_rng);
    let mut dark_rng = StdRng::seed_from_u64(19);
    let dark = render_image(&settings(45, 75, 100), &mut dark_rng);

    assert_eq!(light.dimensions(), medium.dimensions());
    assert_eq!(medium.dimensions(), dark.dimensions());

    for ((light_pixel, medium_pixel), dark_pixel) in
        light.pixels().zip(medium.pixels()).zip(dark.pixels())
    {
        assert_eq!(light_pixel[3], medium_pixel[3]);
        assert_eq!(medium_pixel[3], dark_pixel[3]);
    }

    let average_luma = |image: &image::RgbaImage| {
        let visible: Vec<_> = image.pixels().filter(|pixel| pixel[3] > 0).collect();
        visible.iter().map(|pixel| u64::from(pixel[0])).sum::<u64>() as f64 / visible.len() as f64
    };

    let light_luma = average_luma(&light);
    let medium_luma = average_luma(&medium);
    let dark_luma = average_luma(&dark);

    assert!((220.0..=235.0).contains(&light_luma));
    assert!((115.0..=135.0).contains(&medium_luma));
    assert!((15.0..=30.0).contains(&dark_luma));
    assert!(light_luma > medium_luma);
    assert!(medium_luma > dark_luma);
    assert!(light.pixels().any(|pixel| pixel[3] > 0));
}
