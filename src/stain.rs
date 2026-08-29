use image::{Rgba, RgbaImage};
use rand::Rng;

use crate::render::{RenderError, RenderSettings, write_images};

const MAX_SOFTNESS_FRACTION: f32 = 0.35;
const MIN_SOFTNESS_NORMALIZED: f32 = 0.001;
const MAX_WORKING_CELL_SIZE: f32 = 8.0;
const COVERAGE_TAIL_THRESHOLD: f32 = 1e-4;
const DEFAULT_CONTRAST: u8 = 50;
// Existing local-density bounds also define the contrast normalization range.
const MIN_INTERNAL_DENSITY: f32 = 0.20;
const MAX_INTERNAL_DENSITY: f32 = 1.35;
const TIDE_PRESENCE_ANCHORS: [(u8, f64); 6] = [
    (0, 0.0),
    (10, 0.05),
    (25, 0.08),
    (50, 0.12),
    (75, 0.16),
    (100, 0.20),
];
const SECOND_TIDE_LINE_PROBABILITY: f64 = 0.0;
const MAX_TIDE_RELATIVE_MODULATION: f32 = 0.10;
const DENSITY_STRUCTURES_ENABLED: bool = true;
const NORMAL_STAIN_TIDE_CONTRIBUTION_ENABLED: bool = false;
const DENSITY_ALPHA_ANCHORS: [(u8, f32); 8] = [
    (0, 0.0),
    (5, 18.0),
    (10, 35.0),
    (25, 75.0),
    (50, 145.0),
    (70, 195.0),
    (85, 225.0),
    (100, 250.0),
];
const LIGHTNESS_ANCHORS: [(u8, f32); 7] = [
    (0, 0.0),
    (10, 30.0),
    (25, 100.0),
    (50, 190.0),
    (70, 230.0),
    (80, 245.0),
    (100, 255.0),
];

#[derive(Debug, Clone)]
pub struct StainSettings {
    pub render: RenderSettings,
    pub blur: u8,
    pub lightness: u8,
    pub contrast: u8,
}

impl StainSettings {
    fn validate(&self) -> Result<(), RenderError> {
        if self.blur > 100 {
            return Err(RenderError::InvalidBlur(self.blur));
        }
        if self.lightness > 100 {
            return Err(RenderError::InvalidLightness(self.lightness));
        }
        if self.contrast > 100 {
            return Err(RenderError::InvalidContrast(self.contrast));
        }

        Ok(())
    }
}

pub fn generate_images(settings: &StainSettings) -> Result<(), RenderError> {
    let mut rng = rand::rng();
    generate_images_with_rng(settings, &mut rng)
}

fn generate_images_with_rng<R: Rng + ?Sized>(
    settings: &StainSettings,
    rng: &mut R,
) -> Result<(), RenderError> {
    settings.validate()?;
    write_images(&settings.render, "stain", || render_image(settings, rng))
}

#[cfg(test)]
fn generate_images_with_structure_contribution<R: Rng + ?Sized>(
    settings: &StainSettings,
    structures_enabled: bool,
    rng: &mut R,
) -> Result<(), RenderError> {
    settings.validate()?;
    write_images(&settings.render, "stain", || {
        render_image_with_structure_contribution(settings, structures_enabled, rng)
    })
}

fn render_image<R: Rng + ?Sized>(settings: &StainSettings, rng: &mut R) -> RgbaImage {
    render_image_with_structure_contribution(settings, DENSITY_STRUCTURES_ENABLED, rng)
}

fn render_image_with_structure_contribution<R: Rng + ?Sized>(
    settings: &StainSettings,
    structures_enabled: bool,
    rng: &mut R,
) -> RgbaImage {
    let width = settings.render.resolution.width();
    let height = settings.render.resolution.height();
    let mut effects = vec![0.0; width as usize * height as usize];

    if settings.render.density == 0 {
        return scalar_effects_to_image(&effects, width, height);
    }

    let density = settings.render.density;
    let density_scale = density as f32 / 100.0;
    let contrast_gain = contrast_gain(settings.contrast);
    let smallest_dimension = width.min(height) as f32;
    let mut anchors = Vec::new();

    for _ in 0..stain_count(density, rng) {
        let base_radius = smallest_dimension
            * (0.04 + 0.12 * density_scale.sqrt())
            * rng.random_range(0.78..1.18);
        let anchor = choose_anchor(width, height, base_radius, &anchors, density_scale, rng);
        let stain = Stain::new(
            anchor,
            base_radius,
            density,
            density_scale,
            settings.lightness,
            structures_enabled,
            rng,
        )
        .with_contrast_gain(contrast_gain);
        stain.rasterize(&mut effects, width, height, settings.blur);
        anchors.push(anchor);
    }

    scalar_effects_to_image(&effects, width, height)
}

