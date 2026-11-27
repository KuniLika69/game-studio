use eframe::egui;

use crate::{
    body_physics::{Axis, AxisJoint, BodyKind, MAX_BODIES, MAX_SENSORS, SensorKind, Simulation},
    physics_project::{
        Addition, NamedBody, NamedSensor, PhysicsScene, Selection, bounds, center_half, valid_name,
    },
    world::Collider,
};

#[derive(Default)]
pub struct Editor {
    selected: Option<Selection>,
    placing: bool,
}

impl Editor {
    pub fn selected(&self) -> Option<Selection> {
        self.selected
    }

    pub fn select(&mut self, selection: Selection) {
        self.selected = Some(selection);
        self.placing = false;
    }

    pub fn placing(&self) -> bool {
        self.placing
    }

    /// Invoke on a plan/viewport click only while placing().
    pub fn place(&mut self, scene: &mut PhysicsScene, at: [f32; 3]) -> bool {
        if !self.placing {
            return false;
        }
        self.placing = false;
        self.selected
            .is_some_and(|selection| scene.place(selection, at))
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        scene: &mut PhysicsScene,
        at: [f32; 3],
        source: Option<Collider>,
    ) -> bool {
        ui.heading("Физические объекты");
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            for (label, addition, available) in [
                ("+ Ящик", Addition::Crate, scene.bodies.len() < MAX_BODIES),
                (
                    "+ Платформа",
                    Addition::Platform,
                    scene.bodies.len() < MAX_BODIES,
                ),
                (
                    "+ Нажимная плита",
                    Addition::PressurePlate,
                    scene.sensors.len() < MAX_SENSORS,
                ),
                (
                    "+ Контактный объём",
                    Addition::OverlapTrigger,
                    scene.sensors.len() < MAX_SENSORS,
                ),
            ] {
                if ui
                    .add_enabled(available, egui::Button::new(label))
                    .clicked()
                    && let Some(selection) = scene.add(addition, at)
                {
                    self.select(selection);
                    changed = true;
                }
            }
        });
        if let Some(source) = source
            && ui
                .add_enabled(
                    scene.bodies.len() < MAX_BODIES,
                    egui::Button::new("Тело по коллайдеру выделенного объекта"),
                )
                .clicked()
            && let Some(selection) = scene.add_body_from_bounds(source)
        {
            self.select(selection);
            changed = true;
        }
        if !scene.is_empty() {
            changed |= ui
                .checkbox(&mut scene.implicit_floor, "Общий пол на высоте 0")
                .changed();
        }
        self.choose(ui, scene);
        if let Some(selection) = self.selected {
            ui.label(format!("ID для правил #{}", selection.id()));
            if self.placing {
                ui.weak("Щёлкни по опорной точке на плане или нажми «Отмена».");
                if ui.button("Отмена").clicked() {
                    self.placing = false;
                }
            } else if ui.button("Переместить на плане").clicked() {
                self.placing = true;
            }
            changed |= match selection {
                Selection::Body(id) => scene
                    .bodies
                    .iter_mut()
                    .find(|entry| entry.spec.id == id)
                    .is_some_and(|entry| body_fields(ui, entry)),
                Selection::Sensor(id) => scene
                    .sensors
                    .iter_mut()
                    .find(|entry| entry.spec.id == id)
                    .is_some_and(|entry| sensor_fields(ui, entry)),
            };
            if ui.small_button("Удалить физический объект").clicked() {
                self.selected = None;
                self.placing = false;
                changed |= scene.remove(selection);
            }
        }
        changed
    }

    fn choose(&mut self, ui: &mut egui::Ui, scene: &PhysicsScene) {
        let mut selected_entry = scene
            .entries()
            .find(|(entry, _)| Some(*entry) == self.selected);
        if selected_entry.is_none() {
            selected_entry = scene.entries().next();
            self.selected = selected_entry.map(|(entry, _)| entry);
            self.placing = false;
        }
        let Some((current, name)) = selected_entry else {
            return;
        };
        egui::ComboBox::from_id_salt("physics_entry")
            .selected_text(format!("{name} · #{}", current.id()))
            .show_ui(ui, |ui| {
                for (entry, name) in scene.entries() {
                    if ui
                        .selectable_label(entry == current, format!("{name} · #{}", entry.id()))
                        .clicked()
                    {
                        self.select(entry);
                    }
                }
            });
    }
}

fn body_fields(ui: &mut egui::Ui, entry: &mut NamedBody) -> bool {
    let incoming = entry.spec;
    let mut property_changed = name_field(ui, &mut entry.name);
    let mut body = entry.spec;
    frame(ui, &mut body.center, &mut body.half_extents, false);
    ui.horizontal_wrapped(|ui| {
        number(ui, "Трение ", &mut body.material.friction, 0.0..=2.0);
        number(ui, "Упругость ", &mut body.material.restitution, 0.0..=1.0);
    });
    match &mut body.kind {
        BodyKind::Dynamic { mass, joint } => {
            property_changed |= number(ui, "Влияние ветра ", &mut entry.wind_drag, 0.0..=12.0);
            property_changed |= ui.checkbox(&mut entry.buoyant, "Плавучесть").changed();
            number(ui, "Масса, кг ", mass, 0.01..=30_000.0);
            let mut guided = joint.is_some();
            if ui.checkbox(&mut guided, "Движение по одной оси").changed() {
                *joint = guided.then_some(AxisJoint {
                    axis: Axis::X,
                    travel: [-0.8, 0.8],
                });
            }
            if let Some(joint) = joint {
                choose_axis(ui, &mut joint.axis);
                ui.horizontal_wrapped(|ui| {
                    number(ui, "Нижний предел, м ", &mut joint.travel[0], -100.0..=0.0);
                    number(ui, "Верхний предел, м ", &mut joint.travel[1], 0.0..=100.0);
                });
                if joint.travel[1] - joint.travel[0] < 0.01 {
                    joint.travel[1] = joint.travel[0] + 0.01;
                }
            }
        }
        BodyKind::Platform {
            axis,
            travel,
            period,
        } => {
            choose_axis(ui, axis);
            ui.horizontal_wrapped(|ui| {
                number(ui, "Ход, м ", travel, -100.0..=100.0);
                number(ui, "Цикл, с ", period, 0.25..=600.0);
            });
            *travel = travel.signum() * travel.abs().clamp(0.05, 100.0);
            *period = period.max(2.0 * travel.abs() / 12.0);
        }
    }
    if body != incoming && Simulation::new(&[body], &[]).is_ok() {
        entry.spec = body;
        return true;
    }
    property_changed
}

