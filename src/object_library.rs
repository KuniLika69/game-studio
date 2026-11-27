use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use glam::Vec3;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    assets::{Library, MAX_PLACEMENTS, Placement, Source},
    model::MAX_GLB_BYTES,
};

const MAGIC: &[u8; 12] = b"STUDIO-OBJ\0\x01";
const MAX_HEADER_BYTES: usize = 16 * 1024;
static SAVE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize)]
struct Header {
    placement: Placement,
    glb_size: u32,
}

pub(crate) struct PackedObject {
    pub placement: Placement,
    glb: Vec<u8>,
}

pub fn save(path: &Path, placement: &Placement, assets: &Library) -> Result<(), String> {
    validate_path(path)?;
    Placement::validate_all(std::slice::from_ref(placement))?;
    let glb = match placement.source {
        Source::Imported(_) => assets
            .export_glb(&placement.source)
            .ok_or("Модель объекта недоступна. Сначала восстанови её GLB.")?,
        _ => &[],
    };
    if glb.len() > MAX_GLB_BYTES {
        return Err("Превышен предел размера модели объекта.".into());
    }
    if path.exists() {
        let mut input =
            File::open(path).map_err(|error| format!("Не удалось открыть объект ({error})."))?;
        read_magic(&mut input)?;
    }
    let mut local = placement.clone();
    local.id = Placement::next_id(&[]);
    local.position = [0.0; 3];
    let header = serde_json::to_vec(&Header {
        placement: local,
        glb_size: glb.len() as u32,
    })
    .map_err(|error| error.to_string())?;
    if header.len() > MAX_HEADER_BYTES {
        return Err("Описание объекта слишком велико.".into());
    }

    let mut name = path.file_name().unwrap().to_os_string();
    let sequence = SAVE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    name.push(format!(".{}.{}.tmp", std::process::id(), sequence));
    let temporary = path.with_file_name(name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("Не удалось создать файл объекта ({error})."))?;
    let result = file
        .write_all(MAGIC)
        .and_then(|()| file.write_all(&(header.len() as u32).to_le_bytes()))
        .and_then(|()| file.write_all(&header))
        .and_then(|()| file.write_all(glb))
        .and_then(|()| file.sync_all());
    drop(file);
    if let Err(error) = result.and_then(|()| fs::rename(&temporary, path)) {
        let _ = fs::remove_file(temporary);
        return Err(format!("Не удалось сохранить объект ({error})."));
    }
    Ok(())
}

pub(crate) fn read(path: &Path) -> Result<PackedObject, String> {
    validate_path(path)?;
    let mut file =
        File::open(path).map_err(|error| format!("Не удалось открыть объект ({error})."))?;
    read_magic(&mut file)?;
    let mut size = [0; 4];
    file.read_exact(&mut size).map_err(read_error)?;
    let size = u32::from_le_bytes(size) as usize;
    if !(1..=MAX_HEADER_BYTES).contains(&size) {
        return Err("Некорректный размер описания объекта.".into());
    }
    let mut header_bytes = vec![0; size];
    file.read_exact(&mut header_bytes).map_err(read_error)?;
    let Header {
        placement,
        glb_size,
    } = serde_json::from_slice(&header_bytes)
        .map_err(|error| format!("Некорректное описание объекта ({error})."))?;
    Placement::validate_all(std::slice::from_ref(&placement))?;
    let glb_size = glb_size as usize;
    if glb_size > MAX_GLB_BYTES || matches!(placement.source, Source::Imported(_)) != (glb_size > 0)
    {
        return Err("Некорректный размер вложенной модели объекта.".into());
    }
    let mut glb = vec![0; glb_size];
    file.read_exact(&mut glb).map_err(read_error)?;
    if file.read(&mut [0]).map_err(read_error)? != 0 {
        return Err("После вложенной модели есть лишние данные.".into());
    }
    if let Source::Imported(id) = &placement.source
        && format!("{:x}", Sha256::digest(&glb)) != *id
    {
        return Err("Вложенная модель объекта повреждена.".into());
    }
    Ok(PackedObject { placement, glb })
}

pub fn insert(
    path: &Path,
    assets: &mut Library,
    models: &mut Vec<Placement>,
    foot: [f32; 3],
) -> Result<u64, String> {
    if models.len() >= MAX_PLACEMENTS {
        return Err("Слишком много моделей GLB в сцене.".into());
    }
    if !foot.iter().all(|axis| (-256.0..=256.0).contains(axis)) {
        return Err("Некорректная позиция объекта.".into());
    }
    let PackedObject { mut placement, glb } = read(path)?;
    placement.id = Placement::next_id(models);
    placement.position = [0.0; 3];
    Placement::validate_all(std::slice::from_ref(&placement))?;
    let already_loaded = assets.get(&placement.source).is_some();
    match placement.source {
        Source::Imported(_) => {
            assets.import_bytes(glb)?;
        }
        _ => assets.ensure(&placement.source, None)?,
    }
    let loaded = assets
        .get(&placement.source)
        .ok_or("Не удалось загрузить модель объекта.")?;
    let base = placement.bounds(&loaded.model);
    let base = Vec3::new(
        (base.min[0] + base.max[0]) * 0.5,
        base.min[1],
        (base.min[2] + base.max[2]) * 0.5,
    );
    placement.position = (Vec3::from_array(foot) - base).to_array();
    if let Err(error) = Placement::validate_all(std::slice::from_ref(&placement)) {
        if !already_loaded {
            assets.remove(&placement.source);
        }
        return Err(error);
    }
    let id = placement.id;
    models.push(placement);
    Ok(id)
}