fn accumulate_scalar_effect(accumulated: f32, contribution: f32) -> f32 {
    1.0 - (1.0 - accumulated) * (1.0 - contribution)
}

fn scalar_effects_to_image(effects: &[f32], width: u32, height: u32) -> RgbaImage {
    let mut image = RgbaImage::new(width, height);

    for (effect, pixel) in effects.iter().zip(image.pixels_mut()) {
        let alpha = (effect.clamp(0.0, 1.0) * 255.0).round() as u8;
        *pixel = Rgba([255, 255, 255, alpha]);
    }

    image
}

fn stain_count<R: Rng + ?Sized>(density: u8, rng: &mut R) -> usize {
    let base_count = 1 + (density as f32 / 100.0 * 6.0).round() as usize;
    let additional_count = if density <= 10 {
        1
    } else if density < 70 {
        2
    } else {
        3
    };

    base_count + rng.random_range(0..=additional_count)
}

fn choose_anchor<R: Rng + ?Sized>(
    width: u32,
    height: u32,
    base_radius: f32,
    previous: &[(f32, f32)],
    density_scale: f32,
    rng: &mut R,
) -> (f32, f32) {
    if !previous.is_empty() && rng.random_bool(0.18 + 0.25 * density_scale as f64) {
        let (x, y) = previous[rng.random_range(0..previous.len())];
        return (
            x + rng.random_range(-base_radius..base_radius),
            y + rng.random_range(-base_radius..base_radius),
        );
    }

    let width = width as f32;
    let height = height as f32;
    if !rng.random_bool(0.64) {
        return (
            rng.random_range(width * 0.08..width * 0.92),
            rng.random_range(height * 0.08..height * 0.92),
        );
    }

    let edge_offset = rng.random_range(-base_radius * 0.65..base_radius * 0.35);
    if rng.random_bool(0.34) {
        let x = if rng.random_bool(0.5) {
            edge_offset
        } else {
            width - edge_offset
        };
        let y = if rng.random_bool(0.5) {
            edge_offset
        } else {
            height - edge_offset
        };
        return (x, y);
    }

    match rng.random_range(0..4) {
        0 => (edge_offset, rng.random_range(0.0..height)),
        1 => (width - edge_offset, rng.random_range(0.0..height)),
        2 => (rng.random_range(0.0..width), edge_offset),
        _ => (rng.random_range(0.0..width), height - edge_offset),
    }
}

struct Stain {
    lobes: Vec<Lobe>,
    outline_field: CoarseField,
    body_field: DensityField,
    body_variation_field: DensityField,
    tide: Option<TideMark>,
    structures: Vec<DensityStructure>,
    structures_enabled: bool,
    directional: Option<DirectionalDensity>,
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
    characteristic_size: f32,
    outline_strength: f32,
    feather: f32,
    shade: u8,
    alpha: f32,
    contrast_gain: f32,
}

#[derive(Clone, Copy)]
struct FieldBounds {
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
}

impl Stain {
    fn new<R: Rng + ?Sized>(
        anchor: (f32, f32),
        base_radius: f32,
        density: u8,
        density_scale: f32,
        lightness: u8,
        structures_enabled: bool,
        rng: &mut R,
    ) -> Self {
        let lobe_count = rng.random_range(4..=7);
        let branch_angle = rng.random_range(0.0..std::f32::consts::TAU);
        let mut lobes = Vec::with_capacity(lobe_count);

        for index in 0..lobe_count {
            let direction = if index == 0 {
                branch_angle
            } else {
                branch_angle + rng.random_range(-1.75..1.75)
            };
            let distance = if index == 0 {
                0.0
            } else {
                base_radius * rng.random_range(0.18..1.1)
            };
            let radius_x = base_radius * rng.random_range(0.58..1.3);
            let radius_y = base_radius * rng.random_range(0.58..1.3);
            let angle = rng.random_range(0.0..std::f32::consts::TAU);
            lobes.push(Lobe::new(
                anchor.0 + direction.cos() * distance,
                anchor.1 + direction.sin() * distance,
                radius_x,
                radius_y,
                angle,
            ));
        }

        let mut min_x = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for lobe in &lobes {
            let (extent_x, extent_y) = lobe.extents();
            min_x = min_x.min(lobe.center_x - extent_x);
            max_x = max_x.max(lobe.center_x + extent_x);
            min_y = min_y.min(lobe.center_y - extent_y);
            max_y = max_y.max(lobe.center_y + extent_y);
        }

        let characteristic_size = (max_x - min_x).max(max_y - min_y).max(base_radius * 2.0);
        let field_margin = base_radius * 0.3;
        min_x -= field_margin;
        max_x += field_margin;
        min_y -= field_margin;
        max_y += field_margin;
        let bounds = FieldBounds {
            min_x,
            max_x,
            min_y,
            max_y,
        };

        let outline_field = CoarseField::new(
            min_x,
            max_x,
            min_y,
            max_y,
            (base_radius * rng.random_range(0.24..0.42)).max(12.0),
            rng,
        );
        let body_field = DensityField::new(
            min_x,
            max_x,
            min_y,
            max_y,
            (base_radius * rng.random_range(0.15..0.28)).max(8.0),
            rng,
        );
        let outline_strength = rng.random_range(0.11..0.2);
        let feather = rng.random_range(0.07..0.15);
        let shade = stain_luma(lightness, rng.random_range(155..=235));
        let base_alpha = density_base_alpha(density);
        let alpha = (base_alpha * rng.random_range(0.90..1.10)).clamp(0.0, 255.0);
        let body_variation_field = DensityField::new(
            min_x,
            max_x,
            min_y,
            max_y,
            (base_radius * rng.random_range(0.42..0.8)).max(18.0),
            rng,
        );
        let tide = TideMark::new(bounds, base_radius, feather, density, rng);
        let structures =
            DensityStructure::new_many(&lobes, bounds, base_radius, density_scale, rng);
        let directional = if rng.random_bool(0.65) {
            Some(DirectionalDensity::new(anchor, base_radius, rng))
        } else {
            None
        };

        Self {
            outline_field,
            body_field,
            body_variation_field,
            tide,
            structures,
            structures_enabled,
            directional,
            lobes,
            min_x,
            max_x,
            min_y,
            max_y,
            characteristic_size,
            outline_strength,
            feather,
            shade,
            alpha,
            contrast_gain: 1.0,
        }
    }

