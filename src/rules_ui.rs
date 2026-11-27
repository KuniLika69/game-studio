use std::{hash::Hash, mem::discriminant, path::Path};

use crate::{
    ImportError,
    environment::WeatherPreset,
    simulation_rules::{Command as SimCommand, Contact, Signal},
};
use eframe::egui;

use crate::{
    buildings::Building,
    effect_rules::{self, Command as EffectCommand, Origin as EffectOrigin},
    effects::PresetId,
    rules::{
        self, Action, Binding, BindingValue, Comparison, Condition, Event, GameAction,
        MarkBindings, MarkSource, MarkTarget, PortType, Rule, RuleSet,
    },
    scene::Scene,
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Section {
    #[default]
    Rules,
    Zones,
    Flags,
    Counters,
    Timers,
    Library,
}

#[derive(Default)]
pub struct Editor {
    section: Section,
    rule: Option<u32>,
    zone: Option<u32>,
    flag: Option<u32>,
    counter: Option<u32>,
    timer: Option<u32>,
    template: Option<u32>,
    rule_template: Option<u32>,
    binding_template: Option<u32>,
    bindings: Vec<Binding>,
    sources: [Option<MarkSource>; 2],
    target: Option<MarkTarget>,
    library_path: String,
    library_status: String,
    import_error: Option<ImportError>,
}

impl Editor {
    pub(crate) fn select_rule(&mut self, id: u32) {
        self.section = Section::Rules;
        self.rule = Some(id);
    }

    pub(crate) fn select_zone(&mut self, id: u32) {
        self.section = Section::Zones;
        self.zone = Some(id);
    }

    pub(crate) fn broken_rules(scene: &Scene) -> Vec<u32> {
        let catalog = Catalog::new(scene);
        scene
            .rules
            .rules
            .iter()
            .filter(|rule| {
                rule.enabled && (!rule.bindings_ready() || !catalog.bindings_present(rule))
            })
            .map(|rule| rule.id)
            .collect()
    }

    pub fn show(&mut self, ui: &mut egui::Ui, scene: &mut Scene) -> bool {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.section, Section::Rules, "Сценарии");
            ui.selectable_value(&mut self.section, Section::Zones, "Зоны");
            ui.selectable_value(&mut self.section, Section::Flags, "Флаги");
            ui.selectable_value(&mut self.section, Section::Counters, "Счётчики");
            ui.selectable_value(&mut self.section, Section::Timers, "Таймеры");
            ui.selectable_value(&mut self.section, Section::Library, "Библиотека");
        });
        ui.separator();
        match self.section {
            Section::Rules => self.rules(ui, scene),
            Section::Zones => self.zones(ui, &mut scene.rules, &scene.world.buildings),
            Section::Flags => self.flags(ui, &mut scene.rules),
            Section::Counters => self.counters(ui, &mut scene.rules),
            Section::Timers => self.timers(ui, &mut scene.rules),
            Section::Library => self.library(ui, scene),
        }
    }

    pub fn selected_zone(&self) -> Option<u32> {
        (self.section == Section::Zones)
            .then_some(self.zone)
            .flatten()
    }

    pub(crate) fn take_import_error(&mut self) -> Option<ImportError> {
        self.import_error.take()
    }

    #[cfg(test)]
    fn set_library_path(&mut self, path: &Path) {
        self.library_path = path.to_string_lossy().into_owned();
    }

    fn library(&mut self, ui: &mut egui::Ui, scene: &mut Scene) -> bool {
        let mut changed = false;
        ui.label("Путь к библиотеке .rules.json");
        ui.add(
            egui::TextEdit::singleline(&mut self.library_path)
                .hint_text("Путь к файлу .rules.json"),
        );
        ui.horizontal_wrapped(|ui| {
            let path = Path::new(self.library_path.trim());
            if add_button(
                ui,
                "Сохранить библиотеку",
                !self.library_path.trim().is_empty(),
            ) {
                self.library_status = match scene.rules.save_library(path) {
                    Ok(()) => "Библиотека сохранена".into(),
                    Err(error) => error,
                };
            }
            if add_button(
                ui,
                "Открыть библиотеку",
                !self.library_path.trim().is_empty(),
            ) {
                self.library_status = match scene.rules.load_library(path) {
                    Ok(added) => {
                        if added > 0 {
                            self.template =
                                scene.rules.templates.last().map(|template| template.id);
                            self.rule_template = scene
                                .rules
                                .rule_templates
                                .last()
                                .map(|template| template.id);
                        }
                        changed |= added > 0;
                        format!("Шаблонов добавлено · {added}")
                    }
                    Err(error) => {
                        let status = error.to_string();
                        self.import_error = Some(error);
                        status
                    }
                };
            }
        });
        ui.separator();
        if add_button(
            ui,
            "Сохранить шаблон",
            scene.rules.templates.len() < rules::MAX_TEMPLATES,
        ) && let Some(id) = scene.rules.add_template()
        {
            self.template = Some(id);
            self.library_status = "Шаблон двух разных отметок сохранён в проекте".into();
            changed = true;
        }
        let entries = scene
            .rules
            .templates
            .iter()
            .map(|template| (template.id, template.name.as_str()));
        if let Some(id) = select_entry(ui, "template_entry", &mut self.template, entries, "Шаблон")
        {
            let catalog = Catalog::new(scene);
            if let Some(template) = scene
                .rules
                .templates
                .iter_mut()
                .find(|template| template.id == id)
            {
                changed |= name_field(ui, &mut template.name, "Шаблон", id);
                ui.label("Название счётчика");
                changed |= name_field(ui, &mut template.counter_name, "Отметки", id);
                for (index, name) in template.mark_names.iter_mut().enumerate() {
                    ui.label(if index == 0 {
                        "Подпись первой отметки"
                    } else {
                        "Подпись второй отметки"
                    });
                    changed |= name_field(ui, name, "Отметка", index as u32 + 1);
                }
            }
            ui.separator();
            let sources = catalog.mark_sources();
            for (index, selected) in self.sources.iter_mut().enumerate() {
                ui.push_id(("mark_source", index), |ui| {
                    choose_target(
                        ui,
                        if index == 0 {
                            "Первая отметка"
                        } else {
                            "Вторая отметка"
                        },
                        selected,
                        &sources,
                    );
                });
            }
            let targets = catalog.mark_targets(self.target);
            choose_target(ui, "Что открыть или изменить", &mut self.target, &targets);
            if let Some(MarkTarget::Quest { stage, .. }) = &mut self.target {
                ui.add(
                    egui::DragValue::new(stage)
                        .prefix("Этап ")
                        .range(0..=rules::MAX_STAGE),
                );
            }
            let [first, second] = self.sources;
            let binding = first.zip(second).zip(self.target);
            if add_button(ui, "Применить шаблон", binding.is_some())
                && let Some(((first, second), target)) = binding
            {
                let bindings = MarkBindings {
                    first_mark: first,
                    second_mark: second,
                    result: target,
                };
                self.library_status =
                    match scene
                        .rules
                        .apply_mark_package(id, bindings, &scene.world, &scene.story)
                    {
                        Ok(counter) => {
                            self.counter = Some(counter);
                            changed = true;
                            "Созданы счётчик и правила для двух отметок".into()
                        }
                        Err(error) => error,
                    };
            }
            if ui.small_button("Удалить шаблон").clicked() {
                changed |= scene.rules.remove_template(id);
                self.template = None;
            }
        }
        if !scene.rules.rule_templates.is_empty() {
            ui.separator();
            ui.strong("Сохранённые правила");
            let entries = scene
                .rules
                .rule_templates
                .iter()
                .map(|entry| (entry.id, entry.name.as_str()));
            if let Some(id) = select_entry(
                ui,
                "portable_rule",
                &mut self.rule_template,
                entries,
                "Правило из библиотеки",
            ) {
                changed |= self.named_bindings(ui, scene, id);
                if add_button(
                    ui,
                    "Добавить черновик правила",
                    scene.rules.rules.len() < rules::MAX_RULES,
                ) {
                    self.library_status = match scene.rules.apply_rule_template(id) {
                        Ok(rule_id) => {
                            self.rule = Some(rule_id);
                            self.section = Section::Rules;
                            changed = true;
                            "Черновик добавлен. Назначь ссылки в правиле.".into()
                        }
                        Err(error) => error,
                    };
                }
                if ui.small_button("Удалить сохранённое правило").clicked()
                {
                    changed |= scene.rules.remove_rule_template(id);
                    self.rule_template = None;
                }
            }
        }
        if !self.library_status.is_empty() {
            ui.label(&self.library_status);
        }
        changed
    }

    fn named_bindings(&mut self, ui: &mut egui::Ui, scene: &mut Scene, id: u32) -> bool {
        if self.binding_template != Some(id) {
            self.binding_template = Some(id);
            self.bindings.clear();
        }
        let catalog = Catalog::new(scene);
        let Some(template) = scene
            .rules
            .rule_templates
            .iter_mut()
            .find(|entry| entry.id == id)
        else {
            return false;
        };
        let ports = template.ports();
        let mut changed = false;
        let apply = if ports.is_empty() {
            false
        } else {
            ui.collapsing("Именованные привязки", |ui| {
                for port in &ports {
                    ui.push_id(port.port, |ui| {
                        ui.label(format!("{} · {}", port.port.caption(), port.kind.name()));
                        let mut name = port.name.clone();
                        if ui
                            .add(egui::TextEdit::singleline(&mut name).char_limit(64))
                            .changed()
                        {
                            changed |= template.name_port(port.port, name);
                        }
                        let options = catalog.binding_values(port.kind);
                        let mut selected = self
                            .bindings
                            .iter()
                            .find(|binding| binding.port == port.port)
                            .map(|binding| binding.value);
                        if choose(
                            ui,
                            "package_target",
                            &mut selected,
                            options
                                .iter()
                                .map(|(value, label)| (*value, label.as_str())),
                        ) {
                            self.bindings.retain(|binding| binding.port != port.port);
                            if let Some(value) = selected {
                                self.bindings.push(Binding {
                                    port: port.port,
                                    value,
                                });
                            }
                        }
                    });
                }
                add_button(
                    ui,
                    "Создать связанное правило",
                    self.bindings.len() == ports.len(),
                )
            })
            .body_returned
            .unwrap_or(false)
        };
        if apply {
            self.library_status = match scene.rules.apply_bound_rule(
                id,
                &self.bindings,
                &scene.world,
                &scene.story,
                &scene.linked_scenes,
                &scene.effects,
            ) {
                Ok(rule) => {
                    self.rule = Some(rule);
                    self.section = Section::Rules;
                    changed = true;
                    "Связанное правило создано. Проверь настройки и включи его.".into()
                }
                Err(error) => error,
            };
        }
        changed
    }

    fn counters(&mut self, ui: &mut egui::Ui, rules: &mut RuleSet) -> bool {
        let mut changed = false;
        if add_button(ui, "+ Счётчик", rules.counters.len() < rules::MAX_COUNTERS)
            && let Some(id) = rules.add_counter()
        {
            self.counter = Some(id);
            changed = true;
        }
        let entries = rules
            .counters
            .iter()
            .map(|counter| (counter.id, counter.name.as_str()));
        let Some(id) = select_entry(ui, "counter_entry", &mut self.counter, entries, "Счётчик")
        else {
            return changed;
        };
        if let Some(counter) = rules.counters.iter_mut().find(|counter| counter.id == id) {
            changed |= name_field(ui, &mut counter.name, "Счётчик", id);
            changed |= integer(ui, "Начальное значение ", &mut counter.initial);
        }
        if ui.small_button("Удалить счётчик").clicked() {
            changed |= rules.remove_counter(id);
            self.counter = None;
        }
        changed
    }

    fn timers(&mut self, ui: &mut egui::Ui, rules: &mut RuleSet) -> bool {
        let mut changed = false;
        if add_button(ui, "+ Таймер", rules.timers.len() < rules::MAX_TIMERS)
            && let Some(id) = rules.add_timer()
        {
            self.timer = Some(id);
            changed = true;
        }
        let entries = rules
            .timers
            .iter()
            .map(|timer| (timer.id, timer.name.as_str()));
        let Some(id) = select_entry(ui, "timer_entry", &mut self.timer, entries, "Таймер")
        else {
            return changed;
        };
        if let Some(timer) = rules.timers.iter_mut().find(|timer| timer.id == id) {
            changed |= name_field(ui, &mut timer.name, "Таймер", id);
            changed |= ui
                .add(
                    egui::DragValue::new(&mut timer.seconds)
                        .prefix("Секунд ")
                        .speed(0.1)
                        .range(0.05..=rules::MAX_TIMER_SECONDS),
                )
                .changed();
        }
        if ui.small_button("Удалить таймер").clicked() {
            changed |= rules.remove_timer(id);
            self.timer = None;
        }
        changed
    }

    fn rules(&mut self, ui: &mut egui::Ui, scene: &mut Scene) -> bool {
        let mut changed = false;
        if add_button(ui, "+ Правило", scene.rules.rules.len() < rules::MAX_RULES)
            && let Some(id) = scene.rules.add_rule()
        {
            self.rule = Some(id);
            changed = true;
        }
        let choices = scene
            .rules
            .rules
            .iter()
            .map(|rule| (rule.id, rule.name.as_str()));
        let Some(id) = select_entry(ui, "rule_entry", &mut self.rule, choices, "Правило")
        else {
            ui.label("Добавь правило, чтобы связать событие с действием.");
            return changed;
        };
        let catalog = Catalog::new(scene);
        let Some(rule) = scene.rules.rules.iter_mut().find(|rule| rule.id == id) else {
            return changed;
        };
        let (edited, remove, save) = ui
            .push_id(("rule", id), |ui| {
                ui.label("Название правила");
                let mut edited = name_field(ui, &mut rule.name, "Правило", id);
                let ready = !rule.draft || rule.bindings_ready() && catalog.bindings_present(rule);
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(ready, egui::Checkbox::new(&mut rule.enabled, "Включено")).changed() {
                        if rule.enabled { rule.draft = false; }
                        edited = true;
                    }
                    edited |= ui.checkbox(&mut rule.once, "Только один раз").changed();
                });
                if rule.draft {
                    ui.weak(if ready { "Черновик готов. Включи правило для игры." } else { "Черновик выключен. Назначь событие, действия и все ссылки на объекты сцены." });
                }
                ui.separator();
                ui.strong("Когда");
                edited |= edit_event(ui, &mut rule.event, &catalog, rule.draft);
                ui.separator();
                ui.strong("Если");
                ui.weak(if rule.conditions.is_empty() {
                    "Без условий"
                } else {
                    "Все условия выполняются"
                });
                edited |= edit_conditions(ui, rule, &catalog);
                ui.separator();
                ui.strong("Тогда");
                edited |= edit_actions(ui, rule, &catalog);
                ui.separator();
                let remove = ui.button("Удалить правило").clicked();
                let save = ui.button("Сохранить правило в библиотеку").clicked();
                (edited, remove, save)
            })
            .inner;
        changed |= edited;
        if save {
            self.library_status = match scene.rules.save_rule_template(id) {
                Ok(template_id) => {
                    self.rule_template = Some(template_id);
                    self.section = Section::Library;
                    changed = true;
                    "Правило сохранено в библиотеке. Укажи файл для переноса в другой проект."
                        .into()
                }
                Err(error) => error,
            };
        }
        if remove && scene.rules.remove_rule(id) {
            self.rule = None;
            changed = true;
        }
        changed
    }

    fn zones(&mut self, ui: &mut egui::Ui, rules: &mut RuleSet, buildings: &[Building]) -> bool {
        let mut changed = false;
        if add_button(ui, "+ Зона", rules.zones.len() < rules::MAX_ZONES)
            && let Some(id) = rules.add_zone([0.0; 3])
        {
            self.zone = Some(id);
            changed = true;
        }
        let choices = rules.zones.iter().map(|zone| (zone.id, zone.name.as_str()));
        let Some(id) = select_entry(ui, "zone_entry", &mut self.zone, choices, "Зона") else {
            ui.label("Зона задаёт область, вход в которую запускает правило.");
            return changed;
        };
        let Some(zone) = rules.zones.iter_mut().find(|zone| zone.id == id) else {
            return changed;
        };
        let (edited, remove) = ui
            .push_id(("zone", id), |ui| {
                ui.label("Название зоны");
                let mut edited = name_field(ui, &mut zone.name, "Зона", id);
                ui.label("Привязка к дому");
                let houses = buildings
                    .iter()
                    .map(|house| (Some(house.id), house.name.as_str()));
                edited |= choose(
                    ui,
                    "zone_house",
                    &mut zone.building_id,
                    std::iter::once((None, "Без привязки")).chain(houses),
                );
                if let Some(house) = buildings
                    .iter()
                    .find(|house| Some(house.id) == zone.building_id)
                    && ui.button("По размеру дома").clicked()
                {
                    zone.position = house.position;
                    zone.size = house.size;
                    if house.quarter_turns % 2 != 0 {
                        zone.size.swap(0, 2);
                    }
                    edited = true;
                }
                ui.label("Положение (Y — низ зоны)");
                edited |= axes(
                    ui,
                    &mut zone.position,
                    [-256.0, -32.0, -256.0],
                    [256.0, 128.0, 256.0],
                );
                ui.label("Размер, м");
                edited |= axes(ui, &mut zone.size, [0.2; 3], [128.0; 3]);
                let remove = ui
                    .button("Удалить зону")
                    .on_hover_text("После удаления можно заменить зону в событиях правила.")
                    .clicked();
                (edited, remove)
            })
            .inner;
        changed |= edited;
        if remove && rules.remove_zone(id) {
            self.zone = None;
            changed = true;
        }
        changed
    }

    fn flags(&mut self, ui: &mut egui::Ui, rules: &mut RuleSet) -> bool {
        let mut changed = false;
        if add_button(ui, "+ Флаг", rules.flags.len() < rules::MAX_FLAGS)
            && let Some(id) = rules.add_flag()
        {
            self.flag = Some(id);
            changed = true;
        }
        let choices = rules.flags.iter().map(|flag| (flag.id, flag.name.as_str()));
        let Some(id) = select_entry(ui, "flag_entry", &mut self.flag, choices, "Флаг") else {
            ui.label("Флаг хранит состояние, например «Задание принято».");
            return changed;
        };
        let Some(flag) = rules.flags.iter_mut().find(|flag| flag.id == id) else {
            return changed;
        };
        let (edited, remove) = ui
            .push_id(("flag", id), |ui| {
                ui.label("Название флага");
                let mut edited = name_field(ui, &mut flag.name, "Флаг", id);
                edited |= ui
                    .checkbox(&mut flag.initially_set, "Включён при запуске")
                    .changed();
                let remove = ui
                    .button("Удалить флаг")
                    .on_hover_text("После удаления можно заменить флаг в условиях и действиях.")
                    .clicked();
                (edited, remove)
            })
            .inner;
        changed |= edited;
        if remove && rules.remove_flag(id) {
            self.flag = None;
            changed = true;
        }
        changed
    }
}

