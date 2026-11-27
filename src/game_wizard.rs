use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Duration,
};

use eframe::egui;
use serde::Deserialize;
use serde_json::json;

use crate::{
    ACCENT, Studio, View, ai,
    game_plan::{Atmosphere, Controls, Format, Plan},
};

const SYSTEM: &str = r#"You are a game-planning assistant for Studio 0.22. Respond in the author's language.
Return only JSON with shape {"message":"...","plan":null,"unavailable":[]}.
If an important decision about genre, navigation, style, player, dialogue role, level, win or loss is ambiguous, ask one focused question in message.
When you know enough, include a plan and briefly explain how to play. The author can revise it before building.
A plan must have exactly these fields:
{"title":"Заголовок","format":"top","atmosphere":"blue_hour","controls":"wasd","hero":"Имя героя","companion":"Имя собеседника","signals":2,"seconds":22}
format accepts top, side, workshop3d. top and side are one editable level with three signal locations, a victory threshold of 1 to 3 signals, a deadline between 8 and 180 seconds, and retry.
workshop3d is the one-level rainy workshop with an existing dialogue, locked door/handle and delivery interaction. It has three variants: blue_hour (puddled central approach), amber_rain (warm canopy and side approach), morning_mist (open passage and diffuse morning). Its built-in objective is retained; signals and seconds apply only to top and side.
atmosphere is blue_hour, amber_rain or morning_mist. controls is wasd or arrows.
For unsupported features or missing assets, describe each in unavailable and offer an actual available alternative in message.
Never claim to have built or exported a game. Do not return paths, code or executable actions."#;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    message: String,
    #[serde(default)]
    plan: Option<Plan>,
    #[serde(default)]
    unavailable: Vec<String>,
}

struct Message {
    author: bool,
    text: String,
}

#[derive(Default)]
pub(crate) struct Wizard {
    open: bool,
    messages: Vec<Message>,
    input: String,
    plan: Option<Plan>,
    unavailable: Vec<String>,
    pending: Option<Receiver<Result<Reply, String>>>,
    notice: String,
    built: bool,
}

enum Job {
    Ask,
    Build,
    Review,
}

impl Studio {
    pub(super) fn close_game_wizard(&mut self) {
        self.wizard.open = false;
    }

    pub(super) fn wizard_enabled(&self) -> bool {
        self.wizard.open && !self.game_running
    }

    pub(super) fn wizard_button(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(!self.game_running, egui::Button::new("Мастер игры"))
            .clicked()
        {
            self.wizard.open = !self.wizard.open;
        }
    }

    pub(super) fn wizard_panel(&mut self, ctx: &egui::Context) {
        let mut wizard = std::mem::take(&mut self.wizard);
        wizard.show(self, ctx);
        self.wizard = wizard;
    }
}

impl Wizard {
    pub(crate) fn ready_plan(&self) -> Option<&Plan> {
        self.plan.as_ref()
    }

    pub(crate) fn awaiting_model(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn notice(&self) -> &str {
        &self.notice
    }

    fn show(&mut self, app: &mut Studio, ctx: &egui::Context) {
        self.receive();
        let mut job = None;
        egui::SidePanel::right("game_wizard")
            .default_width(385.0)
            .min_width(300.0)
            .max_width(555.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Мастер игры");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Закрыть мастер").clicked() {
                            self.open = false;
                        }
                    });
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.collapsing("Модель и адрес API", |ui| {
                            app.ai.connection_fields(ui);
                        });
                        for message in &self.messages {
                            ui.label(
                                egui::RichText::new(if message.author {
                                    "Автор"
                                } else {
                                    "Помощник"
                                })
                                .color(ACCENT)
                                .strong(),
                            );
                            ui.label(&message.text);
                        }
                        ui.add(
                            egui::TextEdit::multiline(&mut self.input)
                                .hint_text("Опиши игру или ответь на вопрос")
                                .desired_rows(3)
                                .desired_width(f32::INFINITY)
                                .char_limit(1400),
                        );
                        ui.horizontal_wrapped(|ui| {
                            if self.pending.is_some() {
                                ui.spinner();
                                ui.label("Готовится ответ");
                            } else if ui
                                .add_enabled(
                                    !self.input.trim().is_empty(),
                                    egui::Button::new("Обсудить с моделью"),
                                )
                                .clicked()
                            {
                                job = Some(Job::Ask);
                            }
                            if self.plan.is_none() && ui.button("Настроить план вручную").clicked()
                            {
                                self.plan = Some(Plan::default());
                            }
                        });
                        if !self.notice.is_empty() {
                            ui.label(&self.notice);
                        }
                        for limitation in &self.unavailable {
                            ui.label(format!("Требуется · {limitation}"));
                        }
                        if let Some(plan) = &mut self.plan {
                            ui.separator();
                            ui.heading("План проекта");
                            edit_plan(ui, plan);
                            let valid = plan.validate();
                            if let Err(error) = &valid {
                                ui.colored_label(egui::Color32::LIGHT_RED, error);
                            }
                            if ui
                                .add_enabled(
                                    valid.is_ok() && self.pending.is_none(),
                                    egui::Button::new("Собрать по плану"),
                                )
                                .clicked()
                            {
                                job = Some(Job::Build);
                            }
                        }
                        if self.built && ui.button("Предложить изменение правила").clicked()
                        {
                            job = Some(Job::Review);
                        }
                    });
            });
        match job {
            Some(Job::Ask) => self.ask(app, ctx),
            Some(Job::Build) => {
                if let Some(plan) = &self.plan {
                    let mut library = app.assets.clone();
                    match plan.build(&mut library) {
                        Ok(scene) => {
                            app.commit_editor_change();
                            app.assets = library;
                            app.apply_scene(
                                scene,
                                app.project.clone(),
                                "Игра собрана по плану. Сцену можно редактировать.",
                            );
                            app.view = View::Three;
                            if plan.format == Format::Workshop3d {
                                app.focus_camera();
                            }
                            self.notice = format!(
                                "«{}» готова к проверке. Сборка проекта отменяется одним действием.",
                                plan.title
                            );
                            self.built = true;
                            self.open = false;
                        }
                        Err(error) => self.notice = error,
                    }
                }
            }
            Some(Job::Review) => app.open_scene_review(),
            None => {}
        }
        if self.pending.is_some() {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }

    fn ask(&mut self, app: &Studio, ctx: &egui::Context) {
        self.push(Message {
            author: true,
            text: self.input.trim().into(),
        });
        self.input.clear();
        self.notice.clear();
        let turns: Vec<_> = self
            .messages
            .iter()
            .map(|entry| {
                json!({
                    "role": if entry.author { "author" } else { "assistant" },
                    "text": entry.text,
                })
            })
            .collect();
        let prompt = json!({"turns": turns, "editable_plan": self.plan}).to_string();
        let (endpoint, model, key) = app.ai.connection();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = ai::complete(&endpoint, &model, &key, SYSTEM, &prompt)
                .and_then(|response| parse_reply(ai::response_json(&response)));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    fn receive(&mut self) {
        let Some(waiting) = &self.pending else {
            return;
        };
        let reply = match waiting.try_recv() {
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("Модель завершила запрос без ответа.".into()),
            Ok(reply) => reply,
        };
        self.pending = None;
        match reply {
            Ok(reply) => {
                self.push(Message {
                    author: false,
                    text: reply.message,
                });
                self.unavailable = reply.unavailable;
                if let Some(plan) = reply.plan {
                    self.plan = Some(plan);
                }
            }
            Err(error) => self.notice = error,
        }
    }

    fn push(&mut self, message: Message) {
        if self.messages.len() >= 8 {
            self.messages.remove(0);
        }
        self.messages.push(message);
    }
}

