//! Water projection for the 2D plan and tile maps.
use eframe::egui::{self, Color32, Pos2, Stroke};
use glam::{Vec2, Vec3};

use crate::environment::{Splash, WaterSurface};

#[derive(Clone, Copy)]
pub enum Projection {
    Xz,
    Xy { z: f32 },
}

fn clip_waterline(edge: Vec<[f32; 2]>, surface: WaterSurface) -> Vec<[f32; 2]> {
    let Some(shore) = surface.shore else {
        return edge;
    };
    let gradient = shore.gradient();
    let origin = Vec2::from_array(shore.origin);
    let signed_depth = |point: Vec2| surface.height + gradient.dot(point - origin);
    let mut wet = Vec::with_capacity(edge.len() + 2);
    let mut previous = Vec2::from_array(*edge.last().expect("water outline has vertices"));
    let mut previous_depth = signed_depth(previous);
    for next in edge.into_iter().map(Vec2::from_array) {
        let next_depth = signed_depth(next);
        if (previous_depth > 0.0) != (next_depth > 0.0) {
            wet.push(
                previous.lerp(next, previous_depth / (previous_depth - next_depth)).to_array(),
            );
        }
        if next_depth > 0.0 {
            wet.push(next.to_array());
        }
        previous = next;
        previous_depth = next_depth;
    }
    wet
}

pub fn paint_water(
    painter: &egui::Painter,
    surfaces: impl IntoIterator<Item = WaterSurface>,
    projection: Projection,
    seconds: f64,
    project: impl Fn(Vec3) -> Pos2,
) {
    for surface in surfaces {
        let [x, z] = surface.center;
        let [wide, deep] = surface.size;
        let [x0, x1] = [x - wide * 0.5, x + wide * 0.5];
        let [z0, z1] = [z - deep * 0.5, z + deep * 0.5];
        let depth = surface.height - surface.floor_y;
        if depth <= 0.001 {
            continue;
        }
        let alpha = (72.0 + depth.min(1.2) * 76.0) as u8;
        let [r, g, b] = surface
            .tint
            .map(|value| (value.clamp(0.0, 1.0) * 255.0) as u8);
        let fill = Color32::from_rgba_unmultiplied(r, g, b, alpha);
        let ripple = Color32::from_rgba_unmultiplied(
            r.saturating_add(90),
            g.saturating_add(90),
            b.saturating_add(90),
            150,
        );
        match projection {
            Projection::Xz => {
                let edge = if surface.rounded_edge {
                    (0..32)
                        .map(|index| {
                            let angle = index as f32 * std::f32::consts::TAU / 32.0;
                            [x + angle.cos() * wide * 0.5, z + angle.sin() * deep * 0.5]
                        })
                        .collect()
                } else {
                    vec![[x0, z0], [x1, z0], [x1, z1], [x0, z1]]
                };
                let edge = clip_waterline(edge, surface);
                if edge.len() < 3 {
                    continue;
                }
                painter.add(egui::Shape::convex_polygon(
                    edge.into_iter()
                        .map(|[x, z]| project(Vec3::new(x, surface.height, z)))
                        .collect(),
                    fill,
                    Stroke::new(1.2_f32, ripple),
                ));
                let direction = glam::Vec2::from_array(surface.current);
                let phase =
                    (seconds * f64::from(0.18 + direction.length() * 0.15)).rem_euclid(1.0) as f32;
                for line in 0..6 {
                    let row = ((line as f32 + phase) / 6.0).clamp(0.0, 1.0);
                    let across = z0 + deep * (0.08 + row * 0.84);
                    let Some([left, right]) = surface.span_at_z(across) else {
                        continue;
                    };
                    let row_half = (right - left) * 0.5;
                    let start = left + row_half * (0.12 + (line % 3) as f32 * 0.38);
                    let end = (start + row_half * 0.54).min(right - row_half * 0.07);
                    let points = (0..=6)
                        .map(|index| {
                            let x = start + (end - start) * index as f32 / 6.0;
                            let oscillation = surface.wave_at(x, across, seconds) - surface.height;
                            let z = (across + oscillation).clamp(z0, z1);
                            project(Vec3::new(x, surface.height, z))
                        })
                        .collect();
                    painter.add(egui::Shape::line(points, Stroke::new(1.2_f32, ripple)));
                }
            }
            Projection::Xy { z } if (z0..=z1).contains(&z) => {
                let mut mesh = egui::Mesh::default();
                let mut rim = Vec::with_capacity(25);
                let Some([left, right]) = surface.span_at_z(z) else {
                    continue;
                };
                for column in 0..=24 {
                    let x = left + (right - left) * column as f32 / 24.0;
                    let floor = surface.floor_at(Vec2::new(x, z));
                    let y = surface.wave_at(x, z, seconds).max(floor);
                    let surface_pos = project(Vec3::new(x, y, z));
                    rim.push(surface_pos);
                    mesh.colored_vertex(surface_pos, fill);
                    mesh.colored_vertex(project(Vec3::new(x, floor, z)), fill);
                    if column > 0 {
                        let left = (column - 1) * 2;
                        let right = column * 2;
                        mesh.add_triangle(left, left + 1, right);
                        mesh.add_triangle(right, left + 1, right + 1);
                    }
                }
                painter.add(egui::Shape::mesh(mesh));
                painter.add(egui::Shape::line(rim, Stroke::new(1.7_f32, ripple)));
            }
            Projection::Xy { .. } => {}
        }
    }
}