fn name_field(ui: &mut egui::Ui, name: &mut String, fallback: &str, id: u32) -> bool {
    let response = ui.add(egui::TextEdit::singleline(name).char_limit(64));
    if response.lost_focus() && name.trim().is_empty() {
        *name = format!("{fallback} {id}");
        return true;
    }
    response.changed()
}

fn axes(ui: &mut egui::Ui, values: &mut [f32; 3], minimum: [f32; 3], maximum: [f32; 3]) -> bool {
    ui.horizontal_wrapped(|ui| {
        let mut changed = false;
        for (((axis, value), min), max) in ["X ", "Y ", "Z "]
            .into_iter()
            .zip(values)
            .zip(minimum)
            .zip(maximum)
        {
            changed |= ui
                .add(
                    egui::DragValue::new(value)
                        .prefix(axis)
                        .range(min..=max)
                        .speed(0.05),
                )
                .changed();
        }
        changed
    })
    .inner
}

fn add_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    ui.add_enabled(enabled, egui::Button::new(label)).clicked()
}

fn integer(ui: &mut egui::Ui, prefix: &str, number: &mut i32) -> bool {
    ui.add(
        egui::DragValue::new(number)
            .prefix(prefix)
            .range(-rules::MAX_COUNTER_VALUE..=rules::MAX_COUNTER_VALUE),
    )
    .changed()
}

