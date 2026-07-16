use std::{env, path::PathBuf};

use tokio::{
    fs,
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::mpsc,
};

use crate::ipc::{CompositorEvent, CompositorIpc, CompositorState, WorkspaceInfo};

/// Niri IPC Implementation
pub struct NiriIpc {
    pub socket_path: Option<PathBuf>,
}

impl NiriIpc {
    pub async fn find_socket() -> Option<PathBuf> {
        if let Some(path_str) = env::var_os("NIRI_SOCKET") {
            let path = PathBuf::from(path_str);

            if path.exists() {
                return Some(path);
            }
        }

        // Fallback search: /run/user/<uid>/niri-ipc.*
        let uid = unsafe { libc::getuid() };
        let run_user_dir = format!("/run/user/{}", uid);
        if let Ok(mut entries) = fs::read_dir(&run_user_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().into_owned();

                if name.starts_with("niri-ipc") {
                    let path = entry.path();
                    return Some(path);
                }
            }
        }

        None
    }
}

impl CompositorIpc for NiriIpc {
    fn run(
        &self,
        sender: mpsc::UnboundedSender<CompositorEvent>,
    ) -> tokio::task::JoinHandle<Result<(), String>> {
        let socket_path = self.socket_path.clone();
        tokio::spawn(async move {
            let path = socket_path.ok_or_else(|| "Niri IPC socket path not set".to_string())?;
            let stream = UnixStream::connect(&path)
                .await
                .map_err(|e| format!("Failed to connect to Niri socket: {}", e))?;

            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);

            // Subscribe to event stream: Send "EventStream"\n (or JSON request format)
            // In niri-ipc, request is serialized as simple string for Unit variant:
            // "EventStream"
            writer
                .write_all(b"\"EventStream\"\n")
                .await
                .map_err(|e| format!("Failed to send subscribe request: {}", e))?;
            writer
                .flush()
                .await
                .map_err(|e| format!("Failed to flush stream: {}", e))?;

            let mut line = String::new();
            let mut current_state = CompositorState {
                workspaces: vec![],
                active_window_title: None,
            };

            loop {
                line.clear();
                let bytes_read = reader
                    .read_line(&mut line)
                    .await
                    .map_err(|e| format!("Failed to read Niri event: {}", e))?;

                if bytes_read == 0 {
                    break; // Connection closed
                }

                // Parse the line. Niri returns JSON lines.
                // We'll search for WorkspacesChanged / WindowFocusChanged key details.
                // Doing string-based detection makes our parsing highly robust to minor Niri
                // version differences.
                if line.contains("WorkspacesChanged") {
                    // Extract workspaces details
                    let mut workspaces = Vec::new();
                    // Basic JSON parser helper: look for workspace entries
                    // Each workspace has fields: id, name, is_active, is_focused, is_empty
                    // Let's do a simple regex-like extraction or substring scanning.
                    // This avoids failing due to minor schema or enum changes.
                    let w_blocks: Vec<&str> = line.split("{\"id\"").collect();
                    for block in w_blocks.iter().skip(1) {
                        let id = block
                            .split(",")
                            .next()
                            .unwrap_or("")
                            .trim_matches(|c| c == ':' || c == '"' || c == ' ');
                        let name = if let Some(n_part) = block.split("\"name\":").nth(1) {
                            n_part
                                .split(",")
                                .next()
                                .unwrap_or("")
                                .trim_matches(|c| c == '"' || c == ' ' || c == '}')
                        } else {
                            id
                        };
                        let is_active = block.contains("\"is_active\":true")
                            || block.contains("\"active\":true");
                        let is_focused = block.contains("\"is_focused\":true")
                            || block.contains("\"focused\":true");
                        let is_empty =
                            block.contains("\"is_empty\":true") || block.contains("\"empty\":true");

                        workspaces.push(WorkspaceInfo {
                            id: id.to_string(),
                            name: name.to_string(),
                            is_active,
                            is_focused,
                            is_empty,
                        });
                    }

                    if !workspaces.is_empty() {
                        current_state.workspaces = workspaces;
                        let _ = sender.send(CompositorEvent::StateChanged(current_state.clone()));
                    }
                } else if line.contains("WindowFocusChanged")
                    || line.contains("FocusedWindowChanged")
                {
                    // Extract title
                    if let Some(t_part) = line.split("\"title\":").nth(1) {
                        let title = t_part
                            .split(",")
                            .next()
                            .unwrap_or("null")
                            .trim_matches(|c| c == '"' || c == ' ' || c == '}' || c == ']');
                        let title_opt = if title == "null" {
                            None
                        } else {
                            Some(title.to_string())
                        };
                        current_state.active_window_title = title_opt;
                        let _ = sender.send(CompositorEvent::StateChanged(current_state.clone()));
                    }
                }
            }

            Ok(())
        })
    }
}
