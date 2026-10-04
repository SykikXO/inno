use cairo::{FontSlant, FontWeight};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// Constants
pub const DEFAULT_MARGIN: i32 = 10;
pub const DEFAULT_FONT_SIZE: f64 = 24.0;
pub const DEFAULT_ICON_SIZE: f64 = 24.0;
pub const HIDE_TIMEOUT_SECS: u64 = 86400;

// TOML config file structure
#[derive(Debug, Deserialize, Default)]
struct ConfigFile {
    general: Option<GeneralConfig>,
    position: Option<PositionTable>,
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
    /// Deprecated six-field form. Superseded by the top-level `[position]`
    /// table, which is order-independent and can address each edge.
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
    /// Seconds between repeats while this state persists. Omitted or 0 is off.
    remind: Option<u64>,
    sound: Option<String>,
    #[serde(default)]
    animation_ref: Option<String>,
    /// Buttons drawn on the card. Clicking one runs its command instead of
    /// dismissing the notification.
    ///
    /// Renamed because the TOML reads `[[signal.action]]`, which is what the
    /// notification spec calls these, and a reader should not have to know that
    /// serde matches the field name exactly to find out why theirs did not load.
    #[serde(default, rename = "action")]
    actions: Vec<ActionConfig>,
}

/// A button on a notification: a label to draw and a command to run.
#[derive(Debug, Clone, Deserialize)]
struct ActionConfig {
    label: String,
    command: String,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HAnchor {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VAnchor {
    Top,
    Center,
    #[default]
    Bottom,
}

/// Where the notification sits, and how far from each anchored edge.
///
/// Margins are per edge rather than per axis because that is what the compositor
/// actually acts on: it honours the margin of an anchored edge and ignores the
/// margin of an axis with no anchor, since an unanchored axis is centred. The
/// old format had `margin_h`, `margin_v` and a pair of offsets to express that,
/// where the offsets existed only because a centred axis ignores its margin.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Anchor {
    pub h: HAnchor,
    pub v: VAnchor,
    pub margin_top: i32,
    pub margin_bottom: i32,
    pub margin_left: i32,
    pub margin_right: i32,
}

/// The `[position]` table.
#[derive(Debug, Clone, Default, Deserialize)]
struct PositionTable {
    /// Which edge or corner to sit against. Order does not matter, so
    /// `bottom-center` and `center-bottom` are the same placement.
    anchor: Option<String>,
    /// Distance from every anchored edge, unless overridden per edge below.
    margin: Option<i32>,
    margin_top: Option<i32>,
    margin_bottom: Option<i32>,
    margin_left: Option<i32>,
    margin_right: Option<i32>,
}

impl Anchor {
    /// The anchor names, in the order the error message lists them.
    const ANCHOR_NAMES: &'static str =
        "top, center, bottom (vertical) and left, center, right (horizontal), \
         combined freely, e.g. \"bottom-center\", \"top-left\", \"center\"";

