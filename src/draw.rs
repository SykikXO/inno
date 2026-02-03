use cairo::{Context, FontSlant, FontWeight};
use crate::config::{AppConfig, Signal, Animation};

#[derive(Debug, Clone, Default)]
pub struct DrawState {
    pub frame: u32,
    pub visible: bool,
}

impl DrawState {
    pub fn tick(&mut self, anim: &Animation) {
        self.frame = self.frame.wrapping_add(1);
        self.visible = match anim {
            Animation::Flicker => (self.frame / 15) % 2 == 0,
            _ => true,
        };
    }
    pub fn reset(&mut self) { self.frame = 0; self.visible = true; }
}

fn draw_warning_icon(cr: &Context, x: f64, y: f64, sz: f64, (r, g, b, a): (f64, f64, f64, f64)) {
    let h = sz * 0.866;
    cr.set_source_rgba(r, g, b, a);
    cr.move_to(x + sz/2.0, y);
    cr.line_to(x + sz, y + h);
    cr.line_to(x, y + h);
    cr.close_path();
    cr.fill().unwrap();
    
    let c = if r + g + b > 1.5 { 0.0 } else { 1.0 };
    cr.set_source_rgba(c, c, c, a);
    cr.set_line_width(sz * 0.12);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.move_to(x + sz/2.0, y + h * 0.25);
    cr.line_to(x + sz/2.0, y + h * 0.6);
    cr.stroke().unwrap();
    cr.arc(x + sz/2.0, y + h * 0.75, sz * 0.06, 0.0, std::f64::consts::TAU);
    cr.fill().unwrap();
}

pub fn draw_with_signal(cr: &Context, text: &str, config: &AppConfig, signal: Option<&Signal>, state: &DrawState) -> (i32, i32) {
    let (r_bg, g_bg, b_bg, a_bg) = config.bg_color;
    let (r, g, b, a) = signal.map(|s| s.color).unwrap_or(config.text_color);
    
    // Flicker off
    if signal.is_some_and(|s| s.animation == Animation::Flicker && !state.visible) {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        cr.set_operator(cairo::Operator::Source);
        cr.paint().unwrap();
        return (1, 1);
    }
    
    // Pulse alpha
    let alpha = if signal.is_some_and(|s| s.animation == Animation::Pulse) {
        0.6 + 0.4 * (state.frame as f64 * 0.15).sin().abs()
    } else { 1.0 };

    cr.select_font_face(&config.font, FontSlant::Italic, FontWeight::Bold);
    cr.set_font_size(24.0);
    
    let ext = cr.text_extents(text).unwrap();
    let icon_w = if signal.is_some() { 32.0 } else { 0.0 };
    let w = ext.width().ceil() as i32 + 20 + icon_w as i32;
    let h = ext.height().ceil() as i32 + 20;

    // Clear + background
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    cr.set_operator(cairo::Operator::Source);
    cr.paint().unwrap();
    cr.set_source_rgba(r_bg, g_bg, b_bg, a_bg * alpha);
    cr.set_operator(cairo::Operator::Over);
    cr.rectangle(0.0, 0.0, w as f64, h as f64);
    cr.fill().unwrap();

    // Icon (always warning triangle for signals)
    let text_x = if signal.is_some() {
        draw_warning_icon(cr, 10.0, (h as f64 - 24.0) / 2.0, 24.0, (r, g, b, a * alpha));
        10.0 + icon_w
    } else { 10.0 };

    // Text
    cr.set_source_rgba(r, g, b, a * alpha);
    cr.move_to(text_x, h as f64 / 2.0 - (ext.height() / 2.0 + ext.y_bearing()));
    cr.show_text(text).unwrap();
    (w, h)
}
