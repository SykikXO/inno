use crate::animation::{AnimPlayer, TargetSize};
use crate::config::AppConfig;
use crate::config::{AnimAsset, DisplayMode, Signal, VAnchor};
use crate::draw;
use crate::draw::{DrawState, Rect};
use std::collections::HashMap;
use cairo::FontSlant;
use cairo::FontWeight;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry, delegate_seat,
    delegate_shm,
    output::{OutputHandler, OutputState},
    reexports::client::{
        Connection, QueueHandle,
        globals::registry_queue_init,
        protocol::{wl_output, wl_seat, wl_shm, wl_surface, wl_pointer},
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{Capability, SeatHandler, SeatState, pointer::PointerHandler},
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};

#[derive(Clone, PartialEq, Debug)]
struct RenderKey {
    text: String,
    signal_icon: String,
    signal_icon_size: f64,
    signal_color: (f64, f64, f64, f64),
    font: String,
    font_size: f64,
    font_slant: FontSlant,
    font_weight: FontWeight,
    bg_color: (f64, f64, f64, f64),
    text_color: (f64, f64, f64, f64),
    border_radius: f64,
    gradient: bool,
    scale: f64,
}

/// Sets all four margins from the anchor, scaled. The compositor honours the
/// margin of an anchored edge and ignores the margin of an axis with no anchor,
/// so an unanchored axis is simply centred.
///
/// A margin measures to the visible card, not to the edge of the surface around
/// it. The card is drawn inside a surface that reserves `draw::V_PADDING` of
/// room above and below for a transition to move through, and that padding is
/// an implementation detail of the animation rather than something the person
/// writing the config asked for. Compensating for it here is what makes
/// `margin = 0` sit flush against the screen edge, and it makes a negative
/// margin mean what it looks like: that much of the card hanging off the edge.
fn set_margins(layer: &LayerSurface, config: &AppConfig, s: f64) {
    let a = &config.anchor;
    let pad = draw::V_PADDING / 2.0;
    let top = a.margin_top as f64 - if a.v == VAnchor::Top { pad } else { 0.0 };
    let bottom = a.margin_bottom as f64 - if a.v == VAnchor::Bottom { pad } else { 0.0 };
    layer.set_margin(
        (top * s) as i32,
        (a.margin_right as f64 * s) as i32,
        (bottom * s) as i32,
        (a.margin_left as f64 * s) as i32,
    );
}

fn render_key(text: &str, config: &AppConfig, signal: Option<&Signal>, scale: f64) -> RenderKey {
    RenderKey {
        text: text.to_string(),
        signal_icon: signal.map(|s| s.icon.clone()).unwrap_or_default(),
        signal_icon_size: signal.map(|s| s.icon_size).unwrap_or(0.0),
        signal_color: signal.map(|s| s.color).unwrap_or(config.text_color),
        font: config.font.clone(),
        font_size: config.font_size,
        font_slant: config.font_slant,
        font_weight: config.font_weight,
        bg_color: config.bg_color,
        text_color: config.text_color,
        border_radius: config.border_radius,
        gradient: config.gradient,
        scale,
    }
}

/// Maximum logical pixels for the animation display area in text+anim mode.
const MAX_ANIM_DISPLAY_PX: f64 = 200.0;

/// Decode size for an animation, chosen from how it is displayed.
fn anim_target(asset: &AnimAsset, scale: f64) -> TargetSize {
    match asset.display {
        // The animation is the whole notification, so it is drawn at its own
        // size times the display scale.
        DisplayMode::Anim => TargetSize::Scaled(scale),
        // The animation shares the surface with text and fits a fixed box, so
        // the box size is all that needs decoding.
        DisplayMode::Text => TargetSize::LongestEdge(
            (MAX_ANIM_DISPLAY_PX * scale).round().max(1.0) as i32,
        ),
    }
}

/// A decoded animation frame. Owns the surface; cloning is a refcount bump.
pub struct AnimFrame {
    pub surface: cairo::ImageSurface,
    /// Decoded size.
    pub w: i32,
    pub h: i32,
    /// Natural size of the source frames.
    pub source_w: i32,
    pub source_h: i32,
}

