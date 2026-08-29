use std::{ffi::OsString, str::FromStr};

use clap::{Arg, ArgAction, ArgGroup, ArgMatches, Command as ClapCommand, builder::styling};
use colored::Colorize;

use crate::{
    bokeh::{BokehPlacement, BokehType},
    render::{ExportPolicy, RenderSettings, Resolution, RgbColor},
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
    Bokeh(BokehArgs),
}

#[derive(Debug)]
pub(crate) struct ScratchesArgs {
    pub(crate) render: RenderSettings,
    pub(crate) effects: Vec<ScratchType>,
}

#[derive(Debug)]
pub(crate) struct StainArgs {
    pub(crate) render: RenderSettings,
    pub(crate) blur: u8,
    pub(crate) lightness: u8,
    pub(crate) contrast: u8,
}

#[derive(Debug)]
pub(crate) struct BokehArgs {
    pub(crate) render: RenderSettings,
    pub(crate) types: Vec<BokehType>,
    pub(crate) placements: Vec<BokehPlacement>,
    pub(crate) blur: u8,
    pub(crate) lightness: u8,
    pub(crate) deform: u8,
    pub(crate) size: u8,
    pub(crate) uniform: u8,
}

impl Cli {
    pub(crate) fn parse() -> Self {
        Self::try_parse_from(std::env::args_os()).unwrap_or_else(|error| error.exit())
    }

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
                blur: *arguments
                    .get_one("blur")
                    .expect("clap supplies the stain blur default"),
                lightness: *arguments
                    .get_one("lightness")
                    .expect("clap supplies the stain lightness default"),
                contrast: *arguments
                    .get_one("contrast")
                    .expect("clap supplies the stain contrast default"),
            }),
            "bokeh" => Command::Bokeh(BokehArgs {
                render: render_settings(arguments),
                types: arguments
                    .get_many("type")
                    .expect("clap requires at least one bokeh type")
                    .copied()
                    .collect(),
                placements: bokeh_placements(arguments),
                blur: *arguments
                    .get_one("blur")
                    .expect("clap supplies the bokeh blur default"),
                lightness: *arguments
                    .get_one("lightness")
                    .expect("clap supplies the bokeh lightness default"),
                deform: *arguments
                    .get_one("deform")
                    .expect("clap supplies the bokeh deform default"),
                size: *arguments
                    .get_one("size")
                    .expect("clap supplies the bokeh size default"),
                uniform: *arguments
                    .get_one("uniform")
                    .expect("clap supplies the bokeh uniform default"),
            }),
            _ => unreachable!("clap only exposes configured subcommands"),
        };

        Self { command }
    }
}

fn bokeh_placements(arguments: &ArgMatches) -> Vec<BokehPlacement> {
    let mut placements = Vec::new();

    for value in arguments.get_many::<String>("place").into_iter().flatten() {
        for placement in parse_bokeh_places(value).expect("clap validates bokeh placements") {
            if !placements.contains(&placement) {
                placements.push(placement);
            }
        }
    }

    placements
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
        export_policy: if arguments.get_flag("alpha") {
            ExportPolicy::PreserveAlpha
        } else {
            arguments
                .get_one::<RgbColor>("background")
                .copied()
                .map(ExportPolicy::Flatten)
                .unwrap_or_default()
        },
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
            "{} - generate film-effect overlays",
            APP_NAME.bright_magenta().bold()
        ))
        .arg_required_else_help(true)
        .subcommand_required(true)
        .subcommand(scratches_command(styles.clone()))
        .subcommand(stain_command(styles.clone()))
        .subcommand(bokeh_command(styles.clone()))
        .disable_colored_help(false)
        .styles(styles)
}

fn scratches_command(styles: styling::Styles) -> ClapCommand {
    let types = ScratchType::names().join(", ");
    shared_render_arguments(
        ClapCommand::new("scratches")
            .about("Generate PNG overlays of physical film defects")
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
            .about("Generate PNG overlays of analog-film development stains")
            .styles(styles)
            .arg(
                Arg::new("blur")
                    .short('b')
                    .long("blur")
                    .value_name("PERCENT")
                    .help("Stain edge softness, 0-100.\n0 = hard edges, 100 = maximum diffusion.")
                    .default_value("50")
                    .value_parser(parse_blur),
            )
            .arg(
                Arg::new("lightness")
                    .short('l')
                    .long("lightness")
                    .value_name("PERCENT")
                    .help("Stain brightness, 0-100.\n10 matches the current/default appearance.\n0 = nearly black, 100 = near-white.")
                    .default_value("10")
                    .value_parser(parse_lightness),
            )
            .arg(
                Arg::new("contrast")
                    .short('c')
                    .long("contrast")
                    .value_name("PERCENT")
                    .help("Internal stain contrast, 0-100.\n50 preserves the default/current contrast.")
                    .default_value("50")
                    .value_parser(parse_contrast),
            ),
    )
}

