use std::{fs, path::PathBuf, ptr::NonNull, sync::Arc, time::Duration};

use facet::Facet;
use figue::{self as args, FigueBuiltins};
use raw_window_handle::{HasDisplayHandle, RawWindowHandle, WaylandWindowHandle};
use smithay_client_toolkit::{
    compositor::CompositorState,
    output::OutputState,
    registry::RegistryState,
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell},
    },
};
use tokio::sync::mpsc;
use tracing::{Level, debug, error, info, warn};
use wayland_client::{Connection, Proxy, QueueHandle, globals::registry_queue_init};

use crate::{bar, config, graphics, ipc, launcher, lock, notification, shell::DesktopShell, wasm};

#[derive(Facet)]
struct Cli {
    /// Path to the configuration file
    #[facet(args::named, args::short = 'c')]
    config: Option<PathBuf>,

    /// Run in headless mode (no GUI)
    #[facet(args::named, args::short = 'H')]
    headless: bool,

    /// Run in lock-only mode
    #[facet(args::named, args::short = 'l')]
    lock_only: bool,

    /// Toggle the launcher
    #[facet(args::named, args::short = 't')]
    toggle_launcher: bool,

    /// Verbose logging (debug level)
    #[facet(args::named, args::short = 'v')]
    verbose: bool,

    #[facet(flatten)]
    builtins: FigueBuiltins,
}

