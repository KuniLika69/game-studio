use crate::{
    Studio,
    playback::{Check, Choice, Cursor, Entry, Intent, Probe, Value, compare},
    rules,
};
use eframe::egui;
use std::path::Path;

#[derive(Default)]
enum Mode {
    #[default]
    Idle,
    Record(Check),
    Run(Cursor),
}

#[derive(Default)]
pub(crate) struct Panel {
    open: bool,
    mode: Mode,
    check: Option<Check>,
    title: String,
    next_mark: String,
    path: String,
    with_cell: bool,
    outcomes: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct ResultSummary {
    pub passed: bool,
    pub messages: Vec<String>,
}

pub(crate) struct Isolated {
    app: Box<Studio>,
    context: egui::Context,
    check: Check,
    cursor: Cursor,
}

impl Isolated {
    pub fn new(source: &Studio, scene: crate::Scene, check: Check) -> Result<Self, String> {
        if scene.tactics.is_some() {
            return Err("Запись маршрута пока не воспроизводит команды пошагового боя.".into());
        }
        let mut app = Studio {
            standalone: true,
            assets: source.assets.clone(),
            view: source.view,
            ..Studio::default()
        };
        app.apply_scene(scene, source.project.clone(), "");
        if !app.can_play() {
            return Err("В этом варианте сцены игровая проверка не запускается.".into());
        }
        let context = egui::Context::default();
        app.start_play(&context, false);
        Ok(Self {
            app: Box::new(app),
            context,
            check,
            cursor: Cursor::default(),
        })
    }

    pub fn step(&mut self) -> Option<ResultSummary> {
        self.app
            .replay_batch(&self.context, &self.check, &mut self.cursor, 48)
    }

    pub fn progress(&self) -> (u32, u32) {
        (self.cursor.tick, self.check.ticks)
    }

    pub(crate) fn packaged(
        package: crate::game_export::GamePackage,
        check: Check,
        initial: Option<crate::scene::Checkpoint>,
    ) -> Result<Self, String> {
        if package.scene.tactics.is_some() {
            return Err("Проверка пошаговых команд ещё недоступна.".into());
        }
        let context = egui::Context::default();
        let mut app = Studio::from_package(package, &context);
        if let Some(state) = initial {
            state.validate_for(&app.chapter_root().linked_scenes)?;
            app.stop();
            app.scene.checkpoint = Some(state);
            app.start_play(&context, true);
        }
        Ok(Self {
            app: Box::new(app),
            context,
            check,
            cursor: Cursor::default(),
        })
    }

    pub(crate) fn checkpoint(&self) -> Result<crate::scene::Checkpoint, String> {
        self.app
            .full_checkpoint()
            .ok_or("Нет активной игры для сохранения.".into())
    }
}

impl Studio {
    pub(super) fn open_playback_tools(&mut self) {
        self.playback.open = true;
    }

    pub(super) fn playback_button(&mut self, ui: &mut egui::Ui) {
        let name = if self.playback.recording() {
            "Проверка записывается"
        } else if self.playback.replaying() {
            "Проверка выполняется"
        } else {
            "Проверки…"
        };
        if ui.button(name).clicked() {
            self.playback.open = !self.playback.open;
        }
    }

