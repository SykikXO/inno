//! DBus control interface for inno
//! 
//! Exposes org.inno.Control interface on session bus for external control.
//! Methods: Show(message), Hide, GetState, Reload

use tokio::sync::mpsc;
use zbus::interface;

/// Control events sent from DBus to main loop
#[derive(Debug, Clone)]
pub enum ControlEvent {
    /// Show a custom notification message
    Show { message: String, duration: u64 },
    /// Hide the current notification
    Hide,
    /// Reload configuration
    Reload,
}

/// DBus control service
pub struct InnoService {
    pub tx: mpsc::Sender<ControlEvent>,
    pub battery_percentage: std::sync::Arc<std::sync::atomic::AtomicU32>,
    pub battery_state: std::sync::Arc<std::sync::RwLock<String>>,
}

#[interface(name = "org.inno.Control")]
impl InnoService {
    /// Show a custom notification
    async fn show(&self, message: String, duration: u64) -> zbus::fdo::Result<()> {
        self.tx
            .send(ControlEvent::Show { message, duration })
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        Ok(())
    }

    /// Hide current notification
    async fn hide(&self) -> zbus::fdo::Result<()> {
        self.tx
            .send(ControlEvent::Hide)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        Ok(())
    }

    /// Get current battery state
    fn get_state(&self) -> zbus::fdo::Result<(f64, String)> {
        let pct = self
            .battery_percentage
            .load(std::sync::atomic::Ordering::Relaxed) as f64
            / 100.0;
        let state = self.battery_state.read().unwrap().clone();
        Ok((pct * 100.0, state))
    }

    /// Reload configuration
    async fn reload(&self) -> zbus::fdo::Result<()> {
        self.tx
            .send(ControlEvent::Reload)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        Ok(())
    }

    /// Get daemon version
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }
}

/// Start the DBus control interface
pub async fn start_control_service(
    tx: mpsc::Sender<ControlEvent>,
    battery_percentage: std::sync::Arc<std::sync::atomic::AtomicU32>,
    battery_state: std::sync::Arc<std::sync::RwLock<String>>,
) -> anyhow::Result<zbus::Connection> {
    let conn = zbus::Connection::session().await?;

    let service = InnoService {
        tx,
        battery_percentage,
        battery_state,
    };

    conn.object_server()
        .at("/org/inno/Control", service)
        .await?;

    conn.request_name("org.inno.Control").await?;

    eprintln!("DBus control interface registered at org.inno.Control");

    Ok(conn)
}