/// Outcome of advancing a frame animation by one tick.
pub enum FrameTick {
    /// Drawn. The frame clock already runs at the animation's own rate.
    Continue,
    /// A non-looping animation reached its last frame. The surface already
    /// holds that frame, so stop ticking rather than re-committing it forever.
    /// Carries the animation's `on_complete` so the caller does not have to
    /// look the asset up again.
    Finished(crate::config::OnComplete),
    /// Nothing could be drawn.
    Unavailable,
}

pub struct FrameCache {
    surface: Option<cairo::ImageSurface>,
    key: Option<RenderKey>,
    width: i32,
    height: i32,
}

impl FrameCache {
    fn new() -> Self {
        Self { surface: None, key: None, width: 0, height: 0 }
    }

    pub fn clear(&mut self) {
        self.surface = None;
        self.key = None;
        self.width = 0;
        self.height = 0;
    }

    fn matches(&self, key: &RenderKey) -> bool {
        self.key.as_ref().is_some_and(|k| k == key)
    }
}

pub struct LayerApp {
    pub registry_state: RegistryState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub compositor_state: CompositorState,
    pub shm_state: Shm,
    pub layer_shell: LayerShell,

    pub width: u32,
    pub height: u32,
    pub layer_surface: Option<LayerSurface>,
    pub pool: Option<SlotPool>,
    pub exit: bool,
    pub configured: bool,
    pub scale_factor: i32,
    pub scale_changed: bool,
    pub pointer: Option<wl_pointer::WlPointer>,
    pub clicked: bool,
    pub frame_cache: FrameCache,
    pub anim_players: HashMap<String, AnimPlayer>,
    /// Animations that failed to load, so a broken `source` path is reported
    /// once instead of on every tick.
    pub failed_animations: std::collections::HashSet<String>,
    /// Set after a shm buffer allocation fails so the cause is reported once
    /// rather than leaving the notification silently frozen.
    pub alloc_failed: bool,
    /// True when the surface currently shows the cached text at full opacity and
    /// no offset, so a frame at the same transform would be a no-op.
    committed_identity: bool,
    /// Which parts of the surface may take pointer input, in surface-local
    /// coordinates. Empty means the surface is not clickable at all, which is
    /// the correct answer for a hidden notification.
    interactive: Vec<Rect>,
}

impl LayerApp {
    pub fn new(conn: &Connection, qh: &QueueHandle<Self>) -> anyhow::Result<Self> {
        let (globals, _) = registry_queue_init::<Self>(conn)?;
        let registry_state = RegistryState::new(&globals);
        let seat_state = SeatState::new(&globals, qh);
        let output_state = OutputState::new(&globals, qh);
        let compositor_state = CompositorState::bind(&globals, qh)?;
        let shm_state = Shm::bind(&globals, qh)?;
        let layer_shell = LayerShell::bind(&globals, qh)?;

        Ok(Self {
            registry_state,
            seat_state,
            output_state,
            compositor_state,
            shm_state,
            layer_shell,
            width: 0,
            height: 0,
            layer_surface: None,
            pool: None,
            exit: false,
            configured: false,
            scale_factor: 1,
            scale_changed: false,
            pointer: None,
            clicked: false,
            interactive: Vec::new(),
            frame_cache: FrameCache::new(),
            anim_players: HashMap::new(),
            failed_animations: std::collections::HashSet::new(),
            alloc_failed: false,
            committed_identity: false,
        })
    }

    pub fn create_surface(&mut self, qh: &QueueHandle<Self>, config: &AppConfig) {
        use crate::config::{HAnchor, VAnchor, OutputMode};

        if self.layer_surface.is_some() {
            return;
        }

        let surface = self.compositor_state.create_surface(qh);

        let target_output = match &config.output {
            OutputMode::Named(name) => {
                let mut found = None;
                for output in self.output_state.outputs() {
                    if let Some(info) = self.output_state.info(&output)
                        && (info.model.contains(name) || info.make.contains(name)) {
                            found = Some(output);
                            eprintln!("Matched output: {} {} (requested: {})", info.make, info.model, name);
                            break;
                        }
                }
                if found.is_none() {
                    eprintln!("No output matching '{}' found, using default", name);
                }
                found
            }
            _ => None,
        };

        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("inno_notification"),
            target_output.as_ref(),
        );

