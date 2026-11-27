use super::*;
use crate::{
    audio::{self, AmbientLevels, Clip, Footsteps, Listener, Mixer, Source, StepMeter, Surface, VoiceSettings},
    audio_project::{Library, MarkerSound, Sound, SoundId},
};
use std::{collections::BTreeMap, sync::Arc};

const MAX_RECORDING_FRAMES: usize = 300 * audio::OUTPUT_RATE as usize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub(super) struct RecordingStats {
    pub markers: usize,
    pub dialogue: usize,
    pub dialogue_player: usize,
    pub dialogue_npc: usize,
    pub door_contacts: usize,
    pub footsteps: [usize; 6],
    pub planted_footsteps: usize,
    pub cloth: usize,
    pub breaths: usize,
    pub inside_mix_frames: usize,
    pub rain_mix_frames: usize,
    pub peak_milli: u16,
    pub clipped_frames: usize,
    pub frames: usize,
}

struct Recording {
    path: PathBuf,
    mixer: Mixer,
    frames: Vec<[f32; 2]>,
    fractional_frames: f64,
    stats: RecordingStats,
    cues: Vec<RecordedCue>,
}

#[derive(serde::Serialize)]
struct RecordedCue {
    sample: usize,
    kind: &'static str,
    position: Option<[f32; 3]>,
    surface: Option<Surface>,
}

impl Recording {
    fn cue(&mut self, kind: &'static str, position: Option<Vec3>, surface: Option<Surface>) {
        if self.cues.len() < 4096 {
            self.cues.push(RecordedCue {
                sample: self.frames.len(), kind, position: position.map(|p| p.to_array()), surface,
            });
        }
    }

    fn step(&mut self, dt: f32, listener: Listener) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        self.fractional_frames += f64::from(dt) * f64::from(audio::OUTPUT_RATE);
        let frames = self.fractional_frames.floor() as usize;
        self.fractional_frames = self.fractional_frames.fract();
        let start = self.frames.len();
        let end = start.saturating_add(frames).min(MAX_RECORDING_FRAMES);
        self.frames.resize(end, [0.0; 2]);
        self.mixer.render(&mut self.frames[start..], listener);
        for [l, r] in &self.frames[start..] {
            let peak = l.abs().max(r.abs());
            self.stats.peak_milli = self.stats.peak_milli.max((peak * 1000.0).round() as u16);
            self.stats.clipped_frames += usize::from(peak >= 1.0);
        }
        self.stats.frames = end;
    }
}

#[derive(Default)]
pub(super) struct Runtime {
    sounds: BTreeMap<SoundId, (Sound, Arc<Clip>)>,
    footsteps: Option<Footsteps>,
    latch: Option<Arc<Clip>>,
    cloth: Option<Arc<Clip>>,
    breath: Option<Arc<Clip>>,
    area_mix: AmbientLevels,
    last_body: Option<(f32, f32, f32)>,
    cloth_delay: f32,
    step_meter: StepMeter,
    step_index: u64,
    dialogue: Option<(u64, u32, SoundId, story::Speaker)>,
    recording: Option<Recording>,
    listener: Listener,
    #[cfg(windows)]
    output: Option<crate::audio_output::Output>,
    #[cfg(windows)]
    device_allowed: bool,
}

impl Runtime {
    pub(super) fn allow_device(&mut self, allowed: bool) {
        #[cfg(windows)]
        {
            self.device_allowed = allowed;
            if !allowed {
                self.output = None;
            }
        }
        #[cfg(not(windows))]
        let _ = allowed;
    }

    fn has_consumer(&self) -> bool {
        self.recording.is_some() || self.allows_device()
    }

