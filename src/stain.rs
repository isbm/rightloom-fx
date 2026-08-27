use image::RgbaImage;
use rand::Rng;

use crate::render::{RenderError, RenderSettings, blend_gray_pixel, write_images};

#[derive(Debug, Clone)]
pub struct StainSettings {
    pub render: RenderSettings,
}

pub fn generate_images(settings: &StainSettings) -> Result<(), RenderError> {
    let mut rng = rand::rng();
    generate_images_with_rng(settings, &mut rng)
}

fn generate_images_with_rng<R: Rng + ?Sized>(
    settings: &StainSettings,
    rng: &mut R,
) -> Result<(), RenderError> {
    write_images(&settings.render, "stain", || render_image(settings, rng))
}

fn render_image<R: Rng + ?Sized>(settings: &StainSettings, rng: &mut R) -> RgbaImage {
    let width = settings.render.resolution.width();
    let height = settings.render.resolution.height();
    let mut image = RgbaImage::new(width, height);

    if settings.render.density == 0 {
        return image;
    }

    let density = settings.render.density;
    let density_scale = density as f32 / 100.0;
    let smallest_dimension = width.min(height) as f32;
    let mut anchors = Vec::new();

    for _ in 0..stain_count(density, rng) {
        let base_radius = smallest_dimension
            * (0.04 + 0.12 * density_scale.sqrt())
            * rng.random_range(0.78..1.18);
        let anchor = choose_anchor(width, height, base_radius, &anchors, density_scale, rng);
        let stain = Stain::new(anchor, base_radius, density_scale, rng);
        stain.rasterize(&mut image);
        anchors.push(anchor);
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
    interior_field: CoarseField,
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
    outline_strength: f32,
    feather: f32,
    shade: u8,
    alpha: f32,
}

impl Stain {
    fn new<R: Rng + ?Sized>(
        anchor: (f32, f32),
        base_radius: f32,
        density_scale: f32,
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
            lobes.push(Lobe {
                center_x: anchor.0 + direction.cos() * distance,
                center_y: anchor.1 + direction.sin() * distance,
                radius_x,
                radius_y,
                angle,
            });
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

        let field_margin = base_radius * 0.3;
        min_x -= field_margin;
        max_x += field_margin;
        min_y -= field_margin;
        max_y += field_margin;

        Self {
            outline_field: CoarseField::new(
                min_x,
                max_x,
                min_y,
                max_y,
                (base_radius * rng.random_range(0.24..0.42)).max(12.0),
                rng,
            ),
            interior_field: CoarseField::new(
                min_x,
                max_x,
                min_y,
                max_y,
                (base_radius * rng.random_range(0.15..0.28)).max(8.0),
                rng,
            ),
            lobes,
            min_x,
            max_x,
            min_y,
            max_y,
            outline_strength: rng.random_range(0.11..0.2),
            feather: rng.random_range(0.07..0.15),
            shade: rng.random_range(155..=235),
            alpha: (5.0 + 108.0 * density_scale.powf(0.72)) * rng.random_range(0.72..1.12),
        }
    }

    fn rasterize(&self, image: &mut RgbaImage) {
        let start_x = self.min_x.floor().max(0.0) as u32;
        let end_x = self
            .max_x
            .ceil()
            .min(image.width().saturating_sub(1) as f32) as u32;
        let start_y = self.min_y.floor().max(0.0) as u32;
        let end_y = self
            .max_y
            .ceil()
            .min(image.height().saturating_sub(1) as f32) as u32;

        if start_x > end_x || start_y > end_y {
            return;
        }

        for y in start_y..=end_y {
            for x in start_x..=end_x {
                let x = x as f32 + 0.5;
                let y = y as f32 + 0.5;
                let shape = self
                    .lobes
                    .iter()
                    .map(|lobe| lobe.coverage(x, y))
                    .fold(f32::NEG_INFINITY, f32::max);
                let warped_shape = shape + self.outline_field.sample(x, y) * self.outline_strength;
                let coverage = smoothstep(-self.feather, self.feather, warped_shape);
                if coverage == 0.0 {
                    continue;
                }

                let mottle = 0.35 + 0.65 * smoothstep(-1.0, 1.0, self.interior_field.sample(x, y));
                let alpha = (self.alpha * coverage * mottle).round().clamp(1.0, 255.0) as u8;
                blend_gray_pixel(image, x as u32, y as u32, self.shade, alpha);
            }
        }
    }
}

struct Lobe {
    center_x: f32,
    center_y: f32,
    radius_x: f32,
    radius_y: f32,
    angle: f32,
}

impl Lobe {
    fn coverage(&self, x: f32, y: f32) -> f32 {
        let dx = x - self.center_x;
        let dy = y - self.center_y;
        let (sin, cos) = self.angle.sin_cos();
        let local_x = dx * cos + dy * sin;
        let local_y = -dx * sin + dy * cos;
        let normalized_x = local_x / self.radius_x;
        let normalized_y = local_y / self.radius_y;

        1.0 - (normalized_x * normalized_x + normalized_y * normalized_y).sqrt()
    }

    fn extents(&self) -> (f32, f32) {
        let (sin, cos) = self.angle.sin_cos();
        (
            (self.radius_x * cos).hypot(self.radius_y * sin),
            (self.radius_x * sin).hypot(self.radius_y * cos),
        )
    }
}

// A compact smooth field gives each stain a unique outline and uneven density.
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

fn smoothstep(edge_start: f32, edge_end: f32, value: f32) -> f32 {
    let progress = ((value - edge_start) / (edge_end - edge_start)).clamp(0.0, 1.0);
    progress * progress * (3.0 - 2.0 * progress)
}

fn lerp(start: f32, end: f32, progress: f32) -> f32 {
    start + (end - start) * progress
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{StainSettings, render_image};
    use crate::render::{RenderSettings, Resolution};
    use rand::{SeedableRng, rngs::StdRng};

    fn settings(density: u8) -> StainSettings {
        StainSettings {
            render: RenderSettings {
                resolution: Resolution::new(640, 400).expect("test resolution should be valid"),
                density,
                amount: 1,
                outdir: "unused".into(),
            },
        }
    }

    #[test]
    fn rendered_image_has_requested_dimensions() {
        let mut rng = StdRng::seed_from_u64(10);
        let image = render_image(&settings(30), &mut rng);

        assert_eq!(image.dimensions(), (640, 400));
    }

    #[test]
    fn zero_density_is_transparent() {
        let mut rng = StdRng::seed_from_u64(11);
        let image = render_image(&settings(0), &mut rng);

        assert!(image.pixels().all(|pixel| pixel[3] == 0));
    }

    #[test]
    fn nonzero_density_modifies_monochrome_pixels() {
        let mut rng = StdRng::seed_from_u64(12);
        let image = render_image(&settings(30), &mut rng);

        assert!(image.pixels().any(|pixel| pixel[3] > 0));
        assert!(
            image
                .pixels()
                .filter(|pixel| pixel[3] > 0)
                .all(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2])
        );
    }

    #[test]
    fn generated_alpha_contains_variation() {
        let mut rng = StdRng::seed_from_u64(13);
        let image = render_image(&settings(45), &mut rng);
        let alphas: BTreeSet<_> = image
            .pixels()
            .filter(|pixel| pixel[3] > 0)
            .map(|pixel| pixel[3])
            .collect();

        assert!(alphas.len() > 4, "stains should have varied opacity");
    }

    #[test]
    fn low_density_keeps_most_of_the_canvas_transparent() {
        let mut rng = StdRng::seed_from_u64(14);
        let image = render_image(&settings(5), &mut rng);
        let transparent = image.pixels().filter(|pixel| pixel[3] == 0).count();

        assert!(transparent * 100 / image.pixels().len() >= 70);
    }
}
