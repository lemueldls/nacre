pub mod hyprland;
pub mod mock;
pub mod niri;

use facet::Facet;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Facet, PartialEq)]
pub struct WorkspaceInfo {
    pub id: String,
    pub name: String,
    pub is_active: bool,
    pub is_focused: bool,
    pub is_empty: bool,
}

#[derive(Debug, Clone, Facet, PartialEq)]
pub struct CompositorState {
    pub workspaces: Vec<WorkspaceInfo>,
    pub active_window_title: Option<String>,
}

#[derive(Debug, Clone)]
pub enum CompositorEvent {
    StateChanged(CompositorState),
}

pub trait CompositorIpc: Send + Sync {
    fn run(
        &self,
        sender: mpsc::UnboundedSender<CompositorEvent>,
    ) -> tokio::task::JoinHandle<Result<(), String>>;
}
