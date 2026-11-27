use std::ops::RangeInclusive;

use eframe::egui::{self, Align2, Color32, DragValue, FontId, Key, Pos2, Sense, Stroke};

use crate::{
    plan2d::{Plan, key_marker_id, marker_key_id},
    scene::{Effect, MAX_KEYS, PartMotion, Scene, Shape},
    world::{DoorSettings, KeyPickup, MAX_KEY_PICKUPS, World},
};

const KEY_TOLERANCE: f32 = 0.001;

pub(crate) fn edit_rgb(ui: &mut egui::Ui, color: &mut [f32; 3]) -> bool {
    let mut edited = *color;
    // egui converts its temporary HSV value to RGB even on an idle frame.
    if ui.color_edit_button_rgb(&mut edited).changed() {
        *color = edited;
        true
    } else {
        false
    }
}

fn key_placement_id() -> egui::Id {
    egui::Id::new("world_key_placement")
}

pub fn key_list(
    ui: &mut egui::Ui,
    world: &mut World,
    selected: &mut Option<u64>,
    enabled: bool,
    position: [f32; 3],
) -> bool {
    ui.add_enabled_ui(enabled, |ui| {
        let mut changed = false;
        let [x, y, z] = position;
        let position = [
            x.clamp(-256.0, 256.0),
            y.clamp(0.0, 100.0),
            z.clamp(-256.0, 256.0),
        ];
        if ui
            .add_enabled(
                world.keys.len() < MAX_KEY_PICKUPS,
                egui::Button::new("+ Ключ"),
            )
            .clicked()
            && let Some(id) = world.add_key_at(position)
        {
            *selected = Some(key_marker_id(id));
            changed = true;
        }
        if !world.keys.is_empty() {
            egui::ScrollArea::vertical()
                .id_salt("world_keys")
                .max_height(145.0)
                .show(ui, |ui| {
                    for key in &world.keys {
                        let marker = Some(key_marker_id(key.id));
                        ui.push_id(key.id, |ui| {
                            if ui
                                .selectable_label(*selected == marker, key_name(key))
                                .clicked()
                            {
                                *selected = marker;
                            }
                        });
                    }
                });
        }
        changed
    })
    .inner
}

pub fn key_properties(
    ui: &mut egui::Ui,
    world: &mut World,
    selected: Option<u64>,
    enabled: bool,
    open_plan: &mut bool,
) -> Option<bool> {
    let id = marker_key_id(selected?)?;
    let key = world.keys.iter_mut().find(|key| key.id == id)?;
    let mut changed = false;
    let mut remove = false;
    ui.add_enabled_ui(enabled, |ui| {
        ui.heading("Подбираемый ключ");
        ui.weak("Игрок подбирает ключ рядом с ним клавишей E.");
        let name = ui.add(
            egui::TextEdit::singleline(&mut key.name)
                .char_limit(64)
                .hint_text("Название ключа"),
        );
        if name.changed() {
            key.name.retain(|character| !character.is_control());
            changed = true;
        }
        if name.lost_focus() && key.name.trim().is_empty() {
            key.name = format!("Ключ {}", key.id);
            changed = true;
        }
        ui.label("Позиция");
        ui.horizontal_wrapped(|ui| {
            for ((axis, bounds), value) in [
                ("X ", -256.0..=256.0),
                ("Y ", 0.0..=100.0),
                ("Z ", -256.0..=256.0),
            ]
            .into_iter()
            .zip(&mut key.position)
            {
                changed |= ui
                    .add(DragValue::new(value).prefix(axis).range(bounds).speed(0.05))
                    .changed();
            }
        });
        let placement = key_placement_id();
        let placing = ui.ctx().data(|data| data.get_temp::<u32>(placement)) == Some(id);
        if ui
            .button(if placing {
                "Отменить выбор позиции"
            } else {
                "Поставить на 2D-плане"
            })
            .clicked()
        {
            ui.ctx().data_mut(|data| {
                if placing {
                    data.remove::<u32>(placement);
                } else {
                    data.insert_temp(placement, id);
                }
            });
            *open_plan = !placing;
        }
        if placing {
            ui.weak("Щёлкни по точке на плане.");
        }
        remove = ui
            .button("Удалить ключ")
            .on_hover_text("Связанные двери останутся без замка.")
            .clicked();
    });
    if remove {
        changed |= world.remove_key(id);
    }
    Some(changed)
}

