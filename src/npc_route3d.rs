use eframe::egui::{self, Color32, Pos2, Rect, Response, Ui};
use glam::Vec2;

use crate::{EditorTab, Studio, editor_theme, npc_ui, npcs::Npc, viewport3d::Camera};

#[derive(Clone, Copy)]
struct Drag {
    actor: u64,
    index: usize,
    position: [f32; 3],
}

impl Studio {
    pub(super) fn route_3d_input(&mut self, ui: &Ui, rect: Rect, view: &Response) -> bool {
        let id = egui::Id::new("npc_route_3d_drag");
        if !self.route_visible() {
            ui.data_mut(|data| data.remove::<Drag>(id));
            return false;
        }
        let Some(npc) = self
            .scene
            .npcs
            .iter()
            .find(|npc| Some(npc.id) == self.selected_npc)
        else {
            return false;
        };
        let actor_id = npc.id;
        let camera = self.camera;
        let nearest = |point: Pos2| {
            npc.route
                .iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    camera
                        .screen_point(rect, item.position)
                        .map(|projected| (index, projected.distance_sq(point)))
                })
                .filter(|item| item.1 <= 17.0_f32.powi(2))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|item| item.0)
        };
        let mut drag = ui
            .data(|data| data.get_temp::<Drag>(id))
            .filter(|drag| drag.actor == actor_id);
        if view.drag_started_by(egui::PointerButton::Primary)
            && let Some(origin) = ui.input(|input| input.pointer.press_origin())
            && let Some(index) = nearest(origin)
        {
            drag = Some(Drag {
                actor: actor_id,
                index,
                position: npc.route[index].position,
            });
            self.npc_waypoint = index;
        }
        if let Some(mut move_to) = drag {
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                ui.data_mut(|data| data.remove::<Drag>(id));
                return true;
            }
            if let Some(mouse) = view.interact_pointer_pos()
                && let Some(point) = self.route_point(rect, mouse, move_to.position[1])
            {
                move_to.position = point;
            }
            if view.dragged_by(egui::PointerButton::Primary) {
                ui.data_mut(|data| data.insert_temp(id, move_to));
            } else {
                ui.data_mut(|data| data.remove::<Drag>(id));
                if let Some(npc) = self
                    .scene
                    .npcs
                    .iter_mut()
                    .find(|npc| npc.id == move_to.actor)
                    && let Some(point) = npc.route.get_mut(move_to.index)
                    && point.position != move_to.position
                {
                    point.position = move_to.position;
                    self.changed();
                }
            }
            return true;
        }
        if view.clicked_by(egui::PointerButton::Primary)
            && let Some(mouse) = view.interact_pointer_pos()
        {
            if let Some(index) = nearest(mouse) {
                self.npc_waypoint = index;
                return true;
            }
            let height = npc
                .route
                .get(self.npc_waypoint)
                .map_or(0.0, |item| item.position[1]);
            if self.npc_place_on_plan
                && let Some(point) = self.route_point(rect, mouse, height)
                && let Some(actor) = self.scene.npcs.iter_mut().find(|npc| npc.id == actor_id)
                && actor.route.len() < crate::npcs::MAX_WAYPOINTS
            {
                npc_ui::add_point(actor, point, &mut self.npc_waypoint);
                self.changed();
                return true;
            }
        }
        false
    }

    fn route_point(&self, rect: Rect, mouse: Pos2, mut height: f32) -> Option<[f32; 3]> {
        for _ in 0..2 {
            let [x, z] = self.camera.ground_point(rect, mouse, height)?;
            height = self
                .route_surface_height(Vec2::new(x, z), height)
                .clamp(0.0, 100.0);
        }
        let [x, z] = self.camera.ground_point(rect, mouse, height)?;
        Some([x.clamp(-100.0, 100.0), height, z.clamp(-100.0, 100.0)])
    }

    pub(super) fn paint_route_3d(&self, ui: &Ui, rect: Rect, camera: Camera) {
        if !self.route_visible() {
            return;
        }
        let Some(actor) = self.route_actor() else {
            return;
        };
        let id = egui::Id::new("npc_route_3d_drag");
        let drag = ui
            .data(|data| data.get_temp::<Drag>(id))
            .filter(|drag| drag.actor == actor.id);
        let painter = ui.painter_at(rect);
        let font = egui::TextStyle::Small.resolve(ui.style());
        let mut last = None;
        for (index, waypoint) in actor.route.iter().enumerate() {
            let at = drag
                .filter(|drag| drag.index == index)
                .map_or(waypoint.position, |drag| drag.position);
            let point = camera.screen_point(rect, at);
            if let (Some(before), Some(next)) = (last, point) {
                painter.line_segment([before, next], (2.5, editor_theme::ACCENT));
            }
            last = point;
        }
        for (i, waypoint) in actor.route.iter().enumerate() {
            let at = drag
                .filter(|drag| drag.index == i)
                .map_or(waypoint.position, |drag| drag.position);
            if let Some(point) = camera.screen_point(rect, at) {
                let color = if self.npc_waypoint == i {
                    Color32::from_rgb(239, 213, 255)
                } else {
                    editor_theme::ACCENT
                };
                painter.circle_filled(point, 10.0, editor_theme::BACKDROP);
                painter.circle_stroke(point, 10.0, (2.0, color));
                painter.text(
                    point,
                    egui::Align2::CENTER_CENTER,
                    i + 1,
                    font.clone(),
                    color,
                );
            }
        }
        if self.npc_place_on_plan {
            painter.text(
                rect.left_bottom() + egui::vec2(14.0, -14.0),
                egui::Align2::LEFT_BOTTOM,
                "Клик — добавить точку · перетащи номер, чтобы изменить маршрут",
                font,
                editor_theme::BRIGHT,
            );
        }
    }

    fn route_visible(&self) -> bool {
        !self.game_running
            && self.editor_tab == EditorTab::Npcs
            && self.npc_panel == npc_ui::Panel::Route
    }

    fn route_actor(&self) -> Option<&Npc> {
        let id = self.selected_npc?;
        self.scene.npcs.iter().find(|actor| actor.id == id)
    }
}
