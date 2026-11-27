use eframe::egui::{self, Align2, Color32, FontId, Key, Pos2, Rect, Stroke, Vec2};
use glam::{EulerRot, Mat4, Quat, Vec3};

use crate::{
    assets::{Library, Placement},
    buildings::Building,
    viewport3d::Camera,
    world::{Collider, World, WorldObject},
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Move,
    Rotate,
    Scale,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
    Z,
    Uniform,
}

impl Axis {
    const DIRECTIONS: [Self; 3] = [Self::X, Self::Y, Self::Z];

    fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
            Self::Uniform => 3,
        }
    }

    fn unit(self) -> Vec3 {
        match self {
            Self::X => Vec3::X,
            Self::Y => Vec3::Y,
            Self::Z => Vec3::Z,
            Self::Uniform => Vec3::ONE.normalize(),
        }
    }

    fn color(self) -> Color32 {
        match self {
            Self::X => Color32::from_rgb(246, 108, 107),
            Self::Y => Color32::from_rgb(133, 228, 126),
            Self::Z => Color32::from_rgb(122, 176, 255),
            Self::Uniform => Color32::from_rgb(236, 241, 247),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
            Self::Uniform => "",
        }
    }
}

pub struct Editor {
    selection: Vec<u64>,
    mode: Mode,
    snap: bool,
    grid: f32,
    drag: Option<Drag>,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            selection: Vec::new(),
            mode: Mode::Move,
            snap: true,
            grid: 0.5,
            drag: None,
        }
    }
}

#[derive(Default)]
pub struct Change {
    pub changed: bool,
    pub owns_pointer: bool,
    pub status: Option<String>,
}

pub(crate) fn extend_selection(input: &egui::InputState) -> bool {
    input
        .events
        .iter()
        .rev()
        .find_map(|event| match event {
            egui::Event::PointerButton {
                button: egui::PointerButton::Primary,
                modifiers,
                ..
            } => Some(modifiers.ctrl || modifiers.command),
            _ => None,
        })
        .unwrap_or(input.modifiers.ctrl || input.modifiers.command)
}

impl Editor {
    pub fn clear(&mut self) {
        self.selection.clear();
        self.drag = None;
    }

    pub fn end_drag(&mut self) {
        self.drag = None;
    }

    pub fn selection(&self) -> &[u64] {
        &self.selection
    }

    pub(crate) fn select(&mut self, hit: Option<u64>, extend: bool, lead: &mut Option<u64>) {
        if extend {
            if let Some(id) = hit {
                if self.selection.contains(&id) {
                    self.selection.retain(|&entry| entry != id);
                } else {
                    self.selection.push(id);
                }
            }
        } else {
            self.selection = hit.into_iter().collect();
        }
        *lead = self.selection.last().copied();
    }

    fn sync(&mut self, world: &World, lead: Option<u64>) {
        self.selection
            .retain(|&id| entity_position(world, id).is_some());
        if self.selection.last().copied() != lead {
            self.clear();
            if let Some(id) = lead.filter(|&id| entity_position(world, id).is_some()) {
                self.selection.push(id);
            }
        }
    }

