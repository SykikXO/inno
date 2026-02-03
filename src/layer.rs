use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm, delegate_seat,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{Capability, SeatHandler, SeatState},
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, ShmHandler, Shm},
    reexports::client::{
        protocol::{wl_output, wl_seat, wl_shm, wl_surface},
        Connection, QueueHandle, globals::registry_queue_init,
    },
};
use crate::config::AppConfig;
use crate::config::Signal;
use crate::draw;
use crate::draw::DrawState;

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
        })
    }
    
    pub fn create_surface(&mut self, qh: &QueueHandle<Self>, config: &AppConfig) {
        use crate::config::{HAnchor, VAnchor};
        
        if self.layer_surface.is_some() { return; }
        
        let surface = self.compositor_state.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("inno_notification"),
            None,
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
        layer.set_margin(config.anchor.margin_v, config.anchor.margin_h, config.anchor.margin_v, config.anchor.margin_h);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(1, 1);
        layer.commit();
        
        self.layer_surface = Some(layer);
    }
    
    pub fn draw_text(&mut self, text: &str, config: &AppConfig) {
        self.draw_text_with_signal(text, config, None, &DrawState::default())
    }
    
    pub fn draw_text_with_signal(&mut self, text: &str, config: &AppConfig, signal: Option<&Signal>, draw_state: &DrawState) {
        if self.layer_surface.is_none() || !self.configured { return; }
        
        let dummy = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1).unwrap();
        let cr = cairo::Context::new(&dummy).unwrap();
        let (w, h) = draw::draw_with_signal(&cr, text, config, signal, draw_state);
        
        // Skip drawing if animation says not visible
        if w <= 1 || h <= 1 {
            // For flicker-off, just clear the surface
            if let Some(layer) = &self.layer_surface {
                if let Some(pool) = &mut self.pool {
                    if let Ok((buffer, canvas)) = pool.create_buffer(1, 1, 4, wl_shm::Format::Argb8888) {
                        for i in canvas.iter_mut() { *i = 0; }
                        layer.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
                        layer.wl_surface().damage(0, 0, 1, 1);
                        layer.commit();
                    }
                }
            }
            return;
        }
        
        self.width = w as u32;
        self.height = h as u32;
        
        // Create pool if needed
        if self.pool.is_none() {
             self.pool = Some(SlotPool::new(self.width as usize * self.height as usize * 4, &self.shm_state).expect("Failed to create pool"));
        }
        
        let stride = self.width as i32 * 4;
        
        // Get buffer from pool
        let (buffer, canvas) = {
            let pool = self.pool.as_mut().unwrap();
            pool.create_buffer(
                self.width as i32,
                self.height as i32,
                stride,
                wl_shm::Format::Argb8888
            ).expect("create buffer")
        };
        
        // Draw to canvas using unsafe
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
            ).expect("cairo surface");
            
            let cr = cairo::Context::new(&surface).expect("cairo context");
            draw::draw_with_signal(&cr, text, config, signal, draw_state);
            surface.flush(); 
        }
        
        // Attach buffer to surface
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
            
            if let Some(pool) = &mut self.pool {
                if let Ok((buffer, canvas)) = pool.create_buffer(1, 1, 4, wl_shm::Format::Argb8888) {
                    for i in canvas.iter_mut() { *i = 0; }
                    layer.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
                    layer.wl_surface().damage(0, 0, 1, 1);
                    layer.commit();
                }
            }
        }
    }
}

delegate_registry!(LayerApp);
delegate_seat!(LayerApp);
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
        _new_factor: i32,
    ) {
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

    fn new_seat(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
    ) {
    }

    fn new_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        _capability: Capability,
    ) {
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        _capability: Capability,
    ) {
    }

    fn remove_seat(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
    ) {
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
