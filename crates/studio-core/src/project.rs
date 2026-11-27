use serde::{Serialize, de::DeserializeOwned};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::ImportError;

const MAX_PROJECT_BYTES: u64 = 24 * 1024 * 1024;
static SAVE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn save(path: &Path, project: &impl Serialize) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(project).map_err(|error| error.to_string())?;
    if data.len() as u64 > MAX_PROJECT_BYTES {
        return Err("Файл проекта больше 24 МБ".to_owned());
    }
    write_atomic(path, &data).map_err(|error| format!("Не удалось сохранить проект ({error})"))
}

pub fn load<T: DeserializeOwned>(path: &Path) -> Result<T, ImportError> {
    let file = File::open(path).map_err(|error| ImportError::read(path, error))?;
    let mut data = Vec::new();
    file.take(MAX_PROJECT_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|error| ImportError::read(path, error))?;
    if data.len() as u64 > MAX_PROJECT_BYTES {
        return Err(ImportError::new("Проект", "Файл проекта больше 24 МБ").with_file(path));
    }
    serde_json::from_slice(&data).map_err(|error| {
        ImportError::new(
            format!("JSON, строка {}, столбец {}", error.line(), error.column()),
            format!("Неверный формат проекта ({error})"),
        )
        .with_file(path)
    })
}

pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut temporary_name = path
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "empty file name"))?
        .to_os_string();
    let sequence = SAVE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    temporary_name.push(format!(".{}.{}.tmp", std::process::id(), sequence));
    let temporary = path.with_file_name(temporary_name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = file.write_all(data).and_then(|()| file.sync_all());
    drop(file);
    let result = result.and_then(|()| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