pub fn key_at_plan_click(
    ctx: &egui::Context,
    world: &mut World,
    selected: &mut Option<u64>,
    plan: Plan,
    mouse: Pos2,
) -> Option<bool> {
    let placement = key_placement_id();
    let pending = ctx.data_mut(|data| {
        let pending = data.get_temp::<u32>(placement);
        data.remove::<u32>(placement);
        pending
    });
    if let Some(id) = pending
        && *selected == Some(key_marker_id(id))
        && let Some(key) = world.keys.iter_mut().find(|key| key.id == id)
    {
        let point = plan.coordinates(mouse);
        let before = key.position;
        key.position[0] = point.x.clamp(-256.0, 256.0);
        key.position[2] = point.y.clamp(-256.0, 256.0);
        return Some(key.position != before);
    }
    let (id, _) = world
        .keys
        .iter()
        .map(|key| {
            let [x, _, z] = key.position;
            (key.id, plan.point(x, z).distance_sq(mouse))
        })
        .filter(|(_, distance)| *distance <= 14.0 * 14.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    *selected = Some(key_marker_id(id));
    Some(false)
}

pub fn door_lock(ui: &mut egui::Ui, door: &mut DoorSettings, keys: &[KeyPickup]) -> bool {
    let previous = *door;
    let selected = door
        .required_key
        .and_then(|id| keys.iter().find(|key| key.id == id))
        .map_or("Без замка", key_name);
    ui.horizontal_wrapped(|ui| {
        ui.label("Ключ для двери");
        egui::ComboBox::from_id_salt("door_key")
            .selected_text(selected)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut door.required_key, None, "Без замка");
                for key in keys {
                    ui.push_id(key.id, |ui| {
                        ui.selectable_value(&mut door.required_key, Some(key.id), key_name(key));
                    });
                }
            });
    });
    if keys.is_empty() {
        ui.weak("Чтобы запереть дверь, добавь ключ в разделе «Мир».");
    }
    if door.required_key.is_some() {
        door.initially_open = false;
    }
    *door != previous
}

fn key_name(key: &KeyPickup) -> &str {
    if key.name.trim().is_empty() {
        "Ключ"
    } else {
        &key.name
    }
}

#[cfg(test)]
mod key_tests {
    use super::*;

    #[test]
    fn plan_click_selects_and_repositions_the_stable_key() {
        let ctx = egui::Context::default();
        let mut world = World::default();
        let key = world.add_key_at([1.25, 0.6, -0.5]).unwrap();
        let other = world.add_key_at([-1.0, 0.0, 0.5]).unwrap();
        let plan = Plan {
            rect: egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(360.0, 250.0)),
            focus: egui::vec2(2.0, -1.0),
            scale: 31.0,
        };
        let mut selected = None;
        assert_eq!(
            key_at_plan_click(
                &ctx,
                &mut world,
                &mut selected,
                plan,
                plan.point(1.25, -0.5)
            ),
            Some(false)
        );
        assert_eq!(selected, Some(key_marker_id(key)));

        ctx.data_mut(|data| data.insert_temp(key_placement_id(), key));
        assert_eq!(
            key_at_plan_click(&ctx, &mut world, &mut selected, plan, plan.point(2.0, 1.5)),
            Some(true)
        );
        assert_eq!(world.key(key).unwrap().position, [2.0, 0.6, 1.5]);
        assert_eq!(world.key(other).unwrap().position, [-1.0, 0.0, 0.5]);
        assert!(
            ctx.data(|data| data.get_temp::<u32>(key_placement_id()))
                .is_none()
        );
    }
}

