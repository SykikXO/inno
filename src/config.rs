use cairo::{FontSlant, FontWeight};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

// Constants
pub const DEFAULT_MARGIN: i32 = 10;
pub const DEFAULT_FONT_SIZE: f64 = 24.0;
pub const DEFAULT_ICON_SIZE: f64 = 24.0;
pub const HIDE_TIMEOUT_SECS: u64 = 86400;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    ReadError(#[from] std::io::Error),
    #[error("Parse error in config: {0}")]
    ParseError(#[from] toml::de::Error),
}

// TOML config file structure
#[derive(Debug, Deserialize, Default)]
struct ConfigFile {
    general: Option<GeneralConfig>,
    appearance: Option<AppearanceConfig>,
    #[serde(default)]
    colors: HashMap<String, [f64; 4]>,
    #[serde(default)]
    signal: Vec<SignalConfig>,
}

#[derive(Debug, Deserialize, Default)]
struct GeneralConfig {
    font: Option<String>,
    font_size: Option<f64>,
    font_slant: Option<String>,
    font_weight: Option<String>,
    position: Option<String>,
    format: Option<String>,
    output: Option<String>,
    battery_mode: Option<String>,
    fps: Option<u64>,
    scale: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
struct AppearanceConfig {
    text_color: Option<[f64; 4]>,
    bg_color: Option<[f64; 4]>,
    border_radius: Option<f64>,
    gradient: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SignalConfig {
    message: String,
    #[serde(default)]
    icon: String,
    icon_size: Option<f64>,
    color: String,
    threshold: f64,
    state: String,
    #[serde(default)]
    animation: String,
    duration: Option<u64>,
    sound: Option<String>,
}

// Runtime config structures
#[derive(Debug, Clone, PartialEq)]
pub enum Animation {
    None,
    Blink,
    Pulse,
    Fade,
    SlideLeft,
    SlideRight,
    Bounce,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum OutputMode {
    #[default]
    Primary,
    All,
    Named(String),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum BatteryMode {
    #[default]
    First,
    Combined,
    Highest,
    Lowest,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum HAnchor {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum VAnchor {
    Top,
    Center,
    #[default]
    Bottom,
}

#[derive(Debug, Clone, Default)]
pub struct Anchor {
    pub h: HAnchor,
    pub v: VAnchor,
    pub margin_h: i32,
    pub margin_v: i32,
    pub offset_x: i32,
    pub offset_y: i32,
}

impl Anchor {
    pub fn parse(s: &str) -> Self {
        let parts: Vec<&str> = s.split(',').map(|p| p.trim()).collect();
        let h = match parts.first().map(|s| s.to_lowercase()).as_deref() {
            Some("left") => HAnchor::Left,
            Some("right") => HAnchor::Right,
            _ => HAnchor::Center,
        };
        let v = match parts.get(1).map(|s| s.to_lowercase()).as_deref() {
            Some("top") => VAnchor::Top,
            Some("center") => VAnchor::Center,
            _ => VAnchor::Bottom,
        };
        let margin_h = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_MARGIN);
        let margin_v = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(margin_h);
        let offset_x = parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
        let offset_y = parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);
        Anchor { h, v, margin_h, margin_v, offset_x, offset_y }
    }
}

#[derive(Debug, Clone)]
pub struct Signal {
    pub message: String,
    pub icon: String,
    pub icon_size: f64,
    pub color: (f64, f64, f64, f64),
    pub threshold: f64,
    pub state_filter: String,
    pub animation: Animation,
    pub duration: u64,
    pub sound: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub font: String,
    pub font_size: f64,
    pub font_slant: FontSlant,
    pub font_weight: FontWeight,
    pub anchor: Anchor,
    pub text_color: (f64, f64, f64, f64),
    pub bg_color: (f64, f64, f64, f64),
    pub signals: Vec<Signal>,
    pub border_radius: f64,
    pub gradient: bool,
    pub format: String,
    pub output: OutputMode,
    pub battery_mode: BatteryMode,
    pub fps: u64,
    pub scale: f64,
    pub config_path: Option<PathBuf>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            font: "monospace".to_string(),
            font_size: DEFAULT_FONT_SIZE,
            font_slant: FontSlant::Normal,
            font_weight: FontWeight::Normal,
            anchor: Anchor::default(),
            text_color: (1.0, 1.0, 1.0, 1.0),
            bg_color: (0.0, 0.0, 0.0, 0.6),
            signals: Vec::new(),
            border_radius: 0.0,
            gradient: false,
            format: "{message} {percent}%".to_string(),
            output: OutputMode::Primary,
            battery_mode: BatteryMode::First,
            fps: 30,
            scale: 1.0,
            config_path: None,
        }
    }
}

fn parse_font_slant(s: &str) -> FontSlant {
    match s.to_lowercase().as_str() {
        "italic" => FontSlant::Italic,
        "oblique" => FontSlant::Oblique,
        _ => FontSlant::Normal,
    }
}

fn parse_font_weight(s: &str) -> FontWeight {
    match s.to_lowercase().as_str() {
        "bold" => FontWeight::Bold,
        _ => FontWeight::Normal,
    }
}

fn parse_animation(s: &str) -> Animation {
    match s.to_lowercase().as_str() {
        "blink" | "flicker" => Animation::Blink,
        "pulse" => Animation::Pulse,
        "fade" | "fadein" | "fadeout" | "fade-in" | "fade-out" => Animation::Fade,
        "slide" | "slideright" | "slide-right" => Animation::SlideRight,
        "slideleft" | "slide-left" => Animation::SlideLeft,
        "bounce" => Animation::Bounce,
        _ => Animation::None,
    }
}

fn parse_output_mode(s: &str) -> OutputMode {
    match s.to_lowercase().as_str() {
        "all" => OutputMode::All,
        "primary" => OutputMode::Primary,
        _ => OutputMode::Named(s.to_string()),
    }
}

fn parse_battery_mode(s: &str) -> BatteryMode {
    match s.to_lowercase().as_str() {
        "combined" => BatteryMode::Combined,
        "highest" => BatteryMode::Highest,
        "lowest" => BatteryMode::Lowest,
        _ => BatteryMode::First,
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let mut config = Self::default();

        let search_paths = [
            std::env::current_dir().ok().map(|p| p.join("inno.toml")),
            std::env::current_dir().ok().and_then(|p| p.parent().map(|pp| pp.join("inno.toml"))),
            dirs::config_dir().map(|p| p.join("inno/inno.toml")),
            Some(PathBuf::from("/etc/xdg/inno/inno.toml")),
        ];

        let mut loaded_path = None;
        for path in search_paths.iter().flatten() {
            eprintln!("Checking config: {:?}", path);
            if path.exists() {
                loaded_path = Some(path.clone());
                break;
            }
        }

        let Some(config_path) = loaded_path else {
            eprintln!("No config found!");
            return config;
        };

        config.config_path = Some(config_path.clone());
        eprintln!("Loading config from: {:?}", config_path);

        if let Err(e) = config.load_toml(&config_path) {
            eprintln!("Failed to parse TOML config: {}", e);
        }

        config
    }

    fn load_toml(&mut self, path: &PathBuf) -> Result<(), ConfigError> {
        let content = std::fs::read_to_string(path)?;
        let file: ConfigFile = toml::from_str(&content)?;

        // General settings
        if let Some(general) = file.general {
            if let Some(font) = general.font {
                self.font = font;
            }
            if let Some(size) = general.font_size {
                self.font_size = size;
            }
            if let Some(slant) = general.font_slant {
                self.font_slant = parse_font_slant(&slant);
            }
            if let Some(weight) = general.font_weight {
                self.font_weight = parse_font_weight(&weight);
            }
            if let Some(pos) = general.position {
                self.anchor = Anchor::parse(&pos);
            }
            if let Some(fmt) = general.format {
                self.format = fmt;
            }
            if let Some(out) = general.output {
                self.output = parse_output_mode(&out);
            }
            if let Some(bm) = general.battery_mode {
                self.battery_mode = parse_battery_mode(&bm);
            }
            if let Some(fps) = general.fps {
                self.fps = fps;
            }
            if let Some(s) = general.scale {
                self.scale = s.max(0.1);
            }
        }

        // Appearance settings
        if let Some(appearance) = file.appearance {
            if let Some(c) = appearance.text_color {
                self.text_color = (c[0], c[1], c[2], c[3]);
            }
            if let Some(c) = appearance.bg_color {
                self.bg_color = (c[0], c[1], c[2], c[3]);
            }
            if let Some(r) = appearance.border_radius {
                self.border_radius = r;
            }
            if let Some(g) = appearance.gradient {
                self.gradient = g;
            }
        }

        // Parse signals
        for sig_cfg in file.signal {
            let color = file
                .colors
                .get(&sig_cfg.color)
                .map(|c| (c[0], c[1], c[2], c[3]))
                .unwrap_or((1.0, 1.0, 1.0, 1.0));

            let signal = Signal {
                message: sig_cfg.message,
                icon: sig_cfg.icon,
                icon_size: sig_cfg.icon_size.unwrap_or(DEFAULT_ICON_SIZE),
                color,
                threshold: sig_cfg.threshold,
                state_filter: sig_cfg.state.to_lowercase(),
                animation: parse_animation(&sig_cfg.animation),
                duration: sig_cfg.duration.unwrap_or(5),
                sound: sig_cfg.sound.map(PathBuf::from),
            };
            self.signals.push(signal);
        }

        Ok(())
    }

    /// Returns the index of the best matching signal, avoiding allocation
    /// when only the index is needed (e.g. for caching during animation).
    pub fn find_signal_idx(&self, pct: f64, state: &str) -> Option<usize> {
        let is_charging = state.eq_ignore_ascii_case("charging");

        let mut best_idx: Option<usize> = None;
        let mut best_threshold: f64 = if is_charging { f64::MIN } else { f64::MAX };

        for (i, s) in self.signals.iter().enumerate() {
            let state_match = s.state_filter == "any" || s.state_filter.eq_ignore_ascii_case(state);
            let threshold_match = if is_charging { pct >= s.threshold } else { pct <= s.threshold };

            if state_match && threshold_match {
                let is_better = if is_charging {
                    s.threshold > best_threshold
                } else {
                    s.threshold < best_threshold
                };
                if best_idx.is_none() || is_better {
                    best_idx = Some(i);
                    best_threshold = s.threshold;
                }
            }
        }

        best_idx
    }

    /// Validate config and return list of warnings/errors
    pub fn validate(&self) -> (Vec<String>, Vec<String>) {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        if self.signals.is_empty() {
            errors.push("No signals defined in config".to_string());
        }

        for (i, sig) in self.signals.iter().enumerate() {
            if sig.message.is_empty() {
                errors.push(format!("signal[{}]: message is empty", i));
            }
            if sig.duration == 0 {
                errors.push(format!("signal[{}]: duration must be > 0", i));
            }
            if sig.threshold < 0.0 || sig.threshold > 100.0 {
                errors.push(format!("signal[{}]: threshold {} out of range 0-100", i, sig.threshold));
            }
            if sig.icon_size < 1.0 {
                warnings.push(format!("signal[{}]: icon_size {} is very small", i, sig.icon_size));
            }
            let (r, g, b, a) = sig.color;
            if !(0.0..=1.0).contains(&r) || !(0.0..=1.0).contains(&g) || !(0.0..=1.0).contains(&b) || !(0.0..=1.0).contains(&a) {
                errors.push(format!("signal[{}]: color values must be 0.0-1.0", i));
            }
            if let Some(ref sound_path) = sig.sound
                && !sound_path.exists() {
                    warnings.push(format!("signal[{}]: sound file not found: {:?}", i, sound_path));
                }
        }

        if self.fps == 0 {
            errors.push("fps must be > 0".to_string());
        } else if self.fps > 120 {
            warnings.push(format!("fps={} is unusually high", self.fps));
        }

        if self.scale < 0.1 {
            errors.push("scale must be >= 0.1".to_string());
        } else if self.scale > 5.0 {
            warnings.push(format!("scale={} is very large", self.scale));
        }

        if self.font_size < 1.0 {
            errors.push("font_size must be >= 1.0".to_string());
        }

        let (r, g, b, a) = self.bg_color;
        if !(0.0..=1.0).contains(&r) || !(0.0..=1.0).contains(&g) || !(0.0..=1.0).contains(&b) || !(0.0..=1.0).contains(&a) {
            errors.push("bg_color values must be 0.0-1.0".to_string());
        }

        let (r, g, b, a) = self.text_color;
        if !(0.0..=1.0).contains(&r) || !(0.0..=1.0).contains(&g) || !(0.0..=1.0).contains(&b) || !(0.0..=1.0).contains(&a) {
            errors.push("text_color values must be 0.0-1.0".to_string());
        }

        if self.format.is_empty() {
            warnings.push("format string is empty".to_string());
        }

        if self.config_path.is_none() {
            warnings.push("No config file found, using defaults".to_string());
        }

        (errors, warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anchor_parse_full() {
        let a = Anchor::parse("center,bottom,10,20,5,-15");
        assert!(matches!(a.h, HAnchor::Center));
        assert!(matches!(a.v, VAnchor::Bottom));
        assert_eq!(a.margin_h, 10);
        assert_eq!(a.margin_v, 20);
        assert_eq!(a.offset_x, 5);
        assert_eq!(a.offset_y, -15);
    }

    #[test]
    fn test_anchor_parse_minimal() {
        let a = Anchor::parse("left,top");
        assert!(matches!(a.h, HAnchor::Left));
        assert!(matches!(a.v, VAnchor::Top));
        assert_eq!(a.margin_h, DEFAULT_MARGIN);
        assert_eq!(a.margin_v, DEFAULT_MARGIN);
        assert_eq!(a.offset_x, 0);
        assert_eq!(a.offset_y, 0);
    }

    #[test]
    fn test_anchor_parse_right_bottom() {
        let a = Anchor::parse("right,bottom,50");
        assert!(matches!(a.h, HAnchor::Right));
        assert!(matches!(a.v, VAnchor::Bottom));
        assert_eq!(a.margin_h, 50);
        assert_eq!(a.margin_v, 50);
    }

    #[test]
    fn test_anchor_parse_defaults() {
        let a = Anchor::parse("");
        assert!(matches!(a.h, HAnchor::Center));
        assert!(matches!(a.v, VAnchor::Bottom));
    }

    #[test]
    fn test_find_signal_idx_charging() {
        let cfg = AppConfig {
            signals: vec![
                Signal {
                    message: "low".into(),
                    icon: "".into(),
                    icon_size: 24.0,
                    color: (1.0, 0.0, 0.0, 1.0),
                    threshold: 20.0,
                    state_filter: "charging".into(),
                    animation: Animation::None,
                    duration: 5,
                    sound: None,
                },
                Signal {
                    message: "mid".into(),
                    icon: "".into(),
                    icon_size: 24.0,
                    color: (1.0, 1.0, 0.0, 1.0),
                    threshold: 50.0,
                    state_filter: "charging".into(),
                    animation: Animation::None,
                    duration: 5,
                    sound: None,
                },
                Signal {
                    message: "high".into(),
                    icon: "".into(),
                    icon_size: 24.0,
                    color: (0.0, 1.0, 0.0, 1.0),
                    threshold: 80.0,
                    state_filter: "charging".into(),
                    animation: Animation::None,
                    duration: 5,
                    sound: None,
                },
            ],
            ..Default::default()
        };

        assert_eq!(cfg.find_signal_idx(15.0, "charging"), None);
        assert_eq!(cfg.find_signal_idx(25.0, "charging"), Some(0));
        assert_eq!(cfg.find_signal_idx(60.0, "charging"), Some(1));
        assert_eq!(cfg.find_signal_idx(90.0, "charging"), Some(2));
    }

    #[test]
    fn test_find_signal_idx_discharging() {
        let cfg = AppConfig {
            signals: vec![
                Signal {
                    message: "critical".into(),
                    icon: "".into(),
                    icon_size: 24.0,
                    color: (1.0, 0.0, 0.0, 1.0),
                    threshold: 10.0,
                    state_filter: "discharging".into(),
                    animation: Animation::None,
                    duration: 5,
                    sound: None,
                },
                Signal {
                    message: "low".into(),
                    icon: "".into(),
                    icon_size: 24.0,
                    color: (1.0, 1.0, 0.0, 1.0),
                    threshold: 30.0,
                    state_filter: "discharging".into(),
                    animation: Animation::None,
                    duration: 5,
                    sound: None,
                },
            ],
            ..Default::default()
        };

        assert_eq!(cfg.find_signal_idx(5.0, "discharging"), Some(0));
        assert_eq!(cfg.find_signal_idx(20.0, "discharging"), Some(1));
        assert_eq!(cfg.find_signal_idx(50.0, "discharging"), None);
    }

    #[test]
    fn test_find_signal_idx_any_state() {
        let cfg = AppConfig {
            signals: vec![Signal {
                message: "any".into(),
                icon: "".into(),
                icon_size: 24.0,
                color: (1.0, 1.0, 1.0, 1.0),
                threshold: 50.0,
                state_filter: "any".into(),
                animation: Animation::None,
                duration: 5,
                sound: None,
            }],
            ..Default::default()
        };

        assert!(cfg.find_signal_idx(60.0, "charging").is_some());
        assert!(cfg.find_signal_idx(40.0, "discharging").is_some());
        assert!(cfg.find_signal_idx(50.0, "full").is_some());
    }

    #[test]
    fn test_parse_animation() {
        assert!(matches!(parse_animation("blink"), Animation::Blink));
        assert!(matches!(parse_animation("flicker"), Animation::Blink));
        assert!(matches!(parse_animation("pulse"), Animation::Pulse));
        assert!(matches!(parse_animation("fade"), Animation::Fade));
        assert!(matches!(parse_animation("fade-in"), Animation::Fade));
        assert!(matches!(parse_animation("slide-right"), Animation::SlideRight));
        assert!(matches!(parse_animation("slide-left"), Animation::SlideLeft));
        assert!(matches!(parse_animation("bounce"), Animation::Bounce));
        assert!(matches!(parse_animation("none"), Animation::None));
        assert!(matches!(parse_animation("invalid"), Animation::None));
    }

    #[test]
    fn test_parse_output_mode() {
        assert!(matches!(parse_output_mode("all"), OutputMode::All));
        assert!(matches!(parse_output_mode("primary"), OutputMode::Primary));
        assert!(matches!(parse_output_mode("HDMI-A-1"), OutputMode::Named(s) if s == "HDMI-A-1"));
    }

    #[test]
    fn test_parse_battery_mode() {
        assert!(matches!(parse_battery_mode("combined"), BatteryMode::Combined));
        assert!(matches!(parse_battery_mode("highest"), BatteryMode::Highest));
        assert!(matches!(parse_battery_mode("lowest"), BatteryMode::Lowest));
        assert!(matches!(parse_battery_mode("first"), BatteryMode::First));
        assert!(matches!(parse_battery_mode("invalid"), BatteryMode::First));
    }

    #[test]
    fn test_validate_empty_signals() {
        let cfg = AppConfig::default();
        let (errors, _warnings) = cfg.validate();
        assert!(errors.iter().any(|e| e.contains("No signals")));
    }

    #[test]
    fn test_validate_invalid_color() {
        let cfg = AppConfig {
            signals: vec![Signal {
                message: "test".into(),
                icon: "".into(),
                icon_size: 24.0,
                color: (1.5, 0.0, 0.0, 1.0),
                threshold: 50.0,
                state_filter: "any".into(),
                animation: Animation::None,
                duration: 5,
                sound: None,
            }],
            ..Default::default()
        };
        let (errors, _) = cfg.validate();
        assert!(errors.iter().any(|e| e.contains("color")));
    }

    #[test]
    fn test_validate_zero_duration() {
        let cfg = AppConfig {
            signals: vec![Signal {
                message: "test".into(),
                icon: "".into(),
                icon_size: 24.0,
                color: (1.0, 1.0, 1.0, 1.0),
                threshold: 50.0,
                state_filter: "any".into(),
                animation: Animation::None,
                duration: 0,
                sound: None,
            }],
            ..Default::default()
        };
        let (errors, _) = cfg.validate();
        assert!(errors.iter().any(|e| e.contains("duration")));
    }

    #[test]
    fn test_validate_valid_config() {
        let cfg = AppConfig {
            signals: vec![Signal {
                message: "test".into(),
                icon: "".into(),
                icon_size: 24.0,
                color: (1.0, 1.0, 1.0, 1.0),
                threshold: 50.0,
                state_filter: "any".into(),
                animation: Animation::None,
                duration: 5,
                sound: None,
            }],
            fps: 30,
            font_size: 24.0,
            ..Default::default()
        };
        let (errors, _) = cfg.validate();
        assert!(errors.is_empty());
    }

    #[test]
    fn test_validate_scale_too_small() {
        let cfg = AppConfig {
            scale: 0.05,
            signals: vec![Signal {
                message: "test".into(),
                icon: "".into(),
                icon_size: 24.0,
                color: (1.0, 1.0, 1.0, 1.0),
                threshold: 50.0,
                state_filter: "any".into(),
                animation: Animation::None,
                duration: 5,
                sound: None,
            }],
            ..Default::default()
        };
        let (errors, _) = cfg.validate();
        assert!(errors.iter().any(|e| e.contains("scale")));
    }

    #[test]
    fn test_validate_scale_too_large() {
        let cfg = AppConfig {
            scale: 6.0,
            signals: vec![Signal {
                message: "test".into(),
                icon: "".into(),
                icon_size: 24.0,
                color: (1.0, 1.0, 1.0, 1.0),
                threshold: 50.0,
                state_filter: "any".into(),
                animation: Animation::None,
                duration: 5,
                sound: None,
            }],
            ..Default::default()
        };
        let (_, warnings) = cfg.validate();
        assert!(warnings.iter().any(|w| w.contains("scale")));
    }

    #[test]
    fn test_default_scale_is_one() {
        let cfg = AppConfig::default();
        assert!((cfg.scale - 1.0).abs() < f64::EPSILON);
    }
}
