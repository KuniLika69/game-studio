use crate::{
    buildings::Building,
    npcs::{self, Npc, Waypoint},
};
use eframe::egui;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Panel {
    #[default]
    Route,
    Dialogue,
    Tasks,
}

pub fn tabs(ui: &mut egui::Ui, active: &mut Panel) {
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(active, Panel::Route, "Маршрут");
        ui.selectable_value(active, Panel::Dialogue, "Диалог");
        ui.selectable_value(active, Panel::Tasks, "Задания");
    });
}

pub fn route(
    ui: &mut egui::Ui,
    actor: &mut Npc,
    houses: &[Building],
    selected: &mut usize,
    place_on_plan: &mut bool,
) {
    let title = ui.add(
        egui::TextEdit::singleline(&mut actor.avatar.name)
            .char_limit(100)
            .hint_text("Имя персонажа"),
    );
    if title.lost_focus() && actor.avatar.name.trim().is_empty() {
        actor.avatar.name = "NPC".into();
    }
    ui.add(egui::Slider::new(&mut actor.speed, 0.35..=3.15).text("Скорость, м/с"));
    if !houses.is_empty() {
        ui.menu_button("Маршрут через дом", |ui| {
            for house in houses {
                if ui
                    .button(&house.name)
                    .on_hover_text("Заменить маршрут проходом с улицы в этот дом")
                    .clicked()
                {
                    actor.route = npcs::doorway_route(house);
                    *selected = 0;
                    ui.close();
                }
            }
        });
    }
    ui.checkbox(place_on_plan, "Добавлять точки кликом")
        .on_hover_text("Работает на плане и в 3D. В 3D существующую точку можно перетащить мышью.");
    ui.horizontal_wrapped(|ui| {
        for (index, _) in actor.route.iter().enumerate() {
            if ui
                .selectable_label(*selected == index, (index + 1).to_string())
                .clicked()
            {
                *selected = index;
            }
        }
        if ui
            .add_enabled(
                actor.route.len() < npcs::MAX_WAYPOINTS,
                egui::Button::new("+ Точка"),
            )
            .clicked()
        {
            let [x, y, z] = actor.route.last().map_or([0.0; 3], |point| point.position);
            add_point(actor, [x.min(98.5) + 1.5, y, z], selected);
        }
    });
    if let Some(point) = actor.route.get_mut(*selected) {
        ui.horizontal_wrapped(|ui| {
            for ((axis, minimum), value) in [("X ", -100.0), ("Y ", 0.0), ("Z ", -100.0)]
                .into_iter()
                .zip(&mut point.position)
            {
                ui.add(
                    egui::DragValue::new(value)
                        .prefix(axis)
                        .range(minimum..=100.0)
                        .speed(0.06),
                );
            }
        });
        ui.add(
            egui::DragValue::new(&mut point.wait)
                .prefix("Ожидание ")
                .suffix(" с")
                .range(0.0..=60.0),
        );
        if ui
            .add_enabled(actor.route.len() > 1, egui::Button::new("Удалить точку"))
            .clicked()
        {
            actor.route.remove(*selected);
            *selected = (*selected).min(actor.route.len() - 1);
        }
    }
}

pub fn add_point(actor: &mut Npc, position: [f32; 3], selected: &mut usize) {
    if actor.route.len() < npcs::MAX_WAYPOINTS {
        let [x, y, z] = position;
        actor.route.push(Waypoint::at([
            x.clamp(-100.0, 100.0),
            y.clamp(0.0, 100.0),
            z.clamp(-100.0, 100.0),
        ]));
        *selected = actor.route.len() - 1;
    }
}