pub async fn from_std_args() -> Result<(), Box<dyn std::error::Error>> {
    let cli: Cli = figue::from_std_args().unwrap();

    tracing_subscriber::fmt()
        .with_max_level(if cli.verbose {
            Level::DEBUG
        } else {
            Level::INFO
        })
        .init();

    // Handle lock-only robust helper execution
    if cli.lock_only {
        lock::run_lock_session_loop()?;
        return Ok(());
    }

    if cli.toggle_launcher {
        debug!("Toggle launcher signal received");
        // In a real environment, this sends an IPC packet to the running Nacre process
        return Ok(());
    }

    info!("Starting nacre desktop shell...");

    // Load configuration
    let config_path = cli.config.unwrap_or_else(|| {
        let config = dirs::config_dir().expect("Failed to determine config directory");
        config.join("nacre/config.styx")
    });

    let config_content = fs::read_to_string(&config_path).unwrap_or_default();

    let config = config::parse_config(&config_content)?;

    // Select the compositor event loop based on environmental indicators
    let ipc: Box<dyn ipc::CompositorIpc> =
        if let Some(niri_path) = ipc::niri::NiriIpc::find_socket().await {
            debug!(
                "Compositor detected: Niri. Connected to socket {:?}",
                niri_path
            );

            Box::new(ipc::niri::NiriIpc {
                socket_path: Some(niri_path),
            })
        } else if let Some(hypr_sig) = ipc::hyprland::HyprlandIpc::find_signature() {
            debug!(
                "Compositor detected: Hyprland. Connected via signature: {}",
                hypr_sig
            );

            Box::new(ipc::hyprland::HyprlandIpc {
                instance_signature: hypr_sig,
            })
        } else {
            warn!("No running Wayland compositor found. Falling back to Mock IPC events.");

            Box::new(ipc::mock::MockIpc)
        };

    // Initialize shell modules
    let bar_manager = Arc::new(bar::BarManager::new(&config.bar));
    bar_manager.start_polling();

    let _launcher_manager = Arc::new(launcher::Launcher::new());

    // Channels for async coordination
    let (ipc_sender, mut ipc_receiver) = mpsc::unbounded_channel();
    let (notif_sender, mut notif_receiver) = mpsc::unbounded_channel();

    // Start compositor IPC loop
    let _ipc_handle = ipc.run(ipc_sender);

    // Start D-Bus notification server
    notification::start_dbus_server(notif_sender).await;

    // Start plugin host thread coordinator if plugins exist
    let wasm_host = Arc::new(wasm::WasmPluginHost::new()?);
    for plugin_conf in &config.plugins {
        let wasm_host_clone = wasm_host.clone();
        let path = plugin_conf.path.clone();
        let interval = plugin_conf.interval;
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(interval as u64));
            loop {
                ticker.tick().await;
                match wasm_host_clone.execute_plugin(&path) {
                    Ok(payload) => {
                        debug!("[Plugin telemetry widget updated]: {:?}", payload);
                    }
                    Err(e) => {
                        error!("WASM plugin execution error: {}", e);
                    }
                }
            }
        });
    }

    // Running event loops
    if cli.headless {
        debug!("Running in HEADLESS simulation mode. Active event monitors:");

        loop {
            tokio::select! {
                Some(ipc_evt) = ipc_receiver.recv() => {
                    match ipc_evt {
                        ipc::CompositorEvent::StateChanged(state) => {
                            debug!("[IPC Event] Focused Window: {:?}, Workspaces: {}",
                                state.active_window_title,
                                state.workspaces.iter().map(|w| w.name.as_str()).collect::<Vec<&str>>().join(", ")
                            );
                            bar_manager.update_from_compositor(state.workspaces, state.active_window_title);
                        }
                    }
                }
                Some(notif_evt) = notif_receiver.recv() => {
                    match notif_evt {
                        notification::NotificationEvent::Add(item) => {
                            debug!("[Notification Daemon Recv]: App: {}, Summary: '{}', Body (Sanitized): '{}'",
                                item.app_name, item.summary, item.body
                            );
                        }
                        notification::NotificationEvent::Close(id) => {
                            debug!("[Notification Daemon Closed]: ID: {}", id);
                        }
                    }
                }
                else => break,
            }
        }
    } else {
        info!("Initializing Wayland layer-shell surface...");

        let conn = Connection::connect_to_env()
            .map_err(|e| format!("failed to connect to Wayland compositor: {e}"))?;
        let (globals, mut event_queue) = registry_queue_init(&conn)
            .map_err(|e| format!("failed to initialize Wayland globals: {e}"))?;
        let qh: QueueHandle<DesktopShell> = event_queue.handle();

        let compositor_state = CompositorState::bind(&globals, &qh)
            .map_err(|e| format!("wl_compositor not available: {e}"))?;
        let layer_shell = LayerShell::bind(&globals, &qh)
            .map_err(|e| format!("wlr layer shell not available: {e}"))?;

        let surface = compositor_state.create_surface(&qh);
        let layer =
            layer_shell.create_layer_surface(&qh, surface, Layer::Top, Some("nacre-shell"), None);
        layer.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
        layer.set_size(0, 32);
        layer.commit();

        let raw_display_handle = conn
            .backend()
            .display_handle()
            .map_err(|error| format!("failed to read Wayland display handle: {error}"))?
            .as_raw();
        let surface_ptr = layer.wl_surface().id().as_ptr() as *mut std::ffi::c_void;
        let raw_window_handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(
            NonNull::new(surface_ptr).ok_or("Wayland surface pointer was null")?,
        ));

        let mut shell = DesktopShell {
            registry_state: RegistryState::new(&globals),
            output_state: OutputState::new(&globals, &qh),
            layer,
            graphics: graphics::GraphicsEngine::new(false),
            exit: false,
            width: 1,
            height: 1,
        };

        shell
            .graphics
            .init_gpu(raw_display_handle, raw_window_handle)
            .map_err(|e| format!("failed to initialize GPU surface: {e}"))?;

        let _notif_forwarder = tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(ipc_evt) = ipc_receiver.recv() => {
                        match ipc_evt {
                            ipc::CompositorEvent::StateChanged(state) => {
                                debug!(
                                    "[IPC Event] Focused Window: {:?}, Workspaces: {}",
                                    state.active_window_title,
                                    state.workspaces.iter().map(|w| w.name.as_str()).collect::<Vec<&str>>().join(", ")
                                );
                                bar_manager.update_from_compositor(state.workspaces, state.active_window_title);
                            }
                        }
                    }
                    Some(notif_evt) = notif_receiver.recv() => {
                        match notif_evt {
                            notification::NotificationEvent::Add(item) => {
                                debug!("[Notification Daemon Recv]: App: {}, Summary: '{}', Body (Sanitized): '{}'",
                                    item.app_name, item.summary, item.body
                                );
                            }
                            notification::NotificationEvent::Close(id) => {
                                debug!("[Notification Daemon Closed]: ID: {}", id);
                            }
                        }
                    }
                    else => break,
                }
            }
        });

        loop {
            event_queue
                .blocking_dispatch(&mut shell)
                .map_err(|e| format!("Wayland dispatch failed: {e}"))?;

            if shell.exit {
                break;
            }
        }
    }

    Ok(())
}
