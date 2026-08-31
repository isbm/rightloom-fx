use image::{Rgba, RgbaImage};
use rand::Rng;

use crate::render::{RenderError, RenderSettings, write_images};

const FULL_COVERAGE_TARGET: f32 = 0.99;
const OCCUPANCY_GRID_LONGEST_SIDE: u32 = 160;
const OCCUPANCY_THRESHOLD: f32 = 0.10;
const COVERAGE_SOFTNESS: f32 = 0.10;
const COVERAGE_TOLERANCE: f32 = 0.025;
const MIN_LIGHT_FIELDS: usize = 4;
const MAX_LIGHT_FIELDS: usize = 6;
const MAX_TOTAL_FIELDS: usize = 8;
const MAX_FIELD_CANDIDATES: usize = 192;
const TAIL_THRESHOLD: f32 = 1e-4;

#[derive(Debug, Clone)]
pub struct BurnSettings {
    pub render: RenderSettings,
    pub size: u8,
    pub blur: u8,
    pub lightness: u8,
    pub saturation: u8,
}

impl BurnSettings {
    fn validate(&self) -> Result<(), RenderError> {
        if self.size > 100 {
            return Err(RenderError::InvalidSize(self.size));
        }
        if self.blur > 100 {
            return Err(RenderError::InvalidBlur(self.blur));
        }
        if self.lightness > 100 {
            return Err(RenderError::InvalidLightness(self.lightness));
        }
        if self.saturation > 100 {
            return Err(RenderError::InvalidSaturation(self.saturation));
        }

        Ok(())
    }
}

pub fn generate_images(settings: &BurnSettings) -> Result<(), RenderError> {
    let mut rng = rand::rng();
    generate_images_with_rng(settings, &mut rng)
}

fn generate_images_with_rng<R: Rng + ?Sized>(
    settings: &BurnSettings,
    rng: &mut R,
) -> Result<(), RenderError> {
    settings.validate()?;
    write_images(&settings.render, "burn", || render_image(settings, rng))
}