type Named<T> = (T, String);
type AnswerId = (u64, u32, u32);

struct Catalog {
    items: Vec<Named<u32>>,
    actor_ids: Vec<u64>,
    flags: Vec<Named<u32>>,
    zones: Vec<Named<u32>>,
    doors: Vec<Named<u64>>,
    answers: Vec<Named<AnswerId>>,
    quests: Vec<Named<u32>>,
    counters: Vec<Named<u32>>,
    timers: Vec<Named<u32>>,
    objects: Vec<Named<u64>>,
    scenes: Vec<Named<u32>>,
    mark_key: String,
    retry_key: String,
    environment: bool,
    waters: Vec<Named<u32>>,
    contacts: Vec<Named<u64>>,
    rotor: bool,
    effects: Vec<Named<PresetId>>,
    effect_anchors: Vec<Named<u64>>,
    extension_actions: Vec<(u64, String, i32)>,
}

impl Catalog {
    fn binding_values(&self, kind: PortType) -> Vec<Named<Option<BindingValue>>> {
        fn named<T: Copy>(
            entries: &[Named<T>],
            convert: impl Fn(T) -> BindingValue,
        ) -> Vec<Named<Option<BindingValue>>> {
            std::iter::once((None, "Выбрать объект".into()))
                .chain(
                    entries
                        .iter()
                        .map(|(id, label)| (Some(convert(*id)), label.clone())),
                )
                .collect()
        }
        match kind {
            PortType::Item => named(&self.items, BindingValue::Item),
            PortType::Answer => named(&self.answers, |(npc_id, node_id, choice_id)| {
                BindingValue::Answer {
                    npc_id,
                    node_id,
                    choice_id,
                }
            }),
            PortType::Door => named(&self.doors, BindingValue::Door),
            PortType::Zone => named(&self.zones, BindingValue::Zone),
            PortType::Timer => named(&self.timers, BindingValue::Timer),
            PortType::Counter => named(&self.counters, BindingValue::Counter),
            PortType::Flag => named(&self.flags, BindingValue::Flag),
            PortType::Quest => named(&self.quests, BindingValue::Quest),
            PortType::Object => named(&self.objects, BindingValue::Object),
            PortType::Scene => named(&self.scenes, BindingValue::Scene),
            PortType::Effect => named(&self.effects, BindingValue::Effect),
            PortType::EffectAnchor => named(&self.effect_anchors, BindingValue::EffectAnchor),
        }
    }
    fn bindings_present(&self, rule: &Rule) -> bool {
        let event = rule.event.is_some_and(|event| match event {
            Event::Pickup { item_id } => has(&self.items, item_id),
            Event::Dialogue {
                npc_id,
                node_id,
                choice_id,
            } => has(&self.answers, (npc_id, node_id, choice_id)),
            Event::Door { door_id } => has(&self.doors, door_id),
            Event::EnterZone { zone_id } => has(&self.zones, zone_id),
            Event::ActorEnteredZone { actor_id, zone_id } => {
                (actor_id == rules::PLAYER_ACTOR_ID || self.actor_ids.contains(&actor_id))
                    && has(&self.zones, zone_id)
            }
            Event::GameAction { .. } => true,
            Event::TimerElapsed { timer_id } => has(&self.timers, timer_id),
            Event::Simulation(Signal::Contact { sensor_id, .. }) => has(&self.contacts, sensor_id),
            Event::Simulation(Signal::Weather { .. }) => self.environment,
            Event::Simulation(Signal::WaterReached { zone }) => has(&self.waters, zone),
            Event::Simulation(Signal::RotorTurned) => self.rotor,
        });
        event
            && rule.conditions.iter().all(|condition| match *condition {
                Condition::HasItem { item_id } => has(&self.items, item_id),
                Condition::Flag { flag_id, .. } => has(&self.flags, flag_id),
                Condition::Counter { counter_id, .. } => has(&self.counters, counter_id),
                Condition::InZone { zone_id } => has(&self.zones, zone_id),
            })
            && rule.actions.iter().all(|action| match *action {
                Action::SetFlag { flag_id, .. } => has(&self.flags, flag_id),
                Action::UnlockDoor { door_id } => has(&self.doors, door_id),
                Action::SetQuestStage { quest_id, .. } => has(&self.quests, quest_id),
                Action::SetCounter { counter_id, .. } | Action::AddCounter { counter_id, .. } => {
                    has(&self.counters, counter_id)
                }
                Action::StartTimer { timer_id } | Action::StopTimer { timer_id } => {
                    has(&self.timers, timer_id)
                }
                Action::SetObjectEnabled { object_id, .. } => has(&self.objects, object_id),
                Action::ResetAttempt => true,
                Action::Transition { scene_id } => has(&self.scenes, scene_id),
                Action::PlayEffect(command) => {
                    has(&self.effects, command.preset)
                        && match command.origin {
                            EffectOrigin::Player => true,
                            EffectOrigin::Object(id) => has(&self.effect_anchors, id),
                        }
                }
                Action::Extension(call) => {
                    self.extension_actions.iter().any(|(id, _, _)| *id == call.id)
                        && has(&self.counters, call.counter_id)
                }
                Action::Simulation(SimCommand::Weather { .. }) => self.environment,
                Action::Simulation(SimCommand::Water { zone, .. }) => has(&self.waters, zone),
            })
    }

