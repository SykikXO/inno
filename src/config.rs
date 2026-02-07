use cairo::{FontSlant, FontWeight};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

// Constants
pub const DEFAULT_MARGIN: i32 = 10;
pub const DEFAULT_FONT_SIZE: f64 = 24.0;
pub const DEFAULT_ICON_SIZE: f64 = 24.0;
pub const ANIMATION_FPS: u64 = 30;
pub const ANIMATION_INTERVAL_MS: u64 = 1000 / ANIMATION_FPS;
pub const HIDE_TIMEOUT_SECS: u64 = 86400;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[allow(dead_code)]
    #[error("Config file not found in any of the search paths")]
    NotFound,
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
    Flicker,
    Pulse,
    Fade,
    Slide,
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
        let margin_h = parts
            .get(2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_MARGIN);
        let margin_v = parts
            .get(3)
            .and_then(|s| s.parse().ok())
            .unwrap_or(margin_h);
        Anchor {
            h,
            v,
            margin_h,
            margin_v,
        }
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
        "flicker" => Animation::Flicker,
        "pulse" => Animation::Pulse,
        "fade" | "fadein" | "fadeout" | "fade-in" | "fade-out" => Animation::Fade,
        "slide" => Animation::Slide,
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

        // Search paths for config files (TOML first, then legacy .conf)
        let search_paths = [
            std::env::current_dir().ok().map(|p| p.join("inno.toml")),
            std::env::current_dir().ok().and_then(|p| p.parent().map(|pp| pp.join("inno.toml"))),
            dirs::config_dir().map(|p| p.join("inno/inno.toml")),
            Some(PathBuf::from("/etc/xdg/inno/inno.toml")),
            // Legacy .conf paths
            std::env::current_dir().ok().map(|p| p.join("inno.conf")),
            std::env::current_dir().ok().and_then(|p| p.parent().map(|pp| pp.join("inno.conf"))),
            dirs::config_dir().map(|p| p.join("inno/inno.conf")),
            Some(PathBuf::from("/etc/xdg/inno/inno.conf")),
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

        // Check if it's a TOML file
        let is_toml = config_path.extension().map(|e| e == "toml").unwrap_or(false);
        
        if is_toml {
            if let Err(e) = config.load_toml(&config_path) {
                eprintln!("Failed to parse TOML config: {}", e);
            }
        } else {
            config.load_legacy(&config_path);
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
            let color = file.colors.get(&sig_cfg.color)
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

    fn load_legacy(&mut self, path: &PathBuf) {
        let Ok(content) = std::fs::read_to_string(path) else {
            return;
        };

        // First pass: collect colors
        let mut colors: HashMap<String, (f64, f64, f64, f64)> = HashMap::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                if value.starts_with('(') && value.ends_with(')') {
                    if let Some(c) = Self::parse_color(value) {
                        colors.insert(key.to_string(), c);
                    }
                }
            }
        }

        self.text_color = colors.get("text_color").copied().unwrap_or(self.text_color);
        self.bg_color = colors.get("bg_color").copied().unwrap_or(self.bg_color);

        // Second pass: parse everything else
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim();

                match key {
                    "font" => self.font = value.to_string(),
                    "font_size" => self.font_size = value.parse().unwrap_or(DEFAULT_FONT_SIZE),
                    "font_slant" => self.font_slant = parse_font_slant(value),
                    "font_weight" => self.font_weight = parse_font_weight(value),
                    "position" => self.anchor = Anchor::parse(value),
                    "format" => self.format = value.to_string(),
                    "border_radius" => self.border_radius = value.parse().unwrap_or(0.0),
                    "gradient" => self.gradient = value.eq_ignore_ascii_case("true") || value == "1",
                    "output" => self.output = parse_output_mode(value),
                    "battery_mode" => self.battery_mode = parse_battery_mode(value),
                    "signal" => {
                        if let Some(s) = self.parse_legacy_signal(value, &colors) {
                            self.signals.push(s);
                        }
                    }
                    _ => {
                        // Color definitions handled in first pass
                    }
                }
            }
        }
    }

    fn parse_color(value: &str) -> Option<(f64, f64, f64, f64)> {
        let inner = value.trim_start_matches('(').trim_end_matches(')');
        let parts: Vec<f64> = inner
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if parts.len() >= 4 {
            Some((parts[0], parts[1], parts[2], parts[3]))
        } else {
            None
        }
    }

    fn parse_legacy_signal(&self, value: &str, colors: &HashMap<String, (f64, f64, f64, f64)>) -> Option<Signal> {
        let parts: Vec<&str> = value.split(',').map(|s| s.trim()).collect();
        if parts.len() < 8 {
            return None;
        }

        let color = colors.get(parts[3]).copied()
            .or_else(|| Self::parse_color(parts[3]))
            .unwrap_or((1.0, 1.0, 1.0, 1.0));

        Some(Signal {
            message: parts[0].to_string(),
            icon: parts[1].to_string(),
            icon_size: parts[2].parse().unwrap_or(DEFAULT_ICON_SIZE),
            color,
            threshold: parts[4].parse().unwrap_or(100.0),
            state_filter: parts[5].to_lowercase(),
            animation: parse_animation(parts[6]),
            duration: parts[7].parse().unwrap_or(5),
            sound: parts.get(8).filter(|s| !s.is_empty()).map(|s| PathBuf::from(*s)),
        })
    }

    pub fn find_signal(&self, pct: f64, state: &str) -> Option<&Signal> {
        let is_charging = state.eq_ignore_ascii_case("charging");

        let mut matches: Vec<&Signal> = self
            .signals
            .iter()
            .filter(|s| {
                let state_match =
                    s.state_filter == "any" || s.state_filter.eq_ignore_ascii_case(state);
                let threshold_match = if is_charging {
                    pct >= s.threshold
                } else {
                    pct <= s.threshold
                };
                state_match && threshold_match
            })
            .collect();

        if is_charging {
            matches.sort_by(|a, b| b.threshold.partial_cmp(&a.threshold).unwrap());
        } else {
            matches.sort_by(|a, b| a.threshold.partial_cmp(&b.threshold).unwrap());
        }

        matches.first().copied()
    }
}