fn sensor_fields(ui: &mut egui::Ui, entry: &mut NamedSensor) -> bool {
    let property_changed = name_field(ui, &mut entry.name);
    let mut sensor = entry.spec;
    let (mut center, mut half) = center_half(sensor.bounds);
    let top = matches!(sensor.kind, SensorKind::Pressure { .. });
    if frame(ui, &mut center, &mut half, top) {
        sensor.bounds = bounds(center, half);
    }
    match &mut sensor.kind {
        SensorKind::Overlap => {
            ui.label("События при входе и выходе тела.");
        }
        SensorKind::Pressure { minimum_mass } => {
            ui.label("Плита срабатывает, когда вес на её поверхности достигает порога.");
            number(ui, "Порог, кг ", minimum_mass, 0.01..=30_000.0);
        }
    }
    if sensor != entry.spec && Simulation::new(&[], &[sensor]).is_ok() {
        entry.spec = sensor;
        return true;
    }
    property_changed
}

fn frame(ui: &mut egui::Ui, center: &mut [f32; 3], half: &mut [f32; 3], top: bool) -> bool {
    let sign = if top { 1.0 } else { -1.0 };
    let mut anchor = *center;
    anchor[1] += sign * half[1];
    let mut sizes = half.map(|half| half * 2.0);
    let mut changed = false;
    ui.label(if top {
        "Верх плиты"
    } else {
        "Опорная точка"
    });
    ui.horizontal_wrapped(|ui| {
        for (label, value) in ["X ", "Y ", "Z "].into_iter().zip(&mut anchor) {
            changed |= number(ui, label, value, -9_936.0..=9_936.0);
        }
    });
    ui.label("Размеры в метрах");
    ui.horizontal_wrapped(|ui| {
        for (label, value) in ["Ширина ", "Высота ", "Глубина "]
            .into_iter()
            .zip(&mut sizes)
        {
            changed |= number(ui, label, value, 0.03..=64.0);
        }
    });
    if changed {
        *half = sizes.map(|size| size * 0.5);
        anchor[1] -= sign * half[1];
        *center = anchor;
    }
    changed
}

fn choose_axis(ui: &mut egui::Ui, axis: &mut Axis) {
    ui.horizontal(|ui| {
        ui.label("Ось");
        ui.selectable_value(axis, Axis::X, "X");
        ui.selectable_value(axis, Axis::Y, "Y");
        ui.selectable_value(axis, Axis::Z, "Z");
    });
}

fn number(
    ui: &mut egui::Ui,
    prefix: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    ui.add(
        egui::DragValue::new(value)
            .prefix(prefix)
            .range(range)
            .speed(0.025),
    )
    .changed()
}

fn name_field(ui: &mut egui::Ui, name: &mut String) -> bool {
    let mut edited = name.clone();
    if ui
        .add(egui::TextEdit::singleline(&mut edited).char_limit(64))
        .changed()
        && valid_name(&edited)
    {
        *name = edited;
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proof_input::{Driver, click, find_label};

    #[test]
    fn egui_clicks_create_stable_physics_entries_and_place_a_plate_flush_on_plan() {
        let ctx = egui::Context::default();
        let mut driver = Driver::new([860, 1000]);
        let mut scene = PhysicsScene::default();
        let mut editor = Editor::default();
        let source = Collider {
            min: [-1.0, 0.0, -0.5],
            max: [0.0, 1.0, 0.5],
        };
        {
            let mut show = |ctx: &egui::Context| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    editor.show(ui, &mut scene, [0.0; 3], Some(source));
                });
            };
            let mut frame = ctx.run(driver.raw(Vec::new()), &mut show);
            for label in [
                "Тело по коллайдеру выделенного объекта",
                "+ Нажимная плита",
                "Переместить на плане",
            ] {
                let position = find_label(&frame, label, 0)
                    .unwrap_or_else(|| panic!("missing editor control {label}"));
                driver.skip_frames(45);
                let _ = ctx.run(driver.raw(click(position)), &mut show);
                frame = ctx.run(driver.raw(Vec::new()), &mut show);
            }
            assert!(find_label(&frame, "ID для правил #2", 0).is_some());
        }
        assert_eq!(editor.selected(), Some(Selection::Sensor(2)));
        assert!(editor.placing());
        assert!(editor.place(&mut scene, [2.0, 0.4, 0.3]));
        assert!(!editor.placing());
        let plate = scene.sensors[0].spec.bounds;
        assert!((plate.max[1] - 0.4).abs() < f32::EPSILON);
        assert_eq!(scene.simulation().unwrap().body(1).unwrap().bounds, source);
        scene.validate().unwrap();
    }
}
