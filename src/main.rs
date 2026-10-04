use notify::{Event as FsEvent, RecursiveMode, Watcher};
use smithay_client_toolkit::reexports::client::Connection;
use std::fs::File;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::io::unix::AsyncFd;
use tokio::sync::mpsc;

mod animation;
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
#[cfg(test)]
mod testutil;

use args::{Action, Args};
use config::{AppConfig, HIDE_TIMEOUT_SECS};
use control::ControlEvent;
use dbus::Event;
use layer::{FrameTick, LayerApp};
use sound::SoundWorker;
use state::NotificationState;


/// Opt-in frame-by-frame tracing, used to verify playback rate and transitions
/// from a log rather than from pixels. Off unless INNO_TRACE is set, because it
/// prints on every frame.
static TRACE: std::sync::LazyLock<bool> =
    std::sync::LazyLock::new(|| std::env::var_os("INNO_TRACE").is_some());
static TRACE_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
static TICK_COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

thread_local! {
    /// The period the clock was last pointed at, for the trace line.
    static CURRENT_PERIOD: std::cell::RefCell<std::time::Duration> =
        const { std::cell::RefCell::new(std::time::Duration::ZERO) };
}

/// Period between animation frames at `fps`. The lower bound stops a zero fps
/// producing a zero period, which would spin the loop. The upper bound stops a
/// typo like 1000000 producing a 1us period, which spins it just as hard.
fn frame_period(fps: u64) -> Duration {
    Duration::from_micros(1_000_000 / fps.clamp(1, 240))
}

fn frame_delay(config: &AppConfig) -> Duration {
    frame_period(config.fps)
}

/// Loads the config and runs the checks `validate()` performs. Returns None when
/// the result is unusable, so a reload can keep the config it already had
/// rather than bricking the daemon on a half-typed file.
///
/// `validate()` used to run only under `--check-config`, which meant every
/// error it reports was unenforced in the daemon: `fps = 0` reached the frame
/// clock and ran it at the 240 Hz ceiling, and a bad animation path was only
/// discovered when a notification failed to draw.
fn load_config() -> Option<AppConfig> {
    let cfg = AppConfig::load();
    let (errors, warnings) = cfg.validate();
    for w in &warnings {
        eprintln!("inno: config warning: {}", w);
    }
    if errors.is_empty() {
        return Some(cfg);
    }
    for e in &errors {
        eprintln!("inno: config error: {}", e);
    }
    None
}

/// Applies a new config to the running daemon. Both reload paths go through
/// here: they used to be two copies that had drifted, so a `general.fps` or
/// `general.scale` edit applied over DBus silently did not take effect while
/// the same edit saved to disk did.
fn reload_config(
    config: &mut AppConfig,
    app: &mut LayerApp,
    state: &mut NotificationState,
    animation_timer: &mut tokio::time::Interval,
) {
    let old_scale = config.scale;
    let old_anchor = config.anchor.clone();
    let Some(new) = load_config() else { return };
    *config = new;
    eprintln!("inno: reloaded {} signals", config.signals.len());
    app.frame_cache.clear();
    app.clear_animations();
    set_frame_clock(animation_timer, frame_delay(config));
    state.on_config_reload(app, config);
    // The compositor is told the margins once, when the layer surface is
    // created, so a new anchor or margin needs re-applying. scale_changed is the
    // existing flag that drives update_scale_margins plus the redraw that a
    // geometry change wants anyway, so both cases share one path.
    if (config.scale - old_scale).abs() > 0.01 {
        eprintln!("Scale changed, redrawing...");
        app.scale_changed = true;
    } else if config.anchor != old_anchor {
        eprintln!("Position changed, redrawing...");
        app.scale_changed = true;
    }
}

/// Resolves the signal driving the current notification.
///
/// Test modes substitute a synthetic signal. Otherwise it is whichever config
/// signal the event matched, which may be stale after a config reload. Kept in
/// one place because two call sites need it and they used to disagree: a
/// notification raised without a matching signal has no index, and reading the
/// index alone silently dropped its animation on a scale change.
fn active_signal<'a>(
    state: &NotificationState,
    config: &'a config::AppConfig,
    test_signal: Option<&'a config::Signal>,
) -> Option<&'a config::Signal> {
    if let Some(signal) = test_signal {
        return Some(signal);
    }
    state
        .current_signal_idx
        .filter(|&idx| idx < config.signals.len())
        .map(|idx| &config.signals[idx])
}

