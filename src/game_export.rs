use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    ImportError,
    asset_sources,
    assets::{Library, Source},
    model::MAX_GLB_BYTES,
    scene::Scene,
};

const MAGIC: [u8; 16] = *b"RS_GAME_PACKAGE!";
const FORMAT_VERSION: u32 = 1;
const FOOTER_BYTES: usize = 16 + 4 + 8 + 32;
const MAX_MANIFEST_BYTES: usize = 24 * 1024 * 1024;
const MAX_PAYLOAD_BYTES: u64 = 288 * 1024 * 1024;
const IMPORT_ID_BYTES: usize = 64;
static EXPORT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct GamePackage {
    pub scene: Scene,
    pub assets: Library,
    pub start_in_2d: bool,
    pub(crate) checksum: [u8; 32],
}

impl GamePackage {
    pub fn read(path: &Path) -> Result<Option<Self>, ImportError> {
        let mut file = File::open(path).map_err(|error| ImportError::read(path, error))?;
        read_package(&mut file).map_err(|error| error.with_file(path))
    }
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    scene: Scene,
    start_in_2d: bool,
}

struct Footer {
    start: u64,
    bytes: u64,
    checksum: [u8; 32],
}

/// A plain editor executable has no game footer. Games read their resources from the
/// executable itself, independently of the working directory or the original project.
pub fn load_current() -> Result<Option<GamePackage>, ImportError> {
    let path = std::env::current_exe().map_err(|error| {
        ImportError::new("Путь EXE", format!("Не удалось найти файл игры ({error})."))
    })?;
    GamePackage::read(&path)
}

pub fn export(
    path: &Path,
    scene: &Scene,
    library: &Library,
    start_in_2d: bool,
) -> Result<(), String> {
    if !path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
    {
        return Err("Укажи имя игры с расширением .exe.".into());
    }
    let name = path.file_name().ok_or("Укажи имя файла игры.")?;
    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|error| format!("Не удалось открыть папку экспорта ({error})."))?;
    let output = dir.join(name);
    let executable = std::env::current_exe()
        .map_err(|error| format!("Не удалось найти файл редактора ({error})."))?;
    if let Ok(existing) = output.canonicalize()
        && existing
            == executable
                .canonicalize()
                .unwrap_or_else(|_| executable.clone())
    {
        return Err("Выбери для игры другое имя. Этот файл запускает редактор.".into());
    }
    match fs::symlink_metadata(&output) {
        Ok(_) => return Err("Файл с таким именем уже существует. Укажи другое имя игры.".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Не удалось проверить файл экспорта ({error}).")),
    }

    let manifest = manifest_bytes(scene, start_in_2d)?;
    let assets = collect_assets(scene, library)?;
    let payload_bytes = payload_length(&manifest, &assets)?;

    let mut source = File::open(&executable)
        .map_err(|error| format!("Не удалось прочитать файл редактора ({error})."))?;
    let mut pe_magic = [0; 2];
    source.read_exact(&mut pe_magic).map_err(read_error)?;
    if pe_magic != *b"MZ" {
        return Err("Для экспорта игры нужен Windows-редактор (.exe).".into());
    }
    if read_footer(&mut source)?.is_some() {
        return Err("Экспорт запускается из редактора.".into());
    }
    source.seek(SeekFrom::Start(0)).map_err(read_error)?;

    let sequence = EXPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = dir.join(format!(
        ".studio-game-{}-{sequence}.tmp",
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("Не удалось создать временный файл игры ({error})."))?;
    let result = io::copy(&mut source, &mut file)
        .and_then(|_| write_package(&mut file, &manifest, &assets, payload_bytes))
        .and_then(|()| file.sync_all());
    drop(file);
    if let Err(error) = result.and_then(|()| move_new_file(&temporary, &output)) {
        let _ = fs::remove_file(&temporary);
        return Err(if error.kind() == io::ErrorKind::AlreadyExists {
            "Файл с таким именем уже существует. Укажи другое имя игры.".into()
        } else {
            format!("Не удалось сохранить файл игры ({error}).")
        });
    }
    Ok(())
}

fn manifest_bytes(scene: &Scene, start_in_2d: bool) -> Result<Vec<u8>, String> {
    let mut scene = scene.clone();
    scene.checkpoint = None;
    for link in &mut scene.linked_scenes.entries {
        link.scene.checkpoint = None;
    }
    scene.validate()?;
    let manifest = serde_json::to_vec(&Manifest { scene, start_in_2d })
        .map_err(|error| format!("Не удалось упаковать сцену ({error})."))?;
    if manifest.len() > MAX_MANIFEST_BYTES {
        return Err("Сцена игры больше допустимых 24 МБ.".into());
    }
    Ok(manifest)
}

fn imports(scene: &Scene) -> BTreeMap<&str, (&Source, &str)> {
    asset_sources(scene)
        .filter_map(|(source, label)| match source {
            Source::Imported(id) => Some((id.as_str(), (source, label))),
            _ => None,
        })
        .collect()
}

fn collect_assets<'a>(
    scene: &'a Scene,
    library: &'a Library,
) -> Result<Vec<(&'a str, &'a [u8])>, String> {
    imports(scene)
        .into_iter()
        .map(|(id, (source, label))| {
            let bytes = library.export_glb(source).ok_or_else(|| {
                format!("Модель «{label}» отсутствует. Восстанови её GLB перед экспортом игры.")
            })?;
            if bytes.len() > MAX_GLB_BYTES {
                return Err("Модель GLB больше допустимых 64 МБ.".into());
            }
            Ok((id, bytes))
        })
        .collect()
}

