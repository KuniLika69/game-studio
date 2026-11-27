use std::collections::BTreeMap;

use crate::{
    Studio, View,
    linked_scenes::ROOT_SCENE_ID,
    scene::{Checkpoint, Scene},
};
use eframe::egui;

pub(crate) struct Playthrough {
    pub root: Scene,
    pub active: u32,
    root_view: View,
    visited: BTreeMap<u32, Checkpoint>,
}

impl Studio {
    pub(super) fn prepare_linked_play(&mut self, resume: bool) {
        if self.scene.linked_scenes.is_empty() {
            return;
        }
        let checkpoint = self.scene.checkpoint.clone().filter(|_| resume);
        let active = checkpoint
            .as_ref()
            .and_then(|state| state.scene_id)
            .unwrap_or(ROOT_SCENE_ID);
        let mut play = Playthrough {
            root: self.scene.clone(),
            active,
            root_view: self.view,
            visited: BTreeMap::new(),
        };
        let checkpoint = checkpoint.map(|mut state| {
            play.visited = std::mem::take(&mut state.visited);
            state
        });
        if active == ROOT_SCENE_ID {
            self.scene.checkpoint = checkpoint;
        } else if let Some(link) = play.root.linked_scenes.get(active) {
            let mut scene = (*link.scene).clone();
            scene.checkpoint = checkpoint;
            self.switch_play_scene(scene, &link.name, View::Three);
        }
        self.link_playthrough = Some(play);
    }

    pub(super) fn set_link_targets(&mut self) {
        let root = self.chapter_root();
        let ids = root
            .linked_scenes
            .targets()
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        self.rule_session.set_transition_targets(ids);
    }

    pub(super) fn chapter_root(&self) -> &Scene {
        self.link_playthrough
            .as_ref()
            .map_or(&self.scene, |play| &play.root)
    }

    pub(super) fn full_checkpoint(&self) -> Option<Checkpoint> {
        let mut state = self.current_checkpoint()?;
        if let Some(play) = &self.link_playthrough {
            state.visited.clone_from(&play.visited);
        }
        Some(state)
    }

    pub(super) fn check_scene_transition(&mut self, ctx: &egui::Context) {
        // Consume scene-local effect commands before replacing their preset library.
        self.flush_rule_effects();
        let Some(id) = self.rule_session.take_transition_request() else {
            return;
        };
        let Some(mut play) = self.link_playthrough.take() else {
            return;
        };
        if play.active == id || !play.root.linked_scenes.contains(id) {
            self.link_playthrough = Some(play);
            return;
        }

        if let Some(mut state) = self.current_checkpoint() {
            state.scene_id = Some(play.active);
            play.visited.insert(play.active, state);
        }
        let (mut scene, name, view) = if let Some(link) = play.root.linked_scenes.get(id) {
            ((*link.scene).clone(), link.name.as_str(), View::Three)
        } else {
            (play.root.clone(), "Исходная сцена", play.root_view)
        };
        scene.checkpoint = play.visited.remove(&id).map(|mut state| {
            // Return to the entrance instead of the exit trigger that was just used.
            state.position = scene
                .map_2d
                .as_ref()
                .map_or([0.0; 3], |map| map.to_world(map.spawn));
            state
        });
        let resume = scene.checkpoint.is_some();
        self.switch_play_scene(scene, name, view);
        play.active = id;
        self.link_playthrough = Some(play);
        self.begin_play(ctx, resume);
        self.interaction_message = Some((self.status.clone(), 2.0));
    }

    pub(super) fn end_linked_play(&mut self) -> bool {
        let Some(mut play) = self.link_playthrough.take() else {
            return false;
        };
        play.root.checkpoint = self.scene.checkpoint.take();
        let project = self.project.clone();
        self.game_running = false;
        self.apply_scene(play.root, project, "Возврат к исходной сцене проекта.");
        self.view = play.root_view;
        true
    }

    fn switch_play_scene(&mut self, scene: Scene, name: &str, view: View) {
        let assets = self.assets.clone();
        let test = std::mem::take(&mut self.playback);
        let project = self.project.clone();
        self.game_running = false;
        self.apply_scene(scene, project, &format!("Сцена · {name}"));
        self.assets = assets;
        self.playback = test;
        self.view = view;
    }
}
