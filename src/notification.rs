use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use zbus::{interface, zvariant::Value};

#[derive(Debug, Clone)]
pub struct NotificationItem {
    pub id: u32,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    pub icon: String,
    pub timeout_ms: u32,
    pub progress: f32, // for fadeout animations (1.0 -> 0.0)
}

#[derive(Debug, Clone)]
pub enum NotificationEvent {
    Add(NotificationItem),
    Close(u32),
}

pub struct NotificationServer {
    next_id: Arc<Mutex<u32>>,
    sender: tokio::sync::mpsc::UnboundedSender<NotificationEvent>,
}

#[interface(name = "org.freedesktop.Notifications")]
impl NotificationServer {
    async fn get_capabilities(&self) -> Vec<String> {
        vec!["body".to_string(), "markup".to_string()]
    }

    async fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        _actions: Vec<String>,
        _hints: HashMap<String, Value<'_>>,
        expire_timeout: i32,
    ) -> u32 {
        let mut id_lock = self.next_id.lock().unwrap();
        let id = if replaces_id == 0 {
            *id_lock += 1;
            *id_lock
        } else {
            replaces_id
        };

        // Use ammonia to parse and sanitize the HTML body text, stripping tags like
        // script, iframe, etc.
        let sanitized_body = ammonia::clean(&body);

        // Normalize expire timeout
        let timeout = if expire_timeout <= 0 {
            5000
        } else {
            expire_timeout as u32
        };

        let item = NotificationItem {
            id,
            app_name,
            summary,
            body: sanitized_body,
            icon: app_icon,
            timeout_ms: timeout,
            progress: 1.0,
        };

        let _ = self.sender.send(NotificationEvent::Add(item));
        id
    }

    async fn close_notification(&self, id: u32) {
        let _ = self.sender.send(NotificationEvent::Close(id));
    }

    async fn get_server_information(&self) -> (String, String, String, String) {
        (
            "nacre-notification".to_string(),
            "Nacre".to_string(),
            "0.1.0".to_string(),
            "1.2".to_string(),
        )
    }
}

/// Start D-Bus server session. Falls back gracefully if D-Bus is not running.
pub async fn start_dbus_server(sender: tokio::sync::mpsc::UnboundedSender<NotificationEvent>) {
    let server = NotificationServer {
        next_id: Arc::new(Mutex::new(0)),
        sender,
    };

    match zbus::connection::Builder::session() {
        Ok(builder) => {
            match builder.name("org.freedesktop.Notifications") {
                Ok(named_builder) => {
                    match named_builder.serve_at("/org/freedesktop/Notifications", server) {
                        Ok(served_builder) => {
                            match served_builder.build().await {
                                Ok(_conn) => {
                                    println!(
                                        "D-Bus Notification Server published on org.freedesktop.Notifications"
                                    );
                                    return;
                                }
                                Err(e) => println!("Failed to build D-Bus connection: {}", e),
                            }
                        }
                        Err(e) => println!("Failed to register D-Bus path: {}", e),
                    }
                }
                Err(e) => {
                    println!(
                        "Failed to request D-Bus name org.freedesktop.Notifications: {}",
                        e
                    )
                }
            }
        }
        Err(e) => println!("Failed to connect to D-Bus session bus: {}", e),
    }

    println!("Warning: D-Bus notification daemon running in Mock/No-DBus fallback mode");
}
