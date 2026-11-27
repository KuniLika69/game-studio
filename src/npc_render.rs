use super::*;

const NPC_SPRITE_FRAMES: u8 = 12;

impl Studio {
    pub(super) fn npc_draws(&self) -> Vec<ModelDraw<'_>> {
        let talking_to = self.story_session.active_npc();
        let player_eye_height = self
            .scene
            .avatar
            .as_ref()
            .map_or(1.63, |avatar| avatar.height * 0.86);
        let player_eye = self.actor_position() + Vec3::Y * player_eye_height;
        self.npc_poses()
            .into_iter()
            .filter_map(|pose| {
                let npc = self.scene.npcs.iter().find(|npc| npc.id == pose.id)?;
                let asset = self.npc_assets.get(&pose.id)?;
                let avatar = &npc.avatar;
                let foot = Vec3::from_array(pose.position);
                let (position, rotation, scale) = avatar.transform(&asset.model, foot, pose.yaw);
                let talking = talking_to == Some(pose.id);
                let sample_time = |motion, clock| {
                    clock
                        * avatar.animation_speed
                        * avatar.gait_rate(&asset.model, motion, npc.speed)
                };
                let elapsed = sample_time(pose.motion, pose.elapsed);
                let stride = |motion| match motion {
                    Motion::Idle => 0.0,
                    Motion::Walk => (npc.speed / 3.15).clamp(0.13, 0.76),
                    Motion::Run => 1.0,
                    Motion::Jump => 0.41,
                };
                let progress = (pose.elapsed / npcs::MOTION_TRANSITION_SECONDS).clamp(0.0, 1.0);
                let fade = progress * progress * (3.0 - 2.0 * progress);
                let blend = pose.previous.map(|(previous, clock)| BlendPose {
                    clip: avatar.clip(previous),
                    elapsed: sample_time(previous, clock),
                    weight: fade,
                });
                let gait = pose.previous.map_or(stride(pose.motion), |(previous, _)| {
                    stride(previous) * (1.0 - fade) + stride(pose.motion) * fade
                });
                let live_clock = self.live_seconds + (pose.id % 17) as f32 * 0.27;
                let mut bone_offsets =
                    avatar.bone_offsets(elapsed.rem_euclid(self.scene.animation.duration));
                let mut morph_weights = self.dialogue_face_weights(pose.id, &asset.model);
                crate::character_shapes::compose(&asset.model, &avatar.shape_weights, &mut morph_weights);
                let mut secondary = if self.game_running {
                    self.npc_secondary.get(&pose.id).map_or_else(
                        || model::SecondaryPose::natural(live_clock, 0.0),
                        |state| state.pose(live_clock),
                    )
                } else {
                    model::SecondaryPose::default()
                };
                if self.game_running {
                    let eye = foot + Vec3::Y * (avatar.height * 0.86);
                    let motion = actor_motion::Motion {
                        seconds: live_clock,
                        stride: gait,
                        grounded: pose.motion != Motion::Jump,
                        seated: false,
                        yaw: pose.yaw,
                        look_delta: talking.then(|| (player_eye - eye).to_array()),
                        talking: talking && self.active_speaker() == Some(story::Speaker::Npc)
                            && !actor_expression::owns_speech(&morph_weights),
                        breathing: self.npc_secondary.get(&npc.id)
                            .map(|state| state.breath(avatar.actor_tuning)),
                        upper_body_tilt: [0.0; 3],
                    };
                    let morph_gaze = actor_expression::animate(
                        &asset.model, &mut morph_weights, &mut secondary, &motion,
                        avatar.actor_tuning.face_strength, npc.id, talking,
                    );
                    actor_motion::append_with_eye_tracking(
                        &asset.model,
                        &mut bone_offsets,
                        motion,
                        if morph_gaze { 0.0 } else { avatar.actor_tuning.face_strength },
                    );
                    if talking {
                        self.dialogue_listener(story::Speaker::Npc, &asset.model, &mut bone_offsets);
                    }
                }
                let mut draw = ModelDraw {
                    token: asset.token | (1_u64 << 62),
                    model: &asset.model,
                    position,
                    rotation,
                    scale,
                    selected: !self.game_running
                        && self.editor_tab == EditorTab::Npcs
                        && self.selected_npc == Some(pose.id),
                    clip: avatar.clip(pose.motion),
                    elapsed,
                    blend,
                    tuning: assets::materials::Tuning::default(),
                    vegetation: [0.0; 4],
                    receives_weather: false,
                    graph: None,
                    material_preview: crate::material_graph::PreviewLight::Scene,
                    bone_offsets,
                    bone_translations: Vec::new(),
                    morph_weights,
                    secondary,
                };
                draw.secondary.hair_contacts = avatar.actor_tuning.hair_contact_quality(self.low_fx);
                self.apply_foot_locks(
                    pose.id,
                    &mut draw,
                    foot.y,
                    pose.motion != Motion::Jump,
                    pose.motion,
                );
                Some(draw)
            })
            .collect()
    }

    pub(super) fn paint_npc_plan(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        plan: plan2d::Plan,
    ) {
        if !self.game_running
            && self.editor_tab == EditorTab::Npcs
            && let Some(npc) = self
                .scene
                .npcs
                .iter()
                .find(|npc| Some(npc.id) == self.selected_npc)
        {
            let stroke = egui::Stroke::new(1.6_f32, ACCENT.gamma_multiply(0.76));
            for pair in npc.route.windows(2) {
                let [x0, _, z0] = pair[0].position;
                let [x1, _, z1] = pair[1].position;
                painter.line_segment([plan.point(x0, z0), plan.point(x1, z1)], stroke);
            }
            for (index, waypoint) in npc.route.iter().enumerate() {
                let [x, _, z] = waypoint.position;
                let center = plan.point(x, z);
                if plan.rect.contains(center) {
                    let chosen = self.npc_waypoint == index;
                    let color = if chosen {
                        Color32::from_rgb(255, 207, 102)
                    } else {
                        ACCENT
                    };
                    painter.circle_filled(center, if chosen { 5.0 } else { 3.0 }, color);
                }
            }
        }
        if self.game_running
            && let Some([x, z]) = self
                .story_session
                .active_destination_in(&self.scene.story, &self.scene.world)
        {
            let center = plan.point(x, z);
            if plan.rect.contains(center) {
                let pulse = (self.live_seconds * 3.8).sin() * 0.5 + 0.5;
                let color = Color32::from_rgb(255, 211, 98);
                painter.circle_filled(center, 11.0 + pulse * 3.0, color.gamma_multiply(0.28));
                painter.circle_stroke(
                    center,
                    10.0 + pulse * 4.0,
                    egui::Stroke::new(1.8_f32, color),
                );
                painter.add(egui::Shape::convex_polygon(
                    [
                        egui::vec2(0.0, -5.0),
                        egui::vec2(5.0, 0.0),
                        egui::vec2(0.0, 5.0),
                        egui::vec2(-5.0, 0.0),
                    ]
                    .map(|offset| center + offset)
                    .into_iter()
                    .collect(),
                    color,
                    egui::Stroke::NONE,
                ));
            }
        }
        let mut poses = self.npc_poses();
        poses.sort_by(|a, b| a.position[2].total_cmp(&b.position[2]));
        for pose in poses {
            let [x, y, z] = pose.position;
            let center = plan.point(x, z);
            if !plan.rect.expand(3.0 * plan.scale).contains(center) {
                continue;
            }
            let height = self
                .scene
                .npcs
                .iter()
                .find(|npc| npc.id == pose.id)
                .map(|npc| npc.avatar.height);
            let texture = self.npc_sprite(ui, pose, true);
            if texture.is_some() {
                plan.actor(painter, texture, [x, y, z], height);
            } else {
                painter.circle_filled(center, (plan.scale * 0.27).clamp(4.0, 13.0), MUTED);
            }
            if (!self.game_running
                && self.editor_tab == EditorTab::Npcs
                && self.selected_npc == Some(pose.id))
                || self.story_session.active_npc() == Some(pose.id)
            {
                painter.circle_stroke(
                    center,
                    (plan.scale * 0.53).clamp(6.0, 21.0),
                    egui::Stroke::new(1.8_f32, ACCENT),
                );
            }
        }
    }

    pub(super) fn paint_npc_side(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        origin: Pos2,
        camera_x: f32,
        units: f32,
    ) {
        let visible = painter.clip_rect();
        let mut poses = self.npc_poses();
        poses.sort_by(|a, b| a.position[2].total_cmp(&b.position[2]));
        let lane = self.actor_position().z;
        for pose in poses {
            let [x, y, z] = pose.position;
            if (z - lane).abs() > 2.2 {
                continue;
            }
            let Some(npc) = self.scene.npcs.iter().find(|npc| npc.id == pose.id) else {
                continue;
            };
            let edge = units * npc.avatar.height * (sprite::SIDE as f32 / 73.0);
            let foot = origin + egui::vec2(x * units - camera_x, -y * units);
            let frame = Rect::from_min_size(
                foot - egui::vec2(edge * 0.5, edge * sprite::FLOOR / sprite::SIDE as f32),
                Vec2::splat(edge),
            );
            if visible.intersects(frame)
                && let Some(texture) = self.npc_sprite(ui, pose, false)
            {
                painter.image(
                    texture,
                    frame,
                    Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }
    }

    fn npc_sprite(&mut self, ui: &egui::Ui, pose: NpcPose, plan: bool) -> Option<egui::TextureId> {
        let npc = self.scene.npcs.iter().find(|npc| npc.id == pose.id)?;
        let asset = self.npc_assets.get(&pose.id)?;
        let heading = pose.yaw + npc.avatar.yaw;
        let facing = sprite_mesh::direction(heading);
        let moving = pose.motion != Motion::Idle;
        let clip = npc.avatar.clip(pose.motion);
        let duration = clip
            .and_then(|index| asset.model.animations.get(index))
            .map_or(0.0, |animation| animation.duration);
        let phase = if duration > 0.0 {
            (pose.elapsed
                * npc.avatar.animation_speed
                * npc.avatar.gait_rate(&asset.model, pose.motion, npc.speed))
            .rem_euclid(duration)
                / duration
        } else {
            0.0
        };
        let frame = ((phase * f32::from(NPC_SPRITE_FRAMES)) as u8).min(NPC_SPRITE_FRAMES - 1);
        let (swing, secondary) = self
            .npc_secondary
            .get(&pose.id)
            .filter(|_| self.game_running && asset.model.has_secondary_pose())
            .map_or((0, model::SecondaryPose::default()), |state| {
                sprite_mesh::quantize_swing(state.pose(self.live_seconds))
            });
        let key = (
            pose.id,
            moving,
            sprite_mesh::FrameKey {
                direction: facing,
                frame,
                plan,
                swing,
            },
        );
        if let Some(texture) = self.npc_sprites.get(key) {
            return Some(texture);
        }
        let elapsed = duration * f32::from(frame) / f32::from(NPC_SPRITE_FRAMES);
        let offsets = npc
            .avatar
            .bone_offsets(elapsed.rem_euclid(self.scene.animation.duration));
        let rgba = sprite_mesh::rasterize_live(
            &asset.model,
            sprite_mesh::LivePose {
                clip,
                elapsed,
                yaw: heading,
                plan,
                bones: &offsets,
                secondary,
            },
        );
        let texture = ui.ctx().load_texture(
            format!("npc-{}-{facing}-{moving}-{frame}", pose.id),
            egui::ColorImage::from_rgba_unmultiplied([sprite_mesh::LIVE_SIDE; 2], &rgba),
            egui::TextureOptions::LINEAR,
        );
        Some(self.npc_sprites.insert(key, texture))
    }
}
