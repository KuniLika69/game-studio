use crate::{
    scene::{Part, Shape},
    world::{Collider, InteractionState, World},
};
use eframe::egui::{self, Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};

const KEY_MARKER_TAG: u64 = 1 << 60;
const KEY_GOLD: Color32 = Color32::from_rgb(255, 207, 103);
const MARKER_BACK: Color32 = Color32::from_rgb(36, 39, 43);

pub fn key_marker_id(id: u32) -> u64 {
    KEY_MARKER_TAG | u64::from(id)
}

pub fn marker_key_id(id: u64) -> Option<u32> {
    u32::try_from(id ^ KEY_MARKER_TAG).ok()
}

#[derive(Clone, Copy)]
pub struct Plan {
    pub rect: Rect,
    pub focus: Vec2,
    pub scale: f32,
}

impl Plan {
    pub fn doorway(self, painter: &Painter, doorway: Collider) {
        let [x, _, z] = doorway.min;
        let [right, _, front] = doorway.max;
        let shape = Rect::from_two_pos(self.point(x, z), self.point(right, front));
        painter.rect_filled(
            shape,
            2.0,
            Color32::from_rgba_unmultiplied(134, 226, 180, 140),
        );
    }

    pub fn keys(
        self,
        painter: &Painter,
        world: &World,
        session: Option<&InteractionState>,
        selected: Option<u64>,
    ) {
        for key in &world.keys {
            if session.is_some_and(|session| session.has_key(key.id)) {
                continue;
            }
            let [x, _, z] = key.position;
            let point = self.point(x, z);
            if self.rect.expand(12.0).contains(point) {
                let selected = selected == Some(key_marker_id(key.id));
                key_symbol(
                    painter,
                    point,
                    selected,
                    selected.then_some(key.name.as_str()),
                );
            }
        }
    }

    pub fn locked_doors(
        self,
        painter: &Painter,
        world: &World,
        session: Option<&InteractionState>,
    ) {
        for door in world.buildings.iter().flat_map(|building| building.doors()) {
            if door.settings.required_key.is_none()
                || session.is_some_and(|session| !session.locked(&door))
            {
                continue;
            }
            let [x, _, z] = door.interaction_position();
            let point = self.point(x, z);
            if self.rect.expand(10.0).contains(point) {
                locked_symbol(painter, point);
            }
        }
    }

    pub fn point(self, x: f32, z: f32) -> Pos2 {
        self.rect.center() + (egui::vec2(x, z) - self.focus) * self.scale
    }

    pub fn coordinates(self, point: Pos2) -> Vec2 {
        (point - self.rect.center()) / self.scale + self.focus
    }

    fn area(self, x: f32, z: f32, w: f32, d: f32) -> Rect {
        Rect::from_center_size(self.point(x, z), egui::vec2(w.abs(), d.abs()) * self.scale)
    }

    pub fn terrain(self, painter: &Painter, streets: &[[f32; 2]], editing: bool) {
        painter.rect_filled(
            self.rect,
            0.0,
            if streets.is_empty() {
                Color32::from_rgb(40, 56, 58)
            } else {
                Color32::from_rgb(151, 148, 137)
            },
        );
        let min = self.coordinates(self.rect.left_top()).floor();
        let max = self.coordinates(self.rect.right_bottom()).ceil();
        for &[center_x, center_z] in streets {
            let road = self.area(center_x, center_z, 7.5, 36.0);
            if !self.rect.intersects(road.expand(self.scale * 1.35)) {
                continue;
            }
            painter.rect_filled(road, 0.0, Color32::from_rgb(55, 60, 64));
            for side in [-1.0_f32, 1.0] {
                let sidewalk = self.area(center_x + side * 4.33, center_z, 1.15, 36.0);
                painter.rect_filled(sidewalk, 0.0, Color32::from_rgb(174, 168, 154));
                painter.rect_filled(
                    self.area(center_x + side * 3.66, center_z, 0.19, 36.0),
                    0.0,
                    Color32::from_rgb(44, 49, 53),
                );
                let joint = Stroke::new(1.0_f32, Color32::from_black_alpha(30));
                for tile in -14..=14 {
                    let z = center_z + tile as f32 * 1.24;
                    let y = self.point(center_x, z).y;
                    if self.rect.y_range().contains(y) {
                        painter.hline(sidewalk.x_range(), y, joint);
                    }
                }
            }
            for x in [-3.75, 3.75] {
                painter.rect_filled(
                    self.area(center_x + x, center_z, 0.17, 36.0),
                    0.0,
                    Color32::from_rgb(200, 192, 176),
                );
            }
            for z in -7..=7 {
                painter.rect_filled(
                    self.area(center_x, center_z + z as f32 * 2.5, 0.09, 1.1),
                    0.0,
                    Color32::from_rgb(215, 189, 130),
                );
            }
            for x in -6..=6 {
                painter.rect_filled(
                    self.area(center_x + x as f32 * 0.48, center_z - 3.0, 0.22, 1.45),
                    0.0,
                    Color32::from_rgb(216, 212, 198),
                );
            }
        }
        if editing {
            let stroke = Stroke::new(1.0_f32, Color32::from_black_alpha(24));
            for x in (min.x as i32)..=(max.x as i32) {
                painter.vline(self.point(x as f32, 0.0).x, self.rect.y_range(), stroke);
            }
            for z in (min.y as i32)..=(max.y as i32) {
                painter.hline(self.rect.x_range(), self.point(0.0, z as f32).y, stroke);
            }
        }
    }

