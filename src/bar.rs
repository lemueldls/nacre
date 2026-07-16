use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::time;

use crate::ipc::WorkspaceInfo;

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

pub struct BarManager {
    pub state: Arc<Mutex<BarState>>,
}

impl BarManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(BarState::default())),
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

                // Read battery metrics from /sys/class/power_supply/BAT0
                let mut battery_pct = None;
                let mut battery_charging = false;
                let bat_path = Path::new("/sys/class/power_supply/BAT0");
                if bat_path.exists() {
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
}
