use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

const MAX_SUITE: u64 = 128 * 1024;
const MAX_CASES: usize = 12;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Suite {
    pub version: u8,
    pub cases: Vec<Case>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    pub original: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<PathBuf>,
}

impl Suite {
    pub fn load(path: &Path) -> Result<Self, String> {
        let mut suite: Self = read_json(path, MAX_SUITE)?;
        if suite.version != 1 || !(1..=MAX_CASES).contains(&suite.cases.len()) {
            return Err(format!(
                "Набор версии 1 должен содержать от 1 до {MAX_CASES} сценариев."
            ));
        }
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        for case in &mut suite.cases {
            if case.name.trim().is_empty()
                || case.name.chars().count() > 96
                || case.name.chars().any(char::is_control)
            {
                return Err("У сценария должно быть имя до 96 символов.".into());
            }
            for file in std::iter::once(&mut case.original)
                .chain(case.edited.as_mut())
                .chain(case.slot.as_mut())
            {
                *file = dir.join(&*file).canonicalize().map_err(|error| {
                    format!("Сценарий «{}», файл {}. {error}", case.name, file.display())
                })?;
            }
        }
        Ok(suite)
    }
}

pub fn read_json<T: DeserializeOwned>(path: &Path, limit: u64) -> Result<T, String> {
    let file = File::open(path).map_err(|error| format!("{}. {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Файл проверки превышает допустимый размер.".into());
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("{}. {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previous_release_suite_round_trips_without_changing_the_wire_format() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../verification/v0.23/trial/manual/junction.suite.json");
        let original = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        let suite: Suite = read_json(&path, MAX_SUITE).unwrap();
        assert_eq!(
            serde_json::to_vec_pretty(&suite).unwrap(),
            original.as_bytes()
        );
        assert!(
            Suite::load(&path)
                .unwrap()
                .cases
                .into_iter()
                .all(|case| case.original.is_file())
        );
    }
}
