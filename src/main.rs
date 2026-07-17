mod bar;
mod cli;
mod config;
mod graphics;
mod ipc;
mod launcher;
mod lock;
mod notification;
mod shell;
mod wasm;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    cli::from_std_args().await
}
