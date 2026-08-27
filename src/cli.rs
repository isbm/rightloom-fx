use std::str::FromStr;

#[cfg(test)]
use std::ffi::OsString;

use clap::{Arg, ArgAction, ArgGroup, ArgMatches, Command as ClapCommand, builder::styling};
use colored::Colorize;

use crate::{
    render::{RenderSettings, Resolution},
    scratches::ScratchType,
};

const APP_NAME: &str = "rightloom-fx";

#[derive(Debug)]
pub(crate) struct Cli {
    pub(crate) command: Command,
}

#[derive(Debug)]
pub(crate) enum Command {
    Scratches(ScratchesArgs),
    Stain(StainArgs),
}

#[derive(Debug)]
pub(crate) struct ScratchesArgs {
    pub(crate) render: RenderSettings,
    pub(crate) effects: Vec<ScratchType>,
}

#[derive(Debug)]
pub(crate) struct StainArgs {
    pub(crate) render: RenderSettings,
}

impl Cli {
    pub(crate) fn parse() -> Self {
        let matches = cli().get_matches();
        Self::from_matches(&matches)
    }

    #[cfg(test)]
    pub(crate) fn try_parse_from<I, T>(arguments: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let matches = cli().try_get_matches_from(arguments)?;
        Ok(Self::from_matches(&matches))
    }

    fn from_matches(matches: &ArgMatches) -> Self {
        let (name, arguments) = matches
            .subcommand()
            .expect("clap requires a supported subcommand");

        let command = match name {
            "scratches" => Command::Scratches(ScratchesArgs {
                render: render_settings(arguments),
                effects: arguments
                    .get_many("type")
                    .expect("clap requires at least one scratch type")
                    .copied()
                    .collect(),
            }),
            "stain" => Command::Stain(StainArgs {
                render: render_settings(arguments),
            }),
            _ => unreachable!("clap only exposes configured subcommands"),
        };

        Self { command }
    }
}

fn render_settings(arguments: &ArgMatches) -> RenderSettings {
    RenderSettings {
        resolution: arguments
            .get_one::<Resolution>("resolution")
            .or_else(|| arguments.get_one("aspect-ratio"))
            .copied()
            .expect("clap requires a resolution"),
        density: *arguments
            .get_one("density")
            .expect("clap requires a density"),
        amount: *arguments
            .get_one("amount")
            .expect("clap requires an amount"),
        outdir: arguments
            .get_one::<String>("outdir")
            .expect("clap requires an output directory")
            .into(),
    }
}

pub(crate) fn cli() -> ClapCommand {
    let styles = styling::Styles::styled()
        .header(styling::AnsiColor::Yellow.on_default())
        .usage(styling::AnsiColor::Yellow.on_default())
        .literal(styling::AnsiColor::BrightGreen.on_default())
        .placeholder(styling::AnsiColor::BrightMagenta.on_default());

    ClapCommand::new(APP_NAME)
        .version(env!("CARGO_PKG_VERSION"))
        .about(format!(
            "{} - generate transparent film-effect overlays",
            APP_NAME.bright_magenta().bold()
        ))
        .arg_required_else_help(true)
        .subcommand_required(true)
        .subcommand(scratches_command(styles.clone()))
        .subcommand(stain_command(styles.clone()))
        .disable_colored_help(false)
        .styles(styles)
}

fn scratches_command(styles: styling::Styles) -> ClapCommand {
    let types = ScratchType::names().join(", ");
    shared_render_arguments(
        ClapCommand::new("scratches")
            .about("Generate transparent PNG overlays of physical film defects")
            .styles(styles)
            .arg(
                Arg::new("type")
                    .short('t')
                    .long("type")
                    .value_name("TYPE")
                    .help(format!(
                        "Scratch effect to combine; repeat for multiple effects. Available types: {}",
                        types.bright_green()
                    ))
                    .required(true)
                    .action(ArgAction::Append)
                    .value_parser(parse_scratch_type),
            )
            .after_help(format!(
                "{}\n  {}  tiny irregular film-dust artifacts\n  {}  thin, broken transport scratches\n  {}    soft curved crease and stress marks",
                "Supported scratch types:".bright_yellow().bold(),
                "dust".bright_green(),
                "camera".bright_green(),
                "bend".bright_green(),
            )),
    )
}

