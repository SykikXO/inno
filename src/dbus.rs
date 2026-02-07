//! DBus event listener for battery and custom events
//!
//! Listens for DBus signals based on configurable event definitions.

use crate::events::{EventConfig, format_message};
use futures::StreamExt;
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::mpsc;
use zbus::zvariant::Value;
use zbus::Connection;

/// Notification event sent to main loop
#[derive(Debug, Clone)]
pub struct NotifyEvent {
    pub event_name: String,
    #[allow(dead_code)]
    pub message: String,
    #[allow(dead_code)]
    pub values: HashMap<String, String>,
    /// Percentage for signal matching (if applicable)
    pub percentage: Option<f64>,
    /// State string for signal matching (if applicable)
    pub state: Option<String>,
}

pub enum Event {
    Notify(NotifyEvent),
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

/// Extract u32 from a Value
fn extract_u32(val: &Value) -> Option<u32> {
    match val {
        Value::U32(v) => Some(*v),
        Value::I32(v) => Some(*v as u32),
        Value::Value(inner) => extract_u32(inner),
        _ => None,
    }
}

/// Convert Value to String for display
fn value_to_string(val: &Value, state_map: &HashMap<String, String>) -> String {
    match val {
        Value::U32(v) => {
            // Check state map with string key
            state_map.get(&v.to_string()).cloned().unwrap_or_else(|| v.to_string())
        }
        Value::I32(v) => {
            state_map.get(&v.to_string()).cloned().unwrap_or_else(|| v.to_string())
        }
        Value::F64(v) => format!("{:.0}", v),
        Value::I64(v) => v.to_string(),
        Value::U64(v) => v.to_string(),
        Value::Str(s) => s.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Value(inner) => value_to_string(inner, state_map),
        _ => format!("{:?}", val),
    }
}

/// Map UPower state number to string
fn upower_state_to_string(state: u32) -> String {
    match state {
        1 => "charging".to_string(),
        2 => "discharging".to_string(),
        4 => "full".to_string(),
        _ => "unknown".to_string(),
    }
}

/// Query full battery state from UPower
async fn query_battery_state(conn: &Connection, path: &str) -> Option<(f64, String)> {
    // Query Percentage
    let percentage = conn.call_method(
        Some("org.freedesktop.UPower"),
        path,
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &("org.freedesktop.UPower.Device", "Percentage"),
    ).await.ok()
    .and_then(|reply| {
        reply.body().deserialize::<Value>().ok()
            .and_then(|v| extract_f64(&v))
    })?;
    
    // Query State
    let state = conn.call_method(
        Some("org.freedesktop.UPower"),
        path,
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &("org.freedesktop.UPower.Device", "State"),
    ).await.ok()
    .and_then(|reply| {
        reply.body().deserialize::<Value>().ok()
            .and_then(|v| extract_u32(&v))
    })
    .map(upower_state_to_string)
    .unwrap_or_else(|| "unknown".to_string());
    
    Some((percentage, state))
}


/// Run the DBus listener with configurable events
pub async fn run_dbus_listener(
    tx: mpsc::Sender<Event>,
    events: Vec<EventConfig>,
) -> anyhow::Result<()> {
    // Separate events by bus type
    let system_events: Vec<_> = events.iter().filter(|e| e.bus == "system").collect();
    let session_events: Vec<_> = events.iter().filter(|e| e.bus == "session").collect();

    eprintln!("Starting DBus listeners: {} system, {} session events",
        system_events.len(), session_events.len());

    // Start system bus listener if we have system events
    if !system_events.is_empty() {
        let tx_clone = tx.clone();
        let events_clone: Vec<EventConfig> = system_events.into_iter().cloned().collect();
        tokio::spawn(async move {
            if let Err(e) = run_bus_listener("system", tx_clone, events_clone).await {
                eprintln!("System bus listener error: {}", e);
            }
        });
    }

    // Start session bus listener if we have session events
    if !session_events.is_empty() {
        let tx_clone = tx.clone();
        let events_clone: Vec<EventConfig> = session_events.into_iter().cloned().collect();
        tokio::spawn(async move {
            if let Err(e) = run_bus_listener("session", tx_clone, events_clone).await {
                eprintln!("Session bus listener error: {}", e);
            }
        });
    }

    // Keep the main task alive
    futures::future::pending::<()>().await;
    Ok(())
}

async fn run_bus_listener(
    bus_type: &str,
    tx: mpsc::Sender<Event>,
    events: Vec<EventConfig>,
) -> anyhow::Result<()> {
    let conn = if bus_type == "system" {
        Connection::system().await?
    } else {
        Connection::session().await?
    };

    // Build match rules for all events
    for event in &events {
        let match_rule = event.match_rule.to_match_string();
        eprintln!("Adding match rule for '{}': {}", event.name, match_rule);
        
        conn.call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "AddMatch",
            &match_rule,
        ).await?;
    }

