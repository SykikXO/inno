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
    #[serde(default)]
    animations: HashMap<String, AnimAssetConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AnimAssetConfig {
    pub(crate) source: PathBuf,
    pub(crate) fps: Option<u64>,
    #[serde(rename = "loop")]
    pub(crate) loop_: Option<bool>,
    pub(crate) display: Option<String>,
    pub(crate) on_complete: Option<String>,
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
    sound: Option<bool>,
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
    #[serde(default)]
    animation_ref: Option<String>,
}

// Runtime config structures
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Animation {
    None,
    Blink,
    Pulse,
    Fade,
    SlideLeft,
    SlideRight,
    Bounce,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DisplayMode {
    Anim,
    Text,
}

impl DisplayMode {
    /// `None` for an unrecognised value, so the caller can warn rather than
    /// silently rendering an animation-only notification where the user
    /// expected text.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "text" => Some(DisplayMode::Text),
            "anim" | "animation" => Some(DisplayMode::Anim),
            _ => None,
        }
    }
}

/// What happens when a non-looping animation reaches its last frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OnComplete {
    /// Leave the last frame on screen until the notification is dismissed.
    Hold,
    /// Take the notification down as soon as the animation ends.
    Hide,
    /// Restart the animation instead of ending, whatever `loop` says.
    Loop,
}

impl OnComplete {
    /// `None` for an unrecognised value so the caller can warn.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "hold" => Some(OnComplete::Hold),
            "hide" => Some(OnComplete::Hide),
            "loop" => Some(OnComplete::Loop),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AnimAsset {
    pub source: PathBuf,
    pub fps: u64,
    pub loop_: bool,
    pub display: DisplayMode,
    pub on_complete: OnComplete,
    /// Playback length implied by the frame count and `fps`, measured at config
    /// load by counting the directory. Counting is a scan, not a decode, so
    /// this costs nothing.
    pub natural_duration: Option<std::time::Duration>,
}

impl Animation {
    /// All animatable variants (excludes None, used for testing)
    pub const TEST_VARIANTS: &'static [Self] = &[
        Self::Blink,
        Self::Pulse,
        Self::Fade,
        Self::SlideLeft,
        Self::SlideRight,
        Self::Bounce,
    ];
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

/// Renders a notification's `format` string.
///
/// `{percent}%` is substituted before `{percent}` so the suffixed form wins.
/// A missing percent renders as nothing, not as a bare `%`, and the leftover
/// space is trimmed, so `{message} {percent}%` degrades to `{message}` rather
/// than `{message} %`.
pub fn format_text(fmt: &str, icon: &str, message: &str, percent: Option<f64>) -> String {
    let (pct, pct_suffixed) = match percent {
        Some(p) => (format!("{p:.0}"), format!("{p:.0}%")),
        None => (String::new(), String::new()),
    };
    let mut out = fmt
        .replace("{percent}%", &pct_suffixed)
        .replace("{percent}", &pct)
        .replace("{icon}", icon)
        .replace("{message}", message);
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

#[derive(Debug, Clone)]
pub struct Signal {
    pub message: String,
    pub icon: String,
    pub icon_size: f64,
    pub color: (f64, f64, f64, f64),
    pub color_name: String,
    pub threshold: f64,
    pub state_filter: String,
    pub animation: Animation,
    pub animation_ref: Option<String>,
    pub duration: Option<u64>,
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
    pub animations: HashMap<String, AnimAsset>,
    pub border_radius: f64,
    pub gradient: bool,
    pub format: String,
    pub output: OutputMode,
    pub battery_mode: BatteryMode,
    pub fps: u64,
    pub scale: f64,
    pub config_path: Option<PathBuf>,
    pub sound: bool,
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
            animations: HashMap::new(),
            border_radius: 0.0,
            gradient: false,
            format: "{message} {percent}%".to_string(),
            sound: true,
            output: OutputMode::Primary,
            battery_mode: BatteryMode::First,
            fps: 30,
            scale: 1.0,
            config_path: None,
        }
    }
}

