use eframe::egui::{self, InputState, Key, Modifiers};
pub(crate) use studio_core::play_settings::{Action, CameraMode, Movement3d, PlaySettings};
use studio_core::play_settings::{Binding, ControlFrame, Controls};

pub(crate) fn validate(settings: &PlaySettings) -> Result<(), String> {
    settings.validate_with(|name| Key::from_name(name).is_some_and(|key| key != Key::Escape))
}

/// Positive coordinates mean moving right and down in the current view.
pub(crate) fn read_keys(settings: &PlaySettings, input: &InputState) -> ControlFrame {
    let bindings = &settings.controls;
    let held_value = |action| f32::from(u8::from(held(bindings.for_action(action), input)));
    let pressed_action = |action| pressed(bindings.for_action(action), input);
    ControlFrame {
        movement: [
            held_value(Action::Right) - held_value(Action::Left),
            held_value(Action::Back) - held_value(Action::Forward),
        ],
        sprint: held(&bindings.sprint, input),
        jump: pressed_action(Action::Jump),
        interact: pressed_action(Action::Interact),
        mark: pressed_action(Action::Mark),
        retry: pressed_action(Action::Retry),
    }
}

pub(crate) fn uses_key(controls: &Controls, key: Key) -> bool {
    Action::ALL.into_iter().any(|action| {
        controls
            .for_action(action)
            .keys()
            .filter_map(Key::from_name)
            .any(|bound| bound == key)
    })
}

pub(crate) fn label(controls: &Controls, action: Action) -> String {
    let binding = controls.for_action(action);
    binding.alternate.as_deref().map_or_else(
        || key_caption(&binding.main).to_owned(),
        |key| format!("{} / {}", key_caption(&binding.main), key_caption(key)),
    )
}

fn held(binding: &Binding, input: &InputState) -> bool {
    binding.keys().any(|key| match key {
        "Shift" => input.modifiers.shift,
        "Ctrl" => input.modifiers.ctrl,
        "Alt" => input.modifiers.alt,
        _ => Key::from_name(key).is_some_and(|key| input.key_down(key)),
    })
}

fn pressed(binding: &Binding, input: &InputState) -> bool {
    binding.keys().filter_map(Key::from_name).any(|bound| {
        input.events.iter().any(|event| {
            matches!(
                event, egui::Event::Key { key, pressed: true, repeat: false, .. } if *key == bound
            )
        })
    })
}

fn key_caption(name: &str) -> &str {
    match Key::from_name(name) {
        Some(Key::ArrowUp) => "↑",
        Some(Key::ArrowDown) => "↓",
        Some(Key::ArrowLeft) => "←",
        Some(Key::ArrowRight) => "→",
        Some(Key::Space) => "Пробел",
        _ => name,
    }
}

fn action_title(action: Action) -> &'static str {
    match action {
        Action::Forward => "Вперёд / вверх",
        Action::Back => "Назад / вниз",
        Action::Left => "Влево",
        Action::Right => "Вправо",
        Action::Sprint => "Бег",
        Action::Jump => "Прыжок",
        Action::Interact => "Взаимодействие",
        Action::Mark => "Метка правила",
        Action::Retry => "Повтор попытки",
    }
}

fn camera_label(mode: CameraMode) -> &'static str {
    match mode {
        CameraMode::Follow => "Следовать",
        CameraMode::Fixed => "Фиксированная",
        CameraMode::Free => "Свободная",
    }
}

#[derive(Clone, Copy)]
struct Capture {
    action: Action,
    alternate: bool,
    modifiers: Modifiers,
}

