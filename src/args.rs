use std::path::PathBuf;

pub enum Action {
    Help,
    Version,
    Daemon,
    InternalDaemon,
    CheckConfig,
}

pub struct Args {
    pub action: Action,
    pub debug_mode: bool,
    pub enable_dbus: bool,
    pub log_file: Option<PathBuf>,
    pub test_animation: Option<usize>,
    pub test_all_animations: bool,
    pub test_frame_anim: Option<String>,
    pub no_sound: bool,
}

const HELP: &str = r#"inno - Wayland notification daemon with configurable DBus events

USAGE:
    inno [OPTIONS]

OPTIONS:
    -h, --help              Show this help message
    -v, --version           Show version
    -d, --debug             Run in debug mode (spitting logs to terminal)
    --daemon                Run in background (daemon mode)
    -l, --log-file <PATH>   Log output to file (useful with --daemon)
    --no-dbus               Disable DBus control interface
    --test <number>         Preview specific procedural animation (1-6)
    --test-animations       Cycle through all procedural animations
    --test-frame <name>     Preview a frame animation from [animations] config
    --check-config          Validate config and exit
    --no-sound              Disable notification sounds

CONFIG:
    ~/.config/inno/inno.toml   (main config)
    ~/.config/inno/events/     (event definitions)

DBUS CONTROL:
    busctl --user call org.inno.Control /org/inno/Control org.inno.Control Show "st" "Hello" 5
    busctl --user call org.inno.Control /org/inno/Control org.inno.Control Hide
"#;

/// Reads the value that follows a flag, refusing to swallow the next flag.
fn value_after(args: &[String], flag_idx: usize) -> Option<String> {
    let next = args.get(flag_idx + 1)?;
    (!next.starts_with('-')).then(|| next.clone())
}

pub fn parse() -> Args {
    parse_from(std::env::args())
}

pub fn parse_from<I: IntoIterator<Item = String>>(args: I) -> Args {
    let args: Vec<String> = args.into_iter().collect();

    let mut help = false;
    let mut version = false;
    let mut check_config = false;
    let mut daemon = false;
    let mut internal_daemon = false;
    let mut debug_mode = false;
    let mut enable_dbus = true;
    let mut log_file: Option<PathBuf> = None;
    let mut test_animation: Option<usize> = None;
    let mut test_all_animations = false;
    let mut test_frame_anim: Option<String> = None;
    let mut no_sound = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => help = true,
            "-v" | "--version" => version = true,
            "-d" | "--debug" => debug_mode = true,
            "--daemon" => daemon = true,
            "--internal-daemon" => internal_daemon = true,
            "-l" | "--log-file" => log_file = value_after(&args, i).map(PathBuf::from),
            "--no-dbus" => enable_dbus = false,
            "--test" => {
                if let Some(value) = value_after(&args, i)
                    && let Ok(idx) = value.parse::<usize>()
                    && (1..=6).contains(&idx)
                {
                    test_animation = Some(idx - 1);
                    debug_mode = true;
                }
                // Skip the value slot when it parsed as something other than a
                // number, otherwise `--test abc --debug` still sees --debug.
                if args.get(i + 1).is_some_and(|v| !v.starts_with('-')) {
                    i += 1;
                }
            }
            "--test-animations" => {
                test_all_animations = true;
                debug_mode = true;
            }
            "--test-frame" => {
                if let Some(value) = value_after(&args, i) {
                    test_frame_anim = Some(value);
                    debug_mode = true;
                }
                if args.get(i + 1).is_some_and(|v| !v.starts_with('-')) {
                    i += 1;
                }
            }
            "--check-config" => check_config = true,
            "--no-sound" => no_sound = true,
            _ => {}
        }
        i += 1;
    }

    // Resolved after the loop so precedence does not depend on argument order.
    // --internal-daemon beats --daemon so re-spawning cannot fork-bomb.
    let action = if help {
        Action::Help
    } else if version {
        Action::Version
    } else if check_config {
        Action::CheckConfig
    } else if internal_daemon {
        Action::InternalDaemon
    } else if daemon {
        Action::Daemon
    } else {
        Action::InternalDaemon
    };

    if test_animation.is_some() {
        test_all_animations = true;
    }

    // A frame preview and the procedural cycle are mutually exclusive; letting
    // both stand would silently disable the cycle.
    if test_frame_anim.is_some() {
        test_all_animations = false;
    }

    Args {
        action,
        debug_mode,
        enable_dbus,
        log_file,
        test_animation,
        test_all_animations,
        test_frame_anim,
        no_sound,
    }
}

