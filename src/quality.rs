use crate::{
    editor_history::local_directory,
    performance::{Snapshot, Telemetry},
    viewport3d,
};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum Profile {
    Low,
    #[default]
    High,
}

impl Profile {
    pub fn samples(self) -> u16 {
        match self {
            Self::Low => 1,
            Self::High => viewport3d::VIEWPORT_MSAA,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Low => "Низкий",
            Self::High => "Высокий",
        }
    }

    pub fn load() -> Self {
        local_directory()
            .and_then(|root| fs::read(root.join("graphics.json")).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn save(self) -> Result<(), String> {
        let root = local_directory().ok_or("Каталог настроек недоступен.")?;
        fs::create_dir_all(&root).map_err(|err| err.to_string())?;
        fs::write(
            root.join("graphics.json"),
            serde_json::to_vec(&self).map_err(|err| err.to_string())?,
        )
        .map_err(|err| err.to_string())
    }
}

#[derive(Default)]
pub(crate) struct Panel {
    pub open: bool,
    active: Profile,
    selected: Profile,
    adapter: Option<String>,
    cpu_usage: Option<f64>,
    process_bytes: Option<usize>,
    last_sample: Option<(Instant, u64)>,
    native: bool,
    error: Option<String>,
}

impl Panel {
    pub fn new(profile: Profile, adapter: Option<String>) -> Self {
        Self {
            active: profile,
            selected: profile,
            adapter,
            native: true,
            ..Self::default()
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, standalone: bool, telemetry: &mut Telemetry) {
        if ctx.input(|input| input.key_pressed(egui::Key::F10)) {
            self.open = !self.open;
        }
        if standalone && self.native {
            egui::Area::new(egui::Id::new("standalone_performance"))
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 8.0))
                .show(ctx, |ui| {
                    if ui.button("Графика · F10").clicked() {
                        self.open = !self.open;
                    }
                });
        }
        if !self.open {
            return;
        }
        let now = Instant::now();
        let telemetry = telemetry.snapshot(now);
        self.sample(now);
        let active = self.active;
        let previous = self.selected;
        egui::Window::new("Графика и производительность")
            .open(&mut self.open)
            .default_width(360.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for profile in [Profile::Low, Profile::High] {
                        ui.selectable_value(&mut self.selected, profile, profile.name());
                    }
                });
                ui.label(format!("Активный профиль · {}", active.name()));
                ui.weak("Низкий · без MSAA, тени 1024², меньше частиц");
                ui.weak("Высокий · MSAA 4×, тени 2048²");
                if self.selected != active {
                    ui.label("Профиль вступит в силу при следующем запуске.");
                }
                ui.separator();
                show_telemetry(ui, &telemetry);
                ui.separator();
                if let Some(cpu) = self.cpu_usage {
                    ui.label(format!("CPU процесса · {cpu:.1}% от всех ядер"));
                }
                if let Some(bytes) = self.process_bytes {
                    ui.label(format!(
                        "ОЗУ процесса · {:.1} МиБ",
                        bytes as f64 / 1_048_576.0
                    ));
                }
                if let Some(adapter) = &self.adapter {
                    ui.label(format!("GPU · {adapter}"));
                }
                ui.weak("Для этого рендера загрузка GPU не измеряется.");
                if let Some(error) = &self.error {
                    ui.label(error);
                }
            });
        if self.selected != previous && self.native {
            match self.selected.save() {
                Ok(()) => self.error = None,
                Err(error) => {
                    self.selected = previous;
                    self.error = Some(format!("Настройки не сохранены. {error}"));
                }
            }
        }
        if self.open {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
    }

    fn sample(&mut self, now: Instant) {
        if self
            .last_sample
            .is_some_and(|(at, _)| now.duration_since(at) < Duration::from_secs(1))
        {
            return;
        }
        let Some((ticks, bytes)) = process_sample() else {
            return;
        };
        self.process_bytes = Some(bytes);
        if let Some((last, last_ticks)) = self.last_sample {
            let elapsed = now.duration_since(last).as_secs_f64();
            let cpus = std::thread::available_parallelism().map_or(1.0, |count| count.get() as f64);
            self.cpu_usage = Some(
                (ticks.saturating_sub(last_ticks) as f64 / 100_000.0 / elapsed / cpus)
                    .clamp(0.0, 100.0),
            );
        }
        self.last_sample = Some((now, ticks));
    }
}

