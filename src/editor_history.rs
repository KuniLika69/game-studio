use crate::{asset_sources, assets, scene::Scene};
use std::{
    collections::{HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const LIMIT: usize = 40;
const MEMORY_LIMIT: usize = 192 * 1024 * 1024;
const RECOVERY_DELAY: Duration = Duration::from_secs(7);
const RECOVERY_FILE: &str = "recovery.robot.json";
const RECOVERY_META: &str = "project.json";

#[derive(Clone, Copy)]
pub(crate) enum Replace {
    New,
    Open,
    Close,
    Recover(usize),
}

pub(crate) struct Snapshot {
    pub scene: Arc<Scene>,
    pub imports: Vec<(assets::Source, assets::LoadedModel)>,
}

impl Snapshot {
    fn capture(scene: &Scene, assets: &assets::Library) -> Self {
        let mut seen = HashSet::new();
        let imports = asset_sources(scene)
            .filter(|(source, _)| {
                matches!(source, assets::Source::Imported(_)) && seen.insert(*source)
            })
            .filter_map(|(source, _)| Some((source.clone(), assets.get(source)?.clone())))
            .collect();
        Self {
            scene: Arc::new(scene.clone()),
            imports,
        }
    }

    fn scene_bytes(&self) -> usize {
        crate::linked_scenes::all_scenes(&self.scene)
            .map(|scene| {
                let pixels = scene.sprite_pixels.len() * size_of::<crate::scene::SpritePixel>();
                let map = scene.map_2d.as_ref().map_or(0, |map| {
                    map.atlases
                        .iter()
                        .map(|atlas| atlas.png.len())
                        .sum::<usize>()
                        + map
                            .layers
                            .iter()
                            .map(|layer| layer.len() * size_of::<Option<crate::map2d::Tile>>())
                            .sum::<usize>()
                        + map.colliders.len() * size_of::<crate::map2d::Collision>()
                });
                32 * 1024 + pixels + map + scene.audio.byte_count()
            })
            .sum()
    }
}

#[derive(Default)]
pub(crate) struct Desk {
    baseline: Option<Arc<Scene>>,
    current: Option<Snapshot>,
    past: VecDeque<Snapshot>,
    future: Vec<Snapshot>,
    dirty: bool,
    pub watch_revision: u64,
    pub pending: bool,
    pub replace: Option<Replace>,
    pub closing: bool,
    pub recovery_open: bool,
    pub recoveries: Vec<Recovery>,
    pub recovery_root: Option<PathBuf>,
    recovery: Option<PathBuf>,
    recovered_from: Option<PathBuf>,
    deadline: Option<Instant>,
}

impl Desk {
    pub fn initialize(&mut self, scene: &Scene, assets: &assets::Library) {
        if self.current.is_none() {
            self.reset(scene, assets);
        }
    }

    pub fn reset(&mut self, scene: &Scene, assets: &assets::Library) {
        let snapshot = Snapshot::capture(scene, assets);
        self.baseline = Some(snapshot.scene.clone());
        self.current = Some(snapshot);
        self.past.clear();
        self.future.clear();
        self.dirty = false;
        self.pending = false;
        self.recovery = None;
        self.recovered_from = None;
        self.deadline = None;
    }

    pub fn observe(&mut self, scene: &Scene, assets: &assets::Library) -> bool {
        self.pending = false;
        let Some(current) = &self.current else {
            self.reset(scene, assets);
            return false;
        };
        if same_content(&current.scene, scene) {
            return false;
        }
        let snapshot = Snapshot::capture(scene, assets);
        self.past.push_back(self.current.replace(snapshot).unwrap());
        self.future.clear();
        while self.past.len() > LIMIT || self.history_bytes() > MEMORY_LIMIT {
            if self.past.len() <= 1 {
                break;
            }
            self.past.pop_front();
        }
        self.schedule();
        true
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    pub fn travel(&mut self, forward: bool) -> Option<&Snapshot> {
        if forward {
            let next = self.future.pop()?;
            self.past.push_back(self.current.replace(next).unwrap());
        } else {
            let previous = self.past.pop_back()?;
            self.future.push(self.current.replace(previous).unwrap());
        }
        self.schedule();
        self.current.as_ref()
    }

    pub fn saved(&mut self) {
        self.baseline = self.current.as_ref().map(|snapshot| snapshot.scene.clone());
        self.dirty = false;
        self.discard_recovery();
    }

    pub fn recovered(&mut self, copy: &Recovery) {
        self.baseline = None;
        self.recovered_from = copy.scene.parent().map(Path::to_owned);
        self.schedule();
    }

    pub fn discard_recovery(&mut self) {
        for directory in [self.recovery.take(), self.recovered_from.take()]
            .into_iter()
            .flatten()
        {
            remove_recovery(&directory);
        }
        self.deadline = None;
    }

    pub fn show_recoveries(&mut self) {
        self.recoveries = self.root().map_or_else(Vec::new, |root| recoveries(&root));
        self.recoveries.retain(|copy| {
            copy.scene.parent() != self.recovery.as_deref()
                && copy.scene.parent() != self.recovered_from.as_deref()
        });
        self.recovery_open = true;
    }

    pub fn maybe_autosave(
        &mut self,
        assets: &assets::Library,
        project: Option<&Path>,
    ) -> Option<Result<(), String>> {
        if Instant::now() < self.deadline? {
            return None;
        }
        self.deadline = None;
        Some(self.write_recovery(assets, project))
    }

    pub fn remaining(&self) -> Option<Duration> {
        Some(self.deadline?.saturating_duration_since(Instant::now()))
    }

    pub(crate) fn write_recovery(
        &mut self,
        assets: &assets::Library,
        project: Option<&Path>,
    ) -> Result<(), String> {
        let Some(snapshot) = &self.current else {
            return Ok(());
        };
        let directory = if let Some(directory) = &self.recovery {
            directory.clone()
        } else {
            let root = self.root().ok_or("Каталог восстановления недоступен.")?;
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let directory = root.join(format!("{}-{stamp}", std::process::id()));
            fs::create_dir_all(&directory).map_err(|err| err.to_string())?;
            self.recovery = Some(directory.clone());
            directory
        };
        let scene = directory.join(RECOVERY_FILE);
        let meta = serde_json::to_vec(&project).map_err(|err| err.to_string())?;
        let metadata = directory.join(RECOVERY_META);
        if !metadata.exists() {
            fs::write(&metadata, meta).map_err(|err| err.to_string())?;
        }
        assets.save_files(asset_sources(&snapshot.scene), &scene)?;
        snapshot.scene.save(&scene)
    }

    fn root(&self) -> Option<PathBuf> {
        self.recovery_root
            .clone()
            .or_else(|| local_directory().map(|path| path.join("recovery")))
    }

    fn schedule(&mut self) {
        self.dirty = self.current.as_ref().is_some_and(|state| {
            self.baseline.as_ref().is_none_or(|baseline| {
                !Arc::ptr_eq(baseline, &state.scene) && !same_content(baseline, &state.scene)
            })
        });
        self.deadline = self.dirty().then(|| Instant::now() + RECOVERY_DELAY);
        if self.deadline.is_none() {
            self.discard_recovery();
        }
    }

    fn history_bytes(&self) -> usize {
        let mut seen = HashSet::new();
        self.past
            .iter()
            .chain(&self.future)
            .chain(&self.current)
            .map(|snapshot| {
                snapshot.scene_bytes()
                    + snapshot
                        .imports
                        .iter()
                        .filter(|(source, _)| seen.insert(source))
                        .map(|(_, loaded)| loaded.cost)
                        .sum::<usize>()
            })
            .sum()
    }
}

// Runtime checkpoints are managed by Play, Stop and Save. Editing history is independent of a playthrough.
fn same_content(left: &Scene, right: &Scene) -> bool {
    left.parts == right.parts
        && left.animation == right.animation
        && left.effect == right.effect
        && left.effects == right.effects
        && left.audio == right.audio
        && left.sprite_pixels == right.sprite_pixels
        && left.sprite_art == right.sprite_art
        && left.world == right.world
        && left.avatar == right.avatar
        && left.npcs == right.npcs
        && left.story == right.story
        && left.rules == right.rules
        && left.physics == right.physics
        && left.linked_scenes == right.linked_scenes
        && left.wind_rotor == right.wind_rotor
        && left.play_settings == right.play_settings
        && left.map_2d == right.map_2d
        && left.tactics == right.tactics
        && left.lighting == right.lighting
        && left.environment == right.environment
}

pub(crate) fn local_directory() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("StudioGames"))
}

#[derive(Clone)]
pub(crate) struct Recovery {
    pub scene: PathBuf,
    pub original: Option<PathBuf>,
    modified: SystemTime,
}

fn recoveries(root: &Path) -> Vec<Recovery> {
    let Some(entries) = fs::read_dir(root).ok() else {
        return Vec::new();
    };
    let mut result = entries
        .flatten()
        .filter_map(|entry| {
            if !entry.file_type().ok()?.is_dir() {
                return None;
            }
            let scene = entry.path().join(RECOVERY_FILE);
            let modified = scene.metadata().ok()?.modified().ok()?;
            let original = fs::read(entry.path().join(RECOVERY_META))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .flatten();
            Some(Recovery {
                scene,
                original,
                modified,
            })
        })
        .collect::<Vec<_>>();
    result.sort_by_key(|copy| std::cmp::Reverse(copy.modified));
    result
}

fn remove_recovery(directory: &Path) {
    let _ = fs::remove_file(directory.join(RECOVERY_FILE));
    let _ = fs::remove_file(directory.join(RECOVERY_META));
    let assets = directory.join(RECOVERY_FILE).with_extension("assets");
    if assets
        .symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_dir())
        && let Ok(entries) = fs::read_dir(&assets)
    {
        for asset in entries.flatten() {
            if asset.file_type().is_ok_and(|kind| kind.is_file())
                && asset.path().extension().is_some_and(|ext| ext == "glb")
            {
                let _ = fs::remove_file(asset.path());
            }
        }
    }
    let _ = fs::remove_dir(assets);
    let _ = fs::remove_dir(directory);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_follows_saved_content_and_replaces_the_redo_branch() {
        let assets = assets::Library::default();
        let mut scene = Scene::empty();
        let mut desk = Desk::default();
        desk.reset(&scene, &assets);
        scene.add_part(crate::scene::Shape::Box);
        scene.add_part(crate::scene::Shape::Sphere);
        assert!(desk.observe(&scene, &assets));
        assert!(desk.dirty());
        desk.saved();
        assert!(desk.travel(false).unwrap().scene.parts.is_empty());
        assert!(desk.dirty());
        assert_eq!(desk.travel(true).unwrap().scene.parts.len(), 2);
        assert!(!desk.dirty());
        scene = (*desk.travel(false).unwrap().scene).clone();
        scene.add_part(crate::scene::Shape::Cylinder);
        assert!(desk.observe(&scene, &assets));
        assert!(!desk.can_redo());
    }

    #[test]
    fn autosave_keeps_an_imported_model_and_never_overwrites_the_original() {
        let root = std::env::temp_dir().join(format!("studio-recovery-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let original = root.join("author.robot.json");
        let mut assets = assets::Library::default();
        let mut scene = Scene::empty();
        scene.save(&original).unwrap();
        let mut desk = Desk {
            recovery_root: Some(root.clone()),
            ..Desk::default()
        };
        desk.reset(&scene, &assets);
        let source = assets
            .import_bytes(include_bytes!("../demo_assets/quarter/bench_alt.glb").to_vec())
            .unwrap();
        scene.world.models.push(assets::Placement::at_ground(
            &[],
            &assets.get(&source).unwrap().model,
            source.clone(),
            "Тест".into(),
            [0.0; 3],
            2.0,
            0.0,
        ));
        desk.observe(&scene, &assets);
        desk.write_recovery(&assets, Some(&original)).unwrap();
        let entries = recoveries(&root);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].original.as_ref(), Some(&original));
        assert!(Scene::load(&original).unwrap().world.models.is_empty());
        assert_eq!(Scene::load(&entries[0].scene).unwrap(), scene);
        let mut reopened = assets::Library::default();
        reopened.ensure(&source, Some(&entries[0].scene)).unwrap();
        assert!(!reopened.get(&source).unwrap().model.images.is_empty());

        assert!(desk.travel(false).unwrap().scene.world.models.is_empty());
        assets.retain(std::iter::empty());
        assert!(assets.get(&source).is_none());
        assets.restore_cached(&desk.travel(true).unwrap().imports);
        assert!(assets.get(&source).is_some());
        desk.discard_recovery();
        fs::remove_file(original).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
