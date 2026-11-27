use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use serde_json::json;

const WINDOW: Duration = Duration::from_secs(60);
const SPIKE_MS: f64 = 100.0;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Spike {
    pub scene_id: u32,
    pub elapsed_seconds: f64,
    pub millis: f64,
}

#[derive(Debug, Serialize)]
pub(crate) struct Snapshot {
    pub latest_ms: Option<f64>,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub p99_ms: Option<f64>,
    pub count: usize,
    pub spikes: Vec<Spike>,
    pub log_error: Option<String>,
}

#[derive(Default)]
pub(crate) struct Telemetry {
    frames: VecDeque<(Instant, f64)>,
    spikes: VecDeque<(Instant, Spike)>,
    sorted: Vec<f64>,
    start: Option<Instant>,
    last: Option<Instant>,
    log: Option<PathBuf>,
    log_error: Option<String>,
}

impl Telemetry {
    pub(crate) fn new(log: Option<PathBuf>) -> Self {
        Self {
            log,
            ..Self::default()
        }
    }

    pub(crate) fn record(&mut self, now: Instant, scene_id: u32, ms: f64) {
        if !ms.is_finite() || ms < 0.0 || self.last.is_some_and(|last| now < last) {
            return;
        }
        let start = *self.start.get_or_insert(now);
        self.last = Some(now);
        self.prune(now);
        self.frames.push_back((now, ms));
        if ms >= SPIKE_MS {
            let spike = Spike {
                scene_id,
                elapsed_seconds: now.duration_since(start).as_secs_f64(),
                millis: ms,
            };
            if let Some(path) = &self.log
                && let Err(error) = append_spike(path, &spike)
            {
                self.log_error = Some(format!("Не удалось записать «{}». {error}", path.display()));
            }
            self.spikes.push_back((now, spike));
        }
    }

    pub(crate) fn snapshot(&mut self, now: Instant) -> Snapshot {
        self.prune(now);
        self.sorted.clear();
        self.sorted.extend(self.frames.iter().map(|(_, ms)| *ms));
        self.sorted.sort_unstable_by(f64::total_cmp);
        let mut spikes = Vec::<Spike>::with_capacity(3);
        for (_, spike) in self.spikes.iter().rev() {
            let index = spikes.partition_point(|entry| entry.millis >= spike.millis);
            if index < 3 {
                if spikes.len() == 3 {
                    spikes.pop();
                }
                spikes.insert(index, spike.clone());
            }
        }
        Snapshot {
            latest_ms: self.frames.back().map(|(_, ms)| *ms),
            p50_ms: percentile_sorted(&self.sorted, 0.50),
            p95_ms: percentile_sorted(&self.sorted, 0.95),
            p99_ms: percentile_sorted(&self.sorted, 0.99),
            count: self.frames.len(),
            spikes,
            log_error: self.log_error.clone(),
        }
    }

    fn prune(&mut self, now: Instant) {
        while self
            .frames
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) > WINDOW)
        {
            self.frames.pop_front();
        }
        while self
            .spikes
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) > WINDOW)
        {
            self.spikes.pop_front();
        }
    }
}

pub(crate) fn percentile_sorted(values: &[f64], q: f64) -> Option<f64> {
    if !(0.0..=1.0).contains(&q) {
        return None;
    }
    let last = values.len().checked_sub(1)?;
    let index = (last as f64 * q).ceil() as usize;
    values.get(index).copied()
}

