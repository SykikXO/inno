//! Configurable DBus event definitions
//!
//! Loads event definitions from ~/.config/inno/events/*.toml

use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

/// An event definition loaded from TOML
#[derive(Debug, Clone, Deserialize)]
pub struct EventConfig {
    pub name: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_bus")]
    pub bus: String,
    #[serde(rename = "match")]
    pub match_rule: MatchRule,
    #[serde(default)]
    pub extract: HashMap<String, String>,
    #[serde(default)]
    pub state_map: HashMap<String, String>,
    #[serde(default)]
    pub format: FormatConfig,
    #[serde(default)]
    pub conditions: ConditionsConfig,
}

fn default_enabled() -> bool {
    true
}

fn default_bus() -> String {
    "system".to_string()
}

/// DBus match rule configuration
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MatchRule {
    #[serde(default)]
    pub interface: Option<String>,
    #[serde(default)]
    pub member: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub path_prefix: Option<String>,
    #[serde(default)]
    pub arg0: Option<String>,
    #[serde(default)]
    pub sender: Option<String>,
}

impl MatchRule {
    /// Build a DBus match rule string
    pub fn to_match_string(&self) -> String {
        let mut parts = vec!["type='signal'".to_string()];

        if let Some(iface) = &self.interface {
            parts.push(format!("interface='{}'", iface));
        }
        if let Some(member) = &self.member {
            parts.push(format!("member='{}'", member));
        }
        if let Some(path) = &self.path {
            parts.push(format!("path='{}'", path));
        }
        // NOTE: Do NOT include path_prefix as path_namespace here.
        // DBus path_namespace requires '/' separated hierarchy matching,
        // so path_namespace='/devices/battery_BAT' won't match '/devices/battery_BAT0'.
        // We handle prefix filtering ourselves in matches() using starts_with.
        if let Some(arg0) = &self.arg0 {
            parts.push(format!("arg0='{}'", arg0));
        }
        if let Some(sender) = &self.sender {
            parts.push(format!("sender='{}'", sender));
        }

        parts.join(",")
    }

    /// Check if a message matches this rule
    pub fn matches(&self, interface: &str, member: &str, path: &str) -> bool {
        if let Some(ref i) = self.interface
            && i != interface
        {
            return false;
        }
        if let Some(ref m) = self.member
            && m != member
        {
            return false;
        }
        if let Some(ref p) = self.path
            && p != path
        {
            return false;
        }
        if let Some(ref prefix) = self.path_prefix
            && !path.starts_with(prefix)
        {
            return false;
        }
        true
    }
}

/// Format configuration for notifications
#[derive(Debug, Clone, Deserialize, Default)]
pub struct FormatConfig {
    #[serde(default)]
    pub message: String,
}

/// Condition configuration for triggering notifications
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConditionsConfig {
    #[serde(default)]
    pub trigger_on: Vec<String>,
    #[serde(default = "default_debounce")]
    pub debounce_ms: u64,
    #[serde(default)]
    pub require_all: bool, // AND logic when true, OR when false
}

fn default_debounce() -> u64 {
    0
}

/// Load all event configs from the events directory
pub fn load_events() -> Vec<EventConfig> {
    let mut events = Vec::new();

    // Search paths for events directory
    let search_paths = [
        std::env::current_dir().ok().map(|p| p.join("events")),
        std::env::current_dir().ok().and_then(|p| p.parent().map(|pp| pp.join("events"))),
        dirs::config_dir().map(|p| p.join("inno/events")),
        Some(PathBuf::from("/etc/xdg/inno/events")),
    ];

    for events_dir in search_paths.iter().flatten() {
        if events_dir.is_dir() {
            eprintln!("Loading events from: {:?}", events_dir);
            if let Ok(entries) = std::fs::read_dir(events_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map(|e| e == "toml").unwrap_or(false) {
                        match load_event_file(&path) {
                            Ok(event) => {
                                if event.enabled {
                                    eprintln!(
                                        "  Loaded event: {} ({})",
                                        event.name,
                                        path.display()
                                    );
                                    events.push(event);
                                } else {
                                    eprintln!("  Skipped disabled event: {}", event.name);
                                }
                            }
                            Err(e) => {
                                eprintln!("  Failed to load {:?}: {}", path, e);
                            }
                        }
                    }
                }
            }
            break; // Only load from first found directory
        }
    }

    if events.is_empty() {
        eprintln!("No event configs found, using built-in battery event");
        events.push(builtin_battery_event());
    }

    events
}

/// Load a single event config file
fn load_event_file(path: &PathBuf) -> Result<EventConfig, String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("Read error: {}", e))?;

    toml::from_str(&content).map_err(|e| format!("Parse error: {}", e))
}

