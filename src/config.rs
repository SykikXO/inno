use std::collections::HashMap;
use std::fs;

#[derive(Debug, Clone, PartialEq)]
pub enum Animation {
    None,
    Flicker,
    Pulse,
}

#[derive(Debug, Clone)]
pub struct Signal {
    pub message: String, // Custom message like "Warning!" or "Low Battery"
    pub color: (f64, f64, f64, f64),
    pub threshold: f64,       // battery percentage to trigger
    pub state_filter: String, // "charging", "discharging", or "any"
    pub animation: Animation,
    pub duration: u64,
}

// Anchor positions: left/center/right for horizontal, top/center/bottom for vertical
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Anchor {
    pub h: HAnchor, // horizontal
    pub v: VAnchor, // vertical
    pub margin_h: i32,
    pub margin_v: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum HAnchor {
    Left,
    #[default]
    Right,
    Center,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum VAnchor {
    Top,
    #[default]
    Bottom,
    Center,
}

impl Anchor {
    // Parse: "left,top" or "center,bottom,10,20"
    fn parse(s: &str) -> Self {
        let p: Vec<&str> = s.split(',').map(str::trim).collect();
        let h = p
            .get(0)
            .map(|v| match v.to_lowercase().as_str() {
                "left" => HAnchor::Left,
                "center" => HAnchor::Center,
                _ => HAnchor::Right,
            })
            .unwrap_or_default();
        let v = p
            .get(1)
            .map(|v| match v.to_lowercase().as_str() {
                "top" => VAnchor::Top,
                "center" => VAnchor::Center,
                _ => VAnchor::Bottom,
            })
            .unwrap_or_default();
        let margin_h = p.get(2).and_then(|v| v.parse().ok()).unwrap_or(10);
        let margin_v = p.get(3).and_then(|v| v.parse().ok()).unwrap_or(margin_h);
        Self {
            h,
            v,
            margin_h,
            margin_v,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub font: String,
    pub text_color: (f64, f64, f64, f64),
    pub bg_color: (f64, f64, f64, f64),
    pub colors: HashMap<String, (f64, f64, f64, f64)>,
    pub signals: Vec<Signal>,
    pub anchor: Anchor,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            font: "Sans".to_string(),
            text_color: (1.0, 1.0, 1.0, 1.0),
            bg_color: (0.0, 0.0, 0.0, 0.8),
            colors: HashMap::new(),
            signals: Vec::new(),
            anchor: Anchor::default(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let mut config = Self::default();

        // Check these paths in order (user config first, then system default)
        let paths = [
            std::env::current_dir().ok().map(|p| p.join("inno.conf")),
            dirs::config_dir().map(|p| p.join("inno/inno.conf")),
            Some(std::path::PathBuf::from("/etc/xdg/inno/inno.conf")),
        ];

        let mut loaded_path = None;
        for path in paths.iter().flatten() {
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

        eprintln!("Loading config from: {:?}", config_path);
        let Ok(content) = fs::read_to_string(&config_path) else {
            return config;
        };

        // First pass: collect colors
        for line in content.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());

            if value.starts_with('(') && value.ends_with(')') {
                if let Some(c) = parse_rgba(value) {
                    config.colors.insert(key.to_string(), c);
                }
            }
        }

        // Second pass: parse everything else
        for line in content.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());

            match key {
                "font" => config.font = value.to_string(),
                "text_color" => {
                    if let Some(c) = config.resolve_color(value) {
                        config.text_color = c
                    }
                }
                "bg_color" => {
                    if let Some(c) = config.resolve_color(value) {
                        config.bg_color = c
                    }
                }
                "position" => config.anchor = Anchor::parse(value),
                "signal" => {
                    if let Some(s) = config.parse_signal(value) {
                        config.signals.push(s)
                    }
                }
                _ => {}
            }
        }

        // No global sort - find_signal handles priority
        config
    }

    fn resolve_color(&self, s: &str) -> Option<(f64, f64, f64, f64)> {
        // Check if it's a named color from config
        if let Some(c) = self.colors.get(s) {
            return Some(*c);
        }
        // Try parsing as (r,g,b,a) tuple
        parse_rgba(s)
    }

    // signal = message,color,threshold,state,animation,duration
    fn parse_signal(&self, s: &str) -> Option<Signal> {
        let p: Vec<&str> = s.split(',').map(str::trim).collect();
        if p.len() < 6 {
            return None;
        }

        Some(Signal {
            message: p[0].to_string(),
            color: self.resolve_color(p[1]).unwrap_or((1.0, 1.0, 1.0, 1.0)),
            threshold: p[2].parse().unwrap_or(100.0),
            state_filter: p[3].to_lowercase(),
            animation: match p[4] {
                "flicker" => Animation::Flicker,
                "pulse" => Animation::Pulse,
                _ => Animation::None,
            },
            duration: p[5].parse().unwrap_or(5),
        })
    }

    pub fn find_signal(&self, pct: f64, state: &str) -> Option<&Signal> {
        let is_charging = state.eq_ignore_ascii_case("charging");

        // Filter signals matching current state
        let mut matches: Vec<&Signal> = self
            .signals
            .iter()
            .filter(|s| {
                let state_match =
                    s.state_filter == "any" || s.state_filter.eq_ignore_ascii_case(state);
                if !state_match {
                    return false;
                }

                if is_charging {
                    pct >= s.threshold
                } else {
                    pct <= s.threshold
                }
            })
            .collect();

        // Sort by priority:
        // Discharging: lower threshold = higher priority
        // Charging: higher threshold = higher priority
        if is_charging {
            matches.sort_by(|a, b| b.threshold.partial_cmp(&a.threshold).unwrap());
        } else {
            matches.sort_by(|a, b| a.threshold.partial_cmp(&b.threshold).unwrap());
        }

        matches.first().copied()
    }
}

// Parse (r,g,b,a) where each is a float 0.0-1.0
fn parse_rgba(s: &str) -> Option<(f64, f64, f64, f64)> {
    let s = s.trim().trim_start_matches('(').trim_end_matches(')');
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 4 {
        return None;
    }

    let r = parts[0].trim().parse().ok()?;
    let g = parts[1].trim().parse().ok()?;
    let b = parts[2].trim().parse().ok()?;
    let a = parts[3].trim().parse().ok()?;
    Some((r, g, b, a))
}