    fn new(scene: &Scene) -> Self {
        let mut answers = Vec::new();
        for conversation in &scene.story.conversations {
            let Some(npc) = scene.npcs.iter().find(|npc| npc.id == conversation.npc_id) else {
                continue;
            };
            for (line, node) in conversation.nodes.iter().enumerate() {
                for choice in &node.choices {
                    answers.push((
                        (npc.id, node.id, choice.id),
                        format!(
                            "{} · {} · {}",
                            npc.avatar.name,
                            line + 1,
                            short_label(&choice.text),
                        ),
                    ));
                }
            }
        }
        let doors = scene
            .world
            .buildings
            .iter()
            .flat_map(|house| {
                house
                    .doors()
                    .map(move |door| (door.id, format!("{} · {}", house.name, door.name)))
            })
            .collect();
        Self {
            actor_ids: scene.npcs.iter().map(|npc| npc.id).collect(),
            environment: scene.environment.is_some(),
            waters: scene
                .environment
                .iter()
                .flat_map(|env| &env.waters)
                .map(|area| (area.id, area.name.clone()))
                .collect(),
            contacts: scene
                .physics
                .sensors
                .iter()
                .map(|sensor| (sensor.spec.id, sensor.name.clone()))
                .collect(),
            rotor: scene.wind_rotor.is_some(),
            effects: scene
                .effects
                .presets
                .iter()
                .map(|preset| (preset.id, preset.settings.name.clone()))
                .collect(),
            effect_anchors: effect_rules::anchors(&scene.world)
                .map(|(id, name, _)| (id, name.to_owned()))
                .collect(),
            extension_actions: scene.rules.extensions.counter_actions()
                .map(|(id, label, factor)| (id, label.to_owned(), factor))
                .collect(),
            mark_key: crate::play_settings::label(&scene.play_settings.controls, crate::play_settings::Action::Mark),
            retry_key: crate::play_settings::label(&scene.play_settings.controls, crate::play_settings::Action::Retry),
            items: scene
                .world
                .keys
                .iter()
                .map(|key| (key.id, key.name.clone()))
                .collect(),
            flags: scene
                .rules
                .flags
                .iter()
                .map(|flag| (flag.id, flag.name.clone()))
                .collect(),
            zones: scene
                .rules
                .zones
                .iter()
                .map(|zone| (zone.id, zone.name.clone()))
                .collect(),
            doors,
            answers,
            quests: scene
                .story
                .quests
                .iter()
                .map(|quest| (quest.id, quest.title.clone()))
                .collect(),
            counters: scene
                .rules
                .counters
                .iter()
                .map(|entry| (entry.id, entry.name.clone()))
                .collect(),
            timers: scene
                .rules
                .timers
                .iter()
                .map(|entry| (entry.id, entry.name.clone()))
                .collect(),
            scenes: scene
                .linked_scenes
                .targets()
                .map(|(id, name)| (id, name.into()))
                .collect(),
            objects: scene
                .world
                .objects
                .iter()
                .map(|entry| (entry.id, entry.name.clone()))
                .chain(
                    scene
                        .world
                        .models
                        .iter()
                        .map(|model| (model.id, model.label.clone())),
                )
                .collect(),
        }
    }

    fn mark_sources(&self) -> Vec<Named<Option<MarkSource>>> {
        let mut sources = vec![(None, "Выбери источник".into())];
        for &(id, ref name) in &self.zones {
            sources.push((Some(MarkSource::Zone(id)), format!("Вход · {name}")));
            sources.push((
                Some(MarkSource::ActionZone(id)),
                format!("{} в зоне · {name}", self.mark_key),
            ));
        }
        for &(id, ref name) in &self.doors {
            sources.push((Some(MarkSource::Door(id)), format!("Дверь · {name}")));
        }
        sources
    }

    fn mark_targets(&self, current: Option<MarkTarget>) -> Vec<Named<Option<MarkTarget>>> {
        let mut targets = vec![(None, "Выбери цель".into())];
        for &(id, ref name) in &self.doors {
            targets.push((Some(MarkTarget::Door(id)), format!("Отпереть · {name}")));
        }
        for &(id, ref name) in &self.objects {
            targets.push((Some(MarkTarget::Object(id)), format!("Включить · {name}")));
        }
        for &(id, ref name) in &self.quests {
            let stage = match current {
                Some(MarkTarget::Quest { quest_id, stage }) if quest_id == id => stage,
                _ => 1,
            };
            targets.push((
                Some(MarkTarget::Quest {
                    quest_id: id,
                    stage,
                }),
                format!("Этап · {name}"),
            ));
        }
        targets
    }