/// Built-in battery event as fallback
fn builtin_battery_event() -> EventConfig {
    let mut state_map = HashMap::new();
    state_map.insert("1".to_string(), "charging".to_string());
    state_map.insert("2".to_string(), "discharging".to_string());
    state_map.insert("4".to_string(), "full".to_string());

    let mut extract = HashMap::new();
    extract.insert("percentage".to_string(), "Percentage".to_string());
    extract.insert("state".to_string(), "State".to_string());

    EventConfig {
        name: "Battery (built-in)".to_string(),
        enabled: true,
        bus: "system".to_string(),
        match_rule: MatchRule {
            interface: Some("org.freedesktop.DBus.Properties".to_string()),
            member: Some("PropertiesChanged".to_string()),
            path_prefix: Some("/org/freedesktop/UPower/devices".to_string()),
            arg0: Some("org.freedesktop.UPower.Device".to_string()),
            path: None,
            sender: None,
        },
        extract,
        state_map,
        format: FormatConfig { message: "{percentage}%".to_string() },
        conditions: ConditionsConfig { trigger_on: vec![], debounce_ms: 1000, require_all: false },
    }
}

/// Format message using extracted values
pub fn format_message(template: &str, values: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in values {
        result = result.replace(&format!("{{{key}}}"), value);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_rule_exact() {
        let rule = MatchRule {
            interface: Some("org.freedesktop.DBus.Properties".into()),
            member: Some("PropertiesChanged".into()),
            path: Some("/org/freedesktop/UPower/devices/battery_BAT0".into()),
            path_prefix: None,
            arg0: None,
            sender: None,
        };

        assert!(rule.matches("org.freedesktop.DBus.Properties", "PropertiesChanged",
            "/org/freedesktop/UPower/devices/battery_BAT0"));
        assert!(!rule.matches("org.freedesktop.DBus.Properties", "PropertiesChanged",
            "/org/freedesktop/UPower/devices/battery_BAT1"));
        assert!(!rule.matches("other.Interface", "PropertiesChanged",
            "/org/freedesktop/UPower/devices/battery_BAT0"));
    }

    #[test]
    fn test_match_rule_prefix() {
        let rule = MatchRule {
            interface: Some("org.freedesktop.DBus.Properties".into()),
            member: None,
            path: None,
            path_prefix: Some("/org/freedesktop/UPower/devices".into()),
            arg0: None,
            sender: None,
        };

        assert!(rule.matches("org.freedesktop.DBus.Properties", "PropertiesChanged",
            "/org/freedesktop/UPower/devices/battery_BAT0"));
        assert!(rule.matches("org.freedesktop.DBus.Properties", "PropertiesChanged",
            "/org/freedesktop/UPower/devices/battery_BAT1"));
        assert!(!rule.matches("org.freedesktop.DBus.Properties", "PropertiesChanged",
            "/some/other/path"));
    }

    #[test]
    fn test_match_rule_minimal() {
        let rule = MatchRule {
            interface: None,
            member: None,
            path: None,
            path_prefix: None,
            arg0: None,
            sender: None,
        };

        assert!(rule.matches("any.interface", "AnyMember", "/any/path"));
    }

    #[test]
    fn test_match_rule_to_string() {
        let rule = MatchRule {
            interface: Some("org.freedesktop.DBus.Properties".into()),
            member: Some("PropertiesChanged".into()),
            path: None,
            path_prefix: None,
            arg0: Some("org.freedesktop.UPower.Device".into()),
            sender: None,
        };

        let s = rule.to_match_string();
        assert!(s.contains("type='signal'"));
        assert!(s.contains("interface='org.freedesktop.DBus.Properties'"));
        assert!(s.contains("member='PropertiesChanged'"));
        assert!(s.contains("arg0='org.freedesktop.UPower.Device'"));
    }

    #[test]
    fn test_format_message() {
        let mut values = HashMap::new();
        values.insert("percentage".into(), "75".into());
        values.insert("state".into(), "charging".into());

        assert_eq!(format_message("{percentage}% ({state})", &values), "75% (charging)");
        assert_eq!(format_message("Battery at {percentage}%", &values), "Battery at 75%");
    }

    #[test]
    fn test_format_message_missing_key() {
        let values = HashMap::new();
        assert_eq!(format_message("{missing}", &values), "{missing}");
    }

    #[test]
    fn test_builtin_battery_event() {
        let event = builtin_battery_event();
        assert_eq!(event.name, "Battery (built-in)");
        assert!(event.enabled);
        assert_eq!(event.bus, "system");
        assert_eq!(event.match_rule.arg0, Some("org.freedesktop.UPower.Device".into()));
        assert_eq!(event.format.message, "{percentage}%");
        assert_eq!(event.conditions.debounce_ms, 1000);
    }
}
