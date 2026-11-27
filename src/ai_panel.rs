use crate::{ai, scene::Scene, sprite};
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use std::sync::mpsc::{self, Receiver, TryRecvError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Provider {
    Strata,
    LlamaCpp,
    Ollama,
    LmStudio,
    Claude,
    Cloud,
}

impl Provider {
    const ALL: [Self; 6] = [
        Self::Strata,
        Self::LlamaCpp,
        Self::Ollama,
        Self::LmStudio,
        Self::Claude,
        Self::Cloud,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Strata => "Strata",
            Self::LlamaCpp => "llama.cpp",
            Self::Ollama => "Ollama",
            Self::LmStudio => "LM Studio",
            Self::Claude => "Claude (Anthropic)",
            Self::Cloud => "Облачный API",
        }
    }

    fn endpoint(self) -> &'static str {
        match self {
            Self::Strata => "http://127.0.0.1:8082/v1",
            Self::LlamaCpp => "http://127.0.0.1:8080/v1",
            Self::Ollama => "http://127.0.0.1:11434/v1",
            Self::LmStudio => "http://127.0.0.1:1234/v1",
            Self::Claude => "https://api.anthropic.com/v1",
            Self::Cloud => "",
        }
    }

    fn model(self) -> &'static str {
        match self {
            Self::Strata => "qwen3.8-flash-next-iq2_xs",
            Self::Claude => ai::CLAUDE_DEFAULT_MODEL,
            _ => "",
        }
    }
}

pub struct AiPanel {
    provider: Provider,
    endpoint: String,
    model: String,
    key: String,
    prompt: String,
    pending: Option<Receiver<Result<ai::Draft, String>>>,
    draft: Option<Scene>,
    preview: Option<TextureHandle>,
    error: Option<String>,
}

impl Default for AiPanel {
    fn default() -> Self {
        let provider = Provider::Strata;
        Self {
            provider,
            endpoint: provider.endpoint().into(),
            model: provider.model().into(),
            key: String::new(),
            prompt: "Небольшой робот-разведчик. Графитовый корпус, бирюзовый визор и две оранжевые руки.".into(),
            pending: None,
            draft: None,
            preview: None,
            error: None,
        }
    }
}

impl AiPanel {
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool) -> Option<Scene> {
        self.receive(ctx);
        if !*open {
            return None;
        }
        let mut apply = false;
        egui::Window::new("Нейросети")
            .default_width(480.0)
            .open(open)
            .collapsible(false)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height(650.0).show(ui, |ui| {
                    ui.label("Опиши персонажа. Модель предложит набор деталей, которые можно редактировать.");
                    ui.add_enabled_ui(self.pending.is_none(), |ui| {
                        self.connection_fields(ui);
                        ui.label("Описание персонажа");
                        ui.add(
                            egui::TextEdit::multiline(&mut self.prompt)
                                .desired_width(f32::INFINITY)
                                .desired_rows(3)
                                .char_limit(3000),
                        );
                    });
                    if self.pending.is_some() {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Модель готовит черновик…");
                        });
                    } else {
                        let enabled = !self.endpoint.trim().is_empty()
                            && !self.model.trim().is_empty()
                            && !self.prompt.trim().is_empty();
                        if ui.add_enabled(enabled, egui::Button::new("Создать черновик")).clicked() {
                            self.start(ctx);
                        }
                    }
                    if let Some(error) = &self.error {
                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                    }
                    if let Some(scene) = &self.draft {
                        ui.separator();
                        ui.heading(format!("Черновик · {} деталей", scene.parts.len()));
                        if let Some(preview) = &self.preview {
                            ui.add(egui::Image::new(preview).fit_to_exact_size(egui::Vec2::splat(176.0)));
                        }
                        ui.label("Черновик заменит персонажа и его анимацию. Окружение сохранится.");
                        ui.horizontal(|ui| {
                            if ui.button("Применить персонажа").clicked() {
                                apply = true;
                            }
                            if ui.button("Отбросить черновик").clicked() {
                                self.draft = None;
                                self.preview = None;
                            }
                        });
                    }
                });
            });
        if apply {
            *open = false;
            self.preview = None;
            return self.draft.take();
        }
        None
    }

    pub(crate) fn connection(&self) -> (String, String, String) {
        (self.endpoint.clone(), self.model.clone(), self.key.clone())
    }

    pub(crate) fn connection_fields(&mut self, ui: &mut egui::Ui) {
        let previous = self.provider;
        egui::ComboBox::from_label("Сервер")
            .selected_text(self.provider.title())
            .show_ui(ui, |ui| {
                for provider in Provider::ALL {
                    ui.selectable_value(&mut self.provider, provider, provider.title());
                }
            });
        if self.provider != previous {
            self.endpoint = self.provider.endpoint().into();
            self.model = self.provider.model().into();
            self.key.clear();
            self.error = None;
        }
        if self.provider == Provider::Claude {
            ui.label(
                "Ключ создаётся на console.anthropic.com. API оплачивается по токенам, отдельно от подписки Claude Pro. Пустое поле ключа — берётся ANTHROPIC_API_KEY.",
            );
        } else if self.provider == Provider::Cloud {
            ui.label(
                "Запрос уйдёт на указанный HTTPS-адрес. Для платного API действует тариф сервиса.",
            );
        } else {
            ui.label("Запусти сервер с текстовой моделью и укажи его фактический порт.");
        }
        ui.label("Адрес API");
        ui.add(
            egui::TextEdit::singleline(&mut self.endpoint)
                .desired_width(f32::INFINITY)
                .hint_text("https://api.example.com/v1"),
        );
        ui.label("Имя модели");
        ui.add(
            egui::TextEdit::singleline(&mut self.model)
                .desired_width(f32::INFINITY)
                .hint_text("Идентификатор модели на сервере"),
        );
        ui.label("API-ключ, если сервер его запрашивает");
        ui.add(
            egui::TextEdit::singleline(&mut self.key)
                .password(true)
                .desired_width(f32::INFINITY),
        );
    }

    fn start(&mut self, ctx: &egui::Context) {
        self.error = None;
        self.draft = None;
        self.preview = None;
        let endpoint = self.endpoint.clone();
        let model = self.model.clone();
        let key = self.key.clone();
        let prompt = self.prompt.clone();
        let context = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        self.pending = Some(receiver);
        std::thread::spawn(move || {
            let _ = sender.send(ai::generate(
                endpoint.trim(),
                model.trim(),
                key.trim(),
                prompt.trim(),
            ));
            context.request_repaint();
        });
    }

    fn receive(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending else { return };
        let result = match pending.try_recv() {
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("Не удалось завершить запрос к модели.".into()),
            Ok(result) => result,
        };
        self.pending = None;
        match result {
            Err(error) => self.error = Some(error),
            Ok(draft) => {
                let mut scene = Scene::empty();
                for source in draft.parts {
                    scene.add_part(source.shape);
                    if let Some(part) = scene.parts.last_mut() {
                        part.name = source.name;
                        part.position = source.position;
                        part.rotation = source.rotation;
                        part.size = source.size;
                        part.color = source.color;
                    }
                }
                let pixels = sprite::rasterize(
                    &scene.parts,
                    0,
                    &[],
                    sprite::scale_for(&scene.parts),
                    0.0,
                    None,
                );
                self.preview = Some(ctx.load_texture(
                    "ai_draft",
                    ColorImage::from_rgba_unmultiplied([sprite::SIDE; 2], &pixels),
                    TextureOptions::NEAREST,
                ));
                self.draft = Some(scene);
            }
        }
    }
}