    fn events(&self) -> [(&'static str, Option<Event>); 8] {
        [
            (
                "Поднят предмет",
                self.items
                    .first()
                    .map(|&(item_id, _)| Event::Pickup { item_id }),
            ),
            (
                "Выбран ответ NPC",
                self.answers
                    .first()
                    .map(|&((npc_id, node_id, choice_id), _)| Event::Dialogue {
                        npc_id,
                        node_id,
                        choice_id,
                    }),
            ),
            (
                "Взаимодействие с дверью",
                self.doors
                    .first()
                    .map(|&(door_id, _)| Event::Door { door_id }),
            ),
            (
                "Вход в зону",
                self.zones
                    .first()
                    .map(|&(zone_id, _)| Event::EnterZone { zone_id }),
            ),
            (
                "Игрок вошёл в зону",
                self.zones.first().map(|&(zone_id, _)| Event::ActorEnteredZone {
                    actor_id: rules::PLAYER_ACTOR_ID,
                    zone_id,
                }),
            ),
            (
                "Игровое действие",
                Some(Event::GameAction {
                    action: GameAction::Mark,
                }),
            ),
            (
                "Таймер истёк",
                self.timers
                    .first()
                    .map(|&(timer_id, _)| Event::TimerElapsed { timer_id }),
            ),
            (
                "Физика и среда",
                self.simulation_signals()
                    .into_iter()
                    .find_map(|(_, signal)| signal)
                    .map(Event::Simulation),
            ),
        ]
    }

    fn simulation_signals(&self) -> [(&'static str, Option<Signal>); 4] {
        [
            (
                "Контакт датчика",
                self.contacts
                    .first()
                    .map(|&(sensor_id, _)| Signal::Contact {
                        sensor_id,
                        state: Contact::Pressed,
                    }),
            ),
            (
                "Уровень воды достигнут",
                self.waters
                    .first()
                    .map(|&(zone, _)| Signal::WaterReached { zone }),
            ),
            (
                "Погода",
                self.environment.then_some(Signal::Weather {
                    preset: WeatherPreset::Rain,
                    settled: true,
                }),
            ),
            ("Оборот мельницы", self.rotor.then_some(Signal::RotorTurned)),
        ]
    }

    fn simulation_commands(&self) -> [(&'static str, Option<SimCommand>); 2] {
        [
            (
                "Задать уровень воды",
                self.waters.first().map(|&(zone, _)| SimCommand::Water {
                    zone,
                    height_cm: 35,
                    duration_tenths: 7,
                }),
            ),
            (
                "Задать погоду",
                self.environment.then_some(SimCommand::Weather {
                    preset: WeatherPreset::Rain,
                    duration_tenths: 30,
                }),
            ),
        ]
    }

    fn conditions(&self) -> [(&'static str, Option<Condition>); 4] {
        [
            (
                "Есть предмет",
                self.items
                    .first()
                    .map(|&(item_id, _)| Condition::HasItem { item_id }),
            ),
            (
                "Значение флага",
                self.flags.first().map(|&(flag_id, _)| Condition::Flag {
                    flag_id,
                    value: true,
                }),
            ),
            (
                "Сравнение счётчика",
                self.counters
                    .first()
                    .map(|&(counter_id, _)| Condition::Counter {
                        counter_id,
                        comparison: Comparison::AtLeast,
                        value: 1,
                    }),
            ),
            (
                "Игрок в зоне",
                self.zones
                    .first()
                    .map(|&(zone_id, _)| Condition::InZone { zone_id }),
            ),
        ]
    }

    fn actions(&self) -> [(&'static str, Option<Action>); 12] {
        [
            (
                "Установить флаг",
                self.flags.first().map(|&(flag_id, _)| Action::SetFlag {
                    flag_id,
                    value: true,
                }),
            ),
            (
                "Отпереть дверь",
                self.doors
                    .first()
                    .map(|&(door_id, _)| Action::UnlockDoor { door_id }),
            ),
            (
                "Задать этап задания",
                self.quests
                    .first()
                    .map(|&(quest_id, _)| Action::SetQuestStage { quest_id, stage: 1 }),
            ),
            (
                "Задать счётчик",
                self.counters
                    .first()
                    .map(|&(counter_id, _)| Action::SetCounter {
                        counter_id,
                        value: 0,
                    }),
            ),
            (
                "Прибавить к счётчику",
                self.counters
                    .first()
                    .map(|&(counter_id, _)| Action::AddCounter {
                        counter_id,
                        delta: 1,
                    }),
            ),
            (
                "Запустить таймер",
                self.timers
                    .first()
                    .map(|&(timer_id, _)| Action::StartTimer { timer_id }),
            ),
            (
                "Остановить таймер",
                self.timers
                    .first()
                    .map(|&(timer_id, _)| Action::StopTimer { timer_id }),
            ),
            (
                "Включить или выключить объект",
                self.objects
                    .first()
                    .map(|&(object_id, _)| Action::SetObjectEnabled {
                        object_id,
                        enabled: true,
                    }),
            ),
            ("Перезапустить попытку", Some(Action::ResetAttempt)),
            (
                "Показать эффект",
                self.effects.first().map(|&(preset, _)| {
                    Action::PlayEffect(EffectCommand {
                        preset,
                        origin: EffectOrigin::Player,
                    })
                }),
            ),
            (
                "Перейти в сцену",
                self.scenes
                    .first()
                    .map(|&(scene_id, _)| Action::Transition { scene_id }),
            ),
            (
                "Изменить среду",
                self.simulation_commands()
                    .into_iter()
                    .find_map(|(_, command)| command)
                    .map(Action::Simulation),
            ),
        ]
    }
}

fn has<T: PartialEq>(entries: &[Named<T>], selected: T) -> bool {
    entries.iter().any(|(id, _)| *id == selected)
}

fn short_label(text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = one_line.chars();
    let mut shortened = chars.by_ref().take(72).collect::<String>();
    if chars.next().is_some() {
        shortened.push('…');
    }
    shortened
}

fn select_entry<'a>(
    ui: &mut egui::Ui,
    id: &'static str,
    selected: &mut Option<u32>,
    entries: impl Iterator<Item = (u32, &'a str)>,
    label: &str,
) -> Option<u32> {
    let entries = entries.collect::<Vec<_>>();
    if selected.is_none_or(|id| !entries.iter().any(|entry| entry.0 == id)) {
        *selected = entries.first().map(|entry| entry.0);
    }
    let mut value = (*selected)?;
    ui.label(label);
    choose(ui, id, &mut value, entries.into_iter());
    *selected = Some(value);
    *selected
}

fn choose<'a, T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: impl Hash,
    value: &mut T,
    values: impl Iterator<Item = (T, &'a str)> + Clone,
) -> bool {
    let title = values
        .clone()
        .find(|entry| entry.0 == *value)
        .map_or("Выбрать", |entry| entry.1);
    egui::ComboBox::from_id_salt(id)
        .selected_text(title)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            let mut changed = false;
            for (option, label) in values {
                changed |= ui.selectable_value(value, option, label).changed();
            }
            changed
        })
        .inner
        .unwrap_or(false)
}

fn choose_target<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut T,
    values: &[Named<T>],
) -> bool {
    ui.label(label);
    if values.is_empty() {
        ui.weak("Нет подходящих объектов");
        return false;
    }
    choose(
        ui,
        "target",
        value,
        values.iter().map(|(value, name)| (*value, name.as_str())),
    )
}

fn choose_kind<T: Copy>(
    ui: &mut egui::Ui,
    title: &str,
    current: Option<T>,
    options: &[(&str, Option<T>)],
) -> Option<T> {
    choose_kind_matching(ui, title, current, options, |current, option| {
        discriminant(&current) == discriminant(&option)
    })
}

fn choose_kind_matching<T: Copy>(
    ui: &mut egui::Ui,
    title: &str,
    current: Option<T>,
    options: &[(&str, Option<T>)],
    same_kind: impl Fn(T, T) -> bool,
) -> Option<T> {
    let selected = |option: Option<T>| match (current, option) {
        (Some(current), Some(option)) => same_kind(current, option),
        _ => false,
    };
    egui::ComboBox::from_id_salt("kind")
        .selected_text(title)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            let mut result = None;
            for &(label, option) in options {
                let response = ui.add_enabled(
                    option.is_some(),
                    egui::Button::selectable(selected(option), label),
                );
                if response.clicked() && !selected(option) {
                    result = option;
                    ui.close();
                }
            }
            result
        })
        .inner
        .flatten()
}

fn edit_event(
    ui: &mut egui::Ui,
    event: &mut Option<Event>,
    catalog: &Catalog,
    draft: bool,
) -> bool {
    let title = match event {
        None => "Выбрать событие",
        Some(Event::Pickup { .. }) => "Поднят предмет",
        Some(Event::Dialogue { .. }) => "Выбран ответ NPC",
        Some(Event::Door { .. }) => "Взаимодействие с дверью",
        Some(Event::EnterZone { .. }) => "Вход в зону",
        Some(Event::ActorEnteredZone { actor_id, .. }) if *actor_id == rules::PLAYER_ACTOR_ID => {
            "Игрок вошёл в зону"
        }
        Some(Event::ActorEnteredZone { .. }) => "Актёр вошёл в зону",
        Some(Event::GameAction { .. }) => "Игровое действие",
        Some(Event::TimerElapsed { .. }) => "Таймер истёк",
        Some(Event::Simulation(_)) => "Физика и среда",
    };
    let options = catalog.events();
    let mut changed = false;
    if let Some(kind) = choose_kind(ui, title, *event, &options) {
        *event = Some(if draft {
            rules::unbind_event(kind)
        } else {
            kind
        });
        changed = true;
    }
    changed |= match event {
        Some(Event::Pickup { item_id }) => choose_target(ui, "Предмет", item_id, &catalog.items),
        Some(Event::Dialogue {
            npc_id,
            node_id,
            choice_id,
        }) => {
            let mut answer = (*npc_id, *node_id, *choice_id);
            let changed = choose_target(ui, "Ответ NPC", &mut answer, &catalog.answers);
            (*npc_id, *node_id, *choice_id) = answer;
            changed
        }
        Some(Event::Door { door_id }) => choose_target(ui, "Дверь", door_id, &catalog.doors),
        Some(Event::EnterZone { zone_id }) => choose_target(ui, "Зона", zone_id, &catalog.zones),
        Some(Event::ActorEnteredZone { actor_id, zone_id }) => {
            if *actor_id != rules::PLAYER_ACTOR_ID {
                ui.label(format!("Актёр #{actor_id}"));
            }
            choose_target(ui, "Зона", zone_id, &catalog.zones)
        }
        Some(Event::GameAction { action }) => choose_target(
            ui,
            "Действие игрока",
            action,
            &[
                (GameAction::Mark, format!("Отметить · {}", catalog.mark_key)),
                (
                    GameAction::Retry,
                    format!("Повторить попытку · {}", catalog.retry_key),
                ),
            ],
        ),
        Some(Event::TimerElapsed { timer_id }) => {
            choose_target(ui, "Таймер", timer_id, &catalog.timers)
        }
        Some(Event::Simulation(signal)) => edit_simulation_signal(ui, signal, catalog),
        None => {
            if options.iter().all(|option| option.1.is_none()) {
                ui.weak("Добавь предмет, ответ NPC, дверь или зону.");
            }
            false
        }
    };
    if event.is_some() && ui.small_button("Убрать событие").clicked() {
        *event = None;
        changed = true;
    }
    changed
}