    pub fn toolbar(
        &mut self,
        ui: &mut egui::Ui,
        world: &mut World,
        lead: &mut Option<u64>,
    ) -> Change {
        self.sync(world, *lead);
        let mut result = Change::default();
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.mode, Mode::Move, "Переместить")
                .on_hover_text("Перемещение вдоль мировой оси · G");
            ui.selectable_value(&mut self.mode, Mode::Rotate, "Вращать")
                .on_hover_text("Вращение · R. Дома поворачиваются вокруг Y с шагом 90°.");
            ui.selectable_value(&mut self.mode, Mode::Scale, "Масштабировать")
                .on_hover_text(
                    "Локальные оси или белый квадрат для пропорционального масштаба · S",
                );
            ui.checkbox(&mut self.snap, "Привязка")
                .on_hover_text("Координаты по сетке, углы по 15°, масштаб по 10%.");
            ui.add_enabled(
                self.snap,
                egui::DragValue::new(&mut self.grid)
                    .range(0.05..=20.0)
                    .speed(0.025)
                    .suffix(" м"),
            );
            let button = ui
                .add_enabled(!self.selection.is_empty(), egui::Button::new("Дублировать"))
                .on_hover_text("Ctrl+D · сдвинуть копии на одну клетку");
            let shortcut = !ui.ctx().wants_keyboard_input()
                && ui.input(|input| input.modifiers.command && input.key_pressed(Key::D));
            if (button.clicked() || shortcut) && !self.selection.is_empty() {
                match world.duplicate_selection(&self.selection, [self.grid, 0.0, self.grid]) {
                    Ok(copies) if !copies.is_empty() => {
                        result.status = Some(format!("Создано копий: {}", copies.len()));
                        result.changed = true;
                        self.selection = copies;
                        *lead = self.selection.last().copied();
                    }
                    Ok(_) => {}
                    Err(error) => result.status = Some(error),
                }
            }
            if self.selection.len() > 1 {
                ui.weak(format!("Выбрано: {}", self.selection.len()));
            }
        });
        if self.drag.is_none() && !ui.ctx().wants_keyboard_input() {
            ui.input(|input| {
                if input.modifiers.is_none() {
                    for (key, mode) in [
                        (Key::G, Mode::Move),
                        (Key::R, Mode::Rotate),
                        (Key::S, Mode::Scale),
                    ] {
                        if input.key_pressed(key) {
                            self.mode = mode;
                        }
                    }
                }
            });
        }
        result
    }

    pub fn interact(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        camera: Camera,
        world: &mut World,
        assets: &Library,
        lead: &mut Option<u64>,
    ) -> Change {
        self.sync(world, *lead);
        let projection = Projection::new(camera, response.rect);
        let (pointer, pressed, held, released, control, escape) = ui.input(|input| {
            let button = egui::PointerButton::Primary;
            (
                input.pointer.interact_pos(),
                input.pointer.button_pressed(button),
                input.pointer.button_down(button),
                input.pointer.button_released(button),
                extend_selection(input),
                input.key_pressed(Key::Escape),
            )
        });
        if pressed
            && !control
            && response.hovered()
            && let Some(pointer) = pointer
            && let Some(layout) = self.layout(world, projection)
            && let Some(handle) = layout.hit(pointer)
        {
            self.drag = Some(Drag::start(
                self.mode,
                self.snap,
                self.grid,
                layout,
                handle,
                pointer,
                projection,
                originals(world, &self.selection),
            ));
        }
        if let Some(drag) = &mut self.drag {
            let mut result = Change {
                owns_pointer: true,
                ..Change::default()
            };
            if escape && !drag.cancelled {
                drag.cancelled = true;
                result.changed |= apply(world, &drag.originals, Transform::Identity);
            } else if !drag.cancelled
                && let Some(pointer) = pointer
            {
                result.changed |= drag.update(world, projection, pointer);
            }
            if !held || released {
                self.drag = None;
            } else {
                ui.ctx().request_repaint();
            }
            return result;
        }
        if response.clicked_by(egui::PointerButton::Primary)
            && let Some(point) = pointer
        {
            let hit = projection
                .ray(point)
                .and_then(|ray| pick(world, assets, ray));
            self.select(hit, control, lead);
        }
        Change::default()
    }

    pub fn paint(
        &self,
        ui: &egui::Ui,
        rect: Rect,
        camera: Camera,
        world: &World,
        assets: &Library,
    ) {
        let projection = Projection::new(camera, rect);
        let painter = ui.painter().with_clip_rect(rect);
        for id in self.selection.iter().copied() {
            if let Some(bounds) = editor_bounds(world, assets, id) {
                paint_bounds(&painter, projection, bounds);
            }
        }
        let Some(layout) = self.layout(world, projection) else {
            return;
        };
        let hovered = ui
            .input(|input| input.pointer.hover_pos())
            .filter(|point| rect.contains(*point))
            .and_then(|point| layout.hit(point))
            .map(|index| layout.handles[index].axis);
        let active = self
            .drag
            .as_ref()
            .filter(|drag| !drag.cancelled)
            .map(|drag| drag.axis);
        for handle in &layout.handles {
            let color = if Some(handle.axis) == active || Some(handle.axis) == hovered {
                handle.axis.color().lerp_to_gamma(Color32::WHITE, 0.42)
            } else {
                handle.axis.color()
            };
            let stroke = Stroke::new(2.6_f32, color);
            for pair in handle.points.windows(2) {
                if let [Some(a), Some(b)] = pair {
                    painter.line_segment(
                        [*a, *b],
                        Stroke::new(4.9_f32, Color32::from_black_alpha(188)),
                    );
                    painter.line_segment([*a, *b], stroke);
                }
            }
            if let Some(point) = handle.tip() {
                if self.mode == Mode::Rotate {
                    // A colored knot makes the front of a narrow ring easier to grab.
                    painter.circle_filled(point, 3.0, color);
                } else if self.mode == Mode::Scale {
                    painter.rect_filled(
                        Rect::from_center_size(point, Vec2::splat(12.0)),
                        2.0,
                        Color32::from_gray(20),
                    );
                    painter.rect_filled(
                        Rect::from_center_size(point, Vec2::splat(8.0)),
                        1.0,
                        color,
                    );
                } else {
                    let outward = handle.drag_direction;
                    let across = egui::vec2(-outward.y, outward.x);
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            point + outward * 4.0,
                            point - outward * 10.0 + across * 5.5,
                            point - outward * 10.0 - across * 5.5,
                        ],
                        color,
                        Stroke::new(1.2_f32, Color32::from_gray(21)),
                    ));
                }
                if handle.axis != Axis::Uniform && self.mode != Mode::Rotate {
                    painter.text(
                        point + handle.drag_direction * 13.0,
                        Align2::CENTER_CENTER,
                        handle.axis.label(),
                        FontId::proportional(12.0),
                        color,
                    );
                }
            }
        }
    }

    pub fn axis_handle(
        &self,
        world: &World,
        camera: Camera,
        rect: Rect,
        axis: usize,
    ) -> Option<(Pos2, Vec2)> {
        let layout = self.layout(world, Projection::new(camera, rect))?;
        let handle = layout
            .handles
            .iter()
            .find(|handle| handle.axis.index() == axis)?;
        Some((handle.tip()?, handle.drag_direction))
    }

    fn layout(&self, world: &World, projection: Projection) -> Option<Layout> {
        let pivot = pivot(world, &self.selection)?;
        let (center, depth) = projection.point(pivot)?;
        let radius = depth * 2.0 * 20.0_f32.to_radians().tan() * (60.0 / projection.rect.height());
        let building = self
            .selection
            .iter()
            .any(|id| world.buildings.iter().any(|house| house.id == *id));
        let yaw_only = building
            || self
                .selection
                .iter()
                .any(|id| world.objects.iter().any(|object| object.id == *id));
        let orientation = if self.mode == Mode::Scale && self.selection.len() == 1 {
            entity_rotation(world, self.selection[0])
        } else {
            Quat::IDENTITY
        };
        let mut handles = Vec::with_capacity(4);
        match self.mode {
            Mode::Rotate => {
                for axis in Axis::DIRECTIONS
                    .into_iter()
                    .filter(|axis| !yaw_only || *axis == Axis::Y)
                {
                    let unit = axis.unit();
                    let side = match axis {
                        Axis::X => Vec3::Y,
                        Axis::Y => Vec3::Z,
                        _ => Vec3::X,
                    };
                    let up = unit.cross(side);
                    let points = (0..=40)
                        .map(|step| {
                            let angle = std::f32::consts::TAU * step as f32 / 40.0;
                            projection
                                .point(pivot + radius * (side * angle.cos() + up * angle.sin()))
                                .map(|(point, _)| point)
                        })
                        .collect();
                    let tip_angle = std::f32::consts::TAU * 3.0 / 40.0;
                    let tip = projection
                        .point(pivot + radius * (side * tip_angle.cos() + up * tip_angle.sin()))
                        .map(|(point, _)| point);
                    let next = projection
                        .point(
                            pivot
                                + radius
                                    * (side * (tip_angle + 0.13).cos()
                                        + up * (tip_angle + 0.13).sin()),
                        )
                        .map(|(point, _)| point);
                    let drag_direction =
                        tip.zip(next).map_or(Vec2::X, |(a, b)| (b - a).normalized());
                    handles.push(Handle {
                        axis,
                        unit,
                        points,
                        tip,
                        drag_direction,
                        length: 60.0,
                    });
                }
            }
            Mode::Move | Mode::Scale => {
                if self.mode == Mode::Move || self.selection.len() == 1 {
                    for axis in Axis::DIRECTIONS {
                        let unit = orientation * axis.unit();
                        let Some((tip, _)) = projection.point(pivot + unit * radius) else {
                            continue;
                        };
                        let delta = tip - center;
                        if delta.length() < 12.0 {
                            continue;
                        }
                        let direction = delta.normalized();
                        handles.push(Handle {
                            axis,
                            unit,
                            points: vec![Some(center + direction * 10.0), Some(tip)],
                            tip: Some(tip),
                            drag_direction: direction,
                            length: delta.length(),
                        });
                    }
                }
                if self.mode == Mode::Scale {
                    handles.push(Handle {
                        axis: Axis::Uniform,
                        unit: Axis::Uniform.unit(),
                        points: Vec::new(),
                        tip: Some(center),
                        drag_direction: egui::vec2(0.707, -0.707),
                        length: 60.0,
                    });
                }
            }
        }
        Some(Layout {
            pivot,
            radius,
            building,
            handles,
        })
    }
}

