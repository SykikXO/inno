use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, RwLock};

use crate::battery::aggregate_battery_state;
use crate::config::{AppConfig, HIDE_TIMEOUT_SECS};
use crate::dbus::NotifyEvent;
use crate::config::format_text;
use crate::draw::DrawState;
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

/// Whether a running animation can keep playing under a reloaded config.
///
/// True only when the signal at the same index still names the same frame
/// animation. Keeping the index lets playback continue and pick up the new
/// settings, which is what someone editing a font size expects.
fn reload_keeps_animation(
    current_idx: Option<usize>,
    current_anim_ref: Option<&str>,
    config: &AppConfig,
) -> bool {
    let Some(idx) = current_idx else { return false };
    config
        .signals
        .get(idx)
        .is_some_and(|sig| sig.animation_ref.as_deref() == current_anim_ref)
}

pub struct NotificationState {
    pub current_text: Option<String>,
    pub draw_state: DrawState,
    pub animating: bool,
    pub current_signal_idx: Option<usize>,
    /// Frame animation the active notification is playing. Kept so a config
    /// reload can tell whether the signal at that index is still the same one.
    pub current_anim_ref: Option<String>,
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
            current_anim_ref: None,
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
        sound_worker: &mut SoundWorker,
        notify_event: &NotifyEvent,
        battery_percentage: &Arc<AtomicU32>,
        battery_state_shared: &Arc<RwLock<String>>,
    ) -> Option<std::time::Duration> {
        let is_battery = notify_event.is_battery;

        // Two numbers, deliberately. `pct_for_match` always has a value because
        // threshold matching needs one. `display_pct` stays None when the event
        // carried no reading, because a fabricated plausible percentage renders
        // as a number the daemon did not measure: a Bluetooth connect with no
        // battery in it used to display "AirPods connected 100%".
        let (pct_for_match, display_pct, state) = if is_battery {
            let pct = notify_event.percentage.unwrap_or(100.0);
            let st = notify_event.state.clone().unwrap_or_else(|| "unknown".to_string());
            self.battery_devices.insert(notify_event.path.clone(), (pct, st));

            let (agg_pct, agg_state) = aggregate_battery_state(&self.battery_devices, &config.battery_mode);

            battery_percentage.store((agg_pct * 100.0) as u32, Ordering::Relaxed);
            // Recover from a poisoned lock rather than silently giving up: the
            // cached state is still readable, and control.rs already recovers on
            // its side, so the two disagreed about what a poisoned lock means.
            *battery_state_shared.write().unwrap_or_else(|e| e.into_inner()) = agg_state.clone();

            (agg_pct, Some(agg_pct), agg_state)
        } else {
            (notify_event.percentage.unwrap_or(100.0), notify_event.percentage, notify_event.state.clone().unwrap_or_else(|| "unknown".to_string()))
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
                return Some(self.show_notification(app, config, sound_worker, sig, sig_idx, notify_event, display_pct));
            }
        }

        None
    }

    /// Renders `sig` with `text` and reports how long the notification should
    /// stay up. The event path and `--test-signal` share it so both animate,
    /// buffer and hide identically.
    fn show(
        &mut self,
        app: &mut LayerApp,
        config: &AppConfig,
        sig: &crate::config::Signal,
        sig_idx: Option<usize>,
        text: &str,
    ) -> std::time::Duration {
        self.draw_state.reset();
        self.current_signal_idx = sig_idx;
        self.current_anim_ref = sig.animation_ref.clone();
        self.current_text = Some(text.to_string());

        // A frame animation wraps the procedural one, so a signal may name
        // both: `animation` supplies the transition, `animation_ref` the
        // content. Naming an [animations] key in `animation` is shorthand for
        // both fields pointing at that key.
        if let Some(anim_key) = sig.animation_ref.as_deref() {
            match config.animations.get(anim_key) {
                Some(asset) if app.ensure_animation_loaded(anim_key, asset, config) => {
                    app.reset_animation(anim_key);
                    self.animating = true;
                    app.draw_frame_anim(anim_key, config, Some(sig), text, &self.draw_state, false);
                    return notification_duration(sig, config);
                }
                Some(_) => eprintln!("Animation '{}' failed to load", anim_key),
                None => eprintln!("Animation '{}' not found in config", anim_key),
            }
        }

        // Fall through to procedural animation
        app.draw_text_with_signal(text, config, Some(sig), &self.draw_state);
        self.animating = sig.animation != crate::config::Animation::None;
        // Buffer after animation completes before hiding the surface.
        // Must be generous: the animation timer isn't reset on notification show,
        // and accumulated timer jitter over many frames can delay completion.
        notification_duration(sig, config)
    }

    #[allow(clippy::too_many_arguments)]
    fn show_notification(
        &mut self,
        app: &mut LayerApp,
        config: &AppConfig,
        sound_worker: &mut SoundWorker,
        sig: &crate::config::Signal,
        sig_idx: Option<usize>,
        notify_event: &NotifyEvent,
        pct: Option<f64>,
    ) -> std::time::Duration {
        let dynamic_msg = sig.message.replace("{message}", &notify_event.message);
        let text = format_text(&config.format, &sig.icon, &dynamic_msg, pct);

        if let Some(ref sound_path) = sig.sound {
            sound_worker.play(sound_path);
        }

        self.show(app, config, sig, sig_idx, &text)
    }

    /// Renders a configured signal on demand, as if its event had arrived.
    ///
    /// This is the same path a real notification takes, so it is also the only
    /// way to exercise a signal's transition and frame animation together
    /// without having to produce the underlying DBus event.
    pub fn show_test_signal(
        &mut self,
        app: &mut LayerApp,
        config: &AppConfig,
        sig: &crate::config::Signal,
        sig_idx: usize,
        percentage: Option<f64>,
    ) -> std::time::Duration {
        // Show the real battery level when one is known. A hardcoded placeholder
        // renders as a plausible reading, which is worse than showing nothing:
        // it looks like the daemon is reporting the wrong number.
        let text = format_text(&config.format, &sig.icon, &sig.message, percentage);
        self.show(app, config, sig, Some(sig_idx), &text)
    }

    pub fn hide_and_next(&mut self, app: &mut LayerApp) -> std::time::Duration {
        app.hide();
        self.current_text = None;
        self.current_anim_ref = None;
        self.animating = false;
        self.draw_state.reset();

        std::time::Duration::from_secs(HIDE_TIMEOUT_SECS)
    }

    pub fn dismiss_by_click(&mut self, app: &mut LayerApp) {
        if self.current_text.is_some() {
            println!("Dismissed by click");
            app.hide();
            self.current_text = None;
            self.current_anim_ref = None;
            self.animating = false;
            self.draw_state.reset();
        }
    }

    /// Handles a configuration reload.
    ///
    /// The matched signal index is cleared because it refers to the previous
    /// config's signal list. Leaving `animating` set while it was cleared is
    /// what used to freeze the last frame on screen and wake the timer to draw
    /// nothing, so animation stops here.
    ///
    /// The notification itself is kept and redrawn with the new config rather
    /// than hidden: it is still live, and making it vanish because someone
    /// edited a font size would be a poor trade for fixing a busy loop. Its
    /// existing hide timer is left alone.
    pub fn on_config_reload(&mut self, app: &mut LayerApp, config: &AppConfig) {
        self.battery_devices.clear();
        self.prev_battery_agg = None;
        self.prev_state.clear();
        self.prev_signal_msg.clear();
        self.state_key_order.clear();
        self.draw_state.reset();

        // A running animation keeps going when the new config still has the
        // same signal in the same place showing the same animation, which is the
        // common case: people edit a font size or a frame rate and expect what
        // is on screen to pick it up. Only when the signal list moved under us
        // does the animation stop.
        //
        // Clearing the index while leaving `animating` set is what used to
        // freeze the last frame on screen and wake the timer to draw nothing, so
        // the two are always decided together.
        if !reload_keeps_animation(self.current_signal_idx, self.current_anim_ref.as_deref(), config) {
            self.current_signal_idx = None;
            self.current_anim_ref = None;
            self.animating = false;
            // Still a live notification, so redraw it rather than hiding it.
            // Making it vanish because someone edited a config file would be a
            // poor trade for fixing a busy loop.
            if let Some(ref text) = self.current_text {
                app.draw_text(text, config);
            }
        }
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
        // This notification belongs to no signal. Leaving the previous index in
        // place made the caller read it as "an indefinite signal is showing" and
        // re-arm the hide timer to HIDE_TIMEOUT_SECS instead of what was asked.
        self.current_signal_idx = None;
        self.current_anim_ref = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Animation, AnimAsset, DisplayMode, OnComplete, Signal};
    use std::path::PathBuf;

    fn signal(anim_ref: Option<&str>) -> Signal {
        Signal {
            message: "x".into(),
            icon: String::new(),
            icon_size: 24.0,
            color: (1.0, 1.0, 1.0, 1.0),
            color_name: "white".into(),
            threshold: 0.0,
            state_filter: "any".into(),
            animation: Animation::None,
            animation_ref: anim_ref.map(str::to_string),
            duration: Some(5),
            sound: None,
        }
    }

    fn config_with(signals: Vec<Signal>) -> AppConfig {
        AppConfig { signals, ..Default::default() }
    }

    fn anim_asset() -> AnimAsset {
        AnimAsset {
            source: PathBuf::from("/test"),
            fps: 30,
            loop_: true,
            display: DisplayMode::Text,
            on_complete: OnComplete::Hold,
            natural_duration: None,
        }
    }

    #[test]
    fn test_reload_keeps_animation_when_the_signal_still_matches() {
        // Editing a font or a frame rate should not interrupt playback.
        let config = config_with(vec![signal(Some("cube")), signal(Some("ripple"))]);
        assert!(reload_keeps_animation(Some(1), Some("ripple"), &config));
        assert!(reload_keeps_animation(Some(0), Some("cube"), &config));
    }

    #[test]
    fn test_reload_drops_animation_when_the_signal_list_shrank() {
        let config = config_with(vec![signal(Some("cube"))]);
        assert!(!reload_keeps_animation(Some(3), Some("cube"), &config));
        assert!(!reload_keeps_animation(Some(1), Some("cube"), &config));
    }

    #[test]
    fn test_reload_drops_animation_when_that_signal_now_shows_something_else() {
        // The index survived but the signal under it was reordered or edited,
        // so continuing would play the wrong content.
        let config = config_with(vec![signal(None), signal(Some("cube"))]);
        assert!(!reload_keeps_animation(Some(0), Some("cube"), &config));
        assert!(reload_keeps_animation(Some(1), Some("cube"), &config));
    }

    #[test]
    fn test_reload_drops_animation_when_none_was_playing() {
        let config = config_with(vec![signal(None)]);
        assert!(!reload_keeps_animation(None, None, &config));
        // No index but an animation reference recorded is inconsistent and must
        // not be treated as still valid.
        assert!(!reload_keeps_animation(None, Some("cube"), &config));
    }

    #[test]
    fn test_reload_also_keeps_a_running_procedural_transition() {
        // A text-only notification has no animation_ref on either side, so they
        // match and a running fade survives the edit too.
        let config = config_with(vec![signal(None)]);
        assert!(reload_keeps_animation(Some(0), None, &config));
    }

    #[test]
    fn test_reload_decision_does_not_depend_on_animation_frame_rate() {
        // The frame rate is what usually changes in an edit; it must not be part
        // of the decision.
        let mut config = config_with(vec![signal(Some("cube"))]);
        assert!(reload_keeps_animation(Some(0), Some("cube"), &config));
        config.animations.insert("cube".into(), anim_asset());
        assert!(reload_keeps_animation(Some(0), Some("cube"), &config));
    }

    #[test]
    fn test_transition_frames_for_an_animated_signal() {
        let config = AppConfig { fps: 30, ..Default::default() };
        let sig = signal(Some("cube"));
        assert_eq!(transition_frames(&sig, &config), 5.0 * 30.0);
    }
}
