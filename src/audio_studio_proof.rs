use std::{fs, path::Path};

use eframe::egui::{Key, Modifiers};

use super::{
    Driver, SIZE, Sink, io_error, proof_input,
    staged_proof::{number, panel_click, reload},
};
use crate::{
    Studio,
    audio::{Clip, OUTPUT_RATE, Surface},
    configure_context,
    game_export::GamePackage,
    offscreen::Offscreen,
};

pub(super) fn author(output: &Path, wav: &Path) -> Result<(), String> {
    let original = fs::read(wav).map_err(io_error)?;
    fs::create_dir_all(output).map_err(io_error)?;
    let project = output.join("sound-courtyard.robot.json");
    let game = output.join("SoundCourtyard.exe");
    let mix = output.join("editor-stereo.wav");
    let mut renderer = Offscreen::new(SIZE, crate::viewport3d::VIEWPORT_MSAA)?;
    configure_context(renderer.context());
    let mut app = Studio::default();
    app.desk.recovery_root = Some(output.join("recovery-test"));
    let mut sink = Sink::None;
    let mut d = Driver::new(app, &mut renderer, &mut sink, Some((&project, &game)));
    d.settle(6)?;
    d.click("Новый…")?;
    d.click("2D сверху")?;
    d.click("Правила")?;
    d.click("Зоны")?;
    d.click("+ Зона")?;
    number(&mut d, "X ", "6.6")?;
    number(&mut d, "Z ", "10.5")?;
    d.click("Эффекты")?;
    d.click("Добавить эффект")?;
    d.click_n("Пыль", usize::MAX)?;
    let effect = crate::proof_input::labels(d.display.last_output().ok_or("Пустой интерфейс.")?)
        .into_iter()
        .find(|label| label.text.starts_with("Запуск по метке анимации"))
        .ok_or("В редакторе эффекта нет привязки к метке.")?
        .text;
    d.click(&effect)?;

    d.click("Звук")?;
    d.app.proof.as_mut().unwrap().wav_file = Some(wav.to_owned());
    d.click("Импорт WAV…")?;
    number(&mut d, "Метка ", "0.40")?;
    d.click("Звук по метке анимации · 0.40 с")?;
    number(&mut d, "X ", "2.5")?;
    d.click("Звуки шагов")?;
    d.click("+ Материал зоны")?;
    let audio = &d.app.scene.audio;
    if audio.marker.is_none_or(|cue| cue.offset[0] != 2.5)
        || audio
            .footsteps
            .zones
            .first()
            .is_none_or(|zone| zone.surface != Surface::Wood)
        || audio
            .sounds
            .first()
            .is_none_or(|sound| sound.wav_bytes() != original)
    {
        return Err(format!("Параметры звука не применены. {}", d.app.status));
    }

    let preview = output.join("marker-preview.wav");
    d.app.set_audio_recording(preview.clone())?;
    d.click("Воспроизвести анимацию")?;
    d.settle(53)?;
    if d.app
        .audio_recording_stats()
        .is_none_or(|stats| stats.markers != 1)
        || !d.app.effects_runtime.is_active()
    {
        return Err("Предпросмотр метки не запустил звук и эффект в одном проходе.".into());
    }
    d.app.write_recorded_audio()?;
    let rms = inspect(&preview)?;
    if rms[1] <= rms[0] * 2.0 {
        return Err(format!(
            "Источник справа не даёт ожидаемую панораму. RMS {rms:?}."
        ));
    }
    d.click("Стоп анимации")?;

    panel_click(&mut d, "Сохранить как…")?;
    reload(&mut d)?;
    d.app.set_audio_recording(mix.clone())?;
    panel_click(&mut d, "▶ Играть")?;
    walk(&mut d)?;
    d.app.write_recorded_audio()?;
    inspect(&mix)?;
    record_stats(&d.app, &output.join("editor-audio.json"))?;
    panel_click(&mut d, "■ Стоп")?;
    panel_click(&mut d, "Собрать игру для Windows")?;
    if !game.is_file() {
        return Err(format!("EXE со звуками не записан. {}", d.app.status));
    }
    Ok(())
}

pub(super) fn player(package: GamePackage, output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut renderer = Offscreen::new(SIZE, crate::viewport3d::VIEWPORT_MSAA)?;
    configure_context(renderer.context());
    let mut app = Studio::from_package(package, renderer.context());
    let mix = output.join("standalone-stereo.wav");
    app.set_audio_recording(mix.clone())?;
    let mut sink = Sink::None;
    let mut d = Driver::new(app, &mut renderer, &mut sink, None);
    d.settle(6)?;
    walk(&mut d)?;
    d.app.write_recorded_audio()?;
    inspect(&mix)?;
    record_stats(&d.app, &output.join("standalone-audio.json"))
}

fn walk(d: &mut Driver<'_, '_>) -> Result<(), String> {
    let from = d.app.game_position();
    d.frame(vec![proof_input::key_event(Key::D, Modifiers::NONE, true)])?;
    d.settle(88)?;
    d.frame(vec![proof_input::key_event(Key::D, Modifiers::NONE, false)])?;
    d.settle(17)?;
    let position = d.app.game_position();
    let stats = d
        .app
        .audio_recording_stats()
        .ok_or("Звук не записывался.")?;
    if position.x - from.x <= 4.1
        || stats.markers == 0
        || stats.footsteps[Surface::Stone as usize] == 0
        || stats.footsteps[Surface::Wood as usize] == 0
    {
        return Err(format!(
            "Переход с камня на дерево не пройден. Путь {from:?} до {position:?}. События {stats:?}."
        ));
    }
    Ok(())
}

fn record_stats(app: &Studio, output: &Path) -> Result<(), String> {
    let stats = app.audio_recording_stats().ok_or("Звук не записывался.")?;
    let text = serde_json::to_vec_pretty(&stats).map_err(|error| error.to_string())?;
    fs::write(output, text).map_err(io_error)
}

fn inspect(path: &Path) -> Result<[f64; 2], String> {
    let mix = Clip::decode_pcm_wav(&fs::read(path).map_err(io_error)?)?;
    if mix.channels != 2 || mix.sample_rate != OUTPUT_RATE || mix.duration() < 0.4 {
        return Err(format!("Неверные параметры записи. {}", path.display()));
    }
    let mut power = [0.0_f64; 2];
    for frame in mix.frames() {
        for (power, sample) in power.iter_mut().zip(frame) {
            *power += f64::from(*sample).powi(2);
        }
    }
    let rms = power.map(|power| (power / mix.frames().len() as f64).sqrt());
    if rms.iter().all(|&value| value < 0.002) {
        return Err(format!("В записи отсутствует звук. {}", path.display()));
    }
    println!(
        "{} | {:.3} с | RMS {:.6} / {:.6}",
        path.display(),
        mix.duration(),
        rms[0],
        rms[1]
    );
    Ok(rms)
}