    /// Parses an anchor name into a horizontal and vertical anchor.
    ///
    /// Strict on purpose. An unrecognised word used to fall through to a
    /// default, which meant a typo like `center,topp` silently put the
    /// notification on the opposite edge instead of complaining.
    fn parse_anchor(s: &str) -> Result<(HAnchor, VAnchor), String> {
        let (mut h, mut v) = (HAnchor::Center, VAnchor::Center);
        let mut seen = false;
        for part in s.split(['-', ' ']).map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "top" => v = VAnchor::Top,
                "bottom" => v = VAnchor::Bottom,
                "left" => h = HAnchor::Left,
                "right" => h = HAnchor::Right,
                // Already the default for both axes, and the obvious thing to
                // write when naming a single edge.
                "center" | "centre" | "middle" => {}
                other => {
                    return Err(format!(
                        "unknown anchor '{other}'. Expected any of: {}",
                        Anchor::ANCHOR_NAMES
                    ));
                }
            }
            seen = true;
        }
        if !seen {
            return Err(format!("empty anchor. Expected any of: {}", Anchor::ANCHOR_NAMES));
        }
        Ok((h, v))
    }

    /// Builds an anchor from the `[position]` table.
    fn from_table(t: &PositionTable) -> Result<Self, String> {
        let (h, v) = match t.anchor.as_deref() {
            Some(name) => Self::parse_anchor(name)?,
            None => (HAnchor::Center, VAnchor::Bottom),
        };
        // `margin` is the shorthand for every edge; the per-edge keys win.
        let base = t.margin.unwrap_or(DEFAULT_MARGIN);
        Ok(Anchor {
            h,
            v,
            margin_top: t.margin_top.unwrap_or(base),
            margin_bottom: t.margin_bottom.unwrap_or(base),
            margin_left: t.margin_left.unwrap_or(base),
            margin_right: t.margin_right.unwrap_or(base),
        })
    }

    /// Parses the old `position = "horizontal,vertical,mh,mv,ox,oy"` string.
    ///
    /// Kept working so existing configs do not break, and it maps exactly onto
    /// the per-edge margins: the horizontal margin applied to both sides and the
    /// offset shifted one side out by the offset and the other in by it.
    fn parse(s: &str) -> Result<Self, String> {
        let parts: Vec<&str> = s.split(',').map(str::trim).collect();
        let (h, v) = if s.trim().is_empty() {
            (HAnchor::Center, VAnchor::Bottom)
        } else {
            match (h_word(parts[0]), v_word(parts.get(1).copied().unwrap_or("bottom"))) {
                (Some(h), Some(v)) => (h, v),
                _ => {
                    return Err(format!(
                        "unknown anchor in \"{s}\". Expected any of: {}",
                        Anchor::ANCHOR_NAMES
                    ));
                }
            }
        };
        let mh = num(parts.get(2)).unwrap_or(DEFAULT_MARGIN);
        let mv = num(parts.get(3)).unwrap_or(mh);
        let ox = num(parts.get(4)).unwrap_or(0);
        let oy = num(parts.get(5)).unwrap_or(0);
        Ok(Anchor {
            h,
            v,
            margin_top: mv + oy,
            margin_bottom: mv - oy,
            margin_left: mh - ox,
            margin_right: mh + ox,
        })
    }

    /// Which of the given per-edge margin keys the compositor will ignore,
    /// because their axis has no anchor and is therefore centred.
    ///
    /// Only explicit keys are worth reporting. The `margin` shorthand sets all
    /// four edges by design, so on a centred axis two of them are always dead
    /// and saying so on every stock config would be pure noise.
    fn ignored_explicit_keys(&self, t: &PositionTable) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.v == VAnchor::Center {
            if t.margin_top.is_some() {
                out.push("margin_top");
            }
            if t.margin_bottom.is_some() {
                out.push("margin_bottom");
            }
        }
        if self.h == HAnchor::Center {
            if t.margin_left.is_some() {
                out.push("margin_left");
            }
            if t.margin_right.is_some() {
                out.push("margin_right");
            }
        }
        out
    }
}

fn h_word(word: &str) -> Option<HAnchor> {
    match word.to_ascii_lowercase().as_str() {
        "left" => Some(HAnchor::Left),
        "right" => Some(HAnchor::Right),
        "center" | "centre" | "middle" => Some(HAnchor::Center),
        _ => None,
    }
}

fn v_word(word: &str) -> Option<VAnchor> {
    match word.to_ascii_lowercase().as_str() {
        "top" => Some(VAnchor::Top),
        "bottom" => Some(VAnchor::Bottom),
        "center" | "centre" | "middle" => Some(VAnchor::Center),
        _ => None,
    }
}

