use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
    str::FromStr,
};

use image::{ImageFormat, Rgba, RgbaImage};
use rand::Rng;

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
}

impl fmt::Display for ResolutionParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Error for ResolutionParseError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScratchType {
    Dust,
    Camera,
    Bend,
}

const SUPPORTED_SCRATCH_TYPES: [&str; 3] = ["dust", "camera", "bend"];

impl ScratchType {
    pub fn names() -> &'static [&'static str] {
        &SUPPORTED_SCRATCH_TYPES
    }
}

impl FromStr for ScratchType {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "dust" => Ok(Self::Dust),
            "camera" => Ok(Self::Camera),
            "bend" => Ok(Self::Bend),
            _ => Err(format!(
                "unknown scratch type '{value}'; expected one of: {}",
                Self::names().join(", ")
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScratchSettings {
    pub resolution: Resolution,
    pub density: u8,
    pub effects: Vec<ScratchType>,
    pub amount: u32,
    pub outdir: PathBuf,
}

impl ScratchSettings {
    fn validate(&self) -> Result<(), RenderError> {
        if self.density > 100 {
            return Err(RenderError::InvalidDensity(self.density));
        }
        if self.amount == 0 {
            return Err(RenderError::InvalidAmount);
        }
        if self.effects.is_empty() {
            return Err(RenderError::NoEffects);
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum RenderError {
    InvalidDensity(u8),
    InvalidAmount,
    NoEffects,
    CreateOutput {
        path: PathBuf,
        source: io::Error,
    },
    OutputNotDirectory(PathBuf),
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
            Self::InvalidAmount => write!(formatter, "amount must be at least 1"),
            Self::NoEffects => write!(formatter, "at least one scratch type must be supplied"),
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
            Self::WriteImage { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn generate_images(settings: &ScratchSettings) -> Result<(), RenderError> {
    let mut rng = rand::rng();
    generate_images_with_rng(settings, &mut rng)
}

fn generate_images_with_rng<R: Rng + ?Sized>(
    settings: &ScratchSettings,
    rng: &mut R,
) -> Result<(), RenderError> {
    settings.validate()?;
    prepare_output_directory(&settings.outdir)?;

    for number in 1..=settings.amount {
        let image = render_image(settings, rng);
        let path = settings.outdir.join(format!("scratches-{number:04}.png"));
        image
            .save_with_format(&path, ImageFormat::Png)
            .map_err(|source| RenderError::WriteImage { path, source })?;
    }

    Ok(())
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

fn render_image<R: Rng + ?Sized>(settings: &ScratchSettings, rng: &mut R) -> RgbaImage {
    let mut image = RgbaImage::new(settings.resolution.width, settings.resolution.height);

    for effect in &settings.effects {
        effect.render(&mut image, settings.density, rng);
    }

    image
}

trait ScratchEffect {
    fn render<R: Rng + ?Sized>(&self, image: &mut RgbaImage, density: u8, rng: &mut R);
}

impl ScratchEffect for ScratchType {
    fn render<R: Rng + ?Sized>(&self, image: &mut RgbaImage, density: u8, rng: &mut R) {
        match self {
            Self::Dust => DustEffect.render(image, density, rng),
            Self::Camera => CameraEffect.render(image, density, rng),
            Self::Bend => BendEffect.render(image, density, rng),
        }
    }
}

struct DustEffect;

impl ScratchEffect for DustEffect {
    fn render<R: Rng + ?Sized>(&self, image: &mut RgbaImage, density: u8, rng: &mut R) {
        if density == 0 {
            return;
        }

        let megapixels = image.width() as f32 * image.height() as f32 / 1_000_000.0;
        let particle_count = (megapixels * (density as f32 * 1.75 + 3.0)).ceil().max(1.0) as usize;
        let cluster_count = (1 + usize::from(density) / 30).min(4);
        let smallest_dimension = image.width().min(image.height()) as f32;
        let scale = (smallest_dimension / 1080.0).clamp(0.8, 3.0);
        let mut clusters = Vec::with_capacity(cluster_count);

        for _ in 0..cluster_count {
            clusters.push((
                rng.random_range(0.0..image.width() as f32),
                rng.random_range(0.0..image.height() as f32),
                rng.random_range(smallest_dimension * 0.03..smallest_dimension * 0.12),
            ));
        }

        for _ in 0..particle_count {
            let (center_x, center_y) = if rng.random_bool(0.38) {
                let (x, y, spread) = clusters[rng.random_range(0..clusters.len())];
                (
                    clustered_coordinate(x, spread, image.width(), rng),
                    clustered_coordinate(y, spread, image.height(), rng),
                )
            } else {
                (
                    rng.random_range(0.0..image.width() as f32),
                    rng.random_range(0.0..image.height() as f32),
                )
            };

            let large_speck = rng.random_bool(0.06);
            let radius = if large_speck {
                rng.random_range(2.2..5.5) * scale
            } else {
                rng.random_range(0.75..1.9) * scale
            };
            let shade = rng.random_range(175..=250);
            let mut alpha = rng.random_range(12..=(28 + density / 2));
            if large_speck {
                alpha = alpha.saturating_add(rng.random_range(8..=30));
            }

            draw_irregular_speck(image, center_x, center_y, radius, shade, alpha, rng);
        }
    }
}

struct CameraEffect;

impl ScratchEffect for CameraEffect {
    fn render<R: Rng + ?Sized>(&self, image: &mut RgbaImage, density: u8, rng: &mut R) {
        if density == 0 {
            return;
        }

        let megapixel_scale = (image.width() as f32 * image.height() as f32 / 1_000_000.0)
            .sqrt()
            .max(0.35);
        let scratch_count = (density as f32 * 0.045 * megapixel_scale).ceil().max(1.0) as usize;

        for _ in 0..scratch_count {
            draw_camera_scratch(image, density, rng);
        }
    }
}

struct BendEffect;

struct BendPath {
    start: (f32, f32),
    control: (f32, f32),
    end: (f32, f32),
    normal: (f32, f32),
    wobble: f32,
    cycles: f32,
    phase: f32,
}

impl BendPath {
    fn point(&self, progress: f32) -> (f32, f32) {
        let inverse = 1.0 - progress;
        let x = inverse * inverse * self.start.0
            + 2.0 * inverse * progress * self.control.0
            + progress * progress * self.end.0;
        let y = inverse * inverse * self.start.1
            + 2.0 * inverse * progress * self.control.1
            + progress * progress * self.end.1;
        let offset =
            (progress * self.cycles * std::f32::consts::TAU + self.phase).sin() * self.wobble;

        (x + self.normal.0 * offset, y + self.normal.1 * offset)
    }
}

impl ScratchEffect for BendEffect {
    fn render<R: Rng + ?Sized>(&self, image: &mut RgbaImage, density: u8, rng: &mut R) {
        if density == 0 {
            return;
        }

        let megapixel_scale = (image.width() as f32 * image.height() as f32 / 1_000_000.0)
            .sqrt()
            .clamp(0.5, 2.5);
        let bend_count = (0.2 + density as f32 / 85.0 * megapixel_scale)
            .ceil()
            .clamp(1.0, 4.0) as usize;

        for _ in 0..bend_count {
            draw_bend(image, density, rng);
        }
    }
}

fn clustered_coordinate<R: Rng + ?Sized>(center: f32, spread: f32, limit: u32, rng: &mut R) -> f32 {
    let offset = (rng.random_range(-spread..=spread) + rng.random_range(-spread..=spread)) * 0.5;
    (center + offset).clamp(0.0, limit.saturating_sub(1) as f32)
}

fn draw_irregular_speck<R: Rng + ?Sized>(
    image: &mut RgbaImage,
    center_x: f32,
    center_y: f32,
    radius: f32,
    shade: u8,
    alpha: u8,
    rng: &mut R,
) {
    let vertex_count = rng.random_range(5..=8);
    let rotation = rng.random_range(0.0..std::f32::consts::TAU);
    let stretch_x = rng.random_range(0.75..1.35);
    let stretch_y = rng.random_range(0.75..1.35);
    let mut vertices = Vec::with_capacity(vertex_count);

    for vertex in 0..vertex_count {
        let angle = rotation
            + std::f32::consts::TAU * vertex as f32 / vertex_count as f32
            + rng.random_range(-0.22..0.22);
        let edge_radius = radius * rng.random_range(0.65..1.3);
        vertices.push((
            center_x + angle.cos() * edge_radius * stretch_x,
            center_y + angle.sin() * edge_radius * stretch_y,
        ));
    }

    let min_x = vertices
        .iter()
        .map(|(x, _)| *x)
        .fold(f32::INFINITY, f32::min);
    let max_x = vertices
        .iter()
        .map(|(x, _)| *x)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = vertices
        .iter()
        .map(|(_, y)| *y)
        .fold(f32::INFINITY, f32::min);
    let max_y = vertices
        .iter()
        .map(|(_, y)| *y)
        .fold(f32::NEG_INFINITY, f32::max);
    let start_x = min_x.floor().max(0.0) as u32;
    let end_x = max_x.ceil().min(image.width().saturating_sub(1) as f32) as u32;
    let start_y = min_y.floor().max(0.0) as u32;
    let end_y = max_y.ceil().min(image.height().saturating_sub(1) as f32) as u32;
    let mut drew_pixel = false;

    for y in start_y..=end_y {
        for x in start_x..=end_x {
            if point_in_polygon(x as f32 + 0.5, y as f32 + 0.5, &vertices) && !rng.random_bool(0.08)
            {
                let pixel_alpha = if rng.random_bool(0.18) {
                    alpha / 2
                } else {
                    alpha
                };
                blend_pixel(image, x, y, shade, pixel_alpha);
                drew_pixel = true;
            }
        }
    }

    if !drew_pixel {
        blend_pixel(
            image,
            center_x.round().clamp(0.0, image.width() as f32 - 1.0) as u32,
            center_y.round().clamp(0.0, image.height() as f32 - 1.0) as u32,
            shade,
            alpha,
        );
    }
}

fn point_in_polygon(x: f32, y: f32, vertices: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut previous = vertices.last().expect("specks always have vertices");

    for current in vertices {
        if (current.1 > y) != (previous.1 > y)
            && x < (previous.0 - current.0) * (y - current.1) / (previous.1 - current.1) + current.0
        {
            inside = !inside;
        }
        previous = current;
    }

    inside
}

fn draw_camera_scratch<R: Rng + ?Sized>(image: &mut RgbaImage, density: u8, rng: &mut R) {
    let height = image.height();
    let width = image.width();
    let scale = (width.min(height) as f32 / 1080.0).clamp(0.6, 3.0);
    let mostly_full_height = rng.random_bool(0.62);
    let start_limit = if mostly_full_height {
        (height / 8).max(1)
    } else {
        (height * 2 / 3).max(1)
    };
    let start_y = rng.random_range(0..start_limit);
    let remaining_height = height - start_y;
    let minimum_length = ((remaining_height as f32 * if mostly_full_height { 0.58 } else { 0.2 })
        .ceil() as u32)
        .max(1);
    let length = rng.random_range(minimum_length..=remaining_height);
    let end_y = start_y + length;
    let strong_scratch = rng.random_bool(0.12);
    let base_half_width = if strong_scratch {
        rng.random_range(0.75..1.8) * scale
    } else {
        rng.random_range(0.28..0.85) * scale
    };
    let base_alpha = if strong_scratch {
        rng.random_range(55..=(90 + density / 2))
    } else {
        rng.random_range(16..=(35 + density / 2))
    };
    let shade = if strong_scratch {
        rng.random_range(220..=255)
    } else {
        rng.random_range(165..=235)
    };
    let segment_height = (rng.random_range(24.0..80.0) * scale).round().max(8.0) as u32;
    let mut segment_start = start_y;
    let mut start_x = rng.random_range(0.0..width as f32);

    while segment_start < end_y {
        let segment_end = (segment_start + segment_height).min(end_y);
        let end_x = (start_x + rng.random_range(-1.8..1.8) * scale)
            .clamp(-base_half_width, width as f32 - 1.0 + base_half_width);
        let bow = rng.random_range(-1.6..1.6) * scale;
        let visible = segment_start == start_y || rng.random_bool(0.8);
        let segment_alpha = ((base_alpha as f32) * rng.random_range(0.55..1.15))
            .round()
            .clamp(1.0, 255.0) as u8;
        let segment_width = base_half_width * rng.random_range(0.72..1.25);

        if visible {
            let segment_length = (segment_end - segment_start) as f32;
            for y in segment_start..segment_end {
                let progress = (y - segment_start) as f32 / segment_length;
                let x = start_x
                    + (end_x - start_x) * progress
                    + (progress * std::f32::consts::PI).sin() * bow;
                draw_camera_row(image, y, x, segment_width, shade, segment_alpha);
            }
        }

        start_x = end_x;
        segment_start = segment_end;
    }
}

fn draw_camera_row(
    image: &mut RgbaImage,
    y: u32,
    center_x: f32,
    half_width: f32,
    shade: u8,
    alpha: u8,
) {
    let start_x = (center_x - half_width - 0.5).floor().max(0.0) as u32;
    let end_x = (center_x + half_width + 0.5)
        .ceil()
        .min(image.width().saturating_sub(1) as f32) as u32;

    for x in start_x..=end_x {
        let distance = (x as f32 + 0.5 - center_x).abs();
        let coverage = (half_width + 0.5 - distance).clamp(0.0, 1.0);
        let pixel_alpha = (alpha as f32 * coverage).round() as u8;

        if pixel_alpha > 0 {
            blend_pixel(image, x, y, shade, pixel_alpha);
        }
    }
}

fn draw_bend<R: Rng + ?Sized>(image: &mut RgbaImage, density: u8, rng: &mut R) {
    let width = image.width() as f32;
    let height = image.height() as f32;
    let smallest_dimension = width.min(height);
    let scale = (smallest_dimension / 1080.0).clamp(0.6, 3.0);
    let angle = rng.random_range(0.0..std::f32::consts::PI);
    let tangent = (angle.cos(), angle.sin());
    let normal = (-tangent.1, tangent.0);
    let center = (
        rng.random_range(width * 0.15..width * 0.85),
        rng.random_range(height * 0.15..height * 0.85),
    );
    let length = rng.random_range(smallest_dimension * 0.35..smallest_dimension * 1.05);
    let curvature = rng.random_range(-0.3..0.3) * length;
    let start = (
        center.0 - tangent.0 * length * 0.5,
        center.1 - tangent.1 * length * 0.5,
    );
    let end = (
        center.0 + tangent.0 * length * 0.5,
        center.1 + tangent.1 * length * 0.5,
    );
    let control = (
        center.0 + normal.0 * curvature + tangent.0 * rng.random_range(-0.12..0.12) * length,
        center.1 + normal.1 * curvature + tangent.1 * rng.random_range(-0.12..0.12) * length,
    );
    let halo_radius = rng.random_range(12.0..32.0) * scale;
    let core_radius = rng.random_range(1.1..3.4) * scale;
    let halo_alpha = rng.random_range(3..=(8 + density / 6));
    let core_alpha = rng.random_range(8..=(20 + density / 2));
    let halo_shade = rng.random_range(145..=210);
    let core_shade = rng.random_range(180..=245);
    let phase = rng.random_range(0.0..std::f32::consts::TAU);
    let wobble = rng.random_range(0.5..3.0) * scale;
    let cycles = rng.random_range(1.0..3.0);
    let path = BendPath {
        start,
        control,
        end,
        normal,
        wobble,
        cycles,
        phase,
    };

    let halo_steps = (length / (halo_radius * 0.85)).ceil().max(2.0) as usize;
    for step in 0..=halo_steps {
        let progress = step as f32 / halo_steps as f32;
        let point = path.point(progress);
        draw_soft_stamp(image, point.0, point.1, halo_radius, halo_shade, halo_alpha);
    }

    let core_steps = (length / ((core_radius + 1.0) * 0.75)).ceil().max(2.0) as usize;
    for step in 0..=core_steps {
        let progress = step as f32 / core_steps as f32;
        let point = path.point(progress);
        draw_soft_stamp(
            image,
            point.0,
            point.1,
            core_radius + 1.0,
            core_shade,
            core_alpha,
        );
    }
}

fn draw_soft_stamp(
    image: &mut RgbaImage,
    center_x: f32,
    center_y: f32,
    radius: f32,
    shade: u8,
    alpha: u8,
) {
    let start_x = (center_x - radius).floor().max(0.0) as u32;
    let end_x = (center_x + radius)
        .ceil()
        .min(image.width().saturating_sub(1) as f32) as u32;
    let start_y = (center_y - radius).floor().max(0.0) as u32;
    let end_y = (center_y + radius)
        .ceil()
        .min(image.height().saturating_sub(1) as f32) as u32;
    let radius_squared = radius * radius;

    for y in start_y..=end_y {
        for x in start_x..=end_x {
            let dx = x as f32 + 0.5 - center_x;
            let dy = y as f32 + 0.5 - center_y;
            let distance_squared = dx * dx + dy * dy;

            if distance_squared >= radius_squared {
                continue;
            }

            let falloff = 1.0 - distance_squared.sqrt() / radius;
            let pixel_alpha = (alpha as f32 * falloff * falloff).round() as u8;
            if pixel_alpha > 0 {
                blend_pixel(image, x, y, shade, pixel_alpha);
            }
        }
    }
}

fn blend_pixel(image: &mut RgbaImage, x: u32, y: u32, shade: u8, alpha: u8) {
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
mod tests {
    use rand::{SeedableRng, rngs::StdRng};

    use super::{Resolution, ScratchSettings, ScratchType, render_image};

    fn settings(effect: ScratchType) -> ScratchSettings {
        ScratchSettings {
            resolution: Resolution::new(320, 200).expect("test resolution should be valid"),
            density: 35,
            effects: vec![effect],
            amount: 1,
            outdir: "unused".into(),
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
        settings.density = 0;
        let mut rng = StdRng::seed_from_u64(8);
        let image = render_image(&settings, &mut rng);

        assert!(image.pixels().all(|pixel| pixel[3] == 0));
    }
}