pub fn part(
    ui: &mut egui::Ui,
    scene: &mut Scene,
    selected: Option<u64>,
    time: f32,
    enabled: bool,
) -> bool {
    let Some(part) = scene
        .parts
        .iter_mut()
        .find(|part| Some(part.id) == selected)
    else {
        ui.weak("Выбери деталь в списке робота.");
        return false;
    };

    let part_id = part.id;
    ui.add_enabled_ui(enabled, |ui| {
        let mut changed = ui
            .add(egui::TextEdit::singleline(&mut part.name).hint_text("Имя детали"))
            .changed();

        let old_shape = part.shape;
        egui::ComboBox::from_id_salt("part_shape")
            .selected_text(shape_name(part.shape))
            .show_ui(ui, |ui| {
                for shape in [Shape::Box, Shape::Sphere, Shape::Cylinder] {
                    ui.selectable_value(&mut part.shape, shape, shape_name(shape));
                }
            });
        changed |= old_shape != part.shape;
        changed |= vector(ui, "Позиция", &mut part.position, -100.0..=100.0, 0.02);
        changed |= vector(ui, "Размер", &mut part.size, 0.01..=100.0, 0.02);
        changed |= vector(
            ui,
            "Поворот, °",
            &mut part.rotation,
            -3_600.0..=3_600.0,
            1.0,
        );
        changed |= ui
            .horizontal(|ui| {
                ui.label("Цвет");
                edit_rgb(ui, &mut part.color)
            })
            .inner;

        let old_motion = part.motion;
        ui.label("Движение при ходьбе")
            .on_hover_text("Роль добавляет автоматическое движение детали в игровом режиме.");
        egui::ComboBox::from_id_salt("part_motion")
            .selected_text(motion_name(part.motion))
            .show_ui(ui, |ui| {
                for role in [
                    PartMotion::Static,
                    PartMotion::LeftArm,
                    PartMotion::RightArm,
                    PartMotion::LeftLeg,
                    PartMotion::RightLeg,
                    PartMotion::LeftFoot,
                    PartMotion::RightFoot,
                ] {
                    ui.selectable_value(&mut part.motion, role, motion_name(role));
                }
            });
        changed |= part.motion != old_motion;

        ui.add_space(6.0);
        ui.separator();
        ui.strong(format!("Ключ анимации · {time:.2} с"));
        let key_index = scene
            .animation
            .keys
            .iter()
            .position(|key| key.part_id == part_id && (key.time - time).abs() < KEY_TOLERANCE);
        let has_capacity = scene.animation.keys.len() < MAX_KEYS;

        if let Some(index) = key_index {
            let key = &mut scene.animation.keys[index];
            changed |= vector(ui, "Смещение", &mut key.position, -100.0..=100.0, 0.02);
            changed |= vector(
                ui,
                "Доп. поворот, °",
                &mut key.rotation,
                -3_600.0..=3_600.0,
                1.0,
            );
            if ui.button("Удалить ключ").clicked() {
                scene.animation.keys.remove(index);
                changed = true;
            }
        } else {
            ui.weak("Ключ задаёт смещение и поворот детали в выбранный момент.");
            if ui
                .add_enabled(has_capacity, egui::Button::new("Добавить ключ"))
                .clicked()
            {
                scene.animation.keyframe_mut(part_id, time);
                changed = true;
            }
        }
        changed
    })
    .inner
}

