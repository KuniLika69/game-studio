use std::collections::HashSet;

use eframe::egui::{self, DragValue};
use serde::{Deserialize, Serialize};

use crate::{
    ImportError,
    buildings::Building,
    face_editor,
    face_timeline::{self, Lane},
    rules::{Event as RuleEvent, MAX_STAGE},
    world::{InteractionState, World},
};

const MAX_CONVERSATIONS: usize = 32;
const MAX_LINES: usize = 24;
const MAX_CHOICES: usize = 6;
const MAX_QUESTS: usize = 24;
const MAX_NAME: usize = 80;
const MAX_TEXT: usize = 1_000;
const MAX_CHOICE_TEXT: usize = 160;
const WELCOME_STORY: &[u8] = include_bytes!("../demo_assets/gameplay/welcome.story.json");

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Story {
    pub conversations: Vec<Conversation>,
    pub quests: Vec<Quest>,
    next_quest_id: u32,
    next_choice_id: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub npc_id: u64,
    pub name: String,
    pub entry: u32,
    pub nodes: Vec<Node>,
    #[serde(default)]
    next_node_id: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: u32,
    pub text: String,
    #[serde(default)]
    pub choices: Vec<Choice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<Lane>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<crate::audio_project::SoundId>,
    #[serde(default, skip_serializing_if = "Speaker::is_npc")]
    pub speaker: Speaker,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Speaker {
    #[default]
    Npc,
    Player,
}

impl Speaker {
    fn is_npc(&self) -> bool {
        *self == Self::Npc
    }

    fn label(self) -> &'static str {
        match self {
            Self::Npc => "Собеседник",
            Self::Player => "Игрок",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    #[serde(default)]
    pub id: u32,
    pub text: String,
    pub target: Option<u32>,
    #[serde(default)]
    pub starts_quest: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quest {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub completion: String,
    pub objective: Objective,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Objective {
    Visit { xz: [f32; 2], radius: f32 },
    Talk { npc_id: Option<u64> },
    Enter { building_id: u64 },
    Rules { completion_stage: u16 },
}

impl Default for Objective {
    fn default() -> Self {
        Self::Visit {
            xz: [0.0; 2],
            radius: 1.3,
        }
    }
}

impl Conversation {
    fn new(npc_id: u64, name: String) -> Self {
        Self {
            npc_id,
            name,
            entry: 1,
            nodes: vec![Node {
                id: 1,
                text: "Привет!".into(),
                choices: Vec::new(),
                face: None,
                voice: None,
                speaker: Speaker::Npc,
            }],
            next_node_id: 2,
        }
    }

    fn add_node(&mut self) -> Option<u32> {
        if self.nodes.len() >= MAX_LINES {
            return None;
        }
        let id = next_id(self.next_node_id, self.nodes.iter().map(|node| node.id))?;
        self.next_node_id = id + 1;
        self.nodes.push(Node {
            id,
            text: "Новая реплика".into(),
            choices: Vec::new(),
            face: None,
            voice: None,
            speaker: Speaker::Npc,
        });
        Some(id)
    }

    fn remove_node(&mut self, id: u32) {
        if self.nodes.len() < 2 {
            return;
        }
        self.nodes.retain(|node| node.id != id);
        if self.entry == id && let Some(node) = self.nodes.first() {
            self.entry = node.id;
        }
        for choice in self.nodes.iter_mut().flat_map(|node| &mut node.choices) {
            if choice.target == Some(id) {
                choice.target = None;
            }
        }
    }

    pub(crate) fn node(&self, id: u32) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }
}

impl Story {
    pub fn facial_lane(&self, npc_id: u64, node_id: u32) -> Option<&Lane> {
        self.conversation(npc_id)?.node(node_id)?.face.as_ref()
    }

    pub(crate) fn validate_voices(
        &self,
        audio: &crate::audio_project::Library,
    ) -> Result<(), ImportError> {
        for conversation in &self.conversations {
            for node in &conversation.nodes {
                if let Some(sound) = node.voice && audio.sound(sound).is_none() {
                    return Err(ImportError::new(
                        format!("Диалог с персонажем №{}, реплика №{}", conversation.npc_id, node.id),
                        "Указан отсутствующий звук.",
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn visit_destination(&self, id: u32) -> Option<[f32; 2]> {
        match self.quest(id)?.objective {
            Objective::Visit { xz, .. } => Some(xz),
            Objective::Talk { .. } | Objective::Enter { .. } | Objective::Rules { .. } => None,
        }
    }

    pub fn set_visit_destination(&mut self, id: u32, position: [f32; 2]) -> bool {
        let Some(Quest {
            objective: Objective::Visit { xz, .. },
            ..
        }) = self.quests.iter_mut().find(|quest| quest.id == id)
        else {
            return false;
        };
        *xz = position.map(|axis| axis.clamp(-256.0, 256.0));
        true
    }

    pub fn demo(npc_id: u64, destination: [f32; 2]) -> Self {
        let Ok(mut story) = Self::decode(WELCOME_STORY) else {
            return Self::default();
        };
        if let Some(conversation) = story.conversations.first_mut() {
            conversation.npc_id = npc_id;
        }
        if let Some(Quest {
            objective: Objective::Visit { xz, .. },
            ..
        }) = story.quests.first_mut()
        {
            *xz = destination;
        }
        story
    }

    fn decode(data: &[u8]) -> Result<Self, ImportError> {
        let story: Self = serde_json::from_slice(data).map_err(|error| {
            ImportError::new(
                format!(
                    "Сценарий JSON, строка {}, столбец {}",
                    error.line(),
                    error.column()
                ),
                error,
            )
        })?;
        story.validate()?;
        Ok(story)
    }

    pub fn conversation(&self, npc_id: u64) -> Option<&Conversation> {
        self.conversations
            .iter()
            .find(|conversation| conversation.npc_id == npc_id)
    }

    pub fn conversation_label(&self, npc_id: u64) -> Option<&str> {
        self.conversation(npc_id)
            .map(|conversation| nonempty(&conversation.name, "Собеседник"))
    }

    pub fn can_talk(&self, npc_id: u64) -> bool {
        self.conversation(npc_id).is_some() || self.quests.iter().any(|quest| {
            matches!(quest.objective, Objective::Talk { npc_id: Some(target) } if target == npc_id)
        })
    }

    pub fn remove_npc(&mut self, npc_id: u64) {
        self.conversations
            .retain(|conversation| conversation.npc_id != npc_id);
        for quest in &mut self.quests {
            if let Objective::Talk { npc_id: target } = &mut quest.objective
                && *target == Some(npc_id)
            {
                *target = None;
            }
        }
    }

    pub(crate) fn add_quest(&mut self) -> Option<u32> {
        if self.quests.len() >= MAX_QUESTS {
            return None;
        }
        let id = next_id(self.next_quest_id, self.quests.iter().map(|quest| quest.id))?;
        self.next_quest_id = id + 1;
        self.quests.push(Quest {
            id,
            title: "Новое задание".into(),
            description: "Дойди до указанного места.".into(),
            completion: "Цель достигнута.".into(),
            objective: Objective::default(),
        });
        Some(id)
    }

    fn remove_quest(&mut self, id: u32) {
        self.quests.retain(|quest| quest.id != id);
        for choice in self
            .conversations
            .iter_mut()
            .flat_map(|c| &mut c.nodes)
            .flat_map(|n| &mut n.choices)
        {
            if choice.starts_quest == Some(id) {
                choice.starts_quest = None;
            }
        }
    }

    pub fn quest(&self, id: u32) -> Option<&Quest> {
        self.quests.iter().find(|quest| quest.id == id)
    }

    pub fn answer(&self, npc_id: u64, node_id: u32, answer_id: u32) -> Option<&Choice> {
        self.conversation(npc_id)?
            .node(node_id)?
            .choices
            .iter()
            .find(|choice| choice.id == answer_id)
    }

    pub(crate) fn migrate_answer_ids(&mut self) {
        let highest = self
            .conversations
            .iter()
            .flat_map(|conversation| &conversation.nodes)
            .flat_map(|node| &node.choices)
            .map(|choice| choice.id)
            .max()
            .unwrap_or(0);
        if highest > 0 {
            self.next_choice_id = self.next_choice_id.max(highest.saturating_add(1));
        }
        for choice in self
            .conversations
            .iter_mut()
            .flat_map(|conversation| &mut conversation.nodes)
            .flat_map(|node| &mut node.choices)
            .filter(|choice| choice.id == 0)
        {
            let Some(id) = reserve_answer_id(&mut self.next_choice_id) else {
                break;
            };
            choice.id = id;
        }
    }

    pub fn validate(&self) -> Result<(), ImportError> {
        if self.conversations.len() > MAX_CONVERSATIONS || self.quests.len() > MAX_QUESTS {
            return Err(ImportError::new(
                "Сценарий",
                "Слишком много диалогов или заданий в сцене.",
            ));
        }
        let mut quest_ids = HashSet::new();
        for quest in &self.quests {
            let invalid = |detail| ImportError::new(format!("Задание №{}", quest.id), detail);
            if !valid_id(quest.id)
                || !quest_ids.insert(quest.id)
                || !valid_text(&quest.title, MAX_NAME, false)
                || !valid_text(&quest.description, MAX_TEXT, true)
                || !valid_text(&quest.completion, MAX_TEXT, true)
            {
                return Err(invalid("Некорректное название, текст или номер задания."));
            }
            match quest.objective {
                Objective::Visit { xz, radius }
                    if !xz.into_iter().all(|v| (-256.0..=256.0).contains(&v))
                        || !(0.3..=32.0).contains(&radius) =>
                {
                    return Err(invalid("Координаты или радиус цели выходят за допустимые пределы."));
                }
                Objective::Talk { npc_id: Some(0) } => {
                    return Err(invalid("Некорректный номер собеседника задания."));
                }
                Objective::Enter { building_id: 0 } => {
                    return Err(invalid("Некорректный номер здания задания."));
                }
                Objective::Rules { completion_stage }
                    if !(1..=MAX_STAGE).contains(&completion_stage) =>
                {
                    return Err(invalid("Некорректный финальный этап задания."));
                }
                _ => {}
            }
        }
        let mut npc_ids = HashSet::new();
        let mut answer_ids = HashSet::new();
        for conversation in &self.conversations {
            if conversation.npc_id == 0
                || !npc_ids.insert(conversation.npc_id)
                || !valid_text(&conversation.name, MAX_NAME, false)
                || !(1..=MAX_LINES).contains(&conversation.nodes.len())
                || conversation.node(conversation.entry).is_none()
            {
                return Err(ImportError::new(
                    format!("Диалог, персонаж №{}", conversation.npc_id),
                    "Некорректное начало диалога или его собеседник.",
                ));
            }
            let mut node_ids = HashSet::new();
            for node in &conversation.nodes {
                if !valid_id(node.id)
                    || !node_ids.insert(node.id)
                    || !valid_text(&node.text, MAX_TEXT, true)
                    || node.choices.len() > MAX_CHOICES
                {
                    return Err(ImportError::new(
                        format!("Диалог «{}», реплика №{}", conversation.name, node.id),
                        "Некорректная реплика диалога.",
                    ));
                }
                if let Some(lane) = &node.face {
                    lane.validate().map_err(|error| {
                        ImportError::new(
                            format!("Диалог «{}», мимика реплики №{}", conversation.name, node.id),
                            error,
                        )
                    })?;
                }
                for choice in &node.choices {
                    if !valid_id(choice.id)
                        || choice.id >= self.next_choice_id
                        || !answer_ids.insert(choice.id)
                        || !valid_text(&choice.text, MAX_CHOICE_TEXT, false)
                        || choice
                            .target
                            .is_some_and(|target| conversation.node(target).is_none())
                        || choice
                            .starts_quest
                            .is_some_and(|id| !quest_ids.contains(&id))
                    {
                        return Err(ImportError::new(
                            format!(
                                "Диалог «{}», реплика №{}, ответ №{}",
                                conversation.name, node.id, choice.id
                            ),
                            "Ответ содержит некорректный номер, текст или ссылку.",
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn validate_npcs(&self, npc_ids: impl IntoIterator<Item = u64>) -> Result<(), ImportError> {
        let known: HashSet<_> = npc_ids.into_iter().collect();
        if self.conversations.iter().any(|conversation| !known.contains(&conversation.npc_id))
            || self.quests.iter().any(|quest| matches!(quest.objective, Objective::Talk { npc_id: Some(id) } if !known.contains(&id)))
        {
            return Err(ImportError::new(
                "Собеседники",
                "Диалог или задание ссылается на отсутствующего персонажа.",
            ));
        }
        Ok(())
    }
}

fn next_id(current: u32, existing: impl Iterator<Item = u32>) -> Option<u32> {
    let id = existing.max().unwrap_or(0).saturating_add(1).max(current);
    valid_id(id).then_some(id)
}

fn reserve_answer_id(counter: &mut u32) -> Option<u32> {
    let id = next_id(*counter, std::iter::empty())?;
    *counter = id + 1;
    Some(id)
}

fn valid_id(id: u32) -> bool {
    (1..u32::MAX).contains(&id)
}

fn valid_text(text: &str, limit: usize, multiline: bool) -> bool {
    text.chars().count() <= limit
        && text
            .chars()
            .all(|c| !c.is_control() || (multiline && matches!(c, '\n' | '\r' | '\t')))
}

fn nonempty<'a>(text: &'a str, fallback: &'a str) -> &'a str {
    if text.trim().is_empty() {
        fallback
    } else {
        text
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Progress {
    Active,
    Complete,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProgressSnapshot {
    entries: Vec<(u32, Progress)>,
    latest_complete: Option<u32>,
}

impl ProgressSnapshot {
    pub fn validate(&self) -> Result<(), ImportError> {
        let mut seen = HashSet::new();
        if self.entries.len() > MAX_QUESTS
            || self
                .entries
                .iter()
                .any(|&(id, _)| id == 0 || !seen.insert(id))
            || self
                .latest_complete
                .is_some_and(|id| !self.entries.contains(&(id, Progress::Complete)))
        {
            return Err(ImportError::new("Сохранение заданий", "Некорректный прогресс заданий."));
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct Session {
    current: Option<(u64, u32)>,
    face_seconds: f32,
    progress: Vec<(u32, Progress)>,
    latest_complete: Option<u32>,
}

impl Session {
    pub(crate) fn progress_name(&self, id: u32) -> &'static str {
        match self
            .progress
            .iter()
            .find(|(quest, _)| *quest == id)
            .map(|(_, progress)| progress)
        {
            Some(Progress::Active) => "В процессе",
            Some(Progress::Complete) => "Выполнено",
            None => "Не начато",
        }
    }

    pub fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            entries: self.progress.clone(),
            latest_complete: self.latest_complete,
        }
    }

    pub fn restore(&mut self, story: &Story, snapshot: &ProgressSnapshot) {
        self.current = None;
        self.face_seconds = 0.0;
        self.progress = snapshot
            .entries
            .iter()
            .copied()
            .filter(|&(id, _)| {
                story
                    .quest(id)
                    .is_some_and(|quest| !matches!(quest.objective, Objective::Rules { .. }))
            })
            .collect();
        self.latest_complete = snapshot
            .latest_complete
            .filter(|&id| self.progress.contains(&(id, Progress::Complete)))
            .or_else(|| {
                self.progress
                    .iter()
                    .rev()
                    .find_map(|&(id, progress)| (progress == Progress::Complete).then_some(id))
            });
    }

    pub fn is_open(&self) -> bool {
        self.current.is_some()
    }

    pub fn advance_face(&mut self, dt: f32) {
        if self.current.is_some() && dt.is_finite() && dt > 0.0 {
            self.face_seconds =
                (self.face_seconds + dt).min(f32::from(face_timeline::MAX_MS) / 1_000.0);
        }
    }

    pub fn face_time(&self, npc: u64) -> Option<(u32, f32)> {
        let (speaker, node) = self.current?;
        (speaker == npc).then_some((node, self.face_seconds))
    }

    pub fn active_npc(&self) -> Option<u64> {
        self.current.map(|(npc, _)| npc)
    }

    pub fn close(&mut self) {
        self.current = None;
        self.face_seconds = 0.0;
    }

    pub fn open(&mut self, story: &Story, npc_id: u64) -> bool {
        let completed = self.record_talk(story, npc_id);
        let Some(conversation) = story.conversation(npc_id) else {
            return completed;
        };
        if conversation.node(conversation.entry).is_none() {
            return completed;
        }
        self.current = Some((npc_id, conversation.entry));
        self.face_seconds = 0.0;
        true
    }

    pub fn current<'story>(&self, story: &'story Story) -> Option<(&'story str, &'story Node)> {
        let (npc, id) = self.current?;
        let conversation = story.conversation(npc)?;
        Some((story.conversation_label(npc)?, conversation.node(id)?))
    }

    #[cfg(test)]
    pub fn choose(&mut self, story: &Story, index: usize) -> bool {
        self.choose_event(story, index).is_some()
    }

    pub fn choose_event(&mut self, story: &Story, index: usize) -> Option<RuleEvent> {
        let (npc_id, node_id) = self.current?;
        let (_, node) = self.current(story)?;
        let choice = node.choices.get(index)?;
        if let Some(id) = choice.starts_quest
            && story
                .quest(id)
                .is_some_and(|quest| !matches!(quest.objective, Objective::Rules { .. }))
            && !self.progress.iter().any(|(existing, _)| *existing == id)
        {
            self.progress.push((id, Progress::Active));
            self.record_talk(story, npc_id);
        }
        self.current = choice.target.map(|target| (npc_id, target));
        self.face_seconds = 0.0;
        Some(RuleEvent::Dialogue {
            npc_id,
            node_id,
            choice_id: choice.id,
        })
    }

    fn record_talk(&mut self, story: &Story, npc_id: u64) -> bool {
        self.complete_if(story, |objective| matches!(objective, Objective::Talk { npc_id: Some(target) } if *target == npc_id))
    }

    #[cfg(test)]
    pub fn update(&mut self, story: &Story, player_xz: [f32; 2]) {
        self.complete_if(story, |objective| is_visit(objective, player_xz));
    }

    pub fn update_world(
        &mut self,
        story: &Story,
        world: &World,
        state: &InteractionState,
        from: [f32; 3],
        to: [f32; 3],
    ) {
        self.complete_if(story, |objective| match objective {
            Objective::Enter { building_id } => world.entry_crossed(state, *building_id, from, to),
            _ => is_visit(objective, [to[0], to[2]]),
        });
    }

    fn complete_if(&mut self, story: &Story, predicate: impl Fn(&Objective) -> bool) -> bool {
        let mut completed = false;
        for (id, progress) in &mut self.progress {
            if *progress == Progress::Active
                && story
                    .quest(*id)
                    .is_some_and(|quest| predicate(&quest.objective))
            {
                *progress = Progress::Complete;
                self.latest_complete = Some(*id);
                completed = true;
            }
        }
        completed
    }

    fn active<'story>(&self, story: &'story Story) -> impl Iterator<Item = &'story Quest> {
        self.progress
            .iter()
            .filter(|(_, status)| *status == Progress::Active)
            .filter_map(|(id, _)| story.quest(*id))
    }

    #[cfg(test)]
    pub fn active_destination(&self, story: &Story) -> Option<[f32; 2]> {
        self.active(story).find_map(|quest| match quest.objective {
            Objective::Visit { xz, .. } => Some(xz),
            Objective::Talk { .. } | Objective::Enter { .. } | Objective::Rules { .. } => None,
        })
    }

    pub fn active_destination_in(&self, story: &Story, world: &World) -> Option<[f32; 2]> {
        self.active(story).find_map(|quest| match quest.objective {
            Objective::Visit { xz, .. } => Some(xz),
            Objective::Talk { .. } | Objective::Rules { .. } => None,
            Objective::Enter { building_id } => world
                .buildings
                .iter()
                .find(|building| building.id == building_id)
                .map(|building| {
                    let [x, _, z] = building.entrance_position();
                    [x, z]
                }),
        })
    }

    pub fn objective(&self, story: &Story) -> Option<String> {
        if let Some(quest) = self.active(story).next() {
            let fallback = match quest.objective {
                Objective::Visit { .. } => "Дойди до отмеченного места.".into(),
                Objective::Enter { .. } => "Пройди внутрь через входную дверь.".into(),
                Objective::Rules { .. } => "Выполни условия задания.".into(),
                Objective::Talk { npc_id } => npc_id
                    .and_then(|id| story.conversation_label(id))
                    .map_or_else(
                        || "Поговори с собеседником задания.".into(),
                        |name| format!("Поговори с персонажем «{name}»."),
                    ),
            };
            return Some(format!(
                "{} · {}",
                nonempty(&quest.title, "Задание"),
                nonempty(&quest.description, &fallback)
            ));
        }
        let quest = story.quest(self.latest_complete?)?;
        Some(format!(
            "Выполнено · {} · {}",
            nonempty(&quest.title, "Задание"),
            nonempty(&quest.completion, "Цель достигнута.")
        ))
    }

    pub fn show(&mut self, ctx: &egui::Context, story: &Story, portrait: bool) -> Option<RuleEvent> {
        let Some((name, node)) = self.current(story) else {
            self.close();
            return None;
        };
        let mut choice = None;
        let mut close = false;
        let id = egui::Id::new("story_conversation");
        let modal = if portrait {
            egui::Modal::new(id).area(
                egui::Modal::default_area(id)
                    .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -12.0)),
            )
        } else {
            egui::Modal::new(id)
        };
        let modal = modal.show(ctx, |ui| {
            ui.set_width(430.0);
            ui.heading(if node.speaker == Speaker::Player { "Игрок" } else { name });
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| ui.label(nonempty(&node.text, "…")));
            ui.separator();
            for (index, option) in node.choices.iter().enumerate() {
                if ui.button(nonempty(&option.text, "Продолжить")).clicked() {
                    choice = Some(index);
                }
            }
            let label = if node.choices.is_empty() {
                "Завершить"
            } else {
                "Выйти из разговора"
            };
            close = ui.button(label).clicked();
        });
        if let Some(index) = choice {
            self.choose_event(story, index)
        } else if close || modal.should_close() {
            self.close();
            None
        } else {
            None
        }
    }
}

fn is_visit(objective: &Objective, [x, z]: [f32; 2]) -> bool {
    match *objective {
        Objective::Visit { xz, radius } => {
            (x - xz[0]).powi(2) + (z - xz[1]).powi(2) <= radius * radius
        }
        _ => false,
    }
}

pub fn inspector(
    ui: &mut egui::Ui,
    story: &mut Story,
    npc_id: u64,
    enabled: bool,
    preview: &mut face_editor::Preview,
    morphs: [&[String]; 2],
    sounds: &[crate::audio_project::Sound],
) -> bool {
    ui.add_enabled_ui(enabled, |ui| {
        ui.separator();
        ui.strong("Диалог");
        let Some(index) = story
            .conversations
            .iter()
            .position(|item| item.npc_id == npc_id)
        else {
            if ui
                .add_enabled(
                    story.conversations.len() < MAX_CONVERSATIONS,
                    egui::Button::new("Создать диалог"),
                )
                .clicked()
            {
                story
                    .conversations
                    .push(Conversation::new(npc_id, "Собеседник".into()));
                return true;
            }
            return false;
        };
        edit_conversation(
            ui,
            &mut story.conversations[index],
            &story.quests,
            &mut story.next_choice_id,
            preview,
            morphs,
            sounds,
        )
    })
    .inner
}

fn edit_conversation(
    ui: &mut egui::Ui,
    conversation: &mut Conversation,
    quests: &[Quest],
    next_choice_id: &mut u32,
    preview: &mut face_editor::Preview,
    morphs: [&[String]; 2],
    sounds: &[crate::audio_project::Sound],
) -> bool {
    let id = ui.id().with(("dialogue_node", conversation.npc_id));
    let old = ui
        .ctx()
        .data(|data| data.get_temp::<u32>(id))
        .filter(|&id| conversation.node(id).is_some());
    let mut selected = old.unwrap_or(conversation.entry);
    let mut changed = text_line(ui, &mut conversation.name, "Имя в диалоге", MAX_NAME);
    if ui
        .add_enabled(
            conversation.nodes.len() < MAX_LINES,
            egui::Button::new("+ Реплика"),
        )
        .clicked()
        && let Some(node) = conversation.add_node()
    {
        selected = node;
        changed = true;
    }
    egui::ComboBox::from_id_salt("dialogue_line")
        .selected_text(
            conversation
                .node(selected)
                .map_or_else(|| "Реплика".into(), node_label),
        )
        .show_ui(ui, |ui| {
            for node in &conversation.nodes {
                ui.selectable_value(&mut selected, node.id, node_label(node));
            }
        });
    ui.horizontal_wrapped(|ui| {
        if selected == conversation.entry {
            ui.weak("Начало разговора");
        } else if ui.button("Сделать начальной").clicked() {
            conversation.entry = selected;
            changed = true;
        }
        if ui
            .add_enabled(
                conversation.nodes.len() > 1,
                egui::Button::new("Удалить реплику"),
            )
            .clicked()
        {
            conversation.remove_node(selected);
            selected = conversation.entry;
            changed = true;
        }
    });
    ui.ctx().data_mut(|data| data.insert_temp(id, selected));
    let targets: Vec<_> = conversation
        .nodes
        .iter()
        .map(|node| (node.id, node_label(node)))
        .collect();
    let Some(node) = conversation
        .nodes
        .iter_mut()
        .find(|node| node.id == selected)
    else {
        return changed;
    };
    changed |= text_area(ui, &mut node.text, "Текст реплики", 3);
    let previous = node.speaker;
    egui::ComboBox::from_id_salt("dialogue_speaker")
        .selected_text(format!("Говорит · {}", previous.label()))
        .show_ui(ui, |ui| {
            for speaker in [Speaker::Npc, Speaker::Player] {
                ui.selectable_value(&mut node.speaker, speaker, speaker.label());
            }
        });
    changed |= previous != node.speaker;
    let old_voice = node.voice;
    egui::ComboBox::from_id_salt("dialogue_voice")
        .selected_text(old_voice.and_then(|id| sounds.iter().find(|sound| sound.id == id))
            .map_or("Без озвучивания", |sound| sound.name.as_str()))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut node.voice, None, "Без озвучивания");
            for sound in sounds {
                ui.selectable_value(&mut node.voice, Some(sound.id), &sound.name);
            }
        }).response.on_hover_text("Озвучивание реплики. Импорт WAV доступен во вкладке «Звук».");
    changed |= old_voice != node.voice;
    preview.select_node(conversation.npc_id, node.id);
    let speaker_morphs = morphs[usize::from(node.speaker == Speaker::Player)];
    changed |= ui
        .push_id((conversation.npc_id, node.id, "face"), |ui| {
            ui.collapsing("Лицевая анимация", |ui| {
                preview.show(ui, &mut node.face, speaker_morphs)
            })
            .body_returned
            .unwrap_or(false)
        })
        .inner;
    if ui
        .add_enabled(
            node.choices.len() < MAX_CHOICES,
            egui::Button::new("+ Ответ игрока"),
        )
        .clicked()
        && let Some(answer_id) = reserve_answer_id(next_choice_id)
    {
        node.choices.push(Choice {
            id: answer_id,
            text: "Ответить".into(),
            target: None,
            starts_quest: None,
        });
        changed = true;
    }
    let mut remove = None;
    for (index, answer) in node.choices.iter_mut().enumerate() {
        ui.push_id((node.id, answer.id), |ui| {
            ui.group(|ui| {
                changed |= text_line(ui, &mut answer.text, "Текст ответа", MAX_CHOICE_TEXT);
                let target_before = answer.target;
                let target_name = targets
                    .iter()
                    .find(|(id, _)| Some(*id) == answer.target)
                    .map_or("Завершить разговор", |(_, text)| text);
                egui::ComboBox::from_id_salt("answer_target")
                    .selected_text(target_name)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut answer.target, None, "Завершить разговор");
                        for (id, name) in &targets {
                            ui.selectable_value(&mut answer.target, Some(*id), name);
                        }
                    });
                changed |= target_before != answer.target;
                let quest_before = answer.starts_quest;
                let current = quests
                    .iter()
                    .find(|quest| Some(quest.id) == answer.starts_quest)
                    .map_or("Без задания", |quest| {
                        nonempty(&quest.title, "Задание")
                    });
                egui::ComboBox::from_id_salt("answer_quest")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut answer.starts_quest, None, "Без задания");
                        for quest in quests {
                            ui.selectable_value(
                                &mut answer.starts_quest,
                                Some(quest.id),
                                nonempty(&quest.title, "Задание"),
                            );
                        }
                    });
                changed |= quest_before != answer.starts_quest;
                if ui.small_button("Удалить ответ").clicked() {
                    remove = Some(index);
                }
            });
        });
    }
    if let Some(index) = remove {
        node.choices.remove(index);
        changed = true;
    }
    changed
}

pub fn quest_editor(
    ui: &mut egui::Ui,
    story: &mut Story,
    enabled: bool,
    npc_names: &[(u64, &str)],
    placing: &mut Option<u32>,
    buildings: &[Building],
) -> bool {
    ui.add_enabled_ui(enabled, |ui| {
        let key = ui.id().with("selected_quest");
        let mut selected = ui
            .ctx()
            .data(|data| data.get_temp::<u32>(key))
            .filter(|id| story.quest(*id).is_some())
            .or_else(|| story.quests.first().map(|quest| quest.id));
        let mut changed = false;
        if ui
            .add_enabled(
                story.quests.len() < MAX_QUESTS,
                egui::Button::new("+ Задание"),
            )
            .clicked()
        {
            selected = story.add_quest();
            changed = selected.is_some();
        }
        let Some(id) = selected else {
            ui.weak("Создай задание и привяжи его к ответу в диалоге.");
            return changed;
        };
        let current = story
            .quest(id)
            .map_or("Задание", |quest| nonempty(&quest.title, "Задание"));
        let mut selection = id;
        egui::ComboBox::from_id_salt("quest_list")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for quest in &story.quests {
                    ui.selectable_value(
                        &mut selection,
                        quest.id,
                        nonempty(&quest.title, "Задание"),
                    );
                }
            });
        ui.ctx().data_mut(|data| data.insert_temp(key, selection));
        if ui.button("Удалить задание").clicked() {
            story.remove_quest(selection);
            *placing = None;
            return true;
        }
        if placing.is_some_and(|id| id != selection) {
            *placing = None;
        }
        if let Some(quest) = story.quests.iter_mut().find(|quest| quest.id == selection) {
            changed |= edit_quest(ui, quest, npc_names, buildings);
            if matches!(quest.objective, Objective::Visit { .. }) {
                if ui
                    .button(if placing.is_some() {
                        "Отменить выбор места"
                    } else {
                        "Указать цель на 2D-плане"
                    })
                    .clicked()
                {
                    *placing = placing.is_none().then_some(selection);
                }
            } else {
                *placing = None;
            }
        }
        changed
    })
    .inner
}

fn edit_quest(
    ui: &mut egui::Ui,
    quest: &mut Quest,
    speakers: &[(u64, &str)],
    buildings: &[Building],
) -> bool {
    let mut changed = text_line(ui, &mut quest.title, "Название задания", MAX_NAME);
    ui.label("Подсказка игроку");
    changed |= text_area(ui, &mut quest.description, "Например, дойди до дома.", 2);
    ui.label("Сообщение о выполнении");
    changed |= text_area(ui, &mut quest.completion, "Цель достигнута.", 2);
    #[derive(Clone, Copy, PartialEq)]
    enum Kind {
        Visit,
        Talk,
        Enter,
        Rules,
    }
    let previous = match quest.objective {
        Objective::Visit { .. } => Kind::Visit,
        Objective::Talk { .. } => Kind::Talk,
        Objective::Enter { .. } => Kind::Enter,
        Objective::Rules { .. } => Kind::Rules,
    };
    let mut kind = previous;
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut kind, Kind::Visit, "Дойти до места");
        ui.selectable_value(&mut kind, Kind::Talk, "Поговорить с персонажем");
        ui.selectable_value(&mut kind, Kind::Rules, "По правилам");
        ui.add_enabled_ui(!buildings.is_empty(), |ui| {
            ui.selectable_value(&mut kind, Kind::Enter, "Войти через дверь");
        })
        .response
        .on_disabled_hover_text("Построй дом в разделе «Окружение».");
    });
    if kind != previous {
        quest.objective = match kind {
            Kind::Visit => Objective::default(),
            Kind::Talk => Objective::Talk {
                npc_id: speakers.first().map(|(id, _)| *id),
            },
            Kind::Enter => Objective::Enter {
                building_id: buildings[0].id,
            },
            Kind::Rules => Objective::Rules {
                completion_stage: 4,
            },
        };
        changed = true;
    }
    match &mut quest.objective {
        Objective::Visit { xz: [x, z], radius } => {
            ui.horizontal_wrapped(|ui| {
                changed |= ui
                    .add(
                        DragValue::new(x)
                            .range(-256.0..=256.0)
                            .speed(0.05)
                            .prefix("X "),
                    )
                    .changed();
                changed |= ui
                    .add(
                        DragValue::new(z)
                            .range(-256.0..=256.0)
                            .speed(0.05)
                            .prefix("Z "),
                    )
                    .changed();
                changed |= ui
                    .add(
                        DragValue::new(radius)
                            .range(0.3..=32.0)
                            .speed(0.05)
                            .prefix("Радиус "),
                    )
                    .changed();
            });
        }
        Objective::Talk { npc_id } => {
            let before = *npc_id;
            let label = speakers
                .iter()
                .find(|(id, _)| Some(*id) == *npc_id)
                .map_or("Выбрать собеседника", |(_, name)| *name);
            egui::ComboBox::from_id_salt("quest_speaker")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(npc_id, None, "Выбрать собеседника");
                    for &(id, name) in speakers {
                        ui.selectable_value(npc_id, Some(id), name);
                    }
                });
            changed |= before != *npc_id;
        }
        Objective::Enter { building_id } => {
            let before = *building_id;
            let label = buildings
                .iter()
                .find(|building| building.id == *building_id)
                .map_or("Дом отсутствует", |building| {
                    building.name.as_str()
                });
            ui.label("Вход в дом");
            egui::ComboBox::from_id_salt("quest_building")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for house in buildings {
                        ui.selectable_value(building_id, house.id, &house.name);
                    }
                });
            ui.weak("Задание выполнится, когда игрок пройдёт внутрь через вход с улицы.");
            changed |= before != *building_id;
        }
        Objective::Rules { completion_stage } => {
            changed |= ui
                .add(
                    DragValue::new(completion_stage)
                        .range(1..=MAX_STAGE)
                        .prefix("Финальный этап "),
                )
                .changed();
            ui.weak("Настрой этапы в разделе «Правила» действием «Изменить этап задания».");
        }
    }
    changed
}

