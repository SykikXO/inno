//! DBus event listener for battery and custom events
//!
//! Listens for DBus signals based on configurable event definitions.

use crate::events::{self, EventConfig};
use futures::{StreamExt, future::join_all};
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::mpsc;
use zbus::Connection;
use zbus::fdo::PropertiesProxy;
use zbus::names::InterfaceName;
use zbus::zvariant::{OwnedValue, Value};

/// Notification event sent to main loop
#[derive(Debug, Clone)]
pub struct NotifyEvent {
    pub event_name: String,
    pub path: String,
    pub message: String,
    /// Percentage for signal matching (if applicable)
    pub percentage: Option<f64>,
    /// State string for signal matching (if applicable)
    pub state: Option<String>,
    /// True if this event originated from a UPower battery device
    pub is_battery: bool,
}

pub struct Event {
    pub notify: NotifyEvent,
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
        Value::I32(v) => state_map.get(&v.to_string()).cloned().unwrap_or_else(|| v.to_string()),
        Value::F64(v) => format!("{:.0}", v),
        Value::I64(v) => v.to_string(),
        Value::U64(v) => v.to_string(),
        Value::Str(s) => state_map.get(s.as_str()).cloned().unwrap_or_else(|| s.to_string()),
        Value::Bool(b) => {
            let s = b.to_string();
            state_map.get(&s).cloned().unwrap_or(s)
        }
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

/// Current battery percentage, for callers that only need the number.
///
/// `--test-signal` uses this so a preview shows the real level rather than a
/// placeholder that looks like a reading.
pub async fn battery_percentage_now() -> Option<f64> {
    let conn = Connection::system().await.ok()?;
    let proxy = zbus::proxy::Proxy::new(
        &conn,
        "org.freedesktop.UPower",
        "/org/freedesktop/UPower",
        "org.freedesktop.UPower",
    )
    .await
    .ok()?;

    // EnumerateDevices returns `ao`, object paths, not plain strings.
    let devices: Vec<zbus::zvariant::OwnedObjectPath> =
        proxy.call("EnumerateDevices", &()).await.ok()?;

    for path in devices {
        let path = path.as_str();
        if !path.contains("battery") {
            continue;
        }
        if let Some((pct, _)) = query_battery_state(&conn, path).await {
            return Some(pct);
        }
    }
    None
}

/// Reads one property off an object. `PropertiesProxy` is zbus's own wrapper
/// over org.freedesktop.DBus.Properties, so the method name and the reply
/// deserialization do not need writing out by hand.
async fn get_property(
    conn: &Connection,
    destination: &str,
    path: &str,
    interface: &str,
    property: &str,
) -> Option<OwnedValue> {
    let proxy = PropertiesProxy::builder(conn)
        .destination(destination)
        .ok()?
        .path(path)
        .ok()?
        .build()
        .await
        .ok()?;
    proxy.get(InterfaceName::try_from(interface).ok()?, property).await.ok()
}

/// Query full battery state from UPower
async fn query_battery_state(conn: &Connection, path: &str) -> Option<(f64, String)> {
    const IFACE: &str = "org.freedesktop.UPower.Device";
    let value = get_property(conn, "org.freedesktop.UPower", path, IFACE, "Percentage").await?;
    let percentage = extract_f64(&value)?;

    let state = get_property(conn, "org.freedesktop.UPower", path, IFACE, "State")
        .await
        .and_then(|v| extract_u32(&v))
        .map(upower_state_to_string)
        .unwrap_or_else(|| "unknown".to_string());

    Some((percentage, state))
}

/// Query BlueZ device alias (name)
async fn query_bluez_alias(conn: &Connection, path: &str) -> Option<String> {
    let value = get_property(conn, "org.bluez", path, "org.bluez.Device1", "Alias").await?;
    match &*value {
        Value::Str(s) => Some(s.to_string()),
        Value::Value(inner) => match inner.as_ref() {
            Value::Str(s) => Some(s.to_string()),
            _ => None,
        },
        other => {
            eprintln!("Failed to extract alias from Value: {:?}", other);
            None
        }
    }
}

/// Run the DBus listener with configurable events
pub async fn run_dbus_listener(
    tx: mpsc::Sender<Event>,
    events: Vec<EventConfig>,
) -> anyhow::Result<()> {
    // partition moves each config into exactly one bus. Collecting references
    // and cloning them per listener cost a heap allocation per config field.
    let (session_events, system_events): (Vec<EventConfig>, Vec<EventConfig>) =
        events.into_iter().partition(|e| e.bus == "session");

    eprintln!(
        "Starting DBus listeners: {} system, {} session events",
        system_events.len(),
        session_events.len()
    );

    let mut handles = Vec::new();

    if !system_events.is_empty() {
        let tx_clone = tx.clone();
        let events = system_events;
        handles.push(tokio::spawn(async move {
            if let Err(e) = run_bus_listener("system", tx_clone, events).await {
                eprintln!("System bus listener error: {}", e);
            }
        }));
    }

    if !session_events.is_empty() {
        let tx_clone = tx.clone();
        let events = session_events;
        handles.push(tokio::spawn(async move {
            if let Err(e) = run_bus_listener("session", tx_clone, events).await {
                eprintln!("Session bus listener error: {}", e);
            }
        }));
    }

    join_all(handles).await;
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
        )
        .await?;
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