/// Playback length of a frame directory, from a file count and a frame rate.
/// Scanning the directory is cheap, unlike decoding it.
fn natural_duration(dir: &std::path::Path, fps: u64) -> Option<std::time::Duration> {
    let frames = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .and_then(|x| x.to_str())
                        .is_some_and(|x| x.eq_ignore_ascii_case("png"))
                })
                .count()
        })
        .unwrap_or(0);

    (fps > 0 && frames > 0)
        .then(|| std::time::Duration::from_secs_f64(frames as f64 / fps as f64))
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
        Self::load_inner(false)
    }

    pub fn load_quiet() -> Self {
        Self::load_inner(true)
    }

    fn load_inner(quiet: bool) -> Self {
        let mut config = Self::default();

        let search_paths = [
            std::env::current_dir().ok().map(|p| p.join("inno.toml")),
            dirs::config_dir().map(|p| p.join("inno/inno.toml")),
            Some(PathBuf::from("/etc/xdg/inno/inno.toml")),
        ];

        let mut loaded_path = None;
        for path in search_paths.iter().flatten() {
            if !quiet {
                eprintln!("Checking config: {:?}", path);
            }
            if path.exists() {
                loaded_path = Some(path.clone());
                break;
            }
        }

        let Some(config_path) = loaded_path else {
            if !quiet {
                eprintln!("No config found!");
            }
            return config;
        };

        config.config_path = Some(config_path.clone());
        if !quiet {
            eprintln!("Loading config from: {:?}", config_path);
        }

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
            if let Some(enabled) = general.sound {
                self.sound = enabled;
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

        // Prepare config dir for relative path resolution
        let config_dir = path.parent().map(PathBuf::from);

        // Parse animations
        for (name, anim_cfg) in file.animations {
            let display = match anim_cfg.display.as_deref() {
                Some(raw) => match DisplayMode::parse(raw) {
                    Some(mode) => mode,
                    None => {
                        eprintln!(
                            "animations.{}: unknown display '{}', defaulting to 'anim'",
                            name, raw
                        );
                        DisplayMode::Anim
                    }
                },
                None => DisplayMode::Anim,
            };

            let source = if anim_cfg.source.is_absolute() {
                anim_cfg.source
            } else if let Some(ref dir) = config_dir {
                dir.join(&anim_cfg.source)
            } else {
                anim_cfg.source
            };

            let on_complete = match anim_cfg.on_complete.as_deref() {
                Some(raw) => match OnComplete::parse(raw) {
                    Some(mode) => mode,
                    None => {
                        eprintln!(
                            "animations.{}: unknown on_complete '{}', defaulting to 'hold'",
                            name, raw
                        );
                        OnComplete::Hold
                    }
                },
                None => OnComplete::Hold,
            };

            // Not clamped here: a zero fps is reported by validate()
            // rather than silently becoming a 1 fps animation.
            let fps = anim_cfg.fps.unwrap_or(self.fps);
            // Counting is a directory scan, not a decode, so the playback
            // length is known without touching pixel data.
            let natural_duration = natural_duration(&source, fps);

            self.animations.insert(
                name,
                AnimAsset {
                    source,
                    fps,
                    loop_: anim_cfg.loop_.unwrap_or(true),
                    display,
                    on_complete,
                    natural_duration,
                },
            );
        }

        // Parse signals
        for sig_cfg in file.signal {
            let color = file
                .colors
                .get(&sig_cfg.color)
                .map(|c| (c[0], c[1], c[2], c[3]))
                .unwrap_or_else(|| {
                    eprintln!("Warning: color '{}' not found in [colors], defaulting to white", sig_cfg.color);
                    (1.0, 1.0, 1.0, 1.0)
                });

            let sound_path = sig_cfg.sound.map(|s| {
                let p = PathBuf::from(&s);
                if p.is_absolute() {
                    p
                } else if let Some(ref dir) = config_dir {
                    dir.join(&p)
                } else {
                    p
                }
            });

            let (animation, animation_ref) =
                self.resolve_animation(sig_cfg.animation.trim(), sig_cfg.animation_ref.as_deref());

            let signal = Signal {
                message: sig_cfg.message,
                icon: sig_cfg.icon,
                icon_size: sig_cfg.icon_size.unwrap_or(DEFAULT_ICON_SIZE),
                color,
                color_name: sig_cfg.color,
                threshold: sig_cfg.threshold,
                state_filter: sig_cfg.state.to_lowercase(),
                animation,
                animation_ref,
                duration: sig_cfg.duration,
                sound: sound_path,
            };
            self.signals.push(signal);
        }

        Ok(())
    }

    /// Returns the index of the best matching signal, avoiding allocation
    /// when only the index is needed (e.g. for caching during animation).
    pub fn find_signal_idx(&self, pct: f64, state: &str) -> Option<usize> {
        // Charging and full use ascending logic (pct >= threshold, best = highest).
        // Everything else (discharging, connected, etc.) uses descending (pct <= threshold, best = lowest).
        let is_ascending = state.eq_ignore_ascii_case("charging")
            || state.eq_ignore_ascii_case("full");

        let mut best_idx: Option<usize> = None;
        let mut best_threshold: f64 = if is_ascending { f64::MIN } else { f64::MAX };

        for (i, s) in self.signals.iter().enumerate() {
            let state_match = s.state_filter == "any" || s.state_filter.eq_ignore_ascii_case(state);
            let threshold_match = if is_ascending { pct >= s.threshold } else { pct <= s.threshold };

            if state_match && threshold_match {
                let is_better = if is_ascending {
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

    /// Splits a signal's `animation` and `animation_ref` into the procedural
    /// transition and the frame-animation content.
    ///
    /// They are independent and compose: `animation = "fade"` with
    /// `animation_ref = "cube"` fades a cube in. Naming an `[animations]` key
    /// in `animation` is shorthand for "the content is that asset, and there
    /// is no separate transition".
    ///
    /// An `animation_ref` is kept even when it does not resolve, so validate()
    /// can report the typo instead of the config quietly rendering a plain text
    /// card.
    fn resolve_animation(
        &self,
        anim: &str,
        explicit_ref: Option<&str>,
    ) -> (Animation, Option<String>) {
        if self.animations.contains_key(anim) {
            return (Animation::None, Some(anim.to_string()));
        }
        (parse_animation(anim), explicit_ref.map(str::to_string))
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
            // duration = 0 means infinite; omitted means derive it from the
            // animation, so neither is an error.
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
            if !sig.color_name.is_empty()
                && sig.color == (1.0, 1.0, 1.0, 1.0)
                && sig.color_name != "white"
            {
                warnings.push(format!("signal[{}]: color '{}' not found in [colors], resolved to white", i, sig.color_name));
            }
            if let Some(ref sound_path) = sig.sound
                && !sound_path.exists() {
                    warnings.push(format!("signal[{}]: sound file not found: {:?}", i, sound_path));
                }
            // An unresolvable animation_ref silently degrades to a plain text
            // card, so the config would report valid while rendering nothing
            // the user asked for.
            if let Some(ref anim) = sig.animation_ref
                && !self.animations.contains_key(anim)
            {
                errors.push(format!(
                    "signal[{}]: animation_ref '{}' is not defined in [animations]",
                    i, anim
                ));
            }
        }

        for (name, anim) in &self.animations {
            if !anim.source.is_dir() {
                errors.push(format!(
                    "animations.{}: source is not a directory: {}",
                    name,
                    anim.source.display()
                ));
            } else {
                match std::fs::read_dir(&anim.source) {
                    Ok(entries) => {
                        let pngs = entries
                            .filter_map(|e| e.ok())
                            .filter(|e| {
                                e.path()
                                    .extension()
                                    .and_then(|x| x.to_str())
                                    .is_some_and(|x| x.eq_ignore_ascii_case("png"))
                            })
                            .count();
                        if pngs == 0 {
                            errors.push(format!(
                                "animations.{}: no PNG frames in {}",
                                name,
                                anim.source.display()
                            ));
                        }
                    }
                    Err(e) => errors.push(format!(
                        "animations.{}: cannot read {}: {}",
                        name,
                        anim.source.display(),
                        e
                    )),
                }
            }

            if anim.on_complete == OnComplete::Loop && !anim.loop_ {
                warnings.push(format!(
                    "animations.{}: on_complete = \"loop\" overrides loop = false",
                    name
                ));
            }

            // Checked even when the source is unusable, so fixing one problem
            // does not just reveal the next one on the next run.
            if anim.fps == 0 {
                errors.push(format!("animations.{}: fps must be > 0", name));
            } else if anim.fps > 120 {
                warnings.push(format!("animations.{}: fps={} is unusually high", name, anim.fps));
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
    use crate::state::{DEFAULT_DURATION_SECS, display_seconds, notification_duration, transition_frames};
    use crate::testutil::TempDir;

    /// Writes a config file into a temp dir and loads it. `load_toml` takes an
    /// explicit path, so this needs no environment.
    fn load_fixture(body: &str) -> (AppConfig, TempDir) {
        let dir = TempDir::new("cfg");
        let path = dir.join("inno.toml");
        std::fs::write(&path, body).unwrap();
        let mut cfg = AppConfig::default();
        cfg.load_toml(&path).expect("fixture config should parse");
        (cfg, dir)
    }

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
                    color_name: "red".into(),
                    threshold: 20.0,
                    state_filter: "charging".into(),
                    animation: Animation::None,
                    duration: Some(5),
                    sound: None,
                    animation_ref: None,
                },
                Signal {
                    message: "mid".into(),
                    icon: "".into(),
                    icon_size: 24.0,
                    color: (1.0, 1.0, 0.0, 1.0),
                    color_name: "yellow".into(),
                    threshold: 50.0,
                    state_filter: "charging".into(),
                    animation: Animation::None,
                    duration: Some(5),
                    sound: None,
                    animation_ref: None,
                },
                Signal {
                    message: "high".into(),
                    icon: "".into(),
                    icon_size: 24.0,
                    color: (0.0, 1.0, 0.0, 1.0),
                    color_name: "green".into(),
                    threshold: 80.0,
                    state_filter: "charging".into(),
                    animation: Animation::None,
                    duration: Some(5),
                    sound: None,
                    animation_ref: None,
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
                    color_name: "red".into(),
                    threshold: 10.0,
                    state_filter: "discharging".into(),
                    animation: Animation::None,
                    duration: Some(5),
                    sound: None,
                    animation_ref: None,
                },
                Signal {
                    message: "low".into(),
                    icon: "".into(),
                    icon_size: 24.0,
                    color: (1.0, 1.0, 0.0, 1.0),
                    color_name: "yellow".into(),
                    threshold: 30.0,
                    state_filter: "discharging".into(),
                    animation: Animation::None,
                    duration: Some(5),
                    sound: None,
                    animation_ref: None,
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
                color_name: "white".into(),
                threshold: 50.0,
                state_filter: "any".into(),
                animation: Animation::None,
                duration: Some(5),
                sound: None,
                animation_ref: None,
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
                color_name: "red".into(),
                threshold: 50.0,
                state_filter: "any".into(),
                animation: Animation::None,
                duration: Some(5),
                sound: None,
                animation_ref: None,
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
                color_name: "white".into(),
                threshold: 50.0,
                state_filter: "any".into(),
                animation: Animation::None,
                duration: Some(0),
                sound: None,
                animation_ref: None,
            }],
            ..Default::default()
        };
        let (errors, _) = cfg.validate();
        assert!(errors.is_empty());
    }

    #[test]
    fn test_validate_valid_config() {
        let cfg = AppConfig {
            signals: vec![Signal {
                message: "test".into(),
                icon: "".into(),
                icon_size: 24.0,
                color: (1.0, 1.0, 1.0, 1.0),
                color_name: "white".into(),
                threshold: 50.0,
                state_filter: "any".into(),
                animation: Animation::None,
                duration: Some(5),
                sound: None,
                animation_ref: None,
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
                color_name: "white".into(),
                threshold: 50.0,
                state_filter: "any".into(),
                animation: Animation::None,
                duration: Some(5),
                sound: None,
                animation_ref: None,
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
                color_name: "white".into(),
                threshold: 50.0,
                state_filter: "any".into(),
                animation: Animation::None,
                duration: Some(5),
                sound: None,
                animation_ref: None,
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

    #[test]
    fn test_validate_unknown_color_name() {
        let cfg = AppConfig {
            signals: vec![Signal {
                message: "test".into(),
                icon: "".into(),
                icon_size: 24.0,
                color: (1.0, 1.0, 1.0, 1.0),
                color_name: "gren".into(),
                threshold: 50.0,
                state_filter: "any".into(),
                animation: Animation::None,
                duration: Some(5),
                sound: None,
                animation_ref: None,
            }],
            ..Default::default()
        };
        let (_, warnings) = cfg.validate();
        assert!(warnings.iter().any(|w| w.contains("gren") && w.contains("white")));
    }

    #[test]
    fn test_format_text_parse_and_render() {
        assert_eq!(
            format_text("{icon} {message} {percent}%", "BAT", "Battery", Some(75.0)),
            "BAT Battery 75%"
        );
    }

    #[test]
    fn test_format_text_no_percent() {
        assert_eq!(format_text("{icon} {message} {percent}%", "NET", "Connected", None), "NET Connected");
    }

    #[test]
    fn test_format_text_percent_only() {
        assert_eq!(format_text("{message} {percent}%", "", "Hello", None), "Hello");
    }

    #[test]
    fn test_format_text_no_placeholders() {
        assert_eq!(format_text("static text", "X", "Y", Some(1.0)), "static text");
    }

    #[test]
    fn test_format_text_unknown_placeholder() {
        assert_eq!(format_text("{foo} bar", "", "", None), "{foo} bar");
    }

    #[test]
    fn test_format_text_default() {
        let cfg = AppConfig::default();
        assert_eq!(format_text(&cfg.format, "", "Test", Some(42.0)), "Test 42%");
    }

    #[test]
    fn test_find_signal_idx_exact_threshold_charging() {
        let cfg = AppConfig {
            signals: vec![Signal {
                message: "at80".into(),
                icon: "".into(),
                icon_size: 24.0,
                color: (0.0, 1.0, 0.0, 1.0),
                color_name: "green".into(),
                threshold: 80.0,
                state_filter: "charging".into(),
                animation: Animation::None,
                duration: Some(5),
                sound: None,
                animation_ref: None,
            }],
            ..Default::default()
        };
        // pct == threshold should match (pct >= threshold for charging)
        assert_eq!(cfg.find_signal_idx(80.0, "charging"), Some(0));
        // Just below should NOT match
        assert_eq!(cfg.find_signal_idx(79.9, "charging"), None);
    }

    #[test]
    fn test_find_signal_idx_exact_threshold_discharging() {
        let cfg = AppConfig {
            signals: vec![Signal {
                message: "at15".into(),
                icon: "".into(),
                icon_size: 24.0,
                color: (1.0, 0.0, 0.0, 1.0),
                color_name: "red".into(),
                threshold: 15.0,
                state_filter: "discharging".into(),
                animation: Animation::None,
                duration: Some(5),
                sound: None,
                animation_ref: None,
            }],
            ..Default::default()
        };
        // pct == threshold should match (pct <= threshold for discharging)
        assert_eq!(cfg.find_signal_idx(15.0, "discharging"), Some(0));
        // Just above should NOT match
        assert_eq!(cfg.find_signal_idx(15.1, "discharging"), None);
    }

    #[test]
    fn test_find_signal_idx_no_signals() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.find_signal_idx(50.0, "charging"), None);
        assert_eq!(cfg.find_signal_idx(50.0, "discharging"), None);
    }

    #[test]
    fn test_find_signal_idx_wrong_state() {
        let cfg = AppConfig {
            signals: vec![Signal {
                message: "charging only".into(),
                icon: "".into(),
                icon_size: 24.0,
                color: (0.0, 1.0, 0.0, 1.0),
                color_name: "green".into(),
                threshold: 0.0,
                state_filter: "charging".into(),
                animation: Animation::None,
                duration: Some(5),
                sound: None,
                animation_ref: None,
            }],
            ..Default::default()
        };
        // Should not match discharging state
        assert_eq!(cfg.find_signal_idx(50.0, "discharging"), None);
        // Should match charging
        assert_eq!(cfg.find_signal_idx(50.0, "charging"), Some(0));
    }

    #[test]
    fn test_find_signal_idx_case_insensitive_state() {
        let cfg = AppConfig {
            signals: vec![Signal {
                message: "test".into(),
                icon: "".into(),
                icon_size: 24.0,
                color: (1.0, 1.0, 1.0, 1.0),
                color_name: "white".into(),
                threshold: 50.0,
                state_filter: "charging".into(),
                animation: Animation::None,
                duration: Some(5),
                sound: None,
                animation_ref: None,
            }],
            ..Default::default()
        };
        // State matching should be case-insensitive
        assert_eq!(cfg.find_signal_idx(60.0, "Charging"), Some(0));
        assert_eq!(cfg.find_signal_idx(60.0, "CHARGING"), Some(0));
    }

    #[test]
    fn test_find_signal_idx_full_uses_ascending() {
        // "full" state should use ascending semantics like "charging" —
        // a full battery at 100% should match signals with threshold <= 100
        let cfg = AppConfig {
            signals: vec![Signal {
                message: "optimal".into(),
                icon: "".into(),
                icon_size: 24.0,
                color: (0.0, 1.0, 0.0, 1.0),
                color_name: "green".into(),
                threshold: 80.0,
                state_filter: "any".into(),
                animation: Animation::None,
                duration: Some(5),
                sound: None,
                animation_ref: None,
            }],
            ..Default::default()
        };
        // full at 100% with threshold=80: ascending → 100 >= 80 → matches
        assert_eq!(cfg.find_signal_idx(100.0, "full"), Some(0));
        // full at 70% with threshold=80: ascending → 70 >= 80 → no match
        assert_eq!(cfg.find_signal_idx(70.0, "full"), None);
    }

    #[test]
    fn test_anchor_parse_with_whitespace() {
        let a = Anchor::parse("  center , top , 15 ");
        assert!(matches!(a.h, HAnchor::Center));
        assert!(matches!(a.v, VAnchor::Top));
        assert_eq!(a.margin_h, 15);
    }

    #[test]
    fn test_format_text_unclosed_brace() {
        // Unclosed brace is literal text, not a broken placeholder.
        assert_eq!(format_text("hello {world", "", "", None), "hello {world");
    }

    #[test]
    fn test_format_text_empty_string() {
        assert_eq!(format_text("", "icon", "msg", Some(50.0)), "");
    }

    #[test]
    fn test_format_text_consecutive_placeholders() {
        assert_eq!(format_text("{icon}{message}", "A", "B", None), "AB");
    }

    #[test]
    fn test_format_text_percent_zero() {
        assert_eq!(format_text("{percent}%", "", "", Some(0.0)), "0%");
    }

    #[test]
    fn test_format_text_percent_only_no_suffix() {
        assert_eq!(format_text("{message}", "X", "Hello", Some(50.0)), "Hello");
    }

    #[test]
    fn test_validate_negative_threshold() {
        let cfg = AppConfig {
            signals: vec![Signal {
                message: "test".into(),
                icon: "".into(),
                icon_size: 24.0,
                color: (1.0, 1.0, 1.0, 1.0),
                color_name: "white".into(),
                threshold: -5.0,
                state_filter: "any".into(),
                animation: Animation::None,
                duration: Some(5),
                sound: None,
                animation_ref: None,
            }],
            ..Default::default()
        };
        let (errors, _) = cfg.validate();
        assert!(errors.iter().any(|e| e.contains("threshold")));
    }

    #[test]
    fn test_validate_empty_message() {
        let cfg = AppConfig {
            signals: vec![Signal {
                message: "".into(),
                icon: "".into(),
                icon_size: 24.0,
                color: (1.0, 1.0, 1.0, 1.0),
                color_name: "white".into(),
                threshold: 50.0,
                state_filter: "any".into(),
                animation: Animation::None,
                duration: Some(5),
                sound: None,
                animation_ref: None,
            }],
            ..Default::default()
        };
        let (errors, _) = cfg.validate();
        assert!(errors.iter().any(|e| e.contains("message")));
    }

    #[test]
    fn test_validate_high_fps_warning() {
        let cfg = AppConfig {
            fps: 240,
            signals: vec![Signal {
                message: "test".into(),
                icon: "".into(),
                icon_size: 24.0,
                color: (1.0, 1.0, 1.0, 1.0),
                color_name: "white".into(),
                threshold: 50.0,
                state_filter: "any".into(),
                animation: Animation::None,
                duration: Some(5),
                sound: None,
                animation_ref: None,
            }],
            ..Default::default()
        };
        let (_, warnings) = cfg.validate();
        assert!(warnings.iter().any(|w| w.contains("fps")));
    }

    #[test]
    fn test_display_mode_parse() {
        assert_eq!(DisplayMode::parse("anim"), Some(DisplayMode::Anim));
        assert_eq!(DisplayMode::parse("Anim"), Some(DisplayMode::Anim));
        assert_eq!(DisplayMode::parse(" text "), Some(DisplayMode::Text));
        assert_eq!(DisplayMode::parse("animation"), Some(DisplayMode::Anim));
        // Unknown values return None so the caller can warn; they must not
        // silently become Anim, which would drop the notification text.
        assert_eq!(DisplayMode::parse("txet"), None);
        assert_eq!(DisplayMode::parse(""), None);
    }

    fn config_with_animations(names: &[&str]) -> AppConfig {
        let mut config = AppConfig::default();
        for name in names {
            config.animations.insert(
                (*name).to_string(),
                AnimAsset {
                    source: PathBuf::from("/test"),
                    fps: 30,
                    loop_: true,
                    display: DisplayMode::Text,
                    on_complete: OnComplete::Hold,
                    natural_duration: None,
                },
            );
        }
        config
    }

    #[test]
    fn test_resolve_animation_asset_name_shorthand() {
        let config = config_with_animations(&["cube"]);
        let (anim, reference) = config.resolve_animation("cube", None);
        assert_eq!(anim, Animation::None);
        assert_eq!(reference.as_deref(), Some("cube"));
    }

    #[test]
    fn test_resolve_animation_shorthand_wins_over_explicit_ref() {
        let config = config_with_animations(&["cube", "ripple"]);
        let (anim, reference) = config.resolve_animation("cube", Some("ripple"));
        assert_eq!(anim, Animation::None);
        assert_eq!(reference.as_deref(), Some("cube"));
    }

    #[test]
    fn test_resolve_animation_composes_transition_with_content() {
        let config = config_with_animations(&["cube"]);
        let (anim, reference) = config.resolve_animation("fade", Some("cube"));
        assert_eq!(anim, Animation::Fade);
        assert_eq!(reference.as_deref(), Some("cube"));
    }

    #[test]
    fn test_resolve_animation_procedural_only() {
        let config = config_with_animations(&["cube"]);
        let (anim, reference) = config.resolve_animation("bounce", None);
        assert_eq!(anim, Animation::Bounce);
        assert_eq!(reference, None);
    }

    #[test]
    fn test_resolve_animation_keeps_unresolvable_ref_for_validation() {
        // The name is preserved rather than dropped so validate() can report
        // the typo instead of the config rendering a plain text card.
        let config = config_with_animations(&["cube"]);
        let (anim, reference) = config.resolve_animation("pulse", Some("cube_chargee"));
        assert_eq!(anim, Animation::Pulse);
        assert_eq!(reference.as_deref(), Some("cube_chargee"));
    }

    #[test]
    fn test_validate_reports_unresolvable_animation_ref() {
        let mut config = config_with_animations(&["cube"]);
        config.signals = vec![Signal {
            message: "test".into(),
            icon: "".into(),
            icon_size: 24.0,
            color: (1.0, 1.0, 1.0, 1.0),
            color_name: "white".into(),
            threshold: 0.0,
            state_filter: "any".into(),
            animation: Animation::None,
            animation_ref: Some("typo".into()),
            duration: Some(5),
            sound: None,
        }];
        let (errors, _) = config.validate();
        assert!(
            errors.iter().any(|e| e.contains("animation_ref") && e.contains("typo")),
            "expected an error naming the bad ref, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_display_mode_is_copy() {
        // Verify DisplayMode implements Copy (compile-time check via usage)
        let mode = DisplayMode::Text;
        let copy = mode; // Copy, not move
        assert_eq!(mode, copy);
    }

    // --- on_complete ----------------------------------------------------

    #[test]
    fn test_on_complete_parse() {
        assert_eq!(OnComplete::parse("hold"), Some(OnComplete::Hold));
        assert_eq!(OnComplete::parse("Hide"), Some(OnComplete::Hide));
        assert_eq!(OnComplete::parse(" LOOP "), Some(OnComplete::Loop));
        assert_eq!(OnComplete::parse("stop"), None);
    }

    #[test]
    fn test_load_toml_defaults_on_complete_to_hold() {
        let (cfg, _dir) = load_fixture(
            r#"
[animations]
cube = { source = "assets" }

[[signal]]
message = "hi"
color = "white"
threshold = 10
state = "any"
animation_ref = "cube"
"#,
        );
        assert_eq!(cfg.animations["cube"].on_complete, OnComplete::Hold);
        assert!(cfg.animations["cube"].loop_);
    }

    #[test]
    fn test_load_toml_reads_on_complete() {
        let (cfg, _dir) = load_fixture(
            r#"
[animations]
cube = { source = "assets", fps = 30, on_complete = "hide" }
"#,
        );
        assert_eq!(cfg.animations["cube"].on_complete, OnComplete::Hide);
    }

    #[test]
    fn test_load_toml_falls_back_to_hold_on_an_unknown_on_complete() {
        let (cfg, _dir) = load_fixture(
            r#"
[animations]
cube = { source = "assets", on_complete = "explode" }
"#,
        );
        assert_eq!(cfg.animations["cube"].on_complete, OnComplete::Hold);
    }

    // --- derived playback length ----------------------------------------

    #[test]
    fn test_natural_duration_derives_length_from_a_frame_count() {
        let dir = TempDir::new("cfg-dur");
        dir.write_frames(90);

        let at30 = natural_duration(dir.path(), 30).unwrap();
        assert_eq!(at30, std::time::Duration::from_secs(3));

        let at60 = natural_duration(dir.path(), 60).unwrap();
        assert_eq!(at60, std::time::Duration::from_millis(1500));
    }

    #[test]
    fn test_natural_duration_is_absent_without_frames_or_without_fps() {
        let dir = TempDir::new("cfg-dur-edge");
        assert_eq!(natural_duration(dir.path(), 30), None);

        dir.write_frames(4);
        assert_eq!(natural_duration(dir.path(), 0), None);
    }

    // --- notification timing --------------------------------------------

    fn signal_with(duration: Option<u64>, anim_ref: Option<&str>, anim: Animation) -> Signal {
        Signal {
            message: "hi".into(),
            icon: String::new(),
            icon_size: 24.0,
            color: (1.0, 1.0, 1.0, 1.0),
            color_name: "white".into(),
            threshold: 0.0,
            state_filter: "any".into(),
            animation: anim,
            animation_ref: anim_ref.map(str::to_string),
            duration,
            sound: None,
        }
    }

    #[test]
    fn test_explicit_duration_always_wins() {
        let cfg = AppConfig::default();
        let sig = signal_with(Some(12), None, Animation::None);
        assert_eq!(display_seconds(&sig, &cfg), Some(12.0));
    }

    #[test]
    fn test_zero_duration_means_indefinite() {
        let cfg = AppConfig::default();
        let sig = signal_with(Some(0), None, Animation::None);
        assert_eq!(display_seconds(&sig, &cfg), None);
        assert_eq!(notification_duration(&sig, &cfg), std::time::Duration::MAX);
    }

    #[test]
    fn test_omitted_duration_falls_back_to_the_default() {
        let cfg = AppConfig::default();
        let sig = signal_with(None, None, Animation::None);
        assert_eq!(display_seconds(&sig, &cfg), Some(DEFAULT_DURATION_SECS as f64));
    }

    #[test]
    fn test_omitted_duration_follows_the_frame_animation_length() {
        let mut cfg = AppConfig::default();
        cfg.animations.insert(
            "cube".to_string(),
            AnimAsset {
                source: PathBuf::from("/test"),
                fps: 30,
                loop_: true,
                display: DisplayMode::Text,
                on_complete: OnComplete::Hold,
                natural_duration: Some(std::time::Duration::from_secs(7)),
            },
        );

        // 216 frames at 30fps is 7.2s; the config stores 7s here, and either
        // way the point is that nobody has to write `duration` by hand.
        let sig = signal_with(None, Some("cube"), Animation::None);
        assert_eq!(display_seconds(&sig, &cfg), Some(7.0));

        // An explicit duration still overrides the derived one.
        let sig = signal_with(Some(2), Some("cube"), Animation::None);
        assert_eq!(display_seconds(&sig, &cfg), Some(2.0));
    }

    #[test]
    fn test_omitted_duration_uses_the_default_for_an_unresolvable_ref() {
        let cfg = AppConfig::default();
        let sig = signal_with(None, Some("typo"), Animation::None);
        assert_eq!(display_seconds(&sig, &cfg), Some(DEFAULT_DURATION_SECS as f64));
    }

    #[test]
    fn test_transition_frames_scales_with_fps() {
        let cfg = AppConfig { fps: 60, ..Default::default() };
        let sig = signal_with(Some(2), None, Animation::Fade);
        assert_eq!(transition_frames(&sig, &cfg), 120.0);
    }

    #[test]
    fn test_animation_only_reload_reports_the_new_assets() {
        // Loading an [animations] table at all is the case --check-config
        // previously ignored entirely.
        let (cfg, _dir) = load_fixture(
            r#"
[animations]
cube = { source = "assets", fps = 0 }
"#,
        );
        let (errors, _) = cfg.validate();
        assert!(
            errors.iter().any(|e| e.contains("animations.cube") && e.contains("fps")),
            "expected an fps error, got {:?}",
            errors
        );
    }
}
