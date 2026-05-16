use crate::config::AppConfig;
use crate::config::Signal;
use crate::draw;
use crate::draw::DrawState;
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

    /// Draw text without signal (for DBus Show command)
    pub fn draw_text(&mut self, text: &str, config: &AppConfig) {
        let draw_state = DrawState::default();
        self.draw_text_with_signal(text, config, None, &draw_state);
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

        let (w, h) = draw::measure_text(text, config, signal, self.effective_scale(config));

        if w <= 1 || h <= 1 {
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
            return;
        }

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

        unsafe {
            let ptr = canvas.as_ptr() as *mut u8;
            let len = canvas.len();
            let canvas_slice = std::slice::from_raw_parts_mut(ptr, len);

            let surface = cairo::ImageSurface::create_for_data(
                canvas_slice,
                cairo::Format::ARgb32,
                self.width as i32,
                self.height as i32,
                stride,
            )
            .expect("cairo surface");

            let cr = cairo::Context::new(&surface).expect("cairo context");
            draw::draw_with_signal(&cr, text, config, signal, draw_state, self.effective_scale(config));
            surface.flush();
        }

        let layer = self.layer_surface.as_ref().unwrap();
        layer.set_size(self.width, self.height);
        layer.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
        layer.wl_surface().damage(0, 0, self.width as i32, self.height as i32);
        layer.commit();
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
