use std::{collections::HashMap, env, path::PathBuf};

use niri_ipc::{Event, Reply, Request, Window, Workspace};
use tokio::{
    fs,
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::mpsc,
};
use tracing::warn;

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

/// Tracks the subset of niri's compositor state the bar cares about, built up
/// purely from the event stream.
///
/// Per the niri-ipc docs, the event stream always sends the full current state
/// up front (the first `Event::WorkspacesChanged`/`WindowsChanged` contain
/// everything, not a diff), so there's no need to separately send
/// `Request::Workspaces`/`Request::Windows` to seed this before subscribing.
#[derive(Default)]
struct NiriState {
    workspaces: Vec<Workspace>,
    windows: HashMap<u64, Window>,
    focused_window_id: Option<u64>,
}

impl NiriState {
    fn apply(&mut self, event: Event) {
        match event {
            Event::WorkspacesChanged { workspaces } => {
                self.workspaces = workspaces;
            }
            Event::WorkspaceActivated { id, focused } => {
                // Every output has exactly one active workspace; only one
                // workspace across all outputs is ever focused.
                let output = self
                    .workspaces
                    .iter()
                    .find(|w| w.id == id)
                    .and_then(|w| w.output.clone());

                for ws in &mut self.workspaces {
                    if ws.output == output {
                        ws.is_active = ws.id == id;
                    }
                    if focused {
                        ws.is_focused = ws.id == id;
                    }
                }
            }
            Event::WorkspaceActiveWindowChanged {
                workspace_id,
                active_window_id,
            } => {
                if let Some(ws) = self.workspaces.iter_mut().find(|w| w.id == workspace_id) {
                    ws.active_window_id = active_window_id;
                }
            }
            Event::WindowsChanged { windows } => {
                self.windows = windows.into_iter().map(|w| (w.id, w)).collect();
            }
            Event::WindowOpenedOrChanged { window } => {
                self.windows.insert(window.id, window);
            }
            Event::WindowClosed { id } => {
                self.windows.remove(&id);
                if self.focused_window_id == Some(id) {
                    self.focused_window_id = None;
                }
            }
            Event::WindowFocusChanged { id } => {
                self.focused_window_id = id;
            }
            _ => {
                // Keyboard layout, overview, screenshot, cast, and config
                // events don't affect bar state. Falling through here
                // (rather than matching every variant) also means new
                // event variants added in a future niri-ipc patch bump
                // won't fail to compile against this match.
            }
        }
    }

    fn focused_window_title(&self) -> Option<String> {
        self.focused_window_id
            .and_then(|id| self.windows.get(&id))
            .and_then(|w| w.title.clone())
    }

    fn workspace_infos(&self) -> Vec<WorkspaceInfo> {
        self.workspaces
            .iter()
            .map(|ws| {
                WorkspaceInfo {
                    id: ws.id.to_string(),
                    name: ws.name.clone().unwrap_or_else(|| ws.idx.to_string()),
                    is_active: ws.is_active,
                    is_focused: ws.is_focused,
                    // niri doesn't expose "empty" directly on `Workspace`; a
                    // workspace with no active window has no windows on it.
                    is_empty: ws.active_window_id.is_none(),
                }
            })
            .collect()
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

            // Requests are one JSON value per line. `Request::EventStream` is a
            // unit variant, so it serializes to the bare JSON string "EventStream"
            let request = serde_json::to_string(&Request::EventStream)
                .map_err(|e| format!("Failed to serialize EventStream request: {}", e))?;
            writer
                .write_all(request.as_bytes())
                .await
                .map_err(|e| format!("Failed to send EventStream request: {}", e))?;
            writer
                .write_all(b"\n")
                .await
                .map_err(|e| format!("Failed to send EventStream request: {}", e))?;
            writer
                .flush()
                .await
                .map_err(|e| format!("Failed to flush Niri socket: {}", e))?;

            // Niri replies once to the EventStream request itself (normally
            // `Response::Handled`) before it starts pushing events on the
            // same connection.
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .await
                .map_err(|e| format!("Failed to read EventStream reply: {}", e))?;

            let reply: Reply = serde_json::from_str(&line).map_err(|e| {
                format!(
                    "Failed to parse EventStream reply: {} (line: {})",
                    e,
                    line.trim()
                )
            })?;

            if let Err(err) = reply {
                return Err(format!("Niri rejected EventStream request: {}", err));
            }

            let mut state = NiriState::default();

            loop {
                line.clear();
                let bytes_read = reader
                    .read_line(&mut line)
                    .await
                    .map_err(|e| format!("Failed to read Niri event: {}", e))?;

                if bytes_read == 0 {
                    break; // Connection closed
                }

                let event: Event = match serde_json::from_str(&line) {
                    Ok(event) => event,
                    Err(err) => {
                        // Most likely this pinned niri-ipc version doesn't know
                        // about an event variant a newer niri sent. Skip it
                        // rather than tearing down the connection.
                        warn!(
                            "Failed to parse Niri event, skipping (niri newer than pinned niri-ipc?): {} (line: {})",
                            err,
                            line.trim()
                        );
                        continue;
                    }
                };

                state.apply(event);

                let compositor_state = CompositorState {
                    workspaces: state.workspace_infos(),
                    active_window_title: state.focused_window_title(),
                };

                let _ = sender.send(CompositorEvent::StateChanged(compositor_state));
            }

            Ok(())
        })
    }
}