    pub fn objects(
        self,
        painter: &Painter,
        mut parts: Vec<Part>,
        selected: Option<u64>,
        furniture: Option<u32>,
    ) {
        parts.sort_by(|a, b| a.position[1].total_cmp(&b.position[1]));
        for part in parts {
            let [x, _, z] = part.position;
            let footprint = || {
                let half_x = part.size[0] * 0.5;
                let half_z = part.size[2] * 0.5;
                let (sine, cosine) = part.rotation[1].to_radians().sin_cos();
                [
                    (-half_x, -half_z),
                    (half_x, -half_z),
                    (half_x, half_z),
                    (-half_x, half_z),
                ]
                .into_iter()
                .map(|(dx, dz)| {
                    self.point(x + dx * cosine + dz * sine, z - dx * sine + dz * cosine)
                })
                .collect::<Vec<_>>()
            };
            let corners =
                (part.shape == Shape::Box && part.rotation[1].abs() > 0.01).then(footprint);
            let area = corners.as_ref().map_or_else(
                || self.area(x, z, part.size[0], part.size[2]),
                |points| Rect::from_points(points),
            );
            if !self.rect.intersects(area) {
                continue;
            }
            let color = rgb(part.color);
            let outline = Stroke::new(1.0_f32, color.gamma_multiply(0.64));
            match part.shape {
                Shape::Box => {
                    if let Some(corners) = corners {
                        painter.add(egui::Shape::convex_polygon(corners, color, outline));
                    } else {
                        painter.rect(area, 1.0, color, outline, StrokeKind::Inside);
                    }
                }
                Shape::Sphere | Shape::Cylinder => {
                    painter.add(egui::Shape::ellipse_filled(
                        area.center(),
                        area.size() * 0.5,
                        color,
                    ));
                }
            }
            if selected.is_some_and(|id| {
                id == part.id || crate::buildings::building_id(part.id) == Some(id)
            }) {
                let outline =
                    if furniture.is_some() && crate::interior::item_id(part.id) == furniture {
                        Color32::from_rgb(255, 204, 113)
                    } else {
                        Color32::from_rgb(107, 230, 199)
                    };
                painter.rect_stroke(
                    area,
                    1.0,
                    Stroke::new(1.4_f32, outline),
                    StrokeKind::Outside,
                );
            }
        }
    }

    pub fn actor(
        self,
        painter: &Painter,
        texture: Option<egui::TextureId>,
        [x, height, z]: [f32; 3],
        model_height: Option<f32>,
    ) {
        let foot = self.point(x, z);
        painter.add(egui::Shape::ellipse_filled(
            foot,
            egui::vec2(0.44, 0.26) * self.scale,
            Color32::from_black_alpha(70),
        ));
        if let Some(texture) = texture {
            let edge = model_height
                .map_or(3.05, |height| height * crate::sprite::SIDE as f32 / 73.0)
                * self.scale;
            let top = foot
                - egui::vec2(
                    edge * 0.5,
                    edge * crate::sprite::FLOOR / crate::sprite::SIDE as f32 + height * self.scale,
                );
            painter.image(
                texture,
                Rect::from_min_size(top, Vec2::splat(edge)),
                Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }
}

pub fn key_symbol(painter: &Painter, point: Pos2, selected: bool, label: Option<&str>) {
    let outline = if selected {
        Color32::from_rgb(117, 241, 204)
    } else {
        KEY_GOLD.gamma_multiply(0.72)
    };
    painter.circle(
        point,
        10.0,
        MARKER_BACK,
        Stroke::new(if selected { 1.7_f32 } else { 1.0 }, outline),
    );
    let stroke = Stroke::new(1.6_f32, KEY_GOLD);
    painter.circle_stroke(point + egui::vec2(-3.0, -2.0), 2.7, stroke);
    let start = point + egui::vec2(-0.8, 0.0);
    let tip = point + egui::vec2(5.3, 5.0);
    painter.line_segment([start, tip], stroke);
    painter.line_segment([tip, tip + egui::vec2(1.5, -2.0)], stroke);
    let tooth = start.lerp(tip, 0.63);
    painter.line_segment([tooth, tooth + egui::vec2(1.5, -2.0)], stroke);
    if let Some(label) = label.filter(|label| !label.trim().is_empty()) {
        let galley = painter.layout_no_wrap(label.to_owned(), FontId::proportional(12.0), KEY_GOLD);
        let text_rect =
            Align2::LEFT_CENTER.anchor_size(point + egui::vec2(14.0, 0.0), galley.size());
        painter.rect_filled(text_rect.expand2(egui::vec2(4.0, 2.0)), 3.0, MARKER_BACK);
        painter.galley(text_rect.min, galley, KEY_GOLD);
    }
}

fn locked_symbol(painter: &Painter, point: Pos2) {
    painter.circle_filled(point, 9.0, MARKER_BACK);
    painter.circle_stroke(
        point + egui::vec2(0.0, -2.1),
        2.9,
        Stroke::new(1.25_f32, KEY_GOLD),
    );
    let body = Rect::from_center_size(point + egui::vec2(0.0, 2.0), egui::vec2(8.5, 5.4));
    painter.rect_filled(body, 1.25, KEY_GOLD);
    painter.circle_filled(point + egui::vec2(0.0, 2.0), 0.85, MARKER_BACK);
}

fn rgb(value: [f32; 3]) -> Color32 {
    let [r, g, b] = value.map(|x| (x.clamp(0.0, 1.0) * 255.0) as u8);
    Color32::from_rgb(r, g, b)
}