pub(crate) fn inspector(ui: &mut egui::Ui, settings: &mut PlaySettings) -> bool {
    let before = settings.clone();
    let capture_id = ui.id().with("play_key_capture");
    let mut capture = ui.ctx().data(|data| data.get_temp::<Capture>(capture_id));
    if let Some(pending) = capture {
        let choice = ui.input(|input| capture_key(input, pending));
        if let Some(choice) = choice {
            if let Some(key) = choice {
                let binding = settings.controls.for_action_mut(pending.action);
                if pending.alternate {
                    binding.alternate = Some(key);
                } else {
                    binding.main = key;
                }
            }
            capture = None;
        }
    }

    ui.collapsing("Клавиши", |ui| {
        egui::Grid::new("play_bindings")
            .num_columns(4)
            .show(ui, |ui| {
                for action in [
                    Action::Interact,
                    Action::Mark,
                    Action::Retry,
                    Action::Forward,
                    Action::Back,
                    Action::Left,
                    Action::Right,
                    Action::Sprint,
                    Action::Jump,
                ] {
                    let binding = settings.controls.for_action_mut(action);
                    ui.push_id(action, |ui| {
                        ui.label(action_title(action));
                        for (alternate, caption) in [
                            (false, key_caption(&binding.main)),
                            (
                                true,
                                binding
                                    .alternate
                                    .as_deref()
                                    .map_or("Назначить", key_caption),
                            ),
                        ] {
                            let listening = capture.is_some_and(|current| {
                                current.action == action && current.alternate == alternate
                            });
                            let button = ui.button(if listening {
                                "Нажми клавишу…"
                            } else {
                                caption
                            });
                            if button.clicked() {
                                capture = Some(Capture {
                                    action,
                                    alternate,
                                    modifiers: ui.input(|input| input.modifiers),
                                });
                            }
                        }
                        if ui
                            .add_enabled(binding.alternate.is_some(), egui::Button::new("Убрать"))
                            .clicked()
                        {
                            binding.alternate = None;
                        }
                        ui.end_row();
                    });
                }
            });
        if capture.is_some() {
            ui.weak("Esc отменяет назначение клавиши.");
            ui.ctx().request_repaint();
        }
    });

    ui.collapsing("Движение 3D", |ui| {
        let movement = &mut settings.movement_3d;
        number(ui, "Ходьба ", &mut movement.walk_speed, 0.25..=24.0);
        movement.sprint_speed = movement.sprint_speed.max(movement.walk_speed);
        number(
            ui,
            "Бег ",
            &mut movement.sprint_speed,
            movement.walk_speed..=30.0,
        );
        number(ui, "Разгон ", &mut movement.acceleration, 0.5..=200.0);
        number(
            ui,
            "Разгон бега ",
            &mut movement.sprint_acceleration,
            0.5..=200.0,
        );
        number(ui, "Торможение ", &mut movement.braking, 0.5..=200.0);
        number(
            ui,
            "Разгон в воздухе ",
            &mut movement.air_acceleration,
            0.5..=200.0,
        );
        number(ui, "Скорость прыжка ", &mut movement.jump_speed, 0.2..=20.0);
    });
    ui.collapsing("Движение 2D", |ui| {
        let movement = &mut settings.movement_2d;
        number(ui, "Ходьба ", &mut movement.walk_speed, 0.25..=24.0);
        movement.sprint_speed = movement.sprint_speed.max(movement.walk_speed);
        number(
            ui,
            "Бег ",
            &mut movement.sprint_speed,
            movement.walk_speed..=30.0,
        );
        number(ui, "Разгон ", &mut movement.acceleration, 0.5..=200.0);
        number(ui, "Торможение ", &mut movement.braking, 0.5..=200.0);
        number(
            ui,
            "Скорость прыжка сбоку ",
            &mut movement.jump_speed,
            0.2..=20.0,
        );
    });
    ui.collapsing("Камера", |ui| {
        let camera = &mut settings.camera;
        egui::ComboBox::from_id_salt("game_camera_mode")
            .selected_text(camera_label(camera.mode))
            .show_ui(ui, |ui| {
                for mode in [CameraMode::Follow, CameraMode::Fixed, CameraMode::Free] {
                    ui.selectable_value(&mut camera.mode, mode, camera_label(mode));
                }
            });
        let mut has_distance = camera.distance.is_some();
        if ui
            .checkbox(&mut has_distance, "Дистанция при запуске")
            .changed()
        {
            camera.distance = has_distance.then_some(5.1);
        }
        if let Some(distance) = &mut camera.distance {
            number(ui, "Дистанция ", distance, 0.66..=38.0);
        }
        number(ui, "Чувствительность ", &mut camera.sensitivity, 0.1..=4.0);
        number(
            ui,
            "Скорость слежения ",
            &mut camera.follow_rate,
            1.0..=40.0,
        );
    });
    if ui.button("Сбросить управление и движение").clicked() {
        *settings = PlaySettings::default();
        capture = None;
    }
    ui.ctx().data_mut(|data| {
        if let Some(capture) = capture {
            data.insert_temp(capture_id, capture);
        } else {
            data.remove::<Capture>(capture_id);
        }
    });
    before != *settings
}

fn capture_key(input: &InputState, capture: Capture) -> Option<Option<String>> {
    let key = input.events.iter().find_map(|event| match event {
        egui::Event::Key {
            key,
            pressed: true,
            repeat: false,
            ..
        } => Some(*key),
        _ => None,
    });
    if let Some(key) = key {
        return Some((key != Key::Escape).then(|| key.name().to_owned()));
    }
    if capture.action == Action::Sprint {
        for (name, old, new) in [
            ("Shift", capture.modifiers.shift, input.modifiers.shift),
            ("Ctrl", capture.modifiers.ctrl, input.modifiers.ctrl),
            ("Alt", capture.modifiers.alt, input.modifiers.alt),
        ] {
            if new && !old {
                return Some(Some(name.into()));
            }
        }
    }
    None
}

fn number(ui: &mut egui::Ui, caption: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>) {
    ui.add(
        egui::DragValue::new(value)
            .prefix(caption)
            .range(range)
            .speed(0.025)
            .max_decimals(2),
    );
}
