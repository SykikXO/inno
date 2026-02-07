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
        if let Some(prefix) = &self.path_prefix {
            parts.push(format!("path_namespace='{}'", prefix));
        }
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
        if let Some(ref i) = self.interface {
            if i != interface {
                return false;
            }
        }
        if let Some(ref m) = self.member {
            if m != member {
                return false;
            }
        }
        if let Some(ref p) = self.path {
            if p != path {
                return false;
            }
        }
        if let Some(ref prefix) = self.path_prefix {
            if !path.starts_with(prefix) {
                return false;
            }
        }
        true
    }
}

/// Format configuration for notifications
#[derive(Debug, Clone, Deserialize, Default)]
pub struct FormatConfig {
    #[serde(default)]
    pub message: String,
    #[allow(dead_code)]
    #[serde(default)]
    pub signal: Option<String>,
}

/// Condition configuration for triggering notifications
#[derive(Debug, Clone, Deserialize)]
pub struct ConditionsConfig {
    #[serde(default)]
    pub trigger_on: Vec<String>,
    #[serde(default = "default_debounce")]
    pub debounce_ms: u64,
    #[serde(default)]
    pub require_all: bool, // AND logic when true, OR when false
}

impl Default for ConditionsConfig {
    fn default() -> Self {
        Self {
            trigger_on: vec![],
            debounce_ms: 0,
            require_all: false,
        }
    }
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
                                    eprintln!("  Loaded event: {} ({})", event.name, path.display());
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
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Read error: {}", e))?;
    
    toml::from_str(&content)
        .map_err(|e| format!("Parse error: {}", e))
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
        format: FormatConfig {
            message: "{percentage}%".to_string(),
            signal: None,
        },
        conditions: ConditionsConfig {
            trigger_on: vec![],
            debounce_ms: 1000,
            require_all: false,
        },
    }
}

/// Format message using extracted values
pub fn format_message(
    template: &str,
    values: &HashMap<String, String>,
) -> String {
    let mut result = template.to_string();
    for (key, value) in values {
        result = result.replace(&format!("{{{}}}", key), value);
    }
    result
}
