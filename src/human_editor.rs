use crate::{
    assets::Source,
    avatar::Avatar,
    model::{GarmentStyle, HairStyle, HumanStyle},
};
use eframe::egui;

#[derive(Clone)]
pub struct Draft {
    pub source: Source,
    pub name: String,
    pub height: f32,
    pub style: HumanStyle,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            source: Source::Emma,
            name: "Мой герой".into(),
            height: Source::Emma.height(),
            style: HumanStyle::default(),
        }
    }
}

impl Draft {
    pub fn from_avatar(avatar: &Avatar) -> Self {
        Self {
            source: avatar.source.clone(),
            name: avatar.name.clone(),
            height: avatar.height,
            style: avatar.human_style.unwrap_or_default(),
        }
    }
}

pub enum Action {
    Apply,
    Face,
    FullBody,
}

pub fn edit(ctx: &egui::Context, visible: &mut bool, draft: &mut Draft) -> Option<Action> {
    let mut action = None;
    let mut done = false;
    let height = (ctx.content_rect().height() - 275.0).clamp(245.0, 575.0);
    egui::Window::new("Редактор персонажа")
        .id(egui::Id::new("edit_realistic_human"))
        .default_width(332.0)
        .anchor(egui::Align2::RIGHT_TOP, [-14.0, 82.0])
        .collapsible(false)
        .open(visible)
        .show(ctx, |ui| {
            let previous_source = draft.source.clone();
            egui::ComboBox::from_label("Основа")
                .selected_text(draft.source.human_basis())
                .show_ui(ui, |ui| {
                    for source in [
                        Source::Emma,
                        Source::Ryan,
                        Source::CityWoman,
                        Source::CityHuman,
                    ] {
                        let label = source.human_basis();
                        ui.selectable_value(&mut draft.source, source, label);
                    }
                });
            if draft.source != previous_source
                && (draft.height - previous_source.height()).abs() < 0.005
            {
                draft.height = draft.source.height();
            }
            ui.add(
                egui::TextEdit::singleline(&mut draft.name)
                    .hint_text("Имя")
                    .char_limit(100),
            );
            ui.add(egui::Slider::new(&mut draft.height, 1.3..=2.25).text("Рост, м"));
            ui.horizontal_wrapped(|ui| {
                ui.label("Стартовый стиль");
                if ui.button("Реализм").clicked() {
                    draft.style = HumanStyle::default();
                    action = Some(Action::Apply);
                }
                if ui.button("Аниме").clicked() {
                    draft.style = HumanStyle::anime();
                    action = Some(Action::Apply);
                }
            });
            egui::ScrollArea::vertical()
                .max_height(height)
                .show(ui, |ui| edit_appearance(ui, &mut draft.style));
            ui.horizontal_wrapped(|ui| {
                if ui.button("Применить внешность").clicked() {
                    action = Some(Action::Apply);
                }
                if ui.button("К лицу").clicked() {
                    action = Some(Action::Face);
                }
                if ui.button("В полный рост").clicked() {
                    action = Some(Action::FullBody);
                }
            });
            done = ui.button("Закрыть настройки внешности").clicked();
        });
    if done {
        *visible = false;
    }
    action
}

fn edit_appearance(ui: &mut egui::Ui, style: &mut HumanStyle) {
    ui.separator();
    ui.strong("Лицо");
    ui.add(egui::Slider::new(&mut style.face_width, -1.0..=1.0).text("Ширина лица"));
    ui.add(egui::Slider::new(&mut style.jaw_width, -1.0..=1.0).text("Челюсть"));
    ui.add(egui::Slider::new(&mut style.nose_length, -1.0..=1.0).text("Нос"));
    ui.add(egui::Slider::new(&mut style.eye_scale, -1.0..=1.0).text("Размер глаз"));
    ui.add(egui::Slider::new(&mut style.illustration, 0.0..=1.0).text("Рисованное освещение"));
    ui.separator();
    ui.strong("Мимика");
    ui.horizontal_wrapped(|ui| {
        for (label, smile, brows, mouth) in [
            ("Спокойно", 0.0, 0.0, 0.0),
            ("Улыбка", 0.85, 0.15, 0.16),
            ("Удивление", 0.0, 0.92, 0.74),
        ] {
            if ui.button(label).clicked() {
                style.smile = smile;
                style.brow_raise = brows;
                style.mouth_open = mouth;
                style.blink = 0.0;
            }
        }
    });
    ui.add(egui::Slider::new(&mut style.smile, -1.0..=1.0).text("Улыбка"));
    ui.add(egui::Slider::new(&mut style.brow_raise, -1.0..=1.0).text("Брови"));
    ui.add(egui::Slider::new(&mut style.mouth_open, 0.0..=1.0).text("Рот"));
    ui.add(egui::Slider::new(&mut style.blink, 0.0..=1.0).text("Прищур"));
    ui.separator();
    egui::ComboBox::from_label("Причёска")
        .selected_text(hair_label(style.hairstyle))
        .show_ui(ui, |ui| {
            for hair in [
                HairStyle::Original,
                HairStyle::Cropped,
                HairStyle::Full,
                HairStyle::Swept,
            ] {
                ui.selectable_value(&mut style.hairstyle, hair, hair_label(hair));
            }
        });
    ui.add(egui::Slider::new(&mut style.hair_volume, -1.0..=1.0).text("Объём волос"));
    ui.separator();
    ui.strong("Одежда");
    egui::ComboBox::from_label("Фасон")
        .selected_text(garment_label(style.garment))
        .show_ui(ui, |ui| {
            for garment in [
                GarmentStyle::Original,
                GarmentStyle::Sweater,
                GarmentStyle::LongCoat,
            ] {
                ui.selectable_value(&mut style.garment, garment, garment_label(garment));
            }
        });
    ui.add(egui::Slider::new(&mut style.jacket_fit, -1.0..=1.0).text("Посадка верха"));
    ui.add(egui::Slider::new(&mut style.trouser_fit, -1.0..=1.0).text("Посадка брюк"));
    for (label, tint) in [
        ("Верх", &mut style.jacket_tint),
        ("Брюки", &mut style.trouser_tint),
    ] {
        ui.horizontal(|ui| {
            ui.color_edit_button_rgb(tint);
            ui.label(label);
        });
    }
}

fn hair_label(hair: HairStyle) -> &'static str {
    match hair {
        HairStyle::Original => "Исходная",
        HairStyle::Cropped => "Короткая",
        HairStyle::Full => "Объёмная",
        HairStyle::Swept => "Зачёс набок",
    }
}

fn garment_label(garment: GarmentStyle) -> &'static str {
    match garment {
        GarmentStyle::Original => "Исходный комплект",
        GarmentStyle::Sweater => "Свитер",
        GarmentStyle::LongCoat => "Длинное пальто",
    }
}