struct Layout {
    pivot: Vec3,
    radius: f32,
    building: bool,
    handles: Vec<Handle>,
}

impl Layout {
    fn hit(&self, point: Pos2) -> Option<usize> {
        self.handles
            .iter()
            .enumerate()
            .filter_map(|(index, handle)| {
                let minimum = handle
                    .points
                    .windows(2)
                    .filter_map(|pair| match pair {
                        [Some(a), Some(b)] => Some(segment_distance(point, *a, *b)),
                        _ => None,
                    })
                    .fold(f32::INFINITY, f32::min);
                let at_tip = handle
                    .tip()
                    .map_or(f32::INFINITY, |tip| tip.distance(point));
                let distance = minimum.min(at_tip);
                (distance <= 8.0).then_some((index, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(index, _)| index)
    }
}

struct Handle {
    axis: Axis,
    unit: Vec3,
    points: Vec<Option<Pos2>>,
    tip: Option<Pos2>,
    drag_direction: Vec2,
    length: f32,
}

impl Handle {
    fn tip(&self) -> Option<Pos2> {
        self.tip
    }
}

fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let edge = b - a;
    let parameter = ((p - a).dot(edge) / edge.length_sq().max(0.001)).clamp(0.0, 1.0);
    p.distance(a + edge * parameter)
}

struct Drag {
    mode: Mode,
    axis: Axis,
    unit: Vec3,
    pivot: Vec3,
    start: Pos2,
    direction: Vec2,
    meters_per_pixel: f32,
    handle_length: f32,
    snap: bool,
    grid: f32,
    building: bool,
    rotation: f32,
    radial: Option<Vec3>,
    originals: Vec<Original>,
    started: bool,
    cancelled: bool,
}

impl Drag {
    fn start(
        mode: Mode,
        snap: bool,
        grid: f32,
        layout: Layout,
        index: usize,
        start: Pos2,
        projection: Projection,
        originals: Vec<Original>,
    ) -> Self {
        let handle = &layout.handles[index];
        let radial = if mode == Mode::Rotate {
            projection
                .ray(start)
                .and_then(|ray| ray.radial(layout.pivot, handle.unit))
        } else {
            None
        };
        Self {
            mode,
            axis: handle.axis,
            unit: handle.unit,
            pivot: layout.pivot,
            start,
            direction: handle.drag_direction,
            meters_per_pixel: layout.radius / handle.length.max(1.0),
            handle_length: handle.length,
            snap,
            grid,
            building: layout.building,
            rotation: 0.0,
            radial,
            originals,
            started: false,
            cancelled: false,
        }
    }

    fn update(&mut self, world: &mut World, projection: Projection, point: Pos2) -> bool {
        let delta = point - self.start;
        if !self.started && delta.length_sq() < 9.0 {
            return false;
        }
        self.started = true;
        let pixels = delta.dot(self.direction);
        let transform = match self.mode {
            Mode::Move => {
                let distance = pixels * self.meters_per_pixel;
                let distance = if self.snap {
                    let basis = self.pivot.dot(self.unit);
                    snapped(basis + distance, self.grid) - basis
                } else {
                    distance
                };
                Transform::Move(self.unit * distance)
            }
            Mode::Scale => {
                let factor = 1.0 + pixels / self.handle_length;
                let factor = if self.snap {
                    snapped(factor, 0.1)
                } else {
                    factor
                }
                .clamp(0.05, 20.0);
                Transform::Scale {
                    axis: self.axis,
                    factor,
                    pivot: self.pivot,
                }
            }
            Mode::Rotate => {
                if let (Some(old), Some(new)) = (
                    self.radial,
                    projection
                        .ray(point)
                        .and_then(|ray| ray.radial(self.pivot, self.unit)),
                ) {
                    self.rotation += self.unit.dot(old.cross(new)).atan2(old.dot(new));
                    self.radial = Some(new);
                } else if self.radial.is_none() {
                    self.rotation = pixels * 0.019;
                }
                let degrees = self.rotation.to_degrees();
                let degrees = if self.building {
                    snapped(degrees, 90.0)
                } else if self.snap {
                    snapped(degrees, 15.0)
                } else {
                    degrees
                };
                Transform::Rotate {
                    axis: self.unit,
                    degrees,
                    pivot: self.pivot,
                }
            }
        };
        apply(world, &self.originals, transform)
    }
}

fn snapped(value: f32, step: f32) -> f32 {
    (value / step).round() * step
}

#[derive(Clone, Copy)]
enum Transform {
    Identity,
    Move(Vec3),
    Scale {
        axis: Axis,
        factor: f32,
        pivot: Vec3,
    },
    Rotate {
        axis: Vec3,
        degrees: f32,
        pivot: Vec3,
    },
}

impl Transform {
    fn position(self, original: [f32; 3], limit: f32, floor: f32) -> [f32; 3] {
        let point = Vec3::from_array(original);
        let updated = match self {
            Self::Identity => point,
            Self::Move(delta) => point + delta,
            Self::Scale {
                axis: Axis::Uniform,
                factor,
                pivot,
            } => pivot + (point - pivot) * factor,
            Self::Scale { .. } => point,
            Self::Rotate {
                axis,
                degrees,
                pivot,
            } => pivot + Quat::from_axis_angle(axis, degrees.to_radians()) * (point - pivot),
        };
        updated
            .clamp(Vec3::new(-limit, floor, -limit), Vec3::splat(limit))
            .to_array()
    }

    fn scale(self, original: [f32; 3], minimum: [f32; 3], maximum: [f32; 3]) -> [f32; 3] {
        let factors = match self {
            Self::Scale {
                axis: Axis::Uniform,
                factor,
                ..
            } => Vec3::splat(factor),
            Self::Scale { axis, factor, .. } => Vec3::ONE + axis.unit() * (factor - 1.0),
            _ => return original,
        };
        (Vec3::from_array(original) * factors)
            .clamp(Vec3::from_array(minimum), Vec3::from_array(maximum))
            .to_array()
    }
}

enum Original {
    Prop(WorldObject),
    Model(Placement),
    House(Building),
}

fn originals(world: &World, ids: &[u64]) -> Vec<Original> {
    ids.iter()
        .filter_map(|&id| {
            world
                .objects
                .iter()
                .find(|value| value.id == id)
                .cloned()
                .map(Original::Prop)
                .or_else(|| {
                    world
                        .models
                        .iter()
                        .find(|value| value.id == id)
                        .cloned()
                        .map(Original::Model)
                })
                .or_else(|| {
                    world
                        .buildings
                        .iter()
                        .find(|value| value.id == id)
                        .cloned()
                        .map(Original::House)
                })
        })
        .collect()
}

fn apply(world: &mut World, source: &[Original], transform: Transform) -> bool {
    let mut changed = false;
    for entry in source {
        match entry {
            Original::Prop(before) => {
                if let Some(current) = world.objects.iter_mut().find(|item| item.id == before.id) {
                    let mut next = before.clone();
                    next.position = transform.position(before.position, 100.0, -100.0);
                    next.size = transform.scale(before.size, [0.05; 3], [100.0; 3]);
                    if let Transform::Rotate { degrees, .. } = transform {
                        next.yaw = (before.yaw + degrees).rem_euclid(360.0);
                    }
                    changed |= *current != next;
                    *current = next;
                }
            }
            Original::Model(before) => {
                if let Some(current) = world.models.iter_mut().find(|item| item.id == before.id) {
                    let mut next = before.clone();
                    next.position = transform.position(before.position, 256.0, -256.0);
                    next.scale = transform.scale(before.scale, [0.001; 3], [100.0; 3]);
                    if let Transform::Rotate { axis, degrees, .. } = transform
                        && degrees != 0.0
                    {
                        if axis == Vec3::Y && before.rotation[0] == 0.0 && before.rotation[2] == 0.0
                        {
                            next.rotation[1] = (before.rotation[1] + degrees).rem_euclid(360.0);
                        } else {
                            let rotation = Quat::from_axis_angle(axis, degrees.to_radians())
                                * rotation(before.rotation);
                            let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
                            next.rotation = [x, y, z].map(f32::to_degrees);
                        }
                    }
                    changed |= *current != next;
                    *current = next;
                }
            }
            Original::House(before) => {
                if let Some(current) = world.buildings.iter_mut().find(|item| item.id == before.id)
                {
                    let mut next = before.clone();
                    next.position = transform.position(before.position, 100.0, 0.0);
                    next.size = transform.scale(before.size, [4.0, 3.0, 3.6], [25.0, 12.0, 25.0]);
                    if let Transform::Rotate { degrees, .. } = transform {
                        next.quarter_turns = (i32::from(before.quarter_turns)
                            + (degrees / 90.0).round() as i32)
                            .rem_euclid(4) as u8;
                    }
                    if next.size != before.size {
                        next.door_width = next.door_width.clamp(1.10, next.size[0] - 1.4);
                        next.door_height = next.door_height.clamp(2.4, next.size[1] - 0.34);
                        let offset = ((next.size[0] - next.door_width) * 0.5 - 0.60).max(0.0);
                        next.door_offset = next.door_offset.clamp(-offset, offset);
                        next.interior.fit(next.size);
                    }
                    changed |= *current != next;
                    *current = next;
                }
            }
        }
    }
    changed
}

fn entity_position(world: &World, id: u64) -> Option<[f32; 3]> {
    world
        .objects
        .iter()
        .find(|item| item.id == id)
        .map(|item| item.position)
        .or_else(|| {
            world
                .models
                .iter()
                .find(|item| item.id == id)
                .map(|item| item.position)
        })
        .or_else(|| {
            world
                .buildings
                .iter()
                .find(|item| item.id == id)
                .map(|item| item.position)
        })
}

fn entity_rotation(world: &World, id: u64) -> Quat {
    world
        .objects
        .iter()
        .find(|item| item.id == id)
        .map(|item| Quat::from_rotation_y(item.yaw.to_radians()))
        .or_else(|| {
            world
                .models
                .iter()
                .find(|item| item.id == id)
                .map(|item| rotation(item.rotation))
        })
        .or_else(|| {
            world
                .buildings
                .iter()
                .find(|item| item.id == id)
                .map(|item| {
                    Quat::from_rotation_y(
                        f32::from(item.quarter_turns) * std::f32::consts::FRAC_PI_2,
                    )
                })
        })
        .unwrap_or(Quat::IDENTITY)
}

fn rotation(angles: [f32; 3]) -> Quat {
    let [x, y, z] = angles.map(f32::to_radians);
    Quat::from_euler(EulerRot::XYZ, x, y, z)
}

fn pivot(world: &World, ids: &[u64]) -> Option<Vec3> {
    let points = ids.iter().filter_map(|&id| entity_position(world, id));
    let (sum, count) = points.fold((Vec3::ZERO, 0usize), |(sum, count), point| {
        (sum + Vec3::from_array(point), count + 1)
    });
    (count > 0).then(|| sum / count as f32)
}

#[derive(Clone, Copy)]
struct Projection {
    matrix: Mat4,
    rect: Rect,
}

impl Projection {
    fn new(camera: Camera, rect: Rect) -> Self {
        let (_, matrix) = camera.projection(rect);
        Self { matrix, rect }
    }

    fn point(self, point: Vec3) -> Option<(Pos2, f32)> {
        let clip = self.matrix * point.extend(1.0);
        if !clip.is_finite() || clip.w < 0.04 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        let screen = egui::pos2(
            self.rect.left() + (ndc.x + 1.0) * self.rect.width() * 0.5,
            self.rect.top() + (1.0 - ndc.y) * self.rect.height() * 0.5,
        );
        (0.0..=1.0).contains(&ndc.z).then_some((screen, clip.w))
    }

    fn ray(self, point: Pos2) -> Option<Ray> {
        if !self.rect.is_positive() {
            return None;
        }
        let inverse = self.matrix.inverse();
        let x = (point.x - self.rect.left()) / self.rect.width() * 2.0 - 1.0;
        let y = 1.0 - (point.y - self.rect.top()) / self.rect.height() * 2.0;
        let start = inverse.project_point3(Vec3::new(x, y, 0.0));
        let far = inverse.project_point3(Vec3::new(x, y, 1.0));
        let distance = (far - start).length();
        (start.is_finite() && distance.is_finite() && distance > 0.0).then(|| Ray {
            start,
            dir: (far - start) / distance,
            distance,
        })
    }
}

#[derive(Clone, Copy)]
struct Ray {
    start: Vec3,
    dir: Vec3,
    distance: f32,
}

impl Ray {
    fn radial(self, pivot: Vec3, normal: Vec3) -> Option<Vec3> {
        let denominator = normal.dot(self.dir);
        if denominator.abs() < 0.045 {
            return None;
        }
        let distance = normal.dot(pivot - self.start) / denominator;
        if distance < 0.0 {
            return None;
        }
        (self.start + self.dir * distance - pivot).try_normalize()
    }

    fn intersects(self, bounds: Collider) -> Option<f32> {
        let (mut near, mut far) = (0.0_f32, self.distance);
        for axis in 0..3 {
            let origin = self.start[axis];
            let travel = self.dir[axis];
            if travel.abs() < 1e-6 {
                if origin < bounds.min[axis] || origin > bounds.max[axis] {
                    return None;
                }
                continue;
            }
            let low = (bounds.min[axis] - origin) / travel;
            let high = (bounds.max[axis] - origin) / travel;
            near = near.max(low.min(high));
            far = far.min(low.max(high));
            if near > far {
                return None;
            }
        }
        Some(near)
    }

    fn local(self, pivot: [f32; 3], rotation: Quat, scale: [f32; 3]) -> Self {
        let inverse = rotation.inverse();
        let scale = Vec3::from_array(scale);
        Self {
            start: (inverse * (self.start - Vec3::from_array(pivot))) / scale,
            dir: (inverse * self.dir) / scale,
            distance: self.distance,
        }
    }
}

fn pick(world: &World, assets: &Library, ray: Ray) -> Option<u64> {
    let models = world.models.iter().filter_map(|item| {
        let hit = if let Some(loaded) = assets.get(&item.source) {
            ray.local(item.visual_position(), rotation(item.rotation), item.scale)
                .intersects(Collider {
                    min: loaded.model.bounds_min,
                    max: loaded.model.bounds_max,
                })
        } else {
            item.custom_collider()
                .and_then(|bounds| ray.intersects(bounds))
        };
        hit.map(|distance| (item.id, distance))
    });
    let props = world.objects.iter().filter_map(|item| {
        let [width, height, depth] = item.size;
        ray.local(
            item.position,
            Quat::from_rotation_y(item.yaw.to_radians()),
            [1.0; 3],
        )
        .intersects(Collider {
            min: [-width / 2.0, 0.0, -depth / 2.0],
            max: [width / 2.0, height, depth / 2.0],
        })
        .map(|distance| (item.id, distance))
    });
    let houses = world.buildings.iter().filter_map(|house| {
        house
            .colliders()
            .into_iter()
            .filter_map(|bounds| ray.intersects(bounds))
            .min_by(f32::total_cmp)
            .map(|distance| (house.id, distance))
    });
    props
        .chain(models)
        .chain(houses)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

fn editor_bounds(world: &World, assets: &Library, id: u64) -> Option<Collider> {
    if let Some(item) = world.models.iter().find(|item| item.id == id) {
        return assets
            .get(&item.source)
            .map(|loaded| item.bounds(&loaded.model))
            .or_else(|| item.custom_collider());
    }
    if let Some(item) = world.objects.iter().find(|item| item.id == id) {
        return Some(item.editor_bounds());
    }
    let house = world.buildings.iter().find(|item| item.id == id)?;
    house.colliders().into_iter().reduce(|a, b| Collider {
        min: std::array::from_fn(|axis| a.min[axis].min(b.min[axis])),
        max: std::array::from_fn(|axis| a.max[axis].max(b.max[axis])),
    })
}

fn paint_bounds(painter: &egui::Painter, projection: Projection, bounds: Collider) {
    let corners: [_; 8] = std::array::from_fn(|mask| {
        let point = std::array::from_fn(|axis| {
            if mask & (1 << axis) == 0 {
                bounds.min[axis]
            } else {
                bounds.max[axis]
            }
        });
        projection
            .point(Vec3::from_array(point))
            .map(|(screen, _)| screen)
    });
    let stroke = Stroke::new(
        1.25_f32,
        Color32::from_rgba_unmultiplied(118, 226, 206, 175),
    );
    for (index, vertex) in corners.into_iter().enumerate() {
        for axis in 0..3 {
            let bit = 1 << axis;
            if index & bit == 0
                && let (Some(from), Some(to)) = (vertex, corners[index | bit])
            {
                painter.line_segment([from, to], stroke);
            }
        }
    }
}
