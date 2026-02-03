use tokio::sync::mpsc;
use zbus::{Connection, Message};
use zbus::zvariant::ObjectPath;
use futures::StreamExt;
use std::collections::HashMap;
use zbus::zvariant::Value;

/// Current battery state with all relevant data
#[derive(Debug, Clone)]
pub struct BatteryState {
    pub percentage: f64,
    pub state: String,           // "charging", "discharging", "full"
    pub time_to_empty: Option<f64>,  // minutes
    pub time_to_full: Option<f64>,   // minutes
}

impl Default for BatteryState {
    fn default() -> Self {
        Self {
            percentage: 100.0,
            state: "unknown".to_string(),
            time_to_empty: None,
            time_to_full: None,
        }
    }
}

pub enum Event {
    Battery(BatteryState),
    StateChange(String),  // Simple state change notification (charging/discharging/full)
}

/// Extract f64 from a Value, unwrapping nested variants
fn extract_f64(val: &Value) -> Option<f64> {
    match val {
        Value::F64(v) => Some(*v),
        Value::U32(v) => Some(*v as f64),
        Value::I32(v) => Some(*v as f64),
        Value::I64(v) => Some(*v as f64),
        Value::U64(v) => Some(*v as f64),
        Value::Value(inner) => extract_f64(inner),
        _ => None,
    }
}

/// Extract u32 from a Value, unwrapping nested variants
fn extract_u32(val: &Value) -> Option<u32> {
    match val {
        Value::U32(v) => Some(*v),
        Value::I32(v) => Some(*v as u32),
        Value::Value(inner) => extract_u32(inner),
        _ => None,
    }
}

/// Extract i64 from a Value, unwrapping nested variants
fn extract_i64(val: &Value) -> Option<i64> {
    match val {
        Value::I64(v) => Some(*v),
        Value::U64(v) => Some(*v as i64),
        Value::I32(v) => Some(*v as i64),
        Value::U32(v) => Some(*v as i64),
        Value::Value(inner) => extract_i64(inner),
        _ => None,
    }
}

pub async fn run_dbus_listener(tx: mpsc::Sender<Event>) -> anyhow::Result<()> {
    let conn = Connection::system().await?;

    // Match on battery device properties
    conn.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "AddMatch",
        &("type='signal',path='/org/freedesktop/UPower/devices/battery_BAT0',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged'"),
    ).await.ok();
    
    conn.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "AddMatch",
        &("type='signal',path='/org/freedesktop/UPower/devices/battery_BAT1',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged'"),
    ).await.ok();

    conn.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "AddMatch",
        &("type='signal',path='/org/freedesktop/UPower/devices/line_power_ADP1',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged'"),
    ).await?;
    
    conn.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "AddMatch",
        &("type='signal',interface='org.freedesktop.DBus.ObjectManager',member='InterfacesAdded'"),
    ).await?;

    let mut stream = zbus::MessageStream::from(conn.clone());
    
    // Get initial battery state
    if let Ok(initial_state) = get_battery_state(&conn).await {
        let _ = tx.send(Event::Battery(initial_state)).await;
    }

    while let Some(msg_res) = stream.next().await {
        if let Ok(msg) = msg_res {
            let header = msg.header();
            let path = header.path().map(|p| p.to_string()).unwrap_or_default();
            if !path.contains("UPower") { continue; }

            let member = header.member().map(|m| m.to_string()).unwrap_or_default();
            
            if member == "PropertiesChanged" {
                handle_properties_changed(&msg, &tx, &conn).await;
            } else if member == "InterfacesAdded" {
               let _ = tx.send(Event::StateChange(format!("Connected: {}", path))).await;
            }
        }
    }
    Ok(())
}

async fn get_battery_state(conn: &Connection) -> anyhow::Result<BatteryState> {
    let mut state = BatteryState::default();
    
    // Use string slice paths directly
    let bat_paths = [
        "/org/freedesktop/UPower/devices/battery_BAT0",
        "/org/freedesktop/UPower/devices/battery_BAT1",
    ];
    
    for path in &bat_paths {
        let obj_path = ObjectPath::try_from(*path)?;
        
        // Get Percentage
        if let Ok(reply) = conn.call_method(
            Some("org.freedesktop.UPower"),
            obj_path.clone(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.UPower.Device", "Percentage"),
        ).await {
            if let Ok(val) = reply.body().deserialize::<Value>() {
                if let Some(pct) = extract_f64(&val) {
                    state.percentage = pct;
                }
            }
        }
        
        // Get State
        if let Ok(reply) = conn.call_method(
            Some("org.freedesktop.UPower"),
            obj_path.clone(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.UPower.Device", "State"),
        ).await {
            if let Ok(val) = reply.body().deserialize::<Value>() {
                if let Some(s) = extract_u32(&val) {
                    state.state = state_to_string(s);
                }
            }
        }
        
        // Get TimeToEmpty
        if let Ok(reply) = conn.call_method(
            Some("org.freedesktop.UPower"),
            obj_path.clone(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.UPower.Device", "TimeToEmpty"),
        ).await {
            if let Ok(val) = reply.body().deserialize::<Value>() {
                if let Some(secs) = extract_i64(&val) {
                    if secs > 0 {
                        state.time_to_empty = Some(secs as f64 / 60.0);
                    }
                }
            }
        }
        
        // Get TimeToFull
        if let Ok(reply) = conn.call_method(
            Some("org.freedesktop.UPower"),
            obj_path.clone(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.UPower.Device", "TimeToFull"),
        ).await {
            if let Ok(val) = reply.body().deserialize::<Value>() {
                if let Some(secs) = extract_i64(&val) {
                    if secs > 0 {
                        state.time_to_full = Some(secs as f64 / 60.0);
                    }
                }
            }
        }
        
        // If we got valid percentage, break
        if state.percentage < 100.0 || state.state != "unknown" {
            break;
        }
    }
    
    Ok(state)
}

fn state_to_string(state: u32) -> String {
    match state {
        1 => "charging".to_string(),
        2 => "discharging".to_string(),
        4 => "full".to_string(),
        _ => "unknown".to_string(),
    }
}

async fn handle_properties_changed(msg: &Message, tx: &mpsc::Sender<Event>, conn: &Connection) {
    if let Ok((iface, changed, _invalidated)) = msg.body().deserialize::<(String, HashMap<String, Value>, Vec<String>)>() {
        if iface.contains("UPower.Device") {
            // Check if any battery-related property changed
            let has_battery_change = changed.keys().any(|k| {
                matches!(k.as_str(), "Percentage" | "State" | "TimeToEmpty" | "TimeToFull")
            });
            
            if has_battery_change {
                // Fetch full battery state
                if let Ok(state) = get_battery_state(conn).await {
                    eprintln!("Battery update: {:.0}% {:?} (empty: {:?}min, full: {:?}min)", 
                        state.percentage, state.state, state.time_to_empty, state.time_to_full);
                    let _ = tx.send(Event::Battery(state)).await;
                }
            }
            
            // Also send simple state change for immediate feedback
            for (key, value) in changed {
                if key == "State" {
                    if let Some(state) = extract_u32(&value) {
                        let text = match state {
                            1 => "Charging",
                            2 => "Discharging",
                            4 => "Battery Full",
                            _ => continue,
                        };
                        let _ = tx.send(Event::StateChange(text.to_string())).await;
                    }
                }
            }
        }
    }
}