    fn with_contrast_gain(mut self, contrast_gain: f32) -> Self {
        self.contrast_gain = contrast_gain;
        self
    }

    fn rasterize(&self, effects: &mut [f32], width: u32, height: u32, blur: u8) {
        if blur == 0 {
            self.rasterize_hard(effects, width, height);
        } else {
            self.rasterize_diffused(effects, width, height, blur);
        }
    }

    fn rasterize_hard(&self, effects: &mut [f32], width: u32, height: u32) {
        let Some((start_x, end_x, start_y, end_y)) = self.image_bounds(
            width, height, self.min_x, self.max_x, self.min_y, self.max_y,
        ) else {
            return;
        };

        for y in start_y..=end_y {
            for x in start_x..=end_x {
                let world_x = x as f32 + 0.5;
                let world_y = y as f32 + 0.5;
                let warped_shape = self.warped_shape_at(world_x, world_y);
                let coverage = smoothstep(-self.feather, self.feather, warped_shape);
                if coverage == 0.0 {
                    continue;
                }

                self.accumulate_coverage(
                    effects,
                    width,
                    (x, y),
                    coverage,
                    warped_shape,
                    (world_x, world_y),
                );
            }
        }
    }

    fn rasterize_diffused(&self, effects: &mut [f32], width: u32, height: u32, blur: u8) {
        let coverage_field = self.diffused_outer_coverage(blur);

        let Some((start_x, end_x, start_y, end_y)) = self.image_bounds(
            width,
            height,
            coverage_field.min_x(),
            coverage_field.max_x(),
            coverage_field.min_y(),
            coverage_field.max_y(),
        ) else {
            return;
        };

        for y in start_y..=end_y {
            for x in start_x..=end_x {
                let world_x = x as f32 + 0.5;
                let world_y = y as f32 + 0.5;
                let coverage = coverage_field.sample(world_x, world_y).clamp(0.0, 1.0);
                if coverage <= COVERAGE_TAIL_THRESHOLD {
                    continue;
                }

                let warped_shape = self.warped_shape_at(world_x, world_y);
                self.accumulate_coverage(
                    effects,
                    width,
                    (x, y),
                    coverage,
                    warped_shape,
                    (world_x, world_y),
                );
            }
        }
    }

    fn accumulate_coverage(
        &self,
        effects: &mut [f32],
        width: u32,
        pixel: (u32, u32),
        coverage: f32,
        warped_shape: f32,
        world: (f32, f32),
    ) {
        let (x, y) = pixel;
        let (world_x, world_y) = world;
        let alpha = ((self.alpha / 255.0)
            * coverage
            * self.optical_density_at(warped_shape, world_x, world_y))
        .clamp(0.0, 1.0);
        let contribution = alpha * f32::from(self.shade) / 255.0;
        let index = y as usize * width as usize + x as usize;
        effects[index] = accumulate_scalar_effect(effects[index], contribution);
    }