fn stain_command(styles: styling::Styles) -> ClapCommand {
    shared_render_arguments(
        ClapCommand::new("stain")
            .about("Generate transparent PNG overlays of analog-film development stains")
            .styles(styles),
    )
}

fn shared_render_arguments(command: ClapCommand) -> ClapCommand {
    command
        .arg(
            Arg::new("resolution")
                .short('r')
                .long("resolution")
                .value_name("WIDTHxHEIGHT")
                .help("Output dimensions, for example 3840x2160")
                .value_parser(parse_resolution),
        )
        .arg(
            Arg::new("aspect-ratio")
                .short('R')
                .long("aspect-ratio")
                .value_name("RATIOxLONG_SIDE")
                .help("Generate using an aspect ratio and longest output side")
                .long_help(
                    "Generate using a width:height ratio and the longest output side.\n\nExamples:\n  3:2x6240\n  2:3x6240\n  16:9x3840\n  1:1x4000\n\nCommon ratios (examples only):\n  3:2   common still-camera format\n  4:3   digital / Micro Four Thirds\n  1:1   square\n  4:5   portrait\n  5:4   print / large-format\n  16:9  widescreen\n  9:16  vertical widescreen\n  2:1   panoramic\n  21:9  ultrawide / cinematic",
                )
                .value_parser(parse_aspect_ratio),
        )
        .arg(
            Arg::new("density")
                .short('d')
                .long("density")
                .value_name("PERCENT")
                .help("Effect density from 0 to 100")
                .required(true)
                .value_parser(parse_density),
        )
        .arg(
            Arg::new("amount")
                .short('a')
                .long("amount")
                .value_name("COUNT")
                .help("Number of images to generate")
                .required(true)
                .value_parser(parse_amount),
        )
        .arg(
            Arg::new("outdir")
                .short('o')
                .long("outdir")
                .value_name("PATH")
                .help("Directory for numbered PNG outputs")
                .required(true),
        )
        .group(
            ArgGroup::new("dimensions")
                .args(["resolution", "aspect-ratio"])
                .required(true)
                .multiple(false),
        )
}

pub(crate) fn parse_resolution(value: &str) -> Result<Resolution, String> {
    Resolution::from_str(value).map_err(|error| error.to_string())
}

pub(crate) fn parse_aspect_ratio(value: &str) -> Result<Resolution, String> {
    Resolution::from_aspect_ratio(value).map_err(|error| error.to_string())
}

pub(crate) fn parse_density(value: &str) -> Result<u8, String> {
    let density = value
        .parse::<u16>()
        .map_err(|_| "density must be an integer between 0 and 100".to_owned())?;

    if density > 100 {
        return Err("density must be between 0 and 100".to_owned());
    }

    Ok(density as u8)
}

pub(crate) fn parse_scratch_type(value: &str) -> Result<ScratchType, String> {
    ScratchType::from_str(value)
}

pub(crate) fn parse_amount(value: &str) -> Result<u32, String> {
    let amount = value
        .parse::<u32>()
        .map_err(|_| "amount must be an integer of at least 1".to_owned())?;

    if amount == 0 {
        return Err("amount must be at least 1".to_owned());
    }

    Ok(amount)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{Cli, Command, cli, parse_amount, parse_density, parse_scratch_type};
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
        let resolution = match cli.command {
            Command::Stain(args) => args.render.resolution,
            Command::Scratches(_) => panic!("stain command should parse as stain"),
        };

        assert_eq!((resolution.width(), resolution.height()), (6240, 4160));
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
            "-a",
            "1",
            "-o",
            "output",
        ])
        .expect("aspect ratio should parse");
        let resolution = match cli.command {
            Command::Stain(args) => args.render.resolution,
            Command::Scratches(_) => panic!("stain command should parse as stain"),
        };

        assert_eq!((resolution.width(), resolution.height()), (4160, 6240));
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
}
