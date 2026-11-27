//! Strict, local proposals over an exact authored scene. Staging never mutates the live editor.
use std::{
    fmt,
    fs::File,
    io::{Read, Write},
    path::Path,
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

use crate::{
    ImportError,
    rules::{Action, Condition, Rule},
    scene::Scene,
    world::World,
};

#[path = "scene_patch_catalogue.rs"]
mod catalogue;

pub use catalogue::catalogue;

pub const VERSION: u8 = 1;
pub const MAX_JSON_BYTES: usize = 65_536;
pub const MAX_OPS: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Base(String);

impl Base {
    pub fn capture(scene: &Scene) -> Result<Self, Error> {
        let mut authored = scene.clone();
        clear_checkpoints(&mut authored);
        let mut writer = FingerprintWriter(Sha256::new());
        serde_json::to_writer(&mut writer, &authored)
            .map_err(|error| Error::Fingerprint(error.to_string()))?;
        Ok(Self(format!("{:x}", writer.0.finalize())))
    }

    #[cfg(test)]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn valid(&self) -> bool {
        self.0.len() == 64
            && self
                .0
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    }
}

struct FingerprintWriter(Sha256);

impl Write for FingerprintWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub version: u8,
    pub base: Base,
    pub ops: Vec<Op>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Op {
    SetObjectPosition {
        #[serde(with = "id64")]
        id: u64,
        position: [f32; 3],
    },
    SetObjectName {
        #[serde(with = "id64")]
        id: u64,
        name: String,
    },
    SetNpcWaypoint {
        #[serde(with = "id64")]
        npc_id: u64,
        waypoint: u16,
        position: [f32; 3],
    },
    SetReplyText {
        #[serde(with = "id64")]
        npc_id: u64,
        node_id: u32,
        #[serde(default)]
        choice_id: Option<u32>,
        text: String,
    },
    SetRuleEnabled {
        id: u32,
        enabled: bool,
    },
    SetRuleCounter {
        rule_id: u32,
        counter_id: u32,
        value: i32,
    },
    SetRuleConditionCounter {
        rule_id: u32,
        counter_id: u32,
        value: i32,
    },
    SetCounterInitial {
        id: u32,
        value: i32,
    },
    SetZonePosition {
        id: u32,
        position: [f32; 3],
    },
}

mod id64 {
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};

    pub(super) fn serialize<S: Serializer>(id: &u64, output: S) -> Result<S::Ok, S::Error> {
        output.serialize_str(&id.to_string())
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(input: D) -> Result<u64, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Id {
            Text(String),
            Number(u64),
        }

        match Id::deserialize(input)? {
            Id::Number(id) => Ok(id),
            Id::Text(text)
                if !text.is_empty()
                    && text.len() <= 20
                    && text.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                text.parse().map_err(D::Error::custom)
            }
            Id::Text(_) => Err(D::Error::custom("номер объекта должен быть десятичным")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub target: String,
    pub field: &'static str,
    pub before: String,
    pub after: String,
}

#[derive(Debug)]
pub struct Staged {
    base: Base,
    scene: Scene,
    changes: Vec<Change>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    TooLarge,
    Json(String),
    Version(u8),
    InvalidBase,
    Empty,
    TooManyOps,
    Stale,
    Fingerprint(String),
    Operation { index: usize, reason: String },
    InvalidScene(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "Предложение больше {MAX_JSON_BYTES} байт."),
            Self::Json(error) => write!(f, "Некорректный JSON предложения. {error}"),
            Self::Version(version) => {
                write!(f, "Версия предложения {version} пока не поддерживается.")
            }
            Self::InvalidBase => write!(f, "В предложении неверный идентификатор исходной сцены."),
            Self::Empty => write!(f, "В предложении нет операций."),
            Self::TooManyOps => write!(f, "В предложении больше {MAX_OPS} операций."),
            Self::Stale => write!(
                f,
                "Сцена изменилась после создания предложения. Запроси его для текущей сцены."
            ),
            Self::Fingerprint(error) => write!(f, "Не удалось прочитать исходную сцену. {error}"),
            Self::Operation { index, reason } => write!(f, "Правка {}. {reason}", index + 1),
            Self::InvalidScene(error) => write!(f, "Правки дают некорректную сцену. {error}"),
        }
    }
}

impl std::error::Error for Error {}

impl Proposal {
    #[cfg(test)]
    pub fn new(scene: &Scene, ops: Vec<Op>) -> Result<Self, Error> {
        Self::from_ops(Base::capture(scene)?, ops)
    }