fn payload_length(manifest: &[u8], assets: &[(&str, &[u8])]) -> Result<u64, String> {
    let bytes = 8
        + manifest.len() as u64
        + assets
            .iter()
            .map(|(_, bytes)| (IMPORT_ID_BYTES + 4) as u64 + bytes.len() as u64)
            .sum::<u64>();
    if bytes > MAX_PAYLOAD_BYTES {
        return Err("Ресурсы игры больше допустимых 288 МБ.".into());
    }
    Ok(bytes)
}

fn write_package(
    output: &mut impl Write,
    manifest: &[u8],
    assets: &[(&str, &[u8])],
    payload_bytes: u64,
) -> io::Result<()> {
    let mut hash = Sha256::new();
    let mut write = |bytes: &[u8]| -> io::Result<()> {
        hash.update(bytes);
        output.write_all(bytes)
    };
    write(&(manifest.len() as u32).to_le_bytes())?;
    write(manifest)?;
    write(&(assets.len() as u32).to_le_bytes())?;
    for (id, bytes) in assets {
        write(id.as_bytes())?;
        write(&(bytes.len() as u32).to_le_bytes())?;
        write(bytes)?;
    }
    output.write_all(&MAGIC)?;
    output.write_all(&FORMAT_VERSION.to_le_bytes())?;
    output.write_all(&payload_bytes.to_le_bytes())?;
    output.write_all(&hash.finalize())
}

fn read_footer(input: &mut (impl Read + Seek)) -> Result<Option<Footer>, ImportError> {
    let len = input.seek(SeekFrom::End(0)).map_err(package_read_error)?;
    if len < FOOTER_BYTES as u64 {
        return Ok(None);
    }
    input
        .seek(SeekFrom::End(-(FOOTER_BYTES as i64)))
        .map_err(package_read_error)?;
    let mut magic = [0; MAGIC.len()];
    input.read_exact(&mut magic).map_err(package_read_error)?;
    if magic != MAGIC {
        return Ok(None);
    }
    let version = read_u32(input)?;
    if version != FORMAT_VERSION {
        return Err(ImportError::new(
            "Версия встроенного пакета",
            format!("Версия пакета игры {version} пока не поддерживается."),
        ));
    }
    let mut bytes = [0; 8];
    input.read_exact(&mut bytes).map_err(package_read_error)?;
    let bytes = u64::from_le_bytes(bytes);
    let start = (8..=MAX_PAYLOAD_BYTES)
        .contains(&bytes)
        .then(|| len.checked_sub(FOOTER_BYTES as u64 + bytes))
        .flatten()
        .filter(|&start| start >= 2)
        .ok_or("Некорректный размер встроенной игры.")?;
    let mut checksum = [0; 32];
    input.read_exact(&mut checksum).map_err(package_read_error)?;
    Ok(Some(Footer {
        start,
        bytes,
        checksum,
    }))
}

