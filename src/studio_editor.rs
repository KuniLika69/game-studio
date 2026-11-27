#![cfg(feature = "editor")]

mod actor_expression;
mod actor_motion;
mod actor_physics;
mod actor_transitions;
mod ai;
mod ai_panel;
mod ai_scene;
mod assets;
mod avatar;
mod character_attachments;
mod chibi_editor;
mod creature_mesh;
mod creature_workshop;
mod creature_preview;
mod build_check;
mod character_shapes;
mod body_physics;
mod buildings;
mod character_builder;
mod character_library;
mod meridian;
mod editor_desk;
mod editor_door;
mod editor_history;
mod dungeon;
mod dungeon_map;
mod dungeon_ui;
mod fx_optimization;
mod editor_search;
mod editor_theme;
mod editor_workspace;
mod effect_rules;
mod extensions;
mod audio;
#[cfg(windows)]
mod audio_output;
mod audio_project;
mod audio_ui;
mod studio_audio;
mod effects_2d;
mod effects_ui;
mod environment_2d;
mod environment_ui;
mod face_editor;
mod flagship;
mod game_export;
mod game_trial;
mod game_plan;
mod game_wizard;
mod game_save;
mod gizmo;
mod human_editor;
mod import_problems;
mod inspector;
mod interior;
mod linked_scenes;
mod map2d;
mod material_graph_ui;
mod mechanics_pack;
mod mill;
mod model;
#[cfg(test)]
mod npc_integration_tests;
mod npc_render;
mod npc_studio;
mod npc_route3d;
mod npc_ui;
mod npcs;
mod object_library;
mod offscreen;
mod performance;
mod physics_editor;
mod physics_project;
mod plan2d;
mod play_settings;
mod proof;
mod proof_input;
mod proof_video;
mod quality;
mod quarter;
mod rain_workshop;
mod room_camera;
mod rules;
mod rules_ui;
mod scene;
mod scene_patch;
mod secondary_motion;
mod simulation_rules;
mod sprite;
mod sprite_mesh;
mod story;
mod studio_characters;
mod studio_clips;
mod studio_effects;
mod studio_extensions;
mod studio_face;
mod studio_environment;
mod studio_door_hand;
mod studio_links;
mod studio_models;
mod studio_physics;
mod studio_playback;
mod studio_tactics;
mod studio_welcome;
mod snow_tracks;
mod storybook_workshop;
mod surface_palette;
mod tactics_ui;
mod viewport3d;
mod village;
mod village_ui;
mod wind_rotor;
mod world;
mod world_palette;
mod workshop_dialogue;

use avatar::{Avatar, Motion};
use editor_theme::{ACCENT, BACKDROP, MUTED, PANEL};
use eframe::egui::{self, Color32, Key, Pos2, Rect, RichText, Sense, Vec2};
use glam::{EulerRot, Quat, Vec3};
use scene::{Game, MAX_PARTS, Part, Scene, Shape, SpritePixel};
use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use viewport3d::{BlendPose, Camera, ModelDraw, Spark, ViewportScene};
use studio_core::{effects, environment, face_timeline, material_graph, tactics};
pub use studio_core::ImportError;
use studio_rules::playback;


pub fn run() -> eframe::Result {
    let package =
        game_export::load_current().map_err(|error| eframe::Error::AppCreation(error.into()))?;
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if let [mode, output] = arguments.as_slice() && mode == "--verify-exported-game" {
        let result = package.ok_or_else(|| "В этом EXE нет игры.".to_owned())
            .and_then(|package| build_check::record(package, Path::new(output)));
        if let Err(error) = &result { eprintln!("{error}"); }
        return result.map_err(|error| eframe::Error::AppCreation(error.into()));
    }
    if arguments.first().is_some_and(|arg| {
        arg == "--background-proof"
            || arg == "--background-proof-player"
            || arg == "--background-proof-quarter"
            || arg == "--background-proof-quarter-player"
            || arg == "--background-v014"
            || arg == "--background-v015"
            || arg == "--background-v016"
            || arg == "--background-v017"
            || arg == "--background-v018"
            || arg == "--background-v019"
            || arg == "--background-v020"
            || arg == "--background-v021"
            || arg == "--background-rain-author"
            || arg == "--background-rain-player"
            || arg == "--background-rain-measure"
            || arg == "--background-actor-proof"
            || arg == "--background-flagship-author"
            || arg == "--background-v023-weather"
            || arg == "--background-flagship-player"
            || arg == "--background-v022-wizard"
            || arg == "--background-v022-onboarding"
            || arg == "--background-workspaces"
            || arg == "--background-world-palette"
            || arg == "--background-world-palette-player"
            || arg == "--background-coast-author"
            || arg == "--background-coast-player"
            || arg == "--background-weather-world-author"
            || arg == "--background-weather-world-player"
            || arg == "--background-storybook-author"
            || arg == "--background-storybook-player"
            || arg == "--background-mira-author"
            || arg == "--background-mira-player"
            || arg == "--background-v022-wizard-live"
            || arg == "--background-check-package"
            || arg == "--background-capture-trial-slot"
            || arg == "--background-v023-trial"
            || arg == "--background-v023-blank"
            || arg == "--background-v023-matrix"
            || arg == "--background-review-live"
            || arg == "--background-audio-author"
            || arg == "--background-audio-player"
            || arg == "--background-dungeon-author"
            || arg == "--background-dungeon-player"
            || arg == "--background-dungeon-metrics"
            || arg == "--background-fx-budget"
            || arg == "--background-v017-gateway"
            || arg == "--background-v016-measure"
            || arg == "--background-benchmark-export"
            || arg == "--background-staged-player"
            || arg == "--background-chapter"
            || arg == "--background-chapter-player"
            || arg == "--background-chapter-measure"
            || arg == "--background-chapter-soak-author"
            || arg == "--background-chapter-soak"
            || arg == "--background-chapter-render-smoke"
            || arg == "--background-games-023"
            || arg == "--background-legacy-maps-023"
            || arg == "--background-import-errors"
            || arg == "--background-dangling-links"
            || arg == "--background-hdr-post"
            || arg == "--background-transparency"
            || arg == "--background-vrm-decal"
            || arg == "--background-shadow-cascades"
            || arg == "--background-mechanics"
            || arg == "--background-mechanics-player"
            || arg == "--background-frame-telemetry"
    }) {
        return proof::run(&arguments, package)
            .map_err(|error| eframe::Error::AppCreation(error.into()));
    }
    let standalone = package.is_some();
    let run = if standalone {
        None
    } else {
        performance::Options::parse(arguments.into_iter())
            .map_err(|error| eframe::Error::AppCreation(error.into()))?
    };
    let quality = run.as_ref().map_or_else(quality::Profile::load, |run| {
        if run.capture {
            quality::Profile::High
        } else {
            quality::Profile::Low
        }
    });
    let samples = quality.samples();
    let mut options = eframe::NativeOptions {
        viewport: run.as_ref().map_or_else(
            || {
                egui::ViewportBuilder::default()
                    .with_app_id(if standalone {
                        "robot-studio-game"
                    } else {
                        "robot-studio"
                    })
                    .with_inner_size([1240.0, 720.0])
                    .with_maximized(true)
                    .with_min_inner_size([920.0, 660.0])
            },
            performance::Options::window,
        ),
        renderer: eframe::Renderer::Wgpu,
        depth_buffer: 24,
        multisampling: if samples == 1 { 0 } else { samples },
        ..Default::default()
    };
    if run.is_some() {
        options.vsync = false;
        options.wgpu_options.present_mode = wgpu::PresentMode::AutoNoVsync;
    }
    let title = if standalone {
        "Игра · Студия игр"
    } else {
        run.as_ref().map_or(
            concat!("Студия игр ", env!("CARGO_PKG_VERSION")),
            performance::Options::title,
        )
    };
    eframe::run_native(
        title,
        options,
        Box::new(move |cc| {
            viewport3d::install(cc, samples);
            configure_context(&cc.egui_ctx);
            let mut studio = if let Some(package) = package {
                let checksum = package.checksum;
                let mut studio = Studio::from_package(package, &cc.egui_ctx);
                studio.open_game_save(&cc.egui_ctx, game_save::path(checksum));
                studio
            } else {
                Studio::default()
            };
            studio.audio_runtime.allow_device(true);
            studio.telemetry = performance::Telemetry::new(
                editor_history::local_directory().map(|root| root.join("spike.log")),
            );
            studio.quality = quality::Panel::new(
                quality,
                cc.wgpu_render_state
                    .as_ref()
                    .map(|state| state.adapter.get_info().name),
            );
            studio.low_fx = quality == quality::Profile::Low;
            if let Some(path) = run.as_ref().and_then(|run| run.scene.as_ref()) {
                let scene =
                    Scene::load(path).map_err(|error| eframe::Error::AppCreation(error.into()))?;
                studio.set_scene(scene, Some(path.clone()), "Проверка сохранённой сцены");
            } else if !standalone && run.is_some() {
                studio.quarter_demo();
            } else if !standalone {
                studio.welcome = Some(studio_welcome::Page::Home);
                studio.workspace = editor_workspace::Workspace::Create;
            }
            if let Some(run) = run {
                if let Some(source) = &run.actor {
                    studio
                        .assets
                        .ensure(source, studio.project.as_deref())
                        .map_err(|error| eframe::Error::AppCreation(error.into()))?;
                    let name = if *source == assets::Source::Ryan {
                        "Райан"
                    } else {
                        "Эмма"
                    };
                    if let Some(actor) = studio.assets.get(source) {
                        studio.scene.avatar =
                            Some(Avatar::new(source.clone(), name.into(), &actor.model));
                        studio.sync_edited_human();
                    }
                }
                studio.view = if run.overhead { View::Two } else { View::Three };
                studio.overhead = run.overhead;
                studio.game_running = true;
                studio.show_panels_in_game = false;
                studio.camera.distance = if run.capture { 4.8 } else { 4.9 };
                studio.camera.yaw = if run.capture { -1.22 } else { 0.45 };
                studio.camera.pitch = if run.capture { 0.15 } else { 0.38 };
                let adapter = cc
                    .wgpu_render_state
                    .as_ref()
                    .map(|state| state.adapter.get_info());
                studio.performance = Some(performance::Runner::new(run, adapter));
            }
            Ok(Box::new(studio))
        }),
    )
}

