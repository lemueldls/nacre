use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    thread,
};

use facet::Facet;
use nucleo_matcher::{
    Config, Matcher,
    pattern::{CaseMatching, Normalization, Pattern},
};

#[derive(Debug, Clone, Facet)]
pub struct AppInfo {
    pub name: String,
    pub exec: String,
    pub icon: Option<String>,
}

pub struct Launcher {
    pub apps: Arc<Mutex<Vec<AppInfo>>>,
    matcher: Mutex<Matcher>,
}

impl Launcher {
    pub fn new() -> Self {
        let apps = Arc::new(Mutex::new(Vec::new()));
        let matcher = Mutex::new(Matcher::new(Config::DEFAULT));

        let launcher = Self { apps, matcher };
        launcher.start_indexing();

        launcher
    }

    /// Spawns a background thread to read and index all system desktop files
    fn start_indexing(&self) {
        let apps_clone = self.apps.clone();
        thread::spawn(move || {
            let mut list = Vec::new();
            let paths = vec![
                PathBuf::from("/usr/share/applications"),
                // Add user local applications directory if HOME is set
                std::env::var_os("HOME").map_or_else(
                    || PathBuf::from("/home/lemuel/.local/share/applications"),
                    |home| PathBuf::from(home).join(".local/share/applications"),
                ),
            ];

            for path in paths {
                if path.exists() {
                    if let Ok(entries) = fs::read_dir(path) {
                        for entry_res in entries {
                            if let Ok(entry) = entry_res {
                                let file_path = entry.path();
                                if file_path.extension().map_or(false, |ext| ext == "desktop") {
                                    if let Some(app) = parse_desktop_file(&file_path) {
                                        list.push(app);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Deduplicate by name
            list.sort_by(|a, b| a.name.cmp(&b.name));
            list.dedup_by(|a, b| a.name == b.name);

            let mut lock = apps_clone.lock().unwrap();
            *lock = list;
            println!("Indexed {} applications", lock.len());
        });
    }

    /// Run fuzzy search on user input using nucleo-matcher
    pub fn search(&self, query: &str) -> Vec<AppInfo> {
        let apps = self.apps.lock().unwrap().clone();
        if query.is_empty() {
            return apps;
        }

        let mut matcher = self.matcher.lock().unwrap();
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);

        // Map apps to names to run matching
        let app_names: Vec<&str> = apps.iter().map(|app| app.name.as_str()).collect();
        let matches = pattern.match_list(app_names, &mut *matcher);

        // Sort apps by their match score (highest score first)
        let mut results = Vec::new();
        for (name, _score) in matches {
            if let Some(app) = apps.iter().find(|app| app.name == name) {
                results.push(app.clone());
            }
        }

        results
    }

    /// Spawn application detached from Nacre's main thread group to prevent
    /// zombie processes
    pub fn launch(&self, app: &AppInfo) -> Result<(), String> {
        // Strip %u, %F, %U, %f etc. from execution command
        let exec_clean = app
            .exec
            .split_whitespace()
            .filter(|part| !part.starts_with('%'))
            .collect::<Vec<&str>>()
            .join(" ");

        println!(
            "Launching application: {} via command: '{}'",
            app.name, exec_clean
        );

        // Spawn shell to detach it completely
        Command::new("sh")
            .arg("-c")
            .arg(format!("nohup {} >/dev/null 2>&1 &", exec_clean))
            .spawn()
            .map_err(|e| format!("Failed to spawn command: {}", e))?;

        Ok(())
    }
}

/// Simple parser for .desktop files
fn parse_desktop_file(path: &Path) -> Option<AppInfo> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut name = None;
    let mut exec = None;
    let mut icon = None;
    let mut is_app = false;
    let mut in_desktop_entry = false;

    for line_res in reader.lines() {
        let line = line_res.ok()?;
        let line_trimmed = line.trim();

        if line_trimmed == "[Desktop Entry]" {
            in_desktop_entry = true;
            continue;
        } else if line_trimmed.starts_with('[') && line_trimmed.ends_with(']') {
            in_desktop_entry = false;
        }

        if in_desktop_entry {
            if line_trimmed.starts_with("Type=") {
                let val = line_trimmed.split('=').nth(1)?.trim();
                if val == "Application" {
                    is_app = true;
                }
            } else if line_trimmed.starts_with("Name=") && name.is_none() {
                name = Some(line_trimmed.split('=').nth(1)?.trim().to_string());
            } else if line_trimmed.starts_with("Exec=") && exec.is_none() {
                exec = Some(line_trimmed.split('=').nth(1)?.trim().to_string());
            } else if line_trimmed.starts_with("Icon=") && icon.is_none() {
                icon = Some(line_trimmed.split('=').nth(1)?.trim().to_string());
            }
        }
    }

    if is_app && name.is_some() && exec.is_some() {
        Some(AppInfo {
            name: name.unwrap(),
            exec: exec.unwrap(),
            icon,
        })
    } else {
        None
    }
}