fn read_package(input: &mut (impl Read + Seek)) -> Result<Option<GamePackage>, ImportError> {
    let Some(footer) = read_footer(input)? else {
        return Ok(None);
    };
    input
        .seek(SeekFrom::Start(footer.start))
        .map_err(package_read_error)?;
    let mut payload = (&mut *input).take(footer.bytes);
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let len = payload.read(&mut buffer).map_err(package_read_error)?;
        if len == 0 {
            break;
        }
        hash.update(&buffer[..len]);
    }
    let actual: [u8; 32] = hash.finalize().into();
    if payload.limit() != 0 || actual != footer.checksum {
        return Err(ImportError::new(
            "Контрольная сумма пакета",
            "Встроенная сцена или ресурсы игры повреждены.",
        ));
    }
    input
        .seek(SeekFrom::Start(footer.start))
        .map_err(package_read_error)?;
    let mut payload = input.take(footer.bytes);
    let manifest_size = read_u32(&mut payload)? as usize;
    if !(1..=MAX_MANIFEST_BYTES).contains(&manifest_size) || manifest_size as u64 > payload.limit()
    {
        return Err("Некорректный размер встроенной сцены.".into());
    }
    let mut manifest = vec![0; manifest_size];
    payload.read_exact(&mut manifest).map_err(package_read_error)?;
    let Manifest { scene, start_in_2d } = serde_json::from_slice(&manifest).map_err(|error| {
        ImportError::new(
            format!("Сцена в EXE, строка {}, столбец {}", error.line(), error.column()),
            format!("Неверный формат сцены ({error})"),
        )
    })?;
    drop(manifest);
    scene.validate_import()?;

    let mut expected = imports(&scene);
    let count = read_u32(&mut payload)? as usize;
    if count != expected.len() {
        return Err("Набор встроенных моделей не соответствует сцене.".into());
    }
    let mut assets = Library::default();
    for _ in 0..count {
        let mut id = [0; IMPORT_ID_BYTES];
        payload.read_exact(&mut id).map_err(package_read_error)?;
        let id = std::str::from_utf8(&id).map_err(|_| "Некорректный идентификатор модели.")?;
        let (source, label) = expected
            .remove(id)
            .ok_or("Неизвестная или повторная модель.")?;
        let size = read_u32(&mut payload)? as usize;
        if !(1..=MAX_GLB_BYTES).contains(&size) || size as u64 > payload.limit() {
            return Err(ImportError::new(
                format!("Модель «{label}»"),
                "Некорректный размер.",
            ));
        }
        let mut bytes = vec![0; size];
        payload.read_exact(&mut bytes).map_err(package_read_error)?;
        assets
            .insert_exported_glb(source, bytes)
            .map_err(|error| ImportError::new(format!("Модель «{label}» в EXE"), error))?;
    }
    if payload.limit() != 0 {
        return Err("После встроенных моделей остались лишние данные.".into());
    }
    Ok(Some(GamePackage {
        scene,
        assets,
        start_in_2d,
        checksum: footer.checksum,
    }))
}

fn read_u32(input: &mut impl Read) -> Result<u32, ImportError> {
    let mut value = [0; 4];
    input.read_exact(&mut value).map_err(package_read_error)?;
    Ok(u32::from_le_bytes(value))
}

fn read_error(error: io::Error) -> String {
    format!("Не удалось прочитать встроенную игру ({error}).")
}

fn package_read_error(error: io::Error) -> ImportError {
    ImportError::new("Чтение встроенного пакета", error)
}

