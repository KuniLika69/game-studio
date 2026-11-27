use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};

use crate::ImportError;

use super::{
    Action, Comparison, Condition, Event, GameAction, MAX_RULES, RuleSet, remove, reserve,
    valid_definition, valid_entry,
};

pub const MAX_TEMPLATES: usize = 32;
pub const MAX_RULE_TEMPLATES: usize = 32;
const MAX_LIBRARY_BYTES: u64 = 256 * 1024;
const FORMAT_VERSION: u8 = 1;
static SAVE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// The reusable part has names and behavior only. World references are assigned on application.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Template {
    pub id: u32,
    pub name: String,
    pub counter_name: String,
    pub mark_names: [String; 2],
}

/// A portable rule keeps its operations and literals, with references reserved for the next scene.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleTemplate {
    pub id: u32,
    pub name: String,
    pub once: bool,
    pub event: Option<Event>,
    pub conditions: Vec<Condition>,
    pub actions: Vec<Action>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub port_names: Vec<(super::Port, String)>,
}

impl RuleTemplate {
    fn same_content(&self, other: &Self) -> bool {
        self.name == other.name
            && self.once == other.once
            && self.event == other.event
            && self.conditions == other.conditions
            && self.actions == other.actions
            && self.port_names == other.port_names
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarkSource {
    Zone(u32),
    ActionZone(u32),
    Door(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarkTarget {
    Door(u64),
    Object(u64),
    Quest { quest_id: u32, stage: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkBindings {
    pub first_mark: MarkSource,
    pub second_mark: MarkSource,
    pub result: MarkTarget,
}

impl MarkSource {
    pub fn same_subject(self, other: Self) -> bool {
        match (self, other) {
            (Self::Zone(a) | Self::ActionZone(a), Self::Zone(b) | Self::ActionZone(b)) => a == b,
            (Self::Door(a), Self::Door(b)) => a == b,
            _ => false,
        }
    }

    fn event(self) -> (Event, Vec<Condition>) {
        match self {
            Self::Zone(zone_id) => (Event::EnterZone { zone_id }, Vec::new()),
            Self::ActionZone(zone_id) => (
                Event::GameAction {
                    action: GameAction::Mark,
                },
                vec![Condition::InZone { zone_id }],
            ),
            Self::Door(door_id) => (Event::Door { door_id }, Vec::new()),
        }
    }

    fn available(self, rules: &RuleSet) -> bool {
        match self {
            Self::Zone(id) | Self::ActionZone(id) => rules.zone(id).is_some(),
            Self::Door(id) => id != 0,
        }
    }

    fn zone(self, rules: &RuleSet) -> Option<&super::Zone> {
        match self {
            Self::Zone(id) | Self::ActionZone(id) => rules.zone(id),
            Self::Door(_) => None,
        }
    }
}

impl MarkTarget {
    fn action(self) -> Action {
        match self {
            Self::Door(door_id) => Action::UnlockDoor { door_id },
            Self::Object(object_id) => Action::SetObjectEnabled {
                object_id,
                enabled: true,
            },
            Self::Quest { quest_id, stage } => Action::SetQuestStage { quest_id, stage },
        }
    }
}

impl RuleSet {
    pub fn save_rule_template(&mut self, rule_id: u32) -> Result<u32, String> {
        let rule = self
            .rules
            .iter()
            .find(|rule| rule.id == rule_id)
            .ok_or("Выбери правило.")?;
        if uses_simulation(rule.event, &rule.actions) {
            return Err("Правило физики и среды хранится в своей сцене.".into());
        }
        let mut template = RuleTemplate {
            id: 0,
            name: rule.name.clone(),
            once: rule.once,
            event: rule.event.map(unbind_event),
            conditions: rule
                .conditions
                .iter()
                .copied()
                .map(unbind_condition)
                .collect(),
            actions: rule.actions.iter().copied().map(unbind_action).collect(),
            port_names: Vec::new(),
        };
        if let Some(existing) = self
            .rule_templates
            .iter()
            .find(|entry| entry.same_content(&template))
        {
            return Ok(existing.id);
        }
        let mut next = self.next_rule_template;
        template.id = reserve(
            &mut next,
            self.rule_templates.iter().map(|entry| entry.id),
            MAX_RULE_TEMPLATES,
        )
        .ok_or("Достигнут предел сохранённых правил.")?;
        validate_rule_templates(std::slice::from_ref(&template))?;
        self.next_rule_template = next;
        let id = template.id;
        self.rule_templates.push(template);
        Ok(id)
    }

    pub fn apply_rule_template(&mut self, template_id: u32) -> Result<u32, String> {
        let template = self
            .rule_templates
            .iter()
            .find(|entry| entry.id == template_id)
            .ok_or("Выбери сохранённое правило.")?
            .clone();
        validate_rule_templates(std::slice::from_ref(&template))?;
        let id = self.add_rule().ok_or("Достигнут предел правил.")?;
        let rule = self.rules.last_mut().unwrap();
        rule.name = template.name;
        rule.enabled = false;
        rule.draft = true;
        rule.once = template.once;
        rule.event = template.event;
        rule.conditions = template.conditions;
        rule.actions = template.actions;
        Ok(id)
    }

    pub fn remove_rule_template(&mut self, id: u32) -> bool {
        remove(&mut self.rule_templates, |template| template.id == id)
    }

    pub fn add_template(&mut self) -> Option<u32> {
        let id = self.next_template_id()?;
        self.templates.push(Template {
            id,
            name: if id == 1 {
                "Две разные отметки".into()
            } else {
                format!("Две разные отметки {id}")
            },
            counter_name: "Отметки".into(),
            mark_names: ["Первая отметка".into(), "Вторая отметка".into()],
        });
        Some(id)
    }

    pub fn remove_template(&mut self, id: u32) -> bool {
        remove(&mut self.templates, |template| template.id == id)
    }

    fn next_template_id(&mut self) -> Option<u32> {
        reserve(
            &mut self.next_template,
            self.templates.iter().map(|template| template.id),
            MAX_TEMPLATES,
        )
    }

    pub fn apply_template(
        &mut self,
        id: u32,
        sources: [MarkSource; 2],
        target: MarkTarget,
    ) -> Result<u32, String> {
        let template = self
            .templates
            .iter()
            .find(|template| template.id == id)
            .ok_or("Выбери шаблон.")?;
        if sources[0].same_subject(sources[1]) {
            return Err("Для двух отметок нужны разные источники.".into());
        }
        if !sources.iter().all(|source| source.available(self)) || !target.action().valid_id() {
            return Err("Назначь обе отметки и цель шаблона.".into());
        }
        if let [Some(first), Some(second)] = sources.map(|source| source.zone(self))
            && zones_overlap(first, second)
        {
            return Err("Разнеси зоны отметок, чтобы одно место не засчиталось дважды.".into());
        }
        if self.rules.len() + 4 > MAX_RULES {
            return Err("Для шаблона нужно четыре свободных правила.".into());
        }
        let mut updated = self.clone();
        let counter_id = updated.add_counter().ok_or("Достигнут предел счётчиков.")?;
        updated
            .counters
            .last_mut()
            .unwrap()
            .name
            .clone_from(&template.counter_name);
        let complete_flag = updated.add_flag().ok_or("Достигнут предел флагов.")?;
        updated.flags.last_mut().unwrap().name = format!("Две отметки · итог {counter_id}");
        if let MarkTarget::Object(id) = target
            && !updated.initially_disabled_objects.contains(&id)
        {
            updated.initially_disabled_objects.push(id);
        }
        for (source, mark_name) in sources.into_iter().zip(&template.mark_names) {
            let (event, mut conditions) = source.event();
            updated
                .add_rule()
                .ok_or("Не удалось создать правило отметки.")?;
            let mark = updated.rules.last_mut().unwrap();
            mark.name.clone_from(mark_name);
            mark.event = Some(event);
            mark.conditions.clone_from(&conditions);
            mark.actions.push(Action::AddCounter {
                counter_id,
                delta: 1,
            });

            conditions.push(Condition::Counter {
                counter_id,
                comparison: Comparison::AtLeast,
                value: 2,
            });
            conditions.push(Condition::Flag {
                flag_id: complete_flag,
                value: false,
            });
            updated
                .add_rule()
                .ok_or("Не удалось создать итоговое правило.")?;
            let complete = updated.rules.last_mut().unwrap();
            complete.name = "Две отметки · результат".into();
            complete.event = Some(event);
            complete.conditions = conditions;
            complete.actions.push(target.action());
            complete.actions.push(Action::SetFlag {
                flag_id: complete_flag,
                value: true,
            });
        }
        updated.validate()?;
        *self = updated;
        Ok(counter_id)
    }

    pub fn apply_mark_package(
        &mut self,
        id: u32,
        bindings: MarkBindings,
        world: &super::World,
        story: &super::Story,
    ) -> Result<u32, String> {
        use super::BindingValue;
        let sources = [bindings.first_mark, bindings.second_mark];
        for (index, source) in sources.into_iter().enumerate() {
            let value = match source {
                MarkSource::Zone(id) | MarkSource::ActionZone(id) => BindingValue::Zone(id),
                MarkSource::Door(id) => BindingValue::Door(id),
            };
            if !value.exists(self, world, story) {
                return Err(format!(
                    "Источник отметки {} отсутствует в сцене.",
                    index + 1
                ));
            }
        }
        let result = match bindings.result {
            MarkTarget::Door(id) => BindingValue::Door(id),
            MarkTarget::Object(id) => BindingValue::Object(id),
            MarkTarget::Quest { quest_id, .. } => BindingValue::Quest(quest_id),
        };
        if !result.exists(self, world, story) {
            return Err("Цель пакета отсутствует в сцене.".into());
        }
        self.apply_template(id, sources, bindings.result)
    }

    pub fn save_library(&self, path: &Path) -> Result<(), String> {
        validate_path(path)?;
        validate_templates(&self.templates)?;
        validate_rule_templates(&self.rule_templates)?;
        if self.templates.is_empty() && self.rule_templates.is_empty() {
            return Err("Добавь шаблон или правило перед сохранением библиотеки.".into());
        }
        if path.exists() {
            read_library(path)?;
        }
        let actions = || self.rule_templates.iter().flat_map(|template| &template.actions).copied();
        let extensions = self.extensions.subset(actions().filter_map(Action::extension_id));
        super::validate_extensions(&extensions, actions())?;
        let data = serde_json::to_vec_pretty(&Library {
            format_version: FORMAT_VERSION,
            templates: self.templates.clone(),
            rule_templates: self.rule_templates.clone(),
            extensions,
        })
        .map_err(|error| error.to_string())?;
        let mut temp = path.file_name().ok_or("Не указано имя файла библиотеки.")?.to_os_string();
        let serial = SAVE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        temp.push(format!(".{}.{}.tmp", std::process::id(), serial));
        let temp = path.with_file_name(temp);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| format!("Не удалось создать файл библиотеки ({error})."))?;
        let result = file.write_all(&data).and_then(|()| file.sync_all());
        drop(file);
        if let Err(error) = result.and_then(|()| fs::rename(&temp, path)) {
            let _ = fs::remove_file(temp);
            return Err(format!("Не удалось сохранить библиотеку ({error})."));
        }
        Ok(())
    }

    pub fn load_library(&mut self, path: &Path) -> Result<usize, ImportError> {
        validate_path(path).map_err(|error| library_error(path, error))?;
        let library = read_library(path)?;
        let mut updated = self.clone();
        for manifest in library.extensions.manifests() {
            updated.extensions.install(manifest.clone())
                .map_err(|error| library_error(path, error))?;
        }
        let mut added = 0;
        for mut template in library.templates {
            if updated.templates.iter().any(|entry| {
                entry.name == template.name
                    && entry.counter_name == template.counter_name
                    && entry.mark_names == template.mark_names
            }) {
                continue;
            }
            template.id = updated
                .next_template_id()
                .ok_or_else(|| library_error(path, "Достигнут предел шаблонов."))?;
            updated.templates.push(template);
            added += 1;
        }
        for mut template in library.rule_templates {
            if updated
                .rule_templates
                .iter()
                .any(|entry| entry.same_content(&template))
            {
                continue;
            }
            template.id = reserve(
                &mut updated.next_rule_template,
                updated.rule_templates.iter().map(|entry| entry.id),
                MAX_RULE_TEMPLATES,
            )
            .ok_or_else(|| library_error(path, "Достигнут предел сохранённых правил."))?;
            updated.rule_templates.push(template);
            added += 1;
        }
        *self = updated;
        Ok(added)
    }
}

fn zones_overlap(a: &super::Zone, b: &super::Zone) -> bool {
    [0, 2].into_iter().all(|axis| {
        (a.position[axis] - b.position[axis]).abs() <= (a.size[axis] + b.size[axis]) * 0.5
    }) && a.position[1] <= b.position[1] + b.size[1]
        && b.position[1] <= a.position[1] + a.size[1]
}

#[derive(Serialize, Deserialize)]
struct Library {
    format_version: u8,
    templates: Vec<Template>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    rule_templates: Vec<RuleTemplate>,
    #[serde(default, skip_serializing_if = "crate::extensions::Registry::is_empty")]
    extensions: crate::extensions::Registry,
}

fn validate_path(path: &Path) -> Result<(), String> {
    if !path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".rules.json"))
    {
        return Err("Укажи путь к файлу с расширением .rules.json.".into());
    }
    Ok(())
}

fn library_error(path: &Path, detail: impl ToString) -> ImportError {
    ImportError::new("Библиотека правил", detail).with_file(path)
}

fn read_library(path: &Path) -> Result<Library, ImportError> {
    let file = File::open(path).map_err(|error| ImportError::read(path, error))?;
    let mut bytes = Vec::new();
    file.take(MAX_LIBRARY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| ImportError::read(path, error))?;
    if bytes.len() as u64 > MAX_LIBRARY_BYTES {
        return Err(library_error(path, "Файл библиотеки больше 256 КБ."));
    }
    let library: Library = serde_json::from_slice(&bytes)
        .map_err(|error| library_error(path, format!("Повреждённый JSON библиотеки ({error}).")))?;
    if library.format_version != FORMAT_VERSION {
        return Err(library_error(path, format!("Версия библиотеки {} не поддерживается.", library.format_version)));
    }
    validate_templates(&library.templates).map_err(|error| library_error(path, error))?;
    validate_rule_templates(&library.rule_templates).map_err(|error| library_error(path, error))?;
    library.extensions.validate().map_err(|error| library_error(path, error))?;
    super::validate_extensions(&library.extensions, library.rule_templates.iter().flat_map(|template| &template.actions).copied())
        .map_err(|error| library_error(path, error))?;
    Ok(library)
}

pub(super) fn validate_templates(templates: &[Template]) -> Result<(), String> {
    let mut ids = HashSet::new();
    let name_valid = |name: &str| {
        !name.trim().is_empty() && name.chars().count() <= 64 && !name.chars().any(char::is_control)
    };
    if templates.len() > MAX_TEMPLATES
        || templates.iter().any(|template| {
            !valid_entry(template.id, &template.name, &mut ids)
                || !name_valid(&template.counter_name)
                || template.mark_names.iter().any(|name| !name_valid(name))
        })
    {
        return Err("Некорректные имена, номера или количество шаблонов.".into());
    }
    Ok(())
}

pub(super) fn validate_rule_templates(templates: &[RuleTemplate]) -> Result<(), String> {
    let mut ids = HashSet::new();
    if templates.len() > MAX_RULE_TEMPLATES
        || templates.iter().any(|template| {
            !valid_entry(template.id, &template.name, &mut ids)
                || uses_simulation(template.event, &template.actions)
                || !template.valid_port_names()
                || !valid_definition(
                    template.event,
                    &template.conditions,
                    &template.actions,
                    true,
                )
                || template
                    .event
                    .is_some_and(|event| unbind_event(event) != event)
                || template
                    .conditions
                    .iter()
                    .any(|&condition| unbind_condition(condition) != condition)
                || template
                    .actions
                    .iter()
                    .any(|&action| unbind_action(action) != action)
        })
    {
        return Err(
            "Некорректное сохранённое правило. Библиотека хранит правила без ссылок на сцену."
                .into(),
        );
    }
    Ok(())
}

fn uses_simulation(event: Option<Event>, actions: &[Action]) -> bool {
    matches!(event, Some(Event::Simulation(_)))
        || actions
            .iter()
            .any(|action| matches!(action, Action::Simulation(_)))
}

pub(crate) fn unbind_event(event: Event) -> Event {
    match event {
        Event::Pickup { .. } => Event::Pickup { item_id: 0 },
        Event::Dialogue { .. } => Event::Dialogue {
            npc_id: 0,
            node_id: 0,
            choice_id: 0,
        },
        Event::Door { .. } => Event::Door { door_id: 0 },
        Event::EnterZone { .. } => Event::EnterZone { zone_id: 0 },
        Event::ActorEnteredZone { actor_id, .. } => Event::ActorEnteredZone { actor_id, zone_id: 0 },
        Event::GameAction { .. } => event,
        Event::Simulation(_) => event,
        Event::TimerElapsed { .. } => Event::TimerElapsed { timer_id: 0 },
    }
}

pub(crate) fn unbind_condition(condition: Condition) -> Condition {
    match condition {
        Condition::HasItem { .. } => Condition::HasItem { item_id: 0 },
        Condition::Flag { value, .. } => Condition::Flag { flag_id: 0, value },
        Condition::Counter {
            comparison, value, ..
        } => Condition::Counter {
            counter_id: 0,
            comparison,
            value,
        },
        Condition::InZone { .. } => Condition::InZone { zone_id: 0 },
    }
}

pub(crate) fn unbind_action(action: Action) -> Action {
    match action {
        Action::SetFlag { value, .. } => Action::SetFlag { flag_id: 0, value },
        Action::UnlockDoor { .. } => Action::UnlockDoor { door_id: 0 },
        Action::SetQuestStage { stage, .. } => Action::SetQuestStage { quest_id: 0, stage },
        Action::SetCounter { value, .. } => Action::SetCounter {
            counter_id: 0,
            value,
        },
        Action::AddCounter { delta, .. } => Action::AddCounter {
            counter_id: 0,
            delta,
        },
        Action::Extension(call) => Action::Extension(crate::extensions::Call { counter_id: 0, ..call }),
        Action::StartTimer { .. } => Action::StartTimer { timer_id: 0 },
        Action::StopTimer { .. } => Action::StopTimer { timer_id: 0 },
        Action::SetObjectEnabled { enabled, .. } => Action::SetObjectEnabled {
            object_id: 0,
            enabled,
        },
        Action::ResetAttempt => action,
        Action::Transition { .. } => Action::Transition { scene_id: 0 },
        Action::PlayEffect(command) => Action::PlayEffect(crate::effect_rules::Command {
            preset: 0,
            origin: match command.origin {
                crate::effect_rules::Origin::Player => crate::effect_rules::Origin::Player,
                crate::effect_rules::Origin::Object(_) => crate::effect_rules::Origin::Object(0),
            },
        }),
        Action::Simulation(_) => action,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        rules::{Session, TraceOutcome},
        story::Story,
        world::{InteractionState, World},
    };

    fn assert_invalid_library_fixture(name: &str, detail: &str) -> Result<(), ImportError> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name);
        let event = Event::GameAction { action: GameAction::Mark };
        let mut rules = RuleSet::default();
        let counter = rules.add_counter().ok_or("Тестовый счётчик не создан.")?;
        rules.add_rule().ok_or("Тестовое правило не создано.")?;
        let rule = rules.rules.last_mut().ok_or("Нет тестового правила.")?;
        rule.enabled = true;
        rule.draft = false;
        rule.once = false;
        rule.event = Some(event);
        rule.actions = vec![Action::AddCounter { counter_id: counter, delta: 1 }];
        let original = rules.clone();
        let mut session = Session::new(&rules);
        let world = World::default();
        let story = Story::default();
        let mut interactions = InteractionState::default();
        assert_eq!(session.dispatch(&rules, &world, &story, &mut interactions, event), 1);
        assert!(matches!(
            rules.load_library(&path),
            Err(ImportError::Invalid { path: Some(file), context, detail: cause })
                if file == path && context == "Библиотека правил" && cause.contains(detail)
        ));
        assert_eq!(rules, original);
        assert_eq!(session.dispatch(&rules, &world, &story, &mut interactions, event), 1);
        assert_eq!(session.counter(&rules, counter), Some(2));
        Ok(())
    }

    #[test]
    fn a1_rules_rejects_truncated_library_and_keeps_dispatching() -> Result<(), ImportError> {
        assert_invalid_library_fixture("a1_rules_library_truncated.rules.json", "Повреждённый JSON")
    }

    #[test]
    fn a1_rules_rejects_wrong_library_version_and_keeps_dispatching() -> Result<(), ImportError> {
        assert_invalid_library_fixture("a1_rules_library_bad_version.rules.json", "Версия библиотеки 99")
    }

    #[test]
    fn a1_rules_rejects_foreign_namespace_and_keeps_dispatching() -> Result<(), ImportError> {
        assert_invalid_library_fixture("a1_rules_library_foreign_namespace.rules.json", "Ключ расширения")
    }

    #[test]
    fn imported_template_remaps_two_sources_and_never_counts_one_twice() {
        let mut original = RuleSet::default();
        original.add_template().unwrap();
        original.templates[0].name = "Маршрут инспекции".into();
        original.templates[0].counter_name = "Проверено".into();
        let path = std::env::temp_dir().join(format!(
            "studio-template-roundtrip-{}.rules.json",
            std::process::id()
        ));
        original.save_library(&path).unwrap();
        let mut remapped = RuleSet::default();
        remapped.add_template().unwrap();
        assert_eq!(remapped.load_library(&path).unwrap(), 1);
        assert_eq!(remapped.load_library(&path).unwrap(), 0);
        fs::remove_file(path).unwrap();
        let template = remapped.templates[1].id;
        let first = remapped.add_zone([5.0, 0.0, 0.0]).unwrap();
        let second = remapped.add_zone([-5.0, 0.0, 0.0]).unwrap();
        let world = World::demo();
        let story = Story::default();
        let object = world.objects[0].id;
        let target = MarkTarget::Object(object);
        assert!(
            remapped
                .apply_template(
                    template,
                    [MarkSource::Zone(first), MarkSource::ActionZone(first)],
                    target
                )
                .is_err()
        );
        assert!(remapped.rules.is_empty());
        let counter = remapped
            .apply_template(
                template,
                [MarkSource::ActionZone(first), MarkSource::Zone(second)],
                target,
            )
            .unwrap();
        let mut session = Session::new(&remapped);
        let mut interactions = InteractionState::default();
        let press = Event::GameAction {
            action: GameAction::Mark,
        };
        assert!(!session.object_enabled(object));
        assert_eq!(
            session.dispatch_at(
                &remapped,
                &world,
                &story,
                &mut interactions,
                press,
                [0.0; 3]
            ),
            0
        );
        assert_eq!(
            session.dispatch_at(
                &remapped,
                &world,
                &story,
                &mut interactions,
                press,
                [5.0, 0.0, 0.0]
            ),
            1
        );
        assert_eq!(session.counter(&remapped, counter), Some(1));
        let mut resumed = Session::default();
        resumed.restore(&remapped, &story, &session.snapshot());
        assert_eq!(
            resumed.dispatch_at(
                &remapped,
                &world,
                &story,
                &mut interactions,
                press,
                [5.0, 0.0, 0.0]
            ),
            0
        );
        assert!(
            resumed
                .trace()
                .any(|trace| trace.outcome == TraceOutcome::AlreadyFired)
        );
        assert_eq!(resumed.counter(&remapped, counter), Some(1));
        assert!(!resumed.object_enabled(object));
        assert_eq!(
            resumed.move_player(
                &remapped,
                &world,
                &story,
                &mut interactions,
                [0.0; 3],
                [-5.0, 0.0, 0.0]
            ),
            2
        );
        assert_eq!(resumed.counter(&remapped, counter), Some(2));
        assert!(resumed.object_enabled(object));
        assert_eq!(
            resumed.dispatch_at(
                &remapped,
                &world,
                &story,
                &mut interactions,
                press,
                [5.0, 0.0, 0.0]
            ),
            0
        );
        assert_eq!(resumed.counter(&remapped, counter), Some(2));
    }

    #[test]
    fn portable_rule_keeps_operations_but_never_reuses_colliding_project_ids() {
        let mut source = RuleSet::default();
        let zone = source.add_zone([2.0, 0.0, 0.0]).unwrap();
        let counter = source.add_counter().unwrap();
        let flag = source.add_flag().unwrap();
        let id = source.add_rule().unwrap();
        let rule = source.rules.last_mut().unwrap();
        rule.once = false;
        rule.event = Some(Event::EnterZone { zone_id: zone });
        rule.conditions = vec![
            Condition::Flag {
                flag_id: flag,
                value: false,
            },
            Condition::Counter {
                counter_id: counter,
                comparison: Comparison::AtLeast,
                value: -4,
            },
        ];
        rule.actions = vec![Action::AddCounter {
            counter_id: counter,
            delta: 7,
        }];
        source.save_rule_template(id).unwrap();
        let path = std::env::temp_dir().join(format!(
            "studio-portable-rule-{}.rules.json",
            std::process::id()
        ));
        source.save_library(&path).unwrap();

        let mut destination = RuleSet::default();
        assert_eq!(destination.add_zone([2.0, 0.0, 0.0]), Some(zone));
        assert_eq!(destination.add_counter(), Some(counter));
        assert_eq!(destination.add_flag(), Some(flag));
        assert_eq!(destination.load_library(&path).unwrap(), 1);
        assert_eq!(destination.load_library(&path).unwrap(), 0);
        fs::remove_file(path).unwrap();
        let template = destination.rule_templates[0].id;
        destination.apply_rule_template(template).unwrap();
        destination.validate().unwrap();
        let rule = &destination.rules[0];
        assert!(!rule.enabled && rule.draft && !rule.once && !rule.bindings_ready());
        assert_eq!(rule.event, Some(Event::EnterZone { zone_id: 0 }));
        assert_eq!(
            rule.conditions,
            [
                Condition::Flag {
                    flag_id: 0,
                    value: false
                },
                Condition::Counter {
                    counter_id: 0,
                    comparison: Comparison::AtLeast,
                    value: -4,
                }
            ]
        );
        assert_eq!(
            rule.actions,
            [Action::AddCounter {
                counter_id: 0,
                delta: 7
            }]
        );
        destination.rules[0].enabled = true;
        assert!(destination.validate().is_err());
        destination.rules[0].enabled = false;
        let world = World::default();
        let story = Story::default();
        let mut session = Session::new(&destination);
        let mut doors = InteractionState::default();
        assert_eq!(
            session.move_player(
                &destination,
                &world,
                &story,
                &mut doors,
                [0.0; 3],
                [2.0, 0.0, 0.0]
            ),
            0
        );
        assert_eq!(session.counter(&destination, counter), Some(0));
    }
}