        // Build anchor flags from config
        let mut anchor = Anchor::empty();
        match config.anchor.h {
            HAnchor::Left => anchor |= Anchor::LEFT,
            HAnchor::Right => anchor |= Anchor::RIGHT,
            HAnchor::Center => {} // no horizontal anchor = centered
        }
        match config.anchor.v {
            VAnchor::Top => anchor |= Anchor::TOP,
            VAnchor::Bottom => anchor |= Anchor::BOTTOM,
            VAnchor::Center => {} // no vertical anchor = centered
        }

        layer.set_anchor(anchor);
        let s = self.effective_scale(config);
        set_margins(&layer, config, s);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(1, 1);
        layer.commit();

        self.layer_surface = Some(layer);
    }

    fn effective_scale(&self, config: &AppConfig) -> f64 {
        config.scale * self.scale_factor as f64
    }

    pub fn update_scale_margins(&mut self, config: &AppConfig) {
        if let Some(layer) = &self.layer_surface {
            let s = self.effective_scale(config);
                set_margins(layer, config, s);
            layer.commit();
        }
    }

    /// Loads an animation player by key. A no-op once loaded, and a recorded
    /// no-op after a failure so a broken path is not retried every tick.
    pub fn ensure_animation_loaded(
        &mut self,
        key: &str,
        asset: &AnimAsset,
        config: &AppConfig,
    ) -> bool {
        if self.anim_players.contains_key(key) {
            return true;
        }
        if self.failed_animations.contains(key) {
            return false;
        }

        let target = anim_target(asset, self.effective_scale(config));
        // on_complete = "loop" wins over loop = false, otherwise a non-looping
        // asset would reach FrameTick::Finished and never be told to loop.
        let looping = asset.loop_ || asset.on_complete == crate::config::OnComplete::Loop;

        match AnimPlayer::load(&asset.source, looping, target) {
            Ok(player) => {
                self.anim_players.insert(key.to_string(), player);
                true
            }
            Err(e) => {
                eprintln!("Failed to load animation '{}': {}", key, e);
                self.failed_animations.insert(key.to_string());
                false
            }
        }
    }

    /// Advances the animation and returns its current frame.
    fn animation_frame(player: &mut AnimPlayer) -> Option<AnimFrame> {
        let (w, h) = (player.frame_w, player.frame_h);
        let (source_w, source_h) = player.source_size();
        // The surface is refcounted, so handing out a clone is a pointer bump.
        Some(AnimFrame {
            surface: player.frame().ok()?.clone(),
            w,
            h,
            source_w,
            source_h,
        })
    }

    pub fn tick_animation(&mut self, anim_key: &str) -> Option<(AnimFrame, bool)> {
        let player = self.anim_players.get_mut(anim_key)?;
        player.tick();
        let done = player.is_done();
        Some((Self::animation_frame(player)?, done))
    }

    pub fn reset_animation(&mut self, anim_key: &str) {
        if let Some(player) = self.anim_players.get_mut(anim_key) {
            player.reset();
        }
    }

    pub fn get_animation_frame(&mut self, anim_key: &str) -> Option<AnimFrame> {
        Self::animation_frame(self.anim_players.get_mut(anim_key)?)
    }

    /// Allocate a zeroed Wayland buffer backed by a SlotPool, returning the
    /// wl_buffer, a raw pointer to the pixel data, and the stride.
    /// Handles pool creation/resize, buffer creation, zeroing, and pointer
    /// extraction in one place.  Returns `None` on allocation failure.
    fn allocate_buffer(
        &mut self,
        w: i32,
        h: i32,
    ) -> Option<(smithay_client_toolkit::shm::slot::Buffer, *mut u8, i32)> {
        let stride = w * 4;
        let needed = (w as usize) * (h as usize) * 4;

        let needs_new_pool = match &self.pool {
            None => true,
            Some(pool) => pool.len() < needed,
        };

        if needs_new_pool {
            match SlotPool::new(needed, &self.shm_state) {
                Ok(pool) => self.pool = Some(pool),
                Err(e) => {
                    self.report_alloc_failure(&format!("create shm pool: {}", e));
                    return None;
                }
            }
        }

        let pool = self.pool.as_mut()?;
        let (buffer, canvas) = pool
            .create_buffer(w, h, stride, wl_shm::Format::Argb8888)
            .ok()?;

        canvas.fill(0);
        // SAFETY: canvas is the exclusive mutable slice from SlotPool::create_buffer.
        // The raw pointer carries no lifetime, and NLL ends the borrow here, so
        // no aliased mutable reference exists when the Cairo surface wraps it.
        let ptr = canvas.as_mut_ptr();

        self.alloc_failed = false;
        Some((buffer, ptr, stride))
    }

    /// Attaches and commits a freshly drawn buffer over the whole surface.
    /// All callers have already set self.width/self.height to the damage rect.
    fn commit_buffer(&mut self, buffer: &smithay_client_toolkit::shm::slot::Buffer) {
        let Some(layer) = self.layer_surface.as_ref() else { return };
        layer.set_size(self.width, self.height);
        layer.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
        layer.wl_surface().damage(0, 0, self.width as i32, self.height as i32);
        self.apply_input_region(layer.wl_surface());
        layer.commit();
        // Every commit invalidates the assumption that the surface still shows
        // the cached text at full opacity, and this is the only place that knows.
        self.committed_identity = false;
    }

    /// Declares which parts of this surface may take pointer input.
    ///
    /// A surface starts with an infinite input region, so without this a
    /// notification swallows every click that lands on it, including the
    /// transparent padding above and below the card where nothing is drawn and
    /// the user can plainly see there is nothing there to click.
    ///
    /// An empty list is a valid and useful answer: a hidden notification is a
    /// 1x1 transparent pixel, still sitting in the middle of the screen, and it
    /// has no business intercepting a click there.
    fn apply_input_region(&self, surface: &wl_surface::WlSurface) {
        let Ok(region) = Region::new(&self.compositor_state) else { eprintln!("PROBE Region::new FAILED"); return };
        for r in self.interactive.iter().filter(|r| !r.is_empty()) {
            region.add(r.x, r.y, r.w, r.h);
        }
        // The surface copies the region at commit, so letting this one die here
        // is the documented way to use it rather than a leak.
        surface.set_input_region(Some(region.wl_region()));
    }

    /// Reports a shm allocation failure once. Repeating it every frame at
    /// animation rate would bury the cause in scrollback.
    fn report_alloc_failure(&mut self, cause: &str) {
        if !self.alloc_failed {
            eprintln!("inno: shm buffer allocation failed ({}), notification stalled", cause);
            self.alloc_failed = true;
        }
    }

    /// Submit a 1×1 transparent pixel (used to hide or clear the surface).
    fn commit_transparent(&mut self) {
        if self.layer_surface.is_none() {
            return;
        }
        self.width = 1;
        self.height = 1;
        self.interactive.clear();

        if let Some((buffer, _, _)) = self.allocate_buffer(1, 1) {
            self.commit_buffer(&buffer);
        }
    }

    /// Draw animation-only display (replaces text notification entirely).
    fn draw_animation_frame(&mut self, frame: &AnimFrame, scale: f64, draw_state: &DrawState) {
        if self.layer_surface.is_none() || !self.configured {
            return;
        }

        // The surface is sized from the source dimensions, not the decoded ones.
        // Frames are decoded at most at their natural size, so a high-DPI
        // display still scales up; sizing from the decode size instead would
        // pin an animation-only notification to 1x while its text card grew.
        let w = (frame.source_w as f64 * scale).ceil().max(1.0) as i32;
        let h = (frame.source_h as f64 * scale).ceil().max(1.0) as i32;

        self.width = w as u32;
        self.height = h as u32;
        self.interactive = vec![Rect { x: 0, y: 0, w, h }];

        let Some((buffer, ptr, stride)) = self.allocate_buffer(w, h) else { return };

        unsafe {
            let surface = cairo::ImageSurface::create_for_data_unsafe(
                ptr, cairo::Format::ARgb32, w, h, stride,
            )
            .expect("cairo surface");

            let cr = cairo::Context::new(&surface).unwrap();
            let sw = w as f64;
            let sh = h as f64;
            let sx = sw / frame.w as f64;
            let sy = sh / frame.h as f64;
            let s = sx.min(sy);

            cr.set_operator(cairo::Operator::Source);
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
            cr.paint().unwrap();

            let dx = (sw - frame.w as f64 * s) / 2.0;
            let dy = (sh - frame.h as f64 * s) / 2.0;
            cr.translate(dx, dy);
            cr.scale(s, s);
            // Honour the transition, so a fade or slide composes with an
            // animation-only notification the same way it does with text.
            cr.translate(
                draw_state.offset_x * scale,
                draw_state.offset_y * scale,
            );
            cr.set_source_surface(&frame.surface, 0.0, 0.0).unwrap();
            cr.paint_with_alpha(draw_state.alpha).unwrap();
            surface.flush();
        }

        self.commit_buffer(&buffer);
    }

    pub fn draw_text_with_signal(
        &mut self,
        text: &str,
        config: &AppConfig,
        signal: Option<&Signal>,
        draw_state: &DrawState,
    ) {
        if self.layer_surface.is_none() || !self.configured {
            return;
        }

        if signal.is_some_and(|s| s.animation == crate::config::Animation::Blink && !draw_state.visible) {
            self.commit_transparent();
            return;
        }

        let scale = self.effective_scale(config);

        if !self.ensure_cached(text, config, signal, scale) {
            return;
        }

        self.blit_cached(scale, draw_state);
    }

    /// Renders `text` into the frame cache unless it is already there. Returns
    /// false when there is nothing to show, having committed a transparent
    /// surface.
    fn ensure_cached(
        &mut self,
        text: &str,
        config: &AppConfig,
        signal: Option<&Signal>,
        scale: f64,
    ) -> bool {
        let key = render_key(text, config, signal, scale);
        if self.frame_cache.matches(&key) {
            return true;
        }
        let (w, h) = draw::measure_text(text, config, signal, scale);
        if w <= 1 || h <= 1 {
            self.frame_cache.clear();
            self.commit_transparent();
            return false;
        }
        self.render_and_cache(text, config, signal, scale, key, w, h);
        true
    }

    /// Draw notification with animation frame on top, text below (vertical layout).
    /// Each call creates a new buffer compositing the current animation frame
    /// alongside the cached text. Text cache is reused across frames.
    fn draw_text_with_anim_bg(
        &mut self,
        text: &str,
        config: &AppConfig,
        signal: Option<&Signal>,
        draw_state: &DrawState,
        frame: &AnimFrame,
    ) {
        if self.layer_surface.is_none() || !self.configured {
            return;
        }

        let scale = self.effective_scale(config);

        // Ensure text cache exists (rendered once)
        if !self.ensure_cached(text, config, signal, scale) {
            return;
        }

        // Clone the refcounted surface to avoid holding an immutable borrow
        // on self.frame_cache across the allocate_buffer call.
        let cached = match self.frame_cache.surface.clone() {
            Some(s) => s,
            None => return,
        };
        let text_w = self.frame_cache.width;
        let text_h = self.frame_cache.height;

        // Animation display size: cap at MAX_ANIM_DISPLAY_PX logical, scaled
        let anim_w = (MAX_ANIM_DISPLAY_PX * scale).ceil() as i32;
        let anim_h = anim_w; // square

        // Vertical layout: anim on top, text below
        let gap = (8.0 * scale).ceil() as i32;
        let padding = (8.0 * scale).ceil() as i32;
        let total_w = (padding * 2 + anim_w).max(padding * 2 + text_w);
        let total_h = padding + anim_h + gap + text_h + padding;
        let anim_x = (total_w - anim_w) / 2;
        let text_x = (total_w - text_w) / 2;
        let text_y = padding + anim_h + gap;

        self.width = total_w as u32;
        self.height = total_h as u32;

        let Some((buffer, ptr, stride)) = self.allocate_buffer(total_w, total_h) else { return };

        unsafe {
            let surface = cairo::ImageSurface::create_for_data_unsafe(
                ptr, cairo::Format::ARgb32, total_w, total_h, stride,
            )
            .expect("cairo surface");

            let cr = cairo::Context::new(&surface).expect("cairo context");

            // 1. Paint animation frame at top center, scaled to fit anim_w x anim_h
            let sx = anim_w as f64 / frame.w as f64;
            let sy = anim_h as f64 / frame.h as f64;
            let s = sx.min(sy);
            let dx = anim_x as f64 + (anim_w as f64 - frame.w as f64 * s) / 2.0;
            let dy = padding as f64 + (anim_h as f64 - frame.h as f64 * s) / 2.0;
            cr.save().unwrap();
            cr.translate(dx, dy);
            cr.scale(s, s);
            cr.set_source_surface(&frame.surface, 0.0, 0.0).unwrap();
            cr.paint().unwrap();
            cr.restore().unwrap();

            // 2. Blit cached text below the animation
            cr.set_source_surface(
                &cached,
                text_x as f64 + draw_state.offset_x * scale,
                text_y as f64 + draw_state.offset_y * scale,
            )
            .unwrap();
            cr.paint_with_alpha(draw_state.alpha).unwrap();
            surface.flush();
        }

        self.commit_buffer(&buffer);
    }

    #[allow(clippy::too_many_arguments)]
    fn render_and_cache(
        &mut self,
        text: &str,
        config: &AppConfig,
        signal: Option<&Signal>,
        scale: f64,
        key: RenderKey,
        w: i32,
        h: i32,
    ) {
        self.frame_cache.clear();
        self.committed_identity = false;
        self.width = w as u32;
        self.height = h as u32;

        // Drawn straight into a heap surface. It used to be drawn into a shared
        // memory buffer, copied into a second surface so the cache would outlive
        // the recyclable slot, and committed. The caller then composited the
        // cache over the identical pixels and committed again, so every
        // notification paid for two buffers, two memsets, two full-surface
        // composites and two commits to display one image.
        let Ok(surface) = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h) else { return };
        let cr = cairo::Context::new(&surface).expect("cairo context");
        draw::draw_with_signal(&cr, text, config, signal, &DrawState::default(), scale);
        surface.flush();

        self.frame_cache.surface = Some(surface);
        self.frame_cache.key = Some(key);
        self.frame_cache.width = w;
        self.frame_cache.height = h;
    }

    fn blit_cached(&mut self, scale: f64, draw_state: &DrawState) {
        let identity = draw_state.alpha == 1.0 && draw_state.offset_x == 0.0 && draw_state.offset_y == 0.0;
        // Most of a transition's frames land on exactly the transform the
        // previous one did: a slide settles after 20 frames and spends the rest
        // of its run unmoved, a fade sits at full opacity through the middle.
        // Compositing those again re-uploads an identical texture to the
        // compositor 130 times over for nothing.
        if identity && self.committed_identity {
            return;
        }
        // Clone the refcounted surface to avoid holding an immutable borrow
        // on self.frame_cache across the allocate_buffer call.
        let Some(cached) = self.frame_cache.surface.clone() else { return };
        let w = self.frame_cache.width;
        let h = self.frame_cache.height;

        self.width = w as u32;
        self.height = h as u32;
        self.interactive = vec![draw::card_rect(w, h, scale)];

        let Some((buffer, ptr, stride)) = self.allocate_buffer(w, h) else { return };

        unsafe {
            let surface = cairo::ImageSurface::create_for_data_unsafe(
                ptr, cairo::Format::ARgb32, w, h, stride,
            )
            .expect("cairo surface");

            let cr = cairo::Context::new(&surface).expect("cairo context");
            cr.set_source_surface(&cached, draw_state.offset_x * scale, draw_state.offset_y * scale).unwrap();
            cr.paint_with_alpha(draw_state.alpha).unwrap();
            surface.flush();
        }

        self.commit_buffer(&buffer);
        self.committed_identity = identity;
    }

    /// Draw text without signal (for DBus Show command)
    pub fn draw_text(&mut self, text: &str, config: &AppConfig) {
        let draw_state = DrawState::default();
        self.draw_text_with_signal(text, config, None, &draw_state);
    }

    /// Advances a frame animation one step and draws it per its display mode.
    /// With `advance = false` the current frame is redrawn without ticking.
    pub fn draw_frame_anim(
        &mut self,
        anim_key: &str,
        config: &AppConfig,
        signal: Option<&Signal>,
        text: &str,
        draw_state: &DrawState,
        advance: bool,
    ) -> FrameTick {
        let Some(asset) = config.animations.get(anim_key) else {
            return FrameTick::Unavailable;
        };
        if advance && !self.ensure_animation_loaded(anim_key, asset, config) {
            return FrameTick::Unavailable;
        }

        let result = if advance {
            self.tick_animation(anim_key)
        } else {
            self.get_animation_frame(anim_key).map(|frame| (frame, false))
        };
        let Some((frame, finished)) = result else {
            return FrameTick::Unavailable;
        };

        match asset.display {
            DisplayMode::Anim => {
                self.draw_animation_frame(&frame, self.effective_scale(config), draw_state)
            }
            DisplayMode::Text => {
                self.draw_text_with_anim_bg(text, config, signal, draw_state, &frame)
            }
        }

        if advance && finished {
            FrameTick::Finished(asset.on_complete)
        } else {
            FrameTick::Continue
        }
    }

    /// Records an animation that stopped working mid-playback, so it is not
    /// reloaded and rediscussed on every tick of the next notification.
    pub fn failed_animations_insert(&mut self, key: &str) {
        self.failed_animations.insert(key.to_string());
    }

    pub fn clear_animations(&mut self) {
        self.anim_players.clear();
        self.failed_animations.clear();
    }

    pub fn hide(&mut self) {
        self.commit_transparent();
    }
}

