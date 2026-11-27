use std::{fs, path::Path};

use eframe::egui::{self, Key};

use super::{Driver, FPS, SIZE, Sink, io_error, quarter_proof,
    staged_proof::{move_and_play, number, panel_click, play_linden, reload}, top_proof};
use crate::{Studio, View, configure_context, offscreen::Offscreen, proof_input, proof_video::Video, viewport3d};

pub(super) fn record(output: &Path, releases: &Path, source: &Path, donor: &Path, wav: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    fs::create_dir_all(releases).map_err(io_error)?;
    let staging = output.join("export-stage");
    fs::create_dir_all(&staging).map_err(io_error)?;
    let scene = output.join("linden-cast.robot.json");
    let staged = staging.join("LindenCast.exe");
    let game = releases.join("LindenCast.exe");
    let video_path = output.join("stage-020.mp4");
    for path in [&scene, &staged, &game, &video_path] {
        if path.exists() { return Err(format!("Путь занят. {}", path.display())); }
    }
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut video = Video::start(&video_path, SIZE[0], SIZE[1], FPS)?;
    {
        let mut sink = Sink::Video(&mut video);
        let mut app = Studio::default();
        app.desk.recovery_root = Some(output.join("recovery-test"));
        let mut d = Driver::new(app, &mut display, &mut sink, Some((source, &staged)));
        d.settle(6)?;
        panel_click(&mut d, "Открыть…")?;
        if d.app.scene.effects.presets.is_empty() { return Err("Сцена 0.19 не открылась.".into()); }
        d.app.set_audio_recording(output.join("linden-stereo.wav"))?;
        author_face(&mut d, output)?;
        author_clip(&mut d, donor)?;
        author_sprite(&mut d, output)?;
        author_audio(&mut d, wav)?;
        d.app.proof.as_mut().unwrap().scene = Some(scene.clone());
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        quarter_proof::fresh(&mut d, View::Three)?;
        play_cast(&mut d, true)?;
        panel_click(&mut d, "■ Стоп")?;
        let stats = d.app.audio_recording_stats().ok_or("Нет статистики звуковой дорожки.")?;
        if stats.markers == 0 || stats.footsteps.iter().sum::<usize>() == 0 || stats.frames < 4_800 {
            return Err(format!("Не записан звук метки или шаги игрового маршрута. {stats:?}"));
        }
        d.app.write_recorded_audio()?;
        fs::write(output.join("audio-capture.json"), serde_json::to_vec_pretty(&stats).map_err(|e| e.to_string())?).map_err(io_error)?;
        panel_click(&mut d, "Собрать игру для Windows")?;
    }
    move_and_play(&mut video, &mut display, &staged, &game, "cast", &output.join("timing-cast.json"))?;
    video.finish()?;
    eprintln!("0.20: клип, лицевая дорожка, маркер и сохранение дорисовки прошли в редакторе. Квест и формы лица проверены в отдельном EXE.");
    Ok(())
}

fn author_face(d: &mut Driver<'_, '_>, output: &Path) -> Result<(), String> {
    d.click("NPC")?;
    d.click("Марина")?;
    d.click("Диалог")?;
    if d.point("Лицевая анимация", 0).is_err() { top_proof::scroll_panel(d, -250.0)?; }
    d.click("Лицевая анимация")?;
    if d.point("Создать лицевую дорожку", 0).is_err() { top_proof::scroll_panel(d, -220.0)?; }
    d.click("Создать лицевую дорожку")?;
    d.click("Все каналы лица")?;
    d.click_n("Моргание", usize::MAX)?;
    face_key(d, None, Some("0"))?;
    face_key(d, Some(("0.000 с", "0.180")), Some("0.9"))?;
    face_key(d, Some(("0.180 с", "0.340")), Some("0"))?;
    d.click("Моргание")?;
    d.click_n("Рот", usize::MAX)?;
    if d.point("jawOpen", 0).is_err() {
        d.click_n("jawForward", usize::MAX)?;
        d.click_n("jawOpen", usize::MAX)?;
    }
    face_key(d, Some(("0.340 с", "0")), Some("0"))?;
    face_key(d, Some(("0.000 с", "0.100")), Some("0.7"))?;
    face_key(d, Some(("0.100 с", "0.360")), Some("0"))?;
    let npc = d.app.selected_npc.ok_or("NPC не выбран.")?;
    let node = d.app.scene.story.conversation(npc).ok_or("Нет диалога Марины.")?.entry;
    let lane = d.app.scene.story.facial_lane(npc, node).ok_or("Дорожка не сохранилась.")?;
    if lane.channel("eyeBlinkLeft").is_none_or(|track| track.keys.len() != 3)
        || lane.channel("jawOpen").is_none_or(|track| track.keys.len() != 3) {
        return Err(format!("Неполные ключи лица. {lane:?}"));
    }
    if d.point("К лицу NPC", 0).is_err() { top_proof::scroll_panel(d, 900.0)?; }
    d.click("К лицу NPC")?;
    if d.point("Сыграть лицевую дорожку", 0).is_err() { top_proof::scroll_panel(d, 165.0)?; }
    d.click("Сыграть лицевую дорожку")?;
    d.settle(14)?;
    png(d, &output.join("marina-face.png"))?;
    d.hold(22)
}

