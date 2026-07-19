pub mod render;

use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::time;

use crate::{
    config::{BarConfig, BarModuleConfig, BarModuleConfigAlign, BarModuleConfigType},
    ipc::WorkspaceInfo,
};

#[derive(Debug, Clone)]
pub struct BarState {
    pub workspaces: Vec<WorkspaceInfo>,
    pub active_window_title: Option<String>,
    pub time_str: String,
    pub cpu_pct: f32,
    pub mem_pct: f32,
    pub battery_pct: Option<f32>,
    pub battery_charging: bool,
}

impl Default for BarState {
    fn default() -> Self {
        Self {
            workspaces: vec![],
            active_window_title: None,
            time_str: "00:00:00".to_string(),
            cpu_pct: 0.0,
            mem_pct: 0.0,
            battery_pct: None,
            battery_charging: false,
        }
    }
}

/// A single configured bar module, resolved against current runtime state.
/// Renderers should iterate `BarManager::visible_modules` (or
/// `modules_by_alignment`) rather than assuming a fixed module set.
#[derive(Debug, Clone)]
pub struct ResolvedModule {
    pub config: BarModuleConfig,
    pub value: ModuleValue,
}

#[derive(Debug, Clone)]
pub enum ModuleValue {
    Workspaces(Vec<WorkspaceInfo>),
    Title(Option<String>),
    SystemInfo {
        time_str: String,
        cpu_pct: f32,
        mem_pct: f32,
        battery_pct: Option<f32>,
        battery_charging: bool,
    },
    /// Plugin modules aren't fed live data yet. The module still
    /// resolves so layout/rendering can reserve space for it.
    Plugin {
        id: Option<String>,
    },
}

pub struct BarManager {
    pub state: Arc<Mutex<BarState>>,
    pub config: BarConfig,
}

impl BarManager {
    pub fn new(bar_config: BarConfig) -> Self {
        Self {
            state: Arc::new(Mutex::new(BarState::default())),
            config: bar_config,
        }
    }

    /// Spawns background polling tasks for clock and system metrics
    pub fn start_polling(&self) {
        let state_clone = self.state.clone();

        tokio::spawn(async move {
            let mut prev_idle = 0u64;
            let mut prev_total = 0u64;
            let mut interval = time::interval(Duration::from_secs(1));

            loop {
                interval.tick().await;

                // Get current local time via libc to keep binary small and fast
                let time_str = unsafe {
                    let raw_time = libc::time(std::ptr::null_mut());
                    let mut tm = std::mem::zeroed();
                    libc::localtime_r(&raw_time, &mut tm);

                    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
                };

                // Read CPU stats from /proc/stat
                let mut cpu_pct = 0.0;
                if let Ok(file) = File::open("/proc/stat") {
                    let mut reader = BufReader::new(file);
                    let mut first_line = String::new();
                    if let Ok(_) = reader.read_line(&mut first_line) {
                        let parts: Vec<&str> = first_line.split_whitespace().collect();
                        if parts.len() >= 5 {
                            // index 1..=4: user, nice, system, idle
                            let user: u64 = parts[1].parse().unwrap_or(0);
                            let nice: u64 = parts[2].parse().unwrap_or(0);
                            let system: u64 = parts[3].parse().unwrap_or(0);
                            let idle: u64 = parts[4].parse().unwrap_or(0);

                            let idle_time = idle;
                            let total_time = user + nice + system + idle;

                            let idle_delta = idle_time.saturating_sub(prev_idle);
                            let total_delta = total_time.saturating_sub(prev_total);

                            if total_delta > 0 {
                                cpu_pct = 100.0 * (1.0 - (idle_delta as f32 / total_delta as f32));
                            }

                            prev_idle = idle_time;
                            prev_total = total_time;
                        }
                    }
                }

                // Read memory usage from /proc/meminfo
                let mut mem_pct = 0.0;
                if let Ok(file) = File::open("/proc/meminfo") {
                    let reader = BufReader::new(file);
                    let mut total_kb = 0u64;
                    let mut avail_kb = 0u64;
                    for line_res in reader.lines() {
                        if let Ok(line) = line_res {
                            if line.starts_with("MemTotal:") {
                                total_kb = line
                                    .split_whitespace()
                                    .nth(1)
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(0);
                            } else if line.starts_with("MemAvailable:") {
                                avail_kb = line
                                    .split_whitespace()
                                    .nth(1)
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(0);
                            }
                        }
                    }
                    if total_kb > 0 {
                        mem_pct = 100.0 * (1.0 - (avail_kb as f32 / total_kb as f32));
                    }
                }

                // Read battery metrics
                let mut battery_pct = None;
                let mut battery_charging = false;
                if let Some(bat_path) = find_battery() {
                    if let Ok(cap_str) = std::fs::read_to_string(bat_path.join("capacity")) {
                        if let Ok(cap) = cap_str.trim().parse::<f32>() {
                            battery_pct = Some(cap);
                        }
                    }
                    if let Ok(status_str) = std::fs::read_to_string(bat_path.join("status")) {
                        battery_charging = status_str.trim() == "Charging";
                    }
                }

                // Update state
                {
                    let mut lock = state_clone.lock().unwrap();
                    lock.time_str = time_str;
                    lock.cpu_pct = cpu_pct;
                    lock.mem_pct = mem_pct;
                    lock.battery_pct = battery_pct;
                    lock.battery_charging = battery_charging;
                }
            }
        });
    }

