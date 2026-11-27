use crate::{face_timeline::{Channel, Key, Lane}, scene::Scene, story::Speaker};

const TEXT: &str = "Спасибо, что пришёл. Я оставила мастерскую открытой для тебя. Перейди улицу, открой дверь и отметь на верстаке, что инструменты готовы.";
const WAV: &[u8] = include_bytes!("../demo_assets/workshop_audio/lida.wav");
const VISEMES: &str = include_str!("../demo_assets/workshop_audio/lida-visemes.csv");
const DURATION_MS: u16 = 10_799;

pub(crate) fn author(scene: &mut Scene, npc_id: u64) -> Result<(), String> {
    let conversation = scene
        .story
        .conversations
        .iter_mut()
        .find(|conversation| conversation.npc_id == npc_id)
        .ok_or("Демо: не найден диалог Лиды.")?;
    let node = conversation
        .nodes
        .iter_mut()
        .find(|node| node.id == conversation.entry)
        .ok_or("Демо: нет начальной реплики Лиды.")?;
    let lane = speech_lane(VISEMES, DURATION_MS)?;
    let sound = scene
        .audio
        .import_bytes("Лида · просьба у мастерской".into(), WAV.to_vec())?;
    node.text = TEXT.into();
    node.voice = Some(sound);
    node.face = Some(lane);
    Ok(())
}

struct VoicedLine {
    label: &'static str,
    text: &'static str,
    wave: &'static [u8],
    cues: &'static str,
    millis: u16,
    speaker: Speaker,
}

const FLAGSHIP: [VoicedLine; 3] = [
    VoicedLine {
        label: "Лида · решение у шлюза",
        text: "Ты проверил обе линии. Восточный шлюз можно запитать, чтобы снова зажечь набережную. Или оставим питание выключенным и спустим дождевую воду в западный лоток. Решай, какой путь безопаснее.",
        wave: include_bytes!("../demo_assets/workshop_audio/flagship/lida-gate.wav"),
        cues: include_str!("../demo_assets/workshop_audio/flagship/lida-gate.csv"),
        millis: 15_955,
        speaker: Speaker::Npc,
    },
    VoicedLine {
        label: "Арсен · восточный шлюз",
        text: "Запитаю восточный шлюз. Проверю огонь на набережной и вернусь к журналу.",
        wave: include_bytes!("../demo_assets/workshop_audio/flagship/arsen-east.wav"),
        cues: include_str!("../demo_assets/workshop_audio/flagship/arsen-east.csv"),
        millis: 5_646,
        speaker: Speaker::Player,
    },
    VoicedLine {
        label: "Арсен · западный лоток",
        text: "Оставлю питание выключенным. Проверю западный лоток и отмечу сброс в журнале.",
        wave: include_bytes!("../demo_assets/workshop_audio/flagship/arsen-west.wav"),
        cues: include_str!("../demo_assets/workshop_audio/flagship/arsen-west.csv"),
        millis: 5_909,
        speaker: Speaker::Player,
    },
];

pub(crate) fn author_flagship(scene: &mut Scene, npc_id: u64) -> Result<(), String> {
    let conversation = scene.story.conversations.iter_mut()
        .find(|entry| entry.npc_id == npc_id).ok_or("Не найден разговор у шлюза.")?;
    for (index, line) in FLAGSHIP.iter().enumerate() {
        let id = index as u32 + 1;
        let node = conversation.nodes.iter_mut().find(|node| node.id == id)
            .ok_or_else(|| format!("В разговоре у шлюза отсутствует реплика {id}."))?;
        let lane = speech_lane(line.cues, line.millis)?;
        let id = scene.audio.import_bytes(line.label.into(), line.wave.to_vec())?;
        node.text = line.text.into();
        node.voice = Some(id);
        node.speaker = line.speaker;
        node.face = Some(lane);
    }
    Ok(())
}