fn append_spike(path: &Path, spike: &Spike) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?;
    let utc_epoch_ms = u64::try_from(timestamp.as_millis()).map_err(io::Error::other)?;
    let mut line = json!({
        "utc_epoch_ms": utc_epoch_ms,
        "scene_id": spike.scene_id,
        "elapsed_seconds": spike.elapsed_seconds,
        "millis": spike.millis,
    })
    .to_string();
    line.push('\n');
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a7_percentiles_match_chapter_soak_for_empty_and_nonempty_samples() {
        let values = (1..=100).map(f64::from).collect::<Vec<_>>();
        assert_eq!(percentile_sorted(&[], 0.50), None);
        assert_eq!(percentile_sorted(&[16.0], 0.99), Some(16.0));
        assert_eq!(percentile_sorted(&values, 0.50), Some(51.0));
        assert_eq!(percentile_sorted(&values, 0.95), Some(96.0));
        assert_eq!(percentile_sorted(&values, 0.99), Some(100.0));
        assert_eq!(percentile_sorted(&values, f64::NAN), None);
    }

    #[test]
    fn a7_frame_and_spike_pruning_uses_the_same_inclusive_sixty_second_window() {
        let start = Instant::now();
        let mut telemetry = Telemetry::new(None);
        telemetry.record(start, 4, 135.0);
        telemetry.record(start + Duration::from_secs(55), 5, 25.0);
        let edge = start + WINDOW;
        let snapshot = telemetry.snapshot(edge);
        assert_eq!(snapshot.count, 2);
        assert_eq!(snapshot.latest_ms, Some(25.0));
        assert_eq!(snapshot.p99_ms, Some(135.0));
        assert_eq!(snapshot.spikes.len(), 1);
        let snapshot = telemetry.snapshot(edge + Duration::from_millis(1));
        assert_eq!(snapshot.count, 1);
        assert_eq!(snapshot.p99_ms, Some(25.0));
        assert!(snapshot.spikes.is_empty());
        let empty = telemetry.snapshot(edge + WINDOW);
        assert_eq!(empty.count, 0);
        assert_eq!(empty.latest_ms, None);
        assert_eq!(empty.p50_ms, None);
    }

    #[test]
    fn a7_rejects_invalid_or_out_of_order_probes() {
        let now = Instant::now();
        let mut telemetry = Telemetry::new(None);
        telemetry.record(now + Duration::from_secs(2), 1, 22.0);
        for ms in [f64::NAN, f64::INFINITY, -100.0] {
            telemetry.record(now + Duration::from_secs(3), 1, ms);
        }
        telemetry.record(now, 1, 500.0);
        let snapshot = telemetry.snapshot(now + Duration::from_secs(4));
        assert_eq!(snapshot.count, 1);
        assert_eq!(snapshot.latest_ms, Some(22.0));
        assert!(snapshot.spikes.is_empty());
    }

    #[test]
    fn a7_top_three_lists_actual_spikes_and_breaks_ties_by_most_recent() {
        let start = Instant::now();
        let mut telemetry = Telemetry::new(None);
        for (index, ms) in [99.999, 100.0, 110.0, 150.0, 125.0, 110.0]
            .into_iter()
            .enumerate()
        {
            telemetry.record(start + Duration::from_secs(index as u64), index as u32, ms);
        }
        let rows = telemetry.snapshot(start + Duration::from_secs(6)).spikes;
        assert_eq!(
            rows.iter().map(|row| row.scene_id).collect::<Vec<_>>(),
            [3, 4, 5]
        );
        assert_eq!(
            rows.iter().map(|row| row.millis).collect::<Vec<_>>(),
            [150.0, 125.0, 110.0]
        );
        assert_eq!(rows[2].elapsed_seconds, 5.0);
    }

    #[test]
    fn a7_log_appends_only_real_spikes_with_scene_and_timestamp()
    -> Result<(), Box<dyn std::error::Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!("robot-spike-{}-{nonce}", std::process::id()));
        let path = root.join("spike.log");
        let now = Instant::now();
        let mut telemetry = Telemetry::new(Some(path.clone()));
        telemetry.record(now, 4, 99.999);
        assert!(!path.exists());
        telemetry.record(now + Duration::from_secs(1), 7, 100.0);
        telemetry.record(now + Duration::from_secs(2), 9, 175.125);
        let source = fs::read_to_string(&path)?;
        fs::remove_file(&path)?;
        fs::remove_dir(&root)?;
        let entries = source
            .lines()
            .map(serde_json::from_str::<serde_json::Value>)
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["scene_id"], 7);
        assert_eq!(entries[0]["millis"], 100.0);
        assert_eq!(entries[1]["scene_id"], 9);
        assert_eq!(entries[1]["elapsed_seconds"], 2.0);
        assert!(
            entries
                .iter()
                .all(|entry| entry["utc_epoch_ms"].as_u64().is_some_and(|time| time > 0))
        );
        assert!(
            telemetry
                .snapshot(now + Duration::from_secs(2))
                .log_error
                .is_none()
        );
        Ok(())
    }
}