delegate_registry!(LayerApp);
delegate_seat!(LayerApp);
delegate_pointer!(LayerApp);
delegate_output!(LayerApp);
delegate_compositor!(LayerApp);
delegate_shm!(LayerApp);
delegate_layer!(LayerApp);

impl CompositorHandler for LayerApp {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        self.scale_factor = new_factor.max(1);
        self.scale_changed = true;
        eprintln!("Scale factor changed: {}", self.scale_factor);
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for LayerApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl SeatHandler for LayerApp {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            match self.seat_state.get_pointer(qh, &seat) {
                Ok(pointer) => {
                    eprintln!("Pointer capability acquired");
                    self.pointer = Some(pointer);
                }
                Err(e) => {
                    eprintln!("Failed to get pointer: {}", e);
                }
            }
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            self.pointer = None;
        }
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {
    }
}

/// BTN_LEFT from linux/input-event-codes.h.
const BTN_LEFT: u32 = 0x110;

impl PointerHandler for LayerApp {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[smithay_client_toolkit::seat::pointer::PointerEvent],
    ) {
        for event in events {
            if let smithay_client_toolkit::seat::pointer::PointerEventKind::Press {
                button,
                ..
            } = &event.kind
                && *button == BTN_LEFT {
                let (x, y) = event.position;
                // The compositor only routes a press here if it landed inside
                // the input region, so this is also the assertion that the
                // region and the hit test agree about where the card is.
                let hit = self
                    .interactive
                    .iter()
                    .position(|r| r.contains(x, y))
                    .unwrap_or(usize::MAX);
                eprintln!("inno: click at surface-local ({x:.0}, {y:.0}) hit {hit}");
                self.clicked = true;
            }
        }
    }
}