    // Debounce tracking
    let mut last_trigger: HashMap<String, Instant> = HashMap::new();

    // Listen for messages
    let mut stream = zbus::MessageStream::from(&conn);
    while let Some(msg_result) = stream.next().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(_) => continue,
        };

        // Get message header info
        let header = msg.header();
        let interface = match header.interface() {
            Some(i) => i.to_string(),
            None => continue,
        };
        let member = match header.member() {
            Some(m) => m.to_string(),
            None => continue,
        };
        let path = header.path().map(|p| p.to_string()).unwrap_or_default();

        // Find matching event config
        for event in &events {
            if !event.match_rule.matches(&interface, &member, &path) {
                continue;
            }

            // Check arg0 if specified
            if let Some(ref expected_arg0) = event.match_rule.arg0 {
                if let Ok((arg0, _, _)) = msg.body().deserialize::<(String, HashMap<String, Value>, Vec<String>)>() {
                    if &arg0 != expected_arg0 {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            // Parse message body (PropertiesChanged format)
            if let Ok((_, changed_props, _)) = msg.body().deserialize::<(String, HashMap<String, Value>, Vec<String>)>() {
                // Check conditions
                let should_trigger = if event.conditions.trigger_on.is_empty() {
                    true
                } else if event.conditions.require_all {
                    event.conditions.trigger_on.iter().all(|k| changed_props.contains_key(k))
                } else {
                    event.conditions.trigger_on.iter().any(|k| changed_props.contains_key(k))
                };

                if !should_trigger {
                    continue;
                }

                // Check debounce
                let now = Instant::now();
                if event.conditions.debounce_ms > 0 {
                    if let Some(last) = last_trigger.get(&event.name) {
                        if now.duration_since(*last).as_millis() < event.conditions.debounce_ms as u128 {
                            continue;
                        }
                    }
                }
                last_trigger.insert(event.name.clone(), now);

                // For battery events, query full state instead of relying on changed_props only
                let is_battery_event = path.contains("battery") || path.contains("BAT");
                let (percentage, state) = if is_battery_event {
                    // Query full battery state from UPower
                    if let Some((pct, st)) = query_battery_state(&conn, &path).await {
                        eprintln!("Battery state query: {:.0}% {}", pct, st);
                        (Some(pct), Some(st))
                    } else {
                        // Fall back to extracting from changed properties
                        let pct = changed_props.get("Percentage").and_then(|v| extract_f64(v));
                        let st = changed_props.get("State")
                            .and_then(|v| extract_u32(v))
                            .map(upower_state_to_string);
                        (pct, st)
                    }
                } else {
                    // Non-battery events: extract from changed properties
                    let mut pct = None;
                    let mut st = None;
                    for (field_name, prop_path) in &event.extract {
                        if let Some(value) = changed_props.get(prop_path) {
                            if field_name == "percentage" {
                                pct = extract_f64(value);
                            }
                            if field_name == "state" {
                                st = Some(value_to_string(value, &event.state_map));
                            }
                        }
                    }
                    (pct, st)
                };

                // Build values map
                let mut values: HashMap<String, String> = HashMap::new();
                if let Some(pct) = percentage {
                    values.insert("percentage".to_string(), format!("{:.0}", pct));
                }
                if let Some(ref st) = state {
                    values.insert("state".to_string(), st.clone());
                }

                // Format message
                let message = format_message(&event.format.message, &values);

                eprintln!("Event '{}' triggered: {} (pct={:?}, state={:?})", 
                    event.name, message, percentage, state);

                let notify_event = NotifyEvent {
                    event_name: event.name.clone(),
                    message,
                    values,
                    percentage,
                    state,
                };

                if tx.send(Event::Notify(notify_event)).await.is_err() {
                    return Ok(());
                }
            }
        }
    }

    Ok(())
}