pub fn help_text() -> &'static str {
    HELP
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<String> {
        std::iter::once("inno".to_string())
            .chain(args.iter().map(|s| (*s).to_string()))
            .collect()
    }

    fn is_internal(a: &Action) -> bool {
        matches!(a, Action::InternalDaemon)
    }

    #[test]
    fn test_no_args_defaults_to_internal_daemon() {
        let args = parse_from(argv(&[]));
        assert!(is_internal(&args.action));
        assert!(!args.debug_mode);
        assert!(args.enable_dbus);
        assert!(args.log_file.is_none());
        assert!(args.test_animation.is_none());
        assert!(!args.test_all_animations);
        assert!(args.test_frame_anim.is_none());
        assert!(!args.no_sound);
    }

    #[test]
    fn test_help_and_version_win_regardless_of_position() {
        for args in [vec!["--check-config", "--help"], vec!["--help", "--check-config"]] {
            let parsed = parse_from(argv(&args));
            assert!(matches!(parsed.action, Action::Help), "for {:?}", args);
        }
        for args in [vec!["-v", "--check-config"], vec!["--check-config", "-v"]] {
            let parsed = parse_from(argv(&args));
            assert!(matches!(parsed.action, Action::Version), "for {:?}", args);
        }
    }

    #[test]
    fn test_check_config_overrides_daemon_mode() {
        let parsed = parse_from(argv(&["--daemon", "--check-config"]));
        assert!(matches!(parsed.action, Action::CheckConfig));
    }

    #[test]
    fn test_internal_daemon_beats_daemon_so_respawn_cannot_loop() {
        let parsed = parse_from(argv(&["--daemon", "--internal-daemon"]));
        assert!(is_internal(&parsed.action));
    }

    #[test]
    fn test_value_flags_do_not_swallow_the_next_flag() {
        // --test-frame --no-dbus used to consume "--no-dbus" as the value and
        // silently leave the DBus control interface enabled.
        let parsed = parse_from(argv(&["--test-frame", "--no-dbus"]));
        assert_eq!(parsed.test_frame_anim, None);
        assert!(!parsed.enable_dbus);

        let parsed = parse_from(argv(&["-l", "--no-dbus"]));
        assert!(parsed.log_file.is_none());
        assert!(!parsed.enable_dbus);
    }

    #[test]
    fn test_value_flag_does_not_hide_a_later_flag() {
        let parsed = parse_from(argv(&["--test-frame", "ripple", "--no-dbus"]));
        assert_eq!(parsed.test_frame_anim.as_deref(), Some("ripple"));
        assert!(!parsed.enable_dbus);
        assert!(parsed.debug_mode);
    }

    #[test]
    fn test_test_frame_sets_name_and_debug_mode() {
        let parsed = parse_from(argv(&["--test-frame", "cube_charge"]));
        assert_eq!(parsed.test_frame_anim.as_deref(), Some("cube_charge"));
        assert!(parsed.debug_mode);
        assert!(!parsed.test_all_animations);
        assert!(is_internal(&parsed.action));
    }

    #[test]
    fn test_test_frame_without_a_value_is_ignored() {
        let parsed = parse_from(argv(&["--test-frame"]));
        assert_eq!(parsed.test_frame_anim, None);
        assert!(!parsed.debug_mode);
    }

    #[test]
    fn test_log_file_captures_its_value() {
        let parsed = parse_from(argv(&["-l", "/tmp/inno.log"]));
        assert_eq!(parsed.log_file, Some(PathBuf::from("/tmp/inno.log")));
    }

    #[test]
    fn test_test_flag_accepts_1_to_6_and_maps_to_zero_based_index() {
        for (given, expected) in [("1", 0), ("3", 2), ("6", 5)] {
            let parsed = parse_from(argv(&["--test", given]));
            assert_eq!(parsed.test_animation, Some(expected), "for --test {}", given);
        }
    }

    #[test]
    fn test_test_flag_rejects_out_of_range_and_non_numeric() {
        for bad in ["0", "7", "abc", "-1"] {
            let parsed = parse_from(argv(&["--test", bad]));
            assert_eq!(parsed.test_animation, None, "for --test {}", bad);
        }
    }

    #[test]
    fn test_test_flag_does_not_eat_a_following_flag() {
        let parsed = parse_from(argv(&["--test", "abc", "--no-dbus"]));
        assert_eq!(parsed.test_animation, None);
        assert!(!parsed.enable_dbus);
    }

    #[test]
    fn test_specific_test_implies_test_all_animations() {
        let parsed = parse_from(argv(&["--test", "2"]));
        assert!(parsed.test_all_animations);
    }

    #[test]
    fn test_frame_preview_disables_the_procedural_cycle() {
        // Both flags standing would silently disable the cycle, since the frame
        // branch is checked first.
        let parsed = parse_from(argv(&["--test-animations", "--test-frame", "cube"]));
        assert_eq!(parsed.test_frame_anim.as_deref(), Some("cube"));
        assert!(!parsed.test_all_animations);
    }

    #[test]
    fn test_no_sound_is_a_flag_with_no_value() {
        let parsed = parse_from(argv(&["--no-sound"]));
        assert!(parsed.no_sound);
        assert!(parsed.enable_dbus, "--no-sound must not disable dbus");
    }

    #[test]
    fn test_unknown_flags_are_ignored() {
        let parsed = parse_from(argv(&["--nonsense", "--no-dbus", "--debug"]));
        assert!(!parsed.enable_dbus);
        assert!(parsed.debug_mode);
        assert!(is_internal(&parsed.action));
    }

    #[test]
    fn test_help_text_mentions_every_flag_parse_accepts() {
        let help = help_text();
        for flag in [
            "--help", "--version", "--debug", "--daemon", "--log-file", "--no-dbus",
            "--test-animations", "--test-frame", "--check-config", "--no-sound",
        ] {
            assert!(help.contains(flag), "help text is missing {}", flag);
        }
    }
}
