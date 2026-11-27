use std::{collections::HashSet, fs::File, io::Read, path::Path};

use serde::{Deserialize, Serialize};

use crate::assets::materials::Lighting;

const MAX_MANIFEST_BYTES: u64 = 8 * 1024;
const MAX_EXTENSIONS: usize = 16;
const FORMAT: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Call {
    pub id: u64,
    pub counter_id: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    version: u8,
    key: String,
    title: String,
    capability: Capability,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Capability {
    MultiplyCounter {
        label: String,
        factor: i32,
    },
    SetLighting {
        label: String,
        daylight: f32,
        interior_gain: f32,
    },
}

impl Manifest {
    pub fn read(path: &Path) -> Result<Self, String> {
        if !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                let name = name.to_ascii_lowercase();
                name.ends_with(".studio-extension.json") || name.ends_with(".studio-extension")
            })
        {
            return Err("Укажи файл .studio-extension.json.".into());
        }
        let file =
            File::open(path).map_err(|error| format!("Не удалось открыть манифест ({error})."))?;
        let mut bytes = Vec::new();
        file.take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("Не удалось прочитать манифест ({error})."))?;
        Self::decode(&bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err("Манифест расширения больше 8 КБ.".into());
        }
        let manifest: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("Некорректный манифест расширения ({error})."))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn id(&self) -> u64 {
        // Keep identities stable in project and rule-library files.
        self.key.bytes().fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn capability(&self) -> &Capability {
        &self.capability
    }

    fn validate(&self) -> Result<(), String> {
        if self.version != FORMAT {
            return Err("Версия манифеста расширения не поддерживается.".into());
        }
        let id = self.key.as_bytes();
        if !(3..=48).contains(&id.len())
            || !id[0].is_ascii_lowercase()
            || !id
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
            || self.id() == 0
        {
            return Err("Ключ расширения должен начинаться с латинской буквы и содержать 3–48 букв, цифр или дефисов.".into());
        }
        let valid_label = |label: &str| {
            !label.trim().is_empty()
                && label.chars().count() <= 64
                && !label.chars().any(char::is_control)
        };
        let (label, valid_values) = match &self.capability {
            Capability::MultiplyCounter { label, factor } => (label, (-16..=16).contains(factor)),
            Capability::SetLighting {
                label,
                daylight,
                interior_gain,
            } => (
                label,
                Lighting {
                    daylight: *daylight,
                    interior_gain: *interior_gain,
                    ground_tint: None,
                }
                .valid(),
            ),
        };
        if !valid_label(&self.title) || !valid_label(label) || !valid_values {
            return Err("Некорректное имя, коэффициент или параметры инструмента.".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Registry {
    manifests: Vec<Manifest>,
}

impl Registry {
    pub fn is_empty(&self) -> bool {
        self.manifests.is_empty()
    }

    pub fn manifests(&self) -> &[Manifest] {
        &self.manifests
    }

    pub fn install(&mut self, manifest: Manifest) -> Result<bool, String> {
        manifest.validate()?;
        let id = manifest.id();
        if let Some(existing) = self.manifests.iter().find(|entry| entry.id() == id) {
            if *existing == manifest {
                return Ok(false);
            }
            return Err("Есть расширение с тем же ключом и другими настройками.".into());
        }
        if self.manifests.len() >= MAX_EXTENSIONS {
            return Err("В проекте уже 16 расширений.".into());
        }
        self.manifests.push(manifest);
        Ok(true)
    }

    pub fn counter_actions(&self) -> impl Iterator<Item = (u64, &str, i32)> {
        self.manifests
            .iter()
            .filter_map(|manifest| match manifest.capability() {
                Capability::MultiplyCounter { label, factor } => {
                    Some((manifest.id(), label.as_str(), *factor))
                }
                Capability::SetLighting { .. } => None,
            })
    }

    pub fn counter_action(&self, id: u64) -> Option<(&str, i32)> {
        self.counter_actions()
            .find(|(key, _, _)| *key == id)
            .map(|(_, label, factor)| (label, factor))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.manifests.len() > MAX_EXTENSIONS {
            return Err("В проекте слишком много расширений.".into());
        }
        let mut ids = HashSet::new();
        for manifest in &self.manifests {
            manifest.validate()?;
            if !ids.insert(manifest.id()) {
                return Err("В проекте повторяется ключ расширения.".into());
            }
        }
        Ok(())
    }

    pub(crate) fn subset(&self, ids: impl Iterator<Item = u64>) -> Self {
        let required: HashSet<_> = ids.collect();
        Self {
            manifests: self
                .manifests
                .iter()
                .filter(|manifest| required.contains(&manifest.id()))
                .cloned()
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        rules::{
            Action, Binding, BindingValue, Event, GameAction, MAX_COUNTER_VALUE, Port, Session,
            TraceOutcome,
        },
        scene::Scene,
        world::InteractionState,
    };

    const MARK: Event = Event::GameAction {
        action: GameAction::Mark,
    };

    const RULE: &[u8] =
        include_bytes!("../demo_assets/extensions/double-counter.studio-extension.json");
    const TOOL: &[u8] =
        include_bytes!("../demo_assets/extensions/warm-light.studio-extension.json");

    #[test]
    fn two_capabilities_are_distinct_and_stable_after_serialization() {
        let mut registry = Registry::default();
        let action = Manifest::decode(RULE).unwrap();
        let tool = Manifest::decode(TOOL).unwrap();
        assert_ne!(action.id(), tool.id());
        assert!(registry.install(action.clone()).unwrap());
        assert!(registry.install(tool.clone()).unwrap());
        assert!(!registry.install(action.clone()).unwrap());
        assert_eq!(
            registry.counter_action(action.id()),
            Some(("Удвоить счётчик", 2))
        );
        assert!(registry.counter_action(tool.id()).is_none());
        let restored: Registry =
            serde_json::from_slice(&serde_json::to_vec(&registry).unwrap()).unwrap();
        restored.validate().unwrap();
        assert_eq!(registry, restored);
    }

    #[test]
    fn rejects_capabilities_outside_the_schema_and_invalid_ranges() {
        let manifest = |capability: serde_json::Value| {
            let bytes = serde_json::to_vec(&serde_json::json!({
                "version": 1, "key": "test-limit", "title": "Test", "capability": capability
            }))
            .unwrap();
            Manifest::decode(&bytes)
        };
        assert!(manifest(serde_json::json!({"kind":"run_program","program":"x"})).is_err());
        assert!(
            manifest(serde_json::json!({"kind":"multiply_counter","label":"X","factor":17}))
                .is_err()
        );
        assert!(
            manifest(
                serde_json::json!({"kind":"multiply_counter","label":"X","factor":2,"path":"x"})
            )
            .is_err()
        );
        assert!(manifest(serde_json::json!({"kind":"set_lighting","label":"X","daylight":8,"interior_gain":1})).is_err());
        assert!(Manifest::decode(&vec![0; MAX_MANIFEST_BYTES as usize + 1]).is_err());
    }

    fn example_scene() -> (Scene, u64, u32) {
        let mut scene = Scene::empty();
        let manifest = Manifest::decode(RULE).unwrap();
        let extension = manifest.id();
        scene.rules.extensions.install(manifest).unwrap();
        scene
            .rules
            .extensions
            .install(Manifest::decode(TOOL).unwrap())
            .unwrap();
        let counter = scene.rules.add_counter().unwrap();
        scene.rules.counters[0].initial = 3;
        scene.rules.add_rule().unwrap();
        scene.rules.rules[0].event = Some(MARK);
        scene.rules.rules[0].actions = vec![Action::Extension(Call {
            id: extension,
            counter_id: counter,
        })];
        (scene, extension, counter)
    }

    #[test]
    fn real_rule_dispatch_is_bounded_preflighted_and_portable() {
        let (scene, extension, counter) = example_scene();
        let isolated_data = serde_json::to_vec(&scene).unwrap();
        let mut loaded: Scene = serde_json::from_slice(&isolated_data).unwrap();
        loaded.validate().unwrap();
        let mut state = Session::new(&loaded.rules);
        let mut interactions = InteractionState::default();
        let run = |state: &mut Session, scene: &Scene, interactions: &mut InteractionState| {
            state.dispatch(&scene.rules, &scene.world, &scene.story, interactions, MARK)
        };
        assert_eq!(run(&mut state, &loaded, &mut interactions), 1);
        assert_eq!(state.counter(&loaded.rules, counter), Some(6));
        assert_eq!(run(&mut state, &loaded, &mut interactions), 0);

        loaded.rules.counters[0].initial = MAX_COUNTER_VALUE - 10;
        let mut state = Session::new(&loaded.rules);
        assert_eq!(run(&mut state, &loaded, &mut interactions), 1);
        assert_eq!(
            state.counter(&loaded.rules, counter),
            Some(MAX_COUNTER_VALUE)
        );
        let missing = Action::Extension(Call {
            id: extension ^ 1,
            counter_id: counter,
        });
        loaded.rules.rules[0].actions = vec![
            Action::SetCounter {
                counter_id: counter,
                value: 13,
            },
            missing,
        ];
        let mut state = Session::new(&loaded.rules);
        assert_eq!(run(&mut state, &loaded, &mut interactions), 0);
        assert_eq!(
            state.counter(&loaded.rules, counter),
            Some(MAX_COUNTER_VALUE - 10)
        );
        assert_eq!(
            state.trace().last().unwrap().outcome,
            TraceOutcome::MissingActionTarget(missing)
        );
        assert!(loaded.validate().is_err());
    }

    #[test]
    fn rules_library_bundles_only_action_dependency_and_rebinds_counter() {
        let (mut scene, extension, _) = example_scene();
        let rule = scene.rules.rules[0].id;
        scene.rules.save_rule_template(rule).unwrap();
        let path = std::env::temp_dir().join(format!(
            "studio-extension-library-{}.rules.json",
            std::process::id()
        ));
        scene.rules.save_library(&path).unwrap();
        let mut target = Scene::empty();
        target.rules.add_counter().unwrap();
        let counter = target.rules.add_counter().unwrap();
        target.rules.counters[1].initial = 5;
        assert_eq!(target.rules.load_library(&path).unwrap(), 1);
        std::fs::remove_file(path).unwrap();
        assert_eq!(target.rules.extensions.manifests().len(), 1);
        assert_eq!(
            target.rules.extensions.counter_action(extension),
            Some(("Удвоить счётчик", 2))
        );
        let template = target.rules.rule_templates[0].id;
        let id = target
            .rules
            .apply_bound_rule(
                template,
                &[Binding {
                    port: Port::Output(0),
                    value: BindingValue::Counter(counter),
                }],
                &target.world,
                &target.story,
                &target.linked_scenes,
                &target.effects,
            )
            .unwrap();
        let rule = target
            .rules
            .rules
            .iter_mut()
            .find(|rule| rule.id == id)
            .unwrap();
        rule.draft = false;
        rule.enabled = true;
        let mut session = Session::new(&target.rules);
        assert_eq!(
            session.dispatch(
                &target.rules,
                &target.world,
                &target.story,
                &mut InteractionState::default(),
                MARK
            ),
            1
        );
        assert_eq!(session.counter(&target.rules, counter), Some(10));
    }
}
