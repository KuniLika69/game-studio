//! Replay a suite on the runtime loaded from two separately exported Windows executables.
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    Scene,
    assets::Library,
    game_export::{self, GamePackage},
    playback::Check,
    studio_playback::{Isolated, ResultSummary},
};

const MAX_REPORT: u64 = 2 * 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(1);

pub(crate) use studio_core::suite::{Case, Suite};
use studio_core::suite::read_json;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct CaseResult {
    pub name: String,
    pub restored_slot: bool,
    pub step: u32,
    pub outcome: ResultSummary,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Comparison {
    pub folder: PathBuf,
    pub original: Vec<CaseResult>,
    pub edited: Vec<CaseResult>,
    pub elapsed_ms: u128,
}

impl Comparison {
    pub fn approved(&self) -> bool {
        !self.original.is_empty()
            && self.original.len() == self.edited.len()
            && self
                .original
                .iter()
                .chain(&self.edited)
                .all(|case| case.outcome.passed)
    }
}

pub(crate) fn compare(
    original: &Scene,
    edited: &Scene,
    library: &Library,
    in_plan: bool,
    suite: &Path,
) -> Result<Comparison, String> {
    let started = Instant::now();
    let suite = Suite::load(suite)?;
    let root = crate::editor_history::local_directory()
        .ok_or("Локальная папка недоступна.")?
        .join("game-trials");
    fs::create_dir_all(&root).map_err(|err| err.to_string())?;
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let folder = root.join(format!(
        "{time}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&folder).map_err(|err| err.to_string())?;
    let manifest = folder.join("checks.suite.json");
    write_json(&manifest, &suite)?;
    let run = |name: &str, scene: &Scene, revision: &str| -> Result<Vec<CaseResult>, String> {
        let executable = folder.join(format!("{name}.exe"));
        game_export::export(&executable, scene, library, in_plan)?;
        let report = folder.join(format!("{name}.json"));
        launch(
            &executable,
            "--background-check-package",
            &[manifest.as_os_str(), revision.as_ref(), report.as_os_str()],
        )?;
        read_json(&report, MAX_REPORT)
    };
    let comparison = Comparison {
        original: run("Original", original, "original")?,
        edited: run("Edited", edited, "edited")?,
        elapsed_ms: started.elapsed().as_millis(),
        folder,
    };
    write_json(&comparison.folder.join("comparison.json"), &comparison)?;
    Ok(comparison)
}

pub(crate) fn audit_package(
    program: &Path,
    suite: &Path,
    output: &Path,
) -> Result<Vec<CaseResult>, String> {
    launch(
        program,
        "--background-check-package",
        &[suite.as_os_str(), "edited".as_ref(), output.as_os_str()],
    )?;
    read_json(output, MAX_REPORT)
}
pub(crate) fn run(
    package: GamePackage,
    suite: &Path,
    revision: &str,
    output: &Path,
) -> Result<(), String> {
    let use_edited = match revision {
        "original" => false,
        "edited" => true,
        _ => return Err("Ожидалась исходная или изменённая сборка.".into()),
    };
    let suite = Suite::load(suite)?;
    let rows = suite
        .cases
        .into_iter()
        .map(|case| {
            let mut restored_slot = false;
            let run = (|| {
                let file = (if use_edited {
                    case.edited.as_ref()
                } else {
                    None
                })
                .unwrap_or(&case.original);
                let check = Check::load(file)?;
                let state = case
                    .slot
                    .as_deref()
                    .map(|path| crate::game_save::restore(path, &package.scene.linked_scenes))
                    .transpose()?;
                let mut playback = Isolated::packaged(package.clone(), check, state)?;
                restored_slot = case.slot.is_some();
                loop {
                    if let Some(summary) = playback.step() {
                        return Ok((playback.progress().0, summary));
                    }
                }
            })();
            let (step, outcome) = run.unwrap_or_else(|error| {
                (
                    0,
                    ResultSummary {
                        passed: false,
                        messages: vec![error],
                    },
                )
            });
            CaseResult {
                name: case.name,
                restored_slot,
                step,
                outcome,
            }
        })
        .collect::<Vec<_>>();
    write_json(output, &rows)
}

pub(crate) fn capture(package: GamePackage, check: &Path, output: &Path) -> Result<(), String> {
    let mut playback = Isolated::packaged(package, Check::load(check)?, None)?;
    loop {
        if let Some(summary) = playback.step() {
            if !summary.passed {
                return Err(summary.messages.join("\n"));
            }
            return write_json(output, &playback.checkpoint()?);
        }
    }
}

pub(crate) fn capture_slot(program: &Path, check: &Path, output: &Path) -> Result<(), String> {
    launch(
        program,
        "--background-capture-trial-slot",
        &[check.as_os_str(), output.as_os_str()],
    )
}

fn launch(program: &Path, mode: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let mut command = Command::new(program);
    command
        .arg(mode)
        .args(args)
        .current_dir(program.parent().ok_or("Нет папки игры.")?)
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    let path = program.with_extension("log");
    command.stderr(File::create(&path).map_err(|error| error.to_string())?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return if status.success() {
                Ok(())
            } else {
                Err(format!(
                    "Проверка игры завершилась с кодом {status}. Журнал {}",
                    path.display()
                ))
            };
        }
        if started.elapsed() >= Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "Проверка игры не уложилась в минуту. Журнал {}",
                path.display()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn write_json<T: Serialize + ?Sized>(path: &Path, data: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(data).map_err(|error| error.to_string())?;
    crate::scene::write_atomic(path, &bytes).map_err(|error| error.to_string())
}
