use smithay_client_toolkit::reexports::client::Connection;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::io::unix::AsyncFd;
use tokio::sync::mpsc;
use notify::{Watcher, RecursiveMode, Event as NotifyEvent};
use rodio::Source;

mod config;
mod control;
mod dbus;
mod draw;
mod events;
mod layer;

use config::{AppConfig, ANIMATION_INTERVAL_MS, HIDE_TIMEOUT_SECS};
use control::ControlEvent;
use dbus::Event;
use draw::{DrawState, format_text};
use layer::LayerApp;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const HELP: &str = r#"inno - Wayland notification daemon with configurable DBus events

USAGE:
    inno [OPTIONS]

OPTIONS:
    -h, --help              Show this help message
    -v, --version           Show version
    -d, --daemon            Run in background (daemon mode)
    -l, --log-file <PATH>   Log output to file (useful with -d)
    --no-dbus               Disable DBus control interface

CONFIG:
    ~/.config/inno/inno.toml   (main config)
    ~/.config/inno/events/     (event definitions)

DBUS CONTROL:
    busctl --user call org.inno.Control /org/inno/Control org.inno.Control Show "st" "Hello" 5
    busctl --user call org.inno.Control /org/inno/Control org.inno.Control Hide
"#;

/// Play sound file if exists
fn play_sound(path: &PathBuf) {
    if !path.exists() {
        eprintln!("Sound file not found: {:?}", path);
        return;
    }
    
    std::thread::spawn({
        let path = path.clone();
        move || {
            if let Ok((_stream, stream_handle)) = rodio::OutputStream::try_default() {
                if let Ok(file) = File::open(&path) {
                    let reader = BufReader::new(file);
                    if let Ok(source) = rodio::Decoder::new(reader) {
                        let _ = stream_handle.play_raw(source.convert_samples());
                        std::thread::sleep(Duration::from_secs(5));
                    }
                }
            }
        }
    });
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut log_file: Option<PathBuf> = None;
    let mut enable_dbus = true;
    let mut i = 1;
    
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print!("{}", HELP);
                return Ok(());
            }
            "-v" | "--version" => {
                println!("inno {}", VERSION);
                return Ok(());
            }
            "-d" | "--daemon" => {
                unsafe {
                    if libc::fork() != 0 {
                        std::process::exit(0);
                    }
                    libc::setsid();
                }
            }
            "-l" | "--log-file" => {
                i += 1;
                if i < args.len() {
                    log_file = Some(PathBuf::from(&args[i]));
                }
            }
            "--no-dbus" => {
                enable_dbus = false;
            }
            _ => {}
        }
        i += 1;
    }

    // Redirect stderr to log file if specified
    if let Some(ref path) = log_file {
        use std::os::unix::io::AsRawFd;
        if let Ok(file) = File::create(path) {
            unsafe {
                libc::dup2(file.as_raw_fd(), 2);
            }
        }
    }

    let mut config = AppConfig::load();
    eprintln!("inno: loaded {} signals", config.signals.len());

    // Load event configurations
    let event_configs = events::load_events();
    eprintln!("inno: loaded {} event configs", event_configs.len());

    // Channels
    let (tx, mut rx) = mpsc::channel(10);
    let (config_tx, mut config_rx) = mpsc::channel::<()>(1);
    let (control_tx, mut control_rx) = mpsc::channel::<ControlEvent>(10);

    // Shared battery state for DBus interface
    let battery_percentage = Arc::new(AtomicU32::new(10000)); // 100.00%
    let battery_state_shared = Arc::new(RwLock::new("unknown".to_string()));

    // Start DBus control interface
    let _dbus_conn = if enable_dbus {
        match control::start_control_service(
            control_tx.clone(),
            battery_percentage.clone(),
            battery_state_shared.clone(),
        ).await {
            Ok(conn) => Some(conn),
            Err(e) => {
                eprintln!("Failed to start DBus control interface: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Watch config file for changes
    if let Some(ref config_path) = config.config_path {
        let config_path = config_path.clone();
        let config_tx = config_tx.clone();
        
        std::thread::spawn(move || {
            let (watcher_tx, watcher_rx) = std::sync::mpsc::channel();
            let mut watcher = notify::recommended_watcher(move |res: Result<NotifyEvent, _>| {
                if let Ok(event) = res {
                    if event.kind.is_modify() {
                        let _ = watcher_tx.send(());
                    }
                }
            }).ok();
            
            if let Some(ref mut w) = watcher {
                let _ = w.watch(&config_path, RecursiveMode::NonRecursive);
            }
            
            while let Ok(()) = watcher_rx.recv() {
                let _ = config_tx.blocking_send(());
            }
        });
    }

    // Start DBus event listener with configurable events
    tokio::spawn(async move {
        if let Err(e) = dbus::run_dbus_listener(tx, event_configs).await {
            eprintln!("DBus error: {}", e);
        }
    });

    let conn = Connection::connect_to_env()?;
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();

    let mut app = LayerApp::new(&conn, &qh)?;
    event_queue.blocking_dispatch(&mut app)?;

    app.create_surface(&qh, &config);
    event_queue.blocking_dispatch(&mut app)?;

    let backend = conn.backend();
    let fd = backend.poll_fd();
    let async_fd = AsyncFd::new(fd)?;

    let mut current_text: Option<String> = None;
    let mut prev_state: Option<String> = None;
    let mut prev_signal_msg: Option<String> = None;
    let mut draw_state = DrawState::default();
    let mut hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(HIDE_TIMEOUT_SECS)));
    let mut animation_timer = Box::pin(tokio::time::sleep(Duration::from_millis(ANIMATION_INTERVAL_MS)));
    let mut animating = false;
    let mut current_percentage: Option<f64> = None;
    let mut current_state_str: Option<String> = None;

    loop {
        event_queue.dispatch_pending(&mut app)?;

        if app.exit {
            break;
        }

        let _ = conn.flush();

        tokio::select! {
            // Config reload (from file watcher)
            Some(()) = config_rx.recv() => {
                eprintln!("Config file changed, reloading...");
                config = AppConfig::load();
                eprintln!("inno: reloaded {} signals", config.signals.len());
            }

            // DBus control events
            Some(control_event) = control_rx.recv() => {
                match control_event {
                    ControlEvent::Show { message, duration } => {
                        eprintln!("DBus: Show '{}' for {}s", message, duration);
                        draw_state.reset();
                        app.draw_text(&message, &config);
                        hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(duration)));
                        current_text = Some(message);
                        animating = false;
                    }
                    ControlEvent::Hide => {
                        eprintln!("DBus: Hide");
                        app.hide();
                        current_text = None;
                        animating = false;
                        hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(HIDE_TIMEOUT_SECS)));
                    }
                    ControlEvent::Reload => {
                        eprintln!("DBus: Reload config");
                        config = AppConfig::load();
                        eprintln!("inno: reloaded {} signals", config.signals.len());
                    }
                }
            }

            Some(event) = rx.recv() => {
                match event {
                    Event::Notify(notify_event) => {
                        // Update shared state for DBus control interface
                        if let Some(pct) = notify_event.percentage {
                            battery_percentage.store((pct * 100.0) as u32, Ordering::Relaxed);
                        }
                        if let Some(ref state) = notify_event.state {
                            if let Ok(mut s) = battery_state_shared.write() {
                                *s = state.clone();
                            }
                        }

                        // Get percentage and state for signal matching
                        let pct = notify_event.percentage.unwrap_or(100.0);
                        let state = notify_event.state.clone().unwrap_or_else(|| "unknown".to_string());

                        // Find matching signal from config
                        let signal = config.find_signal(pct, &state);
                        let signal_msg = signal.map(|s| s.message.clone());

                        let state_changed = prev_state.as_ref() != Some(&state);
                        let signal_changed = prev_signal_msg != signal_msg;

                        if state_changed || signal_changed {
                            println!("Notify: {:.0}% {} (state={}, signal={})",
                                pct, notify_event.event_name, state_changed, signal_changed);

                            if let Some(sig) = signal {
                                // Format text using config format string
                                let text = format_text(
                                    &config.format,
                                    &sig.icon,
                                    &sig.message,
                                    pct,
                                );
                                
                                // Play sound if configured
                                if let Some(ref sound_path) = sig.sound {
                                    play_sound(sound_path);
                                }
                                
                                draw_state.reset();
                                app.draw_text_with_signal(&text, &config, Some(sig), &draw_state);
                                animating = sig.animation != config::Animation::None;
                                hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(sig.duration)));
                                current_text = Some(text);
                            }
                        }

                        prev_state = Some(state.clone());
                        prev_signal_msg = signal_msg;
                        current_percentage = Some(pct);
                        current_state_str = Some(state);
                    }
                }
            }

            _ = &mut animation_timer, if animating => {
                if let (Some(pct), Some(state), Some(text)) = (current_percentage, &current_state_str, &current_text) {
                    if let Some(signal) = config.find_signal(pct, state) {
                        draw_state.tick(&signal.animation);
                        app.draw_text_with_signal(text, &config, Some(signal), &draw_state);
                    }
                }
                animation_timer = Box::pin(tokio::time::sleep(Duration::from_millis(ANIMATION_INTERVAL_MS)));
            }

            _ = &mut hide_timer => {
                if current_text.is_some() {
                    println!("Auto-hiding");
                    app.hide();
                    current_text = None;
                    animating = false;
                    draw_state.reset();
                    hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(HIDE_TIMEOUT_SECS)));
                }
            }

            guard = async_fd.readable() => {
                match guard {
                    Ok(mut guard) => {
                        guard.clear_ready();

                        match conn.prepare_read() {
                            Some(read_guard) => {
                                match read_guard.read() {
                                    Ok(_) => {}
                                    Err(e) => {
                                        use wayland_client::backend::WaylandError;
                                        let should_break = match &e {
                                            WaylandError::Io(io_err) => {
                                                io_err.kind() != std::io::ErrorKind::WouldBlock
                                            }
                                            _ => true,
                                        };

                                        if should_break {
                                            eprintln!("Wayland Read Error: {}", e);
                                            break;
                                        }
                                    }
                                }
                            }
                            None => {}
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }

    Ok(())
}
