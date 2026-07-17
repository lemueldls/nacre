use raw_window_handle::{HasDisplayHandle, RawDisplayHandle, RawWindowHandle, WaylandWindowHandle};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::wlr_layer::{
        Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
        LayerSurfaceConfigure,
    },
};
use tracing::{debug, error, info};
use vello::wgpu;
use wayland_client::{
    Connection, Proxy, QueueHandle, globals::registry_queue_init, protocol::wl_output,
};

use crate::graphics;

pub struct DesktopShell {
    pub registry_state: RegistryState,
    pub output_state: OutputState,
    pub layer: LayerSurface,
    pub graphics: graphics::GraphicsEngine,
    pub exit: bool,
    pub width: u32,
    pub height: u32,
}

impl CompositorHandler for DesktopShell {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wayland_client::protocol::wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wayland_client::protocol::wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wayland_client::protocol::wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wayland_client::protocol::wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wayland_client::protocol::wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for DesktopShell {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let (width, height) = configure.new_size;
        self.width = width.max(1);
        self.height = height.max(1);

        if let Err(error) = self.graphics.resize_surface(self.width, self.height) {
            error!("Failed to resize GPU surface: {}", error);
            self.exit = true;
            return;
        }

        if let Err(error) = self.graphics.clear_surface(wgpu::Color::BLACK) {
            error!("Failed to render first frame: {}", error);
            self.exit = true;
        }
    }
}

impl OutputHandler for DesktopShell {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl ProvidesRegistryState for DesktopShell {
    registry_handlers![OutputState];

    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
}

smithay_client_toolkit::delegate_compositor!(DesktopShell);
smithay_client_toolkit::delegate_output!(DesktopShell);
smithay_client_toolkit::delegate_layer!(DesktopShell);
// smithay_client_toolkit::delegate_dmabuf!(DesktopShell);
// smithay_client_toolkit::delegate_seat!(DesktopShell);
// smithay_client_toolkit::delegate_xdg_shell!(DesktopShell);
// smithay_client_toolkit::delegate_xdg_window!(DesktopShell);
smithay_client_toolkit::delegate_registry!(DesktopShell);
