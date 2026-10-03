//! Configurable DBus event definitions
//!
//! Loads event definitions from ~/.config/inno/events/*.toml

use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

/// Substitutes `{key}` in an event's message for each value supplied. A key
/// with no value is left exactly as written, so a typo in an event file shows
/// up in the notification instead of quietly rendering as nothing.
pub fn render(message: &str, values: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    // One left-to-right pass, so a substituted value is never rescanned. `name`
    // comes from the BlueZ Alias property, which the advertising device
    // controls; an Alias of "{state}" must render as that text rather than as
    // whatever key the map happened to yield next, in an order that Rust
    // randomizes per process.
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let key = &rest[open + 1..open + close];
        match values.get(key) {
            Some(value) => out.push_str(value),
            None => out.push_str(&rest[open..open + close + 1]),
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

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

/// Quotes a value for a DBus match rule. A quote or backslash inside the value
/// has to be escaped, or the rule fails to parse and `AddMatch` takes the whole
/// listener down, which is how one stray apostrophe in an event file used to
/// silence every notification.
fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

impl MatchRule {
    /// Build a DBus match rule string
    pub fn to_match_string(&self) -> String {
        let mut parts = vec!["type='signal'".to_string()];

        if let Some(iface) = &self.interface {
            parts.push(format!("interface={}", quoted(iface)));
        }
        if let Some(member) = &self.member {
            parts.push(format!("member={}", quoted(member)));
        }
        if let Some(path) = &self.path {
            parts.push(format!("path={}", quoted(path)));
        }
        // NOTE: Do NOT include path_prefix as path_namespace here.
        // DBus path_namespace requires '/' separated hierarchy matching,
        // so path_namespace='/devices/battery_BAT' won't match '/devices/battery_BAT0'.
        // We handle prefix filtering ourselves in matches() using starts_with.
        if let Some(arg0) = &self.arg0 {
            parts.push(format!("arg0={}", quoted(arg0)));
        }
        if let Some(sender) = &self.sender {
            parts.push(format!("sender={}", quoted(sender)));
        }

        parts.join(", ")
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
    load_events_inner(false)
}

pub fn load_events_quiet() -> Vec<EventConfig> {
    load_events_inner(true)
}

fn load_events_inner(quiet: bool) -> Vec<EventConfig> {
    let mut events = Vec::new();

    // Search paths for events directory
    let search_paths = [
        std::env::current_dir().ok().map(|p| p.join("events")),
        dirs::config_dir().map(|p| p.join("inno/events")),
        Some(PathBuf::from("/etc/xdg/inno/events")),
    ];

    for events_dir in search_paths.iter().flatten() {
        if events_dir.is_dir() {
            if !quiet {
                eprintln!("Loading events from: {:?}", events_dir);
            }
            if let Ok(entries) = std::fs::read_dir(events_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map(|e| e == "toml").unwrap_or(false) {
                        match load_event_file(&path) {
                            Ok(event) => {
                                if event.enabled {
                                    if !quiet {
                                        eprintln!(
                                            "  Loaded event: {} ({})",
                                            event.name,
                                            path.display()
                                        );
                                    }
                                    events.push(event);
                                } else if !quiet {
                                    eprintln!("  Skipped disabled event: {}", event.name);
                                }
                            }
                            Err(e) => {
                                if !quiet {
                                    eprintln!("  Failed to load {:?}: {}", path, e);
                                }
                            }
                        }
                    }
                }
            }
            break; // Only load from first found directory
        }
    }

    if events.is_empty() {
        if !quiet {
            eprintln!("No event configs found, using built-in battery event");
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

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

        assert_eq!(render("{percentage}% ({state})", &values), "75% (charging)");
        assert_eq!(render("Battery at {percentage}%", &values), "Battery at 75%");
    }

    #[test]
    fn test_format_message_missing_key() {
        assert_eq!(render("{missing}", &HashMap::new()), "{missing}");
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

    #[test]
    fn test_match_rule_path_and_prefix_both_set() {
        // When both path and path_prefix are set, both must match
        let rule = MatchRule {
            interface: None,
            member: None,
            path: Some("/org/freedesktop/UPower/devices/battery_BAT0".into()),
            path_prefix: Some("/org/freedesktop/UPower/devices".into()),
            arg0: None,
            sender: None,
        };
        // Exact path matches both
        assert!(rule.matches("any", "any", "/org/freedesktop/UPower/devices/battery_BAT0"));
        // Wrong exact path fails even though prefix matches
        assert!(!rule.matches("any", "any", "/org/freedesktop/UPower/devices/battery_BAT1"));
    }

    #[test]
    fn test_match_rule_member_mismatch() {
        let rule = MatchRule {
            interface: Some("org.freedesktop.DBus.Properties".into()),
            member: Some("PropertiesChanged".into()),
            path: None,
            path_prefix: None,
            arg0: None,
            sender: None,
        };
        assert!(!rule.matches("org.freedesktop.DBus.Properties", "Get", "/any"));
    }

    #[test]
    fn test_render_multiple_values() {
        let mut values = HashMap::new();
        values.insert("name".into(), "AirPods".into());
        values.insert("state".into(), "connected".into());
        values.insert("percentage".into(), "85".into());

        assert_eq!(render("{name} {state} at {percentage}%", &values), "AirPods connected at 85%");
    }

    #[test]
    fn test_render_empty_values_map() {
        // Missing keys render as {key}, so a typo in an event file is visible.
        assert_eq!(render("{a} and {b}", &HashMap::new()), "{a} and {b}");
    }

    #[test]
    fn test_render_literal_only() {
        assert_eq!(render("no placeholders here", &HashMap::new()), "no placeholders here");
    }

    #[test]
    fn test_render_empty_input() {
        assert_eq!(render("", &HashMap::new()), "");
    }

    #[test]
    fn test_render_does_not_rescan_a_substituted_value() {
        // `name` is a device-supplied BlueZ Alias. An Alias containing a
        // placeholder must render as that text, identically every run, rather
        // than being expanded by whichever key the map yields next.
        let mut values = HashMap::new();
        values.insert("name".into(), "{state}".into());
        values.insert("state".into(), "connected".into());
        assert_eq!(render("{name} {state}", &values), "{state} connected");
    }

    #[test]
    fn test_render_is_stable_across_repeated_calls() {
        let mut values = HashMap::new();
        values.insert("percentage".into(), "80".into());
        values.insert("state".into(), "discharging".into());
        let first = render("{percentage}% ({state})", &values);
        for _ in 0..32 {
            assert_eq!(render("{percentage}% ({state})", &values), first);
        }
    }

    #[test]
    fn test_render_unclosed_brace_is_literal() {
        assert_eq!(render("hello {world", &HashMap::new()), "hello {world");
    }

    #[test]
    fn test_render_missing_key_echoed() {
        let mut values = HashMap::new();
        values.insert("percentage".into(), "80".into());
        assert_eq!(render("{percentage}% ({state})", &values), "80% ({state})");
    }

    #[test]
    fn test_match_string_with_sender() {
        let rule = MatchRule {
            interface: None,
            member: None,
            path: None,
            path_prefix: None,
            arg0: None,
            sender: Some("org.bluez".into()),
        };
        let s = rule.to_match_string();
        assert!(s.contains("sender='org.bluez'"));
        assert!(s.contains("type='signal'"));
    }

    #[test]
    fn test_match_string_no_path_prefix() {
        // path_prefix should NOT appear in the match string (handled in matches() instead)
        let rule = MatchRule {
            interface: None,
            member: None,
            path: None,
            path_prefix: Some("/org/freedesktop/UPower".into()),
            arg0: None,
            sender: None,
        };
        let s = rule.to_match_string();
        assert!(!s.contains("path_namespace"));
        assert!(!s.contains("/org/freedesktop/UPower"));
    }

    #[test]
    fn test_builtin_event_has_extract_fields() {
        let event = builtin_battery_event();
        assert_eq!(event.extract.get("percentage"), Some(&"Percentage".to_string()));
        assert_eq!(event.extract.get("state"), Some(&"State".to_string()));
    }

    #[test]
    fn test_builtin_event_state_map() {
        let event = builtin_battery_event();
        assert_eq!(event.state_map.get("1"), Some(&"charging".to_string()));
        assert_eq!(event.state_map.get("2"), Some(&"discharging".to_string()));
        assert_eq!(event.state_map.get("4"), Some(&"full".to_string()));
        assert_eq!(event.state_map.get("3"), None); // Not mapped
    }

    // --- event file loading ---------------------------------------------
    //
    // A malformed event file is logged and skipped at load time, so a typo in
    // the battery event silently stops every battery notification.

    fn write(body: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new("event");
        let path = dir.join("event.toml");
        std::fs::write(&path, body).unwrap();
        (dir, path)
    }

    const VALID_EVENT: &str = r#"
name = "Laptop Battery"
enabled = true
bus = "system"

[match]
interface = "org.freedesktop.DBus.Properties"
member = "PropertiesChanged"
arg0 = "org.freedesktop.UPower.Device"
path_prefix = "/org/freedesktop/UPower/devices/battery_BAT"

[extract]
percentage = "Percentage"
state = "State"

[state_map]
"1" = "charging"
"2" = "discharging"
"4" = "full"

[format]
message = "{percentage}%"

[conditions]
debounce_ms = 1000
"#;

    #[test]
    fn test_load_event_file_parses_every_section() {
        let (_dir, path) = write(VALID_EVENT);
        let event = load_event_file(&path).expect("valid event should load");

        assert_eq!(event.name, "Laptop Battery");
        assert!(event.enabled);
        assert_eq!(event.bus, "system");
        assert_eq!(
            event.match_rule.interface.as_deref(),
            Some("org.freedesktop.DBus.Properties")
        );
        assert_eq!(
            event.match_rule.arg0.as_deref(),
            Some("org.freedesktop.UPower.Device")
        );
        assert_eq!(event.extract.get("percentage").map(String::as_str), Some("Percentage"));
        assert_eq!(event.state_map.get("1").map(String::as_str), Some("charging"));
        assert_eq!(event.conditions.debounce_ms, 1000);
    }

    #[test]
    fn test_load_event_file_keeps_the_message_intact() {
        let (_dir, path) = write(VALID_EVENT);
        let event = load_event_file(&path).unwrap();

        let mut values = HashMap::new();
        values.insert("percentage".into(), "80".into());
        assert_eq!(render(&event.format.message, &values), "80%");
    }

    #[test]
    fn test_load_event_file_rejects_malformed_toml() {
        let (_dir, path) = write("name = = broken [[[");
        let err = load_event_file(&path).unwrap_err();
        assert!(err.contains("Parse error"), "got: {}", err);
    }

    #[test]
    fn test_load_event_file_reports_unreadable_paths() {
        let dir = TempDir::new("event-missing");
        let err = load_event_file(&dir.join("absent.toml")).unwrap_err();
        assert!(err.contains("Read error"), "got: {}", err);
    }

    #[test]
    fn test_load_event_file_defaults_enabled_and_bus() {
        let (_dir, path) = write(
            r#"
name = "Minimal"

[match]
interface = "org.example.Thing"

[format]
message = "hi"
"#,
        );
        let event = load_event_file(&path).expect("should still load");
        assert!(event.enabled, "enabled should default to true");
        assert_eq!(event.bus, "system", "bus should default to system");
    }

    #[test]
    fn test_load_event_file_requires_a_match_table() {
        // The match table is not #[serde(default)], so omitting it is a parse
        // error rather than a rule that silently matches nothing.
        let (_dir, path) = write("name = \"No Match\"\n");
        let err = load_event_file(&path).unwrap_err();
        assert!(err.contains("Parse error"), "got: {}", err);
    }

    #[test]
    fn test_builtin_battery_event_maps_upower_states() {
        let event = builtin_battery_event();
        assert!(event.enabled);
        assert_eq!(event.state_map.get("1").map(String::as_str), Some("charging"));
        assert_eq!(event.state_map.get("2").map(String::as_str), Some("discharging"));
        assert_eq!(event.state_map.get("4").map(String::as_str), Some("full"));
        assert_eq!(event.extract.get("percentage").map(String::as_str), Some("Percentage"));
    }

    #[test]
    fn test_match_rule_requires_the_configured_fields() {
        let rule = MatchRule {
            interface: Some("org.freedesktop.DBus.Properties".to_string()),
            member: Some("PropertiesChanged".to_string()),
            path: None,
            path_prefix: Some("/org/freedesktop/UPower/devices".to_string()),
            arg0: Some("org.freedesktop.UPower.Device".to_string()),
            sender: None,
        };

        assert!(rule.matches(
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            "/org/freedesktop/UPower/devices/battery_BAT0"
        ));
        // Wrong member, wrong prefix, and a different interface must all fail.
        assert!(!rule.matches(
            "org.freedesktop.DBus.Properties",
            "InterfacesAdded",
            "/org/freedesktop/UPower/devices/battery_BAT0"
        ));
        assert!(!rule.matches(
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            "/org/freedesktop/UPower"
        ));
        assert!(!rule.matches("org.example.Other", "PropertiesChanged", "/org/freedesktop/UPower/devices/battery_BAT0"));
    }

    #[test]
    fn test_match_rule_to_match_string_includes_every_set_field() {
        let rule = MatchRule {
            interface: Some("org.freedesktop.DBus.Properties".to_string()),
            member: Some("PropertiesChanged".to_string()),
            path: None,
            path_prefix: Some("/org/freedesktop/UPower/devices".to_string()),
            arg0: Some("Percentage".to_string()),
            sender: None,
        };
        let s = rule.to_match_string();
        assert!(s.contains("type='signal'"), "got: {}", s);
        assert!(s.contains("interface='org.freedesktop.DBus.Properties'"), "got: {}", s);
        assert!(s.contains("member='PropertiesChanged'"), "got: {}", s);
        assert!(s.contains("arg0='Percentage'"), "got: {}", s);
        // path was not set, so no path constraint should appear.
        assert!(!s.contains("path="), "got: {}", s);
    }
}
