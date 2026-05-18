use crate::config::AppConfig;
use crate::config::Signal;
use crate::draw;
use crate::draw::DrawState;
use cairo::FontSlant;
use cairo::FontWeight;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
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
            frame_cache: FrameCache::new(),
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
        layer.set_margin(
            ((config.anchor.margin_v + config.anchor.offset_y) as f64 * s) as i32,
            ((config.anchor.margin_h + config.anchor.offset_x) as f64 * s) as i32,
            ((config.anchor.margin_v - config.anchor.offset_y) as f64 * s) as i32,
            ((config.anchor.margin_h - config.anchor.offset_x) as f64 * s) as i32,
        );
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
            layer.set_margin(
                ((config.anchor.margin_v + config.anchor.offset_y) as f64 * s) as i32,
                ((config.anchor.margin_h + config.anchor.offset_x) as f64 * s) as i32,
                ((config.anchor.margin_v - config.anchor.offset_y) as f64 * s) as i32,
                ((config.anchor.margin_h - config.anchor.offset_x) as f64 * s) as i32,
            );
            layer.commit();
        }
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
            self.commit_clear();
            return;
        }

        let scale = self.effective_scale(config);

        let key = RenderKey {
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
        };

        if !self.frame_cache.matches(&key) {
            let (w, h) = draw::measure_text(text, config, signal, scale);
            if w <= 1 || h <= 1 {
                self.frame_cache.clear();
                self.commit_1x1();
                return;
            }
            self.render_and_cache(text, config, signal, scale, key, w, h);
        }

        self.blit_cached(scale, draw_state);
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

        self.width = w as u32;
        self.height = h as u32;

        let needed = self.width as usize * self.height as usize * 4;
        match &mut self.pool {
            None => {
                self.pool =
                    Some(SlotPool::new(needed, &self.shm_state).expect("Failed to create pool"));
            }
            Some(pool) => {
                if pool.len() < needed {
                    self.pool = Some(
                        SlotPool::new(needed, &self.shm_state).expect("Failed to resize pool"),
                    );
                }
            }
        }

        let stride = self.width as i32 * 4;

        let (buffer, canvas) = {
            let pool = self.pool.as_mut().unwrap();
            pool.create_buffer(
                self.width as i32,
                self.height as i32,
                stride,
                wl_shm::Format::Argb8888,
            )
            .expect("create buffer")
        };

        // SAFETY: canvas is the exclusive mutable slice from SlotPool::create_buffer.
        // We extract the raw pointer and drop canvas before creating the Cairo surface,
        // so there is no aliased mutable reference.
        let ptr = canvas.as_mut_ptr();
        let _ = canvas;

        unsafe {
            let surface = cairo::ImageSurface::create_for_data_unsafe(
                ptr,
                cairo::Format::ARgb32,
                self.width as i32,
                self.height as i32,
                stride,
            )
            .expect("cairo surface");

            let base_state = DrawState { frame: 0, visible: true, alpha: 1.0, offset_x: 0.0, offset_y: 0.0 };
            let cr = cairo::Context::new(&surface).expect("cairo context");
            draw::draw_with_signal(&cr, text, config, signal, &base_state, scale);
            surface.flush();

            let cached = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).unwrap();
            let cr_cache = cairo::Context::new(&cached).unwrap();
            cr_cache.set_source_surface(&surface, 0.0, 0.0).unwrap();
            cr_cache.paint().unwrap();

            self.frame_cache.surface = Some(cached);
            self.frame_cache.key = Some(key);
            self.frame_cache.width = w;
            self.frame_cache.height = h;
        }

        let layer = self.layer_surface.as_ref().unwrap();
        layer.set_size(self.width, self.height);
        layer.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
        layer.wl_surface().damage(0, 0, self.width as i32, self.height as i32);
        layer.commit();
    }

    fn blit_cached(&mut self, scale: f64, draw_state: &DrawState) {
        let cached = self.frame_cache.surface.as_ref().unwrap();
        let w = self.frame_cache.width;
        let h = self.frame_cache.height;

        self.width = w as u32;
        self.height = h as u32;

        let stride = w * 4;
        let needed = (w * h * 4) as usize;

        match &mut self.pool {
            None => {
                self.pool =
                    Some(SlotPool::new(needed, &self.shm_state).expect("Failed to create pool"));
            }
            Some(pool) => {
                if pool.len() < needed {
                    self.pool = Some(
                        SlotPool::new(needed, &self.shm_state).expect("Failed to resize pool"),
                    );
                }
            }
        }

        let (buffer, canvas) = {
            let pool = self.pool.as_mut().unwrap();
            pool.create_buffer(w, h, stride, wl_shm::Format::Argb8888)
                .expect("create buffer")
        };

        // SAFETY: canvas is the exclusive mutable slice from SlotPool::create_buffer.
        // We zero it, extract the raw pointer, and drop canvas before creating the Cairo
        // surface, so there is no aliased mutable reference.
        for b in canvas.iter_mut() {
            *b = 0;
        }
        let ptr = canvas.as_mut_ptr();
        let _ = canvas;

        unsafe {
            let surface = cairo::ImageSurface::create_for_data_unsafe(
                ptr,
                cairo::Format::ARgb32,
                w,
                h,
                stride,
            )
            .expect("cairo surface");

            let cr = cairo::Context::new(&surface).expect("cairo context");
            cr.set_source_surface(cached, draw_state.offset_x * scale, draw_state.offset_y * scale).unwrap();
            cr.paint_with_alpha(draw_state.alpha).unwrap();
            surface.flush();
        }

        let layer = self.layer_surface.as_ref().unwrap();
        layer.set_size(self.width, self.height);
        layer.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
        layer.wl_surface().damage(0, 0, w, h);
        layer.commit();
    }

    fn commit_clear(&mut self) {
        if let Some(layer) = &self.layer_surface {
            self.width = 1;
            self.height = 1;
            layer.set_size(1, 1);

            if let Some(pool) = &mut self.pool
                && let Ok((buffer, canvas)) = pool.create_buffer(1, 1, 4, wl_shm::Format::Argb8888)
            {
                for i in canvas.iter_mut() {
                    *i = 0;
                }
                layer.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
                layer.wl_surface().damage(0, 0, 1, 1);
                layer.commit();
            }
        }
    }

    fn commit_1x1(&mut self) {
        if let Some(layer) = &self.layer_surface
            && let Some(pool) = &mut self.pool
            && let Ok((buffer, canvas)) = pool.create_buffer(1, 1, 4, wl_shm::Format::Argb8888)
        {
            for i in canvas.iter_mut() {
                *i = 0;
            }
            layer.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
            layer.wl_surface().damage(0, 0, 1, 1);
            layer.commit();
        }
    }

    /// Draw text without signal (for DBus Show command)
    pub fn draw_text(&mut self, text: &str, config: &AppConfig) {
        let draw_state = DrawState::default();
        self.draw_text_with_signal(text, config, None, &draw_state);
    }

    pub fn hide(&mut self) {
        if let Some(layer) = &self.layer_surface {
            self.width = 1;
            self.height = 1;
            layer.set_size(1, 1);

            if let Some(pool) = &mut self.pool
                && let Ok((buffer, canvas)) = pool.create_buffer(1, 1, 4, wl_shm::Format::Argb8888)
            {
                for i in canvas.iter_mut() {
                    *i = 0;
                }
                layer.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
                layer.wl_surface().damage(0, 0, 1, 1);
                layer.commit();
            }
        }
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
                && *button == 0x110 {
                    eprintln!("Click detected, dismissing");
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