fn bokeh_command(styles: styling::Styles) -> ClapCommand {
    let types = BokehType::names().join(", ");
    shared_render_arguments(
        ClapCommand::new("bokeh")
            .about("Generate monochrome analog-film optical bokeh overlays")
            .styles(styles)
            .arg(
                Arg::new("type")
                    .short('t')
                    .long("type")
                    .value_name("TYPE")
                    .help(format!(
                        "Bokeh artifact type to combine; repeat for multiple types. Available types: {}",
                        types.bright_green()
                    ))
                    .required(true)
                    .action(ArgAction::Append)
                    .value_parser(parse_bokeh_type),
            )
            .arg(
                Arg::new("place")
                    .short('p')
                    .long("place")
                    .value_name("PLACES")
                    .help("Placement bias. Use center/c, left/l, right/r, top/t, or bottom/b; comma-separate or repeat.")
                    .action(ArgAction::Append)
                    .value_parser(validate_bokeh_places),
            )
            .arg(
                Arg::new("blur")
                    .short('b')
                    .long("blur")
                    .value_name("PERCENT")
                    .help("Bokeh edge softness, 0-100.\n0 = sharpest optical edge, 100 = maximum diffusion.")
                    .default_value("100")
                    .value_parser(parse_blur),
            )
            .arg(
                Arg::new("lightness")
                    .short('l')
                    .long("lightness")
                    .value_name("PERCENT")
                    .help("Bokeh layer brightness, 0-100.\nChanges RGB brightness without changing alpha.")
                    .default_value("70")
                    .value_parser(parse_lightness),
            )
            .arg(
                Arg::new("deform")
                    .short('f')
                    .long("deform")
                    .value_name("PERCENT")
                    .help("Twinkle shape deformation, 0-100.\n0 = perfect circle, 100 = maximum organic deformation.")
                    .default_value("0")
                    .value_parser(parse_deform),
            )
            .arg(
                Arg::new("size")
                    .short('s')
                    .long("size")
                    .value_name("PERCENT")
                    .help("Maximum artifact scale from 0 to 100.")
                    .default_value("50")
                    .value_parser(parse_size),
            )
            .arg(
                Arg::new("uniform")
                    .short('u')
                    .long("uniform")
                    .value_name("PERCENT")
                    .help("Artifact size similarity from 0 to 100.")
                    .default_value("50")
                    .value_parser(parse_uniform),
            )
            .after_help(format!(
                "{}\n  {}  large diffuse optical circles\n  {}      broad film-edge exposure strips\n  {}    irregular softened border damage",
                "Supported bokeh types:".bright_yellow().bold(),
                "twinkle".bright_green(),
                "edge".bright_green(),
                "damage".bright_green(),
            )),
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
        .arg(
            Arg::new("alpha")
                .long("alpha")
                .help("Preserve RGBA transparency instead of flattening onto black")
                .action(ArgAction::SetTrue)
                .conflicts_with("background"),
        )
        .arg(
            Arg::new("background")
                .long("background")
                .value_name("RRGGBB")
                .help("Flatten transparency onto a six-digit RGB hex color instead of black")
                .value_parser(parse_background)
                .conflicts_with("alpha"),
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

pub(crate) fn parse_blur(value: &str) -> Result<u8, String> {
    let blur = value
        .parse::<u16>()
        .map_err(|_| "blur must be an integer between 0 and 100".to_owned())?;

    if blur > 100 {
        return Err("blur must be between 0 and 100".to_owned());
    }

    Ok(blur as u8)
}

pub(crate) fn parse_lightness(value: &str) -> Result<u8, String> {
    let lightness = value
        .parse::<u16>()
        .map_err(|_| "lightness must be an integer between 0 and 100".to_owned())?;

    if lightness > 100 {
        return Err("lightness must be between 0 and 100".to_owned());
    }

    Ok(lightness as u8)
}

pub(crate) fn parse_contrast(value: &str) -> Result<u8, String> {
    let contrast = value
        .parse::<u16>()
        .map_err(|_| "contrast must be an integer between 0 and 100".to_owned())?;

    if contrast > 100 {
        return Err("contrast must be between 0 and 100".to_owned());
    }

    Ok(contrast as u8)
}

pub(crate) fn parse_size(value: &str) -> Result<u8, String> {
    parse_bokeh_percentage(value, "size")
}

pub(crate) fn parse_uniform(value: &str) -> Result<u8, String> {
    parse_bokeh_percentage(value, "uniform")
}

pub(crate) fn parse_deform(value: &str) -> Result<u8, String> {
    parse_bokeh_percentage(value, "deform")
}

fn parse_bokeh_percentage(value: &str, name: &str) -> Result<u8, String> {
    let percentage = value
        .parse::<u16>()
        .map_err(|_| format!("{name} must be an integer between 0 and 100"))?;

    if percentage > 100 {
        return Err(format!("{name} must be between 0 and 100"));
    }

    Ok(percentage as u8)
}

pub(crate) fn parse_background(value: &str) -> Result<RgbColor, String> {
    let digits = value.strip_prefix('#').unwrap_or(value);
    if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("background must be exactly six hexadecimal RGB digits".to_owned());
    }

    let red = u8::from_str_radix(&digits[0..2], 16).expect("validated hexadecimal red channel");
    let green = u8::from_str_radix(&digits[2..4], 16).expect("validated hexadecimal green channel");
    let blue = u8::from_str_radix(&digits[4..6], 16).expect("validated hexadecimal blue channel");

    Ok(RgbColor::new(red, green, blue))
}

pub(crate) fn parse_scratch_type(value: &str) -> Result<ScratchType, String> {
    ScratchType::from_str(value)
}

pub(crate) fn parse_bokeh_type(value: &str) -> Result<BokehType, String> {
    BokehType::from_str(value)
}

pub(crate) fn parse_bokeh_places(value: &str) -> Result<Vec<BokehPlacement>, String> {
    BokehPlacement::parse_list(value)
}

fn validate_bokeh_places(value: &str) -> Result<String, String> {
    parse_bokeh_places(value).map(|_| value.to_owned())
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
#[path = "cli_ut.rs"]
mod cli_ut;
