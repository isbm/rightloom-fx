use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
    str::FromStr,
};

use image::{ImageFormat, Rgba, RgbaImage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolution {
    width: u32,
    height: u32,
}

impl Resolution {
    pub fn new(width: u32, height: u32) -> Result<Self, ResolutionParseError> {
        if width == 0 || height == 0 {
            return Err(ResolutionParseError(
                "resolution width and height must both be greater than zero".to_owned(),
            ));
        }

        Ok(Self { width, height })
    }

    pub fn width(self) -> u32 {
        self.width
    }

    pub fn height(self) -> u32 {
        self.height
    }

    pub fn from_aspect_ratio(value: &str) -> Result<Self, ResolutionParseError> {
        let value = value.trim();
        let mut parts = value.split('x');
        let Some(ratio) = parts.next() else {
            return Err(ResolutionParseError::malformed_aspect_ratio(value));
        };
        let Some(long_side) = parts.next() else {
            return Err(ResolutionParseError::malformed_aspect_ratio(value));
        };

        if ratio.is_empty() || long_side.is_empty() || parts.next().is_some() {
            return Err(ResolutionParseError::malformed_aspect_ratio(value));
        }

        let mut ratio_parts = ratio.split(':');
        let Some(width_ratio) = ratio_parts.next() else {
            return Err(ResolutionParseError::malformed_aspect_ratio(value));
        };
        let Some(height_ratio) = ratio_parts.next() else {
            return Err(ResolutionParseError::malformed_aspect_ratio(value));
        };

        if width_ratio.is_empty() || height_ratio.is_empty() || ratio_parts.next().is_some() {
            return Err(ResolutionParseError::malformed_aspect_ratio(value));
        }

        let width_ratio = parse_positive_aspect_component(value, width_ratio, "width ratio")?;
        let height_ratio = parse_positive_aspect_component(value, height_ratio, "height ratio")?;
        let long_side = parse_positive_aspect_component(value, long_side, "long side")?;
        let long_side = u32::try_from(long_side).map_err(|_| {
            ResolutionParseError(format!(
                "invalid aspect ratio '{value}': long side exceeds the maximum output dimension"
            ))
        })?;

        let longest_ratio = u128::from(width_ratio.max(height_ratio));
        let shortest_ratio = u128::from(width_ratio.min(height_ratio));
        let shorter_side =
            (u128::from(long_side) * shortest_ratio + longest_ratio / 2) / longest_ratio;

        if shorter_side == 0 {
            return Err(ResolutionParseError(format!(
                "invalid aspect ratio '{value}': computed output dimension is zero"
            )));
        }

        let shorter_side = u32::try_from(shorter_side).map_err(|_| {
            ResolutionParseError(format!(
                "invalid aspect ratio '{value}': computed output dimension is too large"
            ))
        })?;
        let (width, height) = if width_ratio >= height_ratio {
            (long_side, shorter_side)
        } else {
            (shorter_side, long_side)
        };

        Self::new(width, height)
    }
}

fn parse_positive_aspect_component(
    input: &str,
    value: &str,
    component: &str,
) -> Result<u64, ResolutionParseError> {
    let parsed = value.parse::<u64>().map_err(|_| {
        ResolutionParseError(format!(
            "invalid aspect ratio '{input}': {component} '{value}' must be a positive integer"
        ))
    })?;

    if parsed == 0 {
        return Err(ResolutionParseError(format!(
            "invalid aspect ratio '{input}': {component} must be greater than zero"
        )));
    }

    Ok(parsed)
}

impl FromStr for Resolution {
    type Err = ResolutionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        let mut parts = value.split('x');
        let Some(width) = parts.next() else {
            return Err(ResolutionParseError::malformed());
        };
        let Some(height) = parts.next() else {
            return Err(ResolutionParseError::malformed());
        };

        if width.is_empty() || height.is_empty() || parts.next().is_some() {
            return Err(ResolutionParseError::malformed());
        }

        let width = width.parse::<u32>().map_err(|_| {
            ResolutionParseError(format!(
                "invalid resolution width '{width}': expected a positive integer"
            ))
        })?;
        let height = height.parse::<u32>().map_err(|_| {
            ResolutionParseError(format!(
                "invalid resolution height '{height}': expected a positive integer"
            ))
        })?;

        Self::new(width, height)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionParseError(String);

impl ResolutionParseError {
    fn malformed() -> Self {
        Self("resolution must use WIDTHxHEIGHT, for example 3840x2160".to_owned())
    }

    fn malformed_aspect_ratio(value: &str) -> Self {
        Self(format!(
            "invalid aspect ratio '{value}': expected WIDTH_RATIO:HEIGHT_RATIOxLONG_SIDE, for example 3:2x6240"
        ))
    }
}

impl fmt::Display for ResolutionParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Error for ResolutionParseError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RgbColor {
    r: u8,
    g: u8,
    b: u8,
}

impl RgbColor {
    pub const BLACK: Self = Self::new(0, 0, 0);

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportPolicy {
    PreserveAlpha,
    Flatten(RgbColor),
}

impl Default for ExportPolicy {
    fn default() -> Self {
        Self::Flatten(RgbColor::BLACK)
    }
}

impl ExportPolicy {
    fn apply_to(self, image: &mut RgbaImage) {
        let Self::Flatten(background) = self else {
            return;
        };

        for pixel in image.pixels_mut() {
            let alpha = u32::from(pixel[3]);
            let inverse_alpha = 255 - alpha;
            for (channel, background_channel) in
                [(0, background.r), (1, background.g), (2, background.b)]
            {
                pixel[channel] = ((u32::from(pixel[channel]) * alpha
                    + u32::from(background_channel) * inverse_alpha
                    + 127)
                    / 255) as u8;
            }
            pixel[3] = 255;
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderSettings {
    pub resolution: Resolution,
    pub density: u8,
    pub amount: u32,
    pub outdir: PathBuf,
    pub export_policy: ExportPolicy,
}

impl RenderSettings {
    fn validate(&self) -> Result<(), RenderError> {
        if self.density > 100 {
            return Err(RenderError::InvalidDensity(self.density));
        }
        if self.amount == 0 {
            return Err(RenderError::InvalidAmount);
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum RenderError {
    InvalidDensity(u8),
    InvalidBlur(u8),
    InvalidLightness(u8),
    InvalidContrast(u8),
    InvalidSize(u8),
    InvalidUniform(u8),
    InvalidAmount,
    NoEffects,
    NoBokehTypes,
    InvalidBokehEdgePlacement,
    InvalidBokehDamagePlacement,
    CreateOutput {
        path: PathBuf,
        source: io::Error,
    },
    OutputNotDirectory(PathBuf),
    ReadOutput {
        path: PathBuf,
        source: io::Error,
    },
    CreateOutputFile {
        path: PathBuf,
        source: io::Error,
    },
    SequenceExhausted {
        prefix: String,
    },
    WriteImage {
        path: PathBuf,
        source: image::ImageError,
    },
}

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDensity(density) => {
                write!(
                    formatter,
                    "density must be between 0 and 100, got {density}"
                )
            }
            Self::InvalidBlur(blur) => {
                write!(formatter, "blur must be between 0 and 100, got {blur}")
            }
            Self::InvalidLightness(lightness) => {
                write!(
                    formatter,
                    "lightness must be between 0 and 100, got {lightness}"
                )
            }
            Self::InvalidContrast(contrast) => {
                write!(
                    formatter,
                    "contrast must be between 0 and 100, got {contrast}"
                )
            }
            Self::InvalidSize(size) => {
                write!(formatter, "size must be between 0 and 100, got {size}")
            }
            Self::InvalidUniform(uniform) => {
                write!(
                    formatter,
                    "uniform must be between 0 and 100, got {uniform}"
                )
            }
            Self::InvalidAmount => write!(formatter, "amount must be at least 1"),
            Self::NoEffects => write!(formatter, "at least one scratch type must be supplied"),
            Self::NoBokehTypes => write!(formatter, "at least one bokeh type must be supplied"),
            Self::InvalidBokehEdgePlacement => {
                write!(
                    formatter,
                    "center placement is not available for edge bokeh"
                )
            }
            Self::InvalidBokehDamagePlacement => {
                write!(
                    formatter,
                    "center placement is not available for damage bokeh"
                )
            }
            Self::CreateOutput { path, source } => {
                write!(
                    formatter,
                    "failed to create output directory '{}': {source}",
                    path.display()
                )
            }
            Self::OutputNotDirectory(path) => {
                write!(
                    formatter,
                    "output path '{}' is not a directory",
                    path.display()
                )
            }
            Self::ReadOutput { path, source } => {
                write!(
                    formatter,
                    "failed to read output directory '{}': {source}",
                    path.display()
                )
            }
            Self::CreateOutputFile { path, source } => {
                write!(
                    formatter,
                    "failed to create output file '{}': {source}",
                    path.display()
                )
            }
            Self::SequenceExhausted { prefix } => {
                write!(formatter, "output sequence for '{prefix}' is exhausted")
            }
            Self::WriteImage { path, source } => {
                write!(formatter, "failed to write '{}': {source}", path.display())
            }
        }
    }
}

impl Error for RenderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CreateOutput { source, .. } => Some(source),
            Self::ReadOutput { source, .. } => Some(source),
            Self::CreateOutputFile { source, .. } => Some(source),
            Self::WriteImage { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub(crate) fn write_images<F>(
    settings: &RenderSettings,
    prefix: &str,
    render: F,
) -> Result<(), RenderError>
where
    F: FnMut() -> RgbaImage,
{
    write_images_with_progress(settings, prefix, render, |message| println!("{message}"))
}

fn write_images_with_progress<F, P>(
    settings: &RenderSettings,
    prefix: &str,
    mut render: F,
    mut progress: P,
) -> Result<(), RenderError>
where
    F: FnMut() -> RgbaImage,
    P: FnMut(&str),
{
    settings.validate()?;
    prepare_output_directory(&settings.outdir)?;

    let next_sequence = next_sequence_start(&settings.outdir, prefix)?;
    let (first_sequence, first_path, first_file) =
        reserve_next_output_file(&settings.outdir, prefix, next_sequence)?;
    let first_filename = output_filename(prefix, first_sequence);
    progress(&format!(
        "starting {prefix}: {} image(s), {}x{}, output {}, sequence {first_sequence:04}",
        settings.amount,
        settings.resolution.width(),
        settings.resolution.height(),
        settings.outdir.display(),
    ));

    let mut reservation = Some((first_sequence, first_path, first_file));
    let mut last_filename = first_filename.clone();

    for index in 0..settings.amount {
        let (sequence, path, file) = reservation
            .take()
            .expect("each image generation has a reserved output file");
        let filename = output_filename(prefix, sequence);
        progress(&format!("generating {filename} ..."));

        let mut image = render();
        settings.export_policy.apply_to(&mut image);
        write_reserved_image(&path, file, &image)?;

        progress(&format!("done {filename}"));
        last_filename = filename;

        if index + 1 < settings.amount {
            let next_sequence = advance_sequence(sequence, prefix)?;
            reservation = Some(reserve_next_output_file(
                &settings.outdir,
                prefix,
                next_sequence,
            )?);
        }
    }

    progress(&format!(
        "completed {prefix}: {} image(s) in {}; files {first_filename} through {last_filename}",
        settings.amount,
        settings.outdir.display(),
    ));

    Ok(())
}

fn next_sequence_start(directory: &Path, prefix: &str) -> Result<u64, RenderError> {
    let entries = fs::read_dir(directory).map_err(|source| RenderError::ReadOutput {
        path: directory.to_path_buf(),
        source,
    })?;
    let mut highest = 0;

    for entry in entries {
        let entry = entry.map_err(|source| RenderError::ReadOutput {
            path: directory.to_path_buf(),
            source,
        })?;
        if !entry.path().is_file() {
            continue;
        }
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Some(sequence) = parse_sequence_number(file_name, prefix) else {
            continue;
        };

        highest = highest.max(sequence);
    }

    advance_sequence(highest, prefix)
}

fn parse_sequence_number(file_name: &str, prefix: &str) -> Option<u64> {
    let digits = file_name
        .strip_prefix(prefix)?
        .strip_prefix('-')?
        .strip_suffix(".png")?;

    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    digits.parse().ok()
}

fn reserve_next_output_file(
    directory: &Path,
    prefix: &str,
    starting_sequence: u64,
) -> Result<(u64, PathBuf, fs::File), RenderError> {
    let mut sequence = starting_sequence;

    loop {
        let path = directory.join(output_filename(prefix, sequence));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((sequence, path, file)),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
                sequence = advance_sequence(sequence, prefix)?;
            }
            Err(source) => return Err(RenderError::CreateOutputFile { path, source }),
        }
    }
}

fn advance_sequence(sequence: u64, prefix: &str) -> Result<u64, RenderError> {
    sequence
        .checked_add(1)
        .ok_or_else(|| RenderError::SequenceExhausted {
            prefix: prefix.to_owned(),
        })
}

fn output_filename(prefix: &str, sequence: u64) -> String {
    format!("{prefix}-{sequence:04}.png")
}

fn write_reserved_image(
    path: &Path,
    mut file: fs::File,
    image: &RgbaImage,
) -> Result<(), RenderError> {
    let result = image.write_to(&mut file, ImageFormat::Png);
    drop(file);

    result.map_err(|source| RenderError::WriteImage {
        path: path.to_path_buf(),
        source,
    })
}

fn prepare_output_directory(path: &Path) -> Result<(), RenderError> {
    if path.exists() && !path.is_dir() {
        return Err(RenderError::OutputNotDirectory(path.to_path_buf()));
    }

    fs::create_dir_all(path).map_err(|source| RenderError::CreateOutput {
        path: path.to_path_buf(),
        source,
    })?;

    if !path.is_dir() {
        return Err(RenderError::OutputNotDirectory(path.to_path_buf()));
    }

    Ok(())
}

pub(crate) fn blend_gray_pixel(image: &mut RgbaImage, x: u32, y: u32, shade: u8, alpha: u8) {
    let destination = image.get_pixel_mut(x, y);
    let source_alpha = u32::from(alpha);
    let destination_alpha = u32::from(destination[3]);
    let output_alpha = source_alpha + (destination_alpha * (255 - source_alpha) + 127) / 255;

    if output_alpha == 0 {
        return;
    }

    let mut output = [0; 4];
    for channel in 0..3 {
        let source_premultiplied = u32::from(shade) * source_alpha;
        let destination_premultiplied = u32::from(destination[channel]) * destination_alpha;
        let output_premultiplied =
            source_premultiplied + (destination_premultiplied * (255 - source_alpha) + 127) / 255;
        output[channel] = ((output_premultiplied + output_alpha / 2) / output_alpha) as u8;
    }
    output[3] = output_alpha as u8;
    *destination = Rgba(output);
}

#[cfg(test)]
#[path = "render_ut.rs"]
mod render_ut;
