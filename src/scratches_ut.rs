use std::str::FromStr;

use rand::{SeedableRng, rngs::StdRng};

use crate::render::{ExportPolicy, RenderSettings};

use super::{Resolution, ScratchSettings, ScratchType, render_image};

fn settings(effect: ScratchType) -> ScratchSettings {
    ScratchSettings {
        render: RenderSettings {
            resolution: Resolution::new(320, 200).expect("test resolution should be valid"),
            density: 35,
            amount: 1,
            outdir: "unused".into(),
            export_policy: ExportPolicy::default(),
        },
        effects: vec![effect],
    }
}

#[test]
fn exact_resolutions_parse() {
    for (input, expected) in [("6240x4160", (6240, 4160)), ("3840x2160", (3840, 2160))] {
        let resolution = Resolution::from_str(input).expect("resolution should parse");

        assert_eq!((resolution.width(), resolution.height()), expected);
    }
}

#[test]
fn aspect_ratios_resolve_to_output_dimensions() {
    for (input, expected) in [
        ("3:2x4000", (4000, 2667)),
        ("2:3x4000", (2667, 4000)),
        ("16:9x3840", (3840, 2160)),
        ("9:16x3840", (2160, 3840)),
        ("1:1x4000", (4000, 4000)),
        ("3:2x6240", (6240, 4160)),
        ("2:3x6240", (4160, 6240)),
        ("7:5x5000", (5000, 3571)),
    ] {
        let resolution = Resolution::from_aspect_ratio(input).expect("aspect ratio should resolve");

        assert_eq!(
            (resolution.width(), resolution.height()),
            expected,
            "{input}"
        );
    }
}

#[test]
fn malformed_dimension_inputs_are_rejected() {
    for input in ["6240", "6240x", "x4160", "6240x4160x1", "0x4160", "6240x0"] {
        assert!(Resolution::from_str(input).is_err(), "{input} should fail");
    }

    for input in [
        "3:2",
        "3:2x",
        "x4000",
        ":2x4000",
        "3:x4000",
        "3:2:1x4000",
        "3/2x4000",
        "3:2x4000x1",
        "0:2x4000",
        "3:0x4000",
        "3:2x0",
        "1:3x1",
        "18446744073709551616:2x4000",
        "3:2x4294967296",
    ] {
        assert!(
            Resolution::from_aspect_ratio(input).is_err(),
            "{input} should fail"
        );
    }
}

#[test]
fn rendered_image_has_requested_dimensions() {
    let mut rng = StdRng::seed_from_u64(1);
    let image = render_image(&settings(ScratchType::Dust), &mut rng);

    assert_eq!(image.dimensions(), (320, 200));
}

#[test]
fn effects_leave_a_transparent_background() {
    for (seed, effect) in [
        (2, ScratchType::Dust),
        (3, ScratchType::Camera),
        (4, ScratchType::Bend),
    ] {
        let mut rng = StdRng::seed_from_u64(seed);
        let image = render_image(&settings(effect), &mut rng);

        assert!(image.pixels().any(|pixel| pixel[3] > 0));
        assert!(image.pixels().any(|pixel| pixel[3] == 0));
    }
}

#[test]
fn every_supported_effect_renders_without_failure() {
    for (seed, effect) in [
        (5, ScratchType::Dust),
        (6, ScratchType::Camera),
        (7, ScratchType::Bend),
    ] {
        let mut rng = StdRng::seed_from_u64(seed);
        let image = render_image(&settings(effect), &mut rng);

        assert!(image.pixels().any(|pixel| pixel[3] > 0));
    }
}

#[test]
fn zero_density_is_transparent() {
    let mut settings = settings(ScratchType::Dust);
    settings.render.density = 0;
    let mut rng = StdRng::seed_from_u64(8);
    let image = render_image(&settings, &mut rng);

    assert!(image.pixels().all(|pixel| pixel[3] == 0));
}