fn text_line(ui: &mut egui::Ui, text: &mut String, hint: &str, limit: usize) -> bool {
    ui.add(
        egui::TextEdit::singleline(text)
            .char_limit(limit)
            .hint_text(hint),
    )
    .changed()
}

fn text_area(ui: &mut egui::Ui, text: &mut String, hint: &str, rows: usize) -> bool {
    ui.add(
        egui::TextEdit::multiline(text)
            .char_limit(MAX_TEXT)
            .hint_text(hint)
            .desired_rows(rows),
    )
    .changed()
}

fn node_label(node: &Node) -> String {
    let sample: String = node
        .text
        .trim()
        .chars()
        .take(30)
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    format!("{} · {}", node.id, nonempty(&sample, "Реплика"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a6_json_story_uses_authored_dialogue_targets_and_quest_ids() -> Result<(), ImportError> {
        let story = Story::decode(include_bytes!("../tests/fixtures/a6_authored.story.json"))?;
        story.validate_npcs([4001, 8102])?;
        let mut session = Session::default();
        assert!(session.open(&story, 4001));
        assert_eq!(session.progress_name(42), "Не начато");
        assert_eq!(
            session.choose_event(&story, 0),
            Some(RuleEvent::Dialogue {
                npc_id: 4001,
                node_id: 17,
                choice_id: 77,
            })
        );
        assert_eq!(session.current(&story).map(|(_, node)| node.id), Some(31));
        assert_eq!(session.progress_name(42), "В процессе");
        assert!(!session.open(&story, 107));
        assert_eq!(session.progress_name(42), "В процессе");
        assert!(session.open(&story, 8102));
        assert_eq!(session.progress_name(42), "Выполнено");
        assert!(!session.open(&story, 8102));
        assert_eq!(session.snapshot().latest_complete, Some(42));
        Ok(())
    }

    #[test]
    fn a6_welcome_preset_preserves_the_previous_ids_and_resolves_placement() -> Result<(), ImportError> {
        let npc = 1025;
        let destination = [8.0, -3.5];
        let story = Story::demo(npc, destination);
        story.validate()?;
        story.validate_npcs([npc])?;
        let conversation = story
            .conversation(npc)
            .ok_or("Диалог из шаблона отсутствует.")?;
        assert_eq!(conversation.entry, 1);
        assert_eq!(conversation.next_node_id, 4);
        assert_eq!(
            conversation
                .nodes
                .iter()
                .map(|node| node.id)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(
            conversation
                .nodes
                .iter()
                .flat_map(|node| &node.choices)
                .map(|choice| choice.id)
                .collect::<Vec<_>>(),
            [1, 2, 3, 4, 5]
        );
        assert_eq!(
            story.answer(npc, 1, 1).and_then(|choice| choice.starts_quest),
            Some(1)
        );
        assert_eq!(story.visit_destination(1), Some(destination));
        assert_eq!(story.next_quest_id, 2);
        assert_eq!(story.next_choice_id, 6);
        Ok(())
    }
    use std::path::Path;

    fn assert_invalid_story_fixture(name: &str, context: &str, detail: &str) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name);
        assert!(matches!(
            crate::scene::Scene::load(&path),
            Err(ImportError::Invalid { path: Some(file), context: section, detail: cause })
                if file == path && section.contains(context) && cause.contains(detail)
        ));
    }

    #[test]
    fn a1_story_rejects_missing_dialogue_entry() {
        assert_invalid_story_fixture(
            "a1_rules_story_missing_entry.robot.json",
            "персонаж №12",
            "Некорректное начало диалога",
        );
    }

    #[test]
    fn a1_story_rejects_invalid_quest_radius() {
        assert_invalid_story_fixture(
            "a1_rules_story_bad_radius.robot.json",
            "Задание №1",
            "радиус цели",
        );
    }

    #[test]
    fn facial_time_is_owned_by_exact_speaker_and_restarts_on_a_branch() {
        let npc = 17;
        let mut story = Story::demo(npc, [0.0; 2]);
        let conversation = &mut story.conversations[0];
        let entry = conversation.entry;
        let next = conversation.nodes[0].choices[0].target.unwrap();
        let mut first = super::Lane::default();
        let mut second = super::Lane::default();
        first.add_key("eyeBlinkLeft", 200);
        second.add_key("jawOpen", 0);
        conversation.nodes[0].face = Some(first);
        conversation
            .nodes
            .iter_mut()
            .find(|node| node.id == next)
            .unwrap()
            .face = Some(second);
        let bytes = serde_json::to_vec(&story).unwrap();
        let reopened: Story = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(reopened, story);
        reopened.validate().unwrap();

        let mut session = Session::default();
        assert!(session.open(&reopened, npc));
        session.advance_face(0.1);
        assert_eq!(session.face_time(npc), Some((entry, 0.1)));
        assert_eq!(session.face_time(npc + 1), None);
        assert_eq!(
            reopened
                .facial_lane(npc, entry)
                .unwrap()
                .weight("eyeBlinkLeft", 0.1),
            Some(0.5)
        );
        assert!(session.choose_event(&reopened, 0).is_some());
        assert_eq!(session.face_time(npc), Some((next, 0.0)));
        assert_eq!(
            reopened
                .facial_lane(npc, next)
                .unwrap()
                .weight("jawOpen", 0.0),
            Some(1.0)
        );
        session.close();
        assert_eq!(session.face_time(npc), None);

        let legacy: Node =
            serde_json::from_str(r#"{"id":1,"text":"Привет","choices":[]}"#).unwrap();
        assert!(legacy.face.is_none());
        assert!(serde_json::to_value(&legacy).unwrap().get("face").is_none());
    }

    #[test]
    fn branches_accept_and_complete_a_visit_quest() {
        let story = Story::demo(12, [2.0, -5.0]);
        story.validate().unwrap();
        story.validate_npcs([12]).unwrap();
        let mut session = Session::default();
        assert!(session.open(&story, 12));
        assert!(session.choose(&story, 1));
        assert!(session.current(&story).unwrap().1.text.contains("комнаты"));
        assert!(session.choose(&story, 0));
        assert!(session.choose(&story, 0));
        assert_eq!(session.active_destination(&story), Some([2.0, -5.0]));
        session.update(&story, [2.0, -3.0]);
        assert!(session.objective(&story).unwrap().contains("Дойди"));
        session.update(&story, [2.1, -5.1]);
        assert!(session.objective(&story).unwrap().starts_with("Выполнено"));
        assert_eq!(session.active_destination(&story), None);
        assert!(session.choose(&story, 0));
        assert!(!session.is_open());
    }

    #[test]
    fn conversations_and_talk_quests_roundtrip_and_remove_dangling_targets() {
        let mut story = Story::demo(12, [0.0; 2]);
        story
            .conversations
            .push(Conversation::new(15, "Нина".into()));
        story.quests[0].objective = Objective::Talk { npc_id: Some(15) };
        story.quests[0].description.clear();
        let bytes = serde_json::to_vec(&story).unwrap();
        let mut loaded: Story = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(story, loaded);

        let mut session = Session::default();
        assert!(session.open(&loaded, 12));
        assert!(session.choose(&loaded, 0));
        assert!(session.objective(&loaded).unwrap().contains("Нина"));
        session.close();
        assert!(session.open(&loaded, 15));
        assert!(session.objective(&loaded).unwrap().starts_with("Выполнено"));

        loaded.remove_npc(15);
        assert_eq!(loaded.quests[0].objective, Objective::Talk { npc_id: None });
        let unreachable = loaded.conversations[0].nodes[1].id;
        loaded.conversations[0].remove_node(unreachable);
        assert_eq!(loaded.conversations[0].nodes[0].choices[0].target, None);
        loaded.remove_quest(1);
        assert_eq!(
            loaded.conversations[0].nodes[0].choices[0].starts_quest,
            None
        );
        loaded.validate().unwrap();
        loaded.validate_npcs([12]).unwrap();
    }

    #[test]
    fn talking_to_an_npc_without_dialogue_completes_active_quest_once() {
        let mut story = Story::demo(12, [0.0; 2]);
        story.quests[0].objective = Objective::Talk { npc_id: Some(19) };
        assert!(story.can_talk(19));
        assert!(story.validate_npcs([12]).is_err());
        story.validate_npcs([12, 19]).unwrap();
        let mut session = Session::default();
        assert!(!session.open(&story, 19));
        assert!(session.open(&story, 12));
        assert_eq!(session.active_npc(), Some(12));
        assert!(session.choose(&story, 0));
        session.close();
        assert!(session.open(&story, 19));
        assert!(!session.is_open());
        assert!(!session.open(&story, 19));
        assert!(session.objective(&story).unwrap().starts_with("Выполнено"));
    }

    #[test]
    fn entrance_requires_crossing_the_named_open_door_inward() {
        use crate::world::{DoorSettings, Interaction};

        let mut world = World::default();
        let goal = world.add_building_at([0.0, 0.0, 0.0]).unwrap();
        let other = world.add_building_at([12.0, 0.0, 0.0]).unwrap();
        let key = world.add_key_at([-3.8, 0.0, 0.0]).unwrap();
        let house = &mut world.buildings[0];
        house.quarter_turns = 1;
        house.door = Some(DoorSettings {
            initially_open: false,
            required_key: Some(key),
            ..Default::default()
        });
        let entrance = house.door_bounds();
        let near_outside = [entrance.max[0] + 0.18, 0.0, 0.0];
        let threshold = [entrance.min[0] - 0.10, 0.0, 0.0];
        let close_to_wall = [near_outside[0], 0.0, 1.52];
        let across_wall = [threshold[0], 0.0, 1.52];
        let mut story = Story::demo(12, [0.0; 2]);
        story.quests[0].objective = Objective::Enter { building_id: goal };
        let mut session = Session::default();
        assert!(session.open(&story, 12));
        assert!(session.choose(&story, 0));
        let [marker_x, marker_z] = session.active_destination_in(&story, &world).unwrap();
        assert!((marker_x - 1.62).abs() < 0.001 && marker_z == 0.0);

        let mut state = InteractionState::default();
        let step = |session: &mut Session, state: &InteractionState, from, to| {
            session.update_world(&story, &world, state, from, to)
        };
        assert!(!world.entry_crossed(&state, goal, near_outside, threshold));
        step(&mut session, &state, near_outside, threshold);
        assert!(!session.objective(&story).unwrap().starts_with("Выполнено"));
        assert_eq!(
            world.interact(&mut state, [-3.8, 0.0, 0.0]),
            Some(Interaction::Key { id: key })
        );
        assert!(world.door_locked(&state, world.buildings[0].entrance_id().unwrap()));
        assert!(matches!(
            world.interact(&mut state, near_outside),
            Some(Interaction::Door { opening: true, .. })
        ));
        for _ in 0..12 {
            world.advance_interactions(&mut state, 0.05, near_outside);
        }

        for (from, to) in [
            (near_outside, near_outside),
            (threshold, threshold),
            (threshold, near_outside),
            (close_to_wall, across_wall),
            ([12.0, 0.0, 2.45], [12.0, 0.0, 1.9]),
            ([near_outside[0], 3.1, 0.0], [threshold[0], 3.1, 0.0]),
        ] {
            step(&mut session, &state, from, to);
            assert!(!session.objective(&story).unwrap().starts_with("Выполнено"));
        }
        assert!(world.entry_crossed(&state, other, [12.0, 0.0, 2.45], [12.0, 0.0, 1.9]));
        step(&mut session, &state, near_outside, threshold);
        assert!(session.objective(&story).unwrap().starts_with("Выполнено"));
        assert!(session.active_destination_in(&story, &world).is_none());
    }
}