/// Parks or arms the reminder timer to match the state a reminder is in.
///
/// Arming a `Sleep` to the idle value rather than dropping it keeps the
/// `select!` arm alive, so there is no way to leave a completed timer sitting
/// there ready to fire every loop iteration.
fn arm_reminder(
    timer: &mut std::pin::Pin<Box<tokio::time::Sleep>>,
    state: &NotificationState,
) {
    let delay = state.reminder.map_or(HIDE_TIMEOUT_SECS, |r| r.every.as_secs());
    *timer = Box::pin(tokio::time::sleep(Duration::from_secs(delay)));
}

/// Points the frame clock at a new period.
///
/// Sleeping for the frame period *after* drawing made the real period
/// `render + 1/fps`, and the shortfall accumulated every frame instead of being
/// absorbed, so animations played slow and ran long. Anchoring to deadlines
/// keeps the rate, and skipping missed ticks means a slow frame drops a frame
/// rather than falling behind for good.
fn set_frame_clock(clock: &mut tokio::time::Interval, period: Duration) {
    if clock.period() == period {
        return;
    }
    *clock = tokio::time::interval(period);
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // A fresh interval's first tick is already due, which would fire a frame
    // immediately on every speed change. reset() pushes it out a full period.
    clock.reset();
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let Args {
        action,
        enable_dbus,
        log_file,
        test_animation,
        test_all_animations,
        test_frame_anim,
        test_signal,
        no_sound,
    } = args::parse();

    match action {
        Action::Daemon => {
            // Validated here as well as in the child, so a bad config fails in the
            // user's terminal with a non-zero status. The child re-executes and
            // exits, which the parent has no way to observe.
            let (errors, _) = AppConfig::load_quiet().validate();
            if !errors.is_empty() {
                for e in &errors {
                    eprintln!("inno: config error: {}", e);
                }
                eprintln!("inno: refusing to daemonize with an invalid config");
                std::process::exit(1);
            }
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
            let cfg = AppConfig::load_quiet();
            let event_cfgs = events::load_events_quiet();
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

    let mut config = match load_config() {
        Some(cfg) => cfg,
        None => {
            eprintln!("inno: refusing to start with an invalid config");
            std::process::exit(1);
        }
    };
    eprintln!("inno: loaded {} signals", config.signals.len());

    let event_configs = events::load_events();
    eprintln!("inno: loaded {} event configs", event_configs.len());

    let (tx, mut rx) = mpsc::channel(10);
    let (config_tx, mut config_rx) = mpsc::channel::<()>(1);
    let (control_tx, mut control_rx) = mpsc::channel::<ControlEvent>(10);

    // Probe once at startup and cache the winner. Re-probing per play would
    // fork a process just to find out it still does not work.
    let sound_worker = if no_sound || !config.sound {
        eprintln!("inno: sounds disabled");
        SoundWorker::disabled()
    } else {
        // Probing spawns up to four players with a 2s timeout each. On a
        // current-thread runtime that is up to 8s where org.freedesktop.Notifications
        // does not exist yet, because nothing else gets to run while it polls.
        let worker = tokio::task::spawn_blocking(SoundWorker::probe).await?;
        eprintln!("inno: sound backend: {}", worker.describe());
        worker
    };
    let mut sound_worker = sound_worker;

    let battery_percentage = Arc::new(AtomicU32::new(10000));
    let battery_state_shared = Arc::new(RwLock::new("unknown".to_string()));

    // Keep-alive: dropping this would disconnect the DBus control interface
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

            // Editors save by writing a temp file and renaming over the target,
            // which emits Create/Rename rather than Modify. Watching the parent
            // directory and filtering by name catches both, and keeps working
            // after the original inode is replaced.
            //
            // Resolved first, because a symlinked config otherwise watches the
            // link's directory for the link's name, and every write that goes
            // through the link lands on the target's directory under a different
            // name. That combination never fires. Asset paths still resolve
            // against the link's own directory, which is what a user copying a
            // config directory expects.
            let config_path = config_path.canonicalize().unwrap_or(config_path);
            let file_name = config_path.file_name().map(|n| n.to_os_string());
            let watch_dir = config_path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new("."))
                .to_path_buf();

            let mut watcher = notify::recommended_watcher(move |res: Result<FsEvent, _>| {
                if let Ok(event) = res
                    && event.paths.iter().any(|p| p.file_name() == file_name.as_deref())
                {
                    let _ = watcher_tx.send(());
                }
            })
            .ok();

            if let Some(ref mut w) = watcher {
                let _ = w.watch(&watch_dir, RecursiveMode::NonRecursive);
            }

            while let Ok(()) = watcher_rx.recv() {
                // A single save emits several events; coalesce them into one
                // reload.
                std::thread::sleep(Duration::from_millis(100));
                while watcher_rx.try_recv().is_ok() {}
                let _ = config_tx.blocking_send(());
            }
        });
    }

    if enable_dbus && !test_all_animations && test_frame_anim.is_none() {
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
    // Armed when a notification hides and that signal asked to be reminded. The
    // moment the card goes away is exactly when the reminder falls due, so it
    // rides on the hide path rather than needing a timer of its own running.
    let mut reminder_timer =
        Box::pin(tokio::time::sleep(Duration::from_secs(HIDE_TIMEOUT_SECS)));
    let mut hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(HIDE_TIMEOUT_SECS)));
    let mut animation_timer = tokio::time::interval(frame_delay(&config));
    animation_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // The first tick of a fresh interval is already due; push it out a period.
    animation_timer.reset();

    let test_animations_list = config::Animation::TEST_VARIANTS;
    let mut current_test_signal: Option<config::Signal> = None;
    let mut test_anim_idx = test_animation.unwrap_or(0);
    let mut test_timer = Box::pin(tokio::time::sleep(Duration::from_secs(0)));
    let test_frame_anim_name = test_frame_anim.clone();
    let test_signal_name = test_signal.clone();

    if test_all_animations || test_frame_anim.is_some() {
        eprintln!("Animation testing mode enabled.");
        state.animating = true;
    }

    // A synthetic notification driven straight from the config, so a signal's
    // transition and frame animation can be exercised without producing the
    // underlying DBus event.
    if let Some(ref wanted) = test_signal_name {
        let found = config.signals.iter().position(|s| s.message == *wanted)
            .or_else(|| config.signals.iter().position(|s| s.message.contains(wanted.as_str())));
        match found {
            Some(idx) => {
                eprintln!("Testing signal[{}] {:?}", idx, wanted);
                current_test_signal = Some(config.signals[idx].clone());
                // Best effort, and only for the placeholder: if the query fails
                // the preview still runs, it just leaves {percent} unfilled.
                let percentage = match dbus::battery_percentage_now().await {
                    Some(pct) => Some(pct),
                    None => {
                        eprintln!("Could not read the battery level, so {{percent}} will be omitted");
                        None
                    }
                };
                let delay =
                    state.show_test_signal(&mut app, &config, &config.signals[idx], idx, percentage);
                hide_timer = Box::pin(tokio::time::sleep(delay));
            }
            None => {
                eprintln!(
                    "No signal matches '{}'. Messages: {:?}",
                    wanted,
                    config.signals.iter().map(|s| &s.message).collect::<Vec<_>>()
                );
                return Ok(());
            }
        }
    }

    loop {
        event_queue.dispatch_pending(&mut app)?;

        if app.exit {
            break;
        }

        if app.scale_changed {
            app.scale_changed = false;
            app.update_scale_margins(&config);
            // Frames are decoded for a specific display size, so a scale change
            // invalidates them along with the text cache.
            app.frame_cache.clear();
            app.clear_animations();

            if let Some(ref text) = state.current_text {
                state.draw_state.reset();
                if let Some(signal) =
                    active_signal(&state, &config, current_test_signal.as_ref())
                {
                    // A frame animation and the procedural transition are
                    // independent, so either one keeps the timer running.
                    state.animating = signal.animation_ref.is_some()
                        || signal.animation != config::Animation::None;
                    // Drawing here would flash a text card over an
                    // animation-only notification; the timer redraws in about
                    // a frame instead.
                    if !state.animating {
                        app.draw_text_with_signal(text, &config, Some(signal), &state.draw_state);
                    }
                } else {
                    app.draw_text(text, &config);
                    state.animating = false;
                }
            }
        }

        // A hovered button is baked into the cached card, so moving onto or off
        // one has to force a redraw. Cheap: it only happens when the pointer
        // crosses into a different button, not on every motion event.
        if app.hover_dirty {
            app.hover_dirty = false;
            if let Some(text) = state.current_text.clone() {
                let signal = active_signal(&state, &config, current_test_signal.as_ref()).cloned();
                if state.is_banner {
                    app.draw_banner_surface(&text, &config, signal.as_ref(), &state.draw_state);
                } else {
                    app.draw_text_with_signal(&text, &config, signal.as_ref(), &state.draw_state);
                }
            }
        }

        if app.clicked {
            app.clicked = false;
            // An action runs instead of dismissing. It is the same trust level
            // as the sound field, which already spawns a process named in this
            // config, and it is spawned without waiting so a slow command cannot
            // freeze the notification loop.
            // A banner's buttons come first: it is full screen and may have no
            // card behind it to fall back to.
            if state.is_banner
                && let Some(idx) = app.clicked_banner_action.take()
                && let Some(command) = state
                    .current_signal_idx
                    .and_then(|i| config.signals.get(i))
                    .and_then(|sig| sig.actions.get(idx))
                    .map(|a| a.command.clone())
            {
                {
                    println!("Running banner action '{command}'");
                    tokio::task::spawn_blocking(move || {
                        let _ = std::process::Command::new("sh")
                            .arg("-c")
                            .arg(&command)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();
                    });
                    continue;
                }
            }
            if let Some(idx) = app.clicked_action.take()
                && let Some(command) = state
                    .current_signal_idx
                    .and_then(|i| config.signals.get(i))
                    .and_then(|sig| sig.actions.get(idx))
                    .map(|a| a.command.clone())
            {
                println!("Running action '{}'", command);
                tokio::task::spawn_blocking(move || {
                    let _ = std::process::Command::new("sh")
                        .arg("-c")
                        .arg(&command)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                });
                continue;
            }
            app.clicked_action = None;
            state.dismiss_by_click(&mut app);
            hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(HIDE_TIMEOUT_SECS)));
            if test_animation.is_some()
                || test_frame_anim_name.is_some()
                || test_signal_name.is_some()
            {
                println!("Dismissed test, exiting.");
                break;
            }
        }

        if let Err(e) = conn.flush() {
            eprintln!("Wayland flush error: {}", e);
        }

        tokio::select! {
            Some(()) = config_rx.recv() => {
                eprintln!("Config file changed, reloading...");
                reload_config(&mut config, &mut app, &mut state, &mut animation_timer);
            }

            Some(control_event) = control_rx.recv() => {
                match control_event {
                    ControlEvent::Show { message, duration } => {
                        eprintln!("DBus: Show '{}' for {}s", message, duration);
                        state.on_show_control(&mut app, &config, &message);
                        hide_timer = Box::pin(tokio::time::sleep(if duration == 0 {
                            Duration::MAX
                        } else {
                            Duration::from_secs(duration)
                        }));
                    }
                    ControlEvent::Hide => {
                        eprintln!("DBus: Hide");
                        state.hide_and_next(&mut app);
                        hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(HIDE_TIMEOUT_SECS)));
                    }
                    ControlEvent::Reload => {
                        eprintln!("DBus: Reload config");
                        reload_config(&mut config, &mut app, &mut state, &mut animation_timer);
                    }
                }
            }

            Some(Event { notify: notify_event }) = rx.recv() => {
                if let Some(delay) = state.process_notify(
                    &mut app,
                    &config,
                    &mut sound_worker,
                    &notify_event,
                    &battery_percentage,
                    &battery_state_shared,
                ) {
                    hide_timer = Box::pin(tokio::time::sleep(delay));
                }
            }

            _ = &mut test_timer, if test_all_animations || test_frame_anim_name.is_some() => {
                if let Some(ref frame_name) = test_frame_anim_name {
                    // Frame animation test mode (one-shot)
                    eprintln!("Testing frame animation: {}", frame_name);
                    let test_signal = config::Signal {
                        message: format!("Testing '{}'", frame_name),
                        icon: "󰚗".to_string(),
                        icon_size: 24.0,
                        color: (0.2, 0.8, 0.2, 1.0),
                        color_name: "".to_string(),
                        threshold: 0.0,
                        state_filter: "any".to_string(),
                        animation: config::Animation::None,
                        animation_ref: Some(frame_name.clone()),
                        duration: Some(30),
                        sound: None,
                        remind: None,
                        banner: false,
                        actions: Vec::new(),
                    };
                    let text = config::format_text(
                        &config.format,
                        &test_signal.icon,
                        &test_signal.message,
                        Some(50.0),
                    );
                    state.current_text = Some(text.clone());
                    state.draw_state.reset();
                    current_test_signal = Some(test_signal);

                    match config.animations.get(frame_name) {
                        // Load before drawing. The timer path ticks before it
                        // draws, so without this the preview opened on frame 1
                        // and, with lazy loading, on nothing at all.
                        Some(asset) => {
                            if app.ensure_animation_loaded(frame_name, asset, &config) {
                                app.reset_animation(frame_name);
                                app.draw_frame_anim(
                                    frame_name,
                                    &config,
                                    current_test_signal.as_ref(),
                                    &text,
                                    &state.draw_state,
                                    false,
                                );
                                hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(30)));
                            }
                        }
                        None => eprintln!(
                            "No animation named '{}' in config. Known: {:?}",
                            frame_name,
                            config.animations.keys().collect::<Vec<_>>()
                        ),
                    }

                    // Park the cycle; the animation_timer drives playback.
                    test_timer = Box::pin(tokio::time::sleep(Duration::from_secs(9999)));
                } else if test_all_animations {
                    // Existing procedural animation test mode
                    let anim = test_animations_list[test_anim_idx];
                    let anim_name = format!("{:?}", anim);
                    eprintln!("Testing animation: {}", anim_name);

                    let test_signal = config::Signal {
                        message: format!("Testing {}", anim_name),
                        icon: "󰚗".to_string(),
                        icon_size: 24.0,
                        color: (0.2, 0.8, 0.2, 1.0),
                        color_name: "".to_string(),
                        threshold: 0.0,
                        state_filter: "any".to_string(),
                        animation: anim,
                        animation_ref: None,
                        duration: Some(10),
                        sound: None,
                        remind: None,
                        banner: false,
                        actions: Vec::new(),
                    };

                    let text = config::format_text(
                        &config.format,
                        &test_signal.icon,
                        &test_signal.message,
                        Some(50.0),
                    );

                    state.current_text = Some(text.clone());
                    state.draw_state.reset();
                    app.draw_text_with_signal(&text, &config, Some(&test_signal), &state.draw_state);
                    current_test_signal = Some(test_signal);
                    // Don't set a competing hide_timer in test mode — the test_timer handles cycling.
                    // Setting one here with exact duration kills the fade-out before it completes.
                    hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(HIDE_TIMEOUT_SECS)));

                    if let Some(fixed_idx) = test_animation {
                        test_anim_idx = fixed_idx;
                        test_timer = Box::pin(tokio::time::sleep(Duration::from_secs(HIDE_TIMEOUT_SECS)));
                    } else {
                        test_anim_idx = (test_anim_idx + 1) % test_animations_list.len();
                        test_timer = Box::pin(tokio::time::sleep(Duration::from_secs(12)));
                    }
                    state.animating = true;
                }
            }

            _ = animation_timer.tick(), if state.animating => {
                let Some(text) = &state.current_text else {
                    continue;
                };

                // Only the test modes carry a synthetic signal; a real
                // notification resolves through the matched signal index.
                let synthetic = if test_frame_anim_name.is_some()
                    || test_signal_name.is_some()
                    || test_all_animations
                {
                    current_test_signal.as_ref()
                } else {
                    None
                };

                let Some(signal) = active_signal(&state, &config, synthetic) else {
                    // A DBus Show with no matching signal still shows text.
                    app.draw_text(text, &config);
                    continue;
                };

                let frame_anim_key = signal
                    .animation_ref
                    .as_deref()
                    .filter(|_| !test_all_animations);

                // Point the clock at whatever is about to be ticked: a frame
                // animation runs at its own rate, everything else at the general
                // one. Without this the general path inherits the last frame
                // animation's rate and plays back at the wrong speed.
                let wanted = match frame_anim_key.and_then(|key| config.animations.get(key)) {
                    Some(asset) => frame_period(asset.fps),
                    None => frame_delay(&config),
                };
                set_frame_clock(&mut animation_timer, wanted);
                if *TRACE {
                    let _ = TRACE_START.get_or_init(std::time::Instant::now);
                    CURRENT_PERIOD.with(|p| *p.borrow_mut() = wanted);
                }

                // The procedural transition advances on every tick, including
                // while a frame animation drives the content, which is what
                // lets the two compose.
                let total_frames = state::transition_frames(signal, &config);
                state.draw_state.tick(&signal.animation, total_frames, config.fps as f64);
                if *TRACE {
                    let frame = state.draw_state.frame;
                    let alpha = state.draw_state.alpha;
                    let (ox, oy) = (state.draw_state.offset_x, state.draw_state.offset_y);
                    let tick =
                        TICK_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    eprintln!(
                        "TRACE t={:.4} n={} frame={} alpha={:.3} off=({:.1},{:.1}) key={:?} anim={:?} total={:.0} period={:?}",
                        TRACE_START.get().map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0),
                        tick,
                        frame,
                        alpha,
                        ox,
                        oy,
                        frame_anim_key,
                        signal.animation,
                        total_frames,
                        CURRENT_PERIOD.with(|p| *p.borrow()),
                    );
                }

                if let Some(key) = frame_anim_key {
                    match app.draw_frame_anim(
                        key,
                        &config,
                        Some(signal),
                        text,
                        &state.draw_state,
                        true,
                    ) {
                        FrameTick::Continue => {
                            // The clock already runs at the animation's rate;
                            // re-arming it here would reintroduce the
                            // draw-then-sleep drift this replaced.
                            continue;
                        }
                        FrameTick::Finished(on_complete) => {
                            // A non-looping animation reached its last frame.
                            // Stop ticking rather than re-committing an
                            // identical frame for the rest of the notification.
                            state.animating = false;
                            if on_complete == config::OnComplete::Hide {
                                let delay = state.hide_and_next(&mut app);
                                hide_timer = Box::pin(tokio::time::sleep(delay));
                            }
                            continue;
                        }
                        FrameTick::Unavailable => {
                            eprintln!("Frame animation '{}' unavailable, showing text", key);
                            app.failed_animations_insert(key);
                            app.draw_text_with_signal(text, &config, Some(signal), &state.draw_state);
                            continue;
                        }
                    }
                }

                app.draw_text_with_signal(text, &config, Some(signal), &state.draw_state);
            }

            _ = &mut reminder_timer, if state.reminder.is_some() => {
                let Some(r) = state.reminder else { continue };
                // The index is a position in a config that may have been reloaded
                // since the reminder was armed.
                let Some(sig) = config.signals.get(r.signal_idx) else {
                    state.reminder = None;
                    reminder_timer = Box::pin(tokio::time::sleep(Duration::from_secs(HIDE_TIMEOUT_SECS)));
                    continue;
                };
                println!("Reminding after {}", r.every.as_secs());
                let idx = r.signal_idx;
                let sig = sig.clone();
                // Re-read rather than reusing the percentage from the first
                // showing: the point of a reminder is that things may have got
                // worse in the meantime.
                let pct = dbus::battery_percentage_now().await;
                let delay = state.show_reminder(&mut app, &config, idx, &sig, pct);
                hide_timer = Box::pin(tokio::time::sleep(delay));
                arm_reminder(&mut reminder_timer, &state);
            }

            _ = &mut hide_timer => {
                if state.current_text.is_some() {
                    let is_infinite = state.current_signal_idx
                        .and_then(|idx| config.signals.get(idx))
                        .is_some_and(|sig| state::display_seconds(sig, &config).is_none());

                    if is_infinite {
                        hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(HIDE_TIMEOUT_SECS)));
                    } else {
                        println!("Auto-hiding");
                        let delay = state.hide_and_next(&mut app);
                        hide_timer = Box::pin(tokio::time::sleep(delay));
                        arm_reminder(&mut reminder_timer, &state);

                        // A reminder outlives the card by design, so a test that
                        // wants to watch one repeat has to keep the process
                        // alive past the first hide. INNO_TEST_PERSIST says so.
                        let persisting = std::env::var_os("INNO_TEST_PERSIST").is_some();

                        if !persisting
                            && (test_animation.is_some()
                                || test_frame_anim_name.is_some()
                                || test_signal_name.is_some())
                        {
                            println!("Specific test completed, exiting.");
                            break;
                        }
                    }
                } else {
                    // Nothing to hide. The completed Sleep is Ready forever, so
                    // re-arm it or this branch spins the loop at 100% CPU.
                    hide_timer = Box::pin(tokio::time::sleep(Duration::from_secs(HIDE_TIMEOUT_SECS)));
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
