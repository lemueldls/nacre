use std::time::Duration;

use tokio::{sync::mpsc, time};

use crate::ipc::{CompositorEvent, CompositorIpc, CompositorState, WorkspaceInfo};

/// Mock IPC Implementation (For Headless/Test/Fallback)
pub struct MockIpc;

impl CompositorIpc for MockIpc {
    fn run(
        &self,
        sender: mpsc::UnboundedSender<CompositorEvent>,
    ) -> tokio::task::JoinHandle<Result<(), String>> {
        tokio::spawn(async move {
            let mut tick = 0;
            loop {
                let workspaces = vec![
                    WorkspaceInfo {
                        id: "1".to_string(),
                        name: "Web".to_string(),
                        is_active: true,
                        is_focused: tick % 3 == 0,
                        is_empty: false,
                    },
                    WorkspaceInfo {
                        id: "2".to_string(),
                        name: "Code".to_string(),
                        is_active: true,
                        is_focused: tick % 3 == 1,
                        is_empty: false,
                    },
                    WorkspaceInfo {
                        id: "3".to_string(),
                        name: "Chat".to_string(),
                        is_active: tick % 2 == 0,
                        is_focused: tick % 3 == 2,
                        is_empty: tick % 2 != 0,
                    },
                ];

                let title = match tick % 3 {
                    0 => Some("Firefox - Github".to_string()),
                    1 => Some("nacre - Cargo.toml - VS Code".to_string()),
                    _ => Some("Discord".to_string()),
                };

                let state = CompositorState {
                    workspaces,
                    active_window_title: title,
                };

                if let Err(_) = sender.send(CompositorEvent::StateChanged(state)) {
                    break;
                }

                time::sleep(Duration::from_secs(4)).await;
                tick += 1;
            }

            Ok(())
        })
    }
}
