use super::*;

impl World {
    pub(crate) fn duplicate_selection(
        &mut self,
        ids: &[u64],
        offset: [f32; 3],
    ) -> Result<Vec<u64>, String> {
        let mut next = self.clone();
        let mut copies = Vec::with_capacity(ids.len());
        for &id in ids {
            if let Some(mut object) = self.objects.iter().find(|item| item.id == id).cloned() {
                object.id = next
                    .add(object.kind)
                    .ok_or("Нет места для копии объекта.")?;
                object.name = copy_name(&object.name, 64);
                object.position = shifted(object.position, offset, 100.0, -100.0);
                copies.push(object.id);
                *next.objects.last_mut().expect("new object") = object;
            } else if let Some(mut model) = self.models.iter().find(|item| item.id == id).cloned() {
                if next.models.len() >= crate::assets::MAX_PLACEMENTS {
                    return Err("Нет места для копии модели GLB.".into());
                }
                model.id = next
                    .models
                    .iter()
                    .map(|item| item.id)
                    .max()
                    .and_then(|id| id.checked_add(1))
                    .filter(|id| *id < buildings::FIRST_ID)
                    .ok_or("Закончились идентификаторы моделей GLB.")?;
                model.label = copy_name(&model.label, 100);
                model.position = shifted(model.position, offset, 256.0, -256.0);
                copies.push(model.id);
                next.models.push(model);
            } else if let Some(mut house) =
                self.buildings.iter().find(|item| item.id == id).cloned()
            {
                house.position = shifted(house.position, offset, 100.0, 0.0);
                house.id = next
                    .add_building_at(house.position)
                    .ok_or("Нет места для копии дома.")?;
                house.name = copy_name(&house.name, 64);
                copies.push(house.id);
                *next.buildings.last_mut().expect("new house") = house;
            }
        }
        next.validate()?;
        *self = next;
        Ok(copies)
    }
}

fn shifted(position: [f32; 3], offset: [f32; 3], limit: f32, floor: f32) -> [f32; 3] {
    std::array::from_fn(|axis| {
        (position[axis] + offset[axis]).clamp(if axis == 1 { floor } else { -limit }, limit)
    })
}

fn copy_name(name: &str, max: usize) -> String {
    let suffix = " · копия";
    let mut label: String = name.chars().take(max - suffix.chars().count()).collect();
    label.push_str(suffix);
    label
}