fn face_key(d: &mut Driver<'_, '_>, at: Option<(&str, &str)>, weight: Option<&str>) -> Result<(), String> {
    if let Some((current, value)) = at {
        d.edit(current, 0, value)?;
        d.key(Key::Enter)?;
    }
    if d.point("+ Ключ лица", 0).is_err() { top_proof::scroll_panel(d, -190.0)?; }
    d.click("+ Ключ лица")?;
    if let Some(weight) = weight {
        if !d.display.last_output().is_some_and(|out| proof_input::find_label_containing(out,"Сила ",0).is_some()) {
            top_proof::scroll_panel(d, -130.0)?;
        }
        number(d, "Сила", weight)?;
    }
    Ok(())
}

fn author_clip(d: &mut Driver<'_, '_>, donor: &Path) -> Result<(), String> {
    d.click("Персонаж")?;
    if d.point("Импорт и предпросмотр клипов GLB", 0).is_err() { top_proof::scroll_panel(d, -280.0)?; }
    d.click("Импорт и предпросмотр клипов GLB")?;
    d.app.proof.as_mut().unwrap().import_glb = Some(donor.to_owned());
    d.click("Выбрать GLB с анимацией…")?;
    d.click("Добавить совместимый клип")?;
    let avatar = d.app.scene.avatar.as_ref().ok_or("Герой отсутствует.")?;
    if avatar.imported_clips.len() != 1 { return Err(d.app.status.clone()); }
    d.expect_hud("Предпросмотр · Wave (2)")?;
    d.settle(50)?;
    d.hold(22)
}

fn author_sprite(d: &mut Driver<'_, '_>, output: &Path) -> Result<(), String> {
    d.click("Рисовать спрайт")?;
    d.click("Wave (2)")?;
    d.click_n("Ходьба", usize::MAX)?;
    d.click("Клип в 8 кадров")?;
    let before = d.app.scene.sprite_pixels.iter().filter(|pixel| pixel.baked).cloned().collect::<Vec<_>>();
    let canvas = d.app.proof.as_ref().and_then(|env| env.sprite_canvas).ok_or("Не найден холст.")?;
    let point = canvas.left_top() + egui::vec2(canvas.width() * 0.44, canvas.height() * 0.34);
    d.frame(proof_input::click(point))?;
    d.settle(5)?;
    let brush = d.app.scene.sprite_pixels.iter().find(|pixel| !pixel.baked).ok_or("Нет ручного пикселя.")?.clone();
    d.click("Ходьба")?;
    d.click_n("Wave (2)", usize::MAX)?;
    d.click("Клип в 8 кадров")?;
    if !d.app.scene.sprite_pixels.contains(&brush) { return Err("Повторное запекание потеряло ручную дорисовку.".into()); }
    let after = d.app.scene.sprite_pixels.iter().filter(|pixel| pixel.baked).cloned().collect::<Vec<_>>();
    if before.is_empty() || before == after {
        return Err("Смена клипа на Wave не обновила запечённые кадры.".into());
    }
    png(d, &output.join("sprite-rebake.png"))?;
    d.click("3D")?;
    Ok(())
}

fn author_audio(d: &mut Driver<'_, '_>, wav: &Path) -> Result<(), String> {
    d.click("Звук")?;
    d.app.proof.as_mut().unwrap().wav_file = Some(wav.to_owned());
    d.click("Импорт WAV…")?;
    let marker = d.display.last_output().and_then(|out| proof_input::labels(out).into_iter()
        .find(|label| label.text.starts_with("Звук по метке анимации"))).ok_or("Не показана связь с меткой анимации.")?.text;
    d.click(&marker)?;
    if d.app.scene.audio.marker.is_none() { return Err("Звук не привязан к анимации.".into()); }
    if d.point("Звуки шагов", 0).is_err() { top_proof::scroll_panel(d, -240.0)?; }
    if !d.app.scene.audio.footsteps.enabled { d.click("Звуки шагов")?; }
    d.click("Воспроизвести")?;
    d.settle(78)?;
    if d.point("Пауза", 0).is_ok() { d.click("Пауза")?; }
    Ok(())
}

pub(super) fn play_cast(d: &mut Driver<'_, '_>, preview: bool) -> Result<(), String> {
    let avatar = d.app.scene.avatar.as_ref().ok_or("Герой отсутствует.")?;
    if avatar.imported_clips.is_empty() || d.app.scene.audio.marker.is_none() {
        return Err("После загрузки пропал звуковой маркер или донор клипа.".into());
    }
    let npc = d.app.scene.npcs.iter().find(|actor| actor.avatar.name == "Марина").ok_or("Нет Марины.")?.id;
    d.key(Key::E)?;
    d.settle(10)?;
    let model = &d.app.npc_assets.get(&npc).ok_or("Модель NPC отсутствует.")?.model;
    let weights = d.app.dialogue_face_weights(npc, model);
    if !weights.iter().any(|weight| weight.target == "eyeBlinkLeft" && weight.weight > 0.12)
        || !weights.iter().any(|weight| weight.target == "jawOpen" && weight.weight > 0.12) {
        return Err(format!("Во время реплики не применились обе формы лица. {weights:?}"));
    }
    d.hold(30)?;
    d.key(Key::Escape)?;
    play_linden(d, preview)
}

fn png(d: &Driver<'_, '_>, path: &Path) -> Result<(), String> {
    image::save_buffer(path, &d.last, SIZE[0], SIZE[1], image::ColorType::Rgb8).map_err(|e| e.to_string())
}