    fn diffused_outer_coverage(&self, blur: u8) -> ScalarField {
        let softness = softness_normalized(blur);
        let softness_pixels = self.characteristic_size * softness;
        let cell_size = (self.characteristic_size / 256.0).clamp(1.0, MAX_WORKING_CELL_SIZE);
        let padding = softness_pixels * 3.0 + cell_size * 2.0;
        let mut coverage_field = ScalarField::new(
            self.min_x - padding,
            self.max_x + padding,
            self.min_y - padding,
            self.max_y + padding,
            cell_size,
        );

        for y in 0..coverage_field.height {
            for x in 0..coverage_field.width {
                let (world_x, world_y) = coverage_field.position(x, y);
                let coverage = self.soft_outer_coverage_at(world_x, world_y, softness);
                coverage_field.set(
                    x,
                    y,
                    if coverage < COVERAGE_TAIL_THRESHOLD {
                        0.0
                    } else {
                        coverage
                    },
                );
            }
        }

        coverage_field.blur_once(finishing_blur_radius_cells(softness_pixels, cell_size));
        coverage_field
    }

    fn soft_outer_coverage_at(&self, x: f32, y: f32, softness: f32) -> f32 {
        soft_outer_coverage(self.warped_shape_at(x, y), softness)
    }

    fn image_bounds(
        &self,
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

    fn warped_shape_at(&self, x: f32, y: f32) -> f32 {
        let shape = self
            .lobes
            .iter()
            .map(|lobe| lobe.coverage(x, y))
            .fold(f32::NEG_INFINITY, f32::max);

        shape + self.outline_field.sample(x, y) * self.outline_strength
    }

    fn optical_density_at(&self, warped_shape: f32, x: f32, y: f32) -> f32 {
        contrast_adjusted_density(
            self.unadjusted_optical_density_at(warped_shape, x, y),
            self.contrast_gain,
        )
    }

    fn unadjusted_optical_density_at(&self, warped_shape: f32, x: f32, y: f32) -> f32 {
        self.chemical_density_at(warped_shape, x, y) * self.directional_density_at(x, y)
    }

    fn chemical_density_at(&self, warped_shape: f32, x: f32, y: f32) -> f32 {
        let local_density = self.local_density_at(x, y);
        // Keep TideMark construction for seeded output stability, but omit its contour band normally.
        let tide_density = if NORMAL_STAIN_TIDE_CONTRIBUTION_ENABLED {
            self.tide_contribution_at(warped_shape, x, y, local_density)
        } else {
            0.0
        };

        local_density + tide_density
    }

    fn directional_density_at(&self, x: f32, y: f32) -> f32 {
        self.directional
            .as_ref()
            .map_or(1.0, |directional| directional.multiplier_at(x, y))
    }

    fn local_density_at(&self, x: f32, y: f32) -> f32 {
        let broad_density = 0.45 + 0.35 * normalized_density_field(&self.body_field, x, y);
        let secondary_density =
            0.45 + 0.55 * normalized_density_field(&self.body_variation_field, x, y);
        let structure_density: f32 = self
            .structures
            .iter()
            .map(|structure| structure.density_at(x, y))
            .sum();
        // Diagnostic mode omits only the contribution; structure construction and sampling remain unchanged.
        let structure_density = if self.structures_enabled {
            structure_density
        } else {
            0.0
        };
        (broad_density * secondary_density + structure_density)
            .clamp(MIN_INTERNAL_DENSITY, MAX_INTERNAL_DENSITY)
    }

    fn tide_contribution_at(&self, warped_shape: f32, x: f32, y: f32, local_density: f32) -> f32 {
        let tide_density = self
            .tide
            .as_ref()
            .map_or(0.0, |tide| tide.density_at(warped_shape, x, y));

        bounded_tide_contribution(tide_density, local_density)
    }
}

struct TideMark {
    field: DensityField,
    center: f32,
    width: f32,
    strength: f32,
    presence_threshold: f32,
    second_line: Option<TideLine>,
}

struct TideLine {
    center_offset: f32,
    width_scale: f32,
    strength_scale: f32,
    presence_threshold: f32,
}

impl TideMark {
    fn new<R: Rng + ?Sized>(
        bounds: FieldBounds,
        base_radius: f32,
        feather: f32,
        density: u8,
        rng: &mut R,
    ) -> Option<Self> {
        if !rng.random_bool(tide_presence_probability(density)) {
            return None;
        }

        let second_line = if SECOND_TIDE_LINE_PROBABILITY > 0.0
            && rng.random_bool(SECOND_TIDE_LINE_PROBABILITY)
        {
            Some(TideLine {
                center_offset: rng.random_range(1.6..3.4),
                width_scale: rng.random_range(0.55..0.9),
                strength_scale: rng.random_range(0.18..0.42),
                presence_threshold: rng.random_range(0.42..0.72),
            })
        } else {
            None
        };

        Some(Self {
            field: DensityField::new(
                bounds.min_x,
                bounds.max_x,
                bounds.min_y,
                bounds.max_y,
                (base_radius * rng.random_range(0.28..0.55)).max(14.0),
                rng,
            ),
            center: feather * rng.random_range(0.65..2.0) + rng.random_range(0.0..0.045),
            width: feather * rng.random_range(1.8..3.2),
            strength: rng.random_range(0.02..0.10),
            presence_threshold: rng.random_range(0.45..0.72),
            second_line,
        })
    }