fn edit_conditions(ui: &mut egui::Ui, rule: &mut Rule, catalog: &Catalog) -> bool {
    let kinds = catalog.conditions();
    let first = kinds.iter().find_map(|(_, value)| *value);
    let mut changed = false;
    if add_button(
        ui,
        "+ Условие",
        rule.conditions.len() < rules::MAX_CONDITIONS && first.is_some(),
    ) && let Some(first) = first
    {
        rule.conditions.push(if rule.draft {
            rules::unbind_condition(first)
        } else {
            first
        });
        changed = true;
    }
    let mut remove = None;
    for (index, condition) in rule.conditions.iter_mut().enumerate() {
        ui.push_id(("condition", index), |ui| {
            let title = match condition {
                Condition::HasItem { .. } => "Есть предмет",
                Condition::Flag { .. } => "Значение флага",
                Condition::Counter { .. } => "Сравнение счётчика",
                Condition::InZone { .. } => "Игрок в зоне",
            };
            if let Some(kind) = choose_kind(ui, title, Some(*condition), &kinds) {
                *condition = if rule.draft {
                    rules::unbind_condition(kind)
                } else {
                    kind
                };
                changed = true;
            }
            changed |= match condition {
                Condition::HasItem { item_id } => {
                    choose_target(ui, "Предмет", item_id, &catalog.items)
                }
                Condition::Flag { flag_id, value } => {
                    choose_target(ui, "Флаг", flag_id, &catalog.flags)
                        | ui.checkbox(value, "Флаг включён").changed()
                }
                Condition::Counter {
                    counter_id,
                    comparison,
                    value,
                } => {
                    choose_target(ui, "Счётчик", counter_id, &catalog.counters)
                        | choose(
                            ui,
                            "comparison",
                            comparison,
                            [
                                (Comparison::Equal, "Равно"),
                                (Comparison::NotEqual, "Не равно"),
                                (Comparison::Less, "Меньше"),
                                (Comparison::AtMost, "Не больше"),
                                (Comparison::Greater, "Больше"),
                                (Comparison::AtLeast, "Не меньше"),
                            ]
                            .into_iter(),
                        )
                        | integer(ui, "Значение ", value)
                }
                Condition::InZone { zone_id } => {
                    choose_target(ui, "Проверяемая зона", zone_id, &catalog.zones)
                }
            };
            if ui.small_button("Убрать условие").clicked() {
                remove = Some(index);
            }
            ui.add_space(4.0);
        });
    }
    if let Some(index) = remove {
        rule.conditions.remove(index);
        changed = true;
    }
    changed
}

fn edit_simulation_signal(ui: &mut egui::Ui, signal: &mut Signal, catalog: &Catalog) -> bool {
    let title = match signal {
        Signal::Contact { .. } => "Контакт датчика",
        Signal::Weather { .. } => "Погода",
        Signal::WaterReached { .. } => "Уровень воды достигнут",
        Signal::RotorTurned => "Оборот мельницы",
    };
    let mut changed = false;
    if let Some(value) = ui
        .push_id("signal_kind", |ui| {
            choose_kind(ui, title, Some(*signal), &catalog.simulation_signals())
        })
        .inner
    {
        *signal = value;
        changed = true;
    }
    changed
        | match signal {
            Signal::Contact { sensor_id, state } => {
                choose_target(ui, "Датчик", sensor_id, &catalog.contacts)
                    | choose_target(
                        ui,
                        "Состояние датчика",
                        state,
                        &Contact::ALL.map(|item| (item, item.label().into())),
                    )
            }
            Signal::Weather { preset, settled } => {
                choose_target(
                    ui,
                    "Погода",
                    preset,
                    &WeatherPreset::ALL.map(|item| (item, item.label().into())),
                ) | ui.checkbox(settled, "Переход завершился").changed()
            }
            Signal::WaterReached { zone } => {
                choose_target(ui, "Водный объём", zone, &catalog.waters)
            }
            Signal::RotorTurned => false,
        }
}

fn edit_simulation_command(ui: &mut egui::Ui, command: &mut SimCommand, catalog: &Catalog) -> bool {
    let title = match command {
        SimCommand::Weather { .. } => "Задать погоду",
        SimCommand::Water { .. } => "Задать уровень воды",
    };
    let mut changed = false;
    if let Some(value) = ui
        .push_id("command_kind", |ui| {
            choose_kind(ui, title, Some(*command), &catalog.simulation_commands())
        })
        .inner
    {
        *command = value;
        changed = true;
    }
    let duration = |ui: &mut egui::Ui, value: &mut u16| {
        ui.add(
            egui::DragValue::new(value)
                .prefix("Переход ×0,1 с ")
                .range(0..=6_000),
        )
        .changed()
    };
    changed
        | match command {
            SimCommand::Weather {
                preset,
                duration_tenths,
            } => {
                choose_target(
                    ui,
                    "Погода",
                    preset,
                    &WeatherPreset::ALL.map(|item| (item, item.label().into())),
                ) | duration(ui, duration_tenths)
            }
            SimCommand::Water {
                zone,
                height_cm,
                duration_tenths,
            } => {
                choose_target(ui, "Водный объём", zone, &catalog.waters)
                    | ui.add(egui::DragValue::new(height_cm).prefix("Уровень, см "))
                        .changed()
                    | duration(ui, duration_tenths)
            }
        }
}

fn edit_effect_command(ui: &mut egui::Ui, command: &mut EffectCommand, catalog: &Catalog) -> bool {
    let preset = choose_target(ui, "Пресет эффекта", &mut command.preset, &catalog.effects);
    ui.label("Где показать");
    let origin = choose(
        ui,
        "effect_origin",
        &mut command.origin,
        std::iter::once((EffectOrigin::Player, "Игрок")).chain(
            catalog
                .effect_anchors
                .iter()
                .map(|(id, name)| (EffectOrigin::Object(*id), name.as_str())),
        ),
    );
    preset | origin
}