fn validate_path(path: &Path) -> Result<(), String> {
    let valid = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("studio-object"));
    valid
        .then_some(())
        .ok_or("Укажи файл с расширением .studio-object.".into())
}

fn read_magic(file: &mut File) -> Result<(), String> {
    let mut magic = [0; MAGIC.len()];
    file.read_exact(&mut magic).map_err(read_error)?;
    (&magic == MAGIC)
        .then_some(())
        .ok_or("Формат объекта не поддерживается.".into())
}

fn read_error(error: std::io::Error) -> String {
    format!("Не удалось прочитать объект ({error}).")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        interior::{ModelCollider, SeatAnchor},
        scene::Scene,
    };

    const BENCH: &[u8] = include_bytes!("../demo_assets/quarter/bench_alt.glb");

    #[test]
    fn object_library_restores_glb_and_local_components_in_a_fresh_project() {
        let pack =
            std::env::temp_dir().join(format!("studio-bench-{}.studio-object", std::process::id()));
        let project = pack.with_extension("robot.json");
        let mut original = Library::default();
        let source = original.import_bytes(BENCH.to_vec()).unwrap();
        let loaded = original.get(&source).unwrap();
        assert!(!loaded.model.images.is_empty());
        let mut bench = Placement::at_ground(
            &[],
            &loaded.model,
            source.clone(),
            "Лавка".into(),
            [-8.0, 0.3, 5.0],
            1.7,
            90.0,
        );
        bench.visual_offset = [0.17, -0.04, 0.08];
        bench.material.uv_scale = [2.2, 0.9];
        bench.material.roughness = Some(0.38);
        bench.collider = Some(ModelCollider {
            center: [0.12, 0.34, -0.05],
            size: [1.56, 0.42, 0.51],
        });
        bench.seat = Some(SeatAnchor {
            sit: [0.09, 0.48, -0.20],
            stand: [0.27, 0.0, 1.17],
            yaw: 30.0,
        });
        save(&pack, &bench, &original).unwrap();
        drop(original);

        let mut destination = Scene::empty();
        let mut assets = Library::default();
        let at = [2.3, 0.1, -1.4];
        let id = insert(&pack, &mut assets, &mut destination.world.models, at).unwrap();
        let id2 = insert(
            &pack,
            &mut assets,
            &mut destination.world.models,
            [5.0, 0.0, 1.0],
        )
        .unwrap();
        assert_ne!(id, id2);
        fs::remove_file(&pack).unwrap();
        let inserted = &destination.world.models[0];
        let bounds = inserted.bounds(&assets.get(&source).unwrap().model);
        let foot = [
            (bounds.min[0] + bounds.max[0]) * 0.5,
            bounds.min[1],
            (bounds.min[2] + bounds.max[2]) * 0.5,
        ];
        assert!(Vec3::from_array(foot).distance(Vec3::from_array(at)) < 0.000_01);
        assert_eq!(assets.export_glb(&source), Some(BENCH));
        let mut local = inserted.clone();
        local.id = bench.id;
        local.position = bench.position;
        assert_eq!(local, bench);
        let displacement = Vec3::from_array(inserted.position) - Vec3::from_array(bench.position);
        let old = bench.seat_pose().unwrap();
        let new = inserted.seat_pose().unwrap();
        assert_eq!(old.yaw, new.yaw);
        assert!(
            (Vec3::from_array(new.stand) - Vec3::from_array(old.stand)).distance(displacement)
                < 0.000_01
        );

        assets
            .save_files(
                destination
                    .world
                    .models
                    .iter()
                    .map(|item| (&item.source, item.label.as_str())),
                &project,
            )
            .unwrap();
        destination.save(&project).unwrap();
        let opened = Scene::load(&project).unwrap();
        assert_eq!(opened, destination);
        let mut reopened_assets = Library::default();
        reopened_assets.ensure(&source, Some(&project)).unwrap();
        assert_eq!(reopened_assets.export_glb(&source), Some(BENCH));

        let Source::Imported(digest) = source else {
            unreachable!()
        };
        let directory = project.with_extension("assets");
        fs::remove_file(directory.join(format!("{digest}.glb"))).unwrap();
        fs::remove_dir(directory).unwrap();
        fs::remove_file(project).unwrap();
    }

    #[test]
    fn damaged_payload_is_rejected_before_any_placement_is_created() {
        let path = std::env::temp_dir().join(format!(
            "studio-bench-corrupt-{}.studio-object",
            std::process::id()
        ));
        let mut source_assets = Library::default();
        let source = source_assets.import_bytes(BENCH.to_vec()).unwrap();
        let instance = Placement::at_ground(
            &[],
            &source_assets.get(&source).unwrap().model,
            source.clone(),
            "Скамья".into(),
            [0.0; 3],
            2.0,
            0.0,
        );
        save(&path, &instance, &source_assets).unwrap();
        let mut bytes = fs::read(&path).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        fs::write(&path, bytes).unwrap();
        let mut assets = Library::default();
        let mut objects = Vec::new();
        assert!(insert(&path, &mut assets, &mut objects, [0.0; 3]).is_err());
        assert!(objects.is_empty() && assets.get(&source).is_none());
        fs::remove_file(path).unwrap();
    }
}