    pub(super) fn playback_panel(&mut self, ctx: &egui::Context) {
        if ctx.input(|input| input.key_pressed(egui::Key::F6)) && self.playback.recording() {
            self.add_playback_mark();
        }
        if !self.playback.open {
            return;
        }
        enum Job {
            Start,
            Mark,
            Finish,
            Run,
            Cancel,
            Save,
            Load,
        }
        let mut job = None;
        let ready = self.can_play() && !self.game_running;
        let recording = self.playback.recording();
        let running = self.playback.replaying();
        let check_ready = self.playback.check.is_some();
        egui::Window::new("Проверка прохождения")
            .open(&mut self.playback.open)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_enabled(
                        !recording && !running,
                        egui::TextEdit::singleline(&mut self.playback.title)
                            .hint_text("Имя проверки"),
                    );
                    if ui
                        .add_enabled(
                            ready && !recording && !running,
                            egui::Button::new("Записать проверку"),
                        )
                        .clicked()
                    {
                        job = Some(Job::Start);
                    }
                });
                if recording {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.playback.next_mark)
                                .hint_text("Ожидаемое состояние"),
                        );
                        if ui.button("Отметить состояние · F6").clicked() {
                            job = Some(Job::Mark);
                        }
                    });
                    ui.checkbox(
                        &mut self.playback.with_cell,
                        "Проверять клетку или позицию с округлением до метра",
                    );
                    if ui.button("Завершить запись").clicked() {
                        job = Some(Job::Finish);
                    }
                } else if running {
                    if ui.button("Прервать повтор").clicked() {
                        job = Some(Job::Cancel);
                    }
                } else {
                    if let Some(check) = &mut self.playback.check {
                        ui.label(format!(
                            "{} · {} шагов · {} ожиданий",
                            check.title,
                            check.ticks,
                            check.expectations.len()
                        ));
                        for (index, expected) in check.expectations.iter_mut().enumerate() {
                            ui.push_id(index, |ui| {
                                ui.collapsing(expected.name.clone(), |ui| {
                                    for (index, entry) in expected.state.iter_mut().enumerate() {
                                        ui.push_id(index, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(&entry.label);
                                                match &mut entry.value {
                                                    Value::Number(value) => {
                                                        ui.add(egui::DragValue::new(value));
                                                    }
                                                    Value::Switch(value) => {
                                                        ui.checkbox(value, "Ожидается");
                                                    }
                                                    Value::Cell(cell) => {
                                                        for coord in cell {
                                                            ui.add(egui::DragValue::new(coord));
                                                        }
                                                    }
                                                    Value::Text(value) => {
                                                        ui.add(
                                                            egui::TextEdit::singleline(value)
                                                                .char_limit(80),
                                                        );
                                                    }
                                                    Value::Inventory(ids) => {
                                                        ui.weak(format!("{} ключей", ids.len()));
                                                    }
                                                }
                                            })
                                        });
                                    }
                                })
                            });
                        }
                    }
                    if ui
                        .add_enabled(
                            ready && check_ready,
                            egui::Button::new("Повторить проверку с начала"),
                        )
                        .clicked()
                    {
                        job = Some(Job::Run);
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.playback.path)
                            .desired_width(f32::INFINITY)
                            .hint_text("Файл проверки .check.json"),
                    );
                    ui.horizontal(|ui| {
                        let supplied = !self.playback.path.trim().is_empty();
                        if ui
                            .add_enabled(
                                supplied && check_ready,
                                egui::Button::new("Сохранить проверку"),
                            )
                            .clicked()
                        {
                            job = Some(Job::Save);
                        }
                        if ui
                            .add_enabled(supplied, egui::Button::new("Открыть проверку"))
                            .clicked()
                        {
                            job = Some(Job::Load);
                        }
                    });
                }
                for line in &self.playback.outcomes {
                    ui.label(line);
                }
            });
        match job {
            Some(Job::Start) => {
                self.start_play(ctx, false);
                let name = self
                    .playback
                    .title
                    .trim()
                    .chars()
                    .take(100)
                    .collect::<String>();
                self.playback.mode = Mode::Record(Check::new(if name.is_empty() {
                    "Проверка сцены".into()
                } else {
                    name
                }));
                self.playback.outcomes.clear();
                self.playback.open = false;
            }
            Some(Job::Mark) => self.add_playback_mark(),
            Some(Job::Finish) => self.stop(),
            Some(Job::Run) => {
                self.start_play(ctx, false);
                self.playback.mode = Mode::Run(Cursor::default());
                self.playback.outcomes.clear();
            }
            Some(Job::Cancel) => self.stop_playback(vec!["Проверка прервана.".into()]),
            Some(Job::Save) => {
                if let Some(project) = &self.playback.check {
                    self.playback.outcomes = vec![
                        project
                            .save(Path::new(self.playback.path.trim()))
                            .map_or_else(|e| e, |()| "Проверка сохранена.".into()),
                    ];
                }
            }
            Some(Job::Load) => match Check::load(Path::new(self.playback.path.trim())) {
                Ok(check) => {
                    self.playback.check = Some(check);
                    self.playback.outcomes = vec!["Проверка открыта.".into()];
                }
                Err(error) => self.playback.outcomes = vec![error],
            },
            None => {}
        }
    }

    pub(super) fn advance_playback(&mut self, ctx: &egui::Context) -> bool {
        if !self.playback.replaying() {
            return false;
        }
        let Mode::Run(mut cursor) = std::mem::take(&mut self.playback.mode) else {
            return false;
        };
        let Some(check) = self.playback.check.take() else {
            return true;
        };
        let finished = self.replay_batch(ctx, &check, &mut cursor, 8);
        self.playback.check = Some(check);
        if let Some(result) = finished {
            self.stop_playback(result.messages);
        } else {
            self.playback.mode = Mode::Run(cursor);
        }
        true
    }

    fn replay_batch(
        &mut self,
        ctx: &egui::Context,
        check: &Check,
        cursor: &mut Cursor,
        step_budget: usize,
    ) -> Option<ResultSummary> {
        // Several fixed recorded ticks per editor frame complete long routes without requiring a live input device.
        for _ in 0..step_budget {
            let Some(intent) = cursor.step(check) else {
                return Some(ResultSummary {
                    passed: true,
                    messages: vec![format!(
                        "Проверка пройдена · {} ожиданий, {} шагов.",
                        check.expectations.len(),
                        check.ticks
                    )],
                });
            };
            self.walk_target = intent.target;
            self.advance_intent(ctx, intent);
            self.check_scene_transition(ctx);
            while let Some(choice) = cursor.next_choice(check) {
                if !self.replay_dialog_choice(choice) {
                    return Some(ResultSummary {
                        passed: false,
                        messages: vec![format!(
                            "Шаг {} · Ожидался ответ {} в реплике {}, NPC {}.",
                            cursor.tick, choice.option, choice.node, choice.npc
                        )],
                    });
                }
                self.check_scene_transition(ctx);
            }
            while let Some(mark) = cursor.next_mark(check) {
                let differences = compare(mark, |probe| self.read_probe(probe));
                if !differences.is_empty() {
                    let mut messages = vec![format!(
                        "Первое расхождение · {} · шаг {}",
                        mark.name, mark.tick
                    )];
                    for row in differences {
                        messages.push(format!(
                            "{} · ожидалось {}, получено {}",
                            row.entry.label,
                            value_text(&row.entry.value),
                            row.actual
                                .as_ref()
                                .map_or_else(|| "ссылка отсутствует".into(), value_text)
                        ));
                    }
                    for trace in self.rule_session.trace().rev().take(5) {
                        if let Some((label, _)) = self.rule_trace_label(trace) {
                            messages.push(label);
                        }
                    }
                    return Some(ResultSummary {
                        passed: false,
                        messages,
                    });
                }
            }
        }
        None
    }

    pub(super) fn record_playback_intent(&mut self, intent: Intent) {
        if let Mode::Record(check) = &mut self.playback.mode
            && let Err(error) = check.record(intent)
        {
            self.playback.outcomes = vec![error];
            self.finish_playback_record();
        }
    }

    pub(super) fn record_playback_answer(&mut self, event: rules::Event) {
        if let Mode::Record(check) = &mut self.playback.mode
            && let rules::Event::Dialogue {
                npc_id,
                node_id,
                choice_id,
            } = event
        {
            check.answer(Choice {
                npc: npc_id,
                node: node_id,
                option: choice_id,
            });
        }
    }

    pub(super) fn finish_playback_record(&mut self) {
        if !self.playback.recording() {
            return;
        }
        if let Mode::Record(check) = std::mem::take(&mut self.playback.mode) {
            if check.expectations.is_empty() {
                self.playback.outcomes =
                    vec!["Добавь отметку состояния клавишей F6 и запиши маршрут снова.".into()];
            } else {
                self.playback.outcomes =
                    vec![format!("Записано {} ожиданий.", check.expectations.len())];
                self.playback.check = Some(check);
            }
            self.playback.open = true;
        }
    }

    fn stop_playback(&mut self, outcomes: Vec<String>) {
        let checkpoint = self.chapter_root().checkpoint.clone();
        self.stop();
        self.scene.checkpoint = checkpoint;
        self.playback.mode = Mode::Idle;
        self.playback.outcomes = outcomes;
        self.playback.open = true;
    }

    fn add_playback_mark(&mut self) {
        let state = self.playback_state();
        let Mode::Record(check) = &mut self.playback.mode else {
            return;
        };
        let name = std::mem::take(&mut self.playback.next_mark);
        let name = if name.trim().is_empty() {
            format!("Отметка {}", check.expectations.len() + 1)
        } else {
            name.trim().chars().take(100).collect()
        };
        self.status = match check.expect(name.clone(), state) {
            Ok(()) => format!("Ожидание «{name}» записано."),
            Err(err) => err,
        };
        self.playback.outcomes = vec![self.status.clone()];
    }

    fn replay_dialog_choice(&mut self, choice: Choice) -> bool {
        if self.story_session.active_npc() != Some(choice.npc) {
            return false;
        }
        let Some((_, node)) = self.story_session.current(&self.scene.story) else {
            return false;
        };
        if node.id != choice.node {
            return false;
        }
        let Some(index) = node
            .choices
            .iter()
            .position(|option| option.id == choice.option)
        else {
            return false;
        };
        if let Some(event) = self.story_session.choose_event(&self.scene.story, index) {
            self.dispatch_rule(event);
            true
        } else {
            false
        }
    }

    fn playback_state(&self) -> Vec<Entry> {
        let mut result = Vec::new();
        let mut add = |probe, name: &str| {
            if let Some(value) = self.read_probe(probe) {
                result.push(Entry {
                    probe,
                    label: name.into(),
                    value,
                });
            }
        };
        add(Probe::Keys, "Инвентарь ключей");
        if !self.chapter_root().linked_scenes.is_empty() {
            add(Probe::Location, "Локация");
        }
        if self.playback.with_cell {
            add(Probe::Cell, "Координаты игрока");
        }
        for row in &self.scene.rules.counters {
            add(Probe::Counter(row.id), &row.name);
        }
        for row in &self.scene.rules.flags {
            add(Probe::Flag(row.id), &row.name);
        }
        for row in &self.scene.rules.timers {
            add(Probe::Timer(row.id), &row.name);
        }
        for row in &self.scene.world.objects {
            add(Probe::Object(row.id), &row.name);
        }
        for house in &self.scene.world.buildings {
            for door in house.doors() {
                add(Probe::Door(door.id), &door.name);
            }
        }
        for quest in &self.scene.story.quests {
            add(Probe::Quest(quest.id), &quest.title);
        }
        result
    }

    fn read_probe(&self, probe: Probe) -> Option<Value> {
        let rules = &self.scene.rules;
        let runtime = &self.rule_session;
        match probe {
            Probe::Counter(id) => runtime.counter(rules, id).map(Value::Number),
            Probe::Flag(id) => runtime.flag(rules, id).map(Value::Switch),
            Probe::Timer(id) => rules
                .timer(id)
                .map(|_| Value::Switch(runtime.timer_remaining(id).is_some())),
            Probe::Door(id) => self
                .scene
                .world
                .door(id)
                .map(|_| Value::Switch(!self.scene.world.door_locked(&self.interactions, id))),
            Probe::Object(id) => self
                .scene
                .world
                .objects
                .iter()
                .find(|item| item.id == id)
                .map(|_| Value::Switch(runtime.object_enabled(id))),
            Probe::Quest(id) => self.scene.story.quest(id).map(|quest| {
                if matches!(quest.objective, crate::story::Objective::Rules { .. }) {
                    Value::Number(i32::from(runtime.quest_stage(id).unwrap_or(0)))
                } else {
                    Value::Text(self.story_session.progress_name(id).into())
                }
            }),
            Probe::Keys => {
                let mut keys = self.interactions.key_ids().collect::<Vec<_>>();
                keys.sort_unstable();
                Some(Value::Inventory(keys))
            }
            Probe::Cell => {
                let [x, _, z] = self.game_position().to_array();
                Some(Value::Cell([x.floor() as i32, z.floor() as i32]))
            }
            Probe::Location => Some(Value::Number(
                self.link_playthrough
                    .as_ref()
                    .map_or(crate::linked_scenes::ROOT_SCENE_ID, |play| play.active)
                    as i32,
            )),
            Probe::AnswerText { npc, node, option } => self
                .scene
                .story
                .answer(npc, node, option)
                .map(|choice| Value::Text(choice.text.clone())),
        }
    }
}