pub fn timeline(
    ui: &mut egui::Ui,
    scene: &mut Scene,
    selected: Option<u64>,
    time: &mut f32,
    preview: &mut bool,
    game_running: bool,
) -> bool {
    ui.add_enabled_ui(!game_running, |ui| {
        let mut moved = false;
        ui.horizontal_wrapped(|ui| {
            let label = if *preview {
                "Пауза"
            } else {
                "Воспроизвести"
            };
            if ui.button(label).clicked() {
                if !*preview && *time >= scene.animation.duration {
                    *time = 0.0;
                    moved = true;
                }
                *preview = !*preview;
            }
            if ui.button("В начало").clicked() {
                *time = 0.0;
                *preview = false;
                moved = true;
            }
            ui.separator();
            ui.label("Длина");
            let mut duration = scene.animation.duration;
            if ui.add(seconds(&mut duration, 0.05..=60.0)).changed() {
                *time *= duration / scene.animation.duration;
                scene.animation.set_duration(duration);
                *time = (*time).clamp(0.0, scene.animation.duration);
                moved = true;
            }
            ui.label("Метка эффекта")
                .on_hover_text("Время запуска эффекта при просмотре анимации.");
            let duration = scene.animation.duration;
            ui.add(seconds(&mut scene.animation.event_time, 0.0..=duration));
        });

        let keys: Vec<f32> = scene
            .animation
            .keys
            .iter()
            .filter(|key| Some(key.part_id) == selected)
            .map(|key| key.time)
            .collect();
        let scrubbed = track(
            ui,
            scene.animation.duration,
            &keys,
            scene.animation.event_time,
            time,
        );
        if scrubbed {
            *preview = false;
        }
        moved || scrubbed
    })
    .inner
}

pub fn effects(ui: &mut egui::Ui, effect: &mut Effect) -> bool {
    ui.horizontal(|ui| {
        ui.label("Цвет искр");
        edit_rgb(ui, &mut effect.color);
    });
    ui.add(
        egui::Slider::new(&mut effect.count, 0..=1_024)
            .text("Частицы")
            .logarithmic(true),
    );
    ui.add(egui::Slider::new(&mut effect.intensity, 0.0..=10.0).text("Сила"));
    ui.horizontal(|ui| {
        ui.label("Длительность");
        ui.add(seconds(&mut effect.duration, 0.05..=10.0));
    });
    ui.button("Проверить эффект").clicked()
}

fn shape_name(shape: Shape) -> &'static str {
    match shape {
        Shape::Box => "Блок",
        Shape::Sphere => "Сфера",
        Shape::Cylinder => "Цилиндр",
    }
}

fn motion_name(role: PartMotion) -> &'static str {
    match role {
        PartMotion::Static => "Без роли",
        PartMotion::LeftArm => "Левая рука",
        PartMotion::RightArm => "Правая рука",
        PartMotion::LeftLeg => "Левая нога",
        PartMotion::RightLeg => "Правая нога",
        PartMotion::LeftFoot => "Левая стопа",
        PartMotion::RightFoot => "Правая стопа",
    }
}

fn seconds(value: &mut f32, range: RangeInclusive<f32>) -> DragValue<'_> {
    DragValue::new(value)
        .range(range)
        .speed(0.01)
        .fixed_decimals(2)
        .suffix(" с")
}

fn vector(
    ui: &mut egui::Ui,
    label: &str,
    values: &mut [f32; 3],
    range: RangeInclusive<f32>,
    speed: f64,
) -> bool {
    ui.label(label);
    ui.horizontal(|ui| {
        let spacing = ui.spacing().item_spacing.x;
        let width = ((ui.available_width() - 2.0 * spacing) / 3.0).max(26.0);
        let size = egui::vec2(width, ui.spacing().interact_size.y);
        let mut changed = false;
        for (value, axis) in values.iter_mut().zip(["X ", "Y ", "Z "]) {
            let editor = DragValue::new(value)
                .speed(speed)
                .range(range.clone())
                .max_decimals(2)
                .prefix(axis);
            changed |= ui.add_sized(size, editor).changed();
        }
        changed
    })
    .inner
}

