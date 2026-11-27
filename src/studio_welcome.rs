use eframe::egui;

use crate::{
    Scene, Studio, View,
    editor_theme::{
        ACCENT, BACKDROP, BRIGHT, EDGE, HOVER, MUTED, PANEL, SELECTED, SURFACE, TEXT, studio_mark,
    },
    flagship, game_plan, map2d, meridian, rain_workshop,
};

#[derive(Clone, Copy)]
pub(crate) enum Page {
    Home,
    Create,
    Examples,
}

#[derive(Clone, Copy)]
enum Choice {
    Go(Page),
    Open,
    Empty,
    Character,
    Map(map2d::Mode),
    Rain,
    Flagship,
    Meridian,
    Trail,
    Quarter,
    Mechanics,
}

impl Studio {
    pub(super) fn show_welcome(&mut self, ctx: &egui::Context) -> bool {
        let Some(page) = self.welcome else {
            return false;
        };
        let mut choice = None;
        let frame = egui::Frame::central_panel(&ctx.style()).fill(BACKDROP);
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            let bounds = ctx.content_rect();
            paint_welcome_background(ui.painter(), bounds);
            let top = (ui.available_height() * 0.12).clamp(12.0, 116.0);
            egui::ScrollArea::vertical()
                .id_salt("welcome")
                .show(ui, |ui| {
                    let width = (ui.available_width().min(850.0) - 54.0).max(1.0);
                    ui.add_space(top);
                    ui.vertical_centered(|ui| {
                        egui::Frame::new()
                            .fill(PANEL)
                            .stroke(egui::Stroke::new(1.0_f32, EDGE))
                            .corner_radius(24)
                            .inner_margin(20)
                            .show(ui, |ui| {
                                ui.set_width(width);
                                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                    welcome_heading(ui, page);
                                    ui.add_space(15.0);
                                    choice = page_actions(ui, page, &self.status);
                                });
                            });
                    });
                    ui.add_space(16.0);
                });
        });
        match choice {
            Some(Choice::Go(page)) => self.welcome = Some(page),
            Some(Choice::Open) => {
                self.open();
                if self.project.is_some() {
                    self.welcome = None;
                }
            }
            Some(Choice::Empty) => {
                self.set_scene(Scene::empty(), None, "Создана пустая 3D-сцена.");
                self.welcome = None;
                self.view = View::Three;
            }
            Some(Choice::Character) => {
                self.set_scene(Scene::empty(), None, "Выбери внешность и имя персонажа.");
                self.welcome = None;
                self.open_character_appearance();
            }
            Some(Choice::Map(mode)) => {
                self.set_scene(map2d::scene(mode, false), None, "Создана пустая 2D-карта.");
                self.welcome = None;
            }
            Some(Choice::Rain) => {
                self.rain_workshop_demo(rain_workshop::Style::BlueHour);
                if self.scene.avatar.is_some() {
                    self.welcome = None;
                }
            }
            Some(Choice::Flagship) => {
                if self.flagship_demo() {
                    self.welcome = None;
                }
            }
            Some(Choice::Meridian) => {
                if self.meridian_demo() {
                    self.welcome = None;
                }
            }
            Some(Choice::Trail) => match game_plan::Plan::default().build(&mut self.assets) {
                Ok(scene) => {
                    self.set_scene(scene, None, "Открыта «Сигнальная тропа».");
                    self.welcome = None;
                }
                Err(error) => self.status = format!("Не удалось открыть пример. {error}"),
            },
            Some(Choice::Quarter) => {
                self.quarter_demo();
                self.welcome = None;
            }
            Some(Choice::Mechanics) => match crate::mechanics_pack::sample() {
                Ok(scene) => {
                    self.set_scene(scene, None, "Открыт пример из библиотеки механик JSON.");
                    self.welcome = None;
                    self.view = View::Three;
                }
                Err(error) => self.record_import_problem("Пакет механик", error),
            },
            None => {}
        }
        true
    }
}

