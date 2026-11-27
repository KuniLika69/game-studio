use std::{fs, path::Path};

use eframe::egui::{Key, Modifiers};
use serde_json::json;

use super::{
    Driver, FPS, SIZE, Sink, games23_proof, io_error, proof_input, quarter_proof, rain_proof,
    staged_proof::{number, panel_click, reload},
};
use crate::{
    Studio, View,
    assets::Source,
    configure_context,
    editor_workspace::Workspace,
    face_timeline::Lane,
    game_export::GamePackage,
    npcs::{FIRST_NPC_ID, Npc, Waypoint},
    offscreen::Offscreen,
    proof_video::Video,
    rain_workshop,
    story::{Speaker, Story},
    studio_welcome::Page,
    viewport3d,
};

const NAME: &str = "Мира";
const QUAY_LINE: &str = "Фонарь у входа ещё горит. Занесу инструмент в мастерскую. Сначала осторожно пройду через лужу, потом отмечусь у верстака.";
const DELTA_LINE: &str =
    "Слышишь, как свистит ветер? Значит, мы близко. Сначала проверю медный пост, потом маяк.";

pub(super) fn author(
    output: &Path,
    releases: &Path,
    model: &Path,
    expedition: &Path,
) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    fs::create_dir_all(releases).map_err(io_error)?;
    let assets = model.parent().ok_or("Не указан каталог ресурса.")?;
    let named_model = output.join("Мира.glb");
    fs::copy(model, &named_model).map_err(io_error)?;
    let character = output.join("Mira.character.json");

    let seed = output.join("workshop-base.robot.json");
    let mut library = crate::assets::Library::default();
    let workshop = rain_workshop::create(&mut library, rain_workshop::Style::BlueHour)?;
    library.save_files(crate::asset_sources(&workshop), &seed)?;
    workshop.save(&seed)?;

    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut video = Video::start(&output.join("make-mira.mp4"), SIZE[0], SIZE[1], FPS)?;
    {
        let mut sink = Sink::Video(&mut video);
        let mut app = Studio {
            welcome: Some(Page::Home),
            workspace: Workspace::Create,
            ..Studio::default()
        };
        app.desk.recovery_root = Some(output.join("recovery"));
        let mut d = Driver::new(app, &mut display, &mut sink, None);
        d.settle(9)?;
        screenshot(&d, &output.join("welcome.png"))?;
        d.hold(24)?;
        d.click("Создать проект")?;
        d.settle(8)?;
        d.click("3D-сцена")?;
        d.app.proof.as_mut().unwrap().import_glb = Some(named_model);
        d.click("3D-персонаж…")?;
        require_mira(&d.app)?;
        number(&mut d, "Рост ", "1.73")?;
        d.click("Клипы, дыхание и волосы")?;
        if !d
            .app
            .scene
            .avatar
            .as_ref()
            .is_some_and(|avatar| avatar.actor_tuning.match_stride)
        {
            d.click("Согласовать скорость с длиной шага")?;
        }
        if !d
            .app
            .scene
            .avatar
            .as_ref()
            .is_some_and(|avatar| avatar.actor_tuning.match_stride)
        {
            return Err("Согласование длины шага Миры не включилось.".into());
        }
        d.click("Дыхание и волосы")?;
        number(&mut d, "Жёсткость волос ", "0.86")?;
        number(&mut d, "Затухание волос ", "0.94")?;
        d.click("Разработка")?;
        if d.point("Сохранить персонажа в библиотеку…", 0).is_err() {
            d.click("Библиотека персонажей")?;
            d.settle(10)?;
        }
        d.app.proof.as_mut().unwrap().character_file = Some(character.clone());
        d.click("Сохранить персонажа в библиотеку…")?;
        if !character.is_file() {
            return Err(d.app.status.clone());
        }
        screenshot(&d, &output.join("character-editor.png"))?;
        d.hold(26)?;
    }

    for (kind, input, game) in [
        ("quay", seed.as_path(), "MiraWorkshop.exe"),
        ("delta", expedition, "MiraExpedition.exe"),
    ] {
        let mut sink = Sink::Video(&mut video);
        let project = output.join(format!("mira-{kind}.robot.json"));
        let exported = releases.join(game);
        let mut d = Driver::new(
            Studio::default(),
            &mut display,
            &mut sink,
            Some((input, &exported)),
        );
        d.app.desk.recovery_root = Some(output.join(format!("recovery-{kind}")));
        d.settle(6)?;
        panel_click(&mut d, "Открыть…")?;
        d.click("Персонаж")?;
        if d.point("Разместить из библиотеки…", 0).is_err() {
            d.click("Библиотека персонажей")?;
            d.settle(10)?;
        }
        d.app.proof.as_mut().unwrap().character_file = Some(character.clone());
        d.click("Разместить из библиотеки…")?;
        require_mira(&d.app)?;
        d.click("Звук")?;
        d.app.proof.as_mut().unwrap().wav_file = Some(assets.join(format!("mira-{kind}.wav")));
        d.click("Импорт WAV…")?;
        author_story(&mut d.app, assets, kind)?;
        d.app.proof.as_mut().unwrap().scene = Some(project);
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        require_mira(&d.app)?;
        panel_click(&mut d, "Собрать игру для Windows")?;
        if !exported.is_file() {
            return Err(d.app.status.clone());
        }
        d.hold(22)?;
    }
    video.finish()
}