fn track(ui: &mut egui::Ui, duration: f32, keys: &[f32], event_time: f32, time: &mut f32) -> bool {
    let size = egui::vec2(ui.available_width().max(48.0), 48.0);
    let response = ui.allocate_response(size, Sense::click_and_drag());
    let rect = response.rect.shrink2(egui::vec2(10.0, 4.0));
    let start = rect.left();
    let span = rect.width();
    let x_at = |value: f32| start + (value / duration) * span;
    let painter = ui.painter_at(response.rect);
    let mut moved = false;

    if response.clicked() || response.dragged() || response.drag_started() {
        if let Some(pointer) = response.interact_pointer_pos() {
            *time = ((pointer.x - start) / span).clamp(0.0_f32, 1.0) * duration;
            if response.clicked() {
                response.request_focus();
                if let Some(marker) = keys
                    .iter()
                    .copied()
                    .chain([event_time])
                    .map(|marker| (marker, (x_at(marker) - pointer.x).abs()))
                    .filter(|(_, distance)| *distance < 7.0_f32)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                {
                    *time = marker.0;
                }
            }
            moved = true;
        }
    }

    if response.has_focus() && response.enabled() {
        let direction = ui.input(|input| {
            input.key_pressed(Key::ArrowRight) as i32 - input.key_pressed(Key::ArrowLeft) as i32
        });
        if direction != 0 {
            *time = (*time + 0.05_f32 * direction as f32).clamp(0.0_f32, duration);
            moved = true;
        }
    }

    let line_y = rect.top() + 16.0;
    let line_color = ui.visuals().widgets.noninteractive.bg_stroke.color;
    let text_color = ui.visuals().weak_text_color();
    let cursor_color = ui.visuals().selection.stroke.color;
    let event_color = Color32::from_rgb(244, 172, 87);
    let font = FontId::monospace(11.0);
    painter.rect_filled(response.rect, 6.0, ui.visuals().extreme_bg_color);
    painter.line_segment(
        [
            Pos2::new(rect.left(), line_y),
            Pos2::new(rect.right(), line_y),
        ],
        Stroke::new(2.0_f32, line_color),
    );
    for tick in 0..=8 {
        let x = x_at(duration * tick as f32 / 8.0);
        painter.line_segment(
            [Pos2::new(x, line_y - 3.0), Pos2::new(x, line_y + 4.0)],
            Stroke::new(1.0_f32, line_color),
        );
    }
    for &key in keys {
        diamond(&painter, Pos2::new(x_at(key), line_y), 4.5, cursor_color);
    }
    diamond(
        &painter,
        Pos2::new(x_at(event_time), line_y),
        6.0,
        event_color,
    );
    let cursor_x = x_at(*time);
    painter.line_segment(
        [
            Pos2::new(cursor_x, line_y - 12.0),
            Pos2::new(cursor_x, line_y + 10.0),
        ],
        Stroke::new(2.0_f32, cursor_color),
    );
    for (x, alignment, label) in [
        (rect.left(), Align2::LEFT_BOTTOM, "0.00 с".to_owned()),
        (
            rect.center().x,
            Align2::CENTER_BOTTOM,
            format!("{:.2} с", *time),
        ),
        (
            rect.right(),
            Align2::RIGHT_BOTTOM,
            format!("{duration:.2} с"),
        ),
    ] {
        painter.text(
            Pos2::new(x, rect.bottom()),
            alignment,
            label,
            font.clone(),
            text_color,
        );
    }
    response
        .on_hover_text("Перетащи указатель времени. Цветные ромбы — ключи детали и метка эффекта.");
    moved
}

fn diamond(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    painter.add(egui::Shape::convex_polygon(
        vec![
            center + egui::vec2(0.0, -radius),
            center + egui::vec2(radius, 0.0),
            center + egui::vec2(0.0, radius),
            center + egui::vec2(-radius, 0.0),
        ],
        color,
        Stroke::NONE,
    ));
}
