use std::str::FromStr;

use image::{Rgba, RgbaImage};
use rand::Rng;

use crate::render::{RenderError, RenderSettings, write_images};

const TAIL_THRESHOLD: f32 = 1e-4;
const EDGE_FALLOFF: f32 = 2.7;
const TWINKLE_COUNT_ANCHORS: [(u8, usize); 7] = [
    (0, 0),
    (1, 1),
    (10, 2),
    (25, 4),
    (50, 6),
    (75, 9),
    (100, 14),
];
const SUPPORTED_BOKEH_TYPES: [&str; 3] = ["twinkle", "edge", "damage"];
const SUPPORTED_BOKEH_PLACEMENTS: [&str; 5] = ["center", "left", "right", "top", "bottom"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BokehType {
    Twinkle,
    Edge,
    Damage,
}

impl BokehType {
    pub fn names() -> &'static [&'static str] {
        &SUPPORTED_BOKEH_TYPES
    }
}

impl FromStr for BokehType {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "twinkle" => Ok(Self::Twinkle),
            "edge" => Ok(Self::Edge),
            "damage" => Ok(Self::Damage),
            _ => Err(format!(
                "unknown bokeh type '{value}'; expected one of: {}",
                Self::names().join(", ")
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BokehPlacement {
    Center,
    Left,
    Right,
    Top,
    Bottom,
}

impl BokehPlacement {
    pub fn names() -> &'static [&'static str] {
        &SUPPORTED_BOKEH_PLACEMENTS
    }

    pub fn parse_list(value: &str) -> Result<Vec<Self>, String> {
        let mut placements = Vec::new();

        for item in value.split(',') {
            let item = item.trim();
            if item.is_empty() {
                return Err("bokeh placement list cannot contain an empty value".to_owned());
            }
            let placement = Self::from_str(item)?;
            if !placements.contains(&placement) {
                placements.push(placement);
            }
        }

        Ok(placements)
    }
}

impl FromStr for BokehPlacement {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "center" | "c" => Ok(Self::Center),
            "left" | "l" => Ok(Self::Left),
            "right" | "r" => Ok(Self::Right),
            "top" | "t" => Ok(Self::Top),
            "bottom" | "b" => Ok(Self::Bottom),
            _ => Err(format!(
                "unknown bokeh placement '{value}'; expected center/c, left/l, right/r, top/t, or bottom/b"
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BokehSettings {
    pub render: RenderSettings,
    pub types: Vec<BokehType>,
    pub placements: Vec<BokehPlacement>,
    pub blur: u8,
    pub size: u8,
    pub uniform: u8,
}

impl BokehSettings {
    fn validate(&self) -> Result<(), RenderError> {
        if self.types.is_empty() {
            return Err(RenderError::NoBokehTypes);
        }
        if self.types.contains(&BokehType::Edge)
            && self.placements.contains(&BokehPlacement::Center)
        {
            return Err(RenderError::InvalidBokehEdgePlacement);
        }
        if self.blur > 100 {
            return Err(RenderError::InvalidBlur(self.blur));
        }
        if self.size > 100 {
            return Err(RenderError::InvalidSize(self.size));
        }
        if self.uniform > 100 {
            return Err(RenderError::InvalidUniform(self.uniform));
        }

        Ok(())
    }
}

pub fn generate_images(settings: &BokehSettings) -> Result<(), RenderError> {
    let mut rng = rand::rng();
    generate_images_with_rng(settings, &mut rng)
}

fn generate_images_with_rng<R: Rng + ?Sized>(
    settings: &BokehSettings,
    rng: &mut R,
) -> Result<(), RenderError> {
    settings.validate()?;
    write_images(&settings.render, "bokeh", || render_image(settings, rng))
}

fn render_image<R: Rng + ?Sized>(settings: &BokehSettings, rng: &mut R) -> RgbaImage {
    let width = settings.render.resolution.width();
    let height = settings.render.resolution.height();
    let blur = BlurParameters::new(settings.blur, width, height);
    let mut effects = vec![0.0; width as usize * height as usize];

    if settings.render.density == 0 {
        return scalar_effects_to_image(&effects, width, height);
    }

    for bokeh_type in &settings.types {
        match bokeh_type {
            BokehType::Twinkle => {
                for twinkle in generate_twinkles(settings, width, height, rng) {
                    twinkle.rasterize(&mut effects, width, height, blur);
                }
            }
            BokehType::Edge => {
                for exposure in generate_edge_exposures(settings, width, height, rng) {
                    exposure.rasterize(&mut effects, width, height, blur);
                }
            }
            BokehType::Damage => {
                for segment in generate_damage_segments(settings, width, height, rng) {
                    segment.rasterize(&mut effects, width, height, blur);
                }
            }
        }
    }

    scalar_effects_to_image(&effects, width, height)
}

fn accumulate_scalar_effect(accumulated: f32, contribution: f32) -> f32 {
    1.0 - (1.0 - accumulated) * (1.0 - contribution.clamp(0.0, 1.0))
}

fn accumulate_pixel(effects: &mut [f32], width: u32, x: u32, y: u32, contribution: f32) {
    let index = y as usize * width as usize + x as usize;
    effects[index] = accumulate_scalar_effect(effects[index], contribution);
}

fn scalar_effects_to_image(effects: &[f32], width: u32, height: u32) -> RgbaImage {
    debug_assert_eq!(effects.len(), width as usize * height as usize);
    let mut image = RgbaImage::new(width, height);

    for (effect, pixel) in effects.iter().zip(image.pixels_mut()) {
        *pixel = Rgba([
            255,
            255,
            255,
            (effect.clamp(0.0, 1.0) * 255.0).round() as u8,
        ]);
    }

    image
}

#[derive(Debug, Clone, Copy)]
struct BlurParameters {
    fraction: f32,
    edge_softness: f32,
}

impl BlurParameters {
    fn new(blur: u8, width: u32, height: u32) -> Self {
        let fraction = f32::from(blur) / 100.0;
        let longest_side = width.max(height) as f32;
        let minimum_edge_softness = (0.002 * longest_side).max(1.0);

        Self {
            fraction,
            edge_softness: lerp(minimum_edge_softness, 0.08 * longest_side, fraction),
        }
    }

    fn twinkle_softness(self, radius: f32) -> f32 {
        lerp(
            (radius * 0.018).max(1.0),
            (radius * 0.30).max(1.0),
            self.fraction,
        )
    }
}

fn lerp(start: f32, end: f32, progress: f32) -> f32 {
    start + (end - start) * progress
}

fn twinkle_count(density: u8) -> usize {
    interpolated_count(density, &TWINKLE_COUNT_ANCHORS)
}

fn damage_count(density: u8) -> usize {
    twinkle_count(density) * 3
}

fn edge_count(density: u8) -> usize {
    match density {
        0 => 0,
        1..=35 => 1,
        36..=65 => 2,
        66..=90 => 3,
        _ => 4,
    }
}

fn interpolated_count(density: u8, anchors: &[(u8, usize)]) -> usize {
    let (start, end) = anchors
        .windows(2)
        .find(|segment| density <= segment[1].0)
        .map(|segment| (segment[0], segment[1]))
        .unwrap_or((anchors[anchors.len() - 2], anchors[anchors.len() - 1]));
    let progress = (f32::from(density) - f32::from(start.0)) / f32::from(end.0 - start.0);

    (start.1 as f32 + (end.1 as f32 - start.1 as f32) * progress).round() as usize
}

fn maximum_scale(size: u8) -> f32 {
    (f32::from(size) / 100.0).max(0.02)
}

fn minimum_size_fraction(uniform: u8) -> f32 {
    let uniformity = f32::from(uniform) / 100.0;
    0.02 + 0.96 * uniformity * uniformity
}

fn sample_object_scale<R: Rng + ?Sized>(size: u8, uniform: u8, rng: &mut R) -> f32 {
    sample_object_scale_at(size, uniform, rng.random_range(0.0..1.0), rng)
}

fn sample_object_scale_at<R: Rng + ?Sized>(
    size: u8,
    uniform: u8,
    quantile: f32,
    rng: &mut R,
) -> f32 {
    let maximum = maximum_scale(size);
    if uniform == 100 {
        return maximum * rng.random_range(0.97..1.03);
    }

    let minimum = maximum * minimum_size_fraction(uniform);
    minimum * (maximum / minimum).powf(quantile.clamp(0.0, 1.0))
}

fn stratified_quantile<R: Rng + ?Sized>(index: usize, count: usize, rng: &mut R) -> f32 {
    (index as f32 + rng.random_range(0.0..1.0)) / count.max(1) as f32
}

fn selected_placement<R: Rng + ?Sized>(
    placements: &[BokehPlacement],
    rng: &mut R,
) -> Option<BokehPlacement> {
    (!placements.is_empty()).then(|| placements[rng.random_range(0..placements.len())])
}

fn placement_position<R: Rng + ?Sized>(
    placement: Option<BokehPlacement>,
    width: u32,
    height: u32,
    rng: &mut R,
) -> (f32, f32) {
    let width = width as f32;
    let height = height as f32;
    let Some(placement) = placement else {
        return (
            rng.random_range(-0.12 * width..1.12 * width),
            rng.random_range(-0.12 * height..1.12 * height),
        );
    };

    let (anchor_x, anchor_y, spread_x, spread_y) = match placement {
        BokehPlacement::Center => (0.50, 0.50, 0.34, 0.34),
        BokehPlacement::Left => (0.08, 0.50, 0.30, 0.42),
        BokehPlacement::Right => (0.92, 0.50, 0.30, 0.42),
        BokehPlacement::Top => (0.50, 0.08, 0.42, 0.30),
        BokehPlacement::Bottom => (0.50, 0.92, 0.42, 0.30),
    };

    (
        (anchor_x + bell_curve_offset(spread_x, rng)) * width,
        (anchor_y + bell_curve_offset(spread_y, rng)) * height,
    )
}

fn bell_curve_offset<R: Rng + ?Sized>(spread: f32, rng: &mut R) -> f32 {
    (rng.random_range(-spread..spread) + rng.random_range(-spread..spread)) * 0.5
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Twinkle {
    center_x: f32,
    center_y: f32,
    radius_x: f32,
    radius_y: f32,
    sin_angle: f32,
    cos_angle: f32,
    intensity: f32,
    deformation: f32,
    deformation_frequency: f32,
    deformation_phase: f32,
    glow_phase: f32,
}

impl Twinkle {
    fn rasterize(&self, effects: &mut [f32], width: u32, height: u32, blur: BlurParameters) {
        let nominal_radius = self.radius_x.min(self.radius_y);
        let softness = blur.twinkle_softness(nominal_radius);
        let tail = (1.0 + softness / nominal_radius) * (1.0 + self.deformation);
        let extent_x =
            (self.cos_angle.abs() * self.radius_x + self.sin_angle.abs() * self.radius_y) * tail;
        let extent_y =
            (self.sin_angle.abs() * self.radius_x + self.cos_angle.abs() * self.radius_y) * tail;
        let Some((start_x, end_x, start_y, end_y)) = image_bounds(
            width,
            height,
            self.center_x - extent_x,
            self.center_x + extent_x,
            self.center_y - extent_y,
            self.center_y + extent_y,
        ) else {
            return;
        };

        for y in start_y..=end_y {
            for x in start_x..=end_x {
                let dx = x as f32 + 0.5 - self.center_x;
                let dy = y as f32 + 0.5 - self.center_y;
                let local_x = dx * self.cos_angle + dy * self.sin_angle;
                let local_y = -dx * self.sin_angle + dy * self.cos_angle;
                let angle = local_y.atan2(local_x);
                let radial_adjustment = 1.0
                    + self.deformation
                        * (self.deformation_frequency * angle + self.deformation_phase).sin();
                let d2 = (local_x / self.radius_x).powi(2) + (local_y / self.radius_y).powi(2);
                let normalized_distance = d2.sqrt() / radial_adjustment;
                let coverage =
                    soft_rectangle_coverage((normalized_distance - 1.0) * nominal_radius, softness);
                let rim = smoothstep(0.52, 0.90, normalized_distance);
                let glow = (0.74
                    + 0.14 * rim
                    + 0.04 * (local_x / self.radius_x * 2.1 + self.glow_phase).sin()
                    + 0.03 * (local_y / self.radius_y * 1.7 - self.glow_phase).sin())
                .clamp(0.66, 0.98);
                let contribution = self.intensity * coverage * glow;
                if contribution >= TAIL_THRESHOLD {
                    accumulate_pixel(effects, width, x, y, contribution);
                }
            }
        }
    }
}

fn generate_twinkles<R: Rng + ?Sized>(
    settings: &BokehSettings,
    width: u32,
    height: u32,
    rng: &mut R,
) -> Vec<Twinkle> {
    let count = twinkle_count(settings.render.density);
    let longest_side = width.max(height) as f32;
    let mut twinkles = Vec::with_capacity(count);

    for index in 0..count {
        let scale = sample_object_scale_at(
            settings.size,
            settings.uniform,
            stratified_quantile(index, count, rng),
            rng,
        );
        let radius_x = (longest_side * scale * 0.5).max(0.5);
        let radius_y = radius_x * rng.random_range(0.75..1.25);
        let placement = selected_placement(&settings.placements, rng);
        let (center_x, center_y) = placement_position(placement, width, height, rng);
        let angle = rng.random_range(0.0..std::f32::consts::TAU);

        twinkles.push(Twinkle {
            center_x,
            center_y,
            radius_x,
            radius_y,
            sin_angle: angle.sin(),
            cos_angle: angle.cos(),
            intensity: rng.random_range(0.20..0.72),
            deformation: rng.random_range(0.015..0.065),
            deformation_frequency: rng.random_range(2..=4) as f32,
            deformation_phase: rng.random_range(0.0..std::f32::consts::TAU),
            glow_phase: rng.random_range(0.0..std::f32::consts::TAU),
        });
    }

    twinkles
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeDirection {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StripMode {
    Start,
    End,
}

#[derive(Debug, Clone, PartialEq)]
struct EdgeProfile {
    values: Vec<f32>,
}

impl EdgeProfile {
    fn random<R: Rng + ?Sized>(changes: usize, rng: &mut R) -> Self {
        let mut value: f32 = rng.random_range(-0.60..=0.60);
        let mut values = Vec::with_capacity(changes + 1);
        values.push(value);
        for _ in 0..changes {
            // Correlated controls keep sharp boundaries torn rather than saw-toothed.
            value = (value + rng.random_range(-0.55..=0.55)).clamp(-1.0, 1.0);
            values.push(value);
        }

        Self { values }
    }

    fn random_torn<R: Rng + ?Sized>(changes: usize, rng: &mut R) -> Self {
        let mut value: f32 = rng.random_range(-0.06..=0.06);
        let mut values = Vec::with_capacity(changes + 1);
        values.push(value);
        for _ in 0..changes {
            value = if rng.random_bool(0.18) {
                let amplitude = rng.random_range(0.15..=0.18);
                if rng.random_bool(0.5) {
                    -amplitude
                } else {
                    amplitude
                }
            } else {
                (value + rng.random_range(-0.05..=0.05)).clamp(-0.10, 0.10)
            };
            values.push(value);
        }

        Self { values }
    }

    fn sample(&self, position: f32) -> f32 {
        debug_assert!(self.values.len() >= 2);
        let segments = self.values.len() - 1;
        let scaled = position.clamp(0.0, 1.0) * segments as f32;
        let index = scaled.floor() as usize;
        if index >= segments {
            return self.values[segments];
        }
        let previous = self.values[index.saturating_sub(1)];
        let current = self.values[index];
        let next = self.values[index + 1];
        let following = self.values[(index + 2).min(segments)];
        let progress = scaled - index as f32;
        let progress_squared = progress * progress;
        let progress_cubed = progress_squared * progress;
        let interpolated = 0.5
            * ((2.0 * current)
                + (-previous + next) * progress
                + (2.0 * previous - 5.0 * current + 4.0 * next - following) * progress_squared
                + (-previous + 3.0 * current - 3.0 * next + following) * progress_cubed);

        interpolated.clamp(
            previous.min(current).min(next).min(following),
            previous.max(current).max(next).max(following),
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
struct EdgeExposure {
    direction: EdgeDirection,
    penetration: f32,
    intensity: f32,
    broad_profile: EdgeProfile,
    torn_profile: EdgeProfile,
    brightness_profile: EdgeProfile,
    broad_depth_variation: f32,
    brightness_variation: f32,
    bright_center: f32,
    bright_spread: f32,
    bright_strength: f32,
}

impl EdgeExposure {
    fn rasterize(&self, effects: &mut [f32], width: u32, height: u32, blur: BlurParameters) {
        match self.direction {
            EdgeDirection::Left => {
                self.rasterize_vertical(effects, width, height, StripMode::Start, blur)
            }
            EdgeDirection::Right => {
                self.rasterize_vertical(effects, width, height, StripMode::End, blur)
            }
            EdgeDirection::Top => {
                self.rasterize_horizontal(effects, width, height, StripMode::Start, blur)
            }
            EdgeDirection::Bottom => {
                self.rasterize_horizontal(effects, width, height, StripMode::End, blur)
            }
        }
    }

    fn rasterize_vertical(
        &self,
        effects: &mut [f32],
        width: u32,
        height: u32,
        mode: StripMode,
        blur: BlurParameters,
    ) {
        for y in 0..height {
            let (depth, brightness) = self.modulation(y as f32 / height as f32);
            let (start_x, end_x) = strip_bounds(width, self.tail_extent(depth, blur), mode);

            for x in start_x..=end_x {
                let distance = strip_distance(x as f32 + 0.5, width as f32, mode);
                let contribution = self.contribution(distance, depth, brightness, blur);
                if contribution >= TAIL_THRESHOLD {
                    accumulate_pixel(effects, width, x, y, contribution);
                }
            }
        }
    }

    fn rasterize_horizontal(
        &self,
        effects: &mut [f32],
        width: u32,
        height: u32,
        mode: StripMode,
        blur: BlurParameters,
    ) {
        for x in 0..width {
            let (depth, brightness) = self.modulation(x as f32 / width as f32);
            let (start_y, end_y) = strip_bounds(height, self.tail_extent(depth, blur), mode);

            for y in start_y..=end_y {
                let distance = strip_distance(y as f32 + 0.5, height as f32, mode);
                let contribution = self.contribution(distance, depth, brightness, blur);
                if contribution >= TAIL_THRESHOLD {
                    accumulate_pixel(effects, width, x, y, contribution);
                }
            }
        }
    }

    fn modulation(&self, position: f32) -> (f32, f32) {
        let broad_depth =
            self.penetration * self.broad_depth_variation * self.broad_profile.sample(position);
        let torn_depth = self.penetration * self.torn_profile.sample(position);
        let depth = (self.penetration + broad_depth + torn_depth).max(0.5);
        let bloom = (-2.4 * ((position - self.bright_center) / self.bright_spread).powi(2)).exp();
        let brightness = (0.62
            + self.brightness_variation * self.brightness_profile.sample(position)
            + self.bright_strength * bloom)
            .clamp(0.18, 1.0);

        (depth, brightness)
    }

    fn tail_extent(&self, depth: f32, blur: BlurParameters) -> f32 {
        depth + blur.edge_softness + 1.0
    }

    fn contribution(
        &self,
        distance: f32,
        depth: f32,
        brightness: f32,
        blur: BlurParameters,
    ) -> f32 {
        let field = (-EDGE_FALLOFF * (distance / depth).powi(2)).exp();
        let boundary = soft_rectangle_coverage(distance - depth, blur.edge_softness);

        self.intensity * brightness * field * boundary
    }
}

fn strip_bounds(length: u32, extent: f32, mode: StripMode) -> (u32, u32) {
    let last = length.saturating_sub(1) as f32;
    let (start, end) = match mode {
        StripMode::Start => (0.0, extent),
        StripMode::End => (last - extent, last),
    };

    (
        start.floor().max(0.0) as u32,
        end.ceil().min(last).max(0.0) as u32,
    )
}

fn strip_distance(position: f32, length: f32, mode: StripMode) -> f32 {
    match mode {
        StripMode::Start => position,
        StripMode::End => length - position,
    }
}

fn generate_edge_exposures<R: Rng + ?Sized>(
    settings: &BokehSettings,
    width: u32,
    height: u32,
    rng: &mut R,
) -> Vec<EdgeExposure> {
    let count = edge_count(settings.render.density);
    let density_scale = f32::from(settings.render.density) / 100.0;
    let mut exposures = Vec::with_capacity(count);

    for _ in 0..count {
        let placement = selected_placement(&settings.placements, rng);
        let direction = edge_direction(placement, rng);
        let dimension = match direction {
            EdgeDirection::Left | EdgeDirection::Right => width,
            EdgeDirection::Top | EdgeDirection::Bottom => height,
        } as f32;
        let scale = sample_object_scale(settings.size, settings.uniform, rng);

        exposures.push(EdgeExposure {
            direction,
            penetration: (dimension * scale * rng.random_range(0.70..1.02)).max(0.5),
            intensity: (rng.random_range(0.30..0.70) * (0.78 + density_scale * 0.45)).min(0.92),
            broad_profile: EdgeProfile::random(rng.random_range(3..=7), rng),
            torn_profile: EdgeProfile::random_torn(rng.random_range(8..=20), rng),
            brightness_profile: EdgeProfile::random(rng.random_range(3..=7), rng),
            broad_depth_variation: 0.15,
            brightness_variation: rng.random_range(0.06..0.12),
            bright_center: rng.random_range(0.10..0.90),
            bright_spread: rng.random_range(0.12..0.30),
            bright_strength: rng.random_range(0.10..0.20),
        });
    }

    exposures
}

fn edge_direction<R: Rng + ?Sized>(
    placement: Option<BokehPlacement>,
    rng: &mut R,
) -> EdgeDirection {
    match placement {
        Some(BokehPlacement::Left) => EdgeDirection::Left,
        Some(BokehPlacement::Right) => EdgeDirection::Right,
        Some(BokehPlacement::Top) => EdgeDirection::Top,
        Some(BokehPlacement::Bottom) => EdgeDirection::Bottom,
        Some(BokehPlacement::Center) => {
            unreachable!("BokehSettings::validate rejects center edge placement")
        }
        None => match rng.random_range(0..4) {
            0 => EdgeDirection::Left,
            1 => EdgeDirection::Right,
            2 => EdgeDirection::Top,
            _ => EdgeDirection::Bottom,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct DamageSegment {
    center_x: f32,
    center_y: f32,
    half_width: f32,
    half_height: f32,
    sin_angle: f32,
    cos_angle: f32,
    intensity: f32,
    softness: f32,
    deformation: f32,
    x_frequency: f32,
    y_frequency: f32,
    x_phase: f32,
    y_phase: f32,
    fragment_frequency: f32,
    fragment_phase: f32,
}

impl DamageSegment {
    fn rasterize(&self, effects: &mut [f32], width: u32, height: u32, blur: BlurParameters) {
        let sharp_softness = (self.half_width.min(self.half_height) * 0.025).max(1.0);
        let softness = lerp(
            sharp_softness,
            self.softness.max(sharp_softness),
            blur.fraction,
        );
        let padding = softness * 2.0 + self.deformation;
        let extent_x = self.cos_angle.abs() * self.half_width
            + self.sin_angle.abs() * self.half_height
            + padding;
        let extent_y = self.sin_angle.abs() * self.half_width
            + self.cos_angle.abs() * self.half_height
            + padding;
        let Some((start_x, end_x, start_y, end_y)) = image_bounds(
            width,
            height,
            self.center_x - extent_x,
            self.center_x + extent_x,
            self.center_y - extent_y,
            self.center_y + extent_y,
        ) else {
            return;
        };

        for y in start_y..=end_y {
            for x in start_x..=end_x {
                let dx = x as f32 + 0.5 - self.center_x;
                let dy = y as f32 + 0.5 - self.center_y;
                let local_x = dx * self.cos_angle + dy * self.sin_angle;
                let local_y = -dx * self.sin_angle + dy * self.cos_angle;
                let normalized_x = local_x / self.half_width;
                let normalized_y = local_y / self.half_height;
                let boundary_wave = 0.52
                    * (normalized_x * self.x_frequency * std::f32::consts::TAU + self.x_phase)
                        .sin()
                    + 0.33
                        * (normalized_y * self.y_frequency * std::f32::consts::TAU + self.y_phase)
                            .sin()
                    + 0.15
                        * ((normalized_x * self.x_frequency * 0.53
                            + normalized_y * self.y_frequency * 0.79)
                            * std::f32::consts::TAU
                            + self.x_phase
                            - self.y_phase)
                            .sin();
                let boundary_offset = self.deformation * boundary_wave;
                let signed_distance =
                    rounded_box_distance(local_x, local_y, self.half_width, self.half_height)
                        + boundary_offset;
                let coverage = soft_rectangle_coverage(signed_distance, softness);
                if coverage == 0.0 {
                    continue;
                }

                let fragment = (0.78
                    + 0.15
                        * (normalized_x * self.fragment_frequency * std::f32::consts::TAU
                            + self.fragment_phase)
                            .sin()
                        * (normalized_y * self.fragment_frequency * 0.71 * std::f32::consts::TAU
                            - self.fragment_phase)
                            .sin()
                    + 0.07
                        * ((normalized_x * 0.43 + normalized_y * 0.67)
                            * self.fragment_frequency
                            * std::f32::consts::TAU
                            + self.fragment_phase * 0.3)
                            .sin())
                .clamp(0.38, 1.0);
                let contribution = self.intensity * coverage * fragment;
                if contribution >= TAIL_THRESHOLD {
                    accumulate_pixel(effects, width, x, y, contribution);
                }
            }
        }
    }
}

fn generate_damage_segments<R: Rng + ?Sized>(
    settings: &BokehSettings,
    width: u32,
    height: u32,
    rng: &mut R,
) -> Vec<DamageSegment> {
    let count = damage_count(settings.render.density);
    let longest_side = width.max(height) as f32;
    let cluster_count = (1 + count / 7).clamp(1, 5);
    let mut clusters = Vec::with_capacity(cluster_count);
    let cluster_spread = longest_side * (0.05 + maximum_scale(settings.size) * 0.11);

    for _ in 0..cluster_count {
        clusters.push(placement_position(
            selected_placement(&settings.placements, rng),
            width,
            height,
            rng,
        ));
    }

    let mut segments = Vec::with_capacity(count);
    for index in 0..count {
        let scale = sample_object_scale_at(
            settings.size,
            settings.uniform,
            stratified_quantile(index, count, rng),
            rng,
        );
        let (center_x, center_y) = if rng.random_bool(0.74) {
            let (cluster_x, cluster_y) = clusters[rng.random_range(0..clusters.len())];
            (
                cluster_x + bell_curve_offset(cluster_spread, rng),
                cluster_y + bell_curve_offset(cluster_spread, rng),
            )
        } else {
            placement_position(
                selected_placement(&settings.placements, rng),
                width,
                height,
                rng,
            )
        };
        let characteristic = (longest_side * scale).max(2.0);
        let half_extent = (characteristic * rng.random_range(0.08..0.24)).max(1.0);
        let aspect = rng.random_range(0.38_f32..2.65_f32).sqrt();
        let half_width = (half_extent * aspect).max(1.0);
        let half_height = (half_extent / aspect).max(1.0);
        let angle: f32 = rng.random_range(-0.20..0.20);
        let softness = (half_width.min(half_height) * rng.random_range(0.18..0.40)).max(1.0);

        segments.push(DamageSegment {
            center_x,
            center_y,
            half_width,
            half_height,
            sin_angle: angle.sin(),
            cos_angle: angle.cos(),
            intensity: rng.random_range(0.16..0.62),
            softness,
            deformation: softness * rng.random_range(0.25..0.90),
            x_frequency: rng.random_range(0.45..1.35),
            y_frequency: rng.random_range(0.45..1.35),
            x_phase: rng.random_range(0.0..std::f32::consts::TAU),
            y_phase: rng.random_range(0.0..std::f32::consts::TAU),
            fragment_frequency: rng.random_range(0.35..1.10),
            fragment_phase: rng.random_range(0.0..std::f32::consts::TAU),
        });
    }

    segments
}

fn rounded_box_distance(x: f32, y: f32, half_width: f32, half_height: f32) -> f32 {
    let dx = x.abs() - half_width;
    let dy = y.abs() - half_height;
    let outside = dx.max(0.0).hypot(dy.max(0.0));

    outside + dx.max(dy).min(0.0)
}

fn soft_rectangle_coverage(signed_distance: f32, softness: f32) -> f32 {
    let progress = ((softness - signed_distance) / (softness * 2.0)).clamp(0.0, 1.0);
    progress * progress * (3.0 - 2.0 * progress)
}

fn smoothstep(start: f32, end: f32, value: f32) -> f32 {
    let progress = ((value - start) / (end - start)).clamp(0.0, 1.0);
    progress * progress * (3.0 - 2.0 * progress)
}

fn image_bounds(
    width: u32,
    height: u32,
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
) -> Option<(u32, u32, u32, u32)> {
    let last_x = width.saturating_sub(1) as f32;
    let last_y = height.saturating_sub(1) as f32;
    if max_x < 0.0 || max_y < 0.0 || min_x > last_x || min_y > last_y {
        return None;
    }

    let start_x = min_x.floor().max(0.0) as u32;
    let end_x = max_x.ceil().min(last_x) as u32;
    let start_y = min_y.floor().max(0.0) as u32;
    let end_y = max_y.ceil().min(last_y) as u32;

    (start_x <= end_x && start_y <= end_y).then_some((start_x, end_x, start_y, end_y))
}

#[cfg(test)]
#[path = "bokeh_ut.rs"]
mod bokeh_ut;
