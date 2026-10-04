use crate::config::{Animation, AppConfig, Signal};
use cairo::{Context, LinearGradient};
use std::f64::consts::PI;

/// Total space a transition has to move through, split evenly above and below
/// the card. A bounce travels upward, so the top half is the one in use.
pub const V_PADDING: f64 = 120.0;

/// A rectangle in surface-local coordinates, which is the unit `wl_region`
/// rectangles and pointer event positions are both expressed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    /// Half-open on the far edges, matching the rectangle `wl_region.add` draws.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x as f64
            && x < (self.x + self.w) as f64
            && y >= self.y as f64
            && y < (self.y + self.h) as f64
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }
}

/// Where the visible card sits inside a `w`x`h` notification surface.
///
/// The surface is taller than the card by `V_PADDING` so a transition has room
/// to move without the compositor clipping it. That is also why the compositor
/// positions the surface rather than the card, and why a margin set on the
/// surface does not line up with the card. Anything that needs to know where
/// the card actually is, such as the input region, has to account for the split.
pub fn card_rect(w: i32, h: i32, scale: f64) -> Rect {
    let pad = (V_PADDING / 2.0 * scale).round() as i32;
    Rect { x: 0, y: pad, w, h: (h - pad * 2).max(0) }
}

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

/// How far a slide animation still has to travel at frame `t`, in pixels.
fn slide_offset(t: f64) -> f64 {
    // Ease out: fast start, slow settle.
    let eased = 1.0 - (1.0 - (t * 0.05).min(1.0)).powi(3);
    (1.0 - eased) * 200.0
}

impl DrawState {
    pub fn tick(&mut self, anim: &Animation, total_frames: f64, fps: f64) {
        self.frame = self.frame.wrapping_add(1);
        let t = self.frame as f64;
        let (mut visible, mut alpha) = (true, 1.0);
        let (mut offset_x, mut offset_y) = (0.0, 0.0);

        match anim {
            Animation::Blink => visible = (self.frame / 15).is_multiple_of(2),
            Animation::Pulse => alpha = 0.6 + 0.4 * (t * 0.15).sin().abs(),
            Animation::Fade => {
                // Fade in/out each take 25% of total duration for a smooth, noticeable transition
                let fade_duration = (total_frames * 0.25).max(1.0);
                let fade_out_start = total_frames - fade_duration;
                alpha = if t < fade_duration {
                    (t / fade_duration).min(1.0) // Fade in
                } else if t >= fade_out_start {
                    ((total_frames - t) / fade_duration).clamp(0.0, 1.0) // Fade out
                } else {
                    1.0 // Fully visible
                };
            }
            Animation::SlideRight => offset_x = -slide_offset(t),
            Animation::SlideLeft => offset_x = slide_offset(t),
            Animation::Bounce => {
                // Snappy 0.5s period
                let period = 0.5 * fps;
                let local_t = (t % period) / period;
                offset_y = -4.0 * local_t * (1.0 - local_t) * 35.0; // Parabola: y = 4x(1-x)
            }
            Animation::None => {}
        }
        (self.visible, self.alpha, self.offset_x, self.offset_y) = (visible, alpha, offset_x, offset_y);
    }

