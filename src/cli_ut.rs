use std::str::FromStr;

use super::{
    Cli, Command, cli, parse_amount, parse_background, parse_blur, parse_contrast, parse_density,
    parse_lightness, parse_scratch_type,
};
use crate::render::{ExportPolicy, RgbColor};
use crate::scratches::{Resolution, ScratchType};

#[test]
fn resolution_parses_dimensions() {
    let resolution = Resolution::from_str("3840x2160").expect("resolution should parse");

    assert_eq!(resolution.width(), 3840);
    assert_eq!(resolution.height(), 2160);
}

#[test]
fn malformed_resolutions_are_rejected() {
    for value in ["3840", "3840x", "x2160", "3840x2160x1", "wide x tall"] {
        assert!(Resolution::from_str(value).is_err(), "{value} should fail");
    }

    assert!(Resolution::from_str("0x2160").is_err());
    assert!(Resolution::from_str("3840x0").is_err());
}

#[test]
fn density_is_limited_to_a_percentage() {
    assert_eq!(parse_density("0"), Ok(0));
    assert_eq!(parse_density("100"), Ok(100));
    assert!(parse_density("101").is_err());
    assert!(parse_density("-1").is_err());
}

#[test]
fn blur_is_limited_to_a_percentage() {
    assert_eq!(parse_blur("0"), Ok(0));
    assert_eq!(parse_blur("1"), Ok(1));
    assert_eq!(parse_blur("50"), Ok(50));
    assert_eq!(parse_blur("99"), Ok(99));
    assert_eq!(parse_blur("100"), Ok(100));
    assert!(parse_blur("101").is_err());
    assert!(parse_blur("-1").is_err());
}

#[test]
fn lightness_is_limited_to_a_percentage() {
    for value in ["0", "1", "10", "25", "50", "75", "99", "100"] {
        let parsed = parse_lightness(value).expect("in-range lightness should parse");
        assert_eq!(parsed, value.parse().expect("test value should be numeric"));
    }

    assert!(parse_lightness("101").is_err());
    assert!(parse_lightness("-1").is_err());
}

#[test]
fn contrast_is_limited_to_a_percentage() {
    for value in ["0", "1", "25", "50", "75", "99", "100"] {
        let parsed = parse_contrast(value).expect("in-range contrast should parse");
        assert_eq!(parsed, value.parse().expect("test value should be numeric"));
    }

    assert!(parse_contrast("101").is_err());
    assert!(parse_contrast("-1").is_err());
}

#[test]
fn stain_cli_accepts_contrast_endpoints_and_default() {
    for contrast in [0, 50, 100] {
        let cli = Cli::try_parse_from([
            "rightloom-fx".to_owned(),
            "stain".to_owned(),
            "-r".to_owned(),
            "320x200".to_owned(),
            "-d".to_owned(),
            "10".to_owned(),
            "-c".to_owned(),
            contrast.to_string(),
            "-a".to_owned(),
            "1".to_owned(),
            "-o".to_owned(),
            "output".to_owned(),
        ])
        .expect("in-range contrast should parse");

        let parsed = match cli.command {
            Command::Stain(args) => args.contrast,
            Command::Scratches(_) => panic!("stain command should parse as stain"),
        };
        assert_eq!(parsed, contrast);
    }
}

#[test]
fn backgrounds_parse_as_six_digit_hex_colors() {
    assert_eq!(parse_background("ffffff"), Ok(RgbColor::new(255, 255, 255)));
    assert_eq!(parse_background("#000000"), Ok(RgbColor::new(0, 0, 0)));
    assert_eq!(parse_background("#FF44DD"), Ok(RgbColor::new(255, 68, 221)));
}

#[test]
fn malformed_background_colors_are_rejected() {
    for value in ["", "#", "fff", "00000000", "gg0000", "#12", "##000000"] {
        assert!(parse_background(value).is_err(), "{value:?} should fail");
    }
}

#[test]
fn amount_must_be_positive() {
    assert_eq!(parse_amount("1"), Ok(1));
    assert!(parse_amount("0").is_err());
}

