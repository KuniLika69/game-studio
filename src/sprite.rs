use crate::scene::{Part, Shape, SpritePixel};
use glam::{EulerRot, Mat3, Quat, Vec3};
use std::{fs::File, io::BufWriter, path::Path};

pub const SIDE: usize = 96;
pub const FLOOR: f32 = 82.0;
pub const DIRECTIONS: [&str; 4] = ["Спереди", "Справа", "Сзади", "Слева"];

pub fn scale_for(parts: &[Part]) -> f32 {
    let mut height: f32 = 2.5;
    let mut width: f32 = 1.5;
    for part in parts {
        let radius = Vec3::from_array(part.size).length() * 0.5;
        height = height.max(part.position[1] + radius);
        width = width.max(part.position[0].abs().max(part.position[2].abs()) + radius);
    }
    (74.0 / height).min(43.0 / width).min(31.0)
}

pub fn rasterize(
    parts: &[Part],
    direction: u8,
    pixels: &[SpritePixel],
    scale: f32,
    glow: f32,
    frame: Option<u8>,
) -> Vec<u8> {
    let mut image = vec![0; SIDE * SIDE * 4];
    let mut depth = vec![f32::NEG_INFINITY; SIDE * SIDE];
    let camera = Mat3::from_rotation_y(f32::from(direction) * std::f32::consts::FRAC_PI_2);
    let light = Vec3::new(-0.5, 0.85, 0.75).normalize();
    for part in parts {
        let [x, y, z] = part.rotation.map(f32::to_radians);
        let rotation = Mat3::from_quat(Quat::from_euler(EulerRot::XYZ, x, y, z));
        let size = Vec3::from_array(part.size).max(Vec3::splat(0.001));
        let position = Vec3::from_array(part.position);
        let in_view = camera.transpose() * position;
        let local = Mat3::from_diagonal(size.recip()) * rotation.transpose();
        let view_to_local = local * camera;
        let offset = -local * position;
        let ray = view_to_local * -Vec3::Z;
        let radius = size.length() * 0.5;
        let ray_start = in_view.z + radius + 1.0;
        let range = |center: f32| {
            let radius = radius * scale + 1.0;
            (center - radius).max(0.0) as usize..((center + radius).ceil() as usize).min(SIDE)
        };
        let xs = range(SIDE as f32 * 0.5 + in_view.x * scale);
        let ys = range(FLOOR - in_view.y * scale);
        let normal_to_view = view_to_local.transpose();
        for py in ys {
            let y = (FLOOR - py as f32 - 0.5) / scale;
            for px in xs.clone() {
                let x = (px as f32 + 0.5 - SIDE as f32 * 0.5) / scale;
                let origin = view_to_local * Vec3::new(x, y, ray_start) + offset;
                let Some((distance, normal)) = intersect(part.shape, origin, ray) else {
                    continue;
                };
                let index = py * SIDE + px;
                let surface_depth = ray_start - distance;
                if surface_depth <= depth[index] {
                    continue;
                }
                depth[index] = surface_depth;
                let diffuse = (normal_to_view * normal)
                    .normalize_or_zero()
                    .dot(light)
                    .max(0.0);
                let brightness = 0.64 + 0.36 * diffuse + glow * 0.32;
                let rgb = part.color.map(|channel| {
                    (channel.mul_add(brightness, glow * 0.16).clamp(0.0, 1.0) * 255.0) as u8
                });
                image[4 * index..4 * index + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
    }
    for dot in pixels
        .iter()
        .filter(|dot| dot.direction == direction && dot.baked && dot.frame == frame)
        .chain(
            pixels
                .iter()
                .filter(|dot| dot.direction == direction && !dot.baked && dot.frame.is_none()),
        )
        .chain(pixels.iter().filter(|dot| {
            dot.direction == direction && !dot.baked && frame.is_some() && dot.frame == frame
        }))
    {
        let x = usize::from(dot.x);
        let y = usize::from(dot.y);
        if x < SIDE && y < SIDE {
            let start = (y * SIDE + x) * 4;
            let base = &mut image[start..start + 4];
            if base[3] == 0 {
                base.copy_from_slice(&dot.color);
            } else {
                let alpha = u16::from(dot.color[3]);
                for (base_channel, overlay) in base[..3].iter_mut().zip(dot.color) {
                    let blended =
                        u16::from(*base_channel) * (255 - alpha) + u16::from(overlay) * alpha;
                    *base_channel = ((blended + 127) / 255) as u8;
                }
            }
        }
    }
    image
}

fn intersect(shape: Shape, origin: Vec3, ray: Vec3) -> Option<(f32, Vec3)> {
    match shape {
        Shape::Box => {
            let mut near = 0.0;
            let mut far = f32::INFINITY;
            let mut normal = Vec3::ZERO;
            for axis in 0..3 {
                if ray[axis].abs() < 1e-6 {
                    if origin[axis].abs() > 0.5 {
                        return None;
                    }
                    continue;
                }
                let a = (-0.5 - origin[axis]) / ray[axis];
                let b = (0.5 - origin[axis]) / ray[axis];
                if a.min(b) > near {
                    near = a.min(b);
                    normal = Vec3::ZERO;
                    normal[axis] = -ray[axis].signum();
                }
                far = far.min(a.max(b));
                if near > far {
                    return None;
                }
            }
            Some((near, normal))
        }
        Shape::Sphere => {
            let distance = quadratic(
                ray.length_squared(),
                ray.dot(origin),
                origin.length_squared() - 0.25,
            )?;
            Some((distance, origin + ray * distance))
        }
        Shape::Cylinder => {
            let horizontal = Vec3::new(ray.x, 0.0, ray.z);
            let start = Vec3::new(origin.x, 0.0, origin.z);
            let mut hit = quadratic(
                horizontal.length_squared(),
                horizontal.dot(start),
                start.length_squared() - 0.25,
            )
            .filter(|&distance| (origin.y + ray.y * distance).abs() <= 0.5)
            .map(|distance| (distance, start + horizontal * distance));
            if ray.y.abs() > 1e-6 {
                let sign = -ray.y.signum();
                let distance = (sign * 0.5 - origin.y) / ray.y;
                if distance >= 0.0
                    && (start + horizontal * distance).length_squared() <= 0.25
                    && hit.is_none_or(|(side, _)| distance < side)
                {
                    hit = Some((distance, sign * Vec3::Y));
                }
            }
            hit
        }
    }
}

fn quadratic(a: f32, half_b: f32, c: f32) -> Option<f32> {
    let discriminant = half_b * half_b - a * c;
    if a < 1e-8 || discriminant < 0.0 {
        return None;
    }
    let near = (-half_b - discriminant.sqrt()) / a;
    (near >= 0.0).then_some(near)
}

pub fn write_atlas(
    path: &Path,
    rows: usize,
    columns: usize,
    mut frame: impl FnMut(usize, usize) -> Vec<u8>,
) -> Result<(), String> {
    let width = columns * SIDE;
    let height = rows * SIDE;
    let mut image = vec![0; width * height * 4];
    for row in 0..rows {
        for column in 0..columns {
            let tile = frame(row, column);
            for (y, source) in tile.chunks_exact(SIDE * 4).enumerate() {
                let start = ((row * SIDE + y) * width + column * SIDE) * 4;
                image[start..start + SIDE * 4].copy_from_slice(source);
            }
        }
    }
    let file = File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&image).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Scene;

    #[test]
    fn painted_frames_keep_their_own_pixels() {
        let pixel = |frame, color| SpritePixel {
            direction: 0,
            x: 4,
            y: 9,
            color,
            frame,
            baked: false,
        };
        let pixels = [
            pixel(None, [10, 20, 30, 255]),
            pixel(Some(0), [200, 0, 0, 255]),
            pixel(Some(1), [0, 200, 0, 255]),
        ];
        let offset = (9 * SIDE + 4) * 4;
        let first = rasterize(&[], 0, &pixels, 25.0, 0.0, Some(0));
        let second = rasterize(&[], 0, &pixels, 25.0, 0.0, Some(1));
        let shared = rasterize(&[], 0, &pixels, 25.0, 0.0, None);
        assert_eq!(&first[offset..offset + 4], &[200, 0, 0, 255]);
        assert_eq!(&second[offset..offset + 4], &[0, 200, 0, 255]);
        assert_eq!(&shared[offset..offset + 4], &[10, 20, 30, 255]);
    }

    #[test]
    fn sample_robot_rasterizes_in_each_direction() {
        let scene = Scene::default();
        for direction in 0..4 {
            let sprite = rasterize(
                &scene.parts,
                direction,
                &[],
                scale_for(&scene.parts),
                0.0,
                None,
            );
            let visible = sprite.chunks_exact(4).filter(|pixel| pixel[3] != 0).count();
            assert!(visible > 100 && visible < SIDE * SIDE);
        }
    }
}