fn render_image<R: Rng + ?Sized>(settings: &BurnSettings, rng: &mut R) -> RgbaImage {
    let width = settings.render.resolution.width();
    let height = settings.render.resolution.height();
    let scene = generate_scene(settings, width, height, rng);
    let mut exposure = vec![[0.0; 3]; width as usize * height as usize];
    let mut alpha = vec![0.0; width as usize * height as usize];

    for field in &scene.light_fields {
        field.rasterize_light(&mut exposure, &mut alpha, width, height, settings.blur);
    }
    for field in &scene.burn_fields {
        field.rasterize_burn(&mut exposure, &mut alpha, width, height, settings.blur);
    }

    buffers_to_image(
        &exposure,
        &alpha,
        width,
        height,
        settings.lightness,
        settings.saturation,
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FilmColor {
    red: f32,
    green: f32,
    blue: f32,
}

impl FilmColor {
    const fn new(red: f32, green: f32, blue: f32) -> Self {
        Self { red, green, blue }
    }

    fn channels(self) -> [f32; 3] {
        [self.red, self.green, self.blue]
    }
}

const AMBER: FilmColor = FilmColor::new(1.00, 0.38, 0.035);
const DEEP_ORANGE: FilmColor = FilmColor::new(0.96, 0.16, 0.018);
const GOLDEN_YELLOW: FilmColor = FilmColor::new(1.00, 0.73, 0.12);
const RED_ORANGE: FilmColor = FilmColor::new(0.82, 0.10, 0.022);
const YELLOW_GREEN: FilmColor = FilmColor::new(0.71, 0.86, 0.12);
const GREEN_CYAN: FilmColor = FilmColor::new(0.12, 0.70, 0.50);
const TURQUOISE: FilmColor = FilmColor::new(0.035, 0.66, 0.72);
const DEEP_BLUE: FilmColor = FilmColor::new(0.015, 0.10, 0.62);
const INDIGO: FilmColor = FilmColor::new(0.09, 0.035, 0.39);
const VIOLET: FilmColor = FilmColor::new(0.29, 0.055, 0.46);
const DARK_RED: FilmColor = FilmColor::new(0.38, 0.035, 0.018);
const BROWN: FilmColor = FilmColor::new(0.31, 0.09, 0.018);
const BURGUNDY: FilmColor = FilmColor::new(0.25, 0.012, 0.055);

const BURN_PALETTE: [FilmColor; 3] = [DARK_RED, BROWN, BURGUNDY];
const WARM_TO_COOL_CHAIN: [FilmColor; 7] = [
    RED_ORANGE,
    DEEP_ORANGE,
    AMBER,
    GOLDEN_YELLOW,
    GREEN_CYAN,
    TURQUOISE,
    DEEP_BLUE,
];
const COOL_TO_WARM_CHAIN: [FilmColor; 7] = [
    VIOLET,
    INDIGO,
    DEEP_BLUE,
    GREEN_CYAN,
    YELLOW_GREEN,
    GOLDEN_YELLOW,
    AMBER,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldKind {
    Light,
    Burn,
}

#[derive(Debug, Clone, PartialEq)]
struct BoundaryProfile {
    values: Vec<f32>,
}

impl BoundaryProfile {
    fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
        let count = rng.random_range(2..=6);
        let mut value: f32 = 0.0;
        let mut values = Vec::with_capacity(count);

        for _ in 0..count {
            value = (value * 0.45 + rng.random_range(-0.14..=0.14)).clamp(-0.18, 0.18);
            values.push(value);
        }

        Self { values }
    }

    fn scale(&self, angle: f32) -> f32 {
        let count = self.values.len();
        let progress = angle.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
        let scaled = progress * count as f32;
        let index = scaled.floor() as usize % count;
        let next = (index + 1) % count;
        let blend = smoothstep(0.0, 1.0, scaled - scaled.floor());

        1.0 + lerp(self.values[index], self.values[next], blend)
    }

    fn maximum_scale(&self) -> f32 {
        1.0 + self.values.iter().copied().fold(0.0, f32::max)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct BurnField {
    kind: FieldKind,
    color: FilmColor,
    center_x: f32,
    center_y: f32,
    radius_x: f32,
    radius_y: f32,
    sin_angle: f32,
    cos_angle: f32,
    intensity: f32,
    burn_strength: f32,
    boundary: BoundaryProfile,
}

impl BurnField {
    fn normalized_distance(&self, x: f32, y: f32) -> f32 {
        let dx = x - self.center_x;
        let dy = y - self.center_y;
        let local_x = dx * self.cos_angle + dy * self.sin_angle;
        let local_y = -dx * self.sin_angle + dy * self.cos_angle;
        let angle = local_y.atan2(local_x);
        let elliptical_distance =
            ((local_x / self.radius_x).powi(2) + (local_y / self.radius_y).powi(2)).sqrt();

        elliptical_distance / self.boundary.scale(angle)
    }

    fn contribution(&self, x: f32, y: f32, softness: f32) -> f32 {
        let distance = self.normalized_distance(x, y);
        let edge = 1.0 - smoothstep(1.0 - softness, 1.0 + softness, distance);
        let interior_gradient = 1.0 - 0.28 * distance.clamp(0.0, 1.0);

        (self.intensity * edge * interior_gradient).clamp(0.0, 1.0)
    }

    fn coverage_contribution(&self, x: f32, y: f32) -> f32 {
        self.contribution(x, y, COVERAGE_SOFTNESS)
    }

    fn bounds(&self, width: u32, height: u32, softness: f32) -> Option<(u32, u32, u32, u32)> {
        let expansion = self.boundary.maximum_scale() * (1.0 + softness);
        let extent_x = (self.cos_angle.abs() * self.radius_x
            + self.sin_angle.abs() * self.radius_y)
            * expansion;
        let extent_y = (self.sin_angle.abs() * self.radius_x
            + self.cos_angle.abs() * self.radius_y)
            * expansion;

        image_bounds(
            width,
            height,
            self.center_x - extent_x,
            self.center_x + extent_x,
            self.center_y - extent_y,
            self.center_y + extent_y,
        )
    }

    fn rasterize_light(
        &self,
        exposure: &mut [[f32; 3]],
        alpha: &mut [f32],
        width: u32,
        height: u32,
        blur: u8,
    ) {
        debug_assert_eq!(self.kind, FieldKind::Light);
        let softness = field_softness(blur);
        let Some((start_x, end_x, start_y, end_y)) = self.bounds(width, height, softness) else {
            return;
        };

        for y in start_y..=end_y {
            for x in start_x..=end_x {
                let contribution = self.contribution(x as f32 + 0.5, y as f32 + 0.5, softness);
                if contribution < TAIL_THRESHOLD {
                    continue;
                }

                let index = y as usize * width as usize + x as usize;
                screen_accumulate(&mut exposure[index], self.color, contribution);
                alpha[index] = accumulate_alpha(alpha[index], contribution);
            }
        }
    }

    fn rasterize_burn(
        &self,
        exposure: &mut [[f32; 3]],
        alpha: &mut [f32],
        width: u32,
        height: u32,
        blur: u8,
    ) {
        debug_assert_eq!(self.kind, FieldKind::Burn);
        let softness = field_softness(blur);
        let Some((start_x, end_x, start_y, end_y)) = self.bounds(width, height, softness) else {
            return;
        };

        for y in start_y..=end_y {
            for x in start_x..=end_x {
                let contribution = self.contribution(x as f32 + 0.5, y as f32 + 0.5, softness);
                if contribution < TAIL_THRESHOLD {
                    continue;
                }

                let index = y as usize * width as usize + x as usize;
                let attenuation = burn_attenuation(self.burn_strength, contribution);
                for (channel, burn_channel) in exposure[index].iter_mut().zip(self.color.channels())
                {
                    let warm_filter = 1.0 - 0.24 * contribution * (1.0 - burn_channel);
                    *channel *= attenuation * warm_filter;
                }
                alpha[index] = accumulate_alpha(alpha[index], contribution * 0.88);
            }
        }
    }
}

#[derive(Debug)]
struct BurnScene {
    light_fields: Vec<BurnField>,
    burn_fields: Vec<BurnField>,
    #[cfg(test)]
    initial_light_count: usize,
    #[cfg(test)]
    coverage: f32,
}

#[derive(Debug)]
struct BurnOccupancy {
    grid_width: u32,
    grid_height: u32,
    canvas_width: u32,
    canvas_height: u32,
    occupied: Vec<bool>,
    occupied_count: usize,
}

impl BurnOccupancy {
    fn new(canvas_width: u32, canvas_height: u32) -> Self {
        let longest_side = canvas_width.max(canvas_height);
        let grid_width = occupancy_grid_dimension(canvas_width, longest_side);
        let grid_height = occupancy_grid_dimension(canvas_height, longest_side);

        Self {
            grid_width,
            grid_height,
            canvas_width,
            canvas_height,
            occupied: vec![false; grid_width as usize * grid_height as usize],
            occupied_count: 0,
        }
    }

    fn coverage(&self) -> f32 {
        self.occupied_count as f32 / self.occupied.len() as f32
    }

    fn cells_for_field(&self, field: &BurnField) -> (usize, usize) {
        let Some((start_x, end_x, start_y, end_y)) = self.bounds_for_field(field) else {
            return (0, 0);
        };
        let mut new_cells = 0;
        let mut total_cells = 0;

        for y in start_y..=end_y {
            let sample_y = (y as f32 + 0.5) * self.canvas_height as f32 / self.grid_height as f32;
            for x in start_x..=end_x {
                let index = y as usize * self.grid_width as usize + x as usize;
                let sample_x = (x as f32 + 0.5) * self.canvas_width as f32 / self.grid_width as f32;
                if field.coverage_contribution(sample_x, sample_y) >= OCCUPANCY_THRESHOLD {
                    total_cells += 1;
                    if !self.occupied[index] {
                        new_cells += 1;
                    }
                }
            }
        }

        (new_cells, total_cells)
    }

    fn mark_field(&mut self, field: &BurnField) {
        let Some((start_x, end_x, start_y, end_y)) = self.bounds_for_field(field) else {
            return;
        };

        for y in start_y..=end_y {
            let sample_y = (y as f32 + 0.5) * self.canvas_height as f32 / self.grid_height as f32;
            for x in start_x..=end_x {
                let index = y as usize * self.grid_width as usize + x as usize;
                if self.occupied[index] {
                    continue;
                }

                let sample_x = (x as f32 + 0.5) * self.canvas_width as f32 / self.grid_width as f32;
                if field.coverage_contribution(sample_x, sample_y) >= OCCUPANCY_THRESHOLD {
                    self.occupied[index] = true;
                    self.occupied_count += 1;
                }
            }
        }
    }

    fn coverage_with_added_cells(&self, cells: usize) -> f32 {
        (self.occupied_count + cells) as f32 / self.occupied.len() as f32
    }

    fn bounds_for_field(&self, field: &BurnField) -> Option<(u32, u32, u32, u32)> {
        let expansion = field.boundary.maximum_scale() * (1.0 + COVERAGE_SOFTNESS);
        let extent_x = (field.cos_angle.abs() * field.radius_x
            + field.sin_angle.abs() * field.radius_y)
            * expansion;
        let extent_y = (field.sin_angle.abs() * field.radius_x
            + field.cos_angle.abs() * field.radius_y)
            * expansion;
        let (start_x, end_x) = occupancy_axis_bounds(
            self.grid_width,
            self.canvas_width,
            field.center_x - extent_x,
            field.center_x + extent_x,
        )?;
        let (start_y, end_y) = occupancy_axis_bounds(
            self.grid_height,
            self.canvas_height,
            field.center_y - extent_y,
            field.center_y + extent_y,
        )?;

        Some((start_x, end_x, start_y, end_y))
    }
}

fn generate_scene<R: Rng + ?Sized>(
    settings: &BurnSettings,
    width: u32,
    height: u32,
    rng: &mut R,
) -> BurnScene {
    let target = coverage_target(settings.render.density);
    if target == 0.0 {
        return BurnScene {
            light_fields: Vec::new(),
            burn_fields: Vec::new(),
            #[cfg(test)]
            initial_light_count: 0,
            #[cfg(test)]
            coverage: 0.0,
        };
    }

    let initial_light_count = initial_light_count(settings, width, height, target);
    let chain = choose_light_chain(rng);
    let mut occupancy = BurnOccupancy::new(width, height);
    let mut light_fields = Vec::with_capacity(MAX_LIGHT_FIELDS);

    for index in 0..initial_light_count {
        let field = choose_light_field(
            settings,
            (width, height),
            (index, initial_light_count),
            chain,
            target,
            &occupancy,
            rng,
        );
        occupancy.mark_field(&field);
        light_fields.push(field);
    }

    while occupancy.coverage() < target && light_fields.len() < MAX_LIGHT_FIELDS {
        let remaining_capacity = MAX_LIGHT_FIELDS - light_fields.len();
        let correction =
            correction_light_count(settings, width, height, target, occupancy.coverage())
                .clamp(1, remaining_capacity);
        let total = light_fields.len() + correction;

        for _ in 0..correction {
            let index = light_fields.len();
            let field = choose_light_field(
                settings,
                (width, height),
                (index, total),
                chain,
                target,
                &occupancy,
                rng,
            );
            occupancy.mark_field(&field);
            light_fields.push(field);

            if occupancy.coverage() >= target {
                break;
            }
        }
    }

    if occupancy.coverage() < target && light_fields.len() < MAX_TOTAL_FIELDS {
        let field = coverage_field(settings, width, height, chain, light_fields.len(), rng);
        occupancy.mark_field(&field);
        light_fields.push(field);
    }

    let burn_fields = generate_burn_fields(settings, &light_fields, rng);

    BurnScene {
        light_fields,
        burn_fields,
        #[cfg(test)]
        initial_light_count,
        #[cfg(test)]
        coverage: occupancy.coverage(),
    }
}

fn coverage_target(density: u8) -> f32 {
    match density {
        0 => 0.0,
        100 => FULL_COVERAGE_TARGET,
        _ => f32::from(density) / 100.0,
    }
}

fn initial_light_count(settings: &BurnSettings, width: u32, height: u32, target: f32) -> usize {
    let canvas_area = width as f32 * height as f32;
    let expected_area = expected_light_area(settings, width, height);
    estimated_field_count(canvas_area, target, expected_area).clamp(MIN_LIGHT_FIELDS, 5)
}

fn correction_light_count(
    settings: &BurnSettings,
    width: u32,
    height: u32,
    target: f32,
    coverage: f32,
) -> usize {
    let remaining_target = (target - coverage) / (1.0 - coverage);
    estimated_field_count(
        width as f32 * height as f32,
        remaining_target,
        expected_light_area(settings, width, height),
    )
    .max(1)
}

fn estimated_field_count(canvas_area: f32, target: f32, expected_area: f32) -> usize {
    if target <= 0.0 {
        return 0;
    }

    (-((1.0 - target).ln()) * canvas_area / expected_area)
        .ceil()
        .max(1.0) as usize
}

fn expected_light_area(settings: &BurnSettings, width: u32, height: u32) -> f32 {
    let radius = characteristic_radius(settings.size, width.max(height));
    std::f32::consts::PI * radius * radius * 0.55
}

fn characteristic_radius(size: u8, longest_side: u32) -> f32 {
    let size = f32::from(size) / 100.0;
    longest_side as f32 * lerp(0.12, 1.50, size * size)
}

fn choose_light_field<R: Rng + ?Sized>(
    settings: &BurnSettings,
    canvas: (u32, u32),
    sequence: (usize, usize),
    chain: &[FilmColor],
    target: f32,
    occupancy: &BurnOccupancy,
    rng: &mut R,
) -> BurnField {
    let (width, height) = canvas;
    let (index, total) = sequence;
    let ceiling = coverage_ceiling(target);
    let desired_coverage = target * (index + 1) as f32 / total.max(1) as f32;
    let mut closest_match = None;
    let mut smallest_overshoot = None;

    for _ in 0..MAX_FIELD_CANDIDATES {
        let field = sample_light_field(settings, width, height, index, total, chain, rng);
        let (added_cells, field_cells) = occupancy.cells_for_field(&field);
        if added_cells == 0 {
            continue;
        }

        let resulting_coverage = occupancy.coverage_with_added_cells(added_cells);
        if resulting_coverage <= ceiling {
            let overlap = 1.0 - added_cells as f32 / field_cells as f32;
            let distance = (resulting_coverage - desired_coverage).abs() + 0.25 * overlap;
            if closest_match
                .as_ref()
                .is_none_or(|(_, closest)| distance < *closest)
            {
                closest_match = Some((field, distance));
            }
            continue;
        }

        if smallest_overshoot
            .as_ref()
            .is_none_or(|(_, smallest)| added_cells < *smallest)
        {
            smallest_overshoot = Some((field, added_cells));
        }
    }

    closest_match
        .map(|(field, _)| field)
        .or_else(|| smallest_overshoot.map(|(field, _)| field))
        .unwrap_or_else(|| sample_light_field(settings, width, height, index, total, chain, rng))
}

fn coverage_ceiling(target: f32) -> f32 {
    if target >= FULL_COVERAGE_TARGET {
        1.0
    } else {
        (target + COVERAGE_TOLERANCE).min(1.0)
    }
}

fn sample_light_field<R: Rng + ?Sized>(
    settings: &BurnSettings,
    width: u32,
    height: u32,
    index: usize,
    total: usize,
    chain: &[FilmColor],
    rng: &mut R,
) -> BurnField {
    let base_radius = characteristic_radius(settings.size, width.max(height));
    let maximum_scale = if settings.size >= 95 && rng.random_bool(0.18) {
        1.65
    } else {
        1.08
    };
    let radius_x = base_radius * rng.random_range(0.42..maximum_scale);
    let radius_y = base_radius * rng.random_range(0.38..maximum_scale * 0.90);
    let angle = rng.random_range(0.0..std::f32::consts::TAU);
    let sin_angle = angle.sin();
    let cos_angle = angle.cos();
    let (center_x, center_y) = external_field_center(
        (width, height),
        (radius_x, radius_y),
        (sin_angle, cos_angle),
        index,
        settings.render.density,
        rng,
    );

    BurnField {
        kind: FieldKind::Light,
        color: chain_color(chain, index, total),
        center_x,
        center_y,
        radius_x,
        radius_y,
        sin_angle,
        cos_angle,
        intensity: rng.random_range(0.78..0.98),
        burn_strength: 0.0,
        boundary: BoundaryProfile::random(rng),
    }
}

fn coverage_field<R: Rng + ?Sized>(
    settings: &BurnSettings,
    width: u32,
    height: u32,
    chain: &[FilmColor],
    index: usize,
    rng: &mut R,
) -> BurnField {
    let longest_side = width.max(height) as f32;
    let size_fraction = f32::from(settings.size) / 100.0;
    let angle = rng.random_range(0.0..std::f32::consts::TAU);

    BurnField {
        kind: FieldKind::Light,
        color: chain_color(chain, index, index + 1),
        center_x: width as f32 * rng.random_range(0.36..0.64),
        center_y: height as f32 * rng.random_range(0.36..0.64),
        radius_x: longest_side * lerp(1.45, 2.35, size_fraction),
        radius_y: longest_side * lerp(1.15, 1.90, size_fraction),
        sin_angle: angle.sin(),
        cos_angle: angle.cos(),
        intensity: rng.random_range(0.16..0.28),
        burn_strength: 0.0,
        boundary: BoundaryProfile::random(rng),
    }
}

fn external_field_center<R: Rng + ?Sized>(
    canvas: (u32, u32),
    radii: (f32, f32),
    rotation: (f32, f32),
    index: usize,
    density: u8,
    rng: &mut R,
) -> (f32, f32) {
    let (width, height) = (canvas.0 as f32, canvas.1 as f32);
    if density >= 75 && index % 4 == 1 {
        return (
            rng.random_range(0.34 * width..0.66 * width),
            rng.random_range(0.30 * height..0.62 * height),
        );
    }
    if density < 75 && rng.random_bool(0.12) {
        return (
            rng.random_range(0.08 * width..0.92 * width),
            rng.random_range(0.08 * height..0.92 * height),
        );
    }

    let (radius_x, radius_y) = radii;
    let (sin_angle, cos_angle) = rotation;
    let extent_x = cos_angle.abs() * radius_x + sin_angle.abs() * radius_y;
    let extent_y = sin_angle.abs() * radius_x + cos_angle.abs() * radius_y;
    let density = f32::from(density) / 100.0;
    let offset = rng.random_range(lerp(0.72, 0.10, density)..lerp(0.94, 0.48, density));
    match index % 4 {
        0 => (
            -extent_x * offset,
            rng.random_range(0.18 * height..0.82 * height),
        ),
        1 => (
            rng.random_range(0.18 * width..0.82 * width),
            -extent_y * offset,
        ),
        2 => (
            width + extent_x * offset,
            rng.random_range(0.18 * height..0.82 * height),
        ),
        _ => (
            rng.random_range(0.18 * width..0.82 * width),
            height + extent_y * offset,
        ),
    }
}

fn choose_light_chain<R: Rng + ?Sized>(rng: &mut R) -> &'static [FilmColor] {
    match rng.random_range(0..10) {
        0..=8 => &WARM_TO_COOL_CHAIN,
        _ => &COOL_TO_WARM_CHAIN,
    }
}

fn chain_color(chain: &[FilmColor], index: usize, total: usize) -> FilmColor {
    let last = chain.len().saturating_sub(1);
    let positions: &[usize] = match total {
        1 => &[3],
        2 => &[1, 6],
        3 => &[0, 3, 6],
        4 => &[0, 3, 4, 6],
        5 => &[0, 2, 3, 4, 6],
        _ => &[],
    };

    if let Some(position) = positions.get(index) {
        return chain[*position];
    }

    let progress = index as f32 / total.saturating_sub(1).max(1) as f32;
    chain[(progress * last as f32).round() as usize]
}

fn generate_burn_fields<R: Rng + ?Sized>(
    settings: &BurnSettings,
    light_fields: &[BurnField],
    rng: &mut R,
) -> Vec<BurnField> {
    if light_fields.is_empty() || settings.render.density < 20 {
        return Vec::new();
    }

    let density = settings.render.density;
    let count = if density >= 85 {
        1 + usize::from(rng.random_bool(0.55))
    } else if rng.random_bool(0.55) {
        1
    } else {
        0
    };
    let remaining_capacity = MAX_TOTAL_FIELDS.saturating_sub(light_fields.len());
    let mut fields = Vec::with_capacity(count.min(remaining_capacity));

    for _ in 0..count.min(remaining_capacity) {
        let source = &light_fields[rng.random_range(0..light_fields.len())];
        let angle = rng.random_range(0.0..std::f32::consts::TAU);
        let boundary_angle = rng.random_range(0.0..std::f32::consts::TAU);
        let offset = rng.random_range(0.34..0.86);

        fields.push(BurnField {
            kind: FieldKind::Burn,
            color: BURN_PALETTE[rng.random_range(0..BURN_PALETTE.len())],
            center_x: source.center_x + source.radius_x * offset * boundary_angle.cos(),
            center_y: source.center_y + source.radius_y * offset * boundary_angle.sin(),
            radius_x: source.radius_x * rng.random_range(0.20..0.45),
            radius_y: source.radius_y * rng.random_range(0.18..0.42),
            sin_angle: angle.sin(),
            cos_angle: angle.cos(),
            intensity: rng.random_range(0.35..0.65),
            burn_strength: rng.random_range(0.30..0.65),
            boundary: BoundaryProfile::random(rng),
        });
    }

    fields
}

fn field_softness(blur: u8) -> f32 {
    lerp(0.015, 0.30, f32::from(blur) / 100.0)
}

fn screen_accumulate(destination: &mut [f32; 3], color: FilmColor, contribution: f32) {
    for (channel, source_color) in destination.iter_mut().zip(color.channels()) {
        let source = source_color * contribution;
        *channel = 1.0 - (1.0 - *channel) * (1.0 - source);
    }
}

fn accumulate_alpha(accumulated: f32, contribution: f32) -> f32 {
    1.0 - (1.0 - accumulated) * (1.0 - contribution.clamp(0.0, 1.0))
}

fn burn_attenuation(strength: f32, contribution: f32) -> f32 {
    (1.0 - strength * contribution).clamp(0.0, 1.0)
}

fn buffers_to_image(
    exposure: &[[f32; 3]],
    alpha: &[f32],
    width: u32,
    height: u32,
    lightness: u8,
    saturation: u8,
) -> RgbaImage {
    let mut image = RgbaImage::new(width, height);
    let lightness = f32::from(lightness) / 100.0;
    let saturation = f32::from(saturation) / 100.0;

    for ((exposure, alpha), pixel) in exposure.iter().zip(alpha).zip(image.pixels_mut()) {
        let alpha = alpha.clamp(0.0, 1.0);
        let straight = if alpha > 0.0 {
            [
                (exposure[0] / alpha).clamp(0.0, 1.0),
                (exposure[1] / alpha).clamp(0.0, 1.0),
                (exposure[2] / alpha).clamp(0.0, 1.0),
            ]
        } else {
            [0.0; 3]
        };
        let luminance = rgb_luminance(straight);
        let final_color = straight.map(|channel| lerp(luminance, channel, saturation) * lightness);

        *pixel = Rgba([
            (final_color[0].clamp(0.0, 1.0) * 255.0).round() as u8,
            (final_color[1].clamp(0.0, 1.0) * 255.0).round() as u8,
            (final_color[2].clamp(0.0, 1.0) * 255.0).round() as u8,
            (alpha * 255.0).round() as u8,
        ]);
    }

    image
}

fn rgb_luminance(color: [f32; 3]) -> f32 {
    0.2126 * color[0] + 0.7152 * color[1] + 0.0722 * color[2]
}

fn occupancy_grid_dimension(length: u32, longest_side: u32) -> u32 {
    ((u64::from(length) * u64::from(OCCUPANCY_GRID_LONGEST_SIDE) + u64::from(longest_side) / 2)
        / u64::from(longest_side))
    .max(1) as u32
}

fn occupancy_axis_bounds(
    cell_count: u32,
    canvas_length: u32,
    minimum: f32,
    maximum: f32,
) -> Option<(u32, u32)> {
    if maximum < 0.0 || minimum > canvas_length as f32 {
        return None;
    }

    let cell_length = canvas_length as f32 / cell_count as f32;
    let last_cell = cell_count.saturating_sub(1) as f32;
    let start = (minimum / cell_length - 0.5).ceil().max(0.0) as u32;
    let end = (maximum / cell_length - 0.5).floor().min(last_cell) as u32;

    (start <= end).then_some((start, end))
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

fn lerp(start: f32, end: f32, progress: f32) -> f32 {
    start + (end - start) * progress
}

fn smoothstep(start: f32, end: f32, value: f32) -> f32 {
    let progress = ((value - start) / (end - start)).clamp(0.0, 1.0);
    progress * progress * (3.0 - 2.0 * progress)
}

#[cfg(test)]
#[path = "burn_ut.rs"]
mod burn_ut;
