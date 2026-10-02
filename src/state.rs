use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, RwLock};

use crate::battery::aggregate_battery_state;
use crate::config::{AppConfig, HIDE_TIMEOUT_SECS};
use crate::dbus::NotifyEvent;
use crate::draw::{DrawState, format_text};
use crate::layer::LayerApp;
use crate::sound::SoundWorker;

const MAX_STATE_ENTRIES: usize = 32;

/// Fallback display time when a signal sets neither `duration` nor a frame
/// animation to derive one from.
pub const DEFAULT_DURATION_SECS: u64 = 5;

/// Buffer added to the display time so a transition still playing when the hide
/// timer expires is not cut off mid-frame.
const TRANSITION_TAIL_MS: u64 = 500;

/// How long a notification stays up, in seconds, or `None` for indefinitely.
///
/// `duration = 0` means until dismissed. When omitted, a signal playing a frame
/// animation lasts exactly as long as the animation does, which is almost
/// always what someone animating a notification wants and never has to be kept
/// in sync by hand.
pub fn display_seconds(sig: &crate::config::Signal, config: &AppConfig) -> Option<f64> {
    if let Some(secs) = sig.duration {
        return (secs > 0).then_some(secs as f64);
    }

    let animated =
        sig.animation_ref.is_some() || sig.animation != crate::config::Animation::None;
    let from_animation = sig
        .animation_ref
        .as_deref()
        .and_then(|key| config.animations.get(key))
        .and_then(|asset| asset.natural_duration)
        .map(|length| length.as_secs_f64());

    match (animated, from_animation) {
        (true, Some(length)) => Some(length),
        _ => Some(DEFAULT_DURATION_SECS as f64),
    }
}

/// The hide delay for a notification.
pub fn notification_duration(sig: &crate::config::Signal, config: &AppConfig) -> std::time::Duration {
    match display_seconds(sig, config) {
        None => std::time::Duration::MAX,
        Some(secs) => std::time::Duration::from_millis(
            (secs * 1000.0) as u64 + TRANSITION_TAIL_MS,
        ),
    }
}

/// How many frames the procedural transition spans.
pub fn transition_frames(sig: &crate::config::Signal, config: &AppConfig) -> f64 {
    display_seconds(sig, config).unwrap_or(DEFAULT_DURATION_SECS as f64) * config.fps as f64
}

pub struct NotificationState {
    pub current_text: Option<String>,
    pub draw_state: DrawState,
    pub animating: bool,
    pub current_signal_idx: Option<usize>,
    pub battery_devices: HashMap<String, (f64, String)>,
    pub prev_battery_agg: Option<String>,
    pub prev_state: HashMap<String, Option<String>>,
    pub prev_signal_msg: HashMap<String, Option<String>>,
    pub state_key_order: VecDeque<String>,
}

impl NotificationState {
    pub fn new() -> Self {
        Self {
            current_text: None,
            draw_state: DrawState::default(),
            animating: false,
            current_signal_idx: None,
            battery_devices: HashMap::new(),
            prev_battery_agg: None,
            prev_state: HashMap::new(),
            prev_signal_msg: HashMap::new(),
            state_key_order: VecDeque::new(),
        }
    }

    pub fn process_notify(
        &mut self,
        app: &mut LayerApp,
        config: &AppConfig,
        sound_worker: &SoundWorker,
        notify_event: &NotifyEvent,
        battery_percentage: &Arc<AtomicU32>,
        battery_state_shared: &Arc<RwLock<String>>,
    ) -> Option<std::time::Duration> {
        let is_battery = notify_event.is_battery;

        let (pct_for_match, state) = if is_battery {
            let pct = notify_event.percentage.unwrap_or(100.0);
            let st = notify_event.state.clone().unwrap_or_else(|| "unknown".to_string());
            self.battery_devices.insert(notify_event.path.clone(), (pct, st));

            let (agg_pct, agg_state) = aggregate_battery_state(&self.battery_devices, &config.battery_mode);

            battery_percentage.store((agg_pct * 100.0) as u32, Ordering::Relaxed);
            if let Ok(mut s) = battery_state_shared.write() {
                *s = agg_state.clone();
            }

            (agg_pct, agg_state)
        } else {
            let pct = notify_event.percentage.unwrap_or(100.0);
            let st = notify_event.state.clone().unwrap_or_else(|| "unknown".to_string());

            (pct, st)
        };

        let sig_idx = config.find_signal_idx(pct_for_match, &state);
        let signal = sig_idx.map(|i| &config.signals[i]);
        let signal_msg = signal.map(|s| s.message.clone());

        let should_notify = if is_battery {
            let changed = self.prev_battery_agg.as_ref().map(|s| s != &state).unwrap_or(true);
            self.prev_battery_agg = Some(state.clone());
            changed
        } else {
            let state_key = format!("{}:{}", notify_event.event_name, notify_event.path);
            let prev_s = self.prev_state.get(&state_key).unwrap_or(&None);
            let prev_sig = self.prev_signal_msg.get(&state_key).unwrap_or(&None);
            let state_changed = prev_s.as_ref() != Some(&state);
            let signal_changed = prev_sig != &signal_msg;

            if state_changed || signal_changed {
                if self.prev_state.contains_key(&state_key) {
                    self.prev_state.insert(state_key.clone(), Some(state));
                    self.prev_signal_msg.insert(state_key, signal_msg);
                } else {
                    if self.state_key_order.len() >= MAX_STATE_ENTRIES
                        && let Some(oldest) = self.state_key_order.pop_front()
                    {
                        self.prev_state.remove(&oldest);
                        self.prev_signal_msg.remove(&oldest);
                    }
                    self.state_key_order.push_back(state_key.clone());
                    self.prev_state.insert(state_key.clone(), Some(state));
                    self.prev_signal_msg.insert(state_key, signal_msg);
                }
            }

            state_changed || signal_changed
        };

        if should_notify {
            if is_battery {
                println!("Notify (state change): {} {}", self.prev_battery_agg.as_ref().unwrap(), notify_event.event_name);
            } else if let Some(p) = notify_event.percentage {
                println!("Notify: {:.0}% {} ({})", p, notify_event.event_name, notify_event.path);
            } else {
                println!("Notify: {} ({})", notify_event.event_name, notify_event.path);
            }

            if let Some(sig) = signal {
                return Some(self.show_notification(app, config, sound_worker, sig, sig_idx, notify_event, pct_for_match));
            }
        }

        None
    }