    fn density_at(&self, boundary_distance: f32, x: f32, y: f32) -> f32 {
        let variation = normalized_density_field(&self.field, x, y);
        let local_center = self.center + (variation - 0.5) * self.width * 0.8;
        let local_width = self.width * (0.85 + 0.65 * variation);
        let presence = smoothstep(self.presence_threshold, 0.96, variation);
        let mut density =
            soft_band(boundary_distance, local_center, local_width) * presence * self.strength;

        if let Some(second_line) = &self.second_line {
            let second_presence = smoothstep(second_line.presence_threshold, 0.98, variation);
            density += soft_band(
                boundary_distance,
                local_center + local_width * second_line.center_offset,
                local_width * second_line.width_scale,
            ) * second_presence
                * self.strength
                * second_line.strength_scale;
        }

        density.clamp(0.0, self.strength)
    }
}

fn bounded_tide_contribution(tide_density: f32, local_density: f32) -> f32 {
    tide_density.clamp(0.0, local_density * MAX_TIDE_RELATIVE_MODULATION)
}

struct DensityStructure {
    lobes: Vec<Lobe>,
    field: DensityField,
    outline_strength: f32,
    feather: f32,
    strength: f32,
}

impl DensityStructure {
    fn new_many<R: Rng + ?Sized>(
        parent_lobes: &[Lobe],
        bounds: FieldBounds,
        base_radius: f32,
        density_scale: f32,
        rng: &mut R,
    ) -> Vec<Self> {
        let mut count = if rng.random_bool(0.30) { 0 } else { 1 };
        if count == 1 && density_scale >= 0.70 && rng.random_bool(0.20) {
            count += 1;
        }

        (0..count)
            .map(|_| Self::new(parent_lobes, bounds, base_radius, rng))
            .collect()
    }

    fn new<R: Rng + ?Sized>(
        parent_lobes: &[Lobe],
        bounds: FieldBounds,
        base_radius: f32,
        rng: &mut R,
    ) -> Self {
        let parent = parent_lobes[rng.random_range(0..parent_lobes.len())];
        let structure_radius = base_radius * rng.random_range(0.42..0.95);
        let branch_angle = rng.random_range(0.0..std::f32::consts::TAU);
        let lobe_count = rng.random_range(2..=4);
        let mut lobes = Vec::with_capacity(lobe_count);

        for index in 0..lobe_count {
            let direction = if index == 0 {
                branch_angle
            } else {
                branch_angle + rng.random_range(-1.5..1.5)
            };
            let distance = if index == 0 {
                0.0
            } else {
                structure_radius * rng.random_range(0.12..0.82)
            };
            lobes.push(Lobe::new(
                parent.center_x
                    + rng.random_range(-base_radius * 0.3..base_radius * 0.3)
                    + direction.cos() * distance,
                parent.center_y
                    + rng.random_range(-base_radius * 0.3..base_radius * 0.3)
                    + direction.sin() * distance,
                structure_radius * rng.random_range(0.5..1.2),
                structure_radius * rng.random_range(0.5..1.2),
                rng.random_range(0.0..std::f32::consts::TAU),
            ));
        }

        Self {
            lobes,
            field: DensityField::new(
                bounds.min_x,
                bounds.max_x,
                bounds.min_y,
                bounds.max_y,
                (structure_radius * rng.random_range(0.45..0.78)).max(12.0),
                rng,
            ),
            outline_strength: rng.random_range(0.02..0.07),
            feather: rng.random_range(0.28..0.55),
            strength: rng.random_range(0.03..0.14),
        }
    }

    fn density_at(&self, x: f32, y: f32) -> f32 {
        let shape = self
            .lobes
            .iter()
            .map(|lobe| lobe.coverage(x, y))
            .fold(f32::NEG_INFINITY, f32::max);
        let field_value = self.field.sample(x, y);
        let warped_shape = shape + field_value * self.outline_strength;
        let coverage = smoothstep(-self.feather, self.feather, warped_shape);

        self.strength * coverage * (0.45 + 0.55 * normalized_density_value(field_value))
    }
}

struct DirectionalDensity {
    anchor: (f32, f32),
    direction: (f32, f32),
    span: f32,
    strength: f32,
}

impl DirectionalDensity {
    fn new<R: Rng + ?Sized>(anchor: (f32, f32), base_radius: f32, rng: &mut R) -> Self {
        let angle = rng.random_range(0.0..std::f32::consts::TAU);

        Self {
            anchor,
            direction: (angle.cos(), angle.sin()),
            span: base_radius * 2.5,
            strength: rng.random_range(0.08..0.18),
        }
    }