impl LayerShellHandler for LayerApp {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        if configure.new_size.0 > 0 && configure.new_size.1 > 0 {
            self.width = configure.new_size.0;
            self.height = configure.new_size.1;
        }
        self.configured = true;
    }
}

impl ShmHandler for LayerApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm_state
    }
}

impl ProvidesRegistryState for LayerApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_key_equality() {
        let key1 = RenderKey {
            text: "test".into(),
            signal_icon: "icon".into(),
            signal_icon_size: 24.0,
            signal_color: (1.0, 1.0, 1.0, 1.0),
            font: "monospace".into(),
            font_size: 24.0,
            font_slant: FontSlant::Normal,
            font_weight: FontWeight::Normal,
            bg_color: (0.0, 0.0, 0.0, 0.6),
            text_color: (1.0, 1.0, 1.0, 1.0),
            border_radius: 0.0,
            gradient: false,
            scale: 1.0,
        };
        let key2 = key1.clone();
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_render_key_differs_on_text() {
        let key1 = RenderKey {
            text: "test".into(),
            signal_icon: String::new(),
            signal_icon_size: 0.0,
            signal_color: (1.0, 1.0, 1.0, 1.0),
            font: "monospace".into(),
            font_size: 24.0,
            font_slant: FontSlant::Normal,
            font_weight: FontWeight::Normal,
            bg_color: (0.0, 0.0, 0.0, 0.6),
            text_color: (1.0, 1.0, 1.0, 1.0),
            border_radius: 0.0,
            gradient: false,
            scale: 1.0,
        };
        let key2 = RenderKey { text: "other".into(), ..key1.clone() };
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_render_key_differs_on_scale() {
        let key1 = RenderKey {
            text: "test".into(),
            signal_icon: String::new(),
            signal_icon_size: 0.0,
            signal_color: (1.0, 1.0, 1.0, 1.0),
            font: "monospace".into(),
            font_size: 24.0,
            font_slant: FontSlant::Normal,
            font_weight: FontWeight::Normal,
            bg_color: (0.0, 0.0, 0.0, 0.6),
            text_color: (1.0, 1.0, 1.0, 1.0),
            border_radius: 0.0,
            gradient: false,
            scale: 1.0,
        };
        let key2 = RenderKey { scale: 2.0, ..key1.clone() };
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_frame_cache_empty_initially() {
        let cache = FrameCache::new();
        assert!(cache.key.is_none());
        assert!(cache.surface.is_none());
        assert_eq!(cache.width, 0);
        assert_eq!(cache.height, 0);
    }

    #[test]
    fn test_frame_cache_clear() {
        let key = RenderKey {
            text: "test".into(),
            signal_icon: String::new(),
            signal_icon_size: 0.0,
            signal_color: (1.0, 1.0, 1.0, 1.0),
            font: "monospace".into(),
            font_size: 24.0,
            font_slant: FontSlant::Normal,
            font_weight: FontWeight::Normal,
            bg_color: (0.0, 0.0, 0.0, 0.6),
            text_color: (1.0, 1.0, 1.0, 1.0),
            border_radius: 0.0,
            gradient: false,
            scale: 1.0,
        };
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 10, 10).unwrap();
        let mut cache = FrameCache {
            surface: Some(surface),
            key: Some(key),
            width: 10,
            height: 10,
        };
        cache.clear();
        assert!(cache.key.is_none());
        assert!(cache.surface.is_none());
        assert_eq!(cache.width, 0);
        assert_eq!(cache.height, 0);
    }

    #[test]
    fn test_frame_cache_matches() {
        let key = RenderKey {
            text: "test".into(),
            signal_icon: String::new(),
            signal_icon_size: 0.0,
            signal_color: (1.0, 1.0, 1.0, 1.0),
            font: "monospace".into(),
            font_size: 24.0,
            font_slant: FontSlant::Normal,
            font_weight: FontWeight::Normal,
            bg_color: (0.0, 0.0, 0.0, 0.6),
            text_color: (1.0, 1.0, 1.0, 1.0),
            border_radius: 0.0,
            gradient: false,
            scale: 1.0,
        };
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 10, 10).unwrap();
        let cache = FrameCache {
            surface: Some(surface),
            key: Some(key.clone()),
            width: 10,
            height: 10,
        };
        assert!(cache.matches(&key));

        let different_key = RenderKey { text: "other".into(), ..key };
        assert!(!cache.matches(&different_key));
    }
}