    pub fn from_ops(base: Base, ops: Vec<Op>) -> Result<Self, Error> {
        let proposal = Self {
            version: VERSION,
            base,
            ops,
        };
        proposal.validate_protocol()?;
        Ok(proposal)
    }

    /// Decode saved/editable drafts which carry the original authored base hash.
    pub fn decode(bytes: &[u8]) -> Result<Self, ImportError> {
        let proposal: Self = decode_json(bytes).map_err(draft_error)?;
        proposal.validate_protocol().map_err(draft_error)?;
        Ok(proposal)
    }

    pub fn load(path: &Path) -> Result<Self, ImportError> {
        let file = File::open(path).map_err(|error| ImportError::read(path, error))?;
        let mut bytes = Vec::new();
        file.take(MAX_JSON_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| ImportError::read(path, error))?;
        Self::decode(&bytes).map_err(|error| error.with_file(path))
    }

    /// Model output may provide only typed operations. The caller supplies the captured base.
    pub fn decode_ops(bytes: &[u8], base: Base) -> Result<Self, ImportError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ModelOutput {
            ops: Vec<Op>,
        }

        let ModelOutput { ops } = decode_json(bytes).map_err(draft_error)?;
        Self::from_ops(base, ops).map_err(draft_error)
    }

    pub fn stage(&self, current: &Scene) -> Result<Staged, Error> {
        self.validate_protocol()?;
        self.ensure_base(current)?;
        let mut scene = current.clone();
        let mut changes = Vec::with_capacity(self.ops.len());
        for (index, op) in self.ops.iter().enumerate() {
            let change = op
                .apply(&mut scene)
                .map_err(|reason| Error::Operation { index, reason })?;
            if change.before != change.after {
                changes.push(change);
            }
        }
        scene.validate().map_err(Error::InvalidScene)?;
        for (index, op) in self.ops.iter().enumerate() {
            let id = match *op {
                Op::SetRuleEnabled { id, enabled: true } => id,
                Op::SetRuleCounter { rule_id, .. }
                | Op::SetRuleConditionCounter { rule_id, .. } => rule_id,
                _ => continue,
            };
            let rule = scene
                .rules
                .rules
                .iter()
                .find(|rule| rule.id == id)
                .ok_or_else(|| Error::Operation {
                    index,
                    reason: missing("Правило", id),
                })?;
            if rule.enabled && (!rule.bindings_ready() || !rule.authored_targets_exist(&scene)) {
                return Err(Error::Operation {
                    index,
                    reason: format!("У правила #{id} отсутствует событие или цель действия."),
                });
            }
        }
        Ok(Staged {
            base: self.base.clone(),
            scene,
            changes,
        })
    }

    fn validate_protocol(&self) -> Result<(), Error> {
        if self.version != VERSION {
            return Err(Error::Version(self.version));
        }
        if !self.base.valid() {
            return Err(Error::InvalidBase);
        }
        if self.ops.is_empty() {
            return Err(Error::Empty);
        }
        if self.ops.len() > MAX_OPS {
            return Err(Error::TooManyOps);
        }
        Ok(())
    }

    fn ensure_base(&self, scene: &Scene) -> Result<(), Error> {
        if Base::capture(scene)? != self.base {
            return Err(Error::Stale);
        }
        Ok(())
    }
}

impl Staged {
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    /// Replace authored state in one operation so the caller can record one Desk edit.
    /// Checkpoints are preserved from the latest live state because the proposal never edits them.
    pub fn apply(mut self, current: &mut Scene) -> Result<Vec<Change>, Error> {
        if Base::capture(current)? != self.base {
            return Err(Error::Stale);
        }
        if self.changes.is_empty() {
            return Ok(Vec::new());
        }
        copy_checkpoints(&mut self.scene, current);
        self.scene.validate().map_err(Error::InvalidScene)?;
        *current = self.scene;
        Ok(self.changes)
    }
}