    fn multiplier_at(&self, x: f32, y: f32) -> f32 {
        let projection = ((x - self.anchor.0) * self.direction.0
            + (y - self.anchor.1) * self.direction.1)
            / self.span;

        1.0 + self.strength * projection.clamp(-1.0, 1.0)
    }
}

#[derive(Clone, Copy)]
struct Lobe {
    center_x: f32,
    center_y: f32,
    radius_x: f32,
    radius_y: f32,
    sin_angle: f32,
    cos_angle: f32,
}

impl Lobe {
    fn new(center_x: f32, center_y: f32, radius_x: f32, radius_y: f32, angle: f32) -> Self {
        let (sin_angle, cos_angle) = angle.sin_cos();

        Self {
            center_x,
            center_y,
            radius_x,
            radius_y,
            sin_angle,
            cos_angle,
        }
    }

    fn coverage(&self, x: f32, y: f32) -> f32 {
        let dx = x - self.center_x;
        let dy = y - self.center_y;
        let local_x = dx * self.cos_angle + dy * self.sin_angle;
        let local_y = -dx * self.sin_angle + dy * self.cos_angle;
        let normalized_x = local_x / self.radius_x;
        let normalized_y = local_y / self.radius_y;

        1.0 - (normalized_x * normalized_x + normalized_y * normalized_y).sqrt()
    }

    fn extents(&self) -> (f32, f32) {
        (
            (self.radius_x * self.cos_angle).hypot(self.radius_y * self.sin_angle),
            (self.radius_x * self.sin_angle).hypot(self.radius_y * self.cos_angle),
        )
    }
}

struct ScalarField {
    values: Vec<f32>,
    width: usize,
    height: usize,
    origin_x: f32,
    origin_y: f32,
    cell_size: f32,
}

impl ScalarField {
    fn new(min_x: f32, max_x: f32, min_y: f32, max_y: f32, cell_size: f32) -> Self {
        let origin_x = (min_x / cell_size).floor() * cell_size;
        let origin_y = (min_y / cell_size).floor() * cell_size;
        let end_x = (max_x / cell_size).ceil() * cell_size;
        let end_y = (max_y / cell_size).ceil() * cell_size;
        let width = (((end_x - origin_x) / cell_size).round() as usize + 1).max(2);
        let height = (((end_y - origin_y) / cell_size).round() as usize + 1).max(2);

        Self {
            values: vec![0.0; width * height],
            width,
            height,
            origin_x,
            origin_y,
            cell_size,
        }
    }

    fn position(&self, x: usize, y: usize) -> (f32, f32) {
        (
            self.origin_x + x as f32 * self.cell_size,
            self.origin_y + y as f32 * self.cell_size,
        )
    }

    fn set(&mut self, x: usize, y: usize, value: f32) {
        self.values[y * self.width + x] = value;
    }

    fn min_x(&self) -> f32 {
        self.origin_x
    }

    fn max_x(&self) -> f32 {
        self.origin_x + (self.width - 1) as f32 * self.cell_size
    }

    fn min_y(&self) -> f32 {
        self.origin_y
    }

    fn max_y(&self) -> f32 {
        self.origin_y + (self.height - 1) as f32 * self.cell_size
    }

    fn sample(&self, x: f32, y: f32) -> f32 {
        let grid_x = (x - self.origin_x) / self.cell_size;
        let grid_y = (y - self.origin_y) / self.cell_size;
        let x0 = grid_x.floor() as isize;
        let y0 = grid_y.floor() as isize;
        let horizontal = grid_x - x0 as f32;
        let vertical = grid_y - y0 as f32;
        let top = lerp(
            self.value_or_zero(x0, y0),
            self.value_or_zero(x0 + 1, y0),
            horizontal,
        );
        let bottom = lerp(
            self.value_or_zero(x0, y0 + 1),
            self.value_or_zero(x0 + 1, y0 + 1),
            horizontal,
        );

        lerp(top, bottom, vertical)
    }

    fn blur_once(&mut self, radius: usize) {
        self.blur_horizontally(radius);
        self.blur_vertically(radius);
    }

    fn blur_horizontally(&mut self, radius: usize) {
        let mut output = vec![0.0; self.values.len()];
        let width = radius * 2 + 1;
        let divisor = width as f32;

        for y in 0..self.height {
            let row_start = y * self.width;
            let mut sum: f32 = self.values[row_start..row_start + radius.min(self.width - 1) + 1]
                .iter()
                .sum();

            for x in 0..self.width {
                output[row_start + x] = sum / divisor;
                if x >= radius {
                    sum -= self.values[row_start + x - radius];
                }
                if x + radius + 1 < self.width {
                    sum += self.values[row_start + x + radius + 1];
                }
            }
        }

        self.values = output;
    }