/// A margin measures to the visible card, not to the edge of the surface around
/// it. These pin the arithmetic `set_margins` uses, which is otherwise only
/// observable on a real compositor.
#[cfg(test)]
mod placement_tests {
    use super::*;
    use crate::config::{Anchor, HAnchor, VAnchor};

    /// The four values `set_margins` would hand the compositor, at scale 1.
    fn margins(a: &Anchor, pad: f64) -> (i32, i32, i32, i32) {
        let top = a.margin_top as f64 - if a.v == VAnchor::Top { pad } else { 0.0 };
        let bottom = a.margin_bottom as f64 - if a.v == VAnchor::Bottom { pad } else { 0.0 };
        (top as i32, a.margin_right, bottom as i32, a.margin_left)
    }

    const PAD: f64 = draw::V_PADDING / 2.0;

    #[test]
    fn the_padding_is_the_60px_that_used_to_eat_a_zero_margin() {
        assert_eq!(draw::V_PADDING, 120.0);
        assert_eq!(PAD, 60.0);
    }

    #[test]
    fn a_centred_axis_is_never_compensated() {
        let a = Anchor {
            h: HAnchor::Center,
            v: VAnchor::Center,
            margin_top: 10,
            margin_bottom: 10,
            margin_left: 10,
            margin_right: 10,
        };
        assert_eq!(margins(&a, PAD), (10, 10, 10, 10));
    }

    #[test]
    fn a_bottom_anchored_margin_is_the_distance_to_the_card() {
        let a = Anchor { v: VAnchor::Bottom, margin_bottom: 90, margin_top: 5, ..Default::default() };
        let (top, _right, bottom, _left) = margins(&a, PAD);
        // The surface goes 90 - 60, so that after the 60px of padding the
        // visible card sits 90px above the edge.
        assert_eq!(bottom, 30);
        assert_eq!(top, 5, "an unanchored edge keeps its margin");
    }

    #[test]
    fn a_negative_margin_hangs_the_card_off_the_edge() {
        let a = Anchor { v: VAnchor::Bottom, margin_bottom: -20, ..Default::default() };
        assert_eq!(margins(&a, PAD).2, -80);
    }
}