fn parse_reply(raw: &str) -> Result<Reply, String> {
    let mut reply: Reply = serde_json::from_str(raw)
        .map_err(|error| format!("Модель вернула неполный план. {error}"))?;
    if reply.message.trim().is_empty()
        || reply.message.chars().count() > 2200
        || reply.unavailable.len() > 8
        || reply
            .unavailable
            .iter()
            .any(|entry| entry.trim().is_empty() || entry.chars().count() > 220)
    {
        return Err("Ответ должен содержать короткое сообщение и до восьми ограничений.".into());
    }
    reply.message = reply.message.trim().to_owned();
    if let Some(plan) = &reply.plan {
        plan.validate()?;
    }
    Ok(reply)
}

fn edit_plan(ui: &mut egui::Ui, plan: &mut Plan) {
    text_field(ui, "Название игры", &mut plan.title);
    egui::ComboBox::from_id_salt("wizard_format")
        .selected_text(plan.format.title())
        .show_ui(ui, |ui| {
            for item in Format::ALL {
                ui.selectable_value(&mut plan.format, item, item.title());
            }
        });
    text_field(ui, "Имя или роль героя", &mut plan.hero);
    if plan.format == Format::Workshop3d {
        text_field(ui, "Имя собеседника", &mut plan.companion);
        ui.label("Договорись с мастером, открой дверь и занеси инструмент.");
        ui.label("Свет и расстановка");
        for variant in Atmosphere::ALL {
            ui.radio_value(&mut plan.atmosphere, variant, variant.title());
        }
    } else {
        ui.label("Обойди сигналы и подтверди выход у маяка клавишей F.");
        ui.label("После истечения времени нажми R для новой попытки.");
        ui.add(
            egui::DragValue::new(&mut plan.signals)
                .prefix("Сигналов для победы ")
                .range(1..=3),
        );
        ui.add(
            egui::DragValue::new(&mut plan.seconds)
                .prefix("Лимит времени ")
                .suffix(" с")
                .range(8.0..=180.0)
                .fixed_decimals(0),
        );
        let palettes = ["Хвойная тропа", "Кирпичный двор", "Утренний сад"];
        egui::ComboBox::from_id_salt("wizard_palette")
            .selected_text(
                palettes[Atmosphere::ALL
                    .iter()
                    .position(|item| *item == plan.atmosphere)
                    .unwrap_or(0)],
            )
            .show_ui(ui, |ui| {
                for (variant, label) in Atmosphere::ALL.into_iter().zip(palettes) {
                    ui.selectable_value(&mut plan.atmosphere, variant, label);
                }
            });
    }
    egui::ComboBox::from_id_salt("wizard_controls")
        .selected_text(plan.controls.title())
        .show_ui(ui, |ui| {
            for item in Controls::ALL {
                ui.selectable_value(&mut plan.controls, item, item.title());
            }
        });
}

fn text_field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.add(
        egui::TextEdit::singleline(value)
            .desired_width(f32::INFINITY)
            .char_limit(70),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planner_wire_accepts_questions_and_rejects_invented_fields() {
        let question =
            parse_reply(r#"{"message":"Какой вид камеры?","unavailable":["Нет сетевой игры"]}"#)
                .unwrap();
        assert!(question.plan.is_none());
        assert!(parse_reply(r#"{"message":"ok","executable":"game.exe"}"#).is_err());
        let plan = json!({"message":"Готов план","plan":Plan::default()}).to_string();
        assert_eq!(parse_reply(&plan).unwrap().plan, Some(Plan::default()));
    }
}