    fn allows_device(&self) -> bool {
        #[cfg(windows)]
        {
            self.device_allowed
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    fn play(&mut self, clip: Arc<Clip>, settings: VoiceSettings) -> Result<(), String> {
        self.play_on(clip, settings, false)
    }

    fn play_on(
        &mut self,
        clip: Arc<Clip>,
        settings: VoiceSettings,
        dialogue: bool,
    ) -> Result<(), String> {
        if let Some(recording) = &mut self.recording {
            let played = if dialogue {
                recording.mixer.play_dialogue(clip, settings)
            } else {
                recording.mixer.play(clip, settings)
            };
            return played
                .then_some(())
                .ok_or_else(|| "Некорректные параметры звука.".into());
        }
        #[cfg(windows)]
        if self.device_allowed {
            if let Some(error) = self.output.as_ref().and_then(|output| output.take_error()) {
                self.output = None;
                return Err(error);
            }
            let output = match &mut self.output {
                Some(output) => output,
                None => self.output.insert(crate::audio_output::Output::spawn()?),
            };
            output.listener(self.listener);
            let played = if dialogue {
                output.play_dialogue(clip, settings)
            } else {
                output.play(clip, settings)
            };
            return played
                .then_some(())
                .ok_or_else(|| "Устройство звука занято.".into());
        }
        let _ = (clip, settings, dialogue);
        Ok(())
    }

    fn cue(&mut self, cue: MarkerSound, sound: &Sound, origin: Vec3) -> Result<(), String> {
        self.play_cue(cue, sound, origin, false)
    }

    fn play_cue(
        &mut self,
        cue: MarkerSound,
        sound: &Sound,
        origin: Vec3,
        dialogue: bool,
    ) -> Result<(), String> {
        if !self.has_consumer() || cue.gain == 0.0 {
            return Ok(());
        }
        let clip = self.sound_clip(sound)?;
        let source = if cue.spatial {
            Source::World {
                position: origin + Vec3::from_array(cue.offset),
                reference_distance: 1.0,
                max_distance: cue.reach,
            }
        } else {
            Source::Direct
        };
        self.play_on(
            clip,
            VoiceSettings {
                source,
                gain: cue.gain,
            },
            dialogue,
        )
    }

    fn sound_clip(&mut self, sound: &Sound) -> Result<Arc<Clip>, String> {
        if let Some((cached, clip)) = self.sounds.get(&sound.id)
            && cached == sound
        {
            return Ok(clip.clone());
        }
        let clip = sound.clip()?;
        self.sounds.insert(sound.id, (sound.clone(), clip.clone()));
        Ok(clip)
    }

    fn stop_dialogue(&mut self) {
        if let Some(recording) = &mut self.recording {
            recording.mixer.stop_dialogue();
        }
        #[cfg(windows)]
        if let Some(output) = &mut self.output {
            output.stop_dialogue();
        }
    }

    fn footstep(
        &mut self,
        library: &Library,
        surface: Surface,
        source: Source,
    ) -> Result<(), String> {
        let gain = library.footsteps.gain;
        if !self.has_consumer() || gain == 0.0 {
            return Ok(());
        }
        let clip = if let Some(sound) = library.step_sample(surface, self.step_index) {
            self.sound_clip(sound)?
        } else {
            let footsteps = match &mut self.footsteps {
                Some(footsteps) => footsteps,
                None => self.footsteps.insert(Footsteps::new(audio::OUTPUT_RATE)?),
            };
            footsteps.sample(surface, self.step_index)
        };
        self.step_index = self.step_index.wrapping_add(1);
        self.play(clip, VoiceSettings { source, gain })
    }

    fn update_listener(&mut self, listener: Listener) {
        self.listener = listener;
        #[cfg(windows)]
        if let Some(output) = &self.output {
            output.listener(listener);
        }
    }

    fn advance(&mut self, dt: f32) {
        if let Some(recording) = &mut self.recording {
            recording.step(dt, self.listener);
        }
    }

    fn ambience(&mut self, level: AmbientLevels) -> Result<(), String> {
        if level == self.area_mix {
            return Ok(());
        }
        if let Some(recording) = &mut self.recording {
            recording.mixer.ambience(level);
        }
        #[cfg(windows)]
        if self.device_allowed {
            let output = match &mut self.output {
                Some(output) => output,
                None => self.output.insert(crate::audio_output::Output::spawn()?),
            };
            output.ambience(level);
        }
        self.area_mix = level;
        Ok(())
    }

    pub(super) fn reset_playback(&mut self) {
        self.sounds.clear();
        self.step_meter = StepMeter::default();
        self.step_index = 0;
        self.dialogue = None;
        self.area_mix = AmbientLevels::default();
        self.last_body = None;
        self.cloth_delay = 0.0;
        #[cfg(windows)]
        if let Some(output) = &self.output {
            output.clear();
            output.ambience(AmbientLevels::default());
        }
        if let Some(recording) = &mut self.recording {
            recording.mixer.clear();
        }
    }

    fn error(&mut self) -> Option<String> {
        #[cfg(windows)]
        if let Some(error) = self.output.as_ref().and_then(|output| output.take_error()) {
            self.output = None;
            return Some(error);
        }
        None
    }
}

impl Studio {
    pub(super) fn audio_properties(&mut self, ctx: &egui::Context) -> bool {
        if self.editor_tab != EditorTab::Audio {
            return false;
        }
        egui::SidePanel::right("audio_library")
            .default_width(310.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let zones: Vec<_> = self
                        .scene
                        .rules
                        .zones
                        .iter()
                        .map(|zone| (zone.id, zone.name.as_str()))
                        .collect();
                    let actions = ui
                        .add_enabled_ui(!self.game_running, |ui| {
                            ui.label("Метка анимации для звука и эффекта");
                            let animation = &mut self.scene.animation;
                            let marker_changed = ui
                                .add(
                                    egui::DragValue::new(&mut animation.event_time)
                                        .range(0.0..=animation.duration)
                                        .speed(0.01)
                                        .prefix("Метка ")
                                        .suffix(" с")
                                        .fixed_decimals(2),
                                )
                                .changed();
                            if self.scene.map_2d.is_some() {
                                let label = if self.preview {
                                    "Стоп анимации"
                                } else {
                                    "Воспроизвести анимацию"
                                };
                                if ui.button(label).clicked() {
                                    self.preview = !self.preview;
                                    self.time = 0.0;
                                }
                            }
                            let mut actions = self.audio_editor.show(
                                ui,
                                &mut self.scene.audio,
                                self.scene.animation.event_time,
                                &zones,
                            );
                            actions.changed |= marker_changed;
                            actions
                        })
                        .inner;
                    if actions.changed {
                        for node in self
                            .scene
                            .story
                            .conversations
                            .iter_mut()
                            .flat_map(|conversation| &mut conversation.nodes)
                        {
                            if node
                                .voice
                                .is_some_and(|id| self.scene.audio.sound(id).is_none())
                            {
                                node.voice = None;
                            }
                        }
                        self.changed();
                    }
                    if actions.import_wav
                        && let Some(path) = self.sound_path()
                    {
                        self.status = match self.scene.audio.import(&path) {
                            Ok(id) => {
                                self.audio_editor.select(id);
                                self.changed();
                                "WAV добавлен в проект.".into()
                            }
                            Err(error) => error,
                        };
                    }
                    if let Some(surface) = actions.import_steps && let Some(path) = self.sound_path() {
                        self.status = match self.scene.audio.import_step_recording(&path, surface) {
                            Ok(count) => {
                                self.changed();
                                format!("Найдено шагов: {count}. Материал «{}».", surface.label())
                            }
                            Err(error) => error,
                        };
                    }
                    if let Some(cue) = actions.preview {
                        let result = self.play_sound_from_actor(cue);
                        self.status = result.err().unwrap_or_else(|| "Предпросмотр звука.".into());
                    }
                    if let Some(surface) = actions.preview_step {
                        let result =
                            self.audio_runtime
                                .footstep(&self.scene.audio, surface, Source::Direct);
                        self.status = result.err().unwrap_or_else(|| "Предпросмотр шага.".into());
                    }
                    ui.separator();
                    ui.weak(&self.status);
                });
            });
        true
    }

    fn sound_path(&self) -> Option<PathBuf> {
        self.proof.as_ref().map_or_else(
            || {
                rfd::FileDialog::new()
                    .add_filter("PCM WAV", &["wav"])
                    .pick_file()
            },
            |proof| proof.wav_file.clone(),
        )
    }

    fn play_sound_from_actor(&mut self, cue: MarkerSound) -> Result<(), String> {
        let listener = self.scene_audio_listener();
        let feet = self.animation_origin();
        let Some(sound) = self.scene.audio.sound(cue.sound) else {
            return Err("Звук отсутствует в проекте.".into());
        };
        self.audio_runtime.update_listener(listener);
        self.audio_runtime.cue(cue, sound, feet + Vec3::Y * 0.9)
    }

    pub(super) fn play_marked_sound(&mut self, before: f32, after: f32) {
        let Some(cue) = self.scene.audio.marker else {
            return;
        };
        let marker = self.scene.animation.event_time;
        if !crate::effects::crossed_marker(before, after, marker) {
            return;
        }
        match self.play_sound_from_actor(cue) {
            Ok(()) => {
                if let Some(recording) = &mut self.audio_runtime.recording {
                    recording.stats.markers += 1;
                }
            }
            Err(error) => self.status = error,
        }
    }

    pub(super) fn audio_frame(&mut self, dt: f32, plants: [Option<Vec3>; 2], plant_feet: bool) {
        if let Some(error) = self.audio_runtime.error() {
            self.status = error;
        }
        if !self.audio_runtime.has_consumer()
            || (self.audio_runtime.recording.is_none() && self.scene.audio.is_default())
        {
            return;
        }
        let listener = self.scene_audio_listener();
        self.audio_runtime.update_listener(listener);
        self.advance_dialogue_audio();
        self.advance_area_audio(dt);
        let settings = &self.scene.audio.footsteps;
        if settings.enabled
            && self.game_running
            && !self.free_camera
            && self.seating.is_none()
            && self.tactics_battle.is_none()
            && !self.story_session.is_open()
        {
            let feet = self.game_position();
            let contact = self.map_player.as_ref().map_or_else(
                || self.game.grounded && self.game.horizontal_speed() > 0.15,
                map2d::Player::contact_for_footsteps,
            );
            let plants = if settings.gait_contacts && self.view == View::Three
                && self.scene.map_2d.is_none() && plant_feet
            {
                self.audio_runtime.step_meter = StepMeter::default();
                let [left, right] = plants.map(|plant| plant.filter(|_| contact));
                [(left, true), (right, true), (None, false)]
            } else {
                let count = self.audio_runtime.step_meter.advance(feet, contact, settings.stride);
                std::array::from_fn(|index| ((index < count).then_some(feet), false))
            };
            for (position, measured) in plants {
                let Some(position) = position else { continue; };
                let surface = settings.surface(|id| {
                    self.scene
                        .rules
                        .zone(id)
                        .is_some_and(|zone| zone.accepts(&self.scene.world, position.to_array()))
                });
                let source = Source::World {
                    position,
                    reference_distance: 1.35,
                    max_distance: 12.0,
                };
                match self
                        .audio_runtime
                        .footstep(&self.scene.audio, surface, source)
                    {
                        Ok(()) => {
                            if let Some(recording) = &mut self.audio_runtime.recording {
                                recording.stats.footsteps[surface as usize] += 1;
                                recording.stats.planted_footsteps += usize::from(measured);
                                recording.cue(if measured { "foot_plant" } else { "foot_distance" }, Some(position), Some(surface));
                            }
                        }
                        Err(error) => self.status = error,
                }
            }
        } else {
            self.audio_runtime.step_meter = StepMeter::default();
        }
        self.audio_runtime.advance(dt);
    }

    fn advance_area_audio(&mut self, dt: f32) {
        let settings = self.scene.audio.ambience;
        if !self.game_running || !settings.enabled || self.scene.map_2d.is_some() {
            if let Err(error) = self.audio_runtime.ambience(AmbientLevels::default()) {
                self.status = error;
            }
            self.audio_runtime.last_body = None;
            return;
        }
        let position = self.actor_position();
        let inside = self.scene.world.buildings.iter()
            .any(|building| building.contains_interior(position.x, position.y, position.z));
        let rain = self.environment_session.as_ref().zip(self.scene.environment.as_ref())
            .map_or(0.0, |(runtime, project)| runtime.sample(project).rain);
        let level = AmbientLevels {
            rain: rain * settings.rain * if inside { 0.12 } else { 1.0 },
            room: if inside { settings.room } else { 0.0 },
        };
        if let Err(error) = self.audio_runtime.ambience(level) {
            self.status = error;
        }
        if let Some(recording) = &mut self.audio_runtime.recording {
            recording.stats.inside_mix_frames += usize::from(level.room > 0.0);
            recording.stats.rain_mix_frames += usize::from(!inside && level.rain > 0.01);
        }
        let Some(avatar) = &self.scene.avatar else { return; };
        let breathing = self.player_secondary.breath(avatar.actor_tuning);
        let speed = self.game.horizontal_speed();
        let heading = self.avatar_heading;
        let before = self.audio_runtime.last_body.replace((speed, heading, breathing.phase));
        self.audio_runtime.cloth_delay = (self.audio_runtime.cloth_delay - dt).max(0.0);
        if self.story_session.is_open() || self.free_camera || self.seating.is_some() {
            return;
        }
        let Some((old_speed, old_heading, phase)) = before else { return; };
        let turn = ((heading - old_heading + 180.0).rem_euclid(360.0) - 180.0).abs();
        let cloth = settings.cloth > 0.0 && self.audio_runtime.cloth_delay <= 0.0
            && (old_speed.max(speed) >= 0.55 && (speed - old_speed).abs() >= 0.16 || turn >= 5.0);
        let breath = settings.breath > 0.0 && breathing.exertion > 0.20
            && phase < 0.42 && breathing.phase >= 0.42;
        let source = Source::World {
            position: position + Vec3::Y * 1.26,
            reference_distance: 1.1,
            max_distance: 7.0,
        };
        if cloth {
            let clip = self.audio_runtime.cloth.get_or_insert_with(|| Arc::new(Clip::body_noise(false))).clone();
            if self.audio_runtime.play(clip, VoiceSettings { source, gain: settings.cloth }).is_ok() {
                self.audio_runtime.cloth_delay = 0.33;
                if let Some(recording) = &mut self.audio_runtime.recording {
                    recording.stats.cloth += 1;
                    recording.cue("cloth", Some(position), None);
                }
            }
        }
        if breath {
            let clip = self.audio_runtime.breath.get_or_insert_with(|| Arc::new(Clip::body_noise(true))).clone();
            if self.audio_runtime.play(clip, VoiceSettings { source, gain: settings.breath * breathing.exertion }).is_ok()
                && let Some(recording) = &mut self.audio_runtime.recording
            {
                recording.stats.breaths += 1;
                recording.cue("breath", Some(position), None);
            }
        }
    }

    pub(super) fn play_door_latch(&mut self, position: Vec3) {
        if !self.audio_runtime.has_consumer() {
            return;
        }
        let listener = self.scene_audio_listener();
        self.audio_runtime.update_listener(listener);
        let clip = self
            .audio_runtime
            .latch
            .get_or_insert_with(|| Arc::new(Clip::door_latch()))
            .clone();
        let source = Source::World {
            position,
            reference_distance: 1.0,
            max_distance: 12.0,
        };
        match self
            .audio_runtime
            .play(clip, VoiceSettings { source, gain: 0.9 })
        {
            Ok(()) => {
                if let Some(recording) = &mut self.audio_runtime.recording {
                    recording.stats.door_contacts += 1;
                    recording.cue("latch", Some(position), None);
                }
            }
            Err(error) => self.status = error,
        }
    }

    fn advance_dialogue_audio(&mut self) {
        let line = (|| {
            let npc = self.story_session.active_npc()?;
            let (_, node) = self.story_session.current(&self.scene.story)?;
            Some((npc, node.id, node.voice?, node.speaker))
        })();
        if line == self.audio_runtime.dialogue {
            return;
        }
        self.audio_runtime.stop_dialogue();
        self.audio_runtime.dialogue = line;
        let Some((npc, _, id, speaker)) = line else {
            return;
        };
        let Some(sound) = self.scene.audio.sound(id) else {
            return;
        };
        let from = match speaker {
            story::Speaker::Npc => self.npc_position(npc).unwrap_or_else(|| self.actor_position()),
            story::Speaker::Player => self.actor_position(),
        } + Vec3::Y * 1.57;
        let mut cue = MarkerSound::new(id);
        cue.gain = 1.25;
        cue.reach = 18.0;
        match self.audio_runtime.play_cue(cue, sound, from, true) {
            Ok(()) => {
                if let Some(recording) = &mut self.audio_runtime.recording {
                    recording.stats.dialogue += 1;
                    match speaker {
                        story::Speaker::Npc => {
                            recording.stats.dialogue_npc += 1;
                            recording.cue("voice_npc", Some(from), None);
                        }
                        story::Speaker::Player => {
                            recording.stats.dialogue_player += 1;
                            recording.cue("voice_player", Some(from), None);
                        }
                    }
                }
            }
            Err(error) => self.status = error,
        }
    }

    fn scene_audio_listener(&self) -> Listener {
        if self.view == View::Two || self.scene.map_2d.is_some() {
            return Listener {
                position: self.animation_origin() + Vec3::Y * 1.4,
                right: Vec3::X,
            };
        }
        let yaw = self.camera.yaw + self.camera_shoulder;
        let right = Quat::from_rotation_y(yaw) * Vec3::X;
        let position = if self.game_running && !self.free_camera {
            self.actor_position() + Vec3::Y * 1.4
        } else {
            let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(1.0, 1.0));
            self.camera.projection(rect).0
        };
        Listener { position, right }
    }

    pub(super) fn set_audio_recording(&mut self, path: PathBuf) -> Result<(), String> {
        self.audio_runtime.allow_device(false);
        self.audio_runtime.recording = Some(Recording {
            path,
            mixer: Mixer::new(audio::OUTPUT_RATE)?,
            frames: Vec::new(),
            fractional_frames: 0.0,
            stats: RecordingStats::default(),
            cues: Vec::new(),
        });
        Ok(())
    }

    pub(super) fn audio_recording_stats(&self) -> Option<RecordingStats> {
        self.audio_runtime
            .recording
            .as_ref()
            .map(|recording| recording.stats)
    }

    pub(super) fn write_recorded_audio(&self) -> Result<(), String> {
        let recording = self
            .audio_runtime
            .recording
            .as_ref()
            .ok_or("Запись не запущена.")?;
        let bytes = audio::pcm16_stereo_wav(&recording.frames, audio::OUTPUT_RATE)?;
        std::fs::write(&recording.path, bytes)
            .map_err(|error| format!("Не удалось сохранить запись WAV. {error}"))?;
        if !recording.cues.is_empty() {
            let json = serde_json::to_vec_pretty(&recording.cues).map_err(|error| error.to_string())?;
            std::fs::write(recording.path.with_extension("events.json"), json)
                .map_err(|error| format!("Не удалось сохранить метки звука. {error}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod dialogue_tests {
    use super::*;

    #[test]
    fn voiced_line_starts_once_and_releases_the_dialogue_channel_when_closed() {
        let mut studio = Studio::default();
        let npc = crate::npcs::FIRST_NPC_ID;
        studio.scene.story = crate::story::Story::demo(npc, [0.0, 0.0]);
        crate::workshop_dialogue::author(&mut studio.scene, npc).unwrap();
        studio
            .set_audio_recording(std::env::temp_dir().join("studio-voice-test-unused.wav"))
            .unwrap();
        assert!(studio.story_session.open(&studio.scene.story, npc));
        for _ in 0..5 {
            studio.audio_frame(0.02, [None; 2], false);
        }
        assert_eq!(studio.audio_recording_stats().unwrap().dialogue, 1);
        assert!(
            studio
                .audio_runtime
                .recording
                .as_ref()
                .unwrap()
                .mixer
                .is_active()
        );
        studio.story_session.close();
        studio.audio_frame(0.02, [None; 2], false);
        assert!(
            !studio
                .audio_runtime
                .recording
                .as_ref()
                .unwrap()
                .mixer
                .is_active()
        );
    }
}
