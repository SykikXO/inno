use crate::config::{Animation, AppConfig, FormatTemplate, Signal};
use cairo::{Context, LinearGradient};
use std::f64::consts::PI;

const V_PADDING_TOP: f64 = 60.0; // Space for upward animations
const V_PADDING_BOTTOM: f64 = 60.0; // Space for downward animations

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
        Self { frame: 0, visible: true, alpha: 1.0, offset_x: 0.0, offset_y: 0.0 }
    }
}

impl DrawState {
    pub fn tick(&mut self, anim: &Animation, total_frames: f64, fps: f64) {
        self.frame = self.frame.wrapping_add(1);
        let t = self.frame as f64;

        match anim {
            Animation::Blink => {
                self.visible = (self.frame / 15).is_multiple_of(2);
                self.alpha = 1.0;
                self.offset_x = 0.0;
                self.offset_y = 0.0;
            }
            Animation::Pulse => {
                self.visible = true;
                self.alpha = 0.6 + 0.4 * (t * 0.15).sin().abs();
                self.offset_x = 0.0;
                self.offset_y = 0.0;
            }
            Animation::Fade => {
                self.visible = true;
                self.offset_x = 0.0;
                self.offset_y = 0.0;
                // Fade in/out each take 25% of total duration for a smooth, noticeable transition
                let fade_duration = (total_frames * 0.25).max(1.0);
                let fade_out_start = total_frames - fade_duration;

                if t < fade_duration {
                    self.alpha = (t / fade_duration).min(1.0); // Fade in
                } else if t >= fade_out_start {
                    self.alpha = ((total_frames - t) / fade_duration).clamp(0.0, 1.0); // Fade out
                } else {
                    self.alpha = 1.0; // Fully visible
                }
            }
            Animation::SlideRight => {
                self.visible = true;
                self.alpha = 1.0;
                // Slide in from right, ease out
                let progress = (t * 0.05).min(1.0);
                let eased = 1.0 - (1.0 - progress).powi(3);
                self.offset_x = -(1.0 - eased) * 200.0;
                self.offset_y = 0.0;
            }
            Animation::SlideLeft => {
                self.visible = true;
                self.alpha = 1.0;
                // Slide in from left, ease out
                let progress = (t * 0.05).min(1.0);
                let eased = 1.0 - (1.0 - progress).powi(3);
                self.offset_x = (1.0 - eased) * 200.0;
                self.offset_y = 0.0;
            }
            Animation::Bounce => {
                self.visible = true;
                self.alpha = 1.0;
                self.offset_x = 0.0;
                // Parabolic bounce with exponential decay
                let period = 0.5 * fps; // Snappy 0.5s period
                let local_t = (t % period) / period;
                let height = 4.0 * local_t * (1.0 - local_t); // Parabola: y = 4x(1-x)
                let bounce_num = (t / period).floor();
                // Decay factor is 1.0 (no decay) — bounce continues at full height.
                // Change to e.g. 0.8 if you want exponential decay over time.
                let decay = 1.0_f64.powf(bounce_num);
                self.offset_y = -height * 35.0 * decay;
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

/// Format notification text using pre-compiled template
pub fn format_text(fmt: &FormatTemplate, icon: &str, message: &str, percent: Option<f64>) -> String {
    fmt.render(icon, message, percent)
}

/// Measure text and icon dimensions without rendering
pub fn measure_text(text: &str, config: &AppConfig, signal: Option<&Signal>, scale: f64) -> (i32, i32) {
    let dummy = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1).unwrap();
    let cr = cairo::Context::new(&dummy).unwrap();

    cr.select_font_face(&config.font, config.font_slant, config.font_weight);

    let mut icon_w = 0.0;
    if let Some(s) = signal
        && !s.icon.is_empty()
    {
        let icon_ext = measure_icon(&cr, &s.icon, s.icon_size * scale);
        icon_w = icon_ext.x_advance() + 10.0 * scale;
    }

    cr.set_font_size(config.font_size * scale);
    let ext = cr.text_extents(text).unwrap();

    let w = (ext.width().ceil() + 20.0 * scale + icon_w).ceil() as i32;
    let h_content = ext.height().ceil() + 20.0 * scale;
    let h = (h_content + V_PADDING_TOP * scale + V_PADDING_BOTTOM * scale) as i32;

    (w, h)
}

pub fn draw_with_signal(
    cr: &Context,
    text: &str,
    config: &AppConfig,
    signal: Option<&Signal>,
    state: &DrawState,
    scale: f64,
) -> (i32, i32) {
    let (r_bg, g_bg, b_bg, a_bg) = config.bg_color;
    let (r, g, b, a) = signal.map(|s| s.color).unwrap_or(config.text_color);

    if signal.is_some_and(|s| s.animation == Animation::Blink && !state.visible) {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        cr.set_operator(cairo::Operator::Source);
        cr.paint().unwrap();
        return (1, 1);
    }

    let alpha = state.alpha;

    cr.select_font_face(&config.font, config.font_slant, config.font_weight);

    let mut icon_w = 0.0;
    if let Some(s) = signal
        && !s.icon.is_empty()
    {
        let icon_ext = measure_icon(cr, &s.icon, s.icon_size * scale);
        icon_w = icon_ext.x_advance() + 10.0 * scale;
    }

    cr.set_font_size(config.font_size * scale);
    let ext = cr.text_extents(text).unwrap();

    let w = (ext.width().ceil() + 20.0 * scale + icon_w).ceil() as i32;
    let h_content = ext.height().ceil() + 20.0 * scale;
    let h = (h_content + V_PADDING_TOP * scale + V_PADDING_BOTTOM * scale) as i32;

    cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    cr.set_operator(cairo::Operator::Source);
    cr.paint().unwrap();

    cr.translate(state.offset_x * scale, state.offset_y * scale + V_PADDING_TOP * scale);

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
        rounded_rect(cr, 0.0, 0.0, w as f64, h_content, config.border_radius * scale);
        cr.fill().unwrap();
    } else {
        cr.rectangle(0.0, 0.0, w as f64, h_content);
        cr.fill().unwrap();
    }

    let text_x = if let Some(s) = signal {
        if !s.icon.is_empty() {
            let icon_ext = measure_icon(cr, &s.icon, s.icon_size * scale);
            cr.set_source_rgba(r, g, b, a * alpha);
            cr.move_to(
                10.0 * scale - icon_ext.x_bearing(),
                h_content / 2.0 - (icon_ext.height() / 2.0 + icon_ext.y_bearing()),
            );
            cr.show_text(&s.icon).unwrap();
            cr.set_font_size(config.font_size * scale);
            10.0 * scale + icon_w
        } else {
            10.0 * scale
        }
    } else {
        10.0 * scale
    };

    cr.set_source_rgba(r, g, b, a * alpha);
    cr.move_to(text_x, h_content / 2.0 - (ext.height() / 2.0 + ext.y_bearing()));
    cr.show_text(text).unwrap();

    (w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use crate::config::FormatTemplate;

    #[test]
    fn test_format_text_with_percent() {
        let tmpl = FormatTemplate::parse("{icon} {message} {percent}%");
        let result = format_text(&tmpl, "BAT", "Battery", Some(75.0));
        assert_eq!(result, "BAT Battery 75%");
    }

    #[test]
    fn test_format_text_without_percent() {
        let tmpl = FormatTemplate::parse("{icon} {message} {percent}%");
        let result = format_text(&tmpl, "NET", "Connected", None);
        assert_eq!(result, "NET Connected");
    }

    #[test]
    fn test_format_text_percent_placeholder_only() {
        let tmpl = FormatTemplate::parse("{message} {percent}%");
        let result = format_text(&tmpl, "Test", "Hello", None);
        assert_eq!(result, "Hello");
    }

    #[test]
    fn test_format_text_no_percent_placeholder() {
        let tmpl = FormatTemplate::parse("{message}");
        let result = format_text(&tmpl, "X", "Hello", Some(50.0));
        assert_eq!(result, "Hello");
    }

    #[test]
    fn test_draw_state_reset() {
        let mut state = DrawState {
            frame: 100,
            visible: false,
            alpha: 0.5,
            offset_x: 50.0,
            offset_y: -30.0,
        };
        state.reset();
        assert_eq!(state.frame, 0);
        assert!(state.visible);
        assert!((state.alpha - 1.0).abs() < f64::EPSILON);
        assert!((state.offset_x - 0.0).abs() < f64::EPSILON);
        assert!((state.offset_y - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_blink_animation() {
        let mut state = DrawState::default();
        state.tick(&config::Animation::Blink, 300.0, 60.0);
        assert!(state.visible);
        assert!((state.alpha - 1.0).abs() < f64::EPSILON);
        assert!((state.offset_x - 0.0).abs() < f64::EPSILON);
        assert!((state.offset_y - 0.0).abs() < f64::EPSILON);

        for _ in 0..15 {
            state.tick(&config::Animation::Blink, 300.0, 60.0);
        }
        assert!(!state.visible);
    }

    #[test]
    fn test_pulse_animation() {
        let mut state = DrawState::default();
        for _ in 0..10 {
            state.tick(&config::Animation::Pulse, 300.0, 60.0);
        }
        assert!(state.visible);
        assert!(state.alpha >= 0.6 && state.alpha <= 1.0);
        assert!((state.offset_x - 0.0).abs() < f64::EPSILON);
        assert!((state.offset_y - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_fade_animation() {
        let mut state = DrawState::default();
        let total_frames = 180.0;
        let fps = 60.0;

        state.tick(&config::Animation::Fade, total_frames, fps);
        assert!((state.alpha - (1.0 / (total_frames * 0.25))).abs() < 0.01);

        for _ in 0..50 {
            state.tick(&config::Animation::Fade, total_frames, fps);
        }
        assert!((state.alpha - 1.0).abs() < 0.01);

        for _ in 0..130 {
            state.tick(&config::Animation::Fade, total_frames, fps);
        }
        assert!(state.alpha < 0.5);
    }

    #[test]
    fn test_slide_right_animation() {
        let mut state = DrawState::default();
        state.tick(&config::Animation::SlideRight, 300.0, 60.0);
        assert!(state.visible);
        assert!((state.alpha - 1.0).abs() < f64::EPSILON);
        assert!(state.offset_x < 0.0);
        assert!((state.offset_y - 0.0).abs() < f64::EPSILON);

        for _ in 0..50 {
            state.tick(&config::Animation::SlideRight, 300.0, 60.0);
        }
        assert!((state.offset_x - 0.0).abs() < 1.0);
    }

    #[test]
    fn test_slide_left_animation() {
        let mut state = DrawState::default();
        state.tick(&config::Animation::SlideLeft, 300.0, 60.0);
        assert!(state.visible);
        assert!(state.offset_x > 0.0);

        for _ in 0..50 {
            state.tick(&config::Animation::SlideLeft, 300.0, 60.0);
        }
        assert!((state.offset_x - 0.0).abs() < 1.0);
    }

    #[test]
    fn test_bounce_animation() {
        let mut state = DrawState::default();
        state.tick(&config::Animation::Bounce, 300.0, 60.0);
        assert!(state.visible);
        assert!((state.alpha - 1.0).abs() < f64::EPSILON);
        assert!((state.offset_x - 0.0).abs() < f64::EPSILON);
        assert!(state.offset_y <= 0.0);
    }

    #[test]
    fn test_none_animation() {
        let mut state = DrawState {
            frame: 50,
            visible: false,
            alpha: 0.3,
            offset_x: 100.0,
            offset_y: -50.0,
        };
        state.tick(&config::Animation::None, 300.0, 60.0);
        assert!(state.visible);
        assert!((state.alpha - 1.0).abs() < f64::EPSILON);
        assert!((state.offset_x - 0.0).abs() < f64::EPSILON);
        assert!((state.offset_y - 0.0).abs() < f64::EPSILON);
    }
}