#[test]
fn scratch_types_are_parsed_and_validated() {
    assert_eq!(parse_scratch_type("dust"), Ok(ScratchType::Dust));
    assert_eq!(parse_scratch_type("camera"), Ok(ScratchType::Camera));
    assert_eq!(parse_scratch_type("bend"), Ok(ScratchType::Bend));
    assert!(parse_scratch_type("unknown").is_err());
}

#[test]
fn cli_rejects_unknown_scratch_types() {
    let result = Cli::try_parse_from([
        "rightloom-fx",
        "scratches",
        "-r",
        "320x200",
        "-d",
        "10",
        "-t",
        "unknown",
        "-a",
        "1",
        "-o",
        "output",
    ]);

    assert!(result.is_err());
}

#[test]
fn stain_cli_accepts_exact_resolution() {
    let cli = Cli::try_parse_from([
        "rightloom-fx",
        "stain",
        "-r",
        "6240x4160",
        "-d",
        "10",
        "-a",
        "1",
        "-o",
        "output",
    ])
    .expect("exact resolution should parse");
    let (resolution, blur, lightness, contrast, export_policy) = match cli.command {
        Command::Stain(args) => (
            args.render.resolution,
            args.blur,
            args.lightness,
            args.contrast,
            args.render.export_policy,
        ),
        Command::Scratches(_) => panic!("stain command should parse as stain"),
    };

    assert_eq!((resolution.width(), resolution.height()), (6240, 4160));
    assert_eq!(blur, 50);
    assert_eq!(lightness, 10);
    assert_eq!(contrast, 50);
    assert_eq!(export_policy, ExportPolicy::Flatten(RgbColor::BLACK));
}

#[test]
fn stain_cli_accepts_aspect_ratio_resolution() {
    let cli = Cli::try_parse_from([
        "rightloom-fx",
        "stain",
        "-R",
        "2:3x6240",
        "-d",
        "10",
        "-b",
        "100",
        "-l",
        "100",
        "-c",
        "75",
        "--background=ffffff",
        "-a",
        "1",
        "-o",
        "output",
    ])
    .expect("aspect ratio should parse");
    let (resolution, blur, lightness, contrast, export_policy) = match cli.command {
        Command::Stain(args) => (
            args.render.resolution,
            args.blur,
            args.lightness,
            args.contrast,
            args.render.export_policy,
        ),
        Command::Scratches(_) => panic!("stain command should parse as stain"),
    };

    assert_eq!((resolution.width(), resolution.height()), (4160, 6240));
    assert_eq!(blur, 100);
    assert_eq!(lightness, 100);
    assert_eq!(contrast, 75);
    assert_eq!(
        export_policy,
        ExportPolicy::Flatten(RgbColor::new(255, 255, 255))
    );
}

#[test]
fn stain_cli_alpha_selects_transparent_export() {
    let cli = Cli::try_parse_from([
        "rightloom-fx",
        "stain",
        "-r",
        "320x200",
        "-d",
        "10",
        "--alpha",
        "-a",
        "1",
        "-o",
        "output",
    ])
    .expect("alpha export should parse");

    let export_policy = match cli.command {
        Command::Stain(args) => args.render.export_policy,
        Command::Scratches(_) => panic!("stain command should parse as stain"),
    };

    assert_eq!(export_policy, ExportPolicy::PreserveAlpha);
}

#[test]
fn cli_requires_exactly_one_dimension_input() {
    let neither = Cli::try_parse_from([
        "rightloom-fx",
        "stain",
        "-d",
        "10",
        "-a",
        "1",
        "-o",
        "output",
    ]);
    let both = Cli::try_parse_from([
        "rightloom-fx",
        "stain",
        "-r",
        "6240x4160",
        "-R",
        "3:2x6240",
        "-d",
        "10",
        "-a",
        "1",
        "-o",
        "output",
    ]);

    assert!(neither.is_err());
    assert!(both.is_err());
}

#[test]
fn scratches_help_lists_supported_types() {
    let command = cli();
    let error = command
        .try_get_matches_from(["rightloom-fx", "scratches", "--help"])
        .expect_err("help should short-circuit parsing");
    let help = error.to_string();

    assert!(help.contains("dust"));
    assert!(help.contains("camera"));
    assert!(help.contains("bend"));
    assert!(help.contains("3:2x6240"));
    assert!(help.contains("21:9"));
}