    /// Updates active workspace and title metrics from Compositor IPC
    pub fn update_from_compositor(&self, workspaces: Vec<WorkspaceInfo>, title: Option<String>) {
        let mut lock = self.state.lock().unwrap();
        lock.workspaces = workspaces;
        lock.active_window_title = title;
    }

    /// Resolves `config.bar.modules` against current state, in configured
    /// order. Call this once per redraw; it takes one short-lived lock and
    /// only clones what's needed for that frame.
    pub fn visible_modules(&self) -> Vec<ResolvedModule> {
        let state = self.state.lock().unwrap();
        self.config
            .modules
            .iter()
            .map(|module_config| {
                let value = match module_config.module_type.clone() {
                    BarModuleConfigType::Workspaces => {
                        ModuleValue::Workspaces(state.workspaces.clone())
                    }
                    BarModuleConfigType::Title => {
                        ModuleValue::Title(state.active_window_title.clone())
                    }
                    BarModuleConfigType::SystemInfo => {
                        ModuleValue::SystemInfo {
                            time_str: state.time_str.clone(),
                            cpu_pct: state.cpu_pct,
                            mem_pct: state.mem_pct,
                            battery_pct: state.battery_pct,
                            battery_charging: state.battery_charging,
                        }
                    }
                    BarModuleConfigType::Plugin => {
                        ModuleValue::Plugin {
                            id: module_config.id.clone(),
                        }
                    }
                };

                ResolvedModule {
                    config: module_config.clone(),
                    value,
                }
            })
            .collect()
    }

    /// `visible_modules()` split into start/center/end groups, which is
    /// normally what a layout pass wants directly.
    pub fn modules_by_alignment(
        &self,
    ) -> (
        Vec<ResolvedModule>,
        Vec<ResolvedModule>,
        Vec<ResolvedModule>,
    ) {
        let mut start = Vec::new();
        let mut center = Vec::new();
        let mut end = Vec::new();

        for module in self.visible_modules() {
            match module.config.align.clone() {
                BarModuleConfigAlign::Start => start.push(module),
                BarModuleConfigAlign::Center => center.push(module),
                BarModuleConfigAlign::End => end.push(module),
            }
        }

        (start, center, end)
    }
}

fn find_battery() -> Option<PathBuf> {
    let power_supply = Path::new("/sys/class/power_supply");
    let entries = std::fs::read_dir(power_supply).ok()?;

    for entry in entries.flatten() {
        let name = entry.file_name();

        if name.to_string_lossy().starts_with("BAT") {
            return Some(entry.path());
        }
    }

    None
}