pub fn paint_splash(
    painter: &egui::Painter,
    splash: Splash,
    age: f32,
    projection: Projection,
    project: impl Fn(Vec3) -> Pos2,
) {
    if !(0.0..0.8).contains(&age) {
        return;
    }
    let shade = Color32::from_rgba_unmultiplied(175, 235, 248, (200.0 * (1.0 - age / 0.8)) as u8);
    let volume = 0.25 + splash.strength * 0.55;
    match projection {
        Projection::Xz => {
            let size = (0.12 + age * 1.4) * volume;
            let ring = (0..=24)
                .map(|index| {
                    let angle = index as f32 * std::f32::consts::TAU / 24.0;
                    project(
                        splash.position + Vec3::new(angle.cos() * size, 0.0, angle.sin() * size),
                    )
                })
                .collect();
            painter.add(egui::Shape::line(ring, Stroke::new(1.5_f32, shade)));
        }
        Projection::Xy { .. } => {
            for index in 0..8 {
                let spread = index as f32 / 7.0 * 2.0 - 1.0;
                let y = (age * (2.4 - spread.abs() * 1.4) - age * age * 3.0).max(0.0) * volume;
                let point = splash.position + Vec3::new(spread * age * volume * 2.2, y, 0.0);
                painter.circle_filled(project(point), 1.4, shade);
            }
        }
    }
}

pub fn paint_frame(
    painter: &egui::Painter,
    frame: &crate::environment::Frame,
    projection: Projection,
    project: impl Fn(Vec3) -> Pos2,
) {
    paint_water(
        painter,
        frame.water.iter().copied(),
        projection,
        frame.elapsed,
        &project,
    );
    for &(splash, age) in &frame.splashes {
        paint_splash(painter, splash, age, projection, &project);
    }
}