    fn blur_vertically(&mut self, radius: usize) {
        let mut output = vec![0.0; self.values.len()];
        let width = radius * 2 + 1;
        let divisor = width as f32;

        for x in 0..self.width {
            let mut sum: f32 = (0..=radius.min(self.height - 1))
                .map(|y| self.values[y * self.width + x])
                .sum();

            for y in 0..self.height {
                output[y * self.width + x] = sum / divisor;
                if y >= radius {
                    sum -= self.values[(y - radius) * self.width + x];
                }
                if y + radius + 1 < self.height {
                    sum += self.values[(y + radius + 1) * self.width + x];
                }
            }
        }

        self.values = output;
    }

    fn value_or_zero(&self, x: isize, y: isize) -> f32 {
        if x < 0 || y < 0 || x >= self.width as isize || y >= self.height as isize {
            return 0.0;
        }

        self.values[y as usize * self.width + x as usize]
    }
}

fn density_base_alpha(density: u8) -> f32 {
    let density = f32::from(density);
    let (start, end) = DENSITY_ALPHA_ANCHORS
        .windows(2)
        .find(|segment| density <= f32::from(segment[1].0))
        .map(|segment| (segment[0], segment[1]))
        .unwrap_or((DENSITY_ALPHA_ANCHORS[6], DENSITY_ALPHA_ANCHORS[7]));

    lerp(
        start.1,
        end.1,
        (density - f32::from(start.0)) / f32::from(end.0 - start.0),
    )
}

fn tide_presence_probability(density: u8) -> f64 {
    let density = f64::from(density);
    let (start, end) = TIDE_PRESENCE_ANCHORS
        .windows(2)
        .find(|segment| density <= f64::from(segment[1].0))
        .map(|segment| (segment[0], segment[1]))
        .unwrap_or((TIDE_PRESENCE_ANCHORS[4], TIDE_PRESENCE_ANCHORS[5]));

    (start.1 + (end.1 - start.1) * (density - f64::from(start.0)) / f64::from(end.0 - start.0))
        .clamp(0.0, 1.0)
}

fn lightness_luma(lightness: u8) -> f32 {
    let lightness = f32::from(lightness);
    let (start, end) = LIGHTNESS_ANCHORS
        .windows(2)
        .find(|segment| lightness <= f32::from(segment[1].0))
        .map(|segment| (segment[0], segment[1]))
        .unwrap_or((LIGHTNESS_ANCHORS[5], LIGHTNESS_ANCHORS[6]));

    lerp(
        start.1,
        end.1,
        (lightness - f32::from(start.0)) / f32::from(end.0 - start.0),
    )
}

fn stain_luma(lightness: u8, shade_sample: u8) -> u8 {
    let luma_variation = (f32::from(shade_sample) - 195.0) * 0.1;

    (lightness_luma(lightness) + luma_variation)
        .round()
        .clamp(0.0, 255.0) as u8
}

// A compact control lattice gives each stain an irregular outer outline.
struct CoarseField {
    values: Vec<f32>,
    width: usize,
    height: usize,
    origin_x: f32,
    origin_y: f32,
    cell_size: f32,
}

impl CoarseField {
    fn new<R: Rng + ?Sized>(
        min_x: f32,
        max_x: f32,
        min_y: f32,
        max_y: f32,
        cell_size: f32,
        rng: &mut R,
    ) -> Self {
        let width = (((max_x - min_x) / cell_size).ceil() as usize + 3).max(2);
        let height = (((max_y - min_y) / cell_size).ceil() as usize + 3).max(2);
        let values = (0..width * height)
            .map(|_| rng.random_range(-1.0..1.0))
            .collect();

        Self {
            values,
            width,
            height,
            origin_x: min_x - cell_size,
            origin_y: min_y - cell_size,
            cell_size,
        }
    }

    fn sample(&self, x: f32, y: f32) -> f32 {
        let grid_x = ((x - self.origin_x) / self.cell_size).clamp(0.0, (self.width - 1) as f32);
        let grid_y = ((y - self.origin_y) / self.cell_size).clamp(0.0, (self.height - 1) as f32);
        let x0 = grid_x.floor() as usize;
        let y0 = grid_y.floor() as usize;
        let x1 = (x0 + 1).min(self.width - 1);
        let y1 = (y0 + 1).min(self.height - 1);
        let horizontal = smoothstep(0.0, 1.0, grid_x - x0 as f32);
        let vertical = smoothstep(0.0, 1.0, grid_y - y0 as f32);
        let top = lerp(self.value(x0, y0), self.value(x1, y0), horizontal);
        let bottom = lerp(self.value(x0, y1), self.value(x1, y1), horizontal);

        lerp(top, bottom, vertical)
    }