impl Op {
    fn apply(&self, scene: &mut Scene) -> Result<Change, String> {
        match self {
            Self::SetObjectPosition { id, position } => {
                let (name, current) = object_fields(&mut scene.world, *id)?;
                Ok(position_change(
                    named("Объект", name, *id),
                    current,
                    *position,
                ))
            }
            Self::SetObjectName { id, name } => {
                let (current, _) = object_fields(&mut scene.world, *id)?;
                checked_text(name, 120, false)?;
                let target = named("Объект", current, *id);
                Ok(replace_value(
                    target,
                    "Имя",
                    current,
                    name.clone(),
                    String::clone,
                ))
            }
            Self::SetNpcWaypoint {
                npc_id,
                waypoint,
                position,
            } => {
                let actor = scene
                    .npcs
                    .iter_mut()
                    .find(|npc| npc.id == *npc_id)
                    .ok_or_else(|| missing("NPC", *npc_id))?;
                let slot = usize::from(*waypoint);
                let point = actor
                    .route
                    .get_mut(slot)
                    .ok_or_else(|| format!("У NPC #{npc_id} нет точки маршрута {}.", slot + 1))?;
                let target = format!(
                    "{} · точка {}",
                    named("NPC", &actor.avatar.name, *npc_id),
                    slot + 1
                );
                Ok(position_change(target, &mut point.position, *position))
            }
            Self::SetReplyText {
                npc_id,
                node_id,
                choice_id,
                text,
            } => {
                if !scene.npcs.iter().any(|actor| actor.id == *npc_id) {
                    return Err(missing("NPC", *npc_id));
                }
                let dialogue = scene
                    .story
                    .conversations
                    .iter_mut()
                    .find(|dialogue| dialogue.npc_id == *npc_id)
                    .ok_or_else(|| format!("У NPC #{npc_id} нет диалога."))?;
                let node = dialogue
                    .nodes
                    .iter_mut()
                    .find(|node| node.id == *node_id)
                    .ok_or_else(|| missing("Реплика", *node_id))?;
                let target = format!(
                    "{} · реплика #{node_id}",
                    named("Диалог", &dialogue.name, *npc_id)
                );
                let (target, current) = if let Some(choice_id) = choice_id {
                    checked_text(text, 160, false)?;
                    let choice = node
                        .choices
                        .iter_mut()
                        .find(|choice| choice.id == *choice_id)
                        .ok_or_else(|| missing("Ответ игрока", *choice_id))?;
                    (format!("{target} · ответ #{choice_id}"), &mut choice.text)
                } else {
                    checked_text(text, 1_000, true)?;
                    (target, &mut node.text)
                };
                Ok(replace_value(
                    target,
                    "Текст",
                    current,
                    text.clone(),
                    String::clone,
                ))
            }
            Self::SetRuleEnabled { id, enabled } => {
                let rule = rule_mut(scene, *id)?;
                let target = named("Правило", &rule.name, *id);
                if *enabled {
                    rule.draft = false;
                }
                Ok(replace_value(
                    target,
                    "Включено",
                    &mut rule.enabled,
                    *enabled,
                    yes_no,
                ))
            }
            Self::SetRuleCounter {
                rule_id,
                counter_id,
                value,
            } => {
                let counter = scene
                    .rules
                    .counter(*counter_id)
                    .ok_or_else(|| missing("Счётчик", *counter_id))?;
                let counter_label = named("Счётчик", &counter.name, *counter_id);
                let rule = rule_mut(scene, *rule_id)?;
                let target = format!(
                    "{} · {counter_label}",
                    named("Правило", &rule.name, *rule_id)
                );
                let mut targets = rule.actions.iter_mut().filter_map(|action| match action {
                    Action::SetCounter {
                        counter_id: target,
                        value,
                    } if target == counter_id => Some(("Задать", value)),
                    Action::AddCounter {
                        counter_id: target,
                        delta,
                    } if target == counter_id => Some(("Прибавить", delta)),
                    _ => None,
                });
                let Some((field, current)) = targets.next() else {
                    return Err(format!(
                        "В правиле #{rule_id} нет действия для счётчика #{counter_id}."
                    ));
                };
                if targets.next().is_some() {
                    return Err(format!(
                        "Правило #{rule_id} несколько раз меняет счётчик #{counter_id}."
                    ));
                }
                Ok(replace_value(
                    target,
                    field,
                    current,
                    *value,
                    i32::to_string,
                ))
            }
            Self::SetRuleConditionCounter {
                rule_id,
                counter_id,
                value,
            } => {
                let counter = scene
                    .rules
                    .counter(*counter_id)
                    .ok_or_else(|| missing("Счётчик", *counter_id))?;
                let counter_label = named("Счётчик", &counter.name, *counter_id);
                let rule = rule_mut(scene, *rule_id)?;
                let target = format!(
                    "{} · {counter_label}",
                    named("Правило", &rule.name, *rule_id)
                );
                let mut matching =
                    rule.conditions
                        .iter_mut()
                        .filter_map(|condition| match condition {
                            Condition::Counter {
                                counter_id: target,
                                value,
                                ..
                            } if target == counter_id => Some(value),
                            _ => None,
                        });
                let current = matching.next().ok_or_else(|| {
                    format!("В правиле #{rule_id} нет условия для счётчика #{counter_id}.")
                })?;
                if matching.next().is_some() {
                    return Err(format!(
                        "В правиле #{rule_id} несколько условий для счётчика #{counter_id}."
                    ));
                }
                Ok(replace_value(
                    target,
                    "Порог условия",
                    current,
                    *value,
                    i32::to_string,
                ))
            }
            Self::SetCounterInitial { id, value } => {
                let counter = scene
                    .rules
                    .counters
                    .iter_mut()
                    .find(|counter| counter.id == *id)
                    .ok_or_else(|| missing("Счётчик", *id))?;
                Ok(replace_value(
                    named("Счётчик", &counter.name, *id),
                    "Начальное значение",
                    &mut counter.initial,
                    *value,
                    i32::to_string,
                ))
            }
            Self::SetZonePosition { id, position } => {
                let zone = scene
                    .rules
                    .zones
                    .iter_mut()
                    .find(|zone| zone.id == *id)
                    .ok_or_else(|| missing("Зона", *id))?;
                Ok(position_change(
                    named("Зона", &zone.name, *id),
                    &mut zone.position,
                    *position,
                ))
            }
        }
    }
}