fn author_story(app: &mut Studio, assets: &Path, kind: &str) -> Result<(), String> {
    if kind == "quay" {
        let recorded = fs::read(assets.join("mira-local-walk.wav")).map_err(io_error)?;
        app.scene.audio.install_step_recording(&recorded, crate::audio::Surface::Stone)?;
    }
    let audio_name = format!("mira-{kind}");
    let voice = app
        .scene
        .audio
        .sounds
        .iter()
        .find(|sound| sound.name == audio_name)
        .ok_or_else(|| format!("Запись {audio_name} не импортирована. {}", app.status))?
        .id;
    let bytes = fs::read(assets.join(format!("mira-{kind}.face.json"))).map_err(io_error)?;
    let lane: Lane = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    lane.validate()?;

    let (npc_id, text) = if kind == "quay" {
        let door = app
            .scene
            .world
            .buildings
            .first_mut()
            .and_then(|room| room.door.as_mut())
            .ok_or("Нет входной двери.")?;
        door.hand_contact = false;
        (rain_workshop::NPC_ID, QUAY_LINE)
    } else {
        let terrain = app.assets.import_bytes(
            include_bytes!("../demo_assets/expedition/delta.glb").to_vec(),
        )?;
        let placement = app.scene.world.models.first_mut().ok_or("Нет модели дельты.")?;
        placement.source = terrain;
        placement.label = "Дельта · берега, тропа и лес".into();
        placement.material.roughness = None;
        games23_proof::projects::detail_expedition_props(&mut app.scene, &mut app.assets)?;
        app.assets.ensure(&Source::Ryan, None)?;
        let guard = crate::avatar::Avatar::new(
            Source::Ryan,
            "Смотритель".into(),
            &app.assets.get(&Source::Ryan).unwrap().model,
        );
        app.scene.npcs.push(Npc::new(
            FIRST_NPC_ID,
            guard,
            vec![Waypoint::at([0.78, 0.0, -1.05])],
        ));
        let mut talk = Story::demo(FIRST_NPC_ID, [0.0; 2]).conversations.remove(0);
        talk.name = "Переправа".into();
        talk.nodes.truncate(1);
        talk.nodes[0].choices.clear();
        app.scene.story.conversations.push(talk);
        app.scene.play_settings.camera.distance = Some(4.0);
        app.scene.audio.footsteps.gait_contacts = true;
        (FIRST_NPC_ID, DELTA_LINE)
    };
    let conversation = app
        .scene
        .story
        .conversations
        .iter_mut()
        .find(|entry| entry.npc_id == npc_id)
        .ok_or("Нет разговора.")?;
    let node = conversation
        .nodes
        .iter_mut()
        .find(|node| node.id == conversation.entry)
        .ok_or("Нет начальной реплики.")?;
    let previous = node.voice.replace(voice);
    node.speaker = Speaker::Player;
    node.face = Some(lane);
    node.text = text.into();
    if let Some(old) = previous {
        app.scene.audio.remove(old);
    }
    crate::world_palette::enrich_sample(&mut app.scene, &mut app.assets, kind == "quay")?;
    app.scene.validate()?;
    app.status = "Реплика Миры и лицевая дорожка привязаны к диалогу.".into();
    app.changed();
    Ok(())
}