fn paint_welcome_background(painter: &egui::Painter, rect: egui::Rect) {
    const X_SEGMENTS: u32 = 14;
    const Y_SEGMENTS: u32 = 10;
    let mut mesh = egui::Mesh::default();
    for row in 0..=Y_SEGMENTS {
        for column in 0..=X_SEGMENTS {
            let x = column as f32 / X_SEGMENTS as f32;
            let y = row as f32 / Y_SEGMENTS as f32;
            let glow = |cx: f32, cy: f32, spread: f32| {
                (-((x - cx).powi(2) + (y - cy).powi(2)) / spread).exp()
            };
            let upper = glow(0.76, 0.06, 0.14);
            let lower = glow(0.13, 0.88, 0.09);
            mesh.colored_vertex(
                rect.min + egui::vec2(x * rect.width(), y * rect.height()),
                egui::Color32::from_rgb(
                    BACKDROP.r() + (26.0 * upper + 11.0 * lower) as u8,
                    BACKDROP.g() + (9.0 * upper + 4.0 * lower) as u8,
                    BACKDROP.b() + (41.0 * upper + 22.0 * lower) as u8,
                ),
            );
        }
    }
    for row in 0..Y_SEGMENTS {
        for column in 0..X_SEGMENTS {
            let a = row * (X_SEGMENTS + 1) + column;
            let b = a + X_SEGMENTS + 1;
            mesh.add_triangle(a, a + 1, b);
            mesh.add_triangle(a + 1, b + 1, b);
        }
    }
    painter.add(mesh);
}

fn welcome_heading(ui: &mut egui::Ui, page: Page) {
    let (title, hint) = match page {
        Page::Home => ("Студия игр", "Редактор 2D- и 3D-игр"),
        Page::Create => ("Тип проекта", "Выбери, с чего начнётся твоя игра."),
        Page::Examples => (
            "Редактируемые примеры",
            "Открой сцену, изменяй её и сразу проверяй в игре.",
        ),
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        studio_mark(ui, 46.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.label(egui::RichText::new(title).strong().size(29.0));
            ui.label(egui::RichText::new(hint).size(14.0).color(MUTED));
        });
    });
}

fn page_actions(ui: &mut egui::Ui, page: Page, status: &str) -> Option<Choice> {
    let choices: &[(&str, &str, Choice)] = match page {
        Page::Home => &[
            (
                "Создать проект",
                "Пустая сцена, карта сверху или платформер",
                Choice::Go(Page::Create),
            ),
            (
                "Собрать героя",
                "Внешность и одежда в новой 3D-сцене",
                Choice::Character,
            ),
            (
                "Открыть проект",
                "Продолжить работу с сохранённой сценой",
                Choice::Open,
            ),
            (
                "Попробовать пример",
                "Посмотреть, как устроена готовая игра",
                Choice::Go(Page::Examples),
            ),
        ],
        Page::Create => &[
            (
                "3D-сцена",
                "Персонажи и объекты в свободной камере",
                Choice::Empty,
            ),
            (
                "2D сверху",
                "Карта с видом сверху",
                Choice::Map(map2d::Mode::Top),
            ),
            (
                "Платформер",
                "Карта с видом сбоку",
                Choice::Map(map2d::Mode::Side),
            ),
        ],
        Page::Examples => &[
            (
                flagship::NAME,
                "Набережная, разговор и выбор",
                Choice::Flagship,
            ),
            (
                "Мастерская после дождя",
                "Свет, мокрые материалы и диалог",
                Choice::Rain,
            ),
            (meridian::NAME, "Переходы между локациями", Choice::Meridian),
            (
                "Сигнальная тропа · 2D",
                "Карта и условия победы",
                Choice::Trail,
            ),
            (
                "Липовый проезд",
                "Городская сцена и персонажи",
                Choice::Quarter,
            ),
            (
                crate::mechanics_pack::SAMPLE_NAME,
                "Дверь, допуск и задание из JSON",
                Choice::Mechanics,
            ),
        ],
    };

    let mut choice = None;
    let columns = if ui.available_width() >= 550.0 { 2 } else { 1 };
    let tile_width = (ui.available_width() - 10.0 * (columns - 1) as f32) / columns as f32;
    ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
    for (row_no, row) in choices.chunks(columns).enumerate() {
        ui.horizontal_top(|ui| {
            for (column, &(title, hint, command)) in row.iter().enumerate() {
                if action_tile(
                    ui,
                    tile_width,
                    title,
                    hint,
                    command,
                    row_no == 0 && column == 0,
                ) {
                    choice = Some(command);
                }
            }
        });
    }
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        if !matches!(page, Page::Home) && ui.button("Назад").clicked() {
            choice = Some(Choice::Go(Page::Home));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.weak(env!("CARGO_PKG_VERSION"));
        });
    });
    if status.starts_with("Не удалось") {
        ui.colored_label(ui.visuals().error_fg_color, status);
    }
    choice
}

