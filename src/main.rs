use notify::{Event as FsEvent, RecursiveMode, Watcher};
use smithay_client_toolkit::reexports::client::Connection;
use std::fs::File;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::io::unix::AsyncFd;
use tokio::sync::mpsc;

mod args;
mod battery;
mod config;
mod control;
mod dbus;
mod draw;
mod events;
mod layer;
mod sound;
mod state;

use args::{Action, Args};
use config::{AppConfig, HIDE_TIMEOUT_SECS};
use control::ControlEvent;
use dbus::Event;
use layer::LayerApp;
use sound::SoundWorker;
use state::NotificationState;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let Args { action, debug_mode, enable_dbus, log_file, test_animation, test_all_animations } =
        args::parse();

    match action {
        Action::Help => {
            print!("{}", args::help_text());
            return Ok(());
        }
        Action::Version => {
            println!("inno {}", VERSION);
            return Ok(());
        }
        Action::Daemon => {
            println!("inno is running as a daemon. To stop it, use 'pkill inno'.");
            use std::os::unix::process::CommandExt;

            let args: Vec<String> = std::env::args().collect();
            let mut cmd = std::process::Command::new(&args[0]);
            for arg in &args[1..] {
                if arg != "--daemon" {
                    cmd.arg(arg);
                }
            }
            cmd.arg("--internal-daemon");

            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }

            if let Some(ref path) = log_file
                && let Ok(file) = File::create(path)
            {
                cmd.stderr(file);
            }

            cmd.spawn().expect("Failed to spawn background daemon");
            std::process::exit(0);
        }
        Action::CheckConfig => {
            let cfg = AppConfig::load();
            let event_cfgs = events::load_events();
            let (errors, warnings) = cfg.validate();
            if let Some(ref path) = cfg.config_path {
                println!("Config file: {:?}", path);
            }
            println!("Signals: {}", cfg.signals.len());
            println!("Events: {}", event_cfgs.len());

            if !warnings.is_empty() {
                println!("\nWarnings:");
                for w in &warnings {
                    println!("  WARNING: {}", w);
                }
            }

            if !errors.is_empty() {
                println!("\nErrors:");
                for e in &errors {
                    println!("  ERROR: {}", e);
                }
                std::process::exit(1);
            }

            if warnings.is_empty() && errors.is_empty() {
                println!("Config is valid.");
            }
            return Ok(());
        }
        Action::InternalDaemon => {}
    }

    if debug_mode {
        println!("inno is running in debug mode.");
    }

    let mut config = AppConfig::load();
    eprintln!("inno: loaded {} signals", config.signals.len());

    let event_configs = events::load_events();
    eprintln!("inno: loaded {} event configs", event_configs.len());

    let (tx, mut rx) = mpsc::channel(10);
    let (config_tx, mut config_rx) = mpsc::channel::<()>(1);
    let (control_tx, mut control_rx) = mpsc::channel::<ControlEvent>(10);

    let sound_worker = SoundWorker::new();

    let battery_percentage = Arc::new(AtomicU32::new(10000));
    let battery_state_shared = Arc::new(RwLock::new("unknown".to_string()));

    let _dbus_conn = if enable_dbus {
        match control::start_control_service(
            control_tx.clone(),
            battery_percentage.clone(),
            battery_state_shared.clone(),
        )
        .await
        {
            Ok(conn) => Some(conn),
            Err(e) => {
                eprintln!("Failed to start DBus control interface: {}", e);
                None
            }
        }
    } else {
        None
    };

    if let Some(ref config_path) = config.config_path {
        let config_path = config_path.clone();
        let config_tx = config_tx.clone();

        std::thread::spawn(move || {
            let (watcher_tx, watcher_rx) = std::sync::mpsc::channel();
            let mut watcher = notify::recommended_watcher(move |res: Result<FsEvent, _>| {
                if let Ok(event) = res
                    && event.kind.is_modify()
                {
                    let _ = watcher_tx.send(());
                }
            })
            .ok();

            if let Some(ref mut w) = watcher {
                let _ = w.watch(&config_path, RecursiveMode::NonRecursive);
            }

            while let Ok(()) = watcher_rx.recv() {
                let _ = config_tx.blocking_send(());
            }
        });
    }

    if !test_all_animations {
        tokio::spawn(async move {
            if let Err(e) = dbus::run_dbus_listener(tx, event_configs).await {
                eprintln!("DBus error: {}", e);
            }
        });
    } else {
        eprintln!("Skipping DBus listener in testing mode.");
    }

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

    let mut state = NotificationState::new();
    let mut hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(HIDE_TIMEOUT_SECS)));
    let mut animation_timer =
        Box::pin(tokio::time::sleep(Duration::from_micros(1_000_000 / config.fps.max(1))));

    let test_animations_list = [
        config::Animation::Blink,
        config::Animation::Pulse,
        config::Animation::Fade,
        config::Animation::SlideRight,
        config::Animation::SlideLeft,
        config::Animation::Bounce,
    ];
    let mut current_test_signal: Option<config::Signal> = None;
    let mut test_anim_idx = test_animation.unwrap_or(0);
    let mut test_timer = Box::pin(tokio::time::sleep(Duration::from_secs(0)));

    if test_all_animations {
        eprintln!("Animation testing mode enabled.");
        state.animating = true;
    }

    loop {
        event_queue.dispatch_pending(&mut app)?;

        if app.exit {
            break;
        }

        if app.scale_changed {
            app.scale_changed = false;
            app.update_scale_margins(&config);
            if let Some(ref text) = state.current_text {
                state.draw_state.reset();
                if let Some(idx) = state.current_signal_idx {
                    let signal = &config.signals[idx];
                    app.draw_text_with_signal(text, &config, Some(signal), &state.draw_state);
                    state.animating = signal.animation != config::Animation::None;
                } else {
                    app.draw_text(text, &config);
                    state.animating = false;
                }
            }
        }

        if app.clicked {
            app.clicked = false;
            state.dismiss_by_click(&mut app);
            hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(HIDE_TIMEOUT_SECS)));
        }

        let _ = conn.flush();

        tokio::select! {
            Some(()) = config_rx.recv() => {
                eprintln!("Config file changed, reloading...");
                let old_scale = config.scale;
                config = AppConfig::load();
                eprintln!("inno: reloaded {} signals", config.signals.len());
                app.frame_cache.clear();
                animation_timer = Box::pin(tokio::time::sleep(Duration::from_micros(1_000_000 / config.fps.max(1))));
                state.on_config_reload(&config);
                if (config.scale - old_scale).abs() > 0.01 {
                    eprintln!("Scale changed, redrawing...");
                    app.scale_changed = true;
                }
            }

            Some(control_event) = control_rx.recv() => {
                match control_event {
                    ControlEvent::Show { message, duration } => {
                        eprintln!("DBus: Show '{}' for {}s", message, duration);
                        state.on_show_control(&mut app, &config, &message, duration);
                        hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(duration)));
                    }
                    ControlEvent::Hide => {
                        eprintln!("DBus: Hide");
                        state.on_hide_control(&mut app);
                        hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(HIDE_TIMEOUT_SECS)));
                    }
                    ControlEvent::Reload => {
                        eprintln!("DBus: Reload config");
                        config = AppConfig::load();
                        eprintln!("inno: reloaded {} signals", config.signals.len());
                        app.frame_cache.clear();
                        state.on_config_reload(&config);
                    }
                }
            }

            Some(event) = rx.recv() => {
                match event {
                    Event::Notify(notify_event) => {
                        if let Some(delay) = state.process_notify(
                            &mut app,
                            &config,
                            &sound_worker,
                            &notify_event,
                            &battery_percentage,
                            &battery_state_shared,
                        ) {
                            hide_timer = Box::pin(tokio::time::sleep(delay));
                        }
                    }
                }
            }

            _ = &mut test_timer, if test_all_animations => {
                let anim = test_animations_list[test_anim_idx].clone();
                let anim_name = format!("{:?}", anim);
                eprintln!("Testing animation: {}", anim_name);

                let test_signal = config::Signal {
                    message: format!("Testing {}", anim_name),
                    icon: "󰚗".to_string(),
                    icon_size: 24.0,
                    color: (0.2, 0.8, 0.2, 1.0),
                    threshold: 0.0,
                    state_filter: "any".to_string(),
                    animation: anim,
                    duration: 10,
                    sound: None,
                };

                let text = draw::format_text(
                    &config.format_template,
                    &test_signal.icon,
                    &test_signal.message,
                    Some(50.0),
                );

                state.current_text = Some(text.clone());
                state.draw_state.reset();
                app.draw_text_with_signal(&text, &config, Some(&test_signal), &state.draw_state);
                current_test_signal = Some(test_signal);
                hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(10)));

                if let Some(fixed_idx) = test_animation {
                    test_anim_idx = fixed_idx;
                    test_timer = Box::pin(tokio::time::sleep(Duration::from_secs(HIDE_TIMEOUT_SECS)));
                } else {
                    test_anim_idx = (test_anim_idx + 1) % test_animations_list.len();
                    test_timer = Box::pin(tokio::time::sleep(Duration::from_secs(12)));
                }
                state.animating = true;
            }

            _ = &mut animation_timer, if state.animating => {
                if let Some(text) = &state.current_text {
                    if test_all_animations {
                        if let Some(ref sig) = current_test_signal {
                            let total_frames = sig.duration as f64 * config.fps as f64;
                            state.draw_state.tick(&sig.animation, total_frames, config.fps as f64);
                            app.draw_text_with_signal(text, &config, Some(sig), &state.draw_state);
                        }
                    } else if let Some(idx) = state.current_signal_idx {
                        let signal = &config.signals[idx];
                        let total_frames = signal.duration as f64 * config.fps as f64;
                        state.draw_state.tick(&signal.animation, total_frames, config.fps as f64);
                        app.draw_text_with_signal(text, &config, Some(signal), &state.draw_state);
                    }
                }
                animation_timer = Box::pin(tokio::time::sleep(Duration::from_micros(1_000_000 / config.fps.max(1))));
            }

            _ = &mut hide_timer => {
                if state.current_text.is_some() {
                    println!("Auto-hiding");
                    let delay = state.hide_and_next(&mut app, &config, &sound_worker);
                    hide_timer = Box::pin(tokio::time::sleep(delay));

                    if test_animation.is_some() {
                        println!("Specific test completed, exiting.");
                        break;
                    }
                }
            }

            guard = async_fd.readable() => {
                match guard {
                    Ok(mut guard) => {
                        guard.clear_ready();

                        if let Some(read_guard) = conn.prepare_read() {
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
                    }
                    Err(_) => break,
                }
            }
        }
    }

    Ok(())
}