fn configure_context(ctx: &egui::Context) {
    editor_theme::configure(ctx);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Three,
    Two,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Brush {
    None,
    Paint,
    Erase,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditorTab {
    Character,
    World,
    Npcs,
    Rules,
    Map,
    Tactics,
    Environment,
    Effects,
    Audio,
    Lighting,
}

struct EditedHuman {
    source: assets::Source,
    style: model::HumanStyle,
    model: Arc<model::Model>,
    modular_parts: Vec<avatar::ModularPart>,
    bone_attachments: Vec<avatar::BoneAttachment>,
    look: model::CharacterLook,
    imported_clips: Vec<avatar::ImportedClip>,
    token: u64,
}

struct NpcAsset {
    appearance: Option<model::HumanStyle>,
    modular_parts: Vec<avatar::ModularPart>,
    bone_attachments: Vec<avatar::BoneAttachment>,
    look: model::CharacterLook,
    imported_clips: Vec<avatar::ImportedClip>,
    source: assets::Source,
    token: u64,
    model: Arc<model::Model>,
}

#[derive(Clone, Copy)]
struct NpcPose {
    id: u64,
    position: [f32; 3],
    yaw: f32,
    motion: Motion,
    elapsed: f32,
    previous: Option<(Motion, f32)>,
}

struct Seating {
    seat: world::Seat,
    arrival: Vec3,
    original_heading: f32,
    amount: f32,
    leaving: bool,
}

impl Seating {
    fn weight(&self) -> f32 {
        self.amount * self.amount * (3.0 - 2.0 * self.amount)
    }

    fn position(&self, avatar: Option<&Avatar>) -> Vec3 {
        let mut seated = Vec3::from_array(self.seat.position);
        let height = avatar.map_or(1.75, |avatar| avatar.height);
        if avatar.is_some_and(|actor| actor.sit.is_some()) {
            let forward = Quat::from_rotation_y(self.seat.yaw.to_radians()) * Vec3::Z;
            seated += forward * (height * 0.17);
            seated.y -= height * 0.30;
        } else if avatar.is_some_and(|actor| {
            actor.source == assets::Source::CityHuman || actor.source.modern_human()
        }) {
            seated.y = (seated.y - height * 0.425).max(self.seat.stand[1] - height * 0.174);
        } else {
            seated.y = self.seat.stand[1];
        }
        let standing = if self.leaving {
            Vec3::from_array(self.seat.stand)
        } else {
            self.arrival
        };
        standing.lerp(seated, self.weight())
    }

    fn heading(&self) -> f32 {
        let arc = (self.seat.yaw - self.original_heading + 180.0).rem_euclid(360.0) - 180.0;
        self.original_heading + arc * self.weight()
    }
}

struct Studio {
    performance: Option<performance::Runner>,
    telemetry: performance::Telemetry,
    telemetry_continuous: bool,
    proof: Option<proof::Environment>,
    standalone: bool,
    licenses_open: bool,
    scene: Scene,
    project: Option<PathBuf>,
    desk: editor_history::Desk,
    search: editor_search::Search,
    quality: quality::Panel,
    game_settings_open: bool,
    playback: studio_playback::Panel,
    link_playthrough: Option<studio_links::Playthrough>,
    game_save: game_save::Controls,
    physics_runtime: studio_physics::Runtime,
    physics_editor: physics_editor::Editor,
    selected: Option<u64>,
    world_selection: Option<u64>,
    selected_door: Option<u64>,
    gizmo: gizmo::Editor,
    selected_npc: Option<u64>,
    npc_panel: npc_ui::Panel,
    npc_waypoint: usize,
    npc_place_on_plan: bool,
    npc_quest_on_plan: Option<u32>,
    npc_assets: HashMap<u64, NpcAsset>,
    npc_secondary: HashMap<u64, actor_physics::ActorSecondary>,
    npc_sprites: sprite_mesh::FrameCache<(u64, bool, sprite_mesh::FrameKey), 192>,
    npc_simulation: npcs::NpcSimulation,
    story_session: story::Session,
    face_preview: face_editor::Preview,
    rule_session: rules::Session,
    rule_editor: rules_ui::Editor,
    map_editor: map2d::Editor,
    tactics_editor: tactics_ui::Editor,
    tactics_play: tactics_ui::Play,
    tactics_battle: Option<tactics::Battle>,
    effects_editor: effects_ui::Editor,
    effects_runtime: studio_effects::Runtime,
    audio_runtime: studio_audio::Runtime,
    audio_editor: audio_ui::Editor,
    dungeon_editor: dungeon_ui::Editor,
    environment_editor: environment_ui::Editor,
    environment_session: Option<environment::Session>,
    rotor_angle: f32,
    environment_splashes: Vec<studio_environment::TimedSplash>,
    snow_tracks: snow_tracks::Tracks,
    map_player: Option<map2d::Player>,
    character_role: character_library::Role,
    character_position: [f32; 3],
    character_slot: model::PartSlot,
    attachment_target: Option<character_attachments::Target>,
    chibi_draft: Option<chibi_editor::Draft>,
    creature_workshop: Option<creature_workshop::Workshop>,
    build_check: Option<build_check::Job>,
    build_reference: Option<PathBuf>,
    map_sprite_editor: bool,
    assets: assets::Library,
    import_problems: import_problems::Panel,
    editor_tab: EditorTab,
    workspace: editor_workspace::Workspace,
    workspace_help_open: bool,
    status: String,
    view: View,
    overhead: bool,
    plan_offset: Vec2,
    plan_zoom: f32,
    walk_target: Option<[f32; 2]>,
    camera: Camera,
    free_camera: bool,
    dialogue_closeup: bool,
    direction: u8,
    brush: Brush,
    ink: Color32,
    time: f32,
    preview: bool,
    game_running: bool,
    show_panels_in_game: bool,
    game: Game,
    interactions: world::InteractionState,
    interaction_message: Option<(String, f32)>,
    seating: Option<Seating>,
    door_hand: Option<studio_door_hand::DoorHand>,
    door_contact: Option<studio_door_hand::ContactMeasure>,
    camera_follow: Vec3,
    camera_game_distance: Option<f32>,
    camera_shoulder: f32,
    live_seconds: f32,
    play_dt: f32,
    player_secondary: actor_physics::ActorSecondary,
    foot_locks: RefCell<HashMap<u64, (f32, model::FootLock)>>,
    avatar_motion: Motion,
    avatar_elapsed: f32,
    avatar_clip_time: f32,
    avatar_clip_rate: f32,
    avatar_heading: f32,
    avatar_previous: Option<(Motion, f32, f32)>,
    actor_transition: actor_transitions::Playback,
    avatar_landing: Option<f32>,
    effect_age: Option<f32>,
    effect_origin: Vec3,
    low_fx: bool,
    revision: u64,
    sprite_texture: Option<egui::TextureHandle>,
    live_player_sprites: sprite_mesh::FrameCache<(u8, Option<usize>, sprite_mesh::FrameKey), 128>,
    seat_sprite: Option<egui::TextureHandle>,
    sprite_key: Option<(u64, u8, u8, u32, u32, u32, u16)>,
    sprite_frame: u8,
    character_setup: bool,
    character_profile: character_builder::Profile,
    human_editor_open: bool,
    human_target: Option<u64>,
    human_draft: human_editor::Draft,
    edited_human: Option<EditedHuman>,
    preview_avatar: bool,
    preview_motion: Motion,
    preview_elapsed: f32,
    preview_external_clip: Option<usize>,
    selected_bone: String,
    ai_open: bool,
    ai: ai_panel::AiPanel,
    ai_review: ai_scene::Review,
    wizard: game_wizard::Wizard,
    welcome: Option<studio_welcome::Page>,
    restart_open: bool,
}

impl Default for Studio {
    fn default() -> Self {
        let scene = Scene::default();
        let selected = scene.parts.first().map(|part| part.id);
        Self {
            performance: None,
            telemetry: performance::Telemetry::new(None),
            telemetry_continuous: false,
            proof: None,
            standalone: false,
            licenses_open: false,
            scene,
            project: None,
            desk: editor_history::Desk::default(),
            search: editor_search::Search::default(),
            quality: quality::Panel::default(),
            game_settings_open: false,
            playback: studio_playback::Panel::default(),
            link_playthrough: None,
            game_save: game_save::Controls::default(),
            physics_runtime: studio_physics::Runtime::default(),
            physics_editor: physics_editor::Editor::default(),
            selected,
            world_selection: None,
            selected_door: None,
            gizmo: gizmo::Editor::default(),
            selected_npc: None,
            npc_panel: npc_ui::Panel::Route,
            npc_waypoint: 0,
            npc_place_on_plan: false,
            npc_quest_on_plan: None,
            npc_assets: HashMap::new(),
            npc_secondary: HashMap::new(),
            npc_sprites: sprite_mesh::FrameCache::default(),
            npc_simulation: npcs::NpcSimulation::default(),
            story_session: story::Session::default(),
            face_preview: face_editor::Preview::default(),
            rule_session: rules::Session::default(),
            rule_editor: rules_ui::Editor::default(),
            map_editor: map2d::Editor::default(),
            tactics_editor: tactics_ui::Editor::default(),
            tactics_play: tactics_ui::Play::default(),
            tactics_battle: None,
            effects_editor: effects_ui::Editor::default(),
            effects_runtime: studio_effects::Runtime::default(),
            audio_runtime: studio_audio::Runtime::default(),
            audio_editor: audio_ui::Editor::default(),
            dungeon_editor: dungeon_ui::Editor::default(),
            environment_editor: environment_ui::Editor::default(),
            environment_session: None,
            rotor_angle: 0.0,
            environment_splashes: Vec::new(),
            snow_tracks: snow_tracks::Tracks::default(),
            map_player: None,
            character_role: character_library::Role::default(),
            character_position: [2.0, 0.0, -1.0],
            character_slot: model::PartSlot::Head,
            attachment_target: None,
            chibi_draft: None,
            creature_workshop: None,
            build_check: None,
            build_reference: None,
            map_sprite_editor: false,
            assets: assets::Library::default(),
            import_problems: import_problems::Panel::default(),
            editor_tab: EditorTab::Character,
            workspace: editor_workspace::Workspace::Develop,
            workspace_help_open: false,
            status: "Демо-площадка открыта. Выбери «Окружение» или нажми «Играть».".into(),
            view: View::Three,
            overhead: false,
            plan_offset: Vec2::ZERO,
            plan_zoom: 1.0,
            walk_target: None,
            camera: Camera::default(),
            free_camera: false,
            dialogue_closeup: false,
            direction: 0,
            brush: Brush::None,
            ink: Color32::from_rgb(255, 192, 106),
            time: 0.0,
            preview: false,
            game_running: false,
            show_panels_in_game: false,
            game: Game::default(),
            interactions: world::InteractionState::default(),
            interaction_message: None,
            seating: None,
            door_hand: None,
            door_contact: None,
            camera_follow: Vec3::ZERO,
            camera_game_distance: None,
            camera_shoulder: 0.0,
            live_seconds: 0.0,
            play_dt: 0.0,
            player_secondary: actor_physics::ActorSecondary::default(),
            foot_locks: RefCell::default(),
            avatar_motion: Motion::Idle,
            avatar_elapsed: 0.0,
            avatar_clip_time: 0.0,
            avatar_clip_rate: 1.0,
            avatar_heading: 0.0,
            avatar_previous: None,
            actor_transition: actor_transitions::Playback::default(),
            avatar_landing: None,
            effect_age: None,
            effect_origin: Vec3::ZERO,
            low_fx: false,
            revision: 0,
            sprite_texture: None,
            live_player_sprites: sprite_mesh::FrameCache::default(),
            seat_sprite: None,
            sprite_key: None,
            sprite_frame: 0,
            character_setup: false,
            character_profile: character_builder::Profile::default(),
            human_editor_open: false,
            human_target: None,
            human_draft: human_editor::Draft::default(),
            edited_human: None,
            preview_avatar: false,
            preview_motion: Motion::Walk,
            preview_elapsed: 0.0,
            preview_external_clip: None,
            selected_bone: String::new(),
            ai_open: false,
            ai: ai_panel::AiPanel::default(),
            ai_review: ai_scene::Review::default(),
            wizard: game_wizard::Wizard::default(),
            welcome: None,
            restart_open: false,
        }
    }
}

impl eframe::App for Studio {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let started = Instant::now();
        let interval = ctx.input(|input| {
            (input.focused && input.viewport().minimized != Some(true))
                .then_some(f64::from(input.unstable_dt) * 1000.0)
        });
        self.run_frame(ctx);
        let now = Instant::now();
        let ui_ms = now.duration_since(started).as_secs_f64() * 1000.0;
        let frame_ms = if self.telemetry_continuous {
            interval.filter(|value| value.is_finite()).map_or(ui_ms, |value| value.max(ui_ms))
        } else {
            ui_ms
        };
        let scene_id = self.telemetry_scene_id();
        self.telemetry.record(now, scene_id, frame_ms);
        self.telemetry_continuous = interval.is_some()
            && (self.game_running || self.quality.open || self.performance.is_some());
        if self.quality.open && interval.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }
}

impl Studio {
    fn telemetry_scene_id(&self) -> u32 {
        self.link_playthrough.as_ref().map_or(linked_scenes::ROOT_SCENE_ID, |play| play.active)
    }

    fn from_package(package: game_export::GamePackage, ctx: &egui::Context) -> Self {
        let mut studio = Self {
            standalone: true,
            assets: package.assets,
            ..Self::default()
        };
        studio.set_scene(package.scene, None, "");
        studio.view = if package.start_in_2d {
            View::Two
        } else {
            View::Three
        };
        studio.overhead = true;
        studio.start_play(ctx, false);
        studio
    }

    fn run_frame(&mut self, ctx: &egui::Context) {
        if !self.standalone && self.show_welcome(ctx) {
            return;
        }
        if !self.standalone {
            self.editor_begin(ctx);
        }
        if !self.standalone && self.advance_playback(ctx) {
            // Replay advances the same game simulation on its recorded ticks.
        } else if let Some(mut run) = self.performance.take() {
            for step in run.tick(ctx) {
                self.advance_with_input(
                    ctx,
                    performance::STEP_SECONDS,
                    Some(performance::command(step)),
                );
                run.checkpoint(step, self.actor_position().to_array(), self.avatar_heading);
            }
            self.performance = Some(run);
        } else {
            self.advance(ctx);
        }
        if self.game_running {
            self.check_scene_transition(ctx);
        }
        if !self.standalone {
            let project = self.link_playthrough.as_ref().map_or(&self.scene, |play| &play.root);
            self.import_problems.update_references(project);
            self.header(ctx);
        }
        if !self.standalone && (!self.game_running || self.show_panels_in_game) {
            self.parts_panel(ctx);
            if self.wizard_enabled() {
                self.wizard_panel(ctx);
            } else {
                self.properties_panel(ctx);
            }
            if self.scene.map_2d.is_none() && !self.wizard_enabled()
                && (matches!(self.editor_tab, EditorTab::Audio | EditorTab::Effects)
                    || (self.workspace == editor_workspace::Workspace::Develop
                        && matches!(self.editor_tab, EditorTab::Character | EditorTab::Npcs)))
            {
                self.animation_panel(ctx);
            } else {
                egui::TopBottomPanel::bottom("editor_status").show(ctx, |ui| {
                    let error = self.status.starts_with("Не удалось")
                        || self.status.starts_with("Ошибка");
                    let text = RichText::new(if self.status.is_empty() { "Готово" } else { &self.status })
                        .color(if error { egui::Color32::LIGHT_RED } else { MUTED });
                    ui.add(egui::Label::new(text).truncate()).on_hover_text(&self.status);
                });
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            self.viewport(ui);
            if let Some(run) = &self.performance {
                run.observe_frame(ui);
            }
        });
        self.quality.show(ctx, self.standalone, &mut self.telemetry);
        self.show_game_save_controls(ctx);
        if !self.standalone {
            if let Some(check) = &mut self.build_check && let Some(result) = check.poll(ctx) {
                self.status = match result {
                    Ok(path) => {
                        let message = if path.join("сравнение.json").is_file() {
                            "EXE запустился. Есть кадры до и после."
                        } else { "EXE запустился. Отчёт и кадры записаны." };
                        let status = format!("{message} {}", path.display());
                        self.build_reference = Some(path);
                        status
                    }
                    Err(error) => format!("Сборка или проверка остановилась. {error}"),
                };
                self.build_check = None;
            }
            self.reset_dialog(ctx);
            self.dungeon_window(ctx);
            self.character_dialog(ctx);
            self.human_dialog(ctx);
            self.attachment_window(ctx);
            self.chibi_window(ctx);
            self.creature_window(ctx);
        }
        if self.licenses_open {
            self.licenses(ctx);
        }
        if self.game_running
            && !self.playback.replaying()
            && let Some(event) = self.story_session.show(ctx, &self.scene.story, self.dialogue_closeup)
        {
            self.dispatch_rule(event);
            self.record_playback_answer(event);
            self.check_scene_transition(ctx);
        }
        if !self.standalone {
            self.playback_panel(ctx);
            self.show_ai_scene(ctx);
            self.workspace_help(ctx);
            self.import_problems_window(ctx);
        }
        if !self.standalone && !self.game_running
            && let Some(draft) = self.ai.show(ctx, &mut self.ai_open)
        {
            let mut scene = self.scene.clone();
            scene.parts = draft.parts;
            scene.avatar = None;
            scene.animation = draft.animation;
            scene.sprite_pixels.retain(|pixel| !pixel.baked);
            let path = self.project.clone();
            self.apply_scene(scene, path, "Персонаж из черновика добавлен на площадку.");
        }
        if !self.standalone {
            self.editor_end(ctx);
        }
        if let Some(run) = &mut self.performance {
            run.overlay(ctx);
            run.after_update(ctx);
        } else if self.preview
            || self.game_running
            || self.effect_age.is_some()
            || (self.editor_tab == EditorTab::Npcs
                && self.npc_panel == npc_ui::Panel::Dialogue
                && self.face_preview.playing())
            || self.effects_active()
            || (self.preview_avatar && self.scene.avatar.is_some() && self.view == View::Three)
        {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.live_player_sprites.clear();
    }

    fn set_scene(&mut self, scene: Scene, path: Option<PathBuf>, status: &str) {
        if self.project != path || path.is_none() {
            self.build_reference = None;
            self.build_check = None;
        }
        self.desk.discard_recovery();
        self.apply_scene(scene, path, status);
        if !self.standalone {
            self.desk.reset(&self.scene, &self.assets);
            self.desk.watch_revision = self.revision;
            self.search = editor_search::Search::default();
        }
    }

    fn apply_scene(&mut self, scene: Scene, path: Option<PathBuf>, status: &str) {
        self.stop();
        self.scene = scene;
        self.human_editor_open = false;
        self.attachment_target = None;
        self.chibi_draft = None;
        self.creature_workshop = None;
        self.human_target = None;
        self.selected_bone.clear();
        self.sprite_frame = 0;
        self.plan_offset = Vec2::ZERO;
        self.plan_zoom = 1.0;
        self.camera = Camera::default();
        self.overhead = has_street(&self.scene) || !self.scene.world.buildings.is_empty();
        self.project = path;
        self.selected = self.scene.parts.first().map(|part| part.id);
        self.world_selection = None;
        self.selected_door = None;
        self.gizmo.clear();
        self.selected_npc = self.scene.npcs.first().map(|actor| actor.id);
        self.npc_waypoint = 0;
        self.npc_place_on_plan = false;
        self.npc_panel = npc_ui::Panel::Route;
        self.rule_editor = rules_ui::Editor::default();
        self.face_preview = face_editor::Preview::default();
        self.physics_editor = physics_editor::Editor::default();
        self.map_editor = map2d::Editor::default();
        self.tactics_editor = tactics_ui::Editor::default();
        self.effects_editor = effects_ui::Editor::default();
        self.environment_editor = environment_ui::Editor::default();
        self.map_sprite_editor = false;
        self.editor_tab = if self.scene.map_2d.is_some() {
            EditorTab::Map
        } else {
            EditorTab::Character
        };
        self.status = status.into();
        self.import_problems.clear();
        self.assets
            .retain(asset_sources(&self.scene).map(|(source, _)| source));
        for (source, label) in asset_sources(&self.scene) {
            if let Err(error) = self.assets.ensure(source, self.project.as_deref()) {
                self.status = format!("Модель «{label}» не загружена. {error}");
                self.import_problems.record(label, error);
            }
        }
        self.sync_edited_human();
        self.sync_npc_assets();
        self.changed();
    }

    fn advance(&mut self, ctx: &egui::Context) {
        self.advance_with_input(
            ctx,
            ctx.input(|input| input.stable_dt.clamp(0.0, 0.05)),
            None,
        );
    }

    fn advance_with_input(
        &mut self,
        ctx: &egui::Context,
        dt: f32,
        scripted: Option<performance::Input>,
    ) {
        if !(self.game_running && self.view == View::Three && self.story_session.is_open()) {
            self.dialogue_closeup = false;
        } else if ctx.input(|input| input.key_pressed(Key::F8)) {
            self.dialogue_closeup = !self.dialogue_closeup;
        }
        if self.game_running
            && scripted.is_none()
            && ctx.input(|input| input.key_pressed(Key::Escape))
        {
            if self.story_session.is_open() {
                self.story_session.close();
            } else if self.licenses_open {
                self.licenses_open = false;
            } else if self.free_camera {
                self.free_camera = false;
            } else if self.standalone {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.stop();
            }
            return;
        }
        let intent = self.read_play_intent(ctx, dt, scripted);
        if self.game_running && !self.free_camera && scripted.is_none() {
            self.record_playback_intent(intent);
        }
        self.advance_intent(ctx, intent);
    }

    fn read_play_intent(
        &self,
        ctx: &egui::Context,
        dt: f32,
        scripted: Option<performance::Input>,
    ) -> playback::Intent {
        let mut intent = playback::Intent {
            dt,
            target: self.walk_target,
            ..Default::default()
        };
        if !self.game_running || self.free_camera {
            return intent;
        }
        if let Some(input) = scripted {
            intent.movement = input.movement;
            intent.sprint = input.sprint;
            intent.jump = input.jump;
            return intent;
        }
        if self.ai_open
            || self.restart_open
            || self.character_setup
            || self.human_editor_open
            || self.licenses_open
        {
            return intent;
        }
        ctx.input(|input| {
            let control = play_settings::read_keys(&self.scene.play_settings, input);
            let [side, z] = control.movement;
            intent.movement =
                if self.scene.map_2d.is_some() || self.view == View::Two && self.overhead {
                    [side, z]
                } else if self.view == View::Three {
                    let direction = Quat::from_rotation_y(self.camera.yaw + self.camera_shoulder)
                        * Vec3::new(side, 0.0, z);
                    [direction.x, direction.z]
                } else {
                    [(side - z).clamp(-1.0, 1.0), 0.0]
                };
            intent.sprint = control.sprint;
            intent.jump = control.jump;
            intent.interact = control.interact;
            intent.mark = control.mark;
            intent.retry = control.retry;
        });
        intent
    }

    fn advance_intent(&mut self, ctx: &egui::Context, intent: playback::Intent) {
        self.simulate_intent(ctx, intent);
        let (plants, plant_feet) = self.foot_locks.get_mut().get_mut(&0)
            .map(|(_, lock)| (lock.take_plants(), lock.has_plant_feet()))
            .unwrap_or_default();
        self.harvest_wet_footsteps(plants);
        self.audio_frame(intent.dt, plants, plant_feet);
    }

    fn simulate_intent(&mut self, ctx: &egui::Context, intent: playback::Intent) {
        let dt = intent.dt;
        self.advance_effects(dt);
        if self.game_running {
            self.story_session.advance_face(dt);
        } else if self.editor_tab == EditorTab::Npcs && self.npc_panel == npc_ui::Panel::Dialogue {
            self.face_preview.advance(dt, &self.scene.story);
        }
        self.effect_age = self
            .effect_age
            .map(|age| age + dt)
            .filter(|&age| age < self.scene.effect.duration);
        if self.game_running {
            if self.tactics_battle.is_some() {
                return;
            }
            if self.free_camera {
                if self.map_player.is_none() {
                    self.advance_body_physics(dt, [0.0; 2], false);
                    self.live_seconds += dt;
                    self.advance_player_secondary(dt, [0.0; 2]);
                    self.advance_npc_secondary(dt);
                }
                self.simulate_rules(dt);
                self.advance_free_camera(ctx, dt);
                return;
            }
            self.advance_rule_clocks(dt, intent.mark, intent.retry);
            if self.reset_attempt_if_requested() {
                return;
            }
            if self.map_player.is_some() {
                self.simulate_rules(dt);
                if self.reset_attempt_if_requested() {
                    return;
                }
                self.advance_map(dt, intent);
                return;
            }
            self.play_dt = dt;
            self.live_seconds += dt;
            self.avatar_landing = self.avatar_landing.map(|t| t + dt).filter(|&t| t < 0.24);
            if let Some((_, left)) = &mut self.interaction_message {
                *left -= dt;
                if *left <= 0.0 {
                    self.interaction_message = None;
                }
            }
            self.advance_interactions(dt, intent.interact);
            if self.reset_attempt_if_requested() {
                return;
            }
            self.advance_npcs(dt);
            let before = self.actor_position().to_array();
            self.advance_body_physics(dt, intent.movement, intent.jump);
            self.simulate_rules(dt);
            if self.reset_attempt_if_requested() {
                return;
            }
            if self.advance_seating(dt) {
                self.update_quest(before);
                self.advance_player_secondary(dt, [0.0; 2]);
                return;
            }
            if self.story_session.is_open() {
                self.game.clear_motion();
                self.actor_transition = actor_transitions::Playback::default();
                self.step_avatar_motion(Motion::Idle, 0.0, dt);
                self.advance_player_secondary(dt, [0.0; 2]);
                return;
            }
            let intent = self.door_hand_controls(dt).unwrap_or(intent);
            let playback::Intent {
                movement: mut motion,
                sprint,
                jump,
                ..
            } = intent;
            self.target_walk_motion(&mut motion, sprint, jump);
            let stride = self.measured_player_stride();
            let modern = self
                .scene
                .avatar
                .as_ref()
                .is_some_and(|avatar| avatar.source.modern_human());
            if stride.is_none() && (modern || has_street(&self.scene)) {
                let pace = match (modern, sprint) {
                    (true, true) => 0.62,
                    (true, false) => 0.47,
                    (false, true) => 0.80,
                    (false, false) => 0.58,
                };
                motion = (glam::Vec2::from_array(motion).clamp_length_max(1.0) * pace).to_array();
            }
            let direction_input = motion;
            let (motion, turn_target) = stride.map_or((motion, None), |_| self.stride_turn_input(motion));
            let mut colliders = self.colliders();
            self.append_npc_colliders(&mut colliders);
            let incoming_speed = self.game.vertical_velocity();
            if self.game.update_configured(
                dt,
                motion,
                sprint,
                jump,
                &colliders,
                stride.map_or(self.scene.play_settings.movement_3d, |stride| stride.movement),
            ) {
                self.effect_age = Some(0.0);
                self.effect_origin = Vec3::new(self.game.x, self.game.height, self.game.z);
                if incoming_speed < -2.6 {
                    self.avatar_landing = Some(0.0);
                }
            }
            self.update_quest(before);
            if self.reset_attempt_if_requested() {
                return;
            }
            let follow = Vec3::new(self.game.x, self.game.height * 0.42, self.game.z);
            self.camera_follow = self.camera_follow.lerp(
                follow,
                1.0 - (-dt * self.scene.play_settings.camera.follow_rate).exp(),
            );
            let desired_heading = self.door_hand_heading().or(turn_target).unwrap_or(self.game.yaw);
            if stride.is_some() {
                self.step_stride_heading(desired_heading, dt);
            } else {
                let turn = (desired_heading - self.avatar_heading + 180.0).rem_euclid(360.0) - 180.0;
                let heading_step = (turn * (1.0 - (-17.0 * dt).exp())).clamp(-595.0 * dt, 595.0 * dt);
                self.avatar_heading = (self.avatar_heading + heading_step).rem_euclid(360.0);
            }
            let speed = self.game.horizontal_speed();
            let motion_kind = if !self.game.grounded {
                Motion::Jump
            } else if speed < 0.16 {
                Motion::Idle
            } else if (sprint && speed > stride.map_or(2.25, |stride| stride.enter_run))
                || (self.avatar_motion == Motion::Run
                    && (speed > stride.map_or(2.6, |stride| stride.keep_running)
                        || motion == [0.0; 2]))
            {
                Motion::Run
            } else {
                Motion::Walk
            };
            self.advance_actor_transition(motion_kind, direction_input, speed, dt);
            self.step_avatar_motion(motion_kind, speed, dt);
            self.advance_door_palm();
            self.advance_player_secondary(dt, direction_input);
            self.advance_motion_marker(dt, motion != [0.0, 0.0] || !self.game.grounded, sprint);
        } else if self.preview {
            let old = self.time;
            self.time = (self.time + dt).rem_euclid(self.scene.animation.duration);
            let marker = self.scene.animation.event_time;
            self.play_marked_sound(old, self.time);
            if !self.play_marked_effect(old, self.time)
                && effects::crossed_marker(old, self.time, marker)
            {
                self.effect_age = Some(0.0);
            }
        }
        if !self.game_running && self.preview_avatar && self.view == View::Three {
            let before = self.preview_elapsed;
            self.preview_elapsed += dt;
            self.live_seconds += dt;
            let wind = self.player_hair_wind();
            let tuning = self.scene.avatar.as_ref().map_or_else(avatar::ActorTuning::default, |actor| actor.actor_tuning);
            self.player_secondary
                .preview(dt, self.preview_elapsed, self.preview_motion, wind, tuning);
            if !self.preview {
                self.advance_rig_marker(before);
            }
        }
    }

    fn advance_rig_marker(&mut self, before: f32) {
        if self.scene.audio.marker.is_none() && self.scene.effects.marker_effect.is_none() {
            return;
        }
        let Some(avatar) = &self.scene.avatar else {
            return;
        };
        let clip = self
            .preview_external_clip
            .or_else(|| avatar.clip(self.preview_motion));
        let Some(duration) = self
            .avatar_asset()
            .and_then(|(model, _)| model.animations.get(clip?))
            .map(|clip| clip.duration)
            .filter(|&duration| duration > 0.0 && self.scene.animation.event_time <= duration)
        else {
            return;
        };
        let old = (before * avatar.animation_speed).rem_euclid(duration);
        let next = (self.preview_elapsed * avatar.animation_speed).rem_euclid(duration);
        self.play_marked_effect(old, next);
        self.play_marked_sound(old, next);
    }

    fn advance_motion_marker(&mut self, dt: f32, active: bool, sprint: bool) {
        if !active {
            self.time = 0.0;
            return;
        }
        let previous_time = self.time;
        let rate = if sprint { 1.35 } else { 1.0 };
        self.time = (self.time + dt * rate).rem_euclid(self.scene.animation.duration);
        self.play_marked_effect(previous_time, self.time);
        self.play_marked_sound(previous_time, self.time);
    }

    fn step_avatar_motion(&mut self, motion: Motion, speed: f32, dt: f32) {
        let fade = self.player_blend_seconds();
        let old = self.avatar_motion;
        let avatar = self.scene.avatar.as_ref();
        let model = self.avatar_asset();
        let rate = self.measured_player_stride().map_or_else(
            || avatar.zip(model).map_or(1.0, |(avatar, (model, _))| avatar.gait_rate(model, motion, speed)),
            |stride| stride.rate(motion, speed),
        );
        if motion != old {
            // Reversing an incomplete fade must keep the currently projected pose.
            // Smoothstep mirrors exactly when exchanging both clips and 1 - progress.
            let reverse = self.avatar_previous.filter(|(previous, _, _)| {
                *previous == motion && !matches!(motion, Motion::Jump) && old != Motion::Jump
            });
            let (next_time, next_elapsed) = if let Some((_, time, _)) = reverse {
                (time, (fade - self.avatar_elapsed).max(0.0))
            } else {
                let time = avatar.zip(model).map_or(0.0, |(avatar, (model, token))| {
                    self.player_gait_transition(avatar, model, token, old, motion, self.avatar_clip_time)
                });
                (time, 0.0)
            };
            let previous_rate =
                if motion == Motion::Idle && matches!(old, Motion::Walk | Motion::Run) {
                    0.0
                } else {
                    self.avatar_clip_rate
                };
            self.avatar_previous = Some((old, self.avatar_clip_time, previous_rate));
            self.avatar_clip_time = next_time;
            self.avatar_clip_rate = rate;
            self.avatar_motion = motion;
            self.avatar_elapsed = next_elapsed;
        } else {
            self.avatar_clip_rate += (rate - self.avatar_clip_rate) * (1.0 - (-dt * 16.0).exp());
            self.avatar_clip_time += self.avatar_clip_rate * dt;
            self.avatar_elapsed += dt;
        }
        if let Some((_, time, rate)) = &mut self.avatar_previous {
            *time += dt * *rate;
            if self.avatar_elapsed >= fade {
                self.avatar_previous = None;
            }
        }
    }

    fn stop(&mut self) {
        self.audio_runtime.reset_playback();
        self.finish_playback_record();
        self.playback.abandon_run();
        self.remember_checkpoint();
        self.physics_runtime = studio_physics::Runtime::default();
        self.environment_stop();
        self.effects_runtime = studio_effects::Runtime::default();
        self.rotor_angle = 0.0;
        if self.end_linked_play() {
            return;
        }
        self.preview = false;
        self.preview_avatar = false;
        self.preview_elapsed = 0.0;
        self.preview_external_clip = None;
        self.game_running = false;
        self.map_player = None;
        self.tactics_battle = None;
        self.game.reset();
        self.interactions = world::InteractionState::default();
        self.interaction_message = None;
        self.seating = None;
        self.door_hand = None;
        self.door_contact = None;
        self.seat_sprite = None;
        self.npc_simulation.reset(&self.scene.npcs);
        self.story_session = story::Session::default();
        self.rule_session = rules::Session::default();
        self.free_camera = false;
        self.walk_target = None;
        self.npc_quest_on_plan = None;
        self.camera_follow = Vec3::ZERO;
        self.camera_game_distance = None;
        self.camera_shoulder = 0.0;
        self.live_seconds = 0.0;
        self.play_dt = 0.0;
        self.player_secondary = actor_physics::ActorSecondary::default();
        self.npc_secondary.clear();
        self.foot_locks.get_mut().clear();
        self.avatar_motion = Motion::Idle;
        self.avatar_elapsed = 0.0;
        self.avatar_clip_time = 0.0;
        self.avatar_clip_rate = 1.0;
        self.avatar_heading = 0.0;
        self.avatar_previous = None;
        self.actor_transition = actor_transitions::Playback::default();
        self.avatar_landing = None;
        self.effect_age = None;
        self.effect_origin = Vec3::ZERO;
        self.time = 0.0;
    }

    fn remember_checkpoint(&mut self) {
        if let Some(checkpoint) = self.full_checkpoint() {
            self.scene.checkpoint = Some(checkpoint);
        }
    }

    fn current_checkpoint(&self) -> Option<scene::Checkpoint> {
        self.game_running.then(|| {
            scene::Checkpoint {
                position: self.game_position().to_array(),
                scene_id: self.link_playthrough.as_ref().map(|play| play.active),
                rotor_angle: self.scene.wind_rotor.as_ref().map(|_| self.rotor_angle),
                tactics: self.tactics_battle.as_ref().map(tactics::Battle::snapshot),
                physics: self.physics_runtime.snapshot(),
                environment: self.environment_snapshot(),
                story: self.story_session.snapshot(),
                interactions: self.interactions.snapshot(),
                rules: self.rule_session.snapshot(),
                map_respawn: self
                    .map_player
                    .as_ref()
                    .and_then(map2d::Player::respawn_cell),
                visited: Default::default(),
            }
        })
    }

    fn can_play(&self) -> bool {
        if let Some(playable) = self.tactics_can_play() {
            return playable;
        }
        self.scene.map_2d.is_some()
            || !self.scene.physics.is_empty()
            || !self.scene.parts.is_empty()
            || !self.scene.sprite_pixels.is_empty()
            || self.avatar_asset().is_some()
    }

    fn start_play(&mut self, ctx: &egui::Context, resume: bool) {
        self.stop();
        self.prepare_linked_play(resume);
        self.begin_play(ctx, resume);
    }

    fn begin_play(&mut self, ctx: &egui::Context, resume: bool) {
        self.environment_start(resume);
        if self.start_tactical_battle(ctx, resume) {
            return;
        }
        self.rule_session = rules::Session::new(&self.scene.rules);
        self.rule_session.set_effects(&self.scene.effects);
        self.set_link_targets();
        self.rule_session
            .set_environment(self.scene.environment.as_ref());
        self.rotor_angle = if resume {
            self.scene
                .checkpoint
                .as_ref()
                .and_then(|point| point.rotor_angle)
                .unwrap_or(0.0)
        } else {
            0.0
        };
        self.map_player = self.scene.map_2d.as_ref().map(map2d::Player::new);
        if self.map_player.is_none() {
            self.start_body_physics(resume);
        }
        if resume && let Some(checkpoint) = &self.scene.checkpoint {
            [self.game.x, self.game.height, self.game.z] = checkpoint.position;
            if let Some(player) = &mut self.map_player
                && let Some(map) = &self.scene.map_2d
            {
                player.resume(map, checkpoint.position);
                player.restore_respawn(map, checkpoint.map_respawn);
            }
            self.camera_follow = Vec3::new(self.game.x, self.game.height * 0.42, self.game.z);
            self.story_session
                .restore(&self.scene.story, &checkpoint.story);
            self.interactions
                .restore(&self.scene.world, &checkpoint.interactions);
            self.rule_session
                .restore(&self.scene.rules, &self.scene.story, &checkpoint.rules);
        }
        self.sync_object_activity();
        ctx.memory_mut(|memory| {
            if let Some(widget) = memory.focused() {
                memory.surrender_focus(widget);
            }
        });
        self.game_running = true;
        if has_street(&self.scene) {
            self.camera.distance = 4.9;
        }
        if let Some(distance) = self.scene.play_settings.camera.distance {
            self.camera.distance = distance;
        }
        self.free_camera = self.scene.play_settings.camera.mode == play_settings::CameraMode::Free;
    }

    fn target_walk_motion(&mut self, movement: &mut [f32; 2], sprint: bool, jump: bool) {
        if (self.view == View::Two && !self.overhead) || *movement != [0.0; 2] || jump {
            self.walk_target = None;
        }
        if let Some([x, z]) = self.walk_target {
            let delta = glam::Vec2::new(x - self.game.x, z - self.game.z);
            let distance = delta.length();
            if distance < 0.115 {
                self.walk_target = None;
            } else {
                let slowing_distance = if sprint { 1.2 } else { 0.72 };
                *movement =
                    (delta / distance * (distance / slowing_distance).clamp(0.24, 1.0)).to_array();
            }
        }
    }

    fn header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(5.0);
            ui.horizontal_wrapped(|ui| {
                editor_theme::studio_mark(ui, 29.0);
                ui.label(RichText::new("Студия игр").size(23.0).strong());
                self.workspace_switcher(ui);
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui.button("Сохранить").clicked() {
                    self.save(false);
                }
                if self.game_running {
                    if ui.button(RichText::new("■ Стоп").color(ACCENT)).clicked() {
                        self.stop();
                    }
                } else {
                    let can_play = self.can_play();
                    if ui
                        .add_enabled(
                            can_play,
                            egui::Button::new(RichText::new("▶ Играть").strong().color(BACKDROP))
                                .fill(ACCENT),
                        )
                        .on_hover_text("Начать проверку сцены с начала")
                        .clicked()
                    {
                        self.start_play(ctx, false);
                    }
                    if self.scene.checkpoint.is_some()
                        && ui
                            .add_enabled(can_play, egui::Button::new("Продолжить"))
                            .clicked()
                    {
                        self.start_play(ctx, true);
                    }
                }
                if ui
                    .add_enabled(
                        !self.game_running && self.can_play(),
                        egui::Button::new("Собрать игру для Windows"),
                    )
                    .clicked()
                {
                    self.export_game(false);
                }
                if ui.add_enabled(!self.game_running && self.can_play() && self.build_check.is_none(), egui::Button::new("Собрать и проверить"))
                    .on_hover_text("Запускает собранный EXE в отдельной папке и записывает проверочные кадры и время рендера.")
                    .clicked() {
                    self.export_game(true);
                }
                self.wizard_button(ui);
                ui.separator();
                if ui.button("Новый…").clicked() {
                    self.request_replace(ctx, editor_history::Replace::New);
                }
                if ui.button("Открыть…").clicked() {
                    self.request_replace(ctx, editor_history::Replace::Open);
                }
                if ui.button("Сохранить как…").clicked() {
                    self.save(true);
                }
                self.editor_commands(ui);
                self.import_problems_button(ui);
                if self.workspace == editor_workspace::Workspace::Develop {
                    self.playback_button(ui);
                    if ui.button("Экспорт спрайтов…").clicked() {
                        self.export_sprites();
                    }
                    self.ai_scene_button(ui);
                    if ui.button("Нейросети…").clicked() {
                        self.ai_open = !self.ai_open;
                    }
                }
            });
            ui.add_space(4.0);
        });
    }

    fn parts_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("parts")
            .default_width(208.0)
            .min_width(170.0)
            .show(ctx, |ui| {
                ui.add_space(5.0);
                self.workspace_tabs(ui);
                ui.separator();
                if self.editor_tab == EditorTab::Map {
                    ui.weak("Рисунок, слои и коллизии карты");
                    return;
                }
                if self.editor_tab == EditorTab::Effects {
                    ui.weak("Настройки и библиотека эффектов в правой панели");
                    return;
                }
                if self.editor_tab == EditorTab::Audio {
                    ui.weak("Звуки проекта и материалы шагов в правой панели");
                    return;
                }
                if self.editor_tab == EditorTab::Environment {
                    ui.weak("Время суток, погода, ветер и вода в правой панели");
                    return;
                }
                if self.editor_tab == EditorTab::Tactics {
                    ui.weak("Бойцы и условие победы в правой панели");
                    return;
                }
                if self.editor_tab == EditorTab::Lighting {
                    ui.weak("Настройки освещения сцены");
                    return;
                }
                if self.editor_tab == EditorTab::Rules {
                    ui.weak("Событие → условие → действие");
                    return;
                }
                if self.editor_tab == EditorTab::Npcs {
                    self.npc_list(ui);
                    return;
                }
                if self.editor_tab == EditorTab::World {
                    self.model_list(ui);
                    self.door_choices(ui);
                    ui.separator();
                    if inspector::key_list(
                        ui,
                        &mut self.scene.world,
                        &mut self.world_selection,
                        !self.game_running,
                        [self.plan_offset.x + 1.4, 0.0, self.plan_offset.y],
                    ) {
                        self.changed();
                    }
                    let buildings_before = self.scene.world.buildings.len();
                    if world::list(
                        ui,
                        &mut self.scene.world,
                        &mut self.world_selection,
                        !self.game_running,
                    ) {
                        self.changed();
                    }
                    if self.scene.world.buildings.len() > buildings_before {
                        self.overhead = true;
                    }
                    return;
                }
                let advanced_character =
                    !self.game_running && self.workspace == editor_workspace::Workspace::Develop;
                if advanced_character {
                    self.character_library_tools(ui);
                }
                self.avatar_panel(ui);
                if !self.game_running {
                    if advanced_character {
                        self.animation_clip_tools(ui);
                    } else {
                        ui.collapsing("Библиотека и клипы", |ui| {
                            self.character_library_tools(ui);
                            self.animation_clip_tools(ui);
                        });
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(!self.game_running, egui::Button::new("Собрать персонажа…"))
                        .clicked()
                    {
                        self.character_setup = true;
                    }
                    if ui
                        .add_enabled(!self.game_running, egui::Button::new("Рисовать спрайт"))
                        .clicked()
                    {
                        self.view = View::Two;
                        self.overhead = false;
                        self.brush = Brush::Paint;
                        self.scene.sprite_art.drawn_only = true;
                        self.map_sprite_editor = self.scene.map_2d.is_some();
                        self.changed();
                    }
                });
                ui.separator();
                if self.view == View::Three && self.scene.avatar.is_some() {
                    return;
                }
                ui.label(RichText::new(format!("{} деталей", self.scene.parts.len())).color(MUTED));
                ui.add_enabled_ui(!self.game_running, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (caption, shape) in [
                            ("+ Блок", Shape::Box),
                            ("+ Сфера", Shape::Sphere),
                            ("+ Цилиндр", Shape::Cylinder),
                        ] {
                            if ui
                                .add_enabled(
                                    self.scene.parts.len() < MAX_PARTS,
                                    egui::Button::new(caption),
                                )
                                .clicked()
                            {
                                self.selected = Some(self.scene.add_part(shape));
                                self.changed();
                            }
                        }
                    });
                    if ui
                        .add_enabled(self.selected.is_some(), egui::Button::new("Удалить деталь"))
                        .clicked()
                        && let Some(id) = self.selected
                    {
                        self.scene.remove_part(id);
                        self.selected = self.scene.parts.first().map(|part| part.id);
                        self.changed();
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for part in &self.scene.parts {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("■").color(rgb(part.color)));
                                if ui
                                    .selectable_label(self.selected == Some(part.id), &part.name)
                                    .clicked()
                                {
                                    self.selected = Some(part.id);
                                }
                            });
                        }
                    });
            });
    }

    fn properties_panel(&mut self, ctx: &egui::Context) {
        if self.effect_properties(ctx)
            || self.audio_properties(ctx)
            || self.environment_properties(ctx)
            || self.tactical_properties(ctx)
        {
            return;
        }
        if self.editor_tab == EditorTab::Lighting {
            use studio_core::render_settings::ShadowQuality;

            egui::SidePanel::right("lighting_settings")
                .default_width(300.0)
                .show(ctx, |ui| {
                    let mut quality = u8::from(
                        self.scene.post_process.shadow_quality == ShadowQuality::Cascaded,
                    );
                    let changed = ui
                        .add_enabled(
                            !self.game_running,
                            egui::Slider::new(&mut quality, 0..=1)
                                .text("Качество теней")
                                .show_value(false),
                        )
                        .on_hover_text("Обычные тени подходят для слабых видеокарт. Два каскада повышают детализацию теней рядом с камерой.")
                        .changed();
                    let (mode, label) = if quality == 0 {
                        (ShadowQuality::Single, "Обычные тени")
                    } else {
                        (ShadowQuality::Cascaded, "2 каскада")
                    };
                    ui.weak(label);
                    if changed {
                        self.scene.post_process.shadow_quality = mode;
                        self.changed();
                    }
                    ui.separator();
                    if assets::materials::lighting_inspector(ui, &mut self.scene.lighting) {
                        self.changed();
                    }
                });
            return;
        }
        if self.editor_tab == EditorTab::Map {
            egui::SidePanel::right("map_tools")
                .default_width(290.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if !self.game_running {
                            self.dungeon_editor.check(ui, &self.scene, self.revision);
                        }
                        let result = ui
                            .add_enabled_ui(self.tactics_battle.is_none(), |ui| {
                                self.map_editor.tools(ui, &mut self.scene)
                            })
                            .inner;
                        if result.changed {
                            self.changed();
                        }
                        if result.import_png {
                            self.import_map_png();
                        }
                    });
                });
            return;
        }
        if self.editor_tab == EditorTab::Rules {
            egui::SidePanel::right("rule_properties")
                .default_width(415.0)
                .min_width(320.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.add_enabled_ui(!self.game_running, |ui| {
                            self.reference_banner(ui);
                            self.extension_tools(ui);
                            if linked_scenes::inspector(
                                ui,
                                &mut self.scene,
                                &mut self.assets,
                                &mut self.status,
                            ) {
                                self.changed();
                            }
                            if self.rule_editor.show(ui, &mut self.scene) {
                                self.changed();
                            }
                            if let Some(error) = self.rule_editor.take_import_error() {
                                self.record_import_problem("Библиотека правил", error);
                            }
                        });
                    });
                });
            return;
        }
        egui::SidePanel::right("properties")
            .default_width(295.0)
            .min_width(265.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(5.0);
                    self.reference_banner(ui);
                    if self.editor_tab == EditorTab::Npcs {
                        self.npc_properties(ui);
                        return;
                    }
                    if self.editor_tab == EditorTab::World {
                        self.village_tools(ui);
                        if self.door_properties(ui) {
                            return;
                        }
                        self.physics_editor_tools(ui);
                        if let Some(item) = self
                            .scene
                            .world
                            .models
                            .iter_mut()
                            .find(|item| Some(item.id) == self.world_selection)
                        {
                            let id = item.id;
                            let replace = !self.game_running
                                && ui.button("Заменить модель GLB…").clicked();
                            let changed = assets::inspector(
                                ui,
                                item,
                                !self.game_running,
                                self.assets.get(&item.source).map(|asset| asset.model.as_ref()),
                            );
                            if changed {
                                self.changed();
                            }
                            if replace {
                                self.replace_world_model(id);
                            }
                            if !self.game_running {
                                self.surface_palette(ui, id);
                            }
                        } else {
                            let mut open_plan = false;
                            if let Some(changed) = inspector::key_properties(
                                ui,
                                &mut self.scene.world,
                                self.world_selection,
                                !self.game_running,
                                &mut open_plan,
                            ) {
                                if open_plan {
                                    self.view = View::Two;
                                    self.overhead = true;
                                }
                                if changed {
                                    self.changed();
                                }
                                return;
                            }
                            if world::inspector(
                                ui,
                                &mut self.scene.world,
                                self.world_selection,
                                !self.game_running,
                            ) {
                                self.changed();
                            }
                        }
                        return;
                    }
                    if self.view == View::Three
                        && let Some(avatar) = &mut self.scene.avatar
                    {
                        let model = self
                            .edited_human
                            .as_ref()
                            .filter(|edited| edited.source == avatar.source)
                            .map(|edited| edited.model.as_ref())
                            .or_else(|| self.assets.get(&avatar.source).map(|loaded| loaded.model.as_ref()));
                        let advanced = self.workspace == editor_workspace::Workspace::Develop;
                        avatar::inspector(ui, avatar, model, !self.game_running, advanced);
                        let mut shapes_changed = false;
                        if !self.game_running {
                            if let Some(model) = model {
                                shapes_changed =
                                    character_shapes::inspector(ui, model, &mut avatar.shape_weights);
                            }
                            ui.collapsing("Оживить персонажа", |ui| {
                                let prior = self.preview_motion;
                                egui::ComboBox::from_id_salt("preview_motion")
                                    .selected_text(self.preview_motion.label())
                                    .show_ui(ui, |ui| {
                                        for motion in Motion::ALL {
                                            ui.selectable_value(
                                                &mut self.preview_motion,
                                                motion,
                                                motion.label(),
                                            );
                                        }
                                    });
                                if prior != self.preview_motion {
                                    self.preview_external_clip = None;
                                    self.preview_elapsed = 0.0;
                                }
                                let label = if self.preview_avatar {
                                    "Остановить предпросмотр"
                                } else {
                                    "Проиграть на месте"
                                };
                                if ui.button(label).clicked() {
                                    self.preview_avatar = !self.preview_avatar;
                                    self.preview_external_clip = None;
                                    self.preview_elapsed = 0.0;
                                }
                            });
                            if let Some(model) = model {
                                avatar::pose_editor(
                                    ui,
                                    avatar,
                                    model,
                                    &mut self.selected_bone,
                                    &mut self.time,
                                );
                            }
                        }
                        if avatar.source == assets::Source::CityHuman {
                            ui.hyperlink_to(
                                "Old Man · Bake3D",
                                "https://bake3d.ai/free-3d-models#old-man",
                            )
                            .on_hover_text("CC BY 4.0 · к исходным клипам добавлен прыжок");
                            ui.hyperlink_to(
                                "Лицензия CC BY 4.0",
                                "https://creativecommons.org/licenses/by/4.0/",
                            );
                        } else if avatar.source == assets::Source::CityWoman {
                            ui.hyperlink_to("Майя · Innerscene · CC0", "https://www.innerscene.com/tools/library/3d-parts/locomotion-collection-maya-3d-person-animated-1499b2fd");
                        } else if avatar.source.modern_human() {
                            ui.hyperlink_to("Персонаж · Microsoft Rocketbox · MIT", "https://github.com/microsoft/Microsoft-Rocketbox");
                        }
                        if advanced {
                            ui.separator();
                            if inspector::effects(ui, &mut self.scene.effect) {
                                self.effect_age = Some(0.0);
                                self.effect_origin =
                                    Vec3::new(self.game.x, self.game.height, self.game.z);
                            }
                            ui.checkbox(&mut self.low_fx, "Меньше частиц");
                        }
                        if shapes_changed {
                            self.changed();
                        }
                        return;
                    }
                    if inspector::part(
                        ui,
                        &mut self.scene,
                        self.selected,
                        self.time,
                        !self.game_running,
                    ) {
                        self.changed();
                    }
                    ui.separator();
                    if inspector::effects(ui, &mut self.scene.effect) {
                        self.effect_age = Some(0.0);
                        self.effect_origin = Vec3::new(self.game.x, self.game.height, self.game.z);
                    }
                    ui.checkbox(&mut self.low_fx, "Меньше частиц");
                });
            });
    }

    fn model_list(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(!self.game_running, |ui| {
            self.world_palette(ui);
            self.storybook_tools(ui);
            self.object_library_controls(ui);
            let can_add = self.scene.world.models.len() < assets::MAX_PLACEMENTS;
            if ui
                .add_enabled(can_add, egui::Button::new("Импорт GLB…"))
                .clicked()
                && let Some(path) = self.glb_path()
            {
                self.import_model(&path);
            }
            ui.add_enabled_ui(can_add, |ui| {
                ui.menu_button("Демо-модели", |ui| {
                    for (name, source, size) in [
                        ("Деревянный ящик", assets::Source::Crate, 1.7),
                        ("Скала", assets::Source::Rock, 3.6),
                    ] {
                        if ui.button(name).clicked() {
                            if let Err(error) = self.assets.ensure(&source, None) {
                                self.record_import_problem(name, error);
                            } else {
                                self.place_model(source, name.into(), size);
                            }
                            ui.close();
                        }
                    }
                });
            });
            egui::ScrollArea::vertical()
                .id_salt("imported_models")
                .max_height(165.0)
                .show(ui, |ui| {
                    for item in &self.scene.world.models {
                        ui.push_id(item.id, |ui| {
                            if ui
                                .selectable_label(
                                    self.world_selection == Some(item.id)
                                        || self.gizmo.selection().contains(&item.id),
                                    &item.label,
                                )
                                .on_hover_text(
                                    "Ctrl+клик добавляет или убирает модель из выделения",
                                )
                                .clicked()
                            {
                                let extend = ui.input(gizmo::extend_selection);
                                self.gizmo
                                    .select(Some(item.id), extend, &mut self.world_selection);
                            }
                        });
                    }
                });
            let model_selected = self
                .scene
                .world
                .models
                .iter()
                .any(|item| self.world_selection == Some(item.id));
            if model_selected && ui.button("Удалить модель").clicked() {
                self.scene
                    .world
                    .models
                    .retain(|item| self.world_selection != Some(item.id));
                self.assets
                    .retain(asset_sources(&self.scene).map(|(source, _)| source));
                self.world_selection = None;
                self.changed();
            }
        });
    }

    fn avatar_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(!self.game_running, |ui| {
            self.chibi_button(ui);
            self.creature_button(ui, creature_workshop::Target::Player);
            let native = self
                .scene
                .avatar
                .as_ref()
                .filter(|actor| actor.source.editable_human());
            let caption = if native.is_some() {
                "Лицо и одежда…"
            } else {
                "Создать персонажа…"
            };
            if ui.button(caption).clicked() {
                self.human_target = None;
                self.human_draft = native.map_or_else(
                    human_editor::Draft::default,
                    human_editor::Draft::from_avatar,
                );
                self.human_editor_open = true;
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button("3D-персонаж…").clicked()
                    && let Some(path) = self.proof.as_ref().map_or_else(
                        || rfd::FileDialog::new().add_filter("Персонаж GLB или VRM", &["glb", "vrm"]).pick_file(),
                        |proof| proof.import_glb.clone(),
                    )
                {
                    match self.import_asset(&path) {
                        Ok(source) => {
                            let name = path
                                .file_stem()
                                .map(|part| {
                                    part.to_string_lossy()
                                        .trim()
                                        .chars()
                                        .take(100)
                                        .collect::<String>()
                                })
                                .filter(|part| !part.is_empty())
                                .unwrap_or_else(|| "3D-персонаж".into());
                            self.assign_avatar(source, name);
                            if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("vrm")) {
                                self.status.push_str(" Используются стандартные материалы glTF. MToon и SpringBone пока не воспроизводятся.");
                            }
                        }
                        Err(_) => {}
                    }
                }
                if self.scene.avatar.is_none() && ui.button("Рыцарь").clicked() {
                    match self.assets.ensure(&assets::Source::Knight, None) {
                        Ok(()) => self.assign_avatar(assets::Source::Knight, "Рыцарь".into()),
                        Err(error) => self.record_import_problem("Рыцарь", error),
                    }
                }
            });
            if self.scene.avatar.is_some() {
                ui.horizontal_wrapped(|ui| {
                    self.attachment_button(ui, character_attachments::Target::Player);
                    if ui.button("Снять 3D-модель").clicked() {
                        self.scene.avatar = None;
                        self.edited_human = None;
                        self.assets
                            .retain(asset_sources(&self.scene).map(|(source, _)| source));
                    }
                });
                if self.view == View::Three {
                    ui.weak("Клипы персонажа настраиваются справа.");
                }
            }
        });
        ui.separator();
    }

    fn assign_avatar(&mut self, source: assets::Source, name: String) {
        let Some(loaded) = self.assets.get(&source) else {
            return;
        };
        self.status = format!("Персонаж «{name}» загружен.");
        self.scene.avatar = Some(Avatar::new(source, name, &loaded.model));
        self.edited_human = None;
        self.player_secondary = actor_physics::ActorSecondary::default();
        self.preview_avatar = false;
        self.selected_bone.clear();
        self.assets
            .retain(asset_sources(&self.scene).map(|(source, _)| source));
        self.view = View::Three;
        self.focus_camera();
    }

    fn character_dialog(&mut self, ctx: &egui::Context) {
        if !self.character_setup {
            return;
        }
        let mut create = false;
        egui::Window::new("Собрать персонажа")
            .collapsible(false)
            .resizable(false)
            .open(&mut self.character_setup)
            .show(ctx, |ui| {
                character_builder::controls(ui, &mut self.character_profile);
                ui.weak("Сборка заменит текущую 3D-модель и её ключи. Окружение останется.");
                create = ui.button("Создать из деталей").clicked();
            });
        if create {
            self.character_setup = false;
            self.stop();
            self.scene.avatar = None;
            self.edited_human = None;
            self.scene.parts = character_builder::parts(&self.character_profile);
            self.scene.animation.keys.clear();
            self.scene.sprite_art.drawn_only = false;
            self.selected = self.scene.parts.first().map(|part| part.id);
            self.editor_tab = EditorTab::Character;
            self.view = View::Three;
            self.assets
                .retain(asset_sources(&self.scene).map(|(source, _)| source));
            self.focus_camera();
            self.status =
                "Персонаж собран. Выбери деталь, чтобы изменить её или добавить новую.".into();
            self.changed();
        }
    }

    fn human_dialog(&mut self, ctx: &egui::Context) {
        if !self.human_editor_open {
            return;
        }
        let Some(action) =
            human_editor::edit(ctx, &mut self.human_editor_open, &mut self.human_draft)
        else {
            return;
        };
        ctx.request_repaint();
        if let Some(id) = self.human_target {
            self.edit_npc_human(id, action);
            return;
        }
        match action {
            human_editor::Action::Apply => {
                let source = self.human_draft.source.clone();
                if let Err(error) = self.assets.ensure(&source, None) {
                    self.record_import_problem("Модель человека", error);
                    return;
                }
                let name = if self.human_draft.name.trim().is_empty() {
                    "Мой герой".into()
                } else {
                    self.human_draft.name.trim().to_owned()
                };
                if !self
                    .scene
                    .avatar
                    .as_ref()
                    .is_some_and(|avatar| avatar.source == source)
                {
                    self.assign_avatar(source, name.clone());
                }
                if let Some(avatar) = &mut self.scene.avatar {
                    avatar.name = name;
                    avatar.height = self.human_draft.height.clamp(1.3, 2.25);
                    avatar.human_style = Some(self.human_draft.style);
                }
                self.sync_edited_human();
                self.view = View::Three;
                self.editor_tab = EditorTab::Character;
                self.status =
                    "Внешность применена. Настройки героя сохраняются вместе со сценой.".into();
                self.changed();
            }
            human_editor::Action::Face => {
                let height = self
                    .scene
                    .avatar
                    .as_ref()
                    .map_or(1.75, |actor| actor.height);
                self.view = View::Three;
                self.camera = Camera {
                    target: [0.0, height * 0.91, 0.025],
                    yaw: 0.09,
                    pitch: 0.06,
                    distance: 0.78,
                };
            }
            human_editor::Action::FullBody => {
                self.editor_tab = EditorTab::Character;
                self.focus_camera();
            }
        }
    }

    fn sync_edited_human(&mut self) {
        let Some(avatar) = self.scene.avatar.as_ref() else {
            self.edited_human = None;
            return;
        };
        let style = avatar.human_style.unwrap_or_default();
        if (style == model::HumanStyle::default() || !avatar.source.editable_human())
            && avatar.modular_parts.is_empty()
            && avatar.bone_attachments.is_empty()
            && avatar.look.is_empty()
            && avatar.imported_clips.is_empty()
        {
            self.edited_human = None;
            return;
        }
        if self.edited_human.as_ref().is_some_and(|edited| {
            edited.source == avatar.source
                && edited.style == style
                && edited.modular_parts == avatar.modular_parts
                && edited.bone_attachments == avatar.bone_attachments
                && edited.look == avatar.look
                && edited.imported_clips == avatar.imported_clips
        }) {
            return;
        }
        match self.model_for_avatar(avatar) {
            Ok(model) => {
                self.edited_human = Some(EditedHuman {
                    source: avatar.source.clone(),
                    style,
                    token: assets::render_token(),
                    model,
                    modular_parts: avatar.modular_parts.clone(),
                    bone_attachments: avatar.bone_attachments.clone(),
                    look: avatar.look.clone(),
                    imported_clips: avatar.imported_clips.clone(),
                })
            }
            Err(error) => {
                self.status = error;
                self.edited_human = None;
            }
        }
    }

    fn avatar_asset(&self) -> Option<(&Arc<model::Model>, u64)> {
        let avatar = self.scene.avatar.as_ref()?;
        if let Some(edited) = &self.edited_human
            && avatar.source == edited.source
        {
            return Some((&edited.model, edited.token));
        }
        self.assets
            .get(&avatar.source)
            .map(|loaded| (&loaded.model, loaded.token))
    }

    fn colliders(&self) -> Vec<world::Collider> {
        let mut colliders = self.static_colliders();
        if self.game_running {
            colliders.extend(self.physics_runtime.colliders());
        }
        colliders
    }

    fn static_colliders(&self) -> Vec<world::Collider> {
        let mut colliders = if self.game_running {
            self.scene.world.colliders_with(&self.interactions)
        } else {
            self.scene.world.colliders()
        };
        for item in self.scene.world.models.iter().filter(|item| {
            item.solid && item.collider.is_none() && self.interactions.object_active(item.id)
        }) {
            let Some(loaded) = self.assets.get(&item.source) else {
                continue;
            };
            if item.source == assets::Source::CityStreet {
                for [left, right] in [[-5.0, -3.75], [3.75, 5.0]] {
                    colliders.push(item.transform_bounds(world::Collider {
                        min: [left, -0.15, -18.0],
                        max: [right, 0.070, 18.0],
                    }));
                }
            } else {
                colliders.push(item.bounds(&loaded.model));
            }
        }
        colliders
    }

    fn game_position(&self) -> Vec3 {
        self.map_player.as_ref().map_or_else(
            || Vec3::new(self.game.x, self.game.height, self.game.z),
            |player| Vec3::from_array(player.world_position()),
        )
    }

    fn advance_free_camera(&mut self, ctx: &egui::Context, dt: f32) {
        let delta = ctx.input(|input| {
            let key = |key| f32::from(u8::from(input.key_down(key)));
            let speed = if input.modifiers.shift { 13.0 } else { 4.5 };
            let offset = Vec3::new(
                key(Key::D) - key(Key::A),
                key(Key::PageUp) - key(Key::PageDown),
                key(Key::S) - key(Key::W),
            );
            Quat::from_rotation_y(self.camera.yaw) * offset.clamp_length_max(1.0) * (speed * dt)
        });
        let target = Vec3::from_array(self.camera.target) + delta;
        self.camera.target = target.to_array();
    }

    fn actor_position(&self) -> Vec3 {
        self.seating.as_ref().map_or_else(
            || self.game_position(),
            |seat| seat.position(self.scene.avatar.as_ref()),
        )
    }

    fn animation_origin(&self) -> Vec3 {
        if !self.game_running
            && let Some(map) = &self.scene.map_2d
        {
            return Vec3::from_array(map.to_world(map.spawn));
        }
        self.actor_position()
    }

    fn update_quest(&mut self, before: [f32; 3]) {
        let after = self.actor_position().to_array();
        self.story_session.update_world(
            &self.scene.story,
            &self.scene.world,
            &self.interactions,
            before,
            after,
        );
        self.rule_session.move_player(
            &self.scene.rules,
            &self.scene.world,
            &self.scene.story,
            &mut self.interactions,
            before,
            after,
        );
        self.sync_object_activity();
    }

    fn dispatch_rule(&mut self, event: rules::Event) {
        let position = self.game_position().to_array();
        self.rule_session.dispatch_at(
            &self.scene.rules,
            &self.scene.world,
            &self.scene.story,
            &mut self.interactions,
            event,
            position,
        );
        self.sync_object_activity();
    }

    fn advance_rule_clocks(&mut self, dt: f32, mark: bool, retry: bool) {
        let position = self.game_position().to_array();
        if self.rule_session.advance_timers(
            &self.scene.rules,
            &self.scene.world,
            &self.scene.story,
            &mut self.interactions,
            dt,
            position,
        ) > 0
        {
            self.sync_object_activity();
        }
        if !self.story_session.is_open() && !self.licenses_open {
            for (pressed, action) in [
                (mark, rules::GameAction::Mark),
                (retry, rules::GameAction::Retry),
            ] {
                if pressed {
                    self.dispatch_rule(rules::Event::GameAction { action });
                }
            }
        }
    }

    fn advance_map(&mut self, dt: f32, intent: playback::Intent) {
        let before = self.game_position().to_array();
        self.advance_interactions(dt, intent.interact);
        if self.reset_attempt_if_requested() {
            return;
        }
        let input = if self.story_session.is_open() || self.licenses_open {
            map2d::Input {
                movement: [0.0; 2],
                sprint: false,
                jump: false,
            }
        } else {
            map2d::Input {
                movement: intent.movement,
                sprint: intent.sprint,
                jump: intent.jump,
            }
        };
        if let Some(player) = &mut self.map_player
            && player.step(&self.scene, &self.rule_session, dt, input)
        {
            self.interaction_message = Some(("Возвращение к контрольной точке".into(), 2.5));
        }
        let delta = self.game_position() - Vec3::from_array(before);
        let horizontal = glam::Vec2::new(delta.x, delta.z);
        if horizontal.length_squared() > 0.000_001 {
            self.game.yaw = (-delta.x).atan2(delta.z).to_degrees();
        }
        let speed = horizontal.length() / dt.max(0.001);
        let motion = if delta.y.abs() > 0.04 {
            Motion::Jump
        } else if speed > 4.6 {
            Motion::Run
        } else if speed > 0.12 {
            Motion::Walk
        } else {
            Motion::Idle
        };
        self.step_avatar_motion(motion, speed, dt);
        self.advance_motion_marker(dt, delta.length_squared() > 0.000_001, input.sprint);
        self.update_quest(before);
        self.reset_attempt_if_requested();
        self.interaction_message = self
            .interaction_message
            .take()
            .and_then(|(text, time)| (time > dt).then_some((text, time - dt)));
    }

    fn sync_object_activity(&mut self) {
        let world = &self.scene.world;
        self.interactions.set_disabled_objects(
            world
                .objects
                .iter()
                .map(|item| item.id)
                .chain(world.models.iter().map(|item| item.id))
                .filter(|&id| !self.rule_session.object_enabled(id)),
        );
    }

    fn environment_parts(&self) -> Vec<Part> {
        let air = |position| self.environment_wind(position);
        let wind = self
            .scene
            .environment
            .as_ref()
            .map(|_| &air as &dyn Fn(Vec3) -> Vec3);
        let mut parts = self
            .scene
            .world
            .render_active_parts(self.game_running.then_some(&self.interactions), wind);
        parts.extend(self.physics_parts());
        if let Some(rotor) = &self.scene.wind_rotor {
            parts.extend(rotor.parts(self.rotor_angle));
        }
        parts
    }

    fn reset_attempt_if_requested(&mut self) -> bool {
        if !self.rule_session.take_reset_requested() {
            return false;
        }
        self.environment_start(false);
        self.effects_runtime = studio_effects::Runtime::default();
        self.audio_runtime.reset_playback();
        self.rotor_angle = 0.0;
        self.game.reset();
        self.start_body_physics(false);
        if let (Some(player), Some(map)) = (&mut self.map_player, &self.scene.map_2d) {
            player.reset(map);
        }
        self.story_session = story::Session::default();
        self.interactions = world::InteractionState::default();
        self.npc_simulation.reset(&self.scene.npcs);
        self.sync_object_activity();
        self.seating = None;
        self.door_hand = None;
        self.door_contact = None;
        self.seat_sprite = None;
        self.walk_target = None;
        self.camera_follow = Vec3::ZERO;
        self.camera_game_distance = None;
        self.foot_locks.get_mut().clear();
        self.interaction_message = Some(("Попытка сброшена".into(), 3.0));
        true
    }

    fn import_map_png(&mut self) {
        let path = if let Some(proof) = &self.proof {
            proof.map_png.clone()
        } else {
            rfd::FileDialog::new()
                .add_filter("Изображение PNG", &["png"])
                .pick_file()
        };
        if let Some(path) = path {
            self.status = match self.map_editor.import_png(&mut self.scene, &path) {
                Ok(()) => {
                    self.changed();
                    "Атлас PNG добавлен в карту.".into()
                }
                Err(error) => error,
            };
        }
    }

    fn map_viewport(&mut self, ui: &mut egui::Ui) {
        if self.tactical_viewport(ui) {
            return;
        }
        if self.map_sprite_editor && !self.game_running {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Вернуться к карте").clicked() {
                    self.map_sprite_editor = false;
                    self.editor_tab = EditorTab::Map;
                }
                ui.selectable_value(&mut self.brush, Brush::None, "Вид");
                ui.selectable_value(&mut self.brush, Brush::Paint, "Кисть");
                ui.selectable_value(&mut self.brush, Brush::Erase, "Ластик");
                ui.color_edit_button_srgba(&mut self.ink);
                egui::ComboBox::from_id_salt("map_sprite_direction")
                    .selected_text(sprite::DIRECTIONS[self.direction as usize])
                    .show_ui(ui, |ui| {
                        for (index, label) in sprite::DIRECTIONS.iter().enumerate() {
                            ui.selectable_value(&mut self.direction, index as u8, *label);
                        }
                    });
                self.sprite_controls(ui);
            });
            let (rect, response) = ui.allocate_exact_size(
                ui.available_size().max(Vec2::splat(4.0)),
                Sense::click_and_drag(),
            );
            self.viewport_2d(ui, rect, &response, &[], &[], 0.0);
            return;
        }
        ui.horizontal_wrapped(|ui| {
            let side = self
                .scene
                .map_2d
                .as_ref()
                .is_some_and(|map| map.mode == map2d::Mode::Side);
            ui.strong(if side {
                "2D сбоку"
            } else {
                "2D сверху"
            });
            if self.game_running {
                ui.label(if side {
                    "A / D — ходьба · Пробел — прыжок"
                } else {
                    "WASD / стрелки — ходьба"
                });
                let controls = &self.scene.play_settings.controls;
                ui.label(format!(
                    "{} — отметить · {} — повторить",
                    play_settings::label(controls, play_settings::Action::Mark),
                    play_settings::label(controls, play_settings::Action::Retry)
                ));
                if self.standalone && ui.button("Начать заново").clicked() {
                    self.start_play(ui.ctx(), false);
                }
            } else {
                ui.weak("Карта и зоны в правой панели");
            }
        });
        if self.game_running {
            ui.horizontal_wrapped(|ui| {
                for counter in &self.scene.rules.counters {
                    if let Some(value) = self.rule_session.counter(&self.scene.rules, counter.id) {
                        ui.colored_label(ACCENT, format!("{} · {value}", counter.name));
                    }
                }
                for timer in &self.scene.rules.timers {
                    if let Some(seconds) = self.rule_session.timer_remaining(timer.id) {
                        ui.colored_label(ACCENT, format!("{} · {:.1} с", timer.name, seconds));
                    }
                }
                if let Some(objective) = self.rule_quest_label() {
                    ui.colored_label(ACCENT, objective);
                }
                if let Some((text, _)) = &self.interaction_message {
                    ui.label(RichText::new(text).strong());
                }
            });
            self.paint_rule_trace(ui);
        }
        let natural = self.environment_frame(self.game_position());
        let particles = self.effect_particles();
        if self.map_player.is_some() {
            let use_texture = self.scene.avatar.is_some() || !self.scene.sprite_pixels.is_empty();
            let texture = if use_texture {
                self.prepare_sprite(ui.ctx(), &[], sprite::scale_for(&[]), 0.0)
            } else {
                None
            };
            if let Some(player) = &mut self.map_player {
                player.show(
                    ui,
                    &self.scene,
                    &self.rule_session,
                    &self.interactions,
                    texture,
                    natural.as_ref(),
                    &particles,
                );
            }
        } else if self
            .map_editor
            .view(ui, &mut self.scene, natural.as_ref(), &particles)
        {
            self.changed();
        }
    }

    fn rule_quest_label(&self) -> Option<String> {
        self.rule_session.quest_stages().find_map(|(id, stage)| {
            let quest = self.scene.story.quest(id)?;
            let story::Objective::Rules { completion_stage } = quest.objective else {
                return None;
            };
            if stage >= completion_stage {
                Some(format!(
                    "Выполнено · {} · {}",
                    quest.title, quest.completion
                ))
            } else if stage == 0 {
                None
            } else {
                Some(format!(
                    "{} · этап {stage} из {completion_stage} · {}",
                    quest.title, quest.description
                ))
            }
        })
    }

    fn paint_rule_trace(&self, ui: &mut egui::Ui) {
        if self.standalone {
            return;
        }
        let entries = if self.scene.rules.counters.is_empty() {
            1
        } else {
            3
        };
        for trace in self.rule_session.trace().rev().take(entries) {
            if let Some((message, color)) = self.rule_trace_label(trace) {
                ui.colored_label(color, message);
            }
        }
    }

    fn rule_trace_label(&self, trace: &rules::Trace) -> Option<(String, Color32)> {
        let rule = self
            .scene
            .rules
            .rules
            .iter()
            .find(|rule| rule.id == trace.rule_id)?;
        let name = &rule.name;
        let alert = Color32::from_rgb(255, 207, 109);
        Some(match trace.outcome {
            rules::TraceOutcome::Fired { actions } => {
                (format!("Сработало «{name}» · действий {actions}"), ACCENT)
            }
            rules::TraceOutcome::Blocked(rules::Condition::HasItem { item_id }) => (
                format!(
                    "«{name}» · условие не выполнено · нужен предмет «{}»",
                    self.key_name(item_id)
                ),
                alert,
            ),
            rules::TraceOutcome::Blocked(rules::Condition::Flag { flag_id, value }) => {
                let flag_name = self
                    .scene
                    .rules
                    .flag(flag_id)
                    .map_or("?", |flag| &flag.name);
                let value = if value {
                    "включён"
                } else {
                    "выключен"
                };
                (
                    format!("«{name}» · условие не выполнено · «{flag_name}» должен быть {value}"),
                    alert,
                )
            }
            rules::TraceOutcome::Blocked(rules::Condition::Counter {
                counter_id,
                comparison,
                value,
            }) => {
                use rules::Comparison::*;
                let symbol = match comparison {
                    Equal => "=",
                    NotEqual => "≠",
                    Less => "<",
                    AtMost => "≤",
                    Greater => ">",
                    AtLeast => "≥",
                };
                let counter = self
                    .scene
                    .rules
                    .counter(counter_id)
                    .map_or("?", |counter| &counter.name);
                let current = self
                    .rule_session
                    .counter(&self.scene.rules, counter_id)
                    .unwrap_or_default();
                (
                    format!(
                        "«{name}» · условие не выполнено · {counter} = {current}, нужно {symbol} {value}"
                    ),
                    alert,
                )
            }
            rules::TraceOutcome::Blocked(rules::Condition::InZone { zone_id }) => {
                let zone = self
                    .scene
                    .rules
                    .zone(zone_id)
                    .map_or("?", |zone| &zone.name);
                (
                    format!("«{name}» · условие не выполнено · нужна зона «{zone}»"),
                    alert,
                )
            }
            rules::TraceOutcome::AlreadyFired => (format!("«{name}» · повтор пропущен"), MUTED),
            rules::TraceOutcome::MissingActionTarget(_) => {
                (format!("«{name}» · объект действия отсутствует"), alert)
            }
        })
    }

    fn key_name(&self, id: u32) -> &str {
        self.scene.world.key(id).map_or("Ключ", |key| &key.name)
    }

    fn priority_interaction(action: Option<world::Interaction>) -> bool {
        matches!(
            action,
            Some(
                world::Interaction::Door { opening: true, .. }
                    | world::Interaction::LockedDoor { .. }
                    | world::Interaction::RuleLockedDoor { .. }
                    | world::Interaction::Key { .. }
            )
        )
    }

    fn advance_interactions(&mut self, dt: f32, use_key: bool) {
        let position = self.actor_position().to_array();
        self.scene
            .world
            .advance_interactions(&mut self.interactions, dt, position);
        if !use_key || self.door_hand.is_some() {
            return;
        }
        if self.story_session.is_open() {
            self.story_session.close();
            return;
        }
        if !self.game.grounded {
            return;
        }
        if let Some(seating) = &mut self.seating {
            if seating.amount >= 0.998 {
                seating.leaving = true;
            }
            return;
        }
        let action = self
            .scene
            .world
            .interaction_near(&self.interactions, position);
        if !Self::priority_interaction(action)
            && let Some(id) = self.nearby_talk_npc()
        {
            self.walk_target = None;
            self.story_session.open(&self.scene.story, id);
            return;
        }
        if let Some(door_id) = action.and_then(world::Interaction::door_id) {
            self.dispatch_rule(rules::Event::Door { door_id });
        }
        if self.start_door_hand(position) {
            return;
        }
        match self.scene.world.interact(&mut self.interactions, position) {
            Some(world::Interaction::LockedDoor { key, .. }) => {
                self.interaction_message = Some((
                    format!("Дверь заперта. Нужен ключ «{}».", self.key_name(key)),
                    4.0,
                ));
            }
            Some(world::Interaction::Key { id }) => {
                self.dispatch_rule(rules::Event::Pickup { item_id: id });
                self.interaction_message =
                    Some((format!("Подобран ключ «{}».", self.key_name(id)), 3.0));
            }
            Some(world::Interaction::RuleLockedDoor { .. }) => {
                self.interaction_message = Some(("Дверь заперта. Нужен допуск.".into(), 4.0));
            }
            Some(world::Interaction::Seat(seat)) => {
                self.walk_target = None;
                self.seating = Some(Seating {
                    seat,
                    arrival: Vec3::from_array(position),
                    original_heading: self.avatar_heading,
                    amount: 0.0,
                    leaving: false,
                });
                self.seat_sprite = None;
                self.avatar_motion = Motion::Idle;
                self.avatar_previous = None;
                self.actor_transition = actor_transitions::Playback::default();
                self.avatar_elapsed = 0.0;
                self.avatar_clip_time = 0.0;
                self.avatar_clip_rate = 1.0;
                self.avatar_landing = None;
            }
            Some(world::Interaction::Door { .. }) => self.interaction_message = None,
            None => {}
        }
    }

    fn advance_seating(&mut self, dt: f32) -> bool {
        let Some(seating) = &mut self.seating else {
            return false;
        };
        let native_clip = self
            .scene
            .avatar
            .as_ref()
            .is_some_and(|avatar| avatar.sit.is_some());
        let duration = if native_clip { 1.36 } else { 0.66 };
        let step = dt / duration;
        seating.amount = if seating.leaving {
            seating.amount - step
        } else {
            seating.amount + step
        }
        .clamp(0.0, 1.0);
        let position = seating.position(self.scene.avatar.as_ref());
        let follow = Vec3::new(position.x, position.y * 0.65, position.z);
        self.camera_follow = self.camera_follow.lerp(follow, 1.0 - (-dt * 12.0).exp());
        if seating.leaving && seating.amount == 0.0 {
            let [x, y, z] = seating.seat.stand;
            let heading = seating.seat.yaw;
            self.game.reset();
            self.game.x = x;
            self.game.height = y;
            self.game.z = z;
            self.game.yaw = heading;
            self.avatar_heading = heading;
            self.seating = None;
            self.seat_sprite = None;
        }
        true
    }

    fn import_model(&mut self, path: &Path) {
        match self.import_asset(path) {
            Ok(source) => {
                let label = path.file_stem().map_or_else(
                    || "Модель GLB".to_string(),
                    |name| {
                        name.to_string_lossy()
                            .trim()
                            .chars()
                            .take(100)
                            .collect::<String>()
                    },
                );
                let label = if label.is_empty() {
                    "Модель GLB".into()
                } else {
                    label
                };
                self.place_model(source, label, 1.8);
            }
            Err(error) => self.status = format!("Не удалось импортировать GLB. {error}"),
        }
    }

    fn glb_path(&self) -> Option<PathBuf> {
        if let Some(proof) = &self.proof {
            return proof.import_glb.clone();
        }
        rfd::FileDialog::new()
            .add_filter("Модель GLB", &["glb"])
            .pick_file()
    }

    fn replace_world_model(&mut self, id: u64) {
        let Some(path) = self.glb_path() else {
            return;
        };
        let result = (|| {
            let index = self
                .scene
                .world
                .models
                .iter()
                .position(|item| item.id == id)
                .ok_or("Выбери модель в окружении.")?;
            let before = self
                .assets
                .get(&self.scene.world.models[index].source)
                .map(|asset| Arc::clone(&asset.model));
            let source = self.import_asset(&path)?;
            let after = self.assets.get(&source).ok_or("Модель не найдена в библиотеке.")?;
            if let Some(item) = self.scene.world.models.get_mut(index) {
                item.replace_visual(source, before.as_deref(), &after.model);
            }
            Ok::<_, String>(())
        })();
        self.status = match result {
            Ok(()) => {
                self.changed();
                "Модель заменена. Коллайдер и посадка привязаны к объекту.".into()
            }
            Err(error) => format!("Не удалось заменить GLB. {error}"),
        };
    }

    fn place_model(&mut self, source: assets::Source, name: String, size: f32) {
        let Some(loaded) = self.assets.get(&source) else {
            return;
        };
        let index = self.scene.world.models.len();
        let position = [
            1.7 + (index % 5) as f32 * 1.45,
            0.0,
            -0.4 - (index / 5) as f32 * 2.0,
        ];
        let item = assets::Placement::at_ground(
            &self.scene.world.models,
            &loaded.model,
            source,
            name,
            position,
            size,
            0.0,
        );
        self.status = format!("Добавлена модель «{}».", item.label);
        self.world_selection = Some(item.id);
        self.scene.world.models.push(item);
        self.focus_camera();
    }

    fn materials_demo(&mut self) {
        let mut scene = Scene::default();
        scene.world = world::World::default();
        self.assets = assets::Library::default();
        let mut load_error = None;
        for (source, name, position, size, yaw) in [
            (
                assets::Source::Crate,
                "Деревянный ящик",
                [1.45, 0.0, 0.28],
                1.85,
                30.0,
            ),
            (
                assets::Source::Rock,
                "Скала",
                [-0.55, 0.0, -2.85],
                4.0,
                -16.0,
            ),
            (
                assets::Source::Crate,
                "Малый ящик",
                [-1.50, 0.0, -0.68],
                1.4,
                10.0,
            ),
        ] {
            if let Err(error) = self.assets.ensure(&source, None) {
                load_error = Some(error);
                continue;
            }
            if let Some(loaded) = self.assets.get(&source) {
                let item = assets::Placement::at_ground(
                    &scene.world.models,
                    &loaded.model,
                    source,
                    name.into(),
                    position,
                    size,
                    yaw,
                );
                scene.world.models.push(item);
            }
        }
        self.set_scene(
            scene,
            None,
            "Демо материалов: ящики и скала с текстурами. «К объекту» приближает выбранную модель.",
        );
        if let Some(error) = load_error {
            self.status = format!("Часть демонстрации не загрузилась. {error}");
        }
        self.editor_tab = EditorTab::World;
        self.world_selection = self.scene.world.models.first().map(|item| item.id);
        self.view = View::Three;
        self.camera = Camera {
            target: [0.2, 1.05, -0.60],
            distance: 6.0,
            ..Camera::default()
        };
    }

    fn courtyard_demo(&mut self) {
        use assets::Source;

        let mut scene = Scene::default();
        scene.world = world::World::default();
        self.assets = assets::Library::default();
        let mut load_error = None;
        if let Err(error) = self.assets.ensure(&Source::Knight, None) {
            load_error = Some(error);
        } else if let Some(knight) = self.assets.get(&Source::Knight) {
            scene.avatar = Some(Avatar::new(Source::Knight, "Рыцарь".into(), &knight.model));
        }
        for (source, name, position, size, yaw, solid) in [
            (
                Source::CourtyardGround,
                "Каменная мостовая",
                [0.0, 0.012, 0.0],
                65.0,
                0.0,
                false,
            ),
            (
                Source::StoneBuilding,
                "Северный дом",
                [0.0, 0.0, -8.5],
                7.2,
                0.0,
                true,
            ),
            (
                Source::StoneBuilding,
                "Западный дом",
                [-7.8, 0.0, -4.6],
                7.6,
                55.0,
                true,
            ),
            (
                Source::StoneBuilding,
                "Восточный дом",
                [7.6, 0.0, -5.2],
                6.3,
                -48.0,
                true,
            ),
            (
                Source::Crate,
                "Ящик у двери",
                [-2.0, 0.0, -5.4],
                1.55,
                37.0,
                true,
            ),
            (
                Source::Crate,
                "Ящик во дворе",
                [2.75, 0.0, -2.75],
                1.8,
                20.0,
                true,
            ),
            (
                Source::Rock,
                "Камни у стены",
                [6.15, 0.0, -1.2],
                2.4,
                18.0,
                true,
            ),
        ] {
            if let Err(error) = self.assets.ensure(&source, None) {
                load_error = Some(error);
                continue;
            }
            let Some(loaded) = self.assets.get(&source) else {
                continue;
            };
            let mut placement = assets::Placement::at_ground(
                &scene.world.models,
                &loaded.model,
                source,
                name.into(),
                position,
                size,
                yaw,
            );
            placement.solid = solid;
            scene.world.models.push(placement);
        }
        scene.effect.color = [0.82, 0.66, 0.41];
        scene.effect.count = 12;
        scene.effect.intensity = 0.50;
        self.set_scene(
            scene,
            None,
            "Каменный двор. Нажми «Играть», чтобы управлять рыцарем.",
        );
        if let Some(error) = load_error {
            self.status = format!("Часть сцены не загрузилась. {error}");
        }
        self.view = View::Three;
        self.camera = Camera {
            yaw: 0.39,
            pitch: 0.25,
            target: [0.0, 1.18, -0.65],
            distance: 6.1,
        };
    }

    fn city_demo(&mut self) {
        use assets::Source;

        let mut scene = Scene::empty();
        scene.parts = character_builder::parts(&character_builder::Profile::default());
        self.assets = assets::Library::default();
        let mut error = None;
        if let Err(reason) = self.assets.ensure(&Source::Emma, None) {
            error = Some(reason);
        } else if let Some(human) = self.assets.get(&Source::Emma) {
            scene.avatar = Some(Avatar::new(Source::Emma, "Эмма".into(), &human.model));
        }
        if let Err(reason) = self.assets.ensure(&Source::CityStreet, None) {
            error = Some(reason);
        } else if let Some(street) = self.assets.get(&Source::CityStreet) {
            for (label, z) in [("Дорога и тротуары", 0.0), ("Продолжение улицы", -36.0)]
            {
                let mut placement = assets::Placement::at_ground(
                    &scene.world.models,
                    &street.model,
                    Source::CityStreet,
                    label.into(),
                    [0.0, 0.008, z],
                    36.0,
                    0.0,
                );
                placement.solid = true;
                scene.world.models.push(placement);
            }
        }
        for (name, position, size, turn, color) in [
            (
                "Дом с мебелью",
                [-7.4, 0.0, -2.8],
                [7.3, 3.45, 5.9],
                1,
                [0.57, 0.43, 0.34],
            ),
            (
                "Дом на углу",
                [7.5, 0.0, -5.5],
                [7.5, 4.25, 6.0],
                3,
                [0.54, 0.51, 0.44],
            ),
            (
                "Северный дом",
                [-7.6, 0.0, -11.9],
                [7.2, 6.25, 5.85],
                1,
                [0.44, 0.49, 0.48],
            ),
            (
                "Серый дом",
                [7.7, 0.0, 3.1],
                [7.3, 3.7, 6.0],
                3,
                [0.48, 0.52, 0.55],
            ),
            (
                "Дальний дом",
                [7.8, 0.0, -14.1],
                [6.7, 7.15, 6.0],
                3,
                [0.61, 0.51, 0.43],
            ),
            (
                "Дом с каменным карнизом",
                [-7.42, 0.0, -20.0],
                [7.1, 5.9, 5.7],
                1,
                [0.61, 0.55, 0.48],
            ),
            (
                "Бывшая мастерская",
                [7.82, 0.0, -22.45],
                [8.7, 4.2, 6.1],
                3,
                [0.45, 0.42, 0.36],
            ),
        ] {
            if scene.world.add_building_at(position).is_some()
                && let Some(building) = scene.world.buildings.last_mut()
            {
                building.name = name.into();
                building.size = size;
                building.quarter_turns = turn;
                building.color = color;
                if name == "Дом с мебелью" {
                    building.furnish_demo();
                    building.door = Some(world::DoorSettings::default());
                    if let Some(opening) = building
                        .interior
                        .partitions
                        .first_mut()
                        .and_then(|wall| wall.opening.as_mut())
                    {
                        opening.door = Some(world::DoorSettings::default());
                    }
                }
            }
        }
        if let Err(reason) = self.assets.ensure(&Source::Ryan, None) {
            error = Some(reason);
        } else if let (Some(model), Some(house)) = (
            self.assets.get(&Source::Ryan),
            scene.world.buildings.first(),
        ) {
            let guide = Avatar::new(Source::Ryan, "Райан".into(), &model.model);
            let mut route = npcs::doorway_route(house);
            route[0].wait = 2.6;
            let [x, _, z] = route[route.len() / 2].position;
            let id = npcs::FIRST_NPC_ID;
            scene.story = story::Story::demo(id, [x, z]);
            scene.story.conversations[0].name.clone_from(&guide.name);
            scene.npcs.push(npcs::Npc::new(id, guide, route));
        }
        scene.effect.count = 10;
        scene.effect.intensity = 0.28;
        scene.effect.color = [0.72, 0.64, 0.52];
        self.set_scene(
            scene,
            None,
            "Городская улица. В игре подойди к Райану и нажми E.",
        );
        if let Some(reason) = error {
            self.status = format!("Часть сцены не загрузилась. {reason}");
        }
        self.view = View::Three;
        self.overhead = true;
        self.camera = Camera {
            yaw: 0.58,
            pitch: 0.17,
            target: [-0.52, 1.02, -1.38],
            distance: 5.35,
        };
    }

    fn quarter_demo(&mut self) {
        let mut library = assets::Library::default();
        match quarter::create(&mut library) {
            Ok(scene) => {
                self.assets = library;
                self.set_scene(
                    scene,
                    None,
                    "Липовый проезд. Поговори с Мариной у перехода.",
                );
                self.view = View::Three;
                self.overhead = true;
                self.camera = Camera {
                    yaw: 0.54,
                    pitch: 0.21,
                    distance: 6.8,
                    target: [0.0, 1.2, -2.4],
                };
            }
            Err(error) => self.status = format!("Не удалось загрузить квартал. {error}"),
        }
    }

    fn animation_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("animation")
            .min_height(124.0)
            .show(ctx, |ui| {
                let old_duration = self.scene.animation.duration;
                if inspector::timeline(
                    ui,
                    &mut self.scene,
                    self.selected,
                    &mut self.time,
                    &mut self.preview,
                    self.game_running,
                ) {
                    let elapsed = self.time - self.scene.animation.event_time;
                    self.effect_age = (0.0..self.scene.effect.duration)
                        .contains(&elapsed)
                        .then_some(elapsed);
                    self.changed();
                }
                let new_duration = self.scene.animation.duration;
                if old_duration != new_duration
                    && let Some(avatar) = &mut self.scene.avatar
                {
                    avatar.retime_bones(old_duration, new_duration);
                }
                if old_duration != new_duration {
                    for npc in &mut self.scene.npcs {
                        npc.avatar.retime_bones(old_duration, new_duration);
                    }
                }
                ui.separator();
                ui.label(RichText::new(&self.status).color(MUTED));
            });
    }

    fn viewport(&mut self, ui: &mut egui::Ui) {
        if self.scene.map_2d.is_some() {
            self.map_viewport(ui);
            return;
        }
        ui.horizontal_wrapped(|ui| {
            let previous_view = (self.view, self.overhead);
            ui.selectable_value(&mut self.view, View::Three, "3D");
            ui.selectable_value(&mut self.view, View::Two, "2D");
            if self.view == View::Two {
                ui.selectable_value(&mut self.overhead, true, "План");
                if !self.standalone {
                    ui.selectable_value(&mut self.overhead, false, "Сбоку / спрайт");
                }
            }
            if (self.view, self.overhead) != previous_view {
                self.walk_target = None;
                self.free_camera = false;
            }
            ui.separator();
            if self.game_running {
                let walking = if self.free_camera {
                    "WASD — камера · PgUp / PgDn — высота"
                } else if self.view == View::Three || self.overhead {
                    "WASD / ЛКМ — ходьба"
                } else {
                    "A / D — ходьба"
                };
                ui.label(
                    RichText::new(format!(
                        "{walking} · Shift — бег · Пробел — прыжок · Esc — выход"
                    ))
                    .color(ACCENT),
                );
                if self.standalone {
                    if ui.button("Начать заново").clicked() {
                        self.start_play(ui.ctx(), false);
                    }
                    if ui.button("Лицензии").clicked() {
                        self.licenses_open = !self.licenses_open;
                        self.walk_target = None;
                    }
                } else {
                    ui.checkbox(&mut self.show_panels_in_game, "Панели");
                }
                if self.view == View::Three
                    && ui
                        .checkbox(&mut self.free_camera, "Свободная камера")
                        .changed()
                {
                    self.walk_target = None;
                    self.camera.target = (Vec3::from_array(Camera::default().target)
                        + self.camera_follow)
                        .to_array();
                    self.camera_game_distance = None;
                }
                let action = self
                    .scene
                    .world
                    .interaction_near(&self.interactions, self.actor_position().to_array());
                let controls = &self.scene.play_settings.controls;
                let interact_key = play_settings::label(controls, play_settings::Action::Interact);
                for (action, action_key) in [
                    (rules::GameAction::Mark, play_settings::Action::Mark),
                    (rules::GameAction::Retry, play_settings::Action::Retry),
                ] {
                    if self.scene.rules.rules.iter().any(|rule| {
                        rule.enabled && rule.event == Some(rules::Event::GameAction { action })
                    }) {
                        ui.weak(format!(
                            "{} — {}",
                            play_settings::label(controls, action_key),
                            if action == rules::GameAction::Mark {
                                "игровое действие"
                            } else {
                                "повтор попытки"
                            }
                        ));
                    }
                }
                if self.free_camera {
                    ui.weak("ЛКМ — обзор · ПКМ — сдвиг");
                } else if self.story_session.is_open() {
                    ui.weak(format!("{interact_key} / Esc — закрыть разговор"));
                    if self.view == View::Three {
                        ui.weak("F8 — крупный план");
                    }
                } else if let Some(seat) = &self.seating {
                    ui.label(if seat.leaving {
                        "Встаёт".into()
                    } else {
                        format!("{interact_key} — встать")
                    });
                } else if let Some(npc) = self.nearby_talk_npc()
                    && !Self::priority_interaction(action)
                    && let Some(actor) = self.scene.npcs.iter().find(|actor| actor.id == npc)
                {
                    ui.label(
                        RichText::new(format!(
                            "{interact_key} — поговорить с NPC «{}»",
                            actor.avatar.name
                        ))
                        .color(ACCENT)
                        .strong(),
                    );
                } else if let Some(action) = action {
                    let label = match action {
                        world::Interaction::Door { opening: true, .. } => "открыть дверь".into(),
                        world::Interaction::Door { opening: false, .. } => "закрыть дверь".into(),
                        world::Interaction::LockedDoor { .. }
                        | world::Interaction::RuleLockedDoor { .. } => "проверить замок".into(),
                        world::Interaction::Key { id } => {
                            format!("подобрать «{}»", self.key_name(id))
                        }
                        world::Interaction::Seat(_) => "сесть".into(),
                    };
                    ui.label(
                        RichText::new(format!("{interact_key} — {label}"))
                            .color(ACCENT)
                            .strong(),
                    );
                }
                if let Some(building) = self.scene.world.buildings.iter().find(|building| {
                    building.contains_interior(self.game.x, self.game.height, self.game.z)
                }) {
                    ui.label(RichText::new(format!("Внутри: {}", building.name)).color(ACCENT));
                }
                if let Some(objective) = self
                    .rule_quest_label()
                    .or_else(|| self.story_session.objective(&self.scene.story))
                {
                    ui.label(RichText::new(objective).color(ACCENT).strong());
                }
                let keys = self
                    .interactions
                    .key_ids()
                    .map(|id| self.key_name(id))
                    .collect::<Vec<_>>();
                if !keys.is_empty() {
                    ui.label(RichText::new(format!("Ключи: {}", keys.join(", "))).color(ACCENT));
                }
                if let Some((message, _)) = &self.interaction_message {
                    ui.label(
                        RichText::new(message)
                            .color(Color32::from_rgb(255, 207, 109))
                            .strong(),
                    );
                }
            }
            if self.view == View::Three {
                if ui.button("Сброс камеры").clicked() {
                    self.camera = Camera::default();
                }
                if !self.game_running {
                    let focus = match self.editor_tab {
                        EditorTab::World => "К объекту",
                        EditorTab::Npcs => "К NPC",
                        EditorTab::Character
                        | EditorTab::Rules
                        | EditorTab::Map
                        | EditorTab::Tactics
                        | EditorTab::Environment
                        | EditorTab::Effects
                        | EditorTab::Audio
                        | EditorTab::Lighting => "К детали",
                    };
                    if ui.button(focus).clicked() {
                        self.focus_camera();
                    }
                    if !self.standalone {
                        use studio_core::render_settings::PostProcess;

                        let before = self.scene.post_process;
                        ui.menu_button("Постобработка", |ui| {
                            let post = &mut self.scene.post_process;
                            ui.add(
                                egui::Slider::new(
                                    &mut post.exposure_ev,
                                    PostProcess::EXPOSURE_EV_RANGE,
                                )
                                .text("Экспозиция")
                                .suffix(" EV"),
                            )
                            .on_hover_text("+1 EV удваивает яркость сцены перед тонмаппингом.");
                            ui.checkbox(&mut post.bloom, "Свечение (Bloom)");
                            ui.add_enabled(
                                post.bloom,
                                egui::Slider::new(
                                    &mut post.bloom_threshold,
                                    PostProcess::BLOOM_THRESHOLD_RANGE,
                                )
                                .text("Порог свечения"),
                            )
                            .on_hover_text("Чем выше порог, тем ярче должна быть область для свечения.");
                            if ui.button("Сбросить постобработку").clicked() {
                                *post = PostProcess {
                                    shadow_quality: post.shadow_quality,
                                    ..PostProcess::default()
                                };
                            }
                        });
                        if self.scene.post_process != before {
                            self.changed();
                        }
                    }
                    ui.label(
                        RichText::new("ЛКМ — вращать · ПКМ — сдвигать · колесо — масштаб")
                            .color(MUTED),
                    );
                }
            } else if !self.overhead {
                if !self.game_running {
                    egui::ComboBox::from_id_salt("direction")
                        .selected_text(sprite::DIRECTIONS[self.direction as usize])
                        .show_ui(ui, |ui| {
                            for (index, label) in sprite::DIRECTIONS.iter().enumerate() {
                                ui.selectable_value(&mut self.direction, index as u8, *label);
                            }
                        });
                }
                ui.add_enabled_ui(!self.game_running && !self.preview, |ui| {
                    ui.selectable_value(&mut self.brush, Brush::None, "Вид");
                    ui.selectable_value(&mut self.brush, Brush::Paint, "Кисть");
                    ui.selectable_value(&mut self.brush, Brush::Erase, "Ластик");
                    ui.color_edit_button_srgba(&mut self.ink);
                });
                if !self.game_running {
                    self.sprite_controls(ui);
                }
                if !self.scene.world.models.is_empty() {
                    ui.weak("GLB в 2D показаны по габаритам");
                }
            } else if !self.game_running {
                ui.weak("ПКМ — сдвинуть · колесо — масштаб");
                if self.editor_tab == EditorTab::World {
                    ui.weak("Мебель — клик · Shift+клик — переместить");
                } else if self.editor_tab == EditorTab::Npcs {
                    if ui.button("К NPC").clicked() {
                        self.focus_camera();
                    }
                    if self.npc_place_on_plan && self.npc_panel == npc_ui::Panel::Route {
                        ui.label(RichText::new("ЛКМ — добавить точку маршрута").color(ACCENT));
                    }
                    if self.npc_quest_on_plan.is_some() && self.npc_panel == npc_ui::Panel::Tasks {
                        ui.label(RichText::new("ЛКМ — указать цель задания").color(ACCENT));
                    }
                }
            }
        });
        if self.game_running {
            self.paint_rule_trace(ui);
        }
        if self.game_running && !self.scene.rules.counters.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for counter in &self.scene.rules.counters {
                    if let Some(value) = self.rule_session.counter(&self.scene.rules, counter.id) {
                        ui.colored_label(ACCENT, format!("{} · {value}", counter.name));
                    }
                }
            });
        }
        let edit_world =
            !self.game_running && self.view == View::Three && self.editor_tab == EditorTab::World;
        if edit_world {
            let update = self
                .gizmo
                .toolbar(ui, &mut self.scene.world, &mut self.world_selection);
            if update.changed {
                self.changed();
            }
            if let Some(status) = update.status {
                self.status = status;
            }
        } else {
            self.gizmo.end_drag();
        }
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(Vec2::splat(4.0)),
            Sense::click_and_drag(),
        );
        if !self.game_running
            && let Some(workshop) = &mut self.creature_workshop
            && workshop.paint_preview(ui, rect, &response, self.workspace == editor_workspace::Workspace::Develop)
        {
            return;
        }
        let mut parts = if self.game_running {
            self.scene.sample_game(self.time, &self.game)
        } else {
            self.scene.sample(self.time)
        };
        let glow = self.effect_strength();
        let mut sparks = self.sparks(glow);
        if self.view == View::Three
            && self.game_running
            && let Some([x, z]) = self
                .story_session
                .active_destination_in(&self.scene.story, &self.scene.world)
        {
            let float = (self.live_seconds * 2.5).sin() * 0.11;
            for (height, size) in [(0.81, 0.045), (0.99, 0.082), (1.17, 0.045)] {
                sparks.push(Spark {
                    position: [x, height + float, z],
                    size,
                    color: [1.0, 0.70, 0.27],
                });
            }
        }
        if self.view == View::Three {
            let move_physics = edit_world && self.physics_editor.placing();
            if move_physics
                && response.clicked_by(egui::PointerButton::Primary)
                && let Some(point) = response.interact_pointer_pos()
                && let Some([x, z]) =
                    self.camera
                        .ground_point(rect, point, self.physics_placement_height())
            {
                self.place_physics_at(x, z);
            }
            let handles_pointer = if move_physics {
                true
            } else if edit_world {
                let update = self.gizmo.interact(
                    ui,
                    &response,
                    self.camera,
                    &mut self.scene.world,
                    &self.assets,
                    &mut self.world_selection,
                );
                if update.changed {
                    self.changed();
                }
                update.owns_pointer
            } else {
                self.route_3d_input(ui, rect, &response)
            };
            let camera_settings = self.scene.play_settings.camera;
            let fixed_camera = self.game_running
                && !self.free_camera
                && camera_settings.mode == play_settings::CameraMode::Fixed;
            let sensitivity = if self.game_running {
                camera_settings.sensitivity
            } else {
                1.0
            };
            if !fixed_camera
                && !handles_pointer
                && response.dragged_by(egui::PointerButton::Primary)
            {
                let delta = ui.input(|input| input.pointer.delta()) * sensitivity;
                self.camera.yaw -= delta.x * 0.009;
                self.camera.pitch = (self.camera.pitch + delta.y * 0.008).clamp(-0.14, 1.34);
            }
            if (!self.game_running || self.free_camera)
                && response.dragged_by(egui::PointerButton::Secondary)
            {
                let delta = ui.input(|input| input.pointer.delta())
                    * self.camera.distance
                    * 0.0035
                    * sensitivity;
                let offset =
                    Quat::from_rotation_y(self.camera.yaw) * Vec3::new(-delta.x, delta.y, 0.0);
                self.camera.target = (Vec3::from_array(self.camera.target) + offset).to_array();
            }
            if !fixed_camera && response.hovered() {
                let scroll = ui.input(|input| input.smooth_scroll_delta.y);
                self.camera.distance = (self.camera.distance
                    * (-scroll * 0.003 * sensitivity).exp())
                .clamp(0.66, 38.0);
            }
            let mut camera = self.camera;
            if self.game_running {
                let heading = self
                    .seating
                    .as_ref()
                    .map_or(self.game.yaw, Seating::heading);
                let yaw = Quat::from_rotation_y(heading.to_radians());
                let position = self.actor_position();
                for part in &mut parts {
                    part.position = (yaw * Vec3::from_array(part.position) + position).to_array();
                    let [rx, ry, rz] = part.rotation.map(f32::to_radians);
                    let (rx, ry, rz) =
                        (yaw * Quat::from_euler(EulerRot::XYZ, rx, ry, rz)).to_euler(EulerRot::XYZ);
                    part.rotation = [rx, ry, rz].map(f32::to_degrees);
                }
                if !self.free_camera && !fixed_camera {
                    let portrait = ((3.0 - camera.distance) / 1.55).clamp(0.0, 1.0);
                    camera.target = (Vec3::from_array(Camera::default().target)
                        + self.camera_follow
                        + Vec3::Y * (portrait * 0.44))
                        .to_array();
                    let colliders = self.colliders();
                    let dt = ui.input(|input| input.stable_dt.clamp(0.0, 0.05));
                    camera = room_camera::resolve_boom(
                        camera,
                        clear_camera_distance,
                        &colliders,
                        dt,
                        &mut self.camera_shoulder,
                        &mut self.camera_game_distance,
                    );
                }
                if !self.free_camera
                    && !self.story_session.is_open()
                    && !self.licenses_open
                    && self.seating.is_none()
                    && response.clicked_by(egui::PointerButton::Primary)
                    && let Some(mouse) = response.interact_pointer_pos()
                {
                    self.walk_target = camera.ground_point(rect, mouse, self.game.height);
                }
                if let Some(portrait) = self.dialogue_portrait() {
                    camera = portrait;
                }
            }
            if let Some(proof) = &mut self.proof {
                proof.projection = Some(proof::Projection::World(camera, rect));
            }
            let selected = if self.game_running {
                None
            } else if self.editor_tab == EditorTab::World {
                if move_physics {
                    self.selected_physics_proxy()
                } else {
                    self.world_selection
                }
            } else {
                self.selected
            };
            let natural = self.environment_frame(Vec3::from_array(camera.target));
            let snow_tracks = self.snow_tracks.near(Vec3::from_array(camera.target), natural.as_ref().map_or(0.0, |frame| frame.elapsed));
            let mut environment = self.environment_parts();
            self.append_environment_splashes(&mut environment);
            environment.extend(viewport3d::key_parts(
                &self.scene.world,
                self.game_running.then_some(&self.interactions),
            ));
            let architecture = if self.game_running {
                self.scene
                    .world
                    .building_parts_with(false, &self.interactions)
            } else {
                self.scene.world.building_parts(false)
            };
            let camera_eye = self.game_running.then(|| camera.projection(rect).0);
            let mut models = self
                .scene
                .world
                .models
                .iter()
                .filter(|item| !self.game_running || self.interactions.object_active(item.id))
                .filter_map(|item| {
                    let original = self.assets.get(&item.source)?;
                    if camera_eye.is_some_and(|eye| !item.visible_from(eye, &original.model)) {
                        return None;
                    }
                    let loaded = self.assets.get(item.render_source()).unwrap_or(original);
                    let vegetation = natural
                        .as_ref()
                        .filter(|_| item.material.wind_response > 0.0)
                        .map_or([0.0; 4], |frame| {
                            let origin = Vec3::from_array(item.position);
                            let wind = self.environment_wind(origin);
                            let height = (loaded.model.bounds_max[1] - loaded.model.bounds_min[1])
                                .max(0.003);
                            let stature = height * item.scale[1];
                            let phase = (frame.elapsed * 2.1
                                + f64::from(origin.x * 0.71 + origin.z * 0.93))
                                as f32;
                            let sway = wind * (0.76 + 0.21 * phase.sin() + 0.10 * (phase * 2.4).sin())
                                + Vec3::new(wind.z, 0.0, -wind.x) * ((phase * 1.63).cos() * 0.19);
                            let bend = sway.clamp_length_max(9.0)
                                * (stature * 0.022 * item.material.wind_response);
                            [bend.x, bend.z, loaded.model.bounds_min[1], height.recip()]
                        });
                    Some(ModelDraw {
                        token: loaded.token,
                        model: &loaded.model,
                        position: item.visual_position(),
                        rotation: item.rotation,
                        scale: item.scale,
                        selected: selected == Some(item.id)
                            || (edit_world && self.gizmo.selection().contains(&item.id)),
                        clip: None,
                        elapsed: 0.0,
                        blend: None,
                        bone_offsets: Vec::new(),
                        bone_translations: Vec::new(),
                        morph_weights: Vec::new(),
                        secondary: model::SecondaryPose::default(),
                        tuning: item.material,
                        vegetation,
                        receives_weather: true,
                        graph: item
                            .material_graph
                            .as_ref()
                            .and_then(material_graph::Graph::compile),
                        material_preview: if edit_world && selected == Some(item.id) {
                            assets::material_preview(ui.ctx(), item.id)
                        } else {
                            material_graph::PreviewLight::Scene
                        },
                    })
                })
                .collect::<Vec<_>>();
            models.extend(self.npc_draws());
            let avatar = self.avatar_draw();
            let character = if let Some(avatar) = avatar {
                models.push(avatar);
                &[][..]
            } else {
                parts.as_slice()
            };
            let particles = self.effect_particles();
            let lights = self.scene.world.models.iter()
                .filter(|item| !self.game_running || self.interactions.object_active(item.id))
                .filter_map(assets::Placement::point_light)
                .collect::<Vec<_>>();
            viewport3d::paint(
                ui,
                rect,
                ViewportScene {
                    robot: character,
                    environment: &environment,
                    architecture: &architecture,
                    models: &models,
                    camera,
                    selected,
                    sparks: &sparks,
                    glow,
                    playing: self.game_running,
                    lighting: self.scene.lighting,
                    post_process: self.scene.post_process,
                    lights: &lights,
                    effects: &particles,
                    natural: natural.as_ref().map(|frame| viewport3d::NaturalScene {
                        atmosphere: &frame.atmosphere,
                        water: &frame.water,
                        elapsed: frame.elapsed,
                        wind: frame.wind,
                        snow_tracks: &snow_tracks,
                    }),
                },
            );
            if !self.game_running
                && self.editor_tab == EditorTab::World
                && let Some(item) = self
                    .scene
                    .world
                    .models
                    .iter()
                    .find(|item| Some(item.id) == self.world_selection)
            {
                viewport3d::placement_guides(ui, rect, camera, item);
            }
            if edit_world {
                self.gizmo
                    .paint(ui, rect, camera, &self.scene.world, &self.assets);
            }
            self.paint_route_3d(ui, rect, camera);
        } else if self.overhead {
            self.viewport_plan(ui, rect, &response, &parts, &sparks, glow);
        } else {
            self.viewport_2d(ui, rect, &response, &parts, &sparks, glow);
        }
    }

    fn avatar_draw(&self) -> Option<ModelDraw<'_>> {
        self.avatar_pose(true)
    }

    fn avatar_draw_without_foot(&self) -> Option<ModelDraw<'_>> {
        self.avatar_pose(false)
    }

    fn avatar_pose(&self, foot_lock: bool) -> Option<ModelDraw<'_>> {
        let avatar = self.scene.avatar.as_ref()?;
        let (model, token) = self.avatar_asset()?;
        let foot = self.actor_position();
        let heading = self
            .seating
            .as_ref()
            .map_or(self.avatar_heading, Seating::heading);
        let (position, rotation, scale) = avatar.transform(model, foot, heading);
        let pose = |motion, time: f32| {
            let clip = avatar.clip(motion);
            let elapsed = if self.game_running
                && motion == Motion::Jump
                && !avatar.procedural_jump()
                && let Some(animation) = clip.and_then(|index| model.animations.get(index))
            {
                (time * avatar.animation_speed).min(0.83) * (animation.duration / 0.84)
            } else {
                time * avatar.animation_speed
            };
            (clip, elapsed)
        };
        let (motion, time) = if self.game_running {
            (self.avatar_motion, self.avatar_clip_time)
        } else if self.preview_avatar {
            (self.preview_motion, self.preview_elapsed)
        } else {
            (self.avatar_motion, self.time)
        };
        let imported_preview = (!self.game_running && self.preview_avatar)
            .then_some(self.preview_external_clip)
            .flatten()
            .filter(|&index| model.animations.get(index).is_some());
        let (clip, elapsed) = imported_preview.map_or_else(
            || pose(motion, time),
            |index| (Some(index), self.preview_elapsed * avatar.animation_speed),
        );
        let blend = self.avatar_previous.filter(|_| imported_preview.is_none()).map(|(previous, time, _)| {
            let (clip, elapsed) = pose(previous, time);
            let progress = (self.avatar_elapsed / self.player_blend_seconds()).clamp(0.0, 1.0);
            BlendPose {
                clip,
                elapsed,
                weight: progress * progress * (3.0 - 2.0 * progress),
            }
        });
        let mut draw = ModelDraw {
            token: token | (1 << 63),
            model,
            position,
            rotation,
            scale,
            selected: false,
            clip,
            elapsed,
            blend,
            secondary: model::SecondaryPose::default(),
            tuning: assets::materials::Tuning::default(),
            vegetation: [0.0; 4],
            receives_weather: false,
            graph: None,
            material_preview: self.chibi_preview_light(model),
            bone_translations: Vec::new(),
            morph_weights: Vec::new(),
            bone_offsets: avatar.bone_offsets(if self.game_running || self.preview_avatar {
                (time * avatar.animation_speed).rem_euclid(self.scene.animation.duration)
            } else {
                self.time
            }),
        };
        if self.game_running
            && let Some((clip, elapsed, blend)) = self.actor_transition.pose(draw.clip, draw.elapsed)
        {
            draw.clip = clip;
            draw.elapsed = elapsed;
            draw.blend = blend;
        }
        if let Some(seat) = &self.seating {
            draw.clip = avatar.idle;
            draw.elapsed = 0.0;
            draw.blend = None;
            if let Some((index, animation)) = avatar.sit.and_then(|index| {
                model
                    .animations
                    .get(index)
                    .map(|animation| (index, animation))
            }) {
                draw.clip = Some(index);
                draw.elapsed = animation.duration * 0.5 * seat.amount;
                if seat.amount < 0.24 {
                    draw.blend = Some(BlendPose {
                        clip: avatar.idle,
                        elapsed: 0.0,
                        weight: seat.amount / 0.24,
                    });
                }
            } else if avatar.source == assets::Source::CityHuman {
                for (bone, angle) in [
                    ("thighL", 87.0),
                    ("thighR", 87.0),
                    ("shinL", -76.0),
                    ("shinR", -76.0),
                    ("footL", -11.0),
                    ("footR", -11.0),
                ] {
                    draw.bone_offsets.push(model::BoneRotation {
                        bone: bone.into(),
                        degrees: [angle * seat.weight(), 0.0, 0.0],
                    });
                }
            } else if avatar.source.modern_human() {
                actor_motion::sit(model, &mut draw.bone_offsets, seat.weight());
            }
        }
        draw.morph_weights = self.player_dialogue_face(model);
        draw.morph_weights.extend(self.chibi_preview_face(model));
        character_shapes::compose(model, &avatar.shape_weights, &mut draw.morph_weights);
        if self.game_running || (self.preview_avatar && imported_preview.is_none()) {
            let look_delta = self
                .story_session
                .active_npc()
                .and_then(|id| {
                    let npc = self.scene.npcs.iter().find(|npc| npc.id == id)?;
                    let position = self.npc_position(id)?;
                    let height_offset = (npc.avatar.height - avatar.height) * 0.86;
                    Some((position - foot + Vec3::Y * height_offset).to_array())
                });
            let stride = if self.story_session.is_open() {
                0.0
            } else if self.game_running {
                self.game.move_blend
            } else if motion == Motion::Idle {
                0.0
            } else {
                0.6
            };
            draw.secondary = self.player_secondary.pose(self.live_seconds);
            draw.secondary.hair_contacts = avatar.actor_tuning.hair_contact_quality(self.low_fx);
            if self.seating.is_none() {
                if avatar.procedural_jump() && motion == Motion::Jump {
                    let phase = if self.game_running {
                        self.game
                            .flight_phase_with(self.scene.play_settings.movement_3d.jump_speed)
                    } else {
                        (time / 0.84).rem_euclid(1.0)
                    };
                    actor_motion::jump(model, &mut draw.bone_offsets, phase, stride);
                } else if let Some(impact) = self.avatar_landing {
                    draw.position[1] -= actor_motion::impact_weight(impact / 0.24) * 0.054;
                    actor_motion::land(model, &mut draw.bone_offsets, impact / 0.24, stride);
                }
                if motion != Motion::Jump {
                    let turn = (self.game.yaw - heading + 180.0).rem_euclid(360.0) - 180.0;
                    actor_motion::balance(model, &mut draw.bone_offsets, stride, turn);
                }
            }
            let current_speaker = self.active_speaker();
            let motion = actor_motion::Motion {
                seconds: self.live_seconds,
                stride,
                grounded: !self.game_running || self.game.grounded,
                seated: self.seating.is_some(),
                yaw: heading,
                look_delta,
                talking: current_speaker == Some(story::Speaker::Player)
                    && !actor_expression::owns_speech(&draw.morph_weights),
                breathing: Some(self.player_secondary.breath(avatar.actor_tuning)),
                upper_body_tilt: self.player_secondary.transfer.torso_tilt(),
            };
            let morph_gaze = actor_expression::animate(
                model, &mut draw.morph_weights, &mut draw.secondary, &motion,
                avatar.actor_tuning.face_strength, 1, self.story_session.is_open(),
            );
            actor_motion::append_with_eye_tracking(
                model,
                &mut draw.bone_offsets,
                motion,
                if morph_gaze { 0.0 } else { avatar.actor_tuning.face_strength },
            );
            self.dialogue_listener(story::Speaker::Player, model, &mut draw.bone_offsets);
        }
        if foot_lock {
            self.apply_foot_locks(
                0,
                &mut draw,
                foot.y,
                self.game.grounded && self.seating.is_none(),
                motion,
            );
        }
        self.pose_door_hand(&mut draw);
        Some(draw)
    }

    fn focus_camera(&mut self) {
        if self.editor_tab == EditorTab::Npcs
            && let Some(id) = self.selected_npc
            && let Some(position) = self.npc_position(id)
            && let Some(npc) = self.scene.npcs.iter().find(|npc| npc.id == id)
        {
            self.camera.target = (position + Vec3::Y * (npc.avatar.height * 0.53)).to_array();
            self.camera.distance = (npc.avatar.height * 2.15).clamp(1.4, 28.0);
            if let Some(pose) = self.npc_poses().into_iter().find(|pose| pose.id == id) {
                self.camera.yaw = pose.yaw.to_radians() + 0.25;
            }
            self.plan_offset = egui::vec2(position.x, position.z);
            return;
        }
        if self.editor_tab == EditorTab::World
            && let Some(building) = self
                .scene
                .world
                .buildings
                .iter()
                .find(|item| Some(item.id) == self.world_selection)
        {
            self.camera.target = [
                building.position[0],
                building.position[1] + building.size[1] * 0.48,
                building.position[2],
            ];
            self.camera.distance = (building.size[0].max(building.size[2]) * 1.85).clamp(2.5, 36.0);
            self.plan_offset = egui::vec2(building.position[0], building.position[2]);
            return;
        }
        if self.editor_tab == EditorTab::Character
            && let Some(avatar) = &self.scene.avatar
        {
            self.camera.target = [0.0, avatar.height * 0.53, 0.0];
            self.camera.distance = (avatar.height * 2.15).clamp(1.4, 28.0);
            return;
        }
        if self.editor_tab == EditorTab::World
            && let Some(placement) = self
                .scene
                .world
                .models
                .iter()
                .find(|item| Some(item.id) == self.world_selection)
            && let Some(loaded) = self.assets.get(&placement.source)
        {
            let bounds = placement.bounds(&loaded.model);
            let min = Vec3::from_array(bounds.min);
            let max = Vec3::from_array(bounds.max);
            self.camera.target = ((min + max) * 0.5).to_array();
            self.camera.distance = ((max - min).max_element() * 2.15).clamp(1.4, 28.0);
            return;
        }
        let parts = if self.editor_tab == EditorTab::World {
            self.scene.world.render_parts()
        } else {
            self.scene.sample(self.time)
        };
        let selected = if self.editor_tab == EditorTab::World {
            self.world_selection
        } else {
            self.selected
        };
        if let Some(part) = parts.iter().find(|part| Some(part.id) == selected) {
            self.camera.target = part.position;
            self.camera.distance = (Vec3::from_array(part.size).length() * 2.4).clamp(2.5, 30.0);
        }
    }

    fn viewport_2d(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        response: &egui::Response,
        parts: &[Part],
        sparks: &[Spark],
        glow: f32,
    ) {
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 7.0, BACKDROP);
        let ground = rect.top() + rect.height() * 0.78;
        let tile =
            ((rect.height() * 0.76).min(rect.width() * 0.78) / sprite::SIDE as f32).max(0.01);
        let scale = sprite::scale_for(&self.scene.parts);
        let units = tile * scale;
        let origin = egui::pos2(rect.center().x, ground);
        let actor = self.actor_position();
        let robot_x = actor.x * units;
        let camera_x = robot_x - robot_x.clamp(-rect.width() * 0.31, rect.width() * 0.31);
        let floor_rect = Rect::from_min_max(egui::pos2(rect.left(), ground), rect.right_bottom());
        painter.rect_filled(floor_rect, 0.0, Color32::from_rgb(19, 36, 53));
        for index in -30..=30 {
            let x = origin.x + index as f32 * (units * 0.55).max(18.0) - camera_x;
            if rect.x_range().contains(x) {
                painter.vline(
                    x,
                    rect.y_range(),
                    egui::Stroke::new(1.0_f32, Color32::from_gray(34)),
                );
            }
        }
        painter.hline(
            rect.x_range(),
            ground,
            egui::Stroke::new(1.2_f32, Color32::from_rgb(64, 120, 122)),
        );
        self.paint_world_2d(&painter, origin, camera_x, units);
        self.paint_npc_side(ui, &painter, origin, camera_x, units);
        let tile_rect = Rect::from_min_size(
            egui::pos2(
                origin.x - camera_x + robot_x - sprite::SIDE as f32 * tile * 0.5,
                ground - actor.y * units - sprite::FLOOR * tile,
            ),
            Vec2::splat(tile * sprite::SIDE as f32),
        );
        if let Some(proof) = &mut self.proof {
            proof.sprite_canvas = Some(tile_rect);
        }
        if !self.preview && !self.game_running && self.brush != Brush::None {
            if self.scene.sprite_art.drawn_only {
                for row in 0..12 {
                    for column in 0..12 {
                        let min =
                            tile_rect.min + egui::vec2(column as f32, row as f32) * tile * 8.0;
                        let color = if (row + column) % 2 == 0 {
                            Color32::from_rgb(36, 48, 62)
                        } else {
                            Color32::from_rgb(47, 61, 76)
                        };
                        painter.rect_filled(
                            Rect::from_min_size(min, Vec2::splat(tile * 8.0)),
                            0.0,
                            color,
                        );
                    }
                }
            }
            self.paint_pixels(ui, response, tile_rect, tile);
        }
        if let Some(texture) = self.prepare_sprite(ui.ctx(), parts, scale, glow) {
            painter.image(
                texture,
                tile_rect,
                Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        for spark in sparks {
            let center = Vec3::from_array(spark.position);
            let p = origin + egui::vec2(center.x * units - camera_x, -center.y * units);
            painter.circle_filled(p, (spark.size * units).max(1.0), rgb(spark.color));
        }
        effects_2d::paint(
            &painter,
            &self.effect_particles(),
            effects_2d::Style::Pixel,
            units,
            |point| origin + egui::vec2(point.x * units - camera_x, -point.y * units),
        );
        if !self.game_running && self.brush != Brush::None {
            painter.rect_stroke(
                tile_rect,
                0.0,
                egui::Stroke::new(1.0_f32, ACCENT),
                egui::StrokeKind::Inside,
            );
        }
    }

    fn sprite_controls(&mut self, ui: &mut egui::Ui) {
        if self.scene.avatar.is_some() {
            egui::ComboBox::from_id_salt("bake_motion")
                .selected_text(
                    self.preview_external_clip
                        .and_then(|index| self.avatar_asset()?.0.animations.get(index))
                        .map_or(self.preview_motion.label(), |clip| clip.name.as_str()),
                )
                .show_ui(ui, |ui| {
                    for motion in Motion::ALL {
                        let selected = self.preview_external_clip.is_none() && self.preview_motion == motion;
                        if ui.selectable_label(selected, motion.label()).clicked() {
                            self.preview_motion = motion;
                            self.preview_external_clip = None;
                            self.preview_avatar = false;
                        }
                    }
                    let count = self.scene.avatar.as_ref().map_or(0, |avatar| avatar.imported_clips.len());
                    let imported = self.avatar_asset().into_iter()
                        .flat_map(|(model, _)| model.animations.iter().enumerate().rev().take(count))
                        .map(|(index, clip)| (index, clip.name.clone()))
                        .collect::<Vec<_>>();
                    for (index, name) in imported.into_iter().rev() {
                        ui.selectable_value(&mut self.preview_external_clip, Some(index), name);
                    }
                });
            if ui
                .button("Кадр из 3D")
                .on_hover_text("Перенести текущий момент шкалы и выбранный клип в этот кадр")
                .clicked()
            {
                self.bake_avatar_sprite(false);
            }
            if ui
                .button("Клип в 8 кадров")
                .on_hover_text("Запечь выбранный клип для текущего ракурса")
                .clicked()
            {
                self.bake_avatar_sprite(true);
            }
        }
        if ui
            .checkbox(&mut self.scene.sprite_art.drawn_only, "Только рисунок")
            .changed()
        {
            self.changed();
        }
        if self.scene.avatar.is_some()
            && ui
                .checkbox(&mut self.scene.sprite_art.use_on_plan, "Рисунок на плане")
                .on_hover_text("Использовать боковые кадры и на 2D-плане")
                .changed()
        {
            self.changed();
        }
        let frames = self.scene.sprite_art.frames;
        let mut frame = self.sprite_frame + 1;
        ui.add(
            egui::DragValue::new(&mut frame)
                .prefix("Кадр ")
                .suffix(format!(" / {frames}"))
                .range(1..=frames),
        );
        self.sprite_frame = frame - 1;
        if ui
            .add_enabled(
                frames < scene::MAX_SPRITE_FRAMES,
                egui::Button::new("+ Кадр"),
            )
            .on_hover_text("Скопировать текущий кадр")
            .clicked()
        {
            let copies = self
                .scene
                .sprite_pixels
                .iter()
                .filter(|pixel| pixel.frame == Some(self.sprite_frame))
                .cloned()
                .map(|mut pixel| {
                    pixel.frame = Some(frames);
                    pixel
                })
                .collect::<Vec<_>>();
            if self.scene.sprite_pixels.len() + copies.len() <= scene::MAX_SPRITE_PIXELS {
                self.scene.sprite_pixels.extend(copies);
                self.scene.sprite_art.frames += 1;
                self.sprite_frame = frames;
                self.changed();
            } else {
                self.status = "Лимит пикселей проекта достигнут.".into();
            }
        }
        ui.add(
            egui::DragValue::new(&mut self.scene.sprite_art.fps)
                .suffix(" кадр/с")
                .range(1..=24),
        );
    }

    fn bake_avatar_sprite(&mut self, sequence: bool) {
        let Some(avatar) = &self.scene.avatar else {
            return;
        };
        let Some((model, _)) = self.avatar_asset() else {
            return;
        };
        let clip = self.preview_external_clip
            .filter(|&index| model.animations.get(index).is_some())
            .or_else(|| avatar.clip(self.preview_motion));
        let direction = self.direction;
        let frames = if sequence {
            0..scene::MAX_SPRITE_FRAMES
        } else {
            self.sprite_frame..self.sprite_frame + 1
        };
        let clip_length = clip
            .and_then(|index| model.animations.get(index))
            .map_or(self.scene.animation.duration, |animation| {
                animation.duration
            });
        let mut pixels = Vec::new();
        for frame in frames.clone() {
            let time = if sequence {
                clip_length * f32::from(frame) / f32::from(scene::MAX_SPRITE_FRAMES)
            } else {
                self.time * avatar.animation_speed
            };
            let pose_time = if sequence {
                time.rem_euclid(self.scene.animation.duration)
            } else {
                self.time
            };
            let offsets = avatar.bone_offsets(pose_time);
            let image = if offsets.is_empty() {
                sprite_mesh::rasterize(model, clip, time, direction)
            } else {
                sprite_mesh::rasterize_with_overrides(model, clip, time, direction, &offsets)
            };
            pixels.extend(
                image
                    .chunks_exact(4)
                    .enumerate()
                    .filter_map(|(index, color)| {
                        (color[3] != 0).then(|| SpritePixel {
                            direction,
                            frame: Some(frame),
                            baked: true,
                            x: (index % sprite::SIDE) as u8,
                            y: (index / sprite::SIDE) as u8,
                            color: [color[0], color[1], color[2], color[3]],
                        })
                    }),
            );
        }
        let replace = |pixel: &SpritePixel| {
            pixel.baked
                && pixel.direction == direction
                && pixel.frame.is_some_and(|frame| frames.contains(&frame))
        };
        let remaining = self
            .scene
            .sprite_pixels
            .iter()
            .filter(|pixel| !replace(pixel))
            .count();
        if pixels.is_empty() {
            self.status = "В выбранной позе не получилось получить изображение.".into();
        } else if remaining + pixels.len() > scene::MAX_SPRITE_PIXELS {
            self.status = "Лимит пикселей проекта достигнут.".into();
        } else {
            self.scene.sprite_pixels.retain(|pixel| !replace(pixel));
            self.scene.sprite_pixels.extend(pixels);
            self.scene.sprite_art.drawn_only = true;
            if sequence {
                self.scene.sprite_art.frames = scene::MAX_SPRITE_FRAMES;
                self.sprite_frame = 0;
                self.status = "Клип запечён в 8 кадров. Ручные штрихи сохранены.".into();
            } else {
                self.status = "Кадр из 3D добавлен. Его можно дорисовать кистью.".into();
            }
            self.changed();
        }
    }

    fn live_player_sprite(&mut self, ctx: &egui::Context) -> Option<egui::TextureId> {
        const FRAMES: u8 = 16;
        let avatar = self.scene.avatar.as_ref()?;
        let (model, _) = self.avatar_asset()?;
        let model = Arc::clone(model);
        let (motion, heading, elapsed) = if self.game_running {
            (
                self.avatar_motion,
                self.avatar_heading + avatar.yaw,
                self.avatar_clip_time * avatar.animation_speed,
            )
        } else if self.overhead {
            (Motion::Idle, avatar.yaw, 0.0)
        } else {
            (
                self.preview_motion,
                -90.0 * f32::from(self.direction),
                self.time * avatar.animation_speed,
            )
        };
        let plan = self.overhead;
        let direction = sprite_mesh::direction(heading);
        let clip = avatar.clip(motion);
        let duration = clip
            .and_then(|id| model.animations.get(id))
            .map_or(1.0, |item| item.duration.max(0.001));
        let procedural_jump = motion == Motion::Jump && avatar.procedural_jump();
        let phase = if procedural_jump && self.game_running {
            self.game
                .flight_phase_with(self.scene.play_settings.movement_3d.jump_speed)
                .min(0.999)
        } else if motion == Motion::Jump {
            (elapsed / 0.84).clamp(0.0, 0.999)
        } else {
            elapsed.rem_euclid(duration) / duration
        };
        let frame = ((phase * f32::from(FRAMES)) as u8).min(FRAMES - 1);
        let (swing, secondary) = if self.game_running && model.has_secondary_pose() {
            sprite_mesh::quantize_swing(self.player_secondary.pose(self.live_seconds))
        } else {
            (0, model::SecondaryPose::default())
        };
        let key = (
            motion as u8,
            clip,
            sprite_mesh::FrameKey {
                direction,
                frame,
                plan,
                swing,
            },
        );
        if let Some(texture) = self.live_player_sprites.get(key) {
            return Some(texture);
        }
        let fraction = f32::from(frame) / f32::from(FRAMES);
        let time = if procedural_jump {
            0.0
        } else {
            duration * fraction
        };
        let mut offsets = avatar.bone_offsets(time.rem_euclid(self.scene.animation.duration));
        if procedural_jump {
            actor_motion::jump(&model, &mut offsets, fraction, 0.4);
        }
        let rgba = sprite_mesh::rasterize_live(
            &model,
            sprite_mesh::LivePose {
                clip,
                elapsed: time,
                yaw: heading,
                plan,
                bones: &offsets,
                secondary,
            },
        );
        let image = egui::ColorImage::from_rgba_unmultiplied([sprite_mesh::LIVE_SIDE; 2], &rgba);
        let texture = ctx.load_texture("live_player_pose", image, egui::TextureOptions::LINEAR);
        Some(self.live_player_sprites.insert(key, texture))
    }

    fn prepare_sprite(
        &mut self,
        ctx: &egui::Context,
        parts: &[Part],
        scale: f32,
        glow: f32,
    ) -> Option<egui::TextureId> {
        let live_plan = self.uses_live_plan_actor();
        if (!self.scene.sprite_art.drawn_only || live_plan)
            && self.seating.as_ref().is_some_and(|seat| seat.amount > 0.95)
        {
            if self.seat_sprite.is_none()
                && let Some(draw) = self.avatar_draw()
            {
                let direction = (-self.seating.as_ref().unwrap().seat.yaw / 90.0)
                    .round()
                    .rem_euclid(4.0) as u8;
                let rgba = sprite_mesh::rasterize_with_overrides(
                    draw.model,
                    draw.clip,
                    draw.elapsed,
                    direction,
                    &draw.bone_offsets,
                );
                let image = egui::ColorImage::from_rgba_unmultiplied([sprite::SIDE; 2], &rgba);
                self.seat_sprite =
                    Some(ctx.load_texture("seated_actor", image, egui::TextureOptions::NEAREST));
            }
            if let Some(texture) = &self.seat_sprite {
                return Some(texture.id());
            }
        }
        if (live_plan
            || (!self.scene.sprite_art.drawn_only
                && self.scene.sprite_pixels.is_empty()
                && (self.game_running || self.brush == Brush::None)))
            && let Some(texture) = self.live_player_sprite(ctx)
        {
            return Some(texture);
        }
        let drawing = self.scene.sprite_art.drawn_only;
        let direction = if self.game_running {
            (-self.game.yaw / 90.0).round().rem_euclid(4.0) as u8
        } else {
            self.direction
        };
        let frame = if self.game_running || self.preview {
            let time = if self.game_running {
                if self.avatar_motion == Motion::Idle {
                    0.0
                } else {
                    self.avatar_elapsed
                }
            } else {
                self.time
            };
            ((time * f32::from(self.scene.sprite_art.fps)) as u32
                % u32::from(self.scene.sprite_art.frames)) as u8
        } else {
            self.sprite_frame
        };
        let direction =
            painted_direction(&self.scene, direction, self.game_running || self.preview);
        let key = (
            self.revision,
            direction,
            frame,
            if drawing { 0 } else { self.time.to_bits() },
            if drawing {
                0
            } else {
                self.game.phase.to_bits()
            },
            if drawing {
                0
            } else {
                self.game.move_blend.to_bits()
            },
            if drawing { 0 } else { (glow * 512.0) as u16 },
        );
        if self.sprite_key != Some(key) {
            let parts = if drawing { &[][..] } else { parts };
            let rgba = sprite::rasterize(
                parts,
                direction,
                &self.scene.sprite_pixels,
                scale,
                glow,
                Some(frame),
            );
            let color = egui::ColorImage::from_rgba_unmultiplied([sprite::SIDE; 2], &rgba);
            if let Some(texture) = &mut self.sprite_texture {
                texture.set(color, egui::TextureOptions::NEAREST);
            } else {
                self.sprite_texture =
                    Some(ctx.load_texture("actor_sprite", color, egui::TextureOptions::NEAREST));
            }
            self.sprite_key = Some(key);
        }
        self.sprite_texture.as_ref().map(egui::TextureHandle::id)
    }

    fn uses_live_plan_actor(&self) -> bool {
        self.view == View::Two
            && self.overhead
            && !self.scene.sprite_art.use_on_plan
            && self.avatar_asset().is_some()
    }

    fn viewport_plan(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        response: &egui::Response,
        parts: &[Part],
        sparks: &[Spark],
        glow: f32,
    ) {
        let scale = (rect.height().min(rect.width()) / 18.0).max(8.0);
        if response.hovered() {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            self.plan_zoom = (self.plan_zoom * (scroll * 0.003).exp()).clamp(0.42, 4.0);
        }
        if !self.game_running && response.dragged_by(egui::PointerButton::Secondary) {
            self.plan_offset -= ui.input(|input| input.pointer.delta()) / (scale * self.plan_zoom);
        }
        let focus = if self.game_running {
            egui::vec2(self.camera_follow.x, self.camera_follow.z - 1.4)
        } else {
            self.plan_offset
        };
        let plan = plan2d::Plan {
            rect,
            focus,
            scale: scale * self.plan_zoom,
        };
        if let Some(proof) = &mut self.proof {
            proof.projection = Some(proof::Projection::Plan(plan));
        }
        if self.game_running
            && !self.story_session.is_open()
            && !self.licenses_open
            && self.seating.is_none()
            && response.clicked_by(egui::PointerButton::Primary)
            && let Some(mouse) = response.interact_pointer_pos()
        {
            let target = plan.coordinates(mouse);
            self.walk_target = Some([target.x, target.y]);
        }
        if !self.game_running
            && self.editor_tab == EditorTab::Npcs
            && self.npc_panel == npc_ui::Panel::Route
            && self.npc_place_on_plan
            && response.clicked_by(egui::PointerButton::Primary)
            && let Some(mouse) = response.interact_pointer_pos()
            && let Some(npc) = self
                .scene
                .npcs
                .iter_mut()
                .find(|npc| Some(npc.id) == self.selected_npc)
        {
            let point = plan.coordinates(mouse);
            let height = npc
                .route
                .get(self.npc_waypoint)
                .map_or(0.0, |waypoint| waypoint.position[1]);
            npc_ui::add_point(npc, [point.x, height, point.y], &mut self.npc_waypoint);
            self.changed();
        }
        if !self.game_running
            && self.editor_tab == EditorTab::Npcs
            && self.npc_panel == npc_ui::Panel::Tasks
            && response.clicked_by(egui::PointerButton::Primary)
            && let Some(mouse) = response.interact_pointer_pos()
            && let Some(id) = self.npc_quest_on_plan.take()
        {
            let point = plan.coordinates(mouse);
            if self
                .scene
                .story
                .set_visit_destination(id, [point.x, point.y])
            {
                self.status = "Точка задания поставлена на плане.".into();
                self.changed();
            }
        }
        if !self.game_running
            && self.editor_tab == EditorTab::World
            && response.clicked_by(egui::PointerButton::Primary)
            && let Some(mouse) = response.interact_pointer_pos()
        {
            if self.physics_editor.placing() {
                let point = plan.coordinates(mouse);
                self.place_physics_at(point.x, point.y);
            } else if let Some(changed) = inspector::key_at_plan_click(
                ui.ctx(),
                &mut self.scene.world,
                &mut self.world_selection,
                plan,
                mouse,
            ) {
                if changed {
                    self.changed();
                }
            } else {
                let position = plan.coordinates(mouse);
                if let Some(building) = self.scene.world.buildings.iter_mut().find(|building| {
                    let size = if building.quarter_turns % 2 == 0 {
                        egui::vec2(building.size[0], building.size[2])
                    } else {
                        egui::vec2(building.size[2], building.size[0])
                    };
                    Rect::from_center_size(
                        egui::pos2(building.position[0], building.position[2]),
                        size,
                    )
                    .contains(position.to_pos2())
                }) {
                    let point = [position.x, position.y];
                    let shifting = ui.input(|input| input.modifiers.shift);
                    if shifting
                        && let Some(item) = interior::selected_furniture(ui.ctx(), building.id)
                        && building.move_furniture_at(item, point)
                    {
                        self.status = "Предмет перемещён на плане.".into();
                    } else if !shifting && let Some(item) = building.furniture_at(point) {
                        interior::select_furniture(ui.ctx(), building.id, item);
                    }
                    self.world_selection = Some(building.id);
                }
            }
        }
        let painter = ui.painter().with_clip_rect(rect);
        let streets = self
            .scene
            .world
            .models
            .iter()
            .filter(|model| model.source == assets::Source::CityStreet)
            .map(|model| [model.position[0], model.position[2]])
            .collect::<Vec<_>>();
        plan.terrain(&painter, &streets, !self.game_running);
        let natural = self.environment_frame(self.actor_position());
        if let Some(frame) = &natural {
            environment_2d::paint_frame(
                &painter,
                frame,
                environment_2d::Projection::Xz,
                |position| plan.point(position.x, position.z),
            );
        }
        if !self.game_running
            && self.editor_tab == EditorTab::Rules
            && let Some(zone) = self
                .rule_editor
                .selected_zone()
                .and_then(|id| self.scene.rules.zone(id))
        {
            let [x, _, z] = zone.position;
            let [width, _, depth] = zone.size;
            let bounds =
                Rect::from_center_size(plan.point(x, z), egui::vec2(width, depth) * plan.scale);
            painter.rect_filled(bounds, 3.0, ACCENT.gamma_multiply(0.18));
            painter.rect_stroke(
                bounds,
                3.0,
                egui::Stroke::new(2.0_f32, ACCENT),
                egui::StrokeKind::Inside,
            );
            painter.text(
                bounds.center_top() + egui::vec2(0.0, -5.0),
                egui::Align2::CENTER_BOTTOM,
                &zone.name,
                egui::FontId::proportional(13.0),
                ACCENT,
            );
        }
        if let Some([x, z]) = self.walk_target.filter(|_| self.game_running) {
            painter.circle_stroke(
                plan.point(x, z),
                6.0,
                egui::Stroke::new(1.5_f32, ACCENT.gamma_multiply(0.65)),
            );
        }
        let selected = (!self.game_running && self.editor_tab == EditorTab::World)
            .then_some(self.world_selection)
            .flatten();
        let furniture = selected.and_then(|id| interior::selected_furniture(ui.ctx(), id));
        plan.objects(&painter, self.world_parts_2d(), selected, furniture);
        for building in &self.scene.world.buildings {
            plan.doorway(&painter, building.door_bounds());
            for opening in building.interior_doors() {
                plan.doorway(&painter, opening);
            }
        }
        let runtime = self.game_running.then_some(&self.interactions);
        plan.locked_doors(&painter, &self.scene.world, runtime);
        plan.keys(&painter, &self.scene.world, runtime, selected);
        if !self.game_running
            && self.editor_tab == EditorTab::Npcs
            && self.npc_panel == npc_ui::Panel::Tasks
            && let Some([x, z]) = self
                .npc_quest_on_plan
                .and_then(|id| self.scene.story.visit_destination(id))
        {
            painter.circle_stroke(plan.point(x, z), 9.0, egui::Stroke::new(1.5_f32, ACCENT));
        }
        self.paint_npc_plan(ui, &painter, plan);
        let texture =
            self.prepare_sprite(ui.ctx(), parts, sprite::scale_for(&self.scene.parts), glow);
        let actor = self.actor_position();
        let model_height = self
            .scene
            .avatar
            .as_ref()
            .filter(|_| {
                self.uses_live_plan_actor()
                    || (!self.scene.sprite_art.drawn_only && self.scene.sprite_pixels.is_empty())
            })
            .map(|avatar| avatar.height);
        plan.actor(&painter, texture, actor.to_array(), model_height);
        for spark in sparks {
            let [x, _, z] = spark.position;
            let point = plan.point(x, z);
            let radius = (spark.size * plan.scale).max(1.0);
            painter.circle_filled(point, radius * 2.4, rgb(spark.color).gamma_multiply(0.20));
            painter.circle_filled(point, radius, rgb(spark.color));
        }
        effects_2d::paint(
            &painter,
            &self.effect_particles(),
            effects_2d::Style::Soft,
            plan.scale,
            |point| plan.point(point.x, point.z),
        );
        if let Some(frame) = &natural {
            environment_2d::paint_atmosphere(&painter, frame);
        }
    }

    fn world_parts_2d(&self) -> Vec<Part> {
        let mut parts = self.environment_parts();
        parts.extend(if self.game_running {
            self.scene
                .world
                .building_parts_with(true, &self.interactions)
        } else {
            self.scene.world.building_parts(true)
        });
        for item in &self.scene.world.models {
            if item.source == assets::Source::CityStreet {
                continue;
            }
            let Some(loaded) = self.assets.get(&item.source) else {
                continue;
            };
            let bounds = item.bounds(&loaded.model);
            let min = Vec3::from_array(bounds.min);
            let max = Vec3::from_array(bounds.max);
            parts.push(Part {
                id: item.id,
                name: String::new(),
                shape: Shape::Box,
                position: ((min + max) * 0.5).to_array(),
                rotation: [0.0; 3],
                size: (max - min).to_array(),
                color: [0.42, 0.42, 0.41],
                motion: scene::PartMotion::Static,
            });
        }
        parts
    }

    fn paint_world_2d(&self, painter: &egui::Painter, origin: Pos2, camera_x: f32, units: f32) {
        let mut parts = self.world_parts_2d();
        parts.sort_by(|a, b| a.position[2].total_cmp(&b.position[2]));
        for part in parts {
            let [x, y, z] = part.position;
            let center = origin + egui::vec2(x * units - camera_x, -y * units);
            let size = egui::vec2(part.size[0] * units, part.size[1] * units);
            let rect = Rect::from_center_size(center, size);
            if !painter.clip_rect().intersects(rect) {
                continue;
            }
            let depth = (z - self.game.z).abs();
            let active_slice = depth <= part.size[2] * 0.5 + 0.35;
            let mut tint = rgb(part.color);
            if !active_slice {
                tint = tint.gamma_multiply(0.43);
            }
            match part.shape {
                Shape::Box | Shape::Cylinder => painter.rect_filled(rect, 2.0, tint),
                Shape::Sphere => painter.add(egui::Shape::ellipse_filled(center, size * 0.5, tint)),
            };
            if self.editor_tab == EditorTab::World
                && !self.game_running
                && self.world_selection == Some(part.id)
            {
                painter.rect_stroke(
                    rect,
                    2.0,
                    egui::Stroke::new(1.0_f32, ACCENT),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }

    fn paint_pixels(&mut self, ui: &egui::Ui, response: &egui::Response, canvas: Rect, tile: f32) {
        let begins =
            ui.input(|input| input.pointer.primary_pressed()) && response.contains_pointer();
        if !(begins || response.dragged_by(egui::PointerButton::Primary)) {
            return;
        }
        let Some(current) = ui.input(|input| input.pointer.interact_pos()) else {
            return;
        };
        if !canvas.contains(current) {
            return;
        }
        let delta = ui.input(|input| input.pointer.delta());
        let previous = if begins || !canvas.contains(current - delta) {
            current
        } else {
            current - delta
        };
        let count = ((current - previous).length() / tile).ceil().max(1.0) as usize;
        let [red, green, blue, alpha] = self.ink.to_srgba_unmultiplied();
        let mut changed = false;
        for step in 0..=count {
            let coordinates =
                (previous.lerp(current, step as f32 / count as f32) - canvas.min) / tile;
            let x = (coordinates.x as usize).min(sprite::SIDE - 1) as u8;
            let y = (coordinates.y as usize).min(sprite::SIDE - 1) as u8;
            let matching = |pixel: &SpritePixel| {
                pixel.direction == self.direction && pixel.x == x && pixel.y == y
            };
            let frame = Some(self.sprite_frame);
            let index = self
                .scene
                .sprite_pixels
                .iter()
                .position(|pixel| matching(pixel) && pixel.frame == frame && !pixel.baked)
                .or_else(|| {
                    if self.brush == Brush::Erase {
                        self.scene.sprite_pixels.iter().position(|pixel| {
                            matching(pixel)
                                && (pixel.frame.is_none() || (pixel.baked && pixel.frame == frame))
                        })
                    } else {
                        None
                    }
                });
            let color = [red, green, blue, alpha];
            match (self.brush, index) {
                (Brush::Paint, Some(index)) if self.scene.sprite_pixels[index].color != color => {
                    self.scene.sprite_pixels[index].color = color;
                    changed = true;
                }
                (Brush::Paint, None)
                    if self.scene.sprite_pixels.len() < scene::MAX_SPRITE_PIXELS =>
                {
                    self.scene.sprite_pixels.push(SpritePixel {
                        direction: self.direction,
                        x,
                        y,
                        color,
                        frame,
                        baked: false,
                    });
                    changed = true;
                }
                (Brush::Erase, Some(index)) => {
                    self.scene.sprite_pixels.swap_remove(index);
                    changed = true;
                }
                _ => {}
            }
        }
        if changed {
            self.changed();
        }
    }

    fn effect_strength(&self) -> f32 {
        self.effect_age
            .map(|age| {
                (1.0 - age / self.scene.effect.duration).max(0.0) * self.scene.effect.intensity
            })
            .unwrap_or(0.0)
    }

    fn sparks(&self, strength: f32) -> Vec<Spark> {
        let Some(age) = self.effect_age else {
            return Vec::new();
        };
        let effect = &self.scene.effect;
        let air = self.environment_wind(self.effect_origin) * (0.11 * age * age);
        let count = if self.low_fx {
            effect.count.min(12)
        } else {
            effect.count
        };
        (0..count)
            .map(|index| {
                let angle = index as f32 * 2.399_963;
                let spread = (0.5 + (index % 7) as f32 * 0.15) * (0.22 + age * 1.8);
                let y = (0.07 + age * (1.9 + (index % 5) as f32 * 0.3) - age * age * 2.4).max(0.04);
                Spark {
                    position: (self.effect_origin
                        + Vec3::new(angle.cos() * spread, y, angle.sin() * spread)
                        + air)
                        .to_array(),
                    color: effect
                        .color
                        .map(|component| (component * (0.45 + strength)).clamp(0.0, 1.0)),
                    size: 0.013 + strength * 0.027,
                }
            })
            .collect()
    }

    fn open(&mut self) {
        let Some(path) = self.proof.as_ref().map_or_else(
            || {
                rfd::FileDialog::new()
                    .add_filter("Сцена игры", &["robot.json", "json"])
                    .pick_file()
            },
            |proof| proof.scene.clone(),
        ) else {
            return;
        };
        self.open_scene_path(&path);
    }

    fn open_scene_path(&mut self, path: &Path) {
        if mechanics_pack::matches(path) {
            let loaded = mechanics_pack::load(path).and_then(|scene| {
                let mut staged = self.assets.clone();
                for (source, _) in asset_sources(&scene) {
                    staged.ensure(source, Some(path))?;
                }
                self.assets = staged;
                Ok(scene)
            });
            match loaded {
                Ok(scene) => {
                    self.set_scene(scene, None, "Пакет механик JSON открыт как новый проект.");
                    self.welcome = None;
                    self.view = View::Three;
                }
                Err(error) => self.record_import_problem("Пакет механик", error),
            }
            return;
        }
        match Scene::load(path) {
            Ok(scene) => self.set_scene(scene, Some(path.to_owned()), "Сцена открыта."),
            Err(error) => self.record_import_problem("Сцена", error),
        }
    }

    fn save(&mut self, choose: bool) {
        let path = if choose || self.project.is_none() {
            self.proof.as_ref().map_or_else(
                || {
                    rfd::FileDialog::new()
                        .set_file_name("scene.robot.json")
                        .add_filter("Сцена игры", &["json"])
                        .save_file()
                },
                |proof| proof.scene.clone(),
            )
        } else {
            self.project.clone()
        };
        let Some(path) = path else { return };
        self.remember_checkpoint();
        let linked_root = self.link_playthrough.as_ref().map(|play| {
            let mut root = play.root.clone();
            root.checkpoint = self.scene.checkpoint.clone();
            root
        });
        let scene = linked_root.as_ref().unwrap_or(&self.scene);
        match self
            .assets
            .save_files(asset_sources(scene), &path)
            .and_then(|()| scene.save(&path))
        {
            Ok(()) => {
                self.status = format!("Сохранено: {}", path.display());
                self.project = Some(path);
                self.commit_editor_change();
                self.desk.saved();
            }
            Err(error) => self.status = format!("Не удалось сохранить. {error}"),
        }
    }

    fn export_game(&mut self, verify: bool) {
        let Some(path) = self.proof.as_ref().map_or_else(
            || {
                rfd::FileDialog::new()
                    .set_file_name("Game.exe")
                    .add_filter("Игра для Windows", &["exe"])
                    .save_file()
            },
            |proof| proof.export.clone(),
        ) else {
            return;
        };
        let start_in_2d = self.view == View::Two && self.overhead;
        if verify {
            self.status = match build_check::Job::start(path, self.scene.clone(), self.assets.clone(), start_in_2d, self.build_reference.clone()) {
                Ok(job) => { self.build_check = Some(job); "Собираю EXE и проверяю запуск…".into() }
                Err(error) => format!("Не удалось начать сборку. {error}"),
            };
            return;
        }
        self.status = match game_export::export(&path, &self.scene, &self.assets, start_in_2d) {
            Ok(()) => format!("Игра для Windows собрана. {}", path.display()),
            Err(error) => format!("Не удалось собрать игру. {error}"),
        };
    }

    fn licenses(&mut self, ctx: &egui::Context) {
        egui::Window::new("Лицензии ресурсов")
            .open(&mut self.licenses_open)
            .default_width(640.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(520.0)
                    .show(ui, |ui| {
                        ui.label(include_str!("../demo_assets/README.md"));
                        ui.separator();
                        ui.label(include_str!("../demo_assets/ROCKETBOX-LICENSE.md"));
                        ui.separator();
                        ui.label(include_str!("../demo_assets/AVATAR-STUDIO-LICENSE.txt"));
                    });
            });
    }

    fn export_sprites(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("sprite-atlas.png")
            .add_filter("Изображение PNG", &["png"])
            .save_file()
        else {
            return;
        };
        let scale = sprite::scale_for(&self.scene.parts);
        let art = self.scene.sprite_art;
        let authored = art.drawn_only || art.frames > 1;
        let columns = if authored {
            usize::from(art.frames)
        } else {
            (self.scene.animation.duration * 12.0)
                .ceil()
                .clamp(1.0, 60.0) as usize
        };
        let elapsed = self.scene.animation.duration / columns as f32;
        match sprite::write_atlas(&path, 4, columns, |row, column| {
            let parts = if art.drawn_only {
                Vec::new()
            } else {
                self.scene.sample(column as f32 * elapsed)
            };
            let direction = painted_direction(&self.scene, row as u8, true);
            sprite::rasterize(
                &parts,
                direction,
                &self.scene.sprite_pixels,
                scale,
                0.0,
                Some(if authored { column as u8 } else { 0 }),
            )
        }) {
            Ok(()) => self.status = format!("PNG сохранён: {} кадров × 4 ракурса.", columns),
            Err(error) => self.status = format!("Ошибка экспорта PNG. {error}"),
        }
    }

    fn reset_dialog(&mut self, ctx: &egui::Context) {
        if !self.restart_open {
            return;
        }
        let mut result = None;
        let mut materials_demo = false;
        let mut courtyard_demo = false;
        let mut city_demo = false;
        let mut quarter_demo = false;
        let mut rain_demo = false;
        let mut flagship_demo = false;
        let mut meridian_demo = false;
        let mut river = None;
        let mut map_mode = None;
        let mut dungeon = false;
        egui::Window::new("Новая сцена")
            .collapsible(false)
            .resizable(false)
            .open(&mut self.restart_open)
            .show(ctx, |ui| {
                ui.label("Сохрани текущую сцену, если нужно оставить правки.");
                ui.horizontal_wrapped(|ui| {
                    if ui.button(mill::NAME).clicked() {
                        river = Some(false);
                    }
                    if ui.button(mill::MAP_NAME).clicked() {
                        river = Some(true);
                    }
                    if ui.button(quarter::NAME).clicked() {
                        quarter_demo = true;
                    }
                    if ui.button("Мастерская после дождя").clicked() {
                        rain_demo = true;
                    }
                    if ui.button(flagship::NAME).clicked() {
                        flagship_demo = true;
                    }
                    if ui.button(meridian::NAME).clicked() {
                        meridian_demo = true;
                    }
                    if ui.button("Городская улица").clicked() {
                        city_demo = true;
                    }
                    if ui.button("Каменный двор").clicked() {
                        courtyard_demo = true;
                    }
                    if ui.button("Пустая сцена").clicked() {
                        result = Some(Scene::empty());
                    }
                    if ui.button("2D сверху").clicked() {
                        map_mode = Some(map2d::Mode::Top);
                    }
                    if ui.button("2D сбоку").clicked() {
                        map_mode = Some(map2d::Mode::Side);
                    }
                    if ui.button("Сгенерировать закрытую карту…").clicked() {
                        dungeon = true;
                    }
                    if ui.button("Демо-робот").clicked() {
                        result = Some(Scene::default());
                    }
                    if ui.button("Демо-слизень").clicked() {
                        result = Some(slime_demo());
                    }
                    if ui.button("Демо материалов").clicked() {
                        materials_demo = true;
                    }
                });
            });
        if let Some(top) = river {
            self.restart_open = false;
            self.mill_demo(top);
        } else if dungeon {
            self.restart_open = false;
            self.dungeon_editor.open();
        } else if let Some(mode) = map_mode {
            self.restart_open = false;
            self.set_scene(map2d::scene(mode, false), None, "Открыта пустая 2D-карта.");
        } else if flagship_demo {
            self.restart_open = false;
            self.flagship_demo();
        } else if rain_demo {
            self.restart_open = false;
            self.rain_workshop_demo(rain_workshop::Style::BlueHour);
        } else if meridian_demo {
            self.restart_open = false;
            self.meridian_demo();
        } else if quarter_demo {
            self.restart_open = false;
            self.quarter_demo();
        } else if city_demo {
            self.restart_open = false;
            self.city_demo();
        } else if courtyard_demo {
            self.restart_open = false;
            self.courtyard_demo();
        } else if materials_demo {
            self.restart_open = false;
            self.materials_demo();
        } else if let Some(scene) = result {
            self.restart_open = false;
            self.set_scene(scene, None, "Новая сцена открыта.");
        }
    }
}

fn has_street(scene: &Scene) -> bool {
    scene
        .world
        .models
        .iter()
        .any(|item| item.source == assets::Source::CityStreet)
}

fn painted_direction(scene: &Scene, direction: u8, use_fallback: bool) -> u8 {
    if use_fallback
        && scene.sprite_art.drawn_only
        && !scene
            .sprite_pixels
            .iter()
            .any(|pixel| pixel.direction == direction)
    {
        scene
            .sprite_pixels
            .iter()
            .map(|pixel| pixel.direction)
            .min()
            .unwrap_or(direction)
    } else {
        direction
    }
}

fn asset_sources(scene: &Scene) -> impl Iterator<Item = (&assets::Source, &str)> {
    linked_scenes::all_scenes(scene).flat_map(direct_asset_sources)
}

fn direct_asset_sources(scene: &Scene) -> impl Iterator<Item = (&assets::Source, &str)> {
    scene
        .world
        .models
        .iter()
        .flat_map(assets::Placement::resources)
        .chain(
            scene
                .avatar
                .iter()
                .chain(scene.npcs.iter().map(|npc| &npc.avatar))
                .flat_map(Avatar::resources),
        )
}

fn clear_camera_distance(camera: Camera, colliders: &[world::Collider]) -> f32 {
    let direction = Vec3::new(
        camera.yaw.sin() * camera.pitch.cos(),
        camera.pitch.sin(),
        camera.yaw.cos() * camera.pitch.cos(),
    );
    let mut limit = camera.distance;
    for collider in colliders {
        let mut near = 0.0_f32;
        let mut far = limit;
        for axis in 0..3 {
            let start = camera.target[axis];
            let mut min = collider.min[axis] - 0.11;
            let mut max = collider.max[axis] + 0.11;
            let motion = direction[axis];
            if motion.abs() < 1e-5 {
                if start < min || start > max {
                    far = -1.0;
                    break;
                }
                continue;
            }
            min = (min - start) / motion;
            max = (max - start) / motion;
            near = near.max(min.min(max));
            far = far.min(min.max(max));
        }
        if near <= far {
            limit = limit.min((near - 0.16).max(0.65));
        }
    }
    limit
}

fn slime_demo() -> Scene {
    let mut scene = Scene::empty();
    scene.world = world::World::demo();
    let parts = [
        (
            "Тело",
            [0.0, 0.54, 0.0],
            [1.45, 1.08, 1.11],
            [0.29, 0.78, 0.38],
        ),
        (
            "Левый глаз",
            [-0.26, 0.66, 0.47],
            [0.15, 0.19, 0.12],
            [0.04, 0.10, 0.16],
        ),
        (
            "Правый глаз",
            [0.26, 0.66, 0.47],
            [0.15, 0.19, 0.12],
            [0.04, 0.10, 0.16],
        ),
        (
            "Рот",
            [0.0, 0.47, 0.55],
            [0.22, 0.07, 0.07],
            [0.10, 0.29, 0.19],
        ),
        (
            "Левый румянец",
            [-0.40, 0.52, 0.46],
            [0.15, 0.09, 0.09],
            [0.92, 0.56, 0.35],
        ),
        (
            "Правый румянец",
            [0.40, 0.52, 0.46],
            [0.15, 0.09, 0.09],
            [0.92, 0.56, 0.35],
        ),
    ];
    scene.animation.set_duration(0.8);
    scene.animation.event_time = 0.70;
    for (name, position, size, color) in parts {
        let id = scene.add_part(Shape::Sphere);
        let part = scene.parts.last_mut().expect("demo fits scene limit");
        part.name = name.into();
        part.position = position;
        part.size = size;
        part.color = color;
        scene.animation.keyframe_mut(id, 0.38).position[1] = 0.22;
    }
    scene.effect.color = [0.45, 0.93, 0.62];
    scene
}

fn rgb(value: [f32; 3]) -> Color32 {
    let [red, green, blue] = value.map(|channel| (channel.clamp(0.0, 1.0) * 255.0) as u8);
    Color32::from_rgb(red, green, blue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn customized_pose_bakes_a_sprite_and_retains_brush_edits() {
        let mut studio = Studio::default();
        studio.city_demo();
        let (_, original_token) = studio.avatar_asset().unwrap();
        let appearance = model::HumanStyle {
            jaw_width: 0.6,
            jacket_tint: [0.42, 0.68, 0.9],
            ..Default::default()
        };
        let avatar = studio.scene.avatar.as_mut().unwrap();
        avatar.human_style = Some(appearance);
        avatar.bone_keys.push(avatar::BoneKey {
            bone: "head".into(),
            time: 0.4,
            degrees: [0.0, 20.0, 0.0],
        });
        studio.time = 0.4;
        studio.sync_edited_human();
        assert_ne!(studio.avatar_asset().unwrap().1, original_token);
        assert_eq!(studio.avatar_draw().unwrap().bone_offsets.len(), 1);
        studio.bake_avatar_sprite(false);
        assert!(studio.scene.sprite_art.drawn_only);
        let baked = studio
            .scene
            .sprite_pixels
            .iter()
            .find(|pixel| pixel.baked)
            .unwrap();
        let brush = SpritePixel {
            baked: false,
            color: [255, 64, 20, 255],
            ..baked.clone()
        };
        studio.scene.sprite_pixels.push(brush.clone());
        studio.bake_avatar_sprite(true);
        assert!(studio.scene.sprite_pixels.contains(&brush));
        assert_eq!(studio.scene.sprite_art.frames, scene::MAX_SPRITE_FRAMES);
        let saved = serde_json::to_vec(&studio.scene).unwrap();
        let loaded: Scene = serde_json::from_slice(&saved).unwrap();
        assert_eq!(loaded.avatar.unwrap().human_style, Some(appearance));
        assert!(loaded.sprite_pixels.contains(&brush));
    }

    #[test]
    fn city_loads_locomotion_and_enters_through_its_sidewalk_door() {
        let mut studio = Studio::default();
        studio.city_demo();
        let avatar = studio.scene.avatar.as_ref().expect("city character loaded");
        for motion in [Motion::Idle, Motion::Walk, Motion::Run, Motion::Jump] {
            assert!(avatar.clip(motion).is_some());
        }
        assert!(has_street(&studio.scene) && studio.overhead);
        let door = studio.scene.world.buildings[0].door_bounds();
        for view in [View::Two, View::Three] {
            studio.stop();
            studio.game.z = (door.min[2] + door.max[2]) * 0.5;
            studio.game_running = true;
            studio.camera.yaw = 0.0;
            studio.view = view;
            let ctx = egui::Context::default();
            let mut opened = false;
            for step in 0..222 {
                let mut events = if step == 0 {
                    vec![egui::Event::Key {
                        key: Key::A,
                        physical_key: Some(Key::A),
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }]
                } else {
                    Vec::new()
                };
                if !opened
                    && matches!(
                        studio.scene.world.interaction_near(
                            &studio.interactions,
                            studio.game_position().to_array()
                        ),
                        Some(world::Interaction::Door { opening: true, .. })
                    )
                {
                    opened = true;
                    events.push(egui::Event::Key {
                        key: Key::E,
                        physical_key: Some(Key::E),
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                let _ = ctx.run(
                    egui::RawInput {
                        time: Some(f64::from(step) / 60.0),
                        events,
                        ..Default::default()
                    },
                    |ctx| studio.advance(ctx),
                );
            }
            assert!(opened);
            assert!(studio.scene.world.buildings[0].contains_interior(
                studio.game.x,
                studio.game.height,
                studio.game.z
            ));
        }
    }

    #[test]
    fn city_player_steps_onto_the_visible_curb_and_uses_an_airborne_pose() {
        let mut studio = Studio::default();
        studio.city_demo();
        studio.game_running = true;
        studio.game.x = 3.15;
        studio.game.z = 10.2;
        let colliders = studio.colliders();
        for _ in 0..32 {
            studio
                .game
                .update(1.0 / 60.0, [0.55, 0.0], false, false, &colliders);
        }
        assert!(studio.game.x > 3.8);
        assert!((0.05..0.12).contains(&studio.game.height));
        assert!(studio.game.grounded);

        let ctx = egui::Context::default();
        for frame in 0..7 {
            let events = if frame == 0 {
                vec![egui::Event::Key {
                    key: Key::Space,
                    physical_key: Some(Key::Space),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }]
            } else {
                Vec::new()
            };
            let _ = ctx.run(
                egui::RawInput {
                    predicted_dt: 0.05,
                    events,
                    ..Default::default()
                },
                |ctx| studio.advance(ctx),
            );
        }
        assert_eq!(studio.avatar_motion, Motion::Jump);
        assert!(studio.game.height > 0.8);
        let draw = studio.avatar_draw().unwrap();
        assert!(draw.bone_offsets.iter().any(|offset| {
            offset.bone.ends_with("LeftLeg")
                && offset.degrees.iter().any(|value| value.abs() > 12.0)
        }));
    }

    #[test]
    fn maya_runs_her_seat_clip_and_returns_to_the_stand_anchor() {
        let mut studio = Studio::default();
        let source = assets::Source::CityWoman;
        studio.assets.ensure(&source, None).unwrap();
        studio.assign_avatar(source, "Майя".into());
        let seat = world::Seat {
            id: 1,
            position: [0.0, 0.48, 0.0],
            stand: [0.0, 0.0, 0.67],
            yaw: 0.0,
        };
        studio.seating = Some(Seating {
            seat,
            arrival: Vec3::new(0.0, 0.0, 0.67),
            original_heading: 30.0,
            amount: 0.0,
            leaving: false,
        });
        for _ in 0..32 {
            assert!(studio.advance_seating(0.05));
        }
        let model = studio.avatar_draw().expect("native seated character");
        assert_eq!(model.clip, studio.scene.avatar.as_ref().unwrap().sit);
        assert!((model.elapsed - 3.0).abs() < 0.01);
        studio.seating.as_mut().unwrap().leaving = true;
        for _ in 0..32 {
            studio.advance_seating(0.05);
        }
        assert!(studio.seating.is_none());
        assert!(studio.game_position().distance(Vec3::new(0.0, 0.0, 0.67)) < 0.001);
    }

    #[test]
    fn procedural_seat_on_tall_man_keeps_shoes_above_the_floor() {
        let mut studio = Studio::default();
        let source = assets::Source::CityHuman;
        studio.assets.ensure(&source, None).unwrap();
        studio.assign_avatar(source, "Сидящий герой".into());
        studio.seating = Some(Seating {
            seat: world::Seat {
                id: 1,
                position: [0.0, 0.48, 0.0],
                stand: [0.0, 0.0, 0.60],
                yaw: 0.0,
            },
            arrival: Vec3::ZERO,
            original_heading: 0.0,
            amount: 1.0,
            leaving: false,
        });
        let draw = studio.avatar_draw().unwrap();
        let posed = draw
            .model
            .pose_with_overrides(draw.clip, draw.elapsed, &draw.bone_offsets)
            .unwrap();
        let sole_height = posed
            .iter()
            .flatten()
            .map(|point| point.position[1] * draw.scale[1] + draw.position[1])
            .fold(f32::INFINITY, f32::min);
        assert!((-0.012..0.20).contains(&sole_height), "{sole_height}");
    }

    #[test]
    fn courtyard_loads_textured_surfaces_and_four_motion_clips() {
        let mut studio = Studio::default();
        studio.courtyard_demo();
        let avatar = studio.scene.avatar.as_ref().expect("demo knight loaded");
        for motion in [Motion::Idle, Motion::Walk, Motion::Run, Motion::Jump] {
            let source = studio.assets.get(&avatar.source).unwrap();
            let index = avatar.clip(motion).expect("named locomotion clip loaded");
            assert!(source.model.animations[index].duration > 0.0);
            studio.avatar_motion = motion;
            assert_eq!(studio.avatar_draw().unwrap().clip, Some(index));
        }
        for source in [
            assets::Source::CourtyardGround,
            assets::Source::StoneBuilding,
        ] {
            let asset = studio.assets.get(&source).expect("surface GLB loaded");
            assert!(
                asset
                    .model
                    .primitives
                    .iter()
                    .any(|primitive| primitive.material.base_color_texture.is_some())
            );
        }
        assert!(studio.colliders().len() >= 3);
    }

    #[test]
    fn forward_backward_and_sprint_reach_the_3d_controller() {
        fn move_once(key: Key, shift: bool) -> f32 {
            let mut studio = Studio {
                scene: Scene::empty(),
                game_running: true,
                ..Studio::default()
            };
            studio.camera.yaw = 0.0;
            let modifiers = if shift {
                egui::Modifiers::SHIFT
            } else {
                egui::Modifiers::NONE
            };
            let ctx = egui::Context::default();
            let _ = ctx.run(
                egui::RawInput {
                    modifiers,
                    events: vec![egui::Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed: true,
                        repeat: false,
                        modifiers,
                    }],
                    ..Default::default()
                },
                |ctx| studio.advance(ctx),
            );
            studio.game.z
        }

        let forward = move_once(Key::W, false);
        let backward = move_once(Key::S, false);
        let sprint = move_once(Key::W, true);
        assert!(forward < 0.0 && backward > 0.0 && sprint < forward);
    }

    #[test]
    fn gameplay_space_jumps_and_triggers_the_landing_effect() {
        let mut studio = Studio {
            game_running: true,
            ..Studio::default()
        };
        let ctx = egui::Context::default();
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: Key::Space,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| studio.advance(ctx),
        );
        assert!(!studio.game.grounded);
        let mut landed = false;
        for step in 1..90 {
            let _ = ctx.run(
                egui::RawInput {
                    time: Some(f64::from(step) / 60.0),
                    ..Default::default()
                },
                |ctx| studio.advance(ctx),
            );
            if studio.game.grounded && studio.effect_age == Some(0.0) {
                landed = true;
                assert!(!studio.sparks(studio.effect_strength()).is_empty());
                break;
            }
        }
        assert!(landed);
    }
}