    fn value(&self, x: usize, y: usize) -> f32 {
        self.values[y * self.width + x]
    }
}

// Density is evaluated at final-pixel coordinates with a cubic kernel so the
// control lattice remains broad without exposing its rectangular cells.
struct DensityField {
    control: CoarseField,
}

impl DensityField {
    fn new<R: Rng + ?Sized>(
        min_x: f32,
        max_x: f32,
        min_y: f32,
        max_y: f32,
        cell_size: f32,
        rng: &mut R,
    ) -> Self {
        Self {
            control: CoarseField::new(min_x, max_x, min_y, max_y, cell_size, rng),
        }
    }

    fn sample(&self, x: f32, y: f32) -> f32 {
        let grid_x = ((x - self.control.origin_x) / self.control.cell_size)
            .clamp(0.0, (self.control.width - 1) as f32);
        let grid_y = ((y - self.control.origin_y) / self.control.cell_size)
            .clamp(0.0, (self.control.height - 1) as f32);
        let x0 = grid_x.floor() as isize;
        let y0 = grid_y.floor() as isize;
        let horizontal_weights = cubic_b_spline_weights(grid_x - x0 as f32);
        let vertical_weights = cubic_b_spline_weights(grid_y - y0 as f32);
        let mut value = 0.0;

        for (row_offset, vertical_weight) in vertical_weights.iter().copied().enumerate() {
            let row = y0 + row_offset as isize - 1;
            let mut row_value = 0.0;
            for (column_offset, horizontal_weight) in horizontal_weights.iter().copied().enumerate()
            {
                let column = x0 + column_offset as isize - 1;
                row_value += self.control_value(column, row) * horizontal_weight;
            }
            value += row_value * vertical_weight;
        }

        value
    }

    fn control_value(&self, x: isize, y: isize) -> f32 {
        let x = x.clamp(0, self.control.width as isize - 1) as usize;
        let y = y.clamp(0, self.control.height as isize - 1) as usize;

        self.control.value(x, y)
    }
}

fn cubic_b_spline_weights(progress: f32) -> [f32; 4] {
    let inverse = 1.0 - progress;
    let squared = progress * progress;
    let cubed = squared * progress;

    [
        inverse * inverse * inverse / 6.0,
        (4.0 - 6.0 * squared + 3.0 * cubed) / 6.0,
        (1.0 + 3.0 * progress + 3.0 * squared - 3.0 * cubed) / 6.0,
        cubed / 6.0,
    ]
}

fn normalized_density_field(field: &DensityField, x: f32, y: f32) -> f32 {
    normalized_density_value(field.sample(x, y))
}

fn normalized_density_value(value: f32) -> f32 {
    smoothstep(-1.0, 1.0, value)
}

fn softness_normalized(blur: u8) -> f32 {
    debug_assert!(blur > 0);

    (f32::from(blur) / 100.0 * MAX_SOFTNESS_FRACTION).max(MIN_SOFTNESS_NORMALIZED)
}

fn finishing_blur_radius_cells(softness_pixels: f32, cell_size: f32) -> usize {
    (softness_pixels / cell_size * 0.10).round().clamp(1.0, 6.0) as usize
}

fn soft_outer_coverage(warped_shape: f32, softness: f32) -> f32 {
    if warped_shape >= 0.0 {
        return 1.0;
    }

    let distance = -warped_shape / softness;
    (-(distance * distance)).exp()
}

fn contrast_gain(contrast: u8) -> f32 {
    2.0_f32.powf((f32::from(contrast) - f32::from(DEFAULT_CONTRAST)) / 50.0)
}

fn contrast_adjusted_density(optical_density: f32, gain: f32) -> f32 {
    if gain == 1.0 {
        return optical_density;
    }

    let normalized_density = ((optical_density - MIN_INTERNAL_DENSITY)
        / (MAX_INTERNAL_DENSITY - MIN_INTERNAL_DENSITY))
        .clamp(0.0, 1.0);
    let adjusted = 0.5 + (normalized_density - 0.5) * gain;

    MIN_INTERNAL_DENSITY + adjusted.clamp(0.0, 1.0) * (MAX_INTERNAL_DENSITY - MIN_INTERNAL_DENSITY)
}

fn soft_band(value: f32, center: f32, width: f32) -> f32 {
    let distance = (value - center).abs();

    1.0 - smoothstep(width * 0.15, width, distance)
}

fn smoothstep(edge_start: f32, edge_end: f32, value: f32) -> f32 {
    let progress = ((value - edge_start) / (edge_end - edge_start)).clamp(0.0, 1.0);
    progress * progress * (3.0 - 2.0 * progress)
}

fn lerp(start: f32, end: f32, progress: f32) -> f32 {
    start + (end - start) * progress
}

#[cfg(test)]
#[path = "stain_ut.rs"]
mod stain_ut;