    pub fn reset(&mut self) {
        *self = Self::default();
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

/// Everything both the measuring pass and the drawing pass need from cairo.
struct Layout {
    w: i32,
    /// Height of the card proper, excluding the space reserved for a transition.
    h_content: f64,
    h: i32,
    /// Width the icon occupies, its advance plus the gap after it.
    icon_w: f64,
    /// Extents of the signal's icon at the icon size, if it has one.
    icon_ext: Option<cairo::TextExtents>,
    /// Extents of the body text at the configured font size.
    text_ext: cairo::TextExtents,
    /// The action buttons, in surface-local coordinates so hit testing can use
    /// them without knowing anything about how the card is padded.
    buttons: Vec<Rect>,
}

/// Height of the action row, padding included. Zero when there are no actions,
/// which keeps a signal without buttons laid out exactly as it was before
/// buttons existed.
const BUTTON_ROW_H: f64 = 26.0;
/// Space between one button and the next, and around the row.
const BUTTON_GAP: f64 = 8.0;
/// Horizontal padding inside a button, either side of its label.
const BUTTON_PAD: f64 = 12.0;

/// Selects the font, measures the icon and the body text, and returns the card
/// size. The measuring pass and the drawing pass both need this, and they were
/// two copies of it that could disagree about how wide the card is.
fn layout(cr: &Context, text: &str, config: &AppConfig, signal: Option<&Signal>, scale: f64) -> Layout {
    cr.select_font_face(&config.font, config.font_slant, config.font_weight);

    let icon_ext = signal
        .filter(|s| !s.icon.is_empty())
        .map(|s| measure_icon(cr, &s.icon, s.icon_size * scale));
    let icon_w = icon_ext.as_ref().map_or(0.0, |e| e.x_advance() + 10.0 * scale);

    cr.set_font_size(config.font_size * scale);
    let ext = cr.text_extents(text).unwrap();

    let text_h = ext.height().ceil() + 20.0 * scale;
    let text_w = (ext.width().ceil() + 20.0 * scale + icon_w).ceil() as i32;

    // Buttons are laid out right to left on the bottom row, the way a native
    // dialog does it, and the card grows to whatever they need. Measuring them
    // here rather than at draw time is what keeps the measuring and drawing
    // passes from disagreeing about how big the card is.
    let actions = signal.map(|s| s.actions.as_slice()).unwrap_or(&[]);
    let row_h = if actions.is_empty() { 0.0 } else { BUTTON_ROW_H * scale };
    let mut buttons = Vec::with_capacity(actions.len());
    let mut row_w = 0.0;
    if !actions.is_empty() {
        cr.set_font_size(config.font_size * 0.85 * scale);
        for a in actions.iter().rev() {
            let e = cr.text_extents(&a.label).unwrap();
            let bw = (e.width() + BUTTON_PAD * 2.0 * scale).ceil();
            row_w += bw;
            buttons.push(Rect { x: 0, y: 0, w: bw as i32, h: row_h as i32 });
        }
        row_w += BUTTON_GAP * scale * (actions.len() - 1) as f64;
    }

    let w = text_w.max(row_w as i32 + 20);
    // Right-aligned, with the same 10px inset the text uses. Measured from the
    // card's width, not the text's: a short message with several buttons makes
    // the row wider than the text, and insetting from the text width walks the
    // leftmost button off the side of its own card and off the surface.
    let mut x = w as f64 - 10.0 * scale - row_w;
    for i in (0..actions.len()).rev() {
        let b = buttons[i];
        // Card-local, then shifted down by the half-padding above the card to get
        // surface-local, which is the space pointer events are reported in.
        buttons[i] = Rect {
            x: x as i32,
            // The row occupies the bottom row_h of the card, so its top is
            // text_h, not the middle of the row. Adding half the height here
            // pushed every button down by half its own height, leaving the hit
            // test a strip lower than the pixels that were drawn.
            y: (text_h + V_PADDING / 2.0 * scale) as i32,
            w: b.w,
            h: b.h,
        };
        x += b.w as f64 + BUTTON_GAP * scale;
    }

    let h_content = text_h + row_h;
    Layout {
        w,
        h_content,
        h: (h_content + V_PADDING * scale) as i32,
        icon_w,
        icon_ext,
        text_ext: ext,
        buttons,
    }
}

/// Measure text and icon dimensions without rendering
pub fn measure_text(text: &str, config: &AppConfig, signal: Option<&Signal>, scale: f64) -> (i32, i32) {
    let dummy = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1).unwrap();
    let cr = cairo::Context::new(&dummy).unwrap();
    let l = layout(&cr, text, config, signal, scale);
    (l.w, l.h)
}

pub fn draw_with_signal(
    cr: &Context,
    text: &str,
    config: &AppConfig,
    signal: Option<&Signal>,
    state: &DrawState,
    scale: f64,
    hovered_action: Option<usize>,
) -> (i32, i32, Vec<Rect>) {
    let (r_bg, g_bg, b_bg, a_bg) = config.bg_color;
    let (r, g, b, a) = signal.map(|s| s.color).unwrap_or(config.text_color);

    if signal.is_some_and(|s| s.animation == Animation::Blink && !state.visible) {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        cr.set_operator(cairo::Operator::Source);
        cr.paint().unwrap();
        return (1, 1, Vec::new());
    }

    let alpha = state.alpha;

    let Layout { w, h, h_content, icon_w, icon_ext, text_ext: ext, buttons } =
        layout(cr, text, config, signal, scale);
    // The text centres in the part of the card above the button row rather than
    // in the whole card, or adding a button would visibly shift the message.
    let text_h = h_content - BUTTON_ROW_H * scale * if buttons.is_empty() { 0.0 } else { 1.0 };

    cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    cr.set_operator(cairo::Operator::Source);
    cr.paint().unwrap();

    cr.translate(state.offset_x * scale, state.offset_y * scale + V_PADDING / 2.0 * scale);

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

    let text_x = match (signal, icon_ext) {
        (Some(s), Some(icon_ext)) => {
            cr.set_source_rgba(r, g, b, a * alpha);
            cr.set_font_size(s.icon_size * scale);
            cr.move_to(
                10.0 * scale - icon_ext.x_bearing(),
                text_h / 2.0 - (icon_ext.height() / 2.0 + icon_ext.y_bearing()),
            );
            cr.show_text(&s.icon).unwrap();
            cr.set_font_size(config.font_size * scale);
            10.0 * scale + icon_w
        }
        _ => 10.0 * scale,
    };

    cr.set_source_rgba(r, g, b, a * alpha);
    cr.move_to(text_x, text_h / 2.0 - (ext.height() / 2.0 + ext.y_bearing()));
    cr.show_text(text).unwrap();

    if !buttons.is_empty() {
        let actions = signal.map(|s| s.actions.as_slice()).unwrap_or(&[]);
        let radius = config.border_radius * scale;
        for (i, (btn, act)) in buttons.iter().zip(actions).enumerate() {
            // The card was translated down by half the padding when it started
            // drawing, and the button rects came back out in surface-local, so
            // undo the shift to get back to card-local for drawing.
            let bx = btn.x as f64;
            let by = btn.y as f64 - V_PADDING / 2.0 * scale;
            let bw = btn.w as f64;
            let bh = btn.h as f64;

            // The first action is the one a person is expected to take, so it
            // gets the accent fill and the rest stay quiet until hovered.
            let primary = i == 0;
            let hovered = hovered_action == Some(i);
            let fill = if hovered || primary { 1.0 } else { 0.14 };
            cr.set_source_rgba(r, g, b, a * alpha * fill);
            if radius > 0.0 {
                rounded_rect(cr, bx, by, bw, bh, radius);
            } else {
                cr.rectangle(bx, by, bw, bh);
            }
            cr.fill().unwrap();

            let fg = if hovered || primary {
                (r_bg, g_bg, b_bg, a_bg)
            } else {
                (r, g, b, a)
            };
            cr.set_source_rgba(fg.0, fg.1, fg.2, fg.3 * alpha);
            cr.set_font_size(config.font_size * 0.85 * scale);
            let le = cr.text_extents(&act.label).unwrap();
            cr.move_to(
                bx + (bw - le.width()) / 2.0 - le.x_bearing(),
                by + bh / 2.0 - (le.height() / 2.0 + le.y_bearing()),
            );
            cr.show_text(&act.label).unwrap();
        }
        cr.set_font_size(config.font_size * scale);
    }

    (w, h, buttons)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    #[test]
    fn a_rect_contains_its_own_edges_but_not_the_far_ones() {
        let r = Rect { x: 10, y: 20, w: 100, h: 50 };
        assert!(r.contains(10.0, 20.0), "top-left corner is inside");
        assert!(r.contains(109.9, 69.9), "just inside the far corner");
        assert!(!r.contains(110.0, 20.0), "the far x edge is outside");
        assert!(!r.contains(10.0, 70.0), "the far y edge is outside");
        assert!(!r.contains(9.0, 20.0), "just before the near edge");
    }

    #[test]
    fn a_zero_sized_rect_holds_nothing() {
        assert!(Rect { x: 0, y: 0, w: 0, h: 40 }.is_empty());
        assert!(Rect { x: 0, y: 0, w: 40, h: 0 }.is_empty());
        assert!(!Rect { x: 0, y: 0, w: 1, h: 1 }.is_empty());
    }

    #[test]
    fn the_card_is_the_surface_minus_the_transition_padding() {
        // 120px of padding split evenly, so the card starts 60px down and the
        // surface is 120px taller than the card.
        let r = card_rect(200, 220, 1.0);
        assert_eq!(r, Rect { x: 0, y: 60, w: 200, h: 100 });
    }

    #[test]
    fn the_card_sits_where_the_drawing_code_puts_it() {
        // The rect is derived from the surface size rather than from Layout, so
        // it can drift from where the card is really drawn. Measure a real
        // layout and check the two agree.
        let cr = cairo::Context::new(
            cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1).unwrap(),
        )
        .unwrap();
        let config = config::AppConfig::default();
        for scale in [0.5, 0.8, 1.0, 1.5, 2.0] {
            let l = layout(&cr, "plug your pc in", &config, None, scale);
            let r = card_rect(l.w, l.h, scale);
            assert!(
                r.contains(0.0, V_PADDING / 2.0 * scale + l.h_content / 2.0),
                "the middle of the drawn card is inside the rect at scale {scale}"
            );
            assert!(
                !r.contains(1.0, 1.0),
                "the transparent padding above the card is outside the rect at scale {scale}"
            );
            assert!(
                !r.contains(1.0, (l.h as f64) - 1.0),
                "the transparent padding below the card is outside the rect at scale {scale}"
            );
        }
    }

