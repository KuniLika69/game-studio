use crate::{
    EditorTab, Studio, View,
    model::{Model, MorphWeight},
    npc_ui,
};

impl Studio {
    pub(super) fn active_speaker(&self) -> Option<crate::story::Speaker> {
        self.story_session.current(&self.scene.story).map(|(_, node)| node.speaker)
    }

    pub(super) fn focus_dialogue_face(&mut self, npc: u64) {
        let Some(actor) = self.scene.npcs.iter().find(|actor| actor.id == npc) else {
            return;
        };
        let Some(pose) = self.npc_poses().into_iter().find(|pose| pose.id == npc) else {
            return;
        };
        let [x, y, z] = pose.position;
        self.view = View::Three;
        self.camera.target = [x, y + actor.avatar.height * 0.91, z];
        self.camera.yaw = pose.yaw.to_radians();
        self.camera.pitch = 0.035;
        self.camera.distance = 0.86;
    }
    pub(super) fn dialogue_face_weights(&self, npc: u64, model: &Model) -> Vec<MorphWeight> {
        let time = if self.game_running {
            (self.active_speaker() == Some(crate::story::Speaker::Npc))
                .then(|| self.story_session.face_time(npc)).flatten()
        } else {
            self.editor_face_time(npc, crate::story::Speaker::Npc)
        };
        self.sample_dialogue_face(npc, time, model)
    }

    pub(super) fn player_dialogue_face(&self, model: &Model) -> Vec<MorphWeight> {
        if self.game_running && self.active_speaker() == Some(crate::story::Speaker::Player) {
            let Some(npc) = self.story_session.active_npc() else { return Vec::new(); };
            self.sample_dialogue_face(npc, self.story_session.face_time(npc), model)
        } else if !self.game_running && let Some(npc) = self.selected_npc {
            self.sample_dialogue_face(npc, self.editor_face_time(npc, crate::story::Speaker::Player), model)
        } else {
            Vec::new()
        }
    }

    fn editor_face_time(&self, npc: u64, speaker: crate::story::Speaker) -> Option<(u32, f32)> {
        if self.editor_tab != EditorTab::Npcs || self.npc_panel != npc_ui::Panel::Dialogue
            || self.selected_npc != Some(npc)
        {
            return None;
        }
        let time = self.face_preview.timeline_time(npc)?;
        let node = self.scene.story.conversation(npc)?.node(time.0)?;
        (node.speaker == speaker).then_some(time)
    }

    fn sample_dialogue_face(&self, npc: u64, time: Option<(u32, f32)>, model: &Model) -> Vec<MorphWeight> {
        let Some((lane, seconds)) = time
            .and_then(|(node, seconds)| self.scene.story.facial_lane(npc, node).map(|lane| (lane, seconds)))
        else { return Vec::new(); };
        let names = model.morph_names();
        let mut weights: Vec<MorphWeight> = Vec::new();
        for (name, weight) in lane.sample(seconds) {
            let Some(target) = names.binary_search_by(|target| target.as_str().cmp(name)).ok()
                .map(|index| &names[index])
                .or_else(|| names.iter().find(|target| target.eq_ignore_ascii_case(name))) else { continue };
            if let Some(previous) = weights.iter_mut().find(|value| &value.target == target) {
                if name == target {
                    previous.weight = weight;
                }
            } else {
                weights.push(MorphWeight { target: target.clone(), weight });
            }
        }
        weights
    }

    pub(super) fn dialogue_listener(
        &self,
        role: crate::story::Speaker,
        model: &Model,
        rotations: &mut Vec<crate::model::BoneRotation>,
    ) {
        let Some(npc) = self.story_session.active_npc() else { return };
        let Some((_, node)) = self.story_session.current(&self.scene.story) else { return };
        if node.voice.is_none() || node.speaker == role {
            return;
        }
        let Some((_, seconds)) = self.story_session.face_time(npc) else { return };
        crate::actor_motion::listen(model, rotations, seconds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Avatar, assets,
        face_timeline::{Key as FaceKey, Lane},
        npcs::{FIRST_NPC_ID, Npc, Waypoint},
        proof_input::{self, Driver, click, find_label},
        scene::Scene,
        story::Story,
    };
    use eframe::egui::{self, Key, Modifiers};

    #[test]
    fn live_dialogue_sends_authored_arkit_shapes_for_each_reply_to_renderers() {
        let npc = FIRST_NPC_ID;
        let mut studio = Studio::default();
        let source = assets::Source::Emma;
        studio.assets.ensure(&source, None).unwrap();
        let avatar = Avatar::new(
            source.clone(),
            "Эмма".into(),
            &studio.assets.get(&source).unwrap().model,
        );
        let mut scene = Scene::empty();
        scene
            .npcs
            .push(Npc::new(npc, avatar, vec![Waypoint::at([1.0, 0.0, 0.0])]));
        scene.story = Story::demo(npc, [0.0; 2]);
        let conversation = &mut scene.story.conversations[0];
        let entry = conversation.entry;
        let next = conversation.nodes[0].choices[0].target.unwrap();

        let mut blinking = Lane::default();
        blinking.add_key("eyeBlinkLeft", 0);
        blinking.update_key("eyeBlinkLeft", 0, FaceKey { ms: 0, weight: 0.0 });
        blinking.add_key("eyeBlinkLeft", 200);
        blinking.update_key(
            "eyeBlinkLeft",
            200,
            FaceKey {
                ms: 200,
                weight: 1.0,
            },
        );
        conversation.nodes[0].face = Some(blinking);
        let mut speaking = Lane::default();
        speaking.add_key("jawOpen", 0);
        conversation
            .nodes
            .iter_mut()
            .find(|node| node.id == next)
            .unwrap()
            .face = Some(speaking);
        scene.validate().unwrap();
        studio.standalone = true;
        studio.set_scene(scene, None, "");
        let ctx = egui::Context::default();
        let mut driver = Driver::new([1000, 700]);
        studio.start_play(&ctx, false);
        let _ = ctx.run(
            driver.raw(proof_input::key(Key::E, Modifiers::NONE)),
            |ctx| studio.run_frame(ctx),
        );
        assert_eq!(studio.story_session.face_time(npc), Some((entry, 0.0)));
        let initial = studio.npc_draws();
        assert_eq!(
            initial[0].morph_weights,
            [MorphWeight {
                target: "eyeBlinkLeft".into(),
                weight: 0.0
            }]
        );
        drop(initial);
        let mut output = None;
        for _ in 0..8 {
            output = Some(ctx.run(driver.raw(Vec::new()), |ctx| studio.run_frame(ctx)));
        }
        let samples = studio.npc_draws();
        assert!(samples[0].morph_weights[0].weight > 0.4);
        drop(samples);
        let point = find_label(&output.unwrap(), "Да, покажи, куда идти", 0).unwrap();
        let _ = ctx.run(driver.raw(click(point)), |ctx| studio.run_frame(ctx));
        assert_eq!(studio.story_session.face_time(npc), Some((next, 0.0)));
        assert_eq!(
            studio.npc_draws()[0].morph_weights,
            [MorphWeight {
                target: "jawOpen".into(),
                weight: 1.0
            }]
        );
    }
}
