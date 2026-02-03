use tokio::sync::mpsc;
use smithay_client_toolkit::reexports::client::Connection;
use tokio::io::unix::AsyncFd;
use std::time::Duration;

mod config;
mod dbus;
mod layer;
mod draw;

use config::AppConfig;
use layer::LayerApp;
use dbus::{Event, BatteryState};
use draw::DrawState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = AppConfig::load();
    println!("Loaded config: {:?}", config);
    println!("Loaded {} signals", config.signals.len());
    
    let (tx, mut rx) = mpsc::channel(10);
    
    tokio::spawn(async move {
        if let Err(e) = dbus::run_dbus_listener(tx).await {
            eprintln!("DBus error: {}", e);
        }
    });
    
    let conn = Connection::connect_to_env()?;
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();
    
    let mut app = LayerApp::new(&conn, &qh)?;
    
    // Initial roundtrip
    event_queue.blocking_dispatch(&mut app)?;
    
    app.create_surface(&qh, &config);
    event_queue.blocking_dispatch(&mut app)?;
    
    let backend = conn.backend();
    let fd = backend.poll_fd();
    let async_fd = AsyncFd::new(fd)?;
    
    let mut current_battery: Option<BatteryState> = None;
    let mut current_text: Option<String> = None;
    let mut prev_state: Option<String> = None;
    let mut prev_signal_msg: Option<String> = None;
    let mut draw_state = DrawState::default();
    let mut hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(86400))); 
    let mut animation_timer = Box::pin(tokio::time::sleep(Duration::from_millis(33))); // ~30fps
    let mut animating = false;

    loop {
        event_queue.dispatch_pending(&mut app)?;
        
        if app.exit { break; }
        
        let _ = conn.flush();

        tokio::select! {
            Some(event) = rx.recv() => {
                match event {
                    Event::Battery(state) => {
                        let signal = config.find_signal(state.percentage, &state.state);
                        let signal_msg = signal.map(|s| s.message.clone());
                        
                        // Check if state changed or signal condition newly reached
                        let state_changed = prev_state.as_ref() != Some(&state.state);
                        let signal_changed = prev_signal_msg != signal_msg;
                        
                        if state_changed || signal_changed {
                            println!("Notify: {:.0}% {} (state_changed={}, signal_changed={})", 
                                state.percentage, state.state, state_changed, signal_changed);
                            
                            if let Some(sig) = signal {
                                let text = format!("{} {:.0}%", sig.message, state.percentage);
                                draw_state.reset();
                                app.draw_text_with_signal(&text, &config, Some(sig), &draw_state);
                                animating = sig.animation != config::Animation::None;
                                hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(sig.duration)));
                                current_text = Some(text);
                            } else if state_changed {
                                // Only show plain battery on state change, not on every update
                                let text = format!("{}", state.state.to_uppercase());
                                app.draw_text(&text, &config);
                                animating = false;
                                hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(2)));
                                current_text = Some(text);
                            }
                        }
                        
                        prev_state = Some(state.state.clone());
                        prev_signal_msg = signal_msg;
                        current_battery = Some(state);
                    }
                    Event::StateChange(_) => {
                        // Handled in Battery event now
                    }
                }
            }
            
            _ = &mut animation_timer, if animating => {
                if let (Some(battery), Some(text)) = (&current_battery, &current_text) {
                    if let Some(signal) = config.find_signal(battery.percentage, &battery.state) {
                        draw_state.tick(&signal.animation);
                        app.draw_text_with_signal(text, &config, Some(signal), &draw_state);
                    }
                }
                animation_timer = Box::pin(tokio::time::sleep(Duration::from_millis(33)));
            }
            
            _ = &mut hide_timer => {
                if current_text.is_some() {
                    println!("Auto-hiding");
                    app.hide();
                    current_text = None;
                    current_battery = None;
                    animating = false;
                    draw_state.reset();
                    hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(86400)));
                }
            }
            
            guard = async_fd.readable() => {
                match guard {
                    Ok(mut guard) => {
                        // Clear ready state before attempting read
                        guard.clear_ready();
                        
                        // Try to read - if WouldBlock/EAGAIN, that's fine
                        match conn.prepare_read() {
                            Some(read_guard) => {
                                match read_guard.read() {
                                    Ok(_) => {
                                        // Successfully read events
                                    }
                                    Err(e) => {
                                        // Check if it's just EAGAIN/WouldBlock
                                        use wayland_client::backend::WaylandError;
                                        let should_break = match &e {
                                            WaylandError::Io(io_err) => {
                                                io_err.kind() != std::io::ErrorKind::WouldBlock
                                            }
                                            _ => true, // Other errors are real errors
                                        };
                                        
                                        if should_break {
                                            eprintln!("Wayland Read Error: {}", e);
                                            break;
                                        }
                                    }
                                }
                            }
                            None => {
                                // Events already in queue, will be dispatched next iteration
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }
    
    Ok(())
}
