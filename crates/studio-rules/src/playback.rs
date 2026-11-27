use serde::{Deserialize, Serialize};
use std::path::Path;

const FORMAT: u32 = 1;
const MAX_TICKS: u32 = 60 * 60 * 10;
const MAX_FILE: u64 = 8 * 1024 * 1024;
const MAX_MARKS: usize = 100;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Intent {
    pub dt: f32,
    pub movement: [f32; 2],
    pub sprint: bool,
    pub jump: bool,
    pub interact: bool,
    pub mark: bool,
    pub retry: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<[f32; 2]>,
}

impl Intent {
    fn valid(self) -> bool {
        (0.0..=0.05).contains(&self.dt)
            && self.movement.iter().all(|v| (-1.5..=1.5).contains(v))
            && self
                .target
                .is_none_or(|target| target.iter().all(|v| (-100_000.0..=100_000.0).contains(v)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub npc: u64,
    pub node: u32,
    pub option: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Span {
    ticks: u16,
    intent: Intent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Answer {
    tick: u32,
    choice: Choice,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Probe {
    Counter(u32),
    Flag(u32),
    Timer(u32),
    Door(u64),
    Object(u64),
    Quest(u32),
    Keys,
    Cell,
    Location,
    AnswerText { npc: u64, node: u32, option: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Value {
    Number(i32),
    Switch(bool),
    Cell([i32; 2]),
    Inventory(Vec<u32>),
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub probe: Probe,
    pub label: String,
    pub value: Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Expectation {
    pub tick: u32,
    pub name: String,
    pub state: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Check {
    format: u32,
    pub title: String,
    spans: Vec<Span>,
    answers: Vec<Answer>,
    pub expectations: Vec<Expectation>,
    pub ticks: u32,
}

impl Check {
    pub fn new(title: String) -> Self {
        Self {
            format: FORMAT,
            title,
            spans: Vec::new(),
            answers: Vec::new(),
            expectations: Vec::new(),
            ticks: 0,
        }
    }

    pub fn record(&mut self, intent: Intent) -> Result<(), String> {
        if self.ticks >= MAX_TICKS {
            return Err("Запись достигла лимита в 36 000 шагов симуляции.".into());
        }
        if !intent.valid() {
            return Err("Записано недопустимое игровое действие.".into());
        }
        if let Some(last) = self
            .spans
            .last_mut()
            .filter(|last| last.intent == intent && last.ticks < u16::MAX)
        {
            last.ticks += 1;
        } else {
            self.spans.push(Span { ticks: 1, intent });
        }
        self.ticks += 1;
        Ok(())
    }

    pub fn answer(&mut self, choice: Choice) {
        if self.ticks > 0 {
            self.answers.push(Answer {
                tick: self.ticks,
                choice,
            });
        }
    }

    pub fn expect(&mut self, name: String, mut state: Vec<Entry>) -> Result<(), String> {
        if self.expectations.len() >= MAX_MARKS
            || self.ticks == 0
            || state.is_empty()
            || name.trim().is_empty()
            || name.chars().count() > 100
        {
            return Err("Для отметки нужны название, шаг симуляции и проверяемое состояние. Лимит — 100 отметок.".into());
        }
        state.sort_by_key(|entry| entry.probe);
        self.expectations.push(Expectation {
            tick: self.ticks,
            name,
            state,
        });
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|err| err.to_string())?;
        let mut bytes = Vec::new();
        file.take(MAX_FILE + 1)
            .read_to_end(&mut bytes)
            .map_err(|err| err.to_string())?;
        if bytes.len() as u64 > MAX_FILE {
            return Err("Файл проверки превышает 8 МБ.".into());
        }
        let check = serde_json::from_slice::<Self>(&bytes).map_err(|err| err.to_string())?;
        check.validate()?;
        Ok(check)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|err| err.to_string())?;
        if bytes.len() as u64 > MAX_FILE {
            return Err("Файл проверки превышает 8 МБ.".into());
        }
        std::fs::write(path, bytes).map_err(|err| err.to_string())
    }

    fn validate(&self) -> Result<(), String> {
        let in_order = |ticks: &mut dyn Iterator<Item = u32>| {
            let mut prior = 0;
            for tick in ticks {
                if tick == 0 || tick < prior || tick > self.ticks {
                    return false;
                }
                prior = tick;
            }
            true
        };
        if self.format != FORMAT
            || self.ticks == 0
            || self.ticks > MAX_TICKS
            || self.title.trim().is_empty()
            || self.title.chars().count() > 100
            || self.spans.len() > self.ticks as usize
            || self
                .spans
                .iter()
                .any(|span| span.ticks == 0 || !span.intent.valid())
            || self
                .spans
                .iter()
                .map(|span| u64::from(span.ticks))
                .sum::<u64>()
                != u64::from(self.ticks)
            || self.answers.len() > self.ticks as usize
            || !in_order(&mut self.answers.iter().map(|answer| answer.tick))
            || self.expectations.is_empty()
            || self.expectations.len() > MAX_MARKS
            || !in_order(&mut self.expectations.iter().map(|expectation| expectation.tick))
            || self.expectations.iter().any(|row| {
                row.name.trim().is_empty()
                    || row.name.chars().count() > 100
                    || row.state.is_empty()
                    || row.state.len() > 1024
                    || row
                        .state
                        .windows(2)
                        .any(|neighbors| neighbors[0].probe >= neighbors[1].probe)
            })
        {
            return Err("Некорректный сценарий проверки.".into());
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct Cursor {
    pub tick: u32,
    span: usize,
    offset: u16,
    answer: usize,
    mark: usize,
}

impl Cursor {
    pub fn step(&mut self, check: &Check) -> Option<Intent> {
        let span = check.spans.get(self.span)?;
        self.tick += 1;
        self.offset += 1;
        if self.offset == span.ticks {
            self.span += 1;
            self.offset = 0;
        }
        Some(span.intent)
    }

    pub fn next_choice(&mut self, check: &Check) -> Option<Choice> {
        let answer = check
            .answers
            .get(self.answer)
            .filter(|answer| answer.tick == self.tick)?;
        self.answer += 1;
        Some(answer.choice)
    }

    pub fn next_mark<'a>(&mut self, check: &'a Check) -> Option<&'a Expectation> {
        let mark = check
            .expectations
            .get(self.mark)
            .filter(|mark| mark.tick == self.tick)?;
        self.mark += 1;
        Some(mark)
    }
}

pub struct Difference<'a> {
    pub entry: &'a Entry,
    pub actual: Option<Value>,
}

pub fn compare(
    mark: &Expectation,
    mut read: impl FnMut(Probe) -> Option<Value>,
) -> Vec<Difference<'_>> {
    mark.state
        .iter()
        .filter_map(|entry| {
            let actual = read(entry.probe);
            (actual.as_ref() != Some(&entry.value)).then_some(Difference { entry, actual })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_preserves_ticks_choices_and_first_semantic_divergence() {
        let mut check = Check::new("Терраса".into());
        let walk = Intent {
            dt: 1.0 / 60.0,
            movement: [1.0, 0.0],
            ..Intent::default()
        };
        for _ in 0..5 {
            check.record(walk).unwrap();
        }
        let choice = Choice {
            npc: 9,
            node: 1,
            option: 2,
        };
        check.answer(choice);
        check
            .expect(
                "отметка".into(),
                vec![Entry {
                    probe: Probe::Counter(5),
                    label: "Отметки".into(),
                    value: Value::Number(1),
                }],
            )
            .unwrap();
        check.record(Intent { mark: true, ..walk }).unwrap();
        assert_eq!(check.spans.len(), 2);
        check.validate().unwrap();
        let mut cursor = Cursor::default();
        for _ in 0..4 {
            assert_eq!(cursor.step(&check), Some(walk));
            assert!(cursor.next_choice(&check).is_none());
            assert!(cursor.next_mark(&check).is_none());
        }
        assert_eq!(cursor.step(&check), Some(walk));
        assert_eq!(cursor.next_choice(&check), Some(choice));
        let mark = cursor.next_mark(&check).unwrap();
        let differences = compare(mark, |_| Some(Value::Number(2)));
        assert_eq!(differences[0].actual, Some(Value::Number(2)));
        assert!(compare(mark, |_| Some(Value::Number(1))).is_empty());
        assert!(cursor.step(&check).unwrap().mark);
        assert!(cursor.step(&check).is_none());
    }
}