    #[test]
    fn a_surface_taller_than_its_padding_still_yields_a_card() {
        // Degenerate sizing must not underflow the height into a huge rect
        // that would make the whole surface clickable again.
        let r = card_rect(10, 4, 1.0);
        assert_eq!(r.h, 0);
        assert!(r.is_empty());
    }

    fn with_actions(labels: &[&str]) -> config::Signal {
        config::Signal {
            actions: labels
                .iter()
                .map(|l| config::Action { label: (*l).into(), command: "true".into() })
                .collect(),
            ..test_signal()
        }
    }

    fn layout_for(signal: Option<&config::Signal>, scale: f64) -> Layout {
        let cr = cairo::Context::new(
            cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1).unwrap(),
        )
        .unwrap();
        layout(&cr, "BATTERY LOW", &test_config(), signal, scale)
    }

    #[test]
    fn a_signal_without_actions_gets_no_buttons_and_the_same_size() {
        let bare = layout_for(None, 1.0);
        let with_none = layout_for(Some(&test_signal()), 1.0);
        assert!(bare.buttons.is_empty());
        assert!(with_none.buttons.is_empty());
        assert_eq!((bare.w, bare.h_content), (with_none.w, with_none.h_content));
    }

    #[test]
    fn buttons_stay_inside_the_card_even_when_the_row_is_wider_than_the_text() {
        // A short message with several buttons makes the row wider than the text.
        // Insetting the row from the text width instead of the card width walks
        // the leftmost button off the side of the card, which is how a button
        // ended up at x = -78 and became impossible to click.
        let l = layout_for(Some(&with_actions(&["Plug in", "Snooze", "Ignore"])), 1.0);
        assert_eq!(l.buttons.len(), 3);
        for b in &l.buttons {
            assert!(b.x >= 0, "button starts inside the card, got x = {}", b.x);
            assert!(
                b.x + b.w <= l.w,
                "button ends inside the card, got {} > {}",
                b.x + b.w,
                l.w
            );
        }
    }

    #[test]
    fn buttons_do_not_overlap_and_keep_their_order() {
        let l = layout_for(Some(&with_actions(&["Plug in", "Snooze", "Ignore"])), 1.0);
        // The array is in declaration order and the row is right-aligned, so the
        // first declared button is the rightmost one. That ordering is what lets
        // the hit test report an index that means the same action it did in the
        // config.
        for pair in l.buttons.windows(2) {
            assert!(
                pair[1].x + pair[1].w <= pair[0].x,
                "each button sits left of the one before it, with a gap"
            );
        }
        assert_eq!(l.buttons.len(), 3);
        assert!(l.buttons.iter().all(|b| !b.is_empty()));
        // Right-aligned: the first declared one ends 10px from the card edge.
        let right_gap = l.w - (l.buttons[0].x + l.buttons[0].w);
        assert!((right_gap - 10).abs() <= 1, "right inset, got {right_gap}");
    }

    #[test]
    fn buttons_grow_the_card_downward_by_exactly_one_row() {
        let bare = layout_for(None, 1.0);
        let with = layout_for(Some(&with_actions(&["Plug in"])), 1.0);
        let row = (BUTTON_ROW_H * 1.0) as i64;
        assert_eq!(with.h_content as i64 - bare.h_content as i64, row);
        // And they sit below the text, inside the card.
        let b = with.buttons[0];
        assert!(b.y as f64 >= V_PADDING / 2.0, "below the transition padding");
        assert!(
            (b.y + b.h) as f64 <= V_PADDING / 2.0 + with.h_content + 1.0,
            "button row ends inside the card"
        );
    }

    #[test]
    fn button_rects_scale_with_the_notification() {
        let one = layout_for(Some(&with_actions(&["Plug in"])), 1.0);
        let two = layout_for(Some(&with_actions(&["Plug in"])), 2.0);
        assert_eq!(two.buttons[0].h as f64, one.buttons[0].h as f64 * 2.0);
        assert!(two.buttons[0].w > one.buttons[0].w);
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

    #[test]
    fn test_bounce_first_frame_has_offset() {
        // Regression: bounce_num=0 must NOT zero out offset_y
        let mut state = DrawState::default();
        state.tick(&config::Animation::Bounce, 300.0, 60.0);
        // Frame 1 is early in the first period, should have nonzero bounce
        assert!(state.offset_y < 0.0, "First bounce frame should have negative offset, got {}", state.offset_y);
    }

    #[test]
    fn test_bounce_constant_height() {
        // Bounce uses decay=1.0 (no decay), so all periods should have the same peak height
        let fps = 60.0;
        let period = (0.5 * fps) as u32; // 30 frames per period

        // Scan for peak in the first bounce period
        let mut state = DrawState::default();
        let mut first_peak = 0.0_f64;
        for _ in 0..period {
            state.tick(&config::Animation::Bounce, 300.0, fps);
            if state.offset_y < first_peak {
                first_peak = state.offset_y;
            }
        }

        // Scan for peak in the third bounce period
        let mut third_peak = 0.0_f64;
        for _ in 0..period {
            state.tick(&config::Animation::Bounce, 300.0, fps);
        }
        for _ in 0..period {
            state.tick(&config::Animation::Bounce, 300.0, fps);
            if state.offset_y < third_peak {
                third_peak = state.offset_y;
            }
        }

        assert!(first_peak < 0.0, "Bounce should have negative peak, got {}", first_peak);
        assert!((first_peak - third_peak).abs() < 0.01,
            "Bounce height should be constant: first={}, third={}", first_peak, third_peak);
    }

    #[test]
    fn test_bounce_never_goes_positive() {
        // Bounce offset_y should always be <= 0 (upward)
        let mut state = DrawState::default();
        for _ in 0..500 {
            state.tick(&config::Animation::Bounce, 300.0, 60.0);
            assert!(state.offset_y <= 0.0,
                "Bounce offset_y should never be positive, got {} at frame {}", state.offset_y, state.frame);
        }
    }

    #[test]
    fn test_fade_alpha_never_exceeds_bounds() {
        let mut state = DrawState::default();
        let total_frames = 180.0;
        let fps = 60.0;
        for _ in 0..250 {
            state.tick(&config::Animation::Fade, total_frames, fps);
            assert!(state.alpha >= 0.0 && state.alpha <= 1.0,
                "Fade alpha out of bounds: {} at frame {}", state.alpha, state.frame);
        }
    }

    #[test]
    fn test_fade_fully_visible_in_middle() {
        let mut state = DrawState::default();
        let total_frames = 120.0;
        // Advance to middle (past 25% fade-in, before 75% fade-out start)
        for _ in 0..60 {
            state.tick(&config::Animation::Fade, total_frames, 60.0);
        }
        assert!((state.alpha - 1.0).abs() < f64::EPSILON,
            "Fade should be fully visible at midpoint, got {}", state.alpha);
    }

    #[test]
    fn test_pulse_stays_bounded_long_run() {
        let mut state = DrawState::default();
        for _ in 0..1000 {
            state.tick(&config::Animation::Pulse, 300.0, 60.0);
            assert!(state.alpha >= 0.6 && state.alpha <= 1.0,
                "Pulse alpha out of range: {} at frame {}", state.alpha, state.frame);
            assert!(state.visible);
        }
    }

    #[test]
    fn test_slide_right_converges_to_zero() {
        let mut state = DrawState::default();
        // After 20 frames at 0.05 progress/frame, progress = 1.0
        for _ in 0..25 {
            state.tick(&config::Animation::SlideRight, 300.0, 60.0);
        }
        assert!((state.offset_x - 0.0).abs() < f64::EPSILON,
            "SlideRight should converge to 0, got {}", state.offset_x);
    }

    #[test]
    fn test_slide_left_converges_to_zero() {
        let mut state = DrawState::default();
        for _ in 0..25 {
            state.tick(&config::Animation::SlideLeft, 300.0, 60.0);
        }
        assert!((state.offset_x - 0.0).abs() < f64::EPSILON,
            "SlideLeft should converge to 0, got {}", state.offset_x);
    }

    #[test]
    fn test_draw_state_default() {
        let state = DrawState::default();
        assert_eq!(state.frame, 0);
        assert!(state.visible);
        assert!((state.alpha - 1.0).abs() < f64::EPSILON);
        assert!((state.offset_x).abs() < f64::EPSILON);
        assert!((state.offset_y).abs() < f64::EPSILON);
    }

    // --- measurement and rendering --------------------------------------
    //
    // Absolute pixel sizes depend on the host fontconfig, so these assert
    // relational invariants rather than exact numbers.

    fn test_config() -> config::AppConfig {
        config::AppConfig {
            font: "monospace".to_string(),
            font_size: 18.0,
            font_slant: cairo::FontSlant::Normal,
            font_weight: cairo::FontWeight::Normal,
            border_radius: 8.0,
            gradient: false,
            bg_color: (0.0, 0.0, 0.0, 0.6),
            text_color: (1.0, 1.0, 1.0, 1.0),
            ..Default::default()
        }
    }

    fn test_signal() -> config::Signal {
        config::Signal {
            message: "test".into(),
            icon: String::new(),
            icon_size: 24.0,
            color: (1.0, 1.0, 1.0, 1.0),
            color_name: "white".into(),
            threshold: 0.0,
            state_filter: "any".into(),
            animation: config::Animation::None,
            animation_ref: None,
            duration: Some(5),
            sound: None,
            actions: Vec::new(),
        }
    }

    #[test]
    fn test_measure_text_wider_text_is_wider() {
        let cfg = test_config();
        let short = measure_text("hi", &cfg, None, 1.0);
        let long = measure_text("a considerably longer message", &cfg, None, 1.0);
        assert!(long.0 > short.0, "{:?} should exceed {:?}", long, short);
    }

    #[test]
    fn test_measure_text_height_is_independent_of_width() {
        // Only for the same glyph class: text_extents height is the inked height,
        // so descenders do change it.
        let cfg = test_config();
        let short = measure_text("hhhhh", &cfg, None, 1.0);
        let long = measure_text("hhhhhhhhhhhhhhhhhhhh", &cfg, None, 1.0);
        assert!(long.0 > short.0, "width should grow with text");
        assert_eq!(long.1, short.1, "height should not grow with width");
    }

    #[test]
    fn test_measure_text_grows_with_scale() {
        let cfg = test_config();
        let small = measure_text("hello", &cfg, None, 1.0);
        let large = measure_text("hello", &cfg, None, 2.0);
        assert!(large.0 > small.0 && large.1 > small.1);
        // Doubling the scale should roughly double each dimension.
        let ratio_w = large.0 as f64 / small.0 as f64;
        assert!((1.9..2.1).contains(&ratio_w), "width ratio was {}", ratio_w);
    }

    #[test]
    fn test_measure_text_icon_adds_width() {
        let cfg = test_config();
        let without = measure_text("hi", &cfg, None, 1.0);
        let mut signal = test_signal();
        signal.icon = "\u{f000}".to_string();
        let with = measure_text("hi", &cfg, Some(&signal), 1.0);
        assert!(with.0 > without.0, "icon should widen the surface");
        assert_eq!(with.1, without.1, "icon should not change height");
    }

    #[test]
    fn test_measure_text_never_returns_degenerate_size() {
        // layer.rs treats w <= 1 or h <= 1 as "nothing to render" and hides the
        // surface, so this must never happen for real text.
        let cfg = test_config();
        let (w, h) = measure_text("", &cfg, None, 1.0);
        assert!(w > 1 && h > 1, "empty text gave {}x{}", w, h);
    }

    #[test]
    fn test_draw_with_signal_reports_its_dimensions() {
        let cfg = test_config();
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 800, 300).unwrap();
        let (w, h, _) = {
            let cr = cairo::Context::new(&surface).unwrap();
            draw_with_signal(&cr, "hello", &cfg, Some(&test_signal()), &DrawState::default(), 1.0, None)
        };
        assert_eq!((w, h), measure_text("hello", &cfg, Some(&test_signal()), 1.0));
    }

    #[test]
    fn test_blink_while_invisible_reports_a_one_pixel_surface() {
        // layer.rs keys off this to commit a transparent buffer rather than
        // redrawing the card.
        let cfg = test_config();
        let mut signal = test_signal();
        signal.animation = config::Animation::Blink;
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 800, 300).unwrap();
        let cr = cairo::Context::new(&surface).unwrap();
        let state = DrawState { visible: false, ..Default::default() };

        let (w, h, _) = draw_with_signal(&cr, "hello", &cfg, Some(&signal), &state, 1.0, None);
        assert_eq!((w, h), (1, 1));
    }

    #[test]
    fn test_draw_with_signal_paints_the_card() {
        let mut cfg = test_config();
        cfg.border_radius = 0.0;
        let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 600, 300).unwrap();

        // Empty text so the sample lands on background rather than a glyph.
        let (w, h, _) = {
            let cr = cairo::Context::new(&surface).unwrap();
            draw_with_signal(&cr, "", &cfg, Some(&test_signal()), &DrawState::default(), 1.0, None)
        };
        surface.flush();

        // The card is only as large as the text, so sample using the returned
        // dimensions rather than a fixed point.
        let stride = surface.stride() as usize;
        let alpha = {
            let data = surface.data().unwrap();
            let y = (h / 2) as usize;
            let x = (w / 2) as usize;
            data[y * stride + x * 4 + 3]
        };
        assert!(alpha > 0, "card interior was not painted, alpha={}", alpha);
        assert!(
            (alpha as f64 - cfg.bg_color.3 * 255.0).abs() < 2.0,
            "expected the bg alpha {}, got {}",
            cfg.bg_color.3 * 255.0,
            alpha
        );
    }

    #[test]
    fn test_draw_with_signal_leaves_outside_the_card_clear() {
        let cfg = test_config();
        let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 600, 300).unwrap();
        let (w, _, _) = {
            let cr = cairo::Context::new(&surface).unwrap();
            draw_with_signal(&cr, "hello", &cfg, Some(&test_signal()), &DrawState::default(), 1.0, None)
        };
        surface.flush();

        let stride = surface.stride() as usize;
        let alpha = {
            let data = surface.data().unwrap();
            data[290 * stride + 590 * 4 + 3]
        };
        assert_eq!(alpha, 0, "pixels beyond the card should stay clear");
        assert!(w < 600, "the card should not span the whole surface");
    }

    #[test]
    fn test_rounded_rect_leaves_corners_transparent() {
        let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 100, 100).unwrap();
        {
            let cr = cairo::Context::new(&surface).unwrap();
            cr.set_operator(cairo::Operator::Source);
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
            cr.paint().unwrap();
            cr.set_source_rgba(1.0, 0.0, 0.0, 1.0);
            rounded_rect(&cr, 0.0, 0.0, 100.0, 100.0, 20.0);
            cr.fill().unwrap();
        }
        surface.flush();

        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        let alpha_at = |x: usize, y: usize| data[y * stride + x * 4 + 3];
        // A 20px radius means the very corner stays clear and the centre is filled.
        assert_eq!(alpha_at(1, 1), 0, "corner should be transparent");
        assert_eq!(alpha_at(50, 50), 255, "centre should be filled");
    }

    #[test]
    fn test_rounded_rect_without_radius_fills_the_whole_area() {
        let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 100, 100).unwrap();
        {
            let cr = cairo::Context::new(&surface).unwrap();
            cr.set_operator(cairo::Operator::Source);
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
            cr.paint().unwrap();
            cr.set_source_rgba(1.0, 0.0, 0.0, 1.0);
            rounded_rect(&cr, 0.0, 0.0, 100.0, 100.0, 0.0);
            cr.fill().unwrap();
        }
        surface.flush();

        let data = surface.data().unwrap();
        assert_eq!(data[3], 255, "radius 0 should fill the top-left pixel");
    }
}