fn replace_value<T>(
    target: String,
    field: &'static str,
    current: &mut T,
    value: T,
    display: impl Fn(&T) -> String,
) -> Change {
    let before = display(current);
    let after = display(&value);
    *current = value;
    Change {
        target,
        field,
        before,
        after,
    }
}

fn position_change(target: String, current: &mut [f32; 3], position: [f32; 3]) -> Change {
    replace_value(
        target,
        "Позиция, м",
        current,
        position,
        |[x, y, z]| format!("({x}, {y}, {z})"),
    )
}

fn checked_text(text: &str, maximum: usize, multiline: bool) -> Result<(), String> {
    if (!multiline && text.trim().is_empty())
        || text.chars().count() > maximum
        || text
            .chars()
            .any(|ch| ch.is_control() && !(multiline && matches!(ch, '\n' | '\r' | '\t')))
    {
        return Err(format!(
            "Текст должен содержать не больше {maximum} допустимых символов."
        ));
    }
    Ok(())
}

fn yes_no(value: &bool) -> String {
    if *value { "Да" } else { "Нет" }.into()
}

fn named(kind: &str, name: &str, id: impl fmt::Display) -> String {
    format!("{kind} «{name}» #{id}")
}

fn missing(kind: &str, id: impl fmt::Display) -> String {
    format!("{kind} #{id} отсутствует.")
}

fn object_fields(world: &mut World, id: u64) -> Result<(&mut String, &mut [f32; 3]), String> {
    if let Some(object) = world.objects.iter_mut().find(|object| object.id == id) {
        return Ok((&mut object.name, &mut object.position));
    }
    if let Some(model) = world.models.iter_mut().find(|model| model.id == id) {
        return Ok((&mut model.label, &mut model.position));
    }
    Err(missing("Объект", id))
}

fn rule_mut(scene: &mut Scene, id: u32) -> Result<&mut Rule, String> {
    scene
        .rules
        .rules
        .iter_mut()
        .find(|rule| rule.id == id)
        .ok_or_else(|| missing("Правило", id))
}

fn draft_error(error: Error) -> ImportError {
    ImportError::new("Черновик правок сцены", error)
}

fn decode_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    if bytes.len() > MAX_JSON_BYTES {
        return Err(Error::TooLarge);
    }
    serde_json::from_slice(bytes).map_err(|error| Error::Json(error.to_string()))
}

fn clear_checkpoints(scene: &mut Scene) {
    scene.checkpoint = None;
    for link in &mut scene.linked_scenes.entries {
        clear_checkpoints(&mut link.scene);
    }
}