fn show_telemetry(ui: &mut egui::Ui, snapshot: &Snapshot) {
    if let Some(millis) = snapshot.latest_ms {
        ui.label(format!("Текущий кадр · {millis:.2} мс"));
    } else {
        ui.weak("Ожидание замера кадра");
    }
    ui.label(format!("Последние 60 секунд · кадров {}", snapshot.count));
    ui.horizontal_wrapped(|ui| {
        for (label, value) in [
            ("p50", snapshot.p50_ms),
            ("p95", snapshot.p95_ms),
            ("p99", snapshot.p99_ms),
        ] {
            let value = value.map_or_else(|| "—".into(), |millis| format!("{millis:.2} мс"));
            ui.label(format!("{label} {value}"));
        }
    });
    if snapshot.spikes.is_empty() {
        if snapshot.count != 0 {
            ui.weak("Задержек от 100 мс нет");
        }
    } else {
        ui.strong("Самые долгие задержки");
        for spike in snapshot.spikes.iter().take(3) {
            ui.label(format!(
                "{:.2} мс · сцена #{} · {:.1} с от запуска",
                spike.millis, spike.scene_id, spike.elapsed_seconds
            ));
        }
    }
    if let Some(error) = &snapshot.log_error {
        ui.colored_label(ui.visuals().warn_fg_color, error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proof_input::find_label;

    #[test]
    fn a7_f10_telemetry_shows_percentiles_and_limits_spikes_to_three() {
        let start = Instant::now();
        let mut telemetry = Telemetry::default();
        for (index, millis) in [149.0, 148.0, 147.0, 146.0, 16.25].into_iter().enumerate() {
            let now = start + Duration::from_secs(index as u64 * 2);
            telemetry.record(now, index as u32 + 1, millis);
        }
        let mut snapshot = telemetry.snapshot(start + Duration::from_secs(8));
        snapshot.log_error = Some("Не удалось записать «spike.log». Нет доступа.".into());
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| show_telemetry(ui, &snapshot));
        });
        for label in [
            "Текущий кадр · 16.25 мс",
            "Последние 60 секунд · кадров 5",
            "p50 147.00 мс",
            "p95 149.00 мс",
            "p99 149.00 мс",
            "149.00 мс · сцена #1 · 0.0 с от запуска",
            "147.00 мс · сцена #3 · 4.0 с от запуска",
            "Не удалось записать «spike.log». Нет доступа.",
        ] {
            assert!(find_label(&output, label, 0).is_some(), "Не видно «{label}»");
        }
        assert!(find_label(&output, "146.00 мс · сцена #4 · 6.0 с от запуска", 0).is_none());
    }
}

#[cfg(windows)]
pub(crate) fn process_sample() -> Option<(u64, usize)> {
    use std::ffi::c_void;

    #[derive(Default)]
    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    impl FileTime {
        fn ticks(&self) -> u64 {
            (u64::from(self.high) << 32) | u64::from(self.low)
        }
    }
    #[repr(C)]
    struct Counters {
        size: u32,
        page_faults: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_non_paged: usize,
        quota_non_paged: usize,
        pagefile: usize,
        peak_pagefile: usize,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn GetProcessTimes(
            handle: *mut c_void,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn K32GetProcessMemoryInfo(handle: *mut c_void, counters: *mut Counters, size: u32) -> i32;
    }
    let [mut creation, mut exit, mut kernel, mut user] =
        std::array::from_fn(|_| FileTime::default());
    let size = size_of::<Counters>() as u32;
    let mut counters = std::mem::MaybeUninit::<Counters>::uninit();
    // These APIs fill the supplied Windows ABI structs and take the pseudo-handle for this process.
    unsafe {
        let handle = GetCurrentProcess();
        if GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) == 0
            || K32GetProcessMemoryInfo(handle, counters.as_mut_ptr(), size) == 0
        {
            return None;
        }
        Some((
            kernel.ticks() + user.ticks(),
            counters.assume_init().working_set,
        ))
    }
}

#[cfg(not(windows))]
pub(crate) fn process_sample() -> Option<(u64, usize)> {
    None
}
