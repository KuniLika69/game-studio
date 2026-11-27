//! Projects the same particle samples into an orthographic view.
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use glam::Vec3;

use crate::effects::{Kind, Particle};

#[derive(Clone, Copy)]
pub enum Style {
    Soft,
    Pixel,
}

pub fn paint(
    painter: &egui::Painter,
    particles: &[Particle],
    style: Style,
    scale: f32,
    project: impl Fn(Vec3) -> Pos2,
) {
    for particle in particles {
        let center = project(particle.position);
        let radius = (particle.radius * scale).clamp(0.65, 45.0);
        if !center.is_finite()
            || !painter
                .clip_rect()
                .expand(radius * 1.5 + 8.0)
                .contains(center)
        {
            continue;
        }
        let [r, g, b] = particle
            .color
            .map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8);
        let shade = |fraction: f32| {
            Color32::from_rgba_unmultiplied(
                r,
                g,
                b,
                (particle.opacity.clamp(0.0, 1.0) * fraction * 255.0).round() as u8,
            )
        };
        if particle.kind == Kind::Rain {
            let tail = project(particle.position - particle.velocity * 0.034);
            let raw = center - tail;
            let streak = if raw.is_finite() && raw.length_sq() >= 4.0 {
                raw.normalized() * raw.length().clamp(2.0, 17.0)
            } else {
                egui::vec2(-1.8, 5.9 + radius)
            };
            painter.line_segment([center - streak, center], Stroke::new(1.25_f32, shade(1.0)));
            continue;
        }
        match style {
            Style::Soft => match particle.kind {
                Kind::Smoke | Kind::Dust => {
                    painter.circle_filled(center, radius, shade(0.40));
                    painter.circle_filled(center, radius * 0.69, shade(0.42));
                    painter.circle_filled(center, radius * 0.44, shade(0.56));
                }
                _ => {
                    painter.circle_filled(center, radius, shade(0.98));
                    painter.circle_filled(
                        center + egui::vec2(-0.21, -0.24) * radius,
                        radius * 0.37,
                        Color32::WHITE.gamma_multiply(particle.opacity * 0.48),
                    );
                }
            },
            Style::Pixel => {
                let block = (scale * 0.025).clamp(1.6, 4.0);
                let center = egui::pos2(
                    (center.x / block).round() * block,
                    (center.y / block).round() * block,
                );
                let radius = (radius / block).round().max(1.0) * block;
                match particle.kind {
                    Kind::Smoke | Kind::Dust => {
                        let extent = Vec2::splat(radius * 1.5);
                        painter.rect_filled(
                            Rect::from_center_size(center, extent),
                            0.0,
                            shade(0.72),
                        );
                        painter.rect_filled(
                            Rect::from_center_size(
                                center + egui::vec2(-radius * 0.55, radius * 0.27),
                                extent * 0.45,
                            ),
                            0.0,
                            shade(0.65),
                        );
                    }
                    _ => {
                        painter.rect_filled(
                            Rect::from_center_size(center, Vec2::splat(radius)),
                            0.0,
                            shade(1.0),
                        );
                    }
                }
            }
        }
    }
}