fn require_mira(app: &Studio) -> Result<(), String> {
    let avatar = app
        .scene
        .avatar
        .as_ref()
        .ok_or_else(|| app.status.clone())?;
    let (model, _) = app.avatar_asset().ok_or("Модель Миры отсутствует.")?;
    if avatar.name != NAME
        || avatar.walk.is_none()
        || avatar.run.is_none()
        || !model.has_secondary_pose()
        || model.morph_names().len() < 8
    {
        return Err(format!(
            "Пакет Миры неполный. Имя {}, клипы {}, формы лица {}, вторичное движение {}. {}",
            avatar.name,
            model.animations.len(),
            model.morph_names().len(),
            model.has_secondary_pose(),
            app.status
        ));
    }
    Ok(())
}

pub(super) fn player(package: GamePackage, kind: &str, output: &Path) -> Result<(), String> {
    let (kind, detailed) = kind
        .strip_suffix("-detail")
        .map_or((kind, false), |game| (game, true));
    if !matches!(kind, "quay" | "delta") {
        return Err("Укажи quay, delta, quay-detail или delta-detail.".into());
    }
    fs::create_dir_all(output).map_err(io_error)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut app = Studio::from_package(package, display.context());
    require_mira(&app)?;
    app.view = View::Three;
    app.set_audio_recording(output.join("game.wav"))?;
    let mut video = Video::start(&output.join("game-silent.mp4"), SIZE[0], SIZE[1], FPS)?;
    let mut sink = Sink::Video(&mut video);
    let mut driver = Driver::new(app, &mut display, &mut sink, None);
    if detailed {
        locomotion_detail(&mut driver, kind, output)?;
        driver.app.write_recorded_audio()?;
        return video.finish();
    }
    driver.settle(45)?;
    let face = match kind {
        "quay" => {
            let mut summary = None;
            rain_proof::play_with_intro(&mut driver, |d| {
                summary = Some(portrait(d, output)?);
                Ok(())
            })?;
            summary.ok_or("Не записан портрет Миры.")?
        }
        "delta" => {
            quarter_proof::walk(&mut driver, [0.14, -0.30])?;
            driver.key(Key::E)?;
            let face = portrait(&mut driver, output)?;
            driver.key(Key::Escape)?;
            games23_proof::play_delta(&mut driver)?;
            face
        }
        _ => return Err("Неизвестная сцена Миры.".into()),
    };
    driver.settle(35)?;
    let stats = driver
        .app
        .audio_recording_stats()
        .ok_or("Микс игры отсутствует.")?;
    if stats.dialogue_player == 0 || stats.footsteps.iter().sum::<usize>() == 0 {
        return Err(format!("Голос или шаги отсутствуют в миксе. {stats:?}"));
    }
    driver.app.write_recorded_audio()?;
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "character": NAME, "game": kind, "pid": std::process::id(),
            "finished": true, "gait_and_voice_in_exported_game": true,
            "face": face, "sound": stats, "computer_input": true,
        }))
        .map_err(|error| error.to_string())?,
    )
    .map_err(io_error)?;
    video.finish()
}

fn portrait(d: &mut Driver<'_, '_>, output: &Path) -> Result<serde_json::Value, String> {
    let lane = d
        .app
        .story_session
        .current(&d.app.scene.story)
        .and_then(|(_, node)| node.face.as_ref())
        .ok_or("Диалог Миры не открылся.")?;
    let ticks = usize::from(lane.duration_ms) * 60 / 1000;
    let closeup_start_tick = d.phase;
    d.key(Key::F8)?;
    if d.app.dialogue_portrait().is_none() {
        return Err("Крупный план реплики не открылся.".into());
    }
    let mut jaw: f32 = 0.0;
    let mut blink: f32 = 0.0;
    let mut taken = false;
    for tick in 0..ticks {
        d.frame(Vec::new())?;
        let (model, _) = d.app.avatar_asset().ok_or("Персонаж пропал.")?;
        for morph in d.app.player_dialogue_face(model) {
            match morph.target.as_str() {
                "jawOpen" => jaw = jaw.max(morph.weight),
                "eyeBlinkLeft" => blink = blink.max(morph.weight),
                _ => {}
            }
        }
        if !taken && tick >= 35 && jaw > 0.18 {
            screenshot(d, &output.join("mira-portrait.png"))?;
            taken = true;
        }
    }
    d.key(Key::F8)?;
    if !taken || jaw <= 0.18 || blink <= 0.4 {
        return Err(format!(
            "Игровая мимика не достигла контрольных значений. Рот {jaw}, веки {blink}."
        ));
    }
    Ok(json!({
        "portrait": true, "max_jaw": jaw, "max_blink": blink, "speech_ticks": ticks,
        "closeup_start_tick": closeup_start_tick,
    }))
}