fn action_tile(
    ui: &mut egui::Ui,
    width: f32,
    title: &str,
    hint: &str,
    choice: Choice,
    primary: bool,
) -> bool {
    let painter = ui.painter();
    let text_width = (width - 91.0).max(1.0);
    let title_color = if primary { BRIGHT } else { TEXT };
    let title_text = painter.layout(
        title.into(),
        egui::FontId::proportional(16.0),
        title_color,
        text_width,
    );
    let hint_color = MUTED;
    let hint_text = painter.layout(
        hint.into(),
        egui::FontId::proportional(13.0),
        hint_color,
        text_width,
    );
    let height = (title_text.size().y + hint_text.size().y + 26.0).max(68.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), title)
    });
    let highlighted = response.hovered() || response.has_focus();
    let border = if highlighted {
        BRIGHT
    } else if primary {
        ACCENT
    } else {
        EDGE
    };
    let fill = match (primary, highlighted) {
        (true, true) => egui::Color32::from_rgb(77, 50, 118),
        (true, false) => SELECTED,
        (false, true) => HOVER,
        (false, false) => SURFACE,
    };
    let painter = ui.painter();
    if primary || highlighted {
        painter.rect_stroke(
            rect.expand(2.0),
            14,
            egui::Stroke::new(2.0_f32, ACCENT.gamma_multiply(0.18)),
            egui::StrokeKind::Inside,
        );
    }
    painter.rect(
        rect,
        12,
        fill,
        egui::Stroke::new(1.0_f32, border),
        egui::StrokeKind::Inside,
    );
    let icon_center = rect.left_center() + egui::vec2(29.0, 0.0);
    let icon_box = egui::Rect::from_center_size(icon_center, egui::Vec2::splat(32.0));
    painter.rect_filled(icon_box, 9, if primary { HOVER } else { PANEL });
    paint_action_icon(painter, icon_center, choice);
    let block_height = title_text.size().y + hint_text.size().y + 4.0;
    let origin = rect.left_center() + egui::vec2(56.0, -block_height * 0.5);
    let hint_origin = origin + egui::vec2(0.0, title_text.size().y + 4.0);
    painter.galley(origin, title_text, title_color);
    painter.galley(hint_origin, hint_text, hint_color);
    painter.text(
        rect.right_center() - egui::vec2(14.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        "›",
        egui::FontId::proportional(20.0),
        ACCENT,
    );
    response.clicked()
}

