use crate::config::{Animation, AppConfig, Signal};
use cairo::{Context, LinearGradient};
use std::f64::consts::PI;

#[derive(Debug, Clone)]
pub struct DrawState {
    pub frame: u32,
    pub visible: bool,
    pub alpha: f64,
    pub offset_x: f64,
    pub offset_y: f64,
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            frame: 0,
            visible: true,
            alpha: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
        }
    }
}

impl DrawState {
    pub fn tick(&mut self, anim: &Animation) {
        self.frame = self.frame.wrapping_add(1);
        let t = self.frame as f64;
        
        match anim {
            Animation::Flicker => {
                self.visible = (self.frame / 15) % 2 == 0;
                self.alpha = 1.0;
                self.offset_x = 0.0;
                self.offset_y = 0.0;
            }
            Animation::Pulse => {
                self.visible = true;
                self.alpha = 0.7 + 0.4 * (t * 0.15).sin().abs();
                self.offset_x = 0.0;
                self.offset_y = 0.0;
            }
            Animation::Fade => {
                // Fade in for first ~20 frames, hold, then will fade out before hide
                self.visible = true;
                let fade_in_duration = 20.0;
                if t < fade_in_duration {
                    self.alpha = (t / fade_in_duration).min(1.0);
                } else {
                    self.alpha = 1.0;
                }
                self.offset_x = 0.0;
                self.offset_y = 0.0;
            }
            Animation::Slide => {
                self.visible = true;
                self.alpha = 1.0;
                // Slide in from right, ease out
                let progress = (t * 0.1).min(1.0);
                let eased = 1.0 - (1.0 - progress).powi(3);
                self.offset_x = (1.0 - eased) * 200.0;
                self.offset_y = 0.0;
            }
            Animation::Bounce => {
                self.visible = true;
                self.alpha = 1.0;
                self.offset_x = 0.0;
                // Bouncing effect
                let bounce = (t * 0.2).sin().abs() * (1.0 / (1.0 + t * 0.02));
                self.offset_y = -bounce * 20.0;
            }
            Animation::None => {
                self.visible = true;
                self.alpha = 1.0;
                self.offset_x = 0.0;
                self.offset_y = 0.0;
            }
        }
    }

    pub fn reset(&mut self) {
        self.frame = 0;
        self.visible = true;
        self.alpha = 1.0;
        self.offset_x = 0.0;
        self.offset_y = 0.0;
    }
}

/// Draw a rounded rectangle path
fn rounded_rect(cr: &Context, x: f64, y: f64, w: f64, h: f64, radius: f64) {
    let r = radius.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * PI / 2.0);
    cr.close_path();
}

/// Helper to measure icon extents
fn measure_icon(cr: &Context, icon: &str, size: f64) -> cairo::TextExtents {
    cr.set_font_size(size);
    cr.text_extents(icon).unwrap()
}

/// Format notification text using config format string
pub fn format_text(format: &str, icon: &str, message: &str, percent: f64) -> String {
    format
        .replace("{icon}", icon)
        .replace("{message}", message)
        .replace("{percent}", &format!("{:.0}", percent))
}

pub fn draw_with_signal(
    cr: &Context,
    text: &str,
    config: &AppConfig,
    signal: Option<&Signal>,
    state: &DrawState,
) -> (i32, i32) {
    let (r_bg, g_bg, b_bg, a_bg) = config.bg_color;
    let (r, g, b, a) = signal.map(|s| s.color).unwrap_or(config.text_color);

    // Flicker off - return minimal size
    if signal.is_some_and(|s| s.animation == Animation::Flicker && !state.visible) {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        cr.set_operator(cairo::Operator::Source);
        cr.paint().unwrap();
        return (1, 1);
    }

    // Fade complete (alpha near zero means completely faded)
    if signal.is_some_and(|s| s.animation == Animation::Fade) && state.alpha <= 0.01 {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        cr.set_operator(cairo::Operator::Source);
        cr.paint().unwrap();
        return (1, 1);
    }

    let alpha = state.alpha;

    // Select font and measure icon
    cr.select_font_face(&config.font, config.font_slant, config.font_weight);
    
    let mut icon_w = 0.0;
    if let Some(s) = signal {
        if !s.icon.is_empty() {
            let icon_ext = measure_icon(cr, &s.icon, s.icon_size);
            icon_w = icon_ext.x_advance() + 10.0;
        }
    }

    // Measure main text
    cr.set_font_size(config.font_size);
    let ext = cr.text_extents(text).unwrap();

    let w = ext.width().ceil() as i32 + 20 + icon_w as i32;
    let h = ext.height().ceil() as i32 + 20;

    // Clear canvas
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    cr.set_operator(cairo::Operator::Source);
    cr.paint().unwrap();

    // Draw background (with optional gradient and rounded corners)
    cr.set_operator(cairo::Operator::Over);
    
    if config.gradient {
        let gradient = LinearGradient::new(0.0, 0.0, w as f64, 0.0);
        gradient.add_color_stop_rgba(0.0, r_bg, g_bg, b_bg, a_bg * alpha);
        gradient.add_color_stop_rgba(1.0, r_bg * 0.7, g_bg * 0.7, b_bg * 0.7, a_bg * alpha * 0.8);
        cr.set_source(&gradient).unwrap();
    } else {
        cr.set_source_rgba(r_bg, g_bg, b_bg, a_bg * alpha);
    }

    if config.border_radius > 0.0 {
        rounded_rect(cr, 0.0, 0.0, w as f64, h as f64, config.border_radius);
        cr.fill().unwrap();
    } else {
        cr.rectangle(0.0, 0.0, w as f64, h as f64);
        cr.fill().unwrap();
    }

    // Draw icon
    let text_x = if let Some(s) = signal {
        if !s.icon.is_empty() {
            let icon_ext = measure_icon(cr, &s.icon, s.icon_size);
            cr.set_source_rgba(r, g, b, a * alpha);
            cr.move_to(
                10.0 - icon_ext.x_bearing(),
                h as f64 / 2.0 - (icon_ext.height() / 2.0 + icon_ext.y_bearing()),
            );
            cr.show_text(&s.icon).unwrap();
            cr.set_font_size(config.font_size);
            10.0 + icon_w
        } else {
            10.0
        }
    } else {
        10.0
    };

    // Draw text
    cr.set_source_rgba(r, g, b, a * alpha);
    cr.move_to(
        text_x,
        h as f64 / 2.0 - (ext.height() / 2.0 + ext.y_bearing()),
    );
    cr.show_text(text).unwrap();

    (w, h)
}