#[test]
fn stain_help_documents_blur_lightness_and_contrast() {
    let command = cli();
    let error = command
        .try_get_matches_from(["rightloom-fx", "stain", "--help"])
        .expect_err("help should short-circuit parsing");
    let help = error.to_string();

    assert!(help.contains("-b, --blur <PERCENT>"));
    assert!(help.contains("Stain edge softness, 0-100."));
    assert!(help.contains("0 = hard edges, 100 = maximum diffusion."));
    assert!(help.contains("-l, --lightness <PERCENT>"));
    assert!(help.contains("Stain brightness, 0-100."));
    assert!(help.contains("10 matches the current/default appearance."));
    assert!(help.contains("0 = nearly black, 100 = near-white."));
    assert!(help.contains("[default: 10]"));
    assert!(help.contains("-c, --contrast <PERCENT>"));
    assert!(help.contains("Internal stain contrast, 0-100."));
    assert!(help.contains("50 preserves the default/current contrast."));
    assert!(help.contains("[default: 50]"));
    assert!(help.contains("--alpha"));
    assert!(help.contains("--background <RRGGBB>"));
}

#[test]
fn scratches_help_does_not_expose_stain_only_controls() {
    let command = cli();
    let error = command
        .try_get_matches_from(["rightloom-fx", "scratches", "--help"])
        .expect_err("help should short-circuit parsing");
    let help = error.to_string();

    assert!(!help.contains("--lightness"));
    assert!(!help.contains("--contrast"));
}

#[test]
fn alpha_and_background_conflict() {
    let error = Cli::try_parse_from([
        "rightloom-fx",
        "stain",
        "-r",
        "320x200",
        "-d",
        "10",
        "--alpha",
        "--background=ffffff",
        "-a",
        "1",
        "-o",
        "output",
    ])
    .expect_err("alpha and background should conflict");

    assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
}

#[test]
fn scratches_reject_stain_blur() {
    let result = Cli::try_parse_from([
        "rightloom-fx",
        "scratches",
        "-r",
        "320x200",
        "-d",
        "10",
        "-t",
        "dust",
        "-b",
        "50",
        "-a",
        "1",
        "-o",
        "output",
    ]);

    assert!(result.is_err());
}

#[test]
fn scratches_reject_stain_lightness() {
    let result = Cli::try_parse_from([
        "rightloom-fx",
        "scratches",
        "-r",
        "320x200",
        "-d",
        "10",
        "-t",
        "dust",
        "-l",
        "50",
        "-a",
        "1",
        "-o",
        "output",
    ]);

    assert!(result.is_err());
}

#[test]
fn scratches_reject_stain_contrast() {
    let result = Cli::try_parse_from([
        "rightloom-fx",
        "scratches",
        "-r",
        "320x200",
        "-d",
        "10",
        "-t",
        "dust",
        "-c",
        "50",
        "-a",
        "1",
        "-o",
        "output",
    ]);

    assert!(result.is_err());
}

#[test]
fn stain_rejects_out_of_range_blur() {
    let result = Cli::try_parse_from([
        "rightloom-fx",
        "stain",
        "-r",
        "320x200",
        "-d",
        "10",
        "-b",
        "101",
        "-a",
        "1",
        "-o",
        "output",
    ]);

    assert!(result.is_err());
}

#[test]
fn stain_rejects_out_of_range_lightness() {
    let result = Cli::try_parse_from([
        "rightloom-fx",
        "stain",
        "-r",
        "320x200",
        "-d",
        "10",
        "-l",
        "101",
        "-a",
        "1",
        "-o",
        "output",
    ]);

    assert!(result.is_err());
}

#[test]
fn stain_rejects_out_of_range_contrast() {
    let result = Cli::try_parse_from([
        "rightloom-fx",
        "stain",
        "-r",
        "320x200",
        "-d",
        "10",
        "-c",
        "101",
        "-a",
        "1",
        "-o",
        "output",
    ]);

    assert!(result.is_err());
}
