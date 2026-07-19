use std::sync::Arc;

use raw_window_handle::{HasDisplayHandle, RawDisplayHandle, RawWindowHandle, WaylandWindowHandle};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
};
use tracing::{debug, error, info};
use vello::peniko::Color;
use wayland_client::{
    Connection, Proxy, QueueHandle, globals::registry_queue_init, protocol::wl_output,
};

use crate::{bar, config::BarConfig, graphics};

/// Opaque background behind the bar's own drawing.
const BAR_BACKGROUND: Color = Color::from_rgb8(18, 18, 20);

pub struct DesktopShell {
    pub registry_state: RegistryState,
    pub output_state: OutputState,
    pub layer: LayerSurface,
    pub graphics: graphics::GraphicsEngine,
    pub exit: bool,
    pub width: u32,
    pub height: u32,
    pub bar_manager: Arc<bar::BarManager>,
    pub bar_config: BarConfig,
    pub font_size: f32,
    pub accent_color: Color,
}

impl DesktopShell {
    /// Lays out and paints the current bar state, then requests the next
    /// frame callback so `frame()` fires again once the compositor is
    /// ready for another frame.
    ///
    /// This redraws on every compositor frame tick rather than only when
    /// `BarState` actually changed. That's the simple, low-risk choice for
    /// now: the alternative (only redraw when something's actually dirty)
    /// needs a wake source that can interrupt the main thread's
    /// `event_queue.blocking_dispatch()` loop from the tokio tasks that
    /// mutate `BarState` (the compositor-IPC forwarder, the 1-second
    /// system-info poll), which in this wayland-client/SCTK setup
    /// realistically means moving to a `calloop`-based event loop (already
    /// an indirect dependency via SCTK) with a timer source and a
    /// cross-thread ping/channel source alongside the Wayland one. Worth
    /// doing before this ships on battery-powered hardware, since
    /// continuous redraws at the compositor's refresh rate are wasted GPU
    /// time for a bar that visually changes once a second at most, this
    /// is exactly the kind of cost `reduced-motion` should also cap. Not
    /// done here to avoid rewriting the event loop in the same change
    /// that gets the bar rendering for the first time.
    fn redraw(&mut self, qh: &QueueHandle<Self>) {
        // Request the next callback before presenting, so it attaches to
        // the commit that `present_scene`'s `wgpu::Surface::present()`
        // performs (per wl_surface.frame's semantics: the callback
        // attaches to "the next commit", not to this request itself).
        let surface = self.layer.wl_surface();
        surface.frame(qh, surface.clone());

        self.graphics.clear();
        bar::render::render_bar(
            &mut self.graphics,
            &self.bar_config,
            self.bar_manager.modules_by_alignment(),
            self.width as f32,
            self.font_size,
            self.accent_color,
        );

        if let Err(error) = self.graphics.present_scene(BAR_BACKGROUND) {
            error!("Failed to present bar frame: {}", error);
            self.exit = true;
        }
    }
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
        qh: &QueueHandle<Self>,
        _surface: &wayland_client::protocol::wl_surface::WlSurface,
        _time: u32,
    ) {
        self.redraw(qh);
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
        qh: &QueueHandle<Self>,
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

        self.redraw(qh);
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