fn locomotion_detail(d: &mut Driver<'_, '_>, game: &str, output: &Path) -> Result<(), String> {
    // Keep the normal player, scene and movement controller. Only frame the
    // actor more tightly in this inspection than in the normal route.
    d.app.camera.yaw = 0.0;
    d.app.camera.pitch = 0.15;
    d.app.camera.distance = 3.15;
    d.app.scene.play_settings.camera.distance = Some(3.15);
    d.settle(36)?;
    let full = key_sequence(
        d,
        &[
            ("walk", Some(Key::W), false, 65),
            ("quarter-turn", Some(Key::A), false, 44),
            ("half-turn", Some(Key::D), false, 46),
            ("stop", None, false, 47),
        ],
        output,
    )?;
    let delta = |a, b| {
        let signed: f32 = a - b + 180.0;
        (signed.rem_euclid(360.0) - 180.0).abs()
    };
    let heading = |index: usize| full[index].1;
    let quarter = delta(heading(0), heading(1));
    let half = delta(heading(1), heading(2));
    if (quarter - 90.0).abs() > 20.0 || (half - 180.0).abs() > 20.0 {
        return Err(format!(
            "Маршрут не выполнил оба поворота. Измерены {quarter:.1}° и {half:.1}°."
        ));
    }
    if d.app.game.horizontal_speed() >= 0.14 {
        return Err("Персонаж продолжает движение после остановки.".into());
    }

    d.app.camera.distance = 1.58;
    d.app.scene.play_settings.camera.distance = Some(1.58);
    d.app.camera.pitch = 0.08;
    d.settle(24)?;
    let close = key_sequence(
        d,
        &[
            ("hair-turn", Some(Key::A), false, 37),
            ("hair-reversal", Some(Key::D), true, 33),
            ("hair-settle", None, false, 76),
        ],
        output,
    )?;
    let stages = full
        .into_iter()
        .chain(close)
        .map(|(data, _)| data)
        .collect::<Vec<_>>();
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "character": NAME, "game": game, "pid": std::process::id(),
            "kind": "locomotion and hairstyle", "computer_input": true,
            "quarter_turn_deg": quarter, "half_turn_deg": half, "stages": stages,
            "timebase_hz": 60,
        }))
        .map_err(|error| error.to_string())?,
    )
    .map_err(io_error)
}

fn key_sequence(
    d: &mut Driver<'_, '_>,
    stages: &[(&str, Option<Key>, bool, usize)],
    output: &Path,
) -> Result<Vec<(serde_json::Value, f32)>, String> {
    let mut old_key = None;
    let mut results = Vec::with_capacity(stages.len());
    for &(stage, new_key, sprint, frames) in stages {
        let modifiers = if sprint {
            Modifiers::SHIFT
        } else {
            Modifiers::NONE
        };
        let mut events = Vec::new();
        if let Some(key) = old_key {
            events.push(proof_input::key_event(key, Modifiers::NONE, false));
        }
        if let Some(key) = new_key {
            events.push(proof_input::key_event(key, modifiers, true));
        }
        d.input.set_modifiers(modifiers);
        let start = d.phase;
        d.frame(events)?;
        let mut peak_speed: f32 = 0.0;
        let mut peak_sway: f32 = 0.0;
        for frame in 0..frames {
            d.frame(Vec::new())?;
            peak_speed = peak_speed.max(d.app.game.horizontal_speed());
            let hair = d.app.player_secondary.pose(d.app.live_seconds);
            peak_sway = peak_sway.max(hair.hair_sway[0].hypot(hair.hair_sway[1]));
            if frame == 12 {
                screenshot(d, &output.join(format!("{stage}.png")))?;
            }
        }
        let end_heading = d.app.avatar_heading;
        results.push((
            json!({
                "name": stage, "start_tick": start, "end_tick": d.phase,
                "heading_deg": end_heading, "peak_horizontal_speed": peak_speed,
                "peak_hair_drive": peak_sway,
            }),
            end_heading,
        ));
        old_key = new_key;
    }
    if let Some(key) = old_key {
        d.input.set_modifiers(Modifiers::NONE);
        d.frame(vec![proof_input::key_event(key, Modifiers::NONE, false)])?;
    }
    Ok(results)
}

fn screenshot(d: &Driver<'_, '_>, path: &Path) -> Result<(), String> {
    image::save_buffer(path, &d.last, SIZE[0], SIZE[1], image::ColorType::Rgb8)
        .map_err(|error| error.to_string())
}