    #[allow(clippy::too_many_arguments)]
    fn show_notification(
        &mut self,
        app: &mut LayerApp,
        config: &AppConfig,
        sound_worker: &SoundWorker,
        sig: &crate::config::Signal,
        sig_idx: Option<usize>,
        notify_event: &NotifyEvent,
        pct_for_match: f64,
    ) -> std::time::Duration {
        let dynamic_msg = sig.message.replace("{message}", &notify_event.message);
        let text = format_text(
            &config.format_template,
            &sig.icon,
            &dynamic_msg,
            Some(pct_for_match),
        );

        if let Some(ref sound_path) = sig.sound {
            sound_worker.play(sound_path);
        }

        self.draw_state.reset();
        self.current_signal_idx = sig_idx;
        self.current_text = Some(text.clone());

        // A frame animation wraps the procedural one, so a signal may name
        // both: `animation` supplies the transition, `animation_ref` the
        // content. Naming an [animations] key in `animation` is shorthand for
        // both fields pointing at that key.
        if let Some(ref anim_key) = sig.animation_ref {
            match config.animations.get(anim_key) {
                Some(asset) if app.ensure_animation_loaded(anim_key, asset, config) => {
                    app.reset_animation(anim_key);
                    self.animating = true;
                    app.draw_frame_anim(
                        anim_key,
                        config,
                        Some(sig),
                        &text,
                        &self.draw_state,
                        false,
                    );
                    return notification_duration(sig, config);
                }
                Some(_) => {
                    eprintln!("Animation '{}' failed to load", anim_key);
                }
                None => {
                    eprintln!("Animation '{}' not found in config", anim_key);
                }
            }
        }

        // Fall through to procedural animation
        app.draw_text_with_signal(&text, config, Some(sig), &self.draw_state);
        self.animating = sig.animation != crate::config::Animation::None;
        // Buffer after animation completes before hiding the surface.
        // Must be generous: the animation timer isn't reset on notification show,
        // and accumulated timer jitter over many frames can delay completion.
        notification_duration(sig, config)
    }

    pub fn hide_and_next(&mut self, app: &mut LayerApp) -> std::time::Duration {
        app.hide();
        self.current_text = None;
        self.animating = false;
        self.draw_state.reset();

        std::time::Duration::from_secs(HIDE_TIMEOUT_SECS)
    }

    pub fn dismiss_by_click(&mut self, app: &mut LayerApp) {
        if self.current_text.is_some() {
            println!("Dismissed by click");
            app.hide();
            self.current_text = None;
            self.animating = false;
            self.draw_state.reset();
        }
    }

    pub fn on_config_reload(&mut self, app: &mut LayerApp) {
        self.battery_devices.clear();
        self.prev_battery_agg = None;
        self.prev_state.clear();
        self.prev_signal_msg.clear();
        self.state_key_order.clear();
        // Both animation paths resolve the active signal through
        // current_signal_idx, which no longer refers to anything meaningful.
        // Leaving it cleared while animating stayed true froze the last frame
        // on screen and woke the timer to draw nothing, so drop the
        // notification instead.
        app.hide();
        self.current_text = None;
        self.current_signal_idx = None;
        self.animating = false;
        self.draw_state.reset();
    }

    pub fn on_hide_control(&mut self, app: &mut LayerApp) {
        app.hide();
        self.current_text = None;
        self.animating = false;
        self.draw_state.reset();
    }

    pub fn on_show_control(
        &mut self,
        app: &mut LayerApp,
        config: &AppConfig,
        message: &str,
    ) {
        self.draw_state.reset();
        app.draw_text(message, config);
        self.current_text = Some(message.to_string());
        self.animating = false;
    }
}