            // Parse message body once (PropertiesChanged format)
            let body = msg.body();
            let Ok((arg0, changed_props, _)) =
                body.deserialize::<(String, HashMap<String, Value>, Vec<String>)>()
            else {
                continue;
            };

            // Check arg0 if specified
            if let Some(ref expected_arg0) = event.match_rule.arg0
                && &arg0 != expected_arg0
            {
                continue;
            }

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
            if event.conditions.debounce_ms > 0
                && let Some(last) = last_trigger.get(&event.name)
                && now.duration_since(*last).as_millis() < event.conditions.debounce_ms as u128
            {
                continue;
            }

            // Use event config metadata instead of fragile path string matching
            let is_battery_event = event.match_rule.arg0.as_deref() == Some("org.freedesktop.UPower.Device");
            let is_bluetooth_event = event.match_rule.arg0.as_deref() == Some("org.bluez.Device1");

            // One extraction path for every event kind: the event's own
            // [extract] table names the property behind each field and
            // [state_map] turns it into a state string. Battery events used to
            // hardcode "Percentage" and "State" and call upower_state_to_string
            // instead, so editing either table in a battery event file changed
            // nothing at all.
            let mut percentage = None;
            let mut state = None;
            for (field, property) in &event.extract {
                let Some(value) = changed_props.get(property) else { continue };
                match field.as_str() {
                    "percentage" => percentage = extract_f64(value),
                    "state" => state = Some(value_to_string(value, &event.state_map)),
                    _ => {}
                }
            }

            // A battery sometimes reports its properties through a separate
            // UPower Get instead of the PropertiesChanged body. Ask once, and
            // only when the body carried no reading, so the common case costs no
            // round trip on a runtime that also drives the frame clock.
            if is_battery_event
                && percentage.is_none()
                && let Some((pct, st)) = query_battery_state(&conn, &path).await
            {
                eprintln!("Battery state query: {:.0}% {}", pct, st);
                percentage = Some(pct);
                state = Some(st);
            }

            // Build values map
            let mut values: HashMap<String, String> = HashMap::new();
            if let Some(pct) = percentage {
                values.insert("percentage".to_string(), format!("{:.0}", pct));
            }
            if let Some(ref st) = state {
                values.insert("state".to_string(), st.clone());
            }

            // Inject Bluetooth device name
            if is_bluetooth_event {
                if let Some(alias) = query_bluez_alias(&conn, &path).await {
                    values.insert("name".to_string(), alias);
                } else {
                    values.insert("name".to_string(), "Bluetooth Device".to_string());
                }
            }

            // Format message
            let message = events::render(&event.format.message, &values);