fn copy_checkpoints(staged: &mut Scene, current: &Scene) {
    staged.checkpoint.clone_from(&current.checkpoint);
    for link in &mut staged.linked_scenes.entries {
        if let Some(source) = current.linked_scenes.get(link.id) {
            copy_checkpoints(&mut link.scene, &source.scene);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Avatar, assets,
        editor_history::Desk,
        npcs::{FIRST_NPC_ID, Npc, Waypoint},
        rules::{Comparison, Event, GameAction},
        scene::Checkpoint,
        story::Story,
        world::WorldKind,
    };
    use serde_json::{json, to_vec};

    fn world_scene() -> (Scene, u64) {
        let mut scene = Scene::empty();
        let id = scene.world.add(WorldKind::Crate).unwrap();
        (scene, id)
    }

    fn renamed(id: u64, name: &str) -> Op {
        Op::SetObjectName {
            id,
            name: name.into(),
        }
    }

    fn assert_invalid_patch_fixture(name: &str, detail: &str) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name);
        assert!(matches!(
            Proposal::load(&path),
            Err(ImportError::Invalid { path: Some(file), context, detail: cause })
                if file == path && context == "Черновик правок сцены" && cause.contains(detail)
        ));
    }

    #[test]
    fn a1_patch_rejects_truncated_json() {
        assert_invalid_patch_fixture("a1_rules_patch_truncated.json", "Некорректный JSON");
    }

    #[test]
    fn a1_patch_rejects_wrong_base_format() {
        assert_invalid_patch_fixture("a1_rules_patch_invalid_base.json", "исходной сцены");
    }

    #[test]
    fn strict_patch_wire_format_rejects_unknown_fields_and_limits_batches() {
        let (scene, id) = world_scene();
        let proposal = Proposal::new(&scene, vec![renamed(id, "Деревянный ящик")]).unwrap();
        let bytes = to_vec(&proposal).unwrap();
        assert_eq!(Proposal::decode(&bytes), Ok(proposal.clone()));
        let model = json!({"ops": proposal.ops});
        assert_eq!(
            Proposal::decode_ops(&to_vec(&model).unwrap(), proposal.base.clone()),
            Ok(proposal.clone())
        );
        assert!(matches!(
            Proposal::decode_ops(&bytes, proposal.base.clone()),
            Err(ImportError::Invalid { detail, .. }) if detail.contains("Некорректный JSON")
        ));
        let mut wire = serde_json::to_value(&proposal).unwrap();
        wire["ops"][0]["shell_command"] = json!("unrecognized");
        assert!(matches!(
            Proposal::decode(&to_vec(&wire).unwrap()),
            Err(ImportError::Invalid { detail, .. }) if detail.contains("Некорректный JSON")
        ));
        wire["ops"][0]
            .as_object_mut()
            .unwrap()
            .remove("shell_command");
        wire["base"] = json!("0".repeat(63));
        assert_eq!(
            Proposal::decode(&to_vec(&wire).unwrap()),
            Err(draft_error(Error::InvalidBase))
        );
        wire["base"] = json!(proposal.base.as_str());
        wire["version"] = json!(2);
        assert_eq!(
            Proposal::decode(&to_vec(&wire).unwrap()),
            Err(draft_error(Error::Version(2)))
        );
        wire["version"] = json!(1);
        wire["ops"] = json!([]);
        assert_eq!(
            Proposal::decode(&to_vec(&wire).unwrap()),
            Err(draft_error(Error::Empty))
        );
        wire["ops"] = serde_json::to_value(vec![renamed(id, "Ящик"); MAX_OPS + 1]).unwrap();
        assert_eq!(
            Proposal::decode(&to_vec(&wire).unwrap()),
            Err(draft_error(Error::TooManyOps))
        );
        assert_eq!(
            Proposal::decode(&vec![b' '; MAX_JSON_BYTES + 1]),
            Err(draft_error(Error::TooLarge))
        );
    }

    #[test]
    fn invalid_later_operation_never_mutates_the_input_or_a_validated_preview() {
        let (scene, id) = world_scene();
        let before = scene.clone();
        let proposal = Proposal::new(
            &scene,
            vec![renamed(id, "Новое имя"), renamed(id + 100, "Нет объекта")],
        )
        .unwrap();
        let error = proposal.stage(&scene).unwrap_err();
        assert!(matches!(error, Error::Operation { index: 1, .. }));
        assert_eq!(scene, before);

        let bad_position = Op::SetObjectPosition {
            id,
            position: [10_000.0, 0.0, 0.0],
        };
        let proposal = Proposal::new(&scene, vec![renamed(id, "Новое имя"), bad_position]).unwrap();
        assert!(matches!(
            proposal.stage(&scene),
            Err(Error::InvalidScene(_))
        ));
        assert_eq!(scene, before);
    }

    #[test]
    fn stale_basis_rejects_old_proposals_and_previously_staged_commits() {
        let (mut scene, id) = world_scene();
        let proposal = Proposal::new(&scene, vec![renamed(id, "Вариант ИИ")]).unwrap();
        let staged = proposal.stage(&scene).unwrap();
        scene.world.objects[0].name = "Свежее ручное имя".into();
        assert!(matches!(proposal.stage(&scene), Err(Error::Stale)));
        assert_eq!(staged.apply(&mut scene), Err(Error::Stale));
        assert_eq!(scene.world.objects[0].name, "Свежее ручное имя");
    }

    #[test]
    fn all_changes_commit_as_one_desk_edit_and_preserve_newer_play_state() {
        let (mut scene, id) = world_scene();
        let journal = assets::Library::default();
        let mut desk = Desk::default();
        desk.reset(&scene, &journal);
        let old_name = scene.world.objects[0].name.clone();
        let proposal = Proposal::new(
            &scene,
            vec![
                renamed(id, "Ящик для склада"),
                Op::SetObjectPosition {
                    id,
                    position: [1.25, 0.0, 2.0],
                },
            ],
        )
        .unwrap();
        let staged = proposal.stage(&scene).unwrap();
        assert_eq!(staged.changes().len(), 2);
        assert_eq!(staged.changes()[0].before, old_name);
        assert_eq!(staged.changes()[0].after, "Ящик для склада");
        assert_eq!(scene.world.objects[0].name, old_name);
        assert_eq!(staged.scene().world.objects[0].position, [1.25, 0.0, 2.0]);

        let checkpoint: Checkpoint = serde_json::from_value(json!({
            "position": [4.0, 0.0, 0.0],
            "story": {"entries": [], "latest_complete": null}
        }))
        .unwrap();
        scene.checkpoint = Some(checkpoint.clone());
        assert_eq!(Base::capture(&scene).unwrap(), proposal.base);
        assert_eq!(staged.apply(&mut scene).unwrap().len(), 2);
        assert_eq!(scene.checkpoint, Some(checkpoint));
        assert!(desk.observe(&scene, &journal));
        assert_eq!(
            desk.travel(false).unwrap().scene.world.objects[0].name,
            old_name
        );
        assert!(!desk.can_undo());
        assert_eq!(
            desk.travel(true).unwrap().scene.world.objects[0].position,
            [1.25, 0.0, 2.0]
        );
    }

    #[test]
    fn typed_pack_changes_exact_routes_replies_and_counter_actions() {
        let mut scene = Scene::empty();
        let source = assets::Source::Knight;
        let mut library = assets::Library::default();
        library.ensure(&source, None).unwrap();
        let avatar = Avatar::new(
            source.clone(),
            "Механик".into(),
            &library.get(&source).unwrap().model,
        );
        let npc = FIRST_NPC_ID;
        scene.npcs.push(Npc::new(
            npc,
            avatar,
            vec![Waypoint::at([1.0, 0.0, 0.0]), Waypoint::at([4.0, 0.0, 0.0])],
        ));
        scene.story = Story::demo(npc, [0.0; 2]);
        let node = &scene.story.conversations[0].nodes[0];
        let choice = node.choices[0].id;
        let node_id = node.id;
        let counter_id = scene.rules.add_counter().unwrap();
        let rule_id = scene.rules.add_rule().unwrap();
        let zone_id = scene.rules.add_zone([0.0; 3]).unwrap();
        scene.rules.rules[0].enabled = false;
        scene.rules.rules[0].event = Some(Event::EnterZone { zone_id });
        scene.rules.rules[0].actions = vec![Action::AddCounter {
            counter_id,
            delta: 1,
        }];
        let proposal = Proposal::new(
            &scene,
            vec![
                Op::SetNpcWaypoint {
                    npc_id: npc,
                    waypoint: 1,
                    position: [2.5, 0.0, -1.0],
                },
                Op::SetReplyText {
                    npc_id: npc,
                    node_id,
                    choice_id: None,
                    text: "Сначала проверь проход.".into(),
                },
                Op::SetReplyText {
                    npc_id: npc,
                    node_id,
                    choice_id: Some(choice),
                    text: "Понимаю.".into(),
                },
                Op::SetRuleCounter {
                    rule_id,
                    counter_id,
                    value: 7,
                },
                Op::SetCounterInitial {
                    id: counter_id,
                    value: 3,
                },
                Op::SetZonePosition {
                    id: zone_id,
                    position: [4.0, 0.0, 3.0],
                },
                Op::SetRuleEnabled {
                    id: rule_id,
                    enabled: true,
                },
            ],
        )
        .unwrap();
        let staged = proposal.stage(&scene).unwrap();
        let mut reordered = scene.clone();
        reordered.npcs[0].route.swap(0, 1);
        assert!(matches!(proposal.stage(&reordered), Err(Error::Stale)));
        assert_eq!(staged.changes().len(), 7);
        staged.apply(&mut scene).unwrap();
        assert_eq!(scene.npcs[0].route[0].position, [1.0, 0.0, 0.0]);
        assert_eq!(scene.npcs[0].route[1].position, [2.5, 0.0, -1.0]);
        assert_eq!(
            scene.story.conversations[0].nodes[0].text,
            "Сначала проверь проход."
        );
        assert_eq!(
            scene.story.answer(npc, node_id, choice).unwrap().text,
            "Понимаю."
        );
        assert_eq!(
            scene.rules.rules[0].actions,
            [Action::AddCounter {
                counter_id,
                delta: 7
            }]
        );
        assert!(scene.rules.rules[0].enabled);
        assert_eq!(scene.rules.counters[0].initial, 3);
        assert_eq!(scene.rules.zones[0].position, [4.0, 0.0, 3.0]);

        let before = scene.clone();
        let dangling = Proposal::new(
            &scene,
            vec![
                Op::SetReplyText {
                    npc_id: npc,
                    node_id,
                    choice_id: Some(choice),
                    text: "Правка существующего ответа".into(),
                },
                Op::SetReplyText {
                    npc_id: npc,
                    node_id,
                    choice_id: Some(choice + 100),
                    text: "Лишний ответ".into(),
                },
            ],
        )
        .unwrap();
        let result = dangling
            .stage(&scene)
            .and_then(|staged| staged.apply(&mut scene));
        assert!(matches!(
            result,
            Err(Error::Operation { index: 1, reason }) if reason.contains("Ответ игрока")
        ));
        assert_eq!(
            scene.story.answer(npc, node_id, choice).unwrap().text,
            "Понимаю."
        );
        assert_eq!(scene, before);
    }

    #[test]
    fn a2_patch_does_not_apply_the_first_edit_if_a_later_target_is_missing() {
        let (mut scene, survivor) = world_scene();
        let removed = scene.world.add(WorldKind::Crate).unwrap();
        scene.world.objects.retain(|object| object.id != removed);
        let before = scene.clone();
        let proposal = Proposal::new(
            &scene,
            vec![
                renamed(survivor, "Есть в сцене"),
                renamed(removed, "Уже удалён"),
            ],
        )
        .unwrap();

        let result = proposal
            .stage(&scene)
            .and_then(|staged| staged.apply(&mut scene));
        assert!(matches!(
            result,
            Err(Error::Operation { index: 1, reason }) if reason == missing("Объект", removed)
        ));
        assert_eq!(scene, before);
    }

    #[test]
    fn a2_apply_rejects_a_preview_if_its_target_has_since_been_deleted() {
        let (mut scene, survivor) = world_scene();
        let removed = scene.world.add(WorldKind::Crate).unwrap();
        let proposal = Proposal::new(
            &scene,
            vec![
                renamed(survivor, "Есть в сцене"),
                renamed(removed, "Скоро удалят"),
            ],
        )
        .unwrap();
        let staged = proposal.stage(&scene).unwrap();
        assert_eq!(staged.changes().len(), 2);
        scene.world.objects.retain(|object| object.id != removed);
        let before = scene.clone();

        assert_eq!(staged.apply(&mut scene), Err(Error::Stale));
        assert_eq!(scene, before);
    }

    fn missing_zone_scene() -> (Scene, u64, u32, u32) {
        let (mut scene, object) = world_scene();
        let counter = scene.rules.add_counter().unwrap();
        let zone = scene.rules.add_zone([0.0; 3]).unwrap();
        let id = scene.rules.add_rule().unwrap();
        let rule = &mut scene.rules.rules[0];
        rule.event = Some(Event::EnterZone { zone_id: zone });
        rule.conditions = vec![Condition::Counter {
            counter_id: counter,
            comparison: Comparison::Equal,
            value: 0,
        }];
        rule.actions = vec![Action::AddCounter {
            counter_id: counter,
            delta: 1,
        }];
        assert!(scene.rules.rules[0].authored_targets_exist(&scene));
        assert!(scene.rules.remove_zone(zone));
        (scene, object, id, counter)
    }

    #[test]
    fn a2_rule_action_and_condition_edits_reject_a_missing_zone() {
        let (mut scene, object, rule_id, counter_id) = missing_zone_scene();
        let before = scene.clone();
        for edit in [
            Op::SetRuleCounter {
                rule_id,
                counter_id,
                value: 2,
            },
            Op::SetRuleConditionCounter {
                rule_id,
                counter_id,
                value: 2,
            },
        ] {
            let proposal =
                Proposal::new(&scene, vec![renamed(object, "Не применено"), edit]).unwrap();
            let result = proposal
                .stage(&scene)
                .and_then(|staged| staged.apply(&mut scene));
            assert!(matches!(
                result,
                Err(Error::Operation { index: 1, reason }) if reason.contains(&format!("#{rule_id}"))
            ));
            assert_eq!(scene, before);
        }
    }

    #[test]
    fn a2_disabling_the_broken_rule_is_still_allowed() {
        let (mut scene, _, id, _) = missing_zone_scene();
        let proposal = Proposal::new(
            &scene,
            vec![Op::SetRuleEnabled { id, enabled: false }],
        )
        .unwrap();
        proposal.stage(&scene).unwrap().apply(&mut scene).unwrap();
        assert!(!scene.rules.rules[0].enabled);
    }

    #[test]
    fn a2_unrelated_missing_binding_does_not_block_an_independent_edit() {
        let (mut scene, object, _, _) = missing_zone_scene();
        let before = scene.rules.clone();
        let proposal = Proposal::new(&scene, vec![renamed(object, "Независимая правка")]).unwrap();
        proposal.stage(&scene).unwrap().apply(&mut scene).unwrap();
        assert_eq!(scene.world.objects[0].name, "Независимая правка");
        assert_eq!(scene.rules, before);
    }

    #[test]
    fn enabling_unused_rule_with_stale_references_is_rejected_before_commit() {
        let mut scene = Scene::empty();
        let flag = scene.rules.add_flag().unwrap();
        let id = scene.rules.add_rule().unwrap();
        scene.rules.rules[0].enabled = false;
        scene.rules.rules[0].event = Some(Event::EnterZone { zone_id: 102 });
        scene.rules.rules[0].actions = vec![Action::SetFlag {
            flag_id: flag,
            value: true,
        }];
        assert!(scene.validate().is_ok());
        let enable = Proposal::new(&scene, vec![Op::SetRuleEnabled { id, enabled: true }]).unwrap();
        assert!(matches!(
            enable.stage(&scene),
            Err(Error::Operation { index: 0, .. })
        ));
        assert!(!scene.rules.rules[0].enabled);
    }

    #[test]
    fn model_anchors_preserve_existing_resources_and_ambiguous_counter_is_rejected() {
        let mut scene = Scene::empty();
        let source = assets::Source::Knight;
        let mut library = assets::Library::default();
        library.ensure(&source, None).unwrap();
        scene.world.models.push(assets::Placement::at_ground(
            &[],
            &library.get(&source).unwrap().model,
            source.clone(),
            "Статуя".into(),
            [0.0; 3],
            1.1,
            0.0,
        ));
        let id = scene.world.models[0].id;
        let move_ = Op::SetObjectPosition {
            id,
            position: [-1.0, 0.0, 3.0],
        };
        let proposal = Proposal::new(&scene, vec![move_, renamed(id, "Старая статуя")]).unwrap();
        proposal.stage(&scene).unwrap().apply(&mut scene).unwrap();
        assert_eq!(scene.world.models[0].source, source);
        assert_eq!(scene.world.models[0].label, "Старая статуя");
        assert_eq!(scene.world.models[0].position, [-1.0, 0.0, 3.0]);

        let counter_id = scene.rules.add_counter().unwrap();
        let rule_id = scene.rules.add_rule().unwrap();
        scene.rules.rules[0].event = Some(Event::GameAction {
            action: GameAction::Mark,
        });
        scene.rules.rules[0].actions = vec![
            Action::SetCounter {
                counter_id,
                value: 2,
            },
            Action::AddCounter {
                counter_id,
                delta: 1,
            },
        ];
        let proposal = Proposal::new(
            &scene,
            vec![Op::SetRuleCounter {
                rule_id,
                counter_id,
                value: 99,
            }],
        )
        .unwrap();
        assert!(matches!(
            proposal.stage(&scene),
            Err(Error::Operation { index: 0, .. })
        ));
    }
}