fn paint_action_icon(painter: &egui::Painter, center: egui::Pos2, choice: Choice) {
    let point = |x, y| center + egui::vec2(x, y);
    let stroke = egui::Stroke::new(1.4_f32, BRIGHT);
    let line = |from: (f32, f32), to: (f32, f32)| {
        painter.line_segment([point(from.0, from.1), point(to.0, to.1)], stroke);
    };
    let polygon = |points: &[(f32, f32)]| {
        painter.add(egui::Shape::closed_line(
            points.iter().map(|&(x, y)| point(x, y)).collect(),
            stroke,
        ));
    };

    match choice {
        Choice::Go(Page::Create) => {
            line((-7.0, 0.0), (7.0, 0.0));
            line((0.0, -7.0), (0.0, 7.0));
        }
        Choice::Open => {
            polygon(&[
                (-8.0, 7.0),
                (-8.0, -6.0),
                (-2.0, -6.0),
                (0.0, -3.0),
                (8.0, -3.0),
                (8.0, 7.0),
            ]);
        }
        Choice::Character => {
            painter.circle_stroke(point(0.0, -4.0), 3.5, stroke);
            painter.add(egui::Shape::line(
                vec![
                    point(-7.0, 8.0),
                    point(-5.0, 3.0),
                    point(5.0, 3.0),
                    point(7.0, 8.0),
                ],
                stroke,
            ));
        }
        Choice::Empty => {
            polygon(&[
                (0.0, -9.0),
                (8.0, -4.0),
                (8.0, 5.0),
                (0.0, 9.0),
                (-8.0, 5.0),
                (-8.0, -4.0),
            ]);
            line((-8.0, -4.0), (0.0, 0.0));
            line((8.0, -4.0), (0.0, 0.0));
            line((0.0, 9.0), (0.0, 0.0));
        }
        Choice::Map(map2d::Mode::Top) => {
            polygon(&[(-8.0, -8.0), (8.0, -8.0), (8.0, 8.0), (-8.0, 8.0)]);
            line((0.0, -8.0), (0.0, 8.0));
            line((-8.0, 0.0), (8.0, 0.0));
        }
        Choice::Map(map2d::Mode::Side) => {
            painter.add(egui::Shape::line(
                vec![
                    point(-9.0, 7.0),
                    point(-3.0, 7.0),
                    point(-3.0, 2.0),
                    point(3.0, 2.0),
                    point(3.0, -4.0),
                    point(9.0, -4.0),
                ],
                stroke,
            ));
            painter.circle_filled(point(-4.0, -5.0), 2.0, BRIGHT);
        }
        _ => polygon(&[(-5.0, -8.0), (8.0, 0.0), (-5.0, 8.0)]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proof_input;

    #[test]
    fn welcome_opens_the_project_picker_and_character_builder_at_both_widths() {
        for size in [[640, 640], [1440, 810]] {
            let context = egui::Context::default();
            crate::configure_context(&context);
            let mut input = proof_input::Driver::with_scale(size, 1.25);
            let mut app = Studio {
                welcome: Some(Page::Home),
                ..Studio::default()
            };
            let frame = |input: &mut proof_input::Driver, app: &mut Studio, events| {
                context.run(input.raw(events), |ctx| {
                    assert!(app.show_welcome(ctx));
                })
            };
            frame(&mut input, &mut app, Vec::new());
            let home = frame(&mut input, &mut app, Vec::new());
            for label in ["Открыть проект", "Попробовать пример"] {
                assert!(proof_input::find_label(&home, label, 0).is_some());
            }
            let create = proof_input::find_label(&home, "Создать проект", 0).unwrap();
            frame(&mut input, &mut app, proof_input::hover(create));
            frame(&mut input, &mut app, proof_input::click(create));
            assert!(matches!(app.welcome, Some(Page::Create)));
            let picker = frame(&mut input, &mut app, Vec::new());
            for label in ["3D-сцена", "2D сверху", "Платформер", "Назад"]
            {
                assert!(proof_input::find_label(&picker, label, 0).is_some());
            }
            let back = proof_input::find_label(&picker, "Назад", 0).unwrap();
            frame(&mut input, &mut app, proof_input::hover(back));
            frame(&mut input, &mut app, proof_input::click(back));
            assert!(matches!(app.welcome, Some(Page::Home)));
            let home = frame(&mut input, &mut app, Vec::new());
            let character = proof_input::find_label(&home, "Собрать героя", 0).unwrap();
            frame(&mut input, &mut app, proof_input::hover(character));
            frame(&mut input, &mut app, proof_input::click(character));
            assert!(app.welcome.is_none());
            assert!(app.human_editor_open);
            assert!(app.scene.avatar.is_none());
            assert!(app.editor_tab == crate::EditorTab::Character);
        }
    }
}