            eprintln!(
                "Event '{}' triggered: {} (pct={:?}, state={:?})",
                event.name, message, percentage, state
            );

            let notify_event = NotifyEvent {
                event_name: event.name.clone(),
                path: path.clone(),
                message,
                percentage,
                state,
                is_battery: is_battery_event,
            };

            if tx.send(Event { notify: notify_event }).await.is_err() {
                return Ok(());
            }

            // Record debounce only after successful send
            last_trigger.insert(event.name.clone(), now);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use zbus::zvariant::Value;

    fn state_map() -> HashMap<String, String> {
        let mut map = HashMap::new();
        map.insert("1".to_string(), "charging".to_string());
        map.insert("2".to_string(), "discharging".to_string());
        map.insert("full".to_string(), "charged".to_string());
        map
    }

    #[test]
    fn test_upower_state_to_string_covers_known_states() {
        assert_eq!(upower_state_to_string(1), "charging");
        assert_eq!(upower_state_to_string(2), "discharging");
        assert_eq!(upower_state_to_string(4), "full");
        // 0 is unknown, 3 is the "full but not charging" sentinel, and anything
        // out of range must not be guessed at.
        assert_eq!(upower_state_to_string(0), "unknown");
        assert_eq!(upower_state_to_string(3), "unknown");
        assert_eq!(upower_state_to_string(99), "unknown");
    }

    #[test]
    fn test_value_to_string_applies_the_state_map() {
        let map = state_map();
        assert_eq!(value_to_string(&Value::from(1u32), &map), "charging");
        assert_eq!(value_to_string(&Value::from(2u32), &map), "discharging");
        assert_eq!(value_to_string(&Value::from("full"), &map), "charged");
    }

    #[test]
    fn test_value_to_string_falls_back_to_the_raw_value() {
        let map = state_map();
        assert_eq!(value_to_string(&Value::from(7u32), &map), "7");
        assert_eq!(value_to_string(&Value::from("unknown-key"), &map), "unknown-key");
    }

    #[test]
    fn test_value_to_string_renders_unmapped_types() {
        let map = state_map();
        assert_eq!(value_to_string(&Value::from(42.7f64), &map), "43");
        assert_eq!(value_to_string(&Value::from(5i64), &map), "5");
        assert_eq!(value_to_string(&Value::from(true), &map), "true");
    }

    #[test]
    fn test_value_to_string_unwraps_nested_variants() {
        let map = state_map();
        let nested = Value::Value(Box::new(Value::from(1u32)));
        assert_eq!(value_to_string(&nested, &map), "charging");
    }

    #[test]
    fn test_extract_f64_widens_integer_types() {
        assert_eq!(extract_f64(&Value::from(1.5f64)), Some(1.5));
        assert_eq!(extract_f64(&Value::from(80u32)), Some(80.0));
        assert_eq!(extract_f64(&Value::from(-3i32)), Some(-3.0));
        assert_eq!(extract_f64(&Value::from(7i64)), Some(7.0));
    }

    #[test]
    fn test_extract_f64_rejects_non_numeric_values() {
        assert_eq!(extract_f64(&Value::from("nope")), None);
        assert_eq!(extract_f64(&Value::from(true)), None);
    }

    #[test]
    fn test_extract_f64_unwraps_nested_variants() {
        let nested = Value::Value(Box::new(Value::from(80u32)));
        assert_eq!(extract_f64(&nested), Some(80.0));
    }

    #[test]
    fn test_extract_u32_accepts_signed_and_unsigned() {
        assert_eq!(extract_u32(&Value::from(42u32)), Some(42));
        assert_eq!(extract_u32(&Value::from(42i32)), Some(42));
        assert_eq!(extract_u32(&Value::from(true)), None);
    }

    #[test]
    fn test_extract_u32_unwraps_nested_variants() {
        let nested = Value::Value(Box::new(Value::from(42u32)));
        assert_eq!(extract_u32(&nested), Some(42));
    }
}