fn num(field: Option<&&str>) -> Option<i32> {
    field?.parse().ok()
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
    // One left-to-right pass rather than a chain of replaces, so a substituted
    // value is never rescanned by a later placeholder. Chaining made an icon
    // containing "{message}" expand, because {message} was substituted after it.
    // `{percent}%` is matched before `{percent}` so the suffixed form wins, and
    // anything else is left exactly as written.
    let mut out = String::with_capacity(fmt.len());
    let mut rest = fmt;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}') else {
            out.push_str(&rest[open..]);
            return finish(out);
        };
        let token = &rest[open + 1..open + close];
        let after = &rest[open + close + 1..];
        // `{percent}%` is one token, and with no reading it renders as nothing
        // rather than as a bare `%`.
        if token == "percent" && after.starts_with('%') {
            out.push_str(&pct_suffixed);
            rest = &after[1..];
            continue;
        }
        match token {
            "percent" => out.push_str(&pct),
            "icon" => out.push_str(icon),
            "message" => out.push_str(message),
            _ => out.push_str(&rest[open..open + close + 1]),
        }
        rest = after;
    }
    out.push_str(rest);
    finish(out)
}

/// Trims the space a missing `{percent}` leaves behind, so `{message}
/// {percent}%` degrades to `{message}` rather than `{message} %`.
fn finish(mut out: String) -> String {
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
    pub remind: Option<u64>,
    pub sound: Option<PathBuf>,
    pub actions: Vec<Action>,
}

/// A button on a notification.
///
/// inno does not serve `org.freedesktop.Notifications`, so there is no sending
/// application to hand a choice back to. The only thing an action can do is run
/// something named in the config, which is the same level of trust the existing
/// `sound` field already asks for: it spawns a process.
#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub label: String,
    pub command: String,
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
    /// Problems found while parsing that are the user's to fix. Kept on the
    /// config so `validate()` can report them instead of the loader silently
    /// falling back to a default the user never asked for.
    pub load_errors: Vec<String>,
    /// Non-fatal problems noticed while parsing, for `validate()` to report.
    pub load_warnings: Vec<String>,
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
            load_errors: Vec::new(),
            load_warnings: Vec::new(),
        }
    }
}

/// Playback length of a frame directory, from a file count and a frame rate.
/// Scanning the directory is cheap, unlike decoding it.
fn natural_duration(dir: &std::path::Path, fps: u64) -> Option<std::time::Duration> {
    let frames = std::fs::read_dir(dir)
        .map(|entries| entries.filter_map(|e| e.ok()).filter(|e| crate::animation::is_png(&e.path())).count())
        .unwrap_or(0);

    (fps > 0 && frames > 0)
        .then(|| std::time::Duration::from_secs_f64(frames as f64 / fps as f64))
}

/// Resolves `p` against the config's directory unless it is already absolute.
fn resolve_against(config_dir: Option<&Path>, p: PathBuf) -> PathBuf {
    if p.is_absolute() {
        return p;
    }
    match config_dir {
        Some(dir) => dir.join(p),
        None => p,
    }
}