pub fn paint_atmosphere(painter: &egui::Painter, frame: &crate::environment::Frame) {
    let rect = painter.clip_rect();
    if !rect.is_finite() || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let air = frame.atmosphere;
    let shade = (1.0 - air.sun_strength).clamp(0.0, 1.0);
    if shade > 0.05 {
        let [r, g, b] = air
            .sky_horizon
            .to_array()
            .map(|value| (value.clamp(0.0, 1.0) * 120.0) as u8);
        painter.rect_filled(
            rect,
            0.0,
            Color32::from_rgba_unmultiplied(r, g, b, (shade * 86.0) as u8),
        );
    }
    let fog_alpha = ((1.0 - (-air.fog_density * 12.0).exp()) * 151.0).round() as u8;
    if fog_alpha > 3 {
        let [r, g, b] = air
            .fog_color
            .to_array()
            .map(|value| (value.clamp(0.0, 1.0).sqrt() * 230.0) as u8);
        painter.rect_filled(
            rect,
            0.0,
            Color32::from_rgba_unmultiplied(r, g, b, fog_alpha),
        );
    }
    let rain = air.rain.clamp(0.0, 1.0);
    let count = (96.0 * rain).round() as u32;
    let time = frame.elapsed.rem_euclid(8192.0) as f32;
    let slope = (frame.wind.x * 0.42).clamp(-7.0, 7.0);
    let color = Color32::from_rgba_unmultiplied(183, 221, 239, (rain.sqrt() * 105.0) as u8);
    for seed in 0..count {
        let seed = seed as f32;
        let u = (seed * 0.618_034 + time * slope * 0.003).rem_euclid(1.0);
        let v = (seed * 0.403_975 + time * (0.39 + (seed % 5.0) * 0.032)).rem_euclid(1.0);
        let point = rect.min + egui::vec2(u * rect.width(), v * rect.height());
        painter.line_segment(
            [point, point + egui::vec2(slope, 8.0 + seed % 7.0)],
            Stroke::new(1.0_f32, color),
        );
    }
    let snow = air.snow.clamp(0.0, 1.0);
    let flakes = (124.0 * snow).round() as u32;
    for seed in 0..flakes {
        let seed = seed as f32;
        let depth = (seed * 0.37).fract();
        let drift = frame.wind.x.clamp(-9.0, 9.0) * time * (0.002 + depth * 0.0025);
        let u = (seed * 0.618_034 + drift + (time * 1.37 + seed).sin() * 0.015).rem_euclid(1.0);
        let v = (seed * 0.413_217 + time * (0.052 + depth * 0.069)).rem_euclid(1.0);
        let point = rect.min + egui::vec2(u * rect.width(), v * rect.height());
        let color = Color32::from_rgba_unmultiplied(231, 243, 255, (snow * (92.0 + depth * 141.0)) as u8);
        painter.circle_filled(point, 0.85 + depth * 1.65, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::{Environment, Frame, WeatherCue, WeatherPreset};

    #[test]
    fn snowfall_paints_only_during_snow_and_moves_inside_the_2d_view() {
        let ctx = egui::Context::default();
        let view = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(400.0, 260.0));
        let mut environment = Environment::default();
        environment.weather.cues.push(WeatherCue {
            id: 1,
            start_seconds: 1.0,
            preset: WeatherPreset::Snow,
            blend_seconds: 2.0,
        });
        let snow_at = |elapsed| {
            let frame = Frame {
                elapsed,
                atmosphere: environment.sample(elapsed),
                water: Vec::new(),
                wind: Vec3::new(6.0, 0.0, 1.0),
                splashes: Vec::new(),
            };
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("2D weather"));
                paint_atmosphere(&ctx.layer_painter(layer).with_clip_rect(view), &frame);
            });
            output.shapes.into_iter().filter_map(|shape| match shape.shape {
                egui::Shape::Circle(flake) => {
                    assert_eq!(shape.clip_rect, view);
                    assert!(view.contains(flake.center));
                    Some(flake.center)
                }
                _ => None,
            }).collect::<Vec<_>>()
        };
        assert!(snow_at(0.5).is_empty());
        assert_eq!(snow_at(2.0).len(), 62);
        let full = snow_at(3.1);
        assert_eq!(full.len(), 124);
        let later = snow_at(3.7);
        assert_eq!(later.len(), full.len());
        assert!(later[0].distance(full[0]) > 4.0);
    }
}