fn speech_lane(cues: &str, duration: u16) -> Result<Lane, String> {
    let mouths = ["jawOpen", "mouthClose", "mouthPucker"];
    let mut series = mouths.map(|target| Channel {
        target: target.into(),
        keys: Vec::new(),
    });
    for row in cues.lines() {
        let mut cells = row.trim().split(',');
        let field = |cell: Option<&str>| {
            cell.and_then(|cell| cell.parse::<u16>().ok())
                .ok_or("Демо: неверная таблица артикуляции.")
        };
        let at = field(cells.next())?;
        let length = field(cells.next())?;
        let viseme = field(cells.next())?;
        let ms = at.saturating_add(length / 2).clamp(1, duration - 1);
        for (channel, weight) in series.iter_mut().zip(mouth(viseme)) {
            if channel.keys.last().is_none_or(|key| key.ms < ms) {
                channel.keys.push(Key { ms, weight });
            }
        }
    }
    for channel in &mut series {
        channel.keys.insert(0, Key { ms: 0, weight: 0.0 });
        channel.keys.push(Key {
            ms: duration,
            weight: 0.0,
        });
        compress_held(&mut channel.keys);
    }
    let mut channels = series.into_iter().collect::<Vec<_>>();
    let base_channel_count = channels.len();
    let blink = [
        (0, 0.0),
        (1390, 0.0),
        (1450, 0.96),
        (1510, 0.0),
        (3900, 0.0),
        (3970, 0.98),
        (4050, 0.0),
        (7100, 0.0),
        (7160, 0.95),
        (7240, 0.0),
        (10200, 0.0),
        (10260, 0.98),
        (10340, 0.0),
    ];
    channels.extend([
        channel("eyeBlinkLeft", &blink),
        channel("eyeBlinkRight", &blink),
        channel(
            "browInnerUp",
            &[
                (0, 0.05),
                (460, 0.38),
                (1620, 0.25),
                (2530, 0.0),
                (6320, 0.3),
                (7660, 0.15),
                (10550, 0.03),
            ],
        ),
        channel(
            "browOuterUpRight",
            &[
                (0, 0.0),
                (3090, 0.18),
                (4670, 0.02),
                (9070, 0.19),
                (10540, 0.03),
            ],
        ),
        channel(
            "eyeLookOutLeft",
            &[
                (0, 0.0),
                (5480, 0.03),
                (5720, 0.30),
                (6400, 0.16),
                (6920, 0.0),
            ],
        ),
        channel(
            "eyeLookInRight",
            &[
                (0, 0.0),
                (5480, 0.03),
                (5720, 0.26),
                (6400, 0.13),
                (6920, 0.0),
            ],
        ),
        channel(
            "eyeLookDownLeft",
            &[(0, 0.0), (2510, 0.16), (3000, 0.03), (5520, 0.0)],
        ),
        channel(
            "eyeLookDownRight",
            &[(0, 0.0), (2510, 0.16), (3000, 0.03), (5520, 0.0)],
        ),
    ]);
    if duration != DURATION_MS {
        for channel in &mut channels[base_channel_count..] {
            for key in &mut channel.keys {
                key.ms = ((u32::from(key.ms) * u32::from(duration)) / u32::from(DURATION_MS)) as u16;
            }
        }
    }
    let lane = Lane {
        duration_ms: duration,
        channels,
    };
    lane.validate()?;
    Ok(lane)
}

fn channel(target: &str, values: &[(u16, f32)]) -> Channel {
    Channel {
        target: target.into(),
        keys: values
            .iter()
            .map(|&(ms, weight)| Key { ms, weight })
            .collect(),
    }
}

fn compress_held(keys: &mut Vec<Key>) {
    let mut index = 1;
    while index + 1 < keys.len() {
        if keys[index - 1].weight == keys[index].weight
            && keys[index].weight == keys[index + 1].weight
        {
            keys.remove(index);
        } else {
            index += 1;
        }
    }
}

fn mouth(viseme: u16) -> [f32; 3] {
    match viseme {
        0 => [0.0, 0.0, 0.0],
        1 | 2 => [0.55, 0.0, 0.05],
        3 | 8 | 9 | 10 => [0.34, 0.0, 0.56],
        4 | 6 => [0.32, 0.02, 0.0],
        5 | 11 | 12 => [0.37, 0.03, 0.12],
        7 => [0.18, 0.0, 0.69],
        13 | 14 => [0.23, 0.04, 0.11],
        15 | 16 | 17 => [0.20, 0.025, 0.18],
        18 => [0.13, 0.35, 0.06],
        19 | 20 => [0.22, 0.07, 0.04],
        21 => [0.015, 0.76, 0.13],
        _ => [0.0, 0.0, 0.0],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn speech_contains_silence_articulation_and_real_pcm() {
        let clip = crate::audio::Clip::decode_pcm_wav(WAV).unwrap();
        let lane = speech_lane(VISEMES, DURATION_MS).unwrap();
        assert!((8_000..=12_000).contains(&lane.duration_ms));
        assert_eq!(
            clip.duration().mul_add(1000.0, 0.0).round() as u16,
            DURATION_MS
        );
        for name in [
            "jawOpen",
            "mouthPucker",
            "mouthClose",
            "eyeBlinkRight",
            "eyeLookOutLeft",
            "browInnerUp",
        ] {
            assert!(
                lane.channel(name)
                    .unwrap()
                    .keys
                    .iter()
                    .any(|key| key.weight > 0.25),
                "{name}"
            );
        }
    }

    #[test]
    fn both_actor_lines_have_portable_audio_and_valid_face_curves() {
        for line in FLAGSHIP {
            let clip = crate::audio::Clip::decode_pcm_wav(line.wave).unwrap();
            let lane = speech_lane(line.cues, line.millis).unwrap();
            assert_eq!((clip.duration() * 1000.0).round() as u16, lane.duration_ms);
            assert!(lane.channel("jawOpen").unwrap().keys.iter().any(|key| key.weight > 0.28));
            assert!(lane.channel("mouthClose").unwrap().keys.iter().any(|key| key.weight > 0.3));
        }
    }
}