impl Panel {
    pub(crate) fn check(&self) -> Option<&Check> {
        self.check.as_ref()
    }

    pub fn recording(&self) -> bool {
        matches!(self.mode, Mode::Record(_))
    }
    pub fn replaying(&self) -> bool {
        matches!(self.mode, Mode::Run(_))
    }

    pub(crate) fn outcomes(&self) -> &[String] {
        &self.outcomes
    }

    pub fn abandon_run(&mut self) {
        if self.replaying() {
            self.mode = Mode::Idle;
            self.outcomes = vec!["Проверка прервана.".into()];
        }
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::Number(value) => value.to_string(),
        Value::Switch(value) => if *value { "да" } else { "нет" }.into(),
        Value::Cell([x, z]) => format!("({x}, {z})"),
        Value::Inventory(ids) => format!("{} ключей ({ids:?})", ids.len()),
        Value::Text(value) => value.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_comparison_uses_the_real_rule_runtime_without_editing_source() {
        let mut source = Studio::default();
        let mut scene = crate::Scene::empty();
        scene.map_2d = Some(crate::map2d::Map::new(crate::map2d::Mode::Top));
        let counter = scene.rules.add_counter().unwrap();
        scene.rules.add_rule().unwrap();
        let rule = scene.rules.rules.last_mut().unwrap();
        rule.event = Some(rules::Event::GameAction {
            action: rules::GameAction::Mark,
        });
        rule.actions.push(rules::Action::AddCounter {
            counter_id: counter,
            delta: 1,
        });
        source.set_scene(scene.clone(), None, "");
        let mut check = Check::new("Одна отметка".into());
        check
            .record(Intent {
                mark: true,
                dt: 1.0 / 60.0,
                ..Intent::default()
            })
            .unwrap();
        check
            .expect(
                "После отметки".into(),
                vec![Entry {
                    probe: Probe::Counter(counter),
                    label: "Отметки".into(),
                    value: Value::Number(1),
                }],
            )
            .unwrap();
        let mut variant = scene.clone();
        variant.rules.rules[0].enabled = false;
        let mut original = Isolated::new(&source, scene.clone(), check.clone()).unwrap();
        let mut patched = Isolated::new(&source, variant, check).unwrap();
        assert!(original.step().unwrap().passed);
        let difference = patched.step().unwrap();
        assert!(!difference.passed);
        assert!(difference.messages[1].contains("ожидалось 1, получено 0"));
        assert_eq!(source.scene, scene);
        assert!(!source.game_running);
    }
}
