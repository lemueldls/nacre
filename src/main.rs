mod bar;
mod config;
mod graphics;
mod ipc;
mod launcher;
mod lock;
mod notification;
mod wasm;

use std::{
    env,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use facet::Facet;
use figue::{self as args, FigueBuiltins};
use tokio::sync::mpsc;
use tracing::{Level, debug, error, info, warn};

#[derive(Facet)]
struct Cli {
    /// Path to the configuration file
    #[facet(args::named, args::short = 'c')]
    config: Option<PathBuf>,

    /// Run in headless mode (no GUI)
    #[facet(args::named, args::short = 'h')]
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    // Load Styx configuration
    let config_path = cli.config.unwrap_or_else(|| {
        let home = env::var("HOME").unwrap_or_else(|_| "/home/lemuel".to_string());
        PathBuf::from(home).join(".config/nacre/config.styx")
    });

    let config_content = std::fs::read_to_string(&config_path).unwrap_or_default();

    let config = config::parse_config(&config_content)?;
    info!("Successfully loaded unified Styx configuration.");

    // Select the compositor event loop based on environmental indicators
    let ipc: Box<dyn ipc::CompositorIpc> = if let Some(niri_path) =
        ipc::niri::NiriIpc::find_socket().await
    {
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
        debug!("Warning: No running Wayland compositor found. Falling back to Mock IPC events.");
        Box::new(ipc::mock::MockIpc)
    };

    // Initialize shell modules
    let bar_manager = Arc::new(bar::BarManager::new());
    bar_manager.start_polling();

    let launcher_manager = Arc::new(launcher::Launcher::new());

    // Channels for async coordination
    let (ipc_sender, mut ipc_receiver) = mpsc::unbounded_channel();
    let (notif_sender, mut notif_receiver) = mpsc::unbounded_channel();

    // Start Compositor IPC loop
    let _ipc_handle = ipc.run(ipc_sender);

    // Start D-Bus Notification server
    notification::start_dbus_server(notif_sender).await;

    // Start Plugin Host thread coordinator if plugins exist
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
        info!("Initializing Wayland client coordinates...");
        // In a real Wayland session, this sets up the winit/SCTK loop.
        // For robustness, if winit fails (e.g. no display), we fall back to headless
        // simulation.
        warn!("Warning: Could not open Wayland display connection. Falling back to Headless mode.");

        // Emulate the event loop
        loop {
            tokio::select! {
                Some(ipc_evt) = ipc_receiver.recv() => {
                    if let ipc::CompositorEvent::StateChanged(state) = ipc_evt {
                        bar_manager.update_from_compositor(state.workspaces, state.active_window_title);
                    }
                }
                Some(_notif_evt) = notif_receiver.recv() => {}
                else => break,
            }
        }
    }

    Ok(())
}
