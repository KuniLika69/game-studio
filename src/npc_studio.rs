use super::*;

impl Studio {
    pub(super) fn npc_list(&mut self, ui: &mut egui::Ui) {
        let mut create = None;
        ui.add_enabled_ui(
            !self.game_running && self.scene.npcs.len() < npcs::MAX_NPCS,
            |ui| {
                ui.menu_button("+ Создать NPC", |ui| {
                    self.creature_button(ui, creature_workshop::Target::NewNpc);
                    for (label, source) in [
                        ("Женщина — Эмма", Some(assets::Source::Emma)),
                        ("Мужчина — Райан", Some(assets::Source::Ryan)),
                        ("Копия героя", None),
                    ] {
                        if ui
                            .add_enabled(
                                source.is_some() || self.scene.avatar.is_some(),
                                egui::Button::new(label),
                            )
                            .clicked()
                        {
                            create = Some(source);
                            ui.close();
                        }
                    }
                });
            },
        );
        if let Some(source) = create {
            self.create_npc(source);
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for npc in &self.scene.npcs {
                if ui
                    .selectable_label(self.selected_npc == Some(npc.id), &npc.avatar.name)
                    .clicked()
                {
                    self.selected_npc = Some(npc.id);
                    self.npc_waypoint = 0;
                }
            }
        });
        if ui
            .add_enabled(
                !self.game_running && self.selected_npc.is_some(),
                egui::Button::new("Удалить выбранного NPC"),
            )
            .clicked()
            && let Some(id) = self.selected_npc
        {
            self.scene.npcs.retain(|npc| npc.id != id);
            self.scene.story.remove_npc(id);
            if self.human_target == Some(id) {
                self.human_target = None;
                self.human_editor_open = false;
            }
            self.selected_npc = self.scene.npcs.first().map(|npc| npc.id);
            self.npc_waypoint = 0;
            self.assets
                .retain(asset_sources(&self.scene).map(|(source, _)| source));
            self.sync_npc_assets();
            self.changed();
        }
    }

    fn create_npc(&mut self, source: Option<assets::Source>) {
        let Some(id) = npcs::next_id(&self.scene.npcs) else {
            return;
        };
        let mut avatar = if let Some(source) = source {
            if let Err(error) = self.assets.ensure(&source, self.project.as_deref()) {
                self.record_import_problem("Модель персонажа сцены", error);
                return;
            }
            let Some(base) = self.assets.get(&source) else {
                return;
            };
            Avatar::new(source, String::new(), &base.model)
        } else {
            let Some(avatar) = self.scene.avatar.clone() else {
                return;
            };
            avatar
        };
        avatar.name = format!("NPC {}", id - npcs::FIRST_NPC_ID + 1);
        let x = if self.scene.npcs.len().is_multiple_of(2) {
            2.0
        } else {
            -2.0
        };
        let z = 1.7 * (self.scene.npcs.len() / 2) as f32;
        self.scene.npcs.push(npcs::Npc::new(
            id,
            avatar,
            vec![
                npcs::Waypoint::at([x, 0.0, z]),
                npcs::Waypoint::at([x, 0.0, z - 1.25]),
            ],
        ));
        self.selected_npc = Some(id);
        self.npc_panel = npc_ui::Panel::Route;
        self.npc_waypoint = 0;
        self.sync_npc_assets();
        self.focus_camera();
        self.status = "NPC создан. Настрой маршрут или добавь диалог.".into();
        self.changed();
    }

    pub(super) fn npc_properties(&mut self, ui: &mut egui::Ui) {
        if let Some(id) = self.selected_npc {
            self.attachment_button(ui, character_attachments::Target::Npc(id));
            self.creature_button(ui, creature_workshop::Target::Npc(id));
        }
        npc_ui::tabs(ui, &mut self.npc_panel);
        let editing = !self.game_running;
        match self.npc_panel {
            npc_ui::Panel::Route => {
                let Some(npc) = self
                    .scene
                    .npcs
                    .iter_mut()
                    .find(|npc| Some(npc.id) == self.selected_npc)
                else {
                    ui.weak("Создай NPC в левой панели.");
                    return;
                };
                let shape_model = self.npc_assets.get(&npc.id).map(|asset| &asset.model);
                let mut shapes_changed = false;
                ui.add_enabled_ui(editing, |ui| {
                    if npc.avatar.source.editable_human()
                        && ui.button("Лицо и одежда NPC…").clicked()
                    {
                        self.human_target = Some(npc.id);
                        self.human_draft = human_editor::Draft::from_avatar(&npc.avatar);
                        self.human_editor_open = true;
                    }
                    if let Some(model) = shape_model {
                        shapes_changed = ui
                            .push_id(npc.id, |ui| {
                                crate::character_shapes::inspector(ui, model, &mut npc.avatar.shape_weights)
                            })
                            .inner;
                    }
                    npc.avatar.actor_tuning.inspector(ui);
                    npc_ui::route(
                        ui,
                        npc,
                        &self.scene.world.buildings,
                        &mut self.npc_waypoint,
                        &mut self.npc_place_on_plan,
                    );
                    if self.npc_place_on_plan && ui.button("Открыть 2D-план").clicked() {
                        self.view = View::Two;
                        self.overhead = true;
                    }
                });
                if self.game_running
                    && let Some(state) = self.npc_simulation.state(npc.id)
                {
                    let status = if state.blocked {
                        "Путь перекрыт"
                    } else if state.motion == Motion::Idle {
                        "Ожидает"
                    } else {
                        "Идёт"
                    };
                    ui.weak(format!("{status} · точка {}", state.waypoint + 1));
                }
                if shapes_changed {
                    self.changed();
                }
            }
            npc_ui::Panel::Dialogue => {
                if let Some(npc) = self
                    .scene
                    .npcs
                    .iter()
                    .find(|npc| Some(npc.id) == self.selected_npc)
                {
                    let npc_id = npc.id;
                    let actor_name = npc.avatar.name.clone();
                    let existed = self.scene.story.conversation(npc_id).is_some();
                    let morphs = self.npc_assets.get(&npc_id)
                        .map_or(&[] as &[String], |asset| asset.model.morph_names());
                    if editing && !morphs.is_empty() && ui.small_button("К лицу NPC").clicked() {
                        self.focus_dialogue_face(npc_id);
                    }
                    let morphs = self.npc_assets.get(&npc_id)
                        .map_or(&[] as &[String], |asset| asset.model.morph_names());
                    let player_model = self.avatar_asset().map(|(model, _)| model.clone());
                    if editing && player_model.is_some() && ui.small_button("К лицу игрока").clicked() {
                        let height = self.scene.avatar.as_ref().map_or(1.7, |avatar| avatar.height);
                        self.view = View::Three;
                        self.camera.target = [0.0, height * 0.87, 0.0];
                        self.camera.yaw = 0.18;
                        self.camera.pitch = 0.06;
                        self.camera.distance = 1.24;
                    }
                    let player_morphs = player_model.as_ref()
                        .map_or(&[] as &[String], |model| model.morph_names());
                    if story::inspector(
                        ui,
                        &mut self.scene.story,
                        npc_id,
                        editing,
                        &mut self.face_preview,
                        [morphs, player_morphs],
                        &self.scene.audio.sounds,
                    ) {
                        if !existed
                            && let Some(dialogue) = self
                                .scene
                                .story
                                .conversations
                                .iter_mut()
                                .find(|dialogue| dialogue.npc_id == npc_id)
                        {
                            dialogue.name = actor_name;
                        }
                        self.changed();
                    }
                } else {
                    ui.weak("Выбери NPC в левой панели.");
                }
            }
            npc_ui::Panel::Tasks => {
                let actors = self
                    .scene
                    .npcs
                    .iter()
                    .map(|npc| (npc.id, npc.avatar.name.as_str()))
                    .collect::<Vec<_>>();
                if story::quest_editor(
                    ui,
                    &mut self.scene.story,
                    editing,
                    &actors,
                    &mut self.npc_quest_on_plan,
                    &self.scene.world.buildings,
                ) {
                    self.changed();
                }
                if self.npc_quest_on_plan.is_some() {
                    self.view = View::Two;
                    self.overhead = true;
                }
            }
        }
    }

    pub(super) fn sync_npc_assets(&mut self) {
        self.npc_assets
            .retain(|id, _| self.scene.npcs.iter().any(|npc| npc.id == *id));
        self.npc_sprites
            .retain(|(id, _, _)| self.npc_assets.contains_key(id));
        for actor in &self.scene.npcs {
            let source = &actor.avatar.source;
            let appearance = actor
                .avatar
                .human_style
                .filter(|style| source.editable_human() && *style != model::HumanStyle::default());
            if self.npc_assets.get(&actor.id).is_some_and(|asset| {
                &asset.source == source
                    && asset.appearance == appearance
                    && asset.modular_parts == actor.avatar.modular_parts
                    && asset.bone_attachments == actor.avatar.bone_attachments
                    && asset.look == actor.avatar.look
                    && asset.imported_clips == actor.avatar.imported_clips
            }) {
                continue;
            }
            self.npc_sprites.retain(|(id, _, _)| *id != actor.id);
            self.npc_assets.remove(&actor.id);
            if let Ok(model) = self.model_for_avatar(&actor.avatar) {
                self.npc_assets.insert(
                    actor.id,
                    NpcAsset {
                        appearance,
                        modular_parts: actor.avatar.modular_parts.clone(),
                        bone_attachments: actor.avatar.bone_attachments.clone(),
                        look: actor.avatar.look.clone(),
                        imported_clips: actor.avatar.imported_clips.clone(),
                        source: source.clone(),
                        token: assets::render_token(),
                        model,
                    },
                );
            }
        }
    }

    pub(super) fn edit_npc_human(&mut self, id: u64, action: human_editor::Action) {
        let Some(npc) = self.scene.npcs.iter_mut().find(|npc| npc.id == id) else {
            return;
        };
        match action {
            human_editor::Action::Apply => {
                let source = self.human_draft.source.clone();
                if let Err(error) = self.assets.ensure(&source, self.project.as_deref()) {
                    self.record_import_problem("Модель персонажа сцены", error);
                    return;
                }
                let name = self.human_draft.name.trim();
                let name = if name.is_empty() {
                    npc.avatar.name.clone()
                } else {
                    name.to_owned()
                };
                if source != npc.avatar.source
                    && let Some(base) = self.assets.get(&source)
                {
                    npc.avatar = Avatar::new(source, name.clone(), &base.model);
                }
                npc.avatar.name = name;
                npc.avatar.height = self.human_draft.height.clamp(1.3, 2.25);
                npc.avatar.human_style = Some(self.human_draft.style);
                self.assets
                    .retain(asset_sources(&self.scene).map(|(source, _)| source));
                self.sync_npc_assets();
                self.status = "Внешность NPC сохранена в сцене.".into();
                self.changed();
            }
            human_editor::Action::Face => {
                let height = npc.avatar.height;
                if let Some(pose) = self.npc_poses().into_iter().find(|pose| pose.id == id) {
                    self.view = View::Three;
                    self.camera = Camera {
                        target: (Vec3::from_array(pose.position) + Vec3::Y * (height * 0.91))
                            .to_array(),
                        yaw: pose.yaw.to_radians() + 0.09,
                        pitch: 0.06,
                        distance: 0.78,
                    };
                }
            }
            human_editor::Action::FullBody => {
                self.editor_tab = EditorTab::Npcs;
                self.selected_npc = Some(id);
                self.view = View::Three;
                self.focus_camera();
            }
        }
    }

    pub(super) fn advance_npcs(&mut self, dt: f32) {
        if self.scene.npcs.is_empty() {
            return;
        }
        let colliders = self.colliders();
        let position = self.actor_position().to_array();
        self.npc_simulation.advance(
            &self.scene.npcs,
            &self.scene.world,
            &mut self.interactions,
            &colliders,
            Some(position),
            dt,
            self.story_session.active_npc(),
        );
        self.advance_npc_secondary(dt);
    }

    pub(super) fn npc_position(&self, id: u64) -> Option<Vec3> {
        if self.game_running
            && let Some(state) = self.npc_simulation.state(id)
        {
            return Some(Vec3::from_array(state.position()));
        }
        let npc = self.scene.npcs.iter().find(|npc| npc.id == id)?;
        Some(Vec3::from_array(npc.route.first()?.position))
    }

    pub(super) fn npc_poses(&self) -> Vec<NpcPose> {
        self.scene
            .npcs
            .iter()
            .filter_map(|npc| {
                if self.game_running
                    && let Some(state) = self.npc_simulation.state(npc.id)
                {
                    return Some(NpcPose {
                        id: npc.id,
                        position: state.position(),
                        yaw: state.yaw,
                        motion: state.motion,
                        elapsed: state.elapsed,
                        previous: state.previous,
                    });
                }
                let position = npc.route.first()?.position;
                let delta = npc.route.get(1).map_or(Vec3::Z, |point| {
                    Vec3::from_array(point.position) - Vec3::from_array(position)
                });
                Some(NpcPose {
                    id: npc.id,
                    position,
                    yaw: delta.x.atan2(delta.z).to_degrees(),
                    motion: Motion::Idle,
                    elapsed: 0.0,
                    previous: None,
                })
            })
            .collect()
    }

    pub(super) fn nearby_talk_npc(&self) -> Option<u64> {
        let player = self.actor_position();
        let mut candidates = self
            .scene
            .npcs
            .iter()
            .filter_map(|npc| {
                let position = self.npc_position(npc.id)?;
                let distance = position.distance_squared(player);
                (distance < 2.25
                    && (position.y - player.y).abs() < 0.75
                    && self.scene.story.can_talk(npc.id))
                .then_some((npc.id, position, distance))
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return None;
        }
        let colliders = self.colliders();
        candidates.sort_unstable_by(|a, b| a.2.total_cmp(&b.2));
        candidates.into_iter().find_map(|(id, pos, _)| {
            let from = (player + Vec3::Y * 0.95).to_array();
            let to = (pos + Vec3::Y * 0.95).to_array();
            (!colliders
                .iter()
                .any(|shape| shape.intersects_segment(from, to)))
            .then_some(id)
        })
    }

    pub(super) fn append_npc_colliders(&self, colliders: &mut Vec<world::Collider>) {
        for npc in self.scene.npcs.iter().filter(|npc| !npc.decorative) {
            let Some(position) = self.npc_position(npc.id) else {
                continue;
            };
            colliders.push(world::Collider {
                min: (position - Vec3::new(0.26, 0.0, 0.26)).to_array(),
                max: (position + Vec3::new(0.26, npc.avatar.height, 0.26)).to_array(),
            });
        }
    }
}