#[cfg(windows)]
fn move_new_file(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileW(current: *const u16, destination: *const u16) -> i32;
    }

    let native_path = |path: &Path| -> io::Result<Vec<u16>> {
        let mut wide: Vec<_> = path.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid file path",
            ));
        }
        wide.push(0);
        Ok(wide)
    };
    let from = native_path(from)?;
    let to = native_path(to)?;
    // Both paths are NUL-terminated and stay alive for the call. This move fails if the
    // destination appeared during export, preserving the existing file.
    if unsafe { MoveFileW(from.as_ptr(), to.as_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
fn move_new_file(from: &Path, to: &Path) -> io::Result<()> {
    fs::hard_link(from, to)?;
    let _ = fs::remove_file(from);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        assets::Placement,
        scene::{Checkpoint, Shape},
    };
    use std::io::Cursor;

    fn assert_bad_package(file: &str, reason: &str) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/a1_scene")
            .join(file);
        let error: ImportError = match GamePackage::read(&path) {
            Err(error) => error,
            Ok(_) => panic!("битый пакет принят"),
        };
        assert!(error.to_string().contains(file), "{error}");
        assert!(error.to_string().contains(reason), "{error}");
    }

    #[test]
    fn a1_unknown_game_package_version_is_rejected() {
        assert_bad_package("unsupported-version.package", "Версия пакета игры 2");
    }

    #[test]
    fn a1_modified_game_package_payload_is_rejected() {
        assert_bad_package("checksum-mismatch.package", "Контрольная сумма пакета");
    }

    #[test]
    fn game_package_includes_2d_atlas_pixels_and_map_collisions() {
        use crate::map2d::{self, Collision, Layer, Mode, Tile};

        let mut scene = map2d::scene(Mode::Top, false);
        let map = scene.map_2d.as_mut().unwrap();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo_assets/map2d/props.png");
        let imported = map.import_png(&path, Some([32, 64])).unwrap();
        let tile = Tile {
            atlas: imported,
            index: 0,
        };
        assert!(map.paint([5, 7], Layer::Tall, Some(tile)));
        assert!(map.paint_collision([5, 7], Collision::Solid));
        let expected = scene.map_2d.clone().unwrap();

        let manifest = manifest_bytes(&scene, true).unwrap();
        let mut exe = b"MZ".to_vec();
        write_package(
            &mut exe,
            &manifest,
            &[],
            payload_length(&manifest, &[]).unwrap(),
        )
        .unwrap();
        let package = read_package(&mut Cursor::new(exe)).unwrap().unwrap();
        assert!(package.start_in_2d);
        let loaded = package.scene.map_2d.unwrap();
        assert_eq!(loaded, expected);
        assert_eq!(loaded.atlas(imported).unwrap().png, fs::read(path).unwrap());
        assert_eq!(loaded.collision_at([5, 7]), Collision::Solid);
    }

    #[test]
    fn package_preserves_scene_and_glb_without_external_files() {
        let glb = include_bytes!("../demo_assets/crate.glb");
        let source = Source::Imported(format!("{:x}", Sha256::digest(glb)));
        let mut library = Library::default();
        library.insert_exported_glb(&source, glb.to_vec()).unwrap();
        let mut scene = Scene::empty();
        scene.add_part(Shape::Box);
        scene.checkpoint = Some(Checkpoint {
            position: [1.0, 0.0, -1.0],
            scene_id: None,
            rotor_angle: None,
            tactics: None,
            environment: None,
            physics: None,
            map_respawn: None,
            story: Default::default(),
            interactions: Default::default(),
            rules: Default::default(),
            visited: Default::default(),
        });
        for x in [0.0, 2.0] {
            scene.world.models.push(Placement::at_ground(
                &scene.world.models,
                &library.get(&source).unwrap().model,
                source.clone(),
                "Ящик".into(),
                [x, 0.0, 0.0],
                1.0,
                0.0,
            ));
        }
        assert!(collect_assets(&scene, &Library::default()).is_err());
        let manifest = manifest_bytes(&scene, true).unwrap();
        let assets = collect_assets(&scene, &library).unwrap();
        assert_eq!(assets.len(), 1);
        let mut exe = vec![0; 120];
        exe[..2].copy_from_slice(b"MZ");
        write_package(
            &mut exe,
            &manifest,
            &assets,
            payload_length(&manifest, &assets).unwrap(),
        )
        .unwrap();

        let package = read_package(&mut Cursor::new(&exe)).unwrap().unwrap();
        assert!(package.start_in_2d);
        assert!(scene.checkpoint.is_some());
        scene.checkpoint = None;
        assert_eq!(package.scene, scene);
        let mut assets = package.assets;
        assets.ensure(&source, None).unwrap();
        assert_eq!(assets.export_glb(&source).unwrap(), glb);

        exe[128] ^= 1;
        assert!(read_package(&mut Cursor::new(exe)).is_err());
        assert!(read_package(&mut Cursor::new([0; 128])).unwrap().is_none());
    }

    #[test]
    fn committing_an_export_does_not_replace_existing_files() {
        let sequence = EXPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "studio-export-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap();
        let from = directory.join("writing.tmp");
        let to = directory.join("game.exe");
        fs::write(&from, b"export").unwrap();
        fs::write(&to, b"existing").unwrap();
        assert!(move_new_file(&from, &to).is_err());
        assert_eq!(fs::read(&to).unwrap(), b"existing");
        fs::remove_file(&to).unwrap();
        move_new_file(&from, &to).unwrap();
        assert!(!from.exists());
        assert_eq!(fs::read(&to).unwrap(), b"export");
        fs::remove_file(&to).unwrap();
        fs::remove_dir(&directory).unwrap();
    }
}