/// True when every RGBA channel sits in the 0.0-1.0 range cairo accepts.
fn channels_in_range(c: (f64, f64, f64, f64)) -> bool {
    let (r, g, b, a) = c;
    [r, g, b, a].iter().all(|v| (0.0..=1.0).contains(v))
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

    fn load_toml(&mut self, path: &PathBuf) -> anyhow::Result<()> {
        let content = std::fs::read_to_string(path)?;
        let file: ConfigFile = toml::from_str(&content)?;

        // General settings
        let mut legacy_position: Option<String> = None;
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
            legacy_position = general.position.clone();
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

        if let Some(table) = file.position {
            if legacy_position.is_some() {
                self.load_errors.push(
                    "position is set both as a [position] table and as general.position.                      Keep only the [position] table."
                        .to_string(),
                );
            } else {
                match Anchor::from_table(&table) {
                    Ok(anchor) => {
                        let ignored = anchor.ignored_explicit_keys(&table);
                        if !ignored.is_empty() {
                            self.load_warnings.push(format!(
                                "{} ignored: that axis is not anchored, so the compositor \
                                 centres it. Anchor a side or corner to use them.",
                                ignored.join(" and ")
                            ));
                        }
                        self.anchor = anchor;
                    }
                    Err(e) => self.load_errors.push(e),
                }
            }
        } else if let Some(pos) = legacy_position {
            match Anchor::parse(&pos) {
                Ok(anchor) => {
                    eprintln!(
                        "inno: position = \"{pos}\" is the old six-field form and will be \
                         removed. Use a [position] table: anchor, margin, and margin_<edge> \
                         to override one edge. See the README."
                    );
                    self.anchor = anchor;
                }
                Err(e) => self.load_errors.push(e),
            }
        }

        // Relative paths in the config resolve against the config's own
        // directory, so a config that works from one location works from a copy.
        let config_dir = path.parent();

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

            let source = resolve_against(config_dir, anim_cfg.source);

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
                // validate() reports the unresolved name, with the signal index.
                .unwrap_or((1.0, 1.0, 1.0, 1.0));

            let sound_path = sig_cfg.sound.map(|s| resolve_against(config_dir, PathBuf::from(s)));

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
                remind: sig_cfg.remind,
                sound: sound_path,
                actions: sig_cfg
                    .actions
                    .into_iter()
                    .map(|a| Action { label: a.label, command: a.command })
                    .collect(),
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

        errors.extend(self.load_errors.iter().cloned());
        warnings.extend(self.load_warnings.iter().cloned());

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
            if !channels_in_range(sig.color) {
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

        // A missing or empty frame directory is a warning, not an error: the one
        // notification that names it renders as a plain text card and every other
        // signal still works. It is an error only when a signal actually points
        // at the broken entry, which is checked per signal above.
        for (name, anim) in &self.animations {
            match std::fs::read_dir(&anim.source) {
                Ok(entries) => {
                    let frames = entries.filter_map(|e| e.ok()).filter(|e| crate::animation::is_png(&e.path())).count();
                    if frames == 0 {
                        warnings.push(format!("animations.{}: no PNG frames in {}", name, anim.source.display()));
                    }
                }
                Err(e) => warnings.push(format!(
                    "animations.{}: cannot read {}: {}",
                    name,
                    anim.source.display(),
                    e
                )),
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

        if !channels_in_range(self.bg_color) {
            errors.push("bg_color values must be 0.0-1.0".to_string());
        }

        if !channels_in_range(self.text_color) {
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

    #[test]
    fn actions_are_read_in_order_with_their_commands() {
        let (cfg, _t) = load_fixture(
            r#"
[[signal]]
message = "low"
icon = ""
color = "white"
threshold = 0
state = "any"

[[signal.action]]
label = "Plug in"
command = "notify-send hi"

[[signal.action]]
label = "Ignore"
command = "true"
"#,
        );
        assert_eq!(cfg.signals.len(), 1);
        let actions = &cfg.signals[0].actions;
        assert_eq!(actions.len(), 2, "both actions parsed");
        assert_eq!(actions[0].label, "Plug in", "order is preserved");
        assert_eq!(actions[0].command, "notify-send hi");
        assert_eq!(actions[1].label, "Ignore");
    }

    #[test]
    fn a_signal_without_actions_has_none() {
        let (cfg, _t) = load_fixture(
            r#"
[[signal]]
message = "low"
icon = ""
color = "white"
threshold = 0
state = "any"
"#,
        );
        assert!(cfg.signals[0].actions.is_empty());
    }

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
    fn test_position_table_anchor_and_margin() {
        let (cfg, _) = load_fixture(
            r#"
[position]
anchor = "top-right"
margin = 40
"#,
        );
        assert_eq!(cfg.anchor.h, HAnchor::Right);
        assert_eq!(cfg.anchor.v, VAnchor::Top);
        assert_eq!(cfg.anchor.margin_top, 40);
        assert_eq!(cfg.anchor.margin_right, 40);
        // Not anchored on these axes, so the compositor centres them.
        assert_eq!(cfg.anchor.margin_bottom, 40);
        assert_eq!(cfg.anchor.margin_left, 40);
        assert!(cfg.load_errors.is_empty(), "{:?}", cfg.load_errors);
    }

    #[test]
    fn test_position_per_edge_margin_overrides_the_shorthand() {
        let (cfg, _) = load_fixture(
            r#"
[position]
anchor = "bottom-left"
margin = 90
margin_left = 20
"#,
        );
        assert_eq!(cfg.anchor.margin_left, 20);
        assert_eq!(cfg.anchor.margin_bottom, 90);
        assert_eq!(cfg.anchor.margin_right, 90);
        assert_eq!(cfg.anchor.margin_top, 90);
    }

    #[test]
    fn test_position_anchor_word_order_does_not_matter() {
        for name in ["bottom-center", "center-bottom", "bottom centre", "BOTTOM-CENTER"] {
            let (h, v) = Anchor::parse_anchor(name).expect(name);
            assert_eq!(h, HAnchor::Center, "{name}");
            assert_eq!(v, VAnchor::Bottom, "{name}");
        }
    }

    #[test]
    fn test_position_single_edge_name_centres_the_other_axis() {
        let (h, v) = Anchor::parse_anchor("top").unwrap();
        assert_eq!((h, v), (HAnchor::Center, VAnchor::Top));
        let (h, v) = Anchor::parse_anchor("left").unwrap();
        assert_eq!((h, v), (HAnchor::Left, VAnchor::Center));
    }

    #[test]
    fn test_position_rejects_a_typo_instead_of_silently_moving_it() {
        // "topp" used to fall through to the bottom edge, so a typo relocated the
        // notification to the opposite side of the screen with no complaint.
        let err = Anchor::parse_anchor("topp").unwrap_err();
        assert!(err.contains("topp"), "{err}");
        assert!(err.contains("bottom"), "the message should list what is valid: {err}");
    }

    #[test]
    fn test_position_defaults_to_bottom_centre_when_unspecified() {
        let a = Anchor::from_table(&PositionTable::default()).unwrap();
        assert_eq!(a.h, HAnchor::Center);
        assert_eq!(a.v, VAnchor::Bottom);
        assert_eq!(a.margin_bottom, DEFAULT_MARGIN);
    }

    #[test]
    fn test_only_explicit_ignored_margins_are_reported() {
        // The shorthand sets all four edges, so on a centred axis two are dead
        // by design. That must not warn, or every stock config would.
        let table = PositionTable {
            anchor: Some("bottom-center".into()),
            margin: Some(90),
            ..Default::default()
        };
        let a = Anchor::from_table(&table).unwrap();
        assert!(a.ignored_explicit_keys(&table).is_empty());

        // Naming the dead edge explicitly is a mistake worth reporting.
        let table = PositionTable {
            anchor: Some("bottom-center".into()),
            margin_left: Some(40),
            ..Default::default()
        };
        let a = Anchor::from_table(&table).unwrap();
        assert_eq!(a.ignored_explicit_keys(&table), ["margin_left"]);
    }

    #[test]
    fn test_no_warning_when_every_anchored_edge_is_named() {
        let (cfg, _) = load_fixture(
            r#"
[position]
anchor = "bottom-center"
margin = 90
"#,
        );
        assert!(cfg.load_warnings.is_empty(), "{:?}", cfg.load_warnings);
    }

    #[test]
    fn test_warning_when_a_named_edge_cannot_be_used() {
        let (cfg, _) = load_fixture(
            r#"
[position]
anchor = "bottom-center"
margin = 90
margin_left = 40
"#,
        );
        assert!(
            cfg.load_warnings.iter().any(|w| w.contains("margin_left")),
            "{:?}",
            cfg.load_warnings
        );
    }

    /// The old string form has to keep producing exactly what it produced
    /// before, or an upgrade silently moves everyone's notification.
    #[test]
    fn test_legacy_position_string_still_maps_onto_the_same_margins() {
        // center,bottom,10,20,5,-15 meant: horizontal margin 10 both sides,
        // vertical 20, shifted 5 right and 15 up.
        let a = Anchor::parse("center,bottom,10,20,5,-15").unwrap();
        assert_eq!(a.h, HAnchor::Center);
        assert_eq!(a.v, VAnchor::Bottom);
        assert_eq!(a.margin_left, 10 - 5);
        assert_eq!(a.margin_right, 10 + 5);
        assert_eq!(a.margin_top, 20 + -15);
        assert_eq!(a.margin_bottom, 20 - -15);
    }

    #[test]
    fn test_legacy_position_string_falls_back_as_before() {
        let a = Anchor::parse("left,top").unwrap();
        assert_eq!(a.h, HAnchor::Left);
        assert_eq!(a.v, VAnchor::Top);
        assert_eq!(a.margin_left, DEFAULT_MARGIN);
        assert_eq!(a.margin_bottom, DEFAULT_MARGIN);
        assert_eq!(a.margin_right, DEFAULT_MARGIN);

        let a = Anchor::parse("right,bottom,50").unwrap();
        assert_eq!(a.h, HAnchor::Right);
        assert_eq!(a.margin_right, 50);
        // margin_v used to fall back to margin_h.
        assert_eq!(a.margin_bottom, 50);

        let a = Anchor::parse("").unwrap();
        assert_eq!(a.h, HAnchor::Center);
        assert_eq!(a.v, VAnchor::Bottom);
    }

    #[test]
    fn test_legacy_position_string_rejects_an_unknown_anchor() {
        assert!(Anchor::parse("middleish,top").is_err());
    }

    #[test]
    fn test_position_set_both_ways_is_an_error() {
        let (cfg, _) = load_fixture(
            r#"
[general]
position = "center,bottom,0,90,0,0"

[position]
anchor = "bottom-center"
"#,
        );
        assert!(
            cfg.load_errors.iter().any(|e| e.contains("both")),
            "{:?}",
            cfg.load_errors
        );
    }

    #[test]
    fn test_a_typo_in_position_becomes_a_load_error() {
        let (cfg, _) = load_fixture(
            r#"
[position]
anchor = "bottom-cener"
"#,
        );
        assert!(!cfg.load_errors.is_empty(), "a typo must be reported");
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
                    remind: None,
                    actions: Vec::new(),
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
                    remind: None,
                    actions: Vec::new(),
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
                    remind: None,
                    actions: Vec::new(),
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
                    remind: None,
                    actions: Vec::new(),
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
                    remind: None,
                    actions: Vec::new(),
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
                remind: None,
                actions: Vec::new(),
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
                remind: None,
                actions: Vec::new(),
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
                remind: None,
                actions: Vec::new(),
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
                remind: None,
                actions: Vec::new(),
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
                remind: None,
                actions: Vec::new(),
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
                remind: None,
                actions: Vec::new(),
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
                remind: None,
                actions: Vec::new(),
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
                remind: None,
                actions: Vec::new(),
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
                remind: None,
                actions: Vec::new(),
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
                remind: None,
                actions: Vec::new(),
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
                remind: None,
                actions: Vec::new(),
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
                remind: None,
                actions: Vec::new(),
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
        let a = Anchor::parse("  center , top , 15 ").unwrap();
        assert_eq!(a.h, HAnchor::Center);
        assert_eq!(a.v, VAnchor::Top);
        assert_eq!(a.margin_bottom, 15);
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
                remind: None,
                actions: Vec::new(),
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
                remind: None,
                actions: Vec::new(),
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
                remind: None,
                actions: Vec::new(),
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
            remind: None,
            actions: Vec::new(),
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
            remind: None,
            actions: Vec::new(),
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