fn edit_actions(ui: &mut egui::Ui, rule: &mut Rule, catalog: &Catalog) -> bool {
    let mut kinds = catalog.actions().to_vec();
    kinds.extend(catalog.extension_actions.iter().map(|(id, label, _)| {
        (label.as_str(), catalog.counters.first().map(|&(counter_id, _)| {
            Action::Extension(crate::extensions::Call { id: *id, counter_id })
        }))
    }));
    let first = kinds.iter().find_map(|(_, value)| *value);
    let mut changed = false;
    if add_button(
        ui,
        "+ Действие",
        rule.actions.len() < rules::MAX_ACTIONS && first.is_some(),
    ) && let Some(first) = first
    {
        rule.actions.push(if rule.draft {
            rules::unbind_action(first)
        } else {
            first
        });
        changed = true;
    }
    let mut remove = None;
    for (index, action) in rule.actions.iter_mut().enumerate() {
        ui.push_id(("action", index), |ui| {
            let title = match action {
                Action::SetFlag { .. } => "Установить флаг",
                Action::UnlockDoor { .. } => "Отпереть дверь",
                Action::SetQuestStage { .. } => "Задать этап задания",
                Action::SetCounter { .. } => "Задать счётчик",
                Action::AddCounter { .. } => "Прибавить к счётчику",
                Action::StartTimer { .. } => "Запустить таймер",
                Action::StopTimer { .. } => "Остановить таймер",
                Action::SetObjectEnabled { .. } => "Включить или выключить объект",
                Action::ResetAttempt => "Перезапустить попытку",
                Action::Transition { .. } => "Перейти в сцену",
                Action::PlayEffect(_) => "Показать эффект",
                Action::Extension(call) => catalog.extension_actions.iter()
                    .find(|(id, _, _)| *id == call.id)
                    .map_or("Действие расширения", |(_, label, _)| label.as_str()),
                Action::Simulation(_) => "Изменить среду",
            };
            let same_kind = |current, option| match (current, option) {
                (Action::Extension(a), Action::Extension(b)) => a.id == b.id,
                _ => discriminant(&current) == discriminant(&option),
            };
            if let Some(kind) = choose_kind_matching(ui, title, Some(*action), &kinds, same_kind) {
                *action = if rule.draft {
                    rules::unbind_action(kind)
                } else {
                    kind
                };
                changed = true;
            }
            changed |= match action {
                Action::SetFlag { flag_id, value } => {
                    choose_target(ui, "Флаг", flag_id, &catalog.flags)
                        | ui.checkbox(value, "Включить флаг").changed()
                }
                Action::UnlockDoor { door_id } => {
                    choose_target(ui, "Дверь", door_id, &catalog.doors)
                }
                Action::SetQuestStage { quest_id, stage } => {
                    choose_target(ui, "Задание", quest_id, &catalog.quests)
                        | ui.add(
                            egui::DragValue::new(stage)
                                .prefix("Этап ")
                                .range(0..=rules::MAX_STAGE),
                        )
                        .changed()
                }
                Action::SetCounter { counter_id, value } => {
                    choose_target(ui, "Счётчик", counter_id, &catalog.counters)
                        | integer(ui, "Значение ", value)
                }
                Action::AddCounter { counter_id, delta } => {
                    choose_target(ui, "Счётчик", counter_id, &catalog.counters)
                        | integer(ui, "Прибавить ", delta)
                }
                Action::StartTimer { timer_id } | Action::StopTimer { timer_id } => {
                    choose_target(ui, "Таймер", timer_id, &catalog.timers)
                }
                Action::SetObjectEnabled { object_id, enabled } => {
                    choose_target(ui, "Объект сцены", object_id, &catalog.objects)
                        | ui.checkbox(enabled, "Включить объект").changed()
                }
                Action::ResetAttempt => false,
                Action::Transition { scene_id } => {
                    choose_target(ui, "Целевая сцена", scene_id, &catalog.scenes)
                }
                Action::Simulation(command) => edit_simulation_command(ui, command, catalog),
                Action::PlayEffect(command) => edit_effect_command(ui, command, catalog),
                Action::Extension(call) => {
                    if let Some((_, _, factor)) = catalog.extension_actions.iter().find(|(id, _, _)| *id == call.id) {
                        ui.weak(format!("Коэффициент × {factor}"));
                    }
                    choose_target(ui, "Счётчик", &mut call.counter_id, &catalog.counters)
                }
            };
            if ui.small_button("Убрать действие").clicked() {
                remove = Some(index);
            }
            ui.add_space(4.0);
        });
    }
    if let Some(index) = remove {
        rule.actions.remove(index);
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proof_input::{Driver, click, find_label, replace_text};

    #[test]
    fn effect_action_picker_selects_exact_preset_and_named_origin() {
        use crate::{effects::Kind, world::WorldKind};

        let ctx = egui::Context::default();
        let mut driver = Driver::new([780, 940]);
        let mut editor = Editor::default();
        let mut scene = Scene::empty();
        scene.effects.create(Kind::Smoke).unwrap();
        let effect = scene.effects.create(Kind::Smoke).unwrap();
        scene.effects.presets[0].settings.name = "Дым фона".into();
        scene.effects.presets[1].settings.name = "Вспышка маяка".into();
        let anchor = scene.world.add(WorldKind::Platform).unwrap();
        scene.world.objects[0].name = "Маяк для эффекта".into();

        let mut show = |ctx: &egui::Context| {
            egui::CentralPanel::default().show(ctx, |ui| editor.show(ui, &mut scene));
        };
        let mut output = ctx.run(driver.raw(Vec::new()), &mut show);
        for label in [
            "+ Правило",
            "Выбрать событие",
            "Игровое действие",
            "+ Действие",
            "Включить или выключить объект",
            "Показать эффект",
            "Дым фона",
            "Вспышка маяка",
            "Игрок",
            "Маяк для эффекта",
        ] {
            let point = find_label(&output, label, 0)
                .unwrap_or_else(|| panic!("missing visible effect control {label}"));
            driver.skip_frames(45);
            let _ = ctx.run(driver.raw(click(point)), &mut show);
            output = ctx.run(driver.raw(Vec::new()), &mut show);
        }
        assert_eq!(
            scene.rules.rules[0].actions,
            [Action::PlayEffect(EffectCommand {
                preset: effect,
                origin: EffectOrigin::Object(anchor)
            })],
        );
        scene.validate().unwrap();
    }

    #[test]
    fn named_package_binds_and_runs_in_three_projects_through_the_real_editor() {
        fn author(scene: &mut Scene, editor: &mut Editor, actions: &[(&str, Option<&str>)]) {
            let ctx = egui::Context::default();
            let mut driver = Driver::new([800, 1000]);
            let mut show = |ctx: &egui::Context| {
                egui::CentralPanel::default().show(ctx, |ui| editor.show(ui, scene));
            };
            let mut output = ctx.run(driver.raw(Vec::new()), &mut show);
            for (label, new_text) in actions {
                let point =
                    find_label(&output, label, 0).unwrap_or_else(|| panic!("missing {label}"));
                driver.skip_frames(45);
                let _ = ctx.run(driver.raw(click(point)), &mut show);
                if let Some(value) = new_text {
                    let _ = ctx.run(driver.raw(replace_text(value)), &mut show);
                }
                output = ctx.run(driver.raw(Vec::new()), &mut show);
            }
        }

        let mut source = Scene::empty();
        let zone = source.rules.add_zone([1.0, 0.0, 0.0]).unwrap();
        let flag = source.rules.add_flag().unwrap();
        let counter = source.rules.add_counter().unwrap();
        source.rules.add_rule().unwrap();
        let rule = &mut source.rules.rules[0];
        rule.name = "Правило зачёта".into();
        rule.enabled = false;
        rule.event = Some(Event::EnterZone { zone_id: zone });
        rule.conditions = vec![Condition::Flag {
            flag_id: flag,
            value: true,
        }];
        rule.actions = vec![Action::AddCounter {
            counter_id: counter,
            delta: 7,
        }];

        let path = std::env::temp_dir().join(format!(
            "studio-named-package-ui-{}.rules.json",
            std::process::id()
        ));
        let mut editor = Editor::default();
        editor.set_library_path(&path);
        author(
            &mut source,
            &mut editor,
            &[
                ("Сохранить правило в библиотеку", None),
                ("Именованные привязки", None),
                ("Зона", Some("Контрольная точка")),
                ("Флаг", Some("Разрешение на зачёт")),
                ("Счётчик", Some("Таблица результатов")),
                ("Сохранить библиотеку", None),
            ],
        );
        assert!(path.is_file());
        let names: Vec<_> = source.rules.rule_templates[0]
            .ports()
            .into_iter()
            .map(|port| port.name)
            .collect();
        assert_eq!(
            names,
            [
                "Контрольная точка",
                "Разрешение на зачёт",
                "Таблица результатов"
            ]
        );

        for (index, prefix) in ["Двор", "Лабиринт", "Платформа"].into_iter().enumerate()
        {
            let mut project = Scene::empty();
            for _ in 0..=index {
                project.rules.add_zone([0.0; 3]).unwrap();
                project.rules.add_flag().unwrap();
                project.rules.add_counter().unwrap();
            }
            let decoy = project.rules.zones[0].id;
            assert_eq!(decoy, zone);
            let zone = project
                .rules
                .add_zone([index as f32 + 2.0, 0.0, 0.0])
                .unwrap();
            let flag = project.rules.add_flag().unwrap();
            let counter = project.rules.add_counter().unwrap();
            let input_label = format!("{prefix} · граница");
            let condition_label = format!("{prefix} · можно считать");
            let output_label = format!("{prefix} · очки");
            project.rules.zones.last_mut().unwrap().name = input_label.clone();
            project.rules.flags.last_mut().unwrap().name = condition_label.clone();
            project.rules.flags.last_mut().unwrap().initially_set = true;
            project.rules.counters.last_mut().unwrap().name = output_label.clone();

            let mut editor = Editor::default();
            editor.set_library_path(&path);
            author(
                &mut project,
                &mut editor,
                &[
                    ("Библиотека", None),
                    ("Открыть библиотеку", None),
                    ("Именованные привязки", None),
                    ("Выбрать объект", None),
                    (&input_label, None),
                    ("Выбрать объект", None),
                    (&condition_label, None),
                    ("Выбрать объект", None),
                    (&output_label, None),
                    ("Создать связанное правило", None),
                    ("Включено", None),
                ],
            );
            assert_eq!(
                project.rules.rule_templates[0]
                    .ports()
                    .into_iter()
                    .map(|port| port.name)
                    .collect::<Vec<_>>(),
                names,
            );
            let bound = &project.rules.rules[0];
            assert!(bound.enabled && !bound.draft);
            assert_eq!(bound.event, Some(Event::EnterZone { zone_id: zone }));
            assert_eq!(
                bound.conditions,
                [Condition::Flag {
                    flag_id: flag,
                    value: true
                }]
            );
            assert_eq!(
                bound.actions,
                [Action::AddCounter {
                    counter_id: counter,
                    delta: 7
                }]
            );

            let mut session = rules::Session::new(&project.rules);
            let mut interactions = crate::world::InteractionState::default();
            for (zone_id, expected) in [(decoy, 0), (zone, 1)] {
                assert_eq!(
                    session.dispatch_at(
                        &project.rules,
                        &project.world,
                        &project.story,
                        &mut interactions,
                        Event::EnterZone { zone_id },
                        [0.0; 3],
                    ),
                    expected
                );
            }
            assert_eq!(session.counter(&project.rules, counter), Some(7));
            assert_eq!(
                session.counter(&project.rules, source.rules.counters[0].id),
                Some(0)
            );
            project.validate().unwrap();
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn actual_panel_clicks_create_event_and_switch_existing_condition() {
        let ctx = egui::Context::default();
        let mut driver = Driver::new([740, 900]);
        let mut editor = Editor::default();
        let mut scene = Scene::empty();
        let item = scene.world.add_key_at([0.0; 3]).unwrap();
        let flag = scene.rules.add_flag().unwrap();
        let mut show = |ctx: &egui::Context| {
            egui::CentralPanel::default().show(ctx, |ui| editor.show(ui, &mut scene));
        };
        let mut output = ctx.run(driver.raw(vec![]), &mut show);
        for label in [
            "+ Правило",
            "Выбрать событие",
            "Поднят предмет",
            "+ Условие",
            "Есть предмет",
            "Значение флага",
            "+ Действие",
        ] {
            let position = find_label(&output, label, 0)
                .unwrap_or_else(|| panic!("missing visible control {label}"));
            let _ = ctx.run(driver.raw(click(position)), &mut show);
            output = ctx.run(driver.raw(vec![]), &mut show);
        }
        let rule = &scene.rules.rules[0];
        assert_eq!(rule.event, Some(Event::Pickup { item_id: item }));
        assert_eq!(
            rule.conditions,
            [Condition::Flag {
                flag_id: flag,
                value: true
            }]
        );
        assert_eq!(
            rule.actions,
            [Action::SetFlag {
                flag_id: flag,
                value: true
            }]
        );
        scene.validate().unwrap();
    }

    #[test]
    fn actual_library_panel_exports_and_imports_remapped_template() {
        fn paint(
            ctx: &egui::Context,
            driver: &mut Driver,
            editor: &mut Editor,
            scene: &mut Scene,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            ctx.run(driver.raw(events), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| editor.show(ui, scene));
            })
        }
        fn perform(
            ctx: &egui::Context,
            driver: &mut Driver,
            editor: &mut Editor,
            scene: &mut Scene,
            instructions: &[&str],
        ) {
            let mut output = paint(ctx, driver, editor, scene, Vec::new());
            for label in instructions {
                let point = find_label(&output, label, 0)
                    .unwrap_or_else(|| panic!("missing visible control {label}"));
                driver.skip_frames(45);
                paint(ctx, driver, editor, scene, click(point));
                output = paint(ctx, driver, editor, scene, Vec::new());
            }
        }
        let path = std::env::temp_dir().join(format!(
            "studio-template-ui-{}.rules.json",
            std::process::id()
        ));
        let ctx = egui::Context::default();
        let mut driver = Driver::new([800, 1000]);
        let mut source = Scene::empty();
        let mut editor = Editor::default();
        editor.set_library_path(&path);
        perform(
            &ctx,
            &mut driver,
            &mut editor,
            &mut source,
            &["Библиотека", "Сохранить шаблон", "Сохранить библиотеку"],
        );
        assert!(path.is_file() && source.rules.templates.len() == 1);

        let mut target = Scene::empty();
        let mut editor = Editor::default();
        let ctx = egui::Context::default();
        let mut driver = Driver::new([800, 1000]);
        target.world = crate::world::World::demo();
        target.world.objects[0].name = "Маяк".into();
        let object = target.world.objects[0].id;
        let first = target.rules.add_zone([3.0, 0.0, 0.0]).unwrap();
        target.rules.zones.last_mut().unwrap().name = "Север".into();
        let second = target.rules.add_zone([-3.0, 0.0, 0.0]).unwrap();
        target.rules.zones.last_mut().unwrap().name = "Юг".into();
        perform(
            &ctx,
            &mut driver,
            &mut editor,
            &mut target,
            &["Библиотека", "Путь к файлу .rules.json"],
        );
        paint(
            &ctx,
            &mut driver,
            &mut editor,
            &mut target,
            replace_text(&path.to_string_lossy()),
        );
        perform(
            &ctx,
            &mut driver,
            &mut editor,
            &mut target,
            &[
                "Открыть библиотеку",
                "Выбери источник",
                "F в зоне · Север",
                "Выбери источник",
                "Вход · Юг",
                "Выбери цель",
                "Включить · Маяк",
                "Применить шаблон",
            ],
        );
        std::fs::remove_file(path).unwrap();
        assert_eq!(target.rules.rules.len(), 4);
        assert_eq!(
            target.rules.rules[0].conditions,
            [Condition::InZone { zone_id: first }]
        );
        assert_eq!(
            target.rules.rules[2].event,
            Some(Event::EnterZone { zone_id: second })
        );
        assert_eq!(
            target.rules.rules[3].actions.first(),
            Some(&Action::SetObjectEnabled {
                object_id: object,
                enabled: true
            })
        );
    }

    #[test]
    fn portable_rule_requires_explicit_bindings_in_editor() {
        fn click_through(scene: &mut Scene, editor: &mut Editor, labels: &[&str]) {
            let ctx = egui::Context::default();
            let mut driver = Driver::new([800, 1000]);
            let mut show = |ctx: &egui::Context| {
                egui::CentralPanel::default().show(ctx, |ui| editor.show(ui, scene));
            };
            let mut output = ctx.run(driver.raw(Vec::new()), &mut show);
            for label in labels {
                let point =
                    find_label(&output, label, 0).unwrap_or_else(|| panic!("missing {label}"));
                driver.skip_frames(45);
                let _ = ctx.run(driver.raw(click(point)), &mut show);
                output = ctx.run(driver.raw(Vec::new()), &mut show);
            }
        }

        let path = std::env::temp_dir().join(format!(
            "studio-portable-rule-ui-{}.rules.json",
            std::process::id()
        ));
        let mut source = Scene::empty();
        let source_zone = source.rules.add_zone([1.5, 0.0, 0.0]).unwrap();
        let source_counter = source.rules.add_counter().unwrap();
        source.rules.add_rule().unwrap();
        let original = source.rules.rules.last_mut().unwrap();
        original.name = "Проверка маршрута".into();
        original.event = Some(Event::EnterZone {
            zone_id: source_zone,
        });
        original.actions = vec![Action::AddCounter {
            counter_id: source_counter,
            delta: 8,
        }];
        let mut editor = Editor::default();
        editor.set_library_path(&path);
        click_through(
            &mut source,
            &mut editor,
            &["Сохранить правило в библиотеку", "Сохранить библиотеку"],
        );
        assert!(path.is_file());

        let mut destination = Scene::empty();
        assert_eq!(
            destination.rules.add_zone([2.0, 0.0, 0.0]),
            Some(source_zone)
        );
        assert_eq!(destination.rules.add_counter(), Some(source_counter));
        let zone = destination.rules.add_zone([8.0, 0.0, 0.0]).unwrap();
        destination.rules.zones.last_mut().unwrap().name = "Площадь назначения".into();
        let counter = destination.rules.add_counter().unwrap();
        destination.rules.counters.last_mut().unwrap().name = "Счётчик назначения".into();
        let mut editor = Editor::default();
        editor.set_library_path(&path);
        click_through(
            &mut destination,
            &mut editor,
            &[
                "Библиотека",
                "Открыть библиотеку",
                "Добавить черновик правила",
                "Включено",
            ],
        );
        assert!(!destination.rules.rules[0].enabled);
        assert_eq!(
            destination.rules.rules[0].event,
            Some(Event::EnterZone { zone_id: 0 })
        );
        click_through(
            &mut destination,
            &mut editor,
            &[
                "Выбрать",
                "Площадь назначения",
                "Выбрать",
                "Счётчик назначения",
                "Включено",
            ],
        );
        std::fs::remove_file(path).unwrap();
        let bound = &destination.rules.rules[0];
        assert!(bound.enabled && !bound.draft);
        assert_eq!(bound.event, Some(Event::EnterZone { zone_id: zone }));
        assert_eq!(
            bound.actions,
            [Action::AddCounter {
                counter_id: counter,
                delta: 8
            }]
        );
        destination.validate().unwrap();
    }
}
