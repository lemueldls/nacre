use std::{env, path::PathBuf};

use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::UnixStream,
    sync::mpsc,
};

use crate::ipc::{CompositorEvent, CompositorIpc, CompositorState, WorkspaceInfo};

/// Hyprland IPC Implementation
pub struct HyprlandIpc {
    pub instance_signature: String,
}

impl HyprlandIpc {
    pub fn find_signature() -> Option<String> {
        env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()
    }
}

impl CompositorIpc for HyprlandIpc {
    fn run(
        &self,
        sender: mpsc::UnboundedSender<CompositorEvent>,
    ) -> tokio::task::JoinHandle<Result<(), String>> {
        let signature = self.instance_signature.clone();
        tokio::spawn(async move {
            // Hyprland socket2 path: /tmp/hypr/<signature>/.socket2.sock
            let socket_path = PathBuf::from(format!("/tmp/hypr/{}/.socket2.sock", signature));
            let stream = UnixStream::connect(&socket_path)
                .await
                .map_err(|e| format!("Failed to connect to Hyprland socket2: {}", e))?;

            let mut reader = BufReader::new(stream);
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
                    .map_err(|e| format!("Failed to read Hyprland event: {}", e))?;

                if bytes_read == 0 {
                    break;
                }

                // Hyprland event lines look like: "event>>data"
                if let Some(pos) = line.find(">>") {
                    let event = &line[..pos];
                    let data = line[pos + 2..].trim();

                    match event {
                        "workspace" => {
                            // Focus shifted to workspace with name data
                            // In this simple helper, we will map this to mock workspaces with
                            // active focus
                            let mut workspaces = Vec::new();
                            for i in 1..=5 {
                                let id_str = i.to_string();
                                workspaces.push(WorkspaceInfo {
                                    id: id_str.clone(),
                                    name: format!("Workspace {}", i),
                                    is_active: true,
                                    is_focused: id_str == data,
                                    is_empty: false,
                                });
                            }
                            current_state.workspaces = workspaces;
                            let _ =
                                sender.send(CompositorEvent::StateChanged(current_state.clone()));
                        }
                        "activewindow" => {
                            // Focused window title changed
                            // Format: "class,title"
                            let title = data.split(',').nth(1).unwrap_or(data);
                            current_state.active_window_title = Some(title.to_string());
                            let _ =
                                sender.send(CompositorEvent::StateChanged(current_state.clone()));
                        }
                        _ => {}
                    }
                }
            }

            Ok(())
        })
    }
}
