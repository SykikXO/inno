use clap::Parser;
use std::path::PathBuf;

pub enum Action {
    Daemon,
    InternalDaemon,
    CheckConfig,
}

pub struct Args {
    pub action: Action,
    pub enable_dbus: bool,
    pub log_file: Option<PathBuf>,
    pub test_animation: Option<usize>,
    pub test_all_animations: bool,
    pub test_frame_anim: Option<String>,
    pub test_signal: Option<String>,
    pub no_sound: bool,
}

const AFTER_HELP: &str = "\
CONFIG:
    ~/.config/inno/inno.toml   (main config)
    ~/.config/inno/events/     (event definitions)

DBUS CONTROL:
    busctl --user call org.inno.Control /org/inno/Control org.inno.Control Show \"st\" \"Hello\" 5
    busctl --user call org.inno.Control /org/inno/Control org.inno.Control Hide";

/// Parses `--test` into a zero-based index into `Animation::TEST_VARIANTS`. The
/// range comes from the list itself, so adding a variant cannot leave the parser
/// quietly rejecting it.
fn test_variant(s: &str) -> Result<usize, String> {
    let variants = crate::config::Animation::TEST_VARIANTS;
    let n: usize = s.parse().map_err(|_| format!("`{s}` is not a number"))?;
    match (1..=variants.len()).contains(&n) {
        true => Ok(n - 1),
        false => Err(format!("valid variants are 1-{}", variants.len())),
    }
}

#[derive(Parser, Debug)]
#[command(name = "inno", version, about = "Wayland notification daemon with configurable DBus events", after_help = AFTER_HELP, disable_help_flag = true, disable_version_flag = true)]
struct Cli {
    #[arg(short, long, action = clap::ArgAction::Help)]
    help: Option<bool>,

    #[arg(short = 'v', long, action = clap::ArgAction::Version)]
    version: Option<bool>,

    /// Run in background (daemon mode)
    #[arg(long)]
    daemon: bool,

    /// Set by --daemon when it re-spawns itself. Hidden because nobody types it.
    #[arg(long, hide = true)]
    internal_daemon: bool,

    /// Log output to file (useful with --daemon)
    #[arg(short, long, value_name = "PATH")]
    log_file: Option<PathBuf>,

    /// Disable DBus control interface
    #[arg(long)]
    no_dbus: bool,

    /// Preview specific procedural animation (1-6)
    #[arg(long, value_name = "number", value_parser = test_variant)]
    test: Option<usize>,

    /// Cycle through all procedural animations
    #[arg(long)]
    test_animations: bool,

    /// Preview a frame animation from [animations] config
    #[arg(long, value_name = "name")]
    test_frame: Option<String>,

    /// Preview the signal whose message matches <text>
    #[arg(long, value_name = "text")]
    test_signal: Option<String>,

    /// Validate config and exit
    #[arg(long)]
    check_config: bool,

    /// Disable notification sounds
    #[arg(long)]
    no_sound: bool,
}

pub fn parse() -> Args {
    parse_from(std::env::args())
}

pub fn parse_from<I: IntoIterator<Item = String>>(args: I) -> Args {
    match Cli::try_parse_from(args) {
        Ok(cli) => cli.into(),
        // --help and --version land here too, and clap has already printed them.
        Err(e) => e.exit(),
    }
}

impl From<Cli> for Args {
    fn from(cli: Cli) -> Self {
        let test_animation = cli.test;
        let test_signal = cli.test_signal;
        // A signal preview is a third mode: it excludes the frame preview too.
        let test_frame_anim = if test_signal.is_some() { None } else { cli.test_frame };
        // Precedence runs signal, then frame, then the procedural cycle, so a
        // mode that stands alongside another cannot be silently dropped.
        let test_all_animations = if test_frame_anim.is_some() || test_signal.is_some() {
            false
        } else {
            cli.test_animations || test_animation.is_some()
        };

        Args {
            // Resolved so precedence does not depend on argument order.
            // --internal-daemon beats --daemon so re-spawning cannot fork-bomb.
            action: if cli.check_config {
                Action::CheckConfig
            } else if cli.internal_daemon || !cli.daemon {
                Action::InternalDaemon
            } else {
                Action::Daemon
            },
            enable_dbus: !cli.no_dbus,
            log_file: cli.log_file,
            test_animation,
            test_all_animations,
            test_frame_anim,
            test_signal,
            no_sound: cli.no_sound,
        }
    }
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

    /// Parses without exiting the test runner, so rejections can be asserted.
    fn try_parse(args: &[&str]) -> Result<Args, clap::Error> {
        Cli::try_parse_from(argv(args)).map(Into::into)
    }

    #[test]
    fn test_no_args_defaults_to_internal_daemon() {
        let args = parse_from(argv(&[]));
        assert!(is_internal(&args.action));
        assert!(args.enable_dbus);
        assert!(args.log_file.is_none());
        assert!(args.test_animation.is_none());
        assert!(!args.test_all_animations);
        assert!(args.test_frame_anim.is_none());
        assert!(args.test_signal.is_none());
        assert!(!args.no_sound);
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
        assert!(try_parse(&["--test-frame", "--no-dbus"]).is_err());

        let parsed = parse_from(argv(&["--test-frame=--no-dbus", "--no-dbus"]));
        assert_eq!(parsed.test_frame_anim.as_deref(), Some("--no-dbus"));
        assert!(!parsed.enable_dbus);
    }

    #[test]
    fn test_value_flag_does_not_hide_a_later_flag() {
        let parsed = parse_from(argv(&["--test-frame", "ripple", "--no-dbus"]));
        assert_eq!(parsed.test_frame_anim.as_deref(), Some("ripple"));
        assert!(!parsed.enable_dbus);
    }

    #[test]
    fn test_test_frame_sets_the_preview_name() {
        let parsed = parse_from(argv(&["--test-frame", "cube_charge"]));
        assert_eq!(parsed.test_frame_anim.as_deref(), Some("cube_charge"));
        assert!(!parsed.test_all_animations);
        assert!(is_internal(&parsed.action));
    }

    #[test]
    fn test_log_file_captures_its_value() {
        let parsed = parse_from(argv(&["-l", "/tmp/inno.log"]));
        assert_eq!(parsed.log_file, Some(PathBuf::from("/tmp/inno.log")));
    }

    #[test]
    fn test_test_flag_maps_one_based_input_to_a_zero_based_index() {
        for (given, expected) in [("1", 0), ("3", 2), ("6", 5)] {
            let parsed = parse_from(argv(&["--test", given]));
            assert_eq!(parsed.test_animation, Some(expected), "for --test {}", given);
        }
    }

    #[test]
    fn test_test_flag_rejects_out_of_range_and_non_numeric() {
        for bad in ["0", "7", "abc", "-1"] {
            assert!(try_parse(&["--test", bad]).is_err(), "--test {} should be rejected", bad);
        }
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
    fn test_test_signal_selects_by_message_and_excludes_other_modes() {
        let parsed = parse_from(argv(&["--test-signal", "Charging"]));
        assert_eq!(parsed.test_signal.as_deref(), Some("Charging"));
        assert!(!parsed.test_all_animations);
        assert_eq!(parsed.test_frame_anim, None, "must not also run a frame preview");
    }

    #[test]
    fn test_test_signal_does_not_swallow_a_following_flag() {
        let parsed = parse_from(argv(&["--test-signal", "Charging", "--no-sound"]));
        assert_eq!(parsed.test_signal.as_deref(), Some("Charging"));
        assert!(parsed.no_sound, "--no-sound was swallowed as the value");
        assert!(parsed.enable_dbus, "--no-sound must not disable the control bus");

        let parsed = parse_from(argv(&["--test-signal", "Charging", "--no-dbus"]));
        assert_eq!(parsed.test_signal.as_deref(), Some("Charging"));
        assert!(!parsed.enable_dbus);
    }

    #[test]
    fn test_test_signal_frame_preview_are_exclusive() {
        let parsed = parse_from(argv(&["--test-frame", "cube", "--test-signal", "Charging"]));
        assert_eq!(parsed.test_signal.as_deref(), Some("Charging"));
        assert_eq!(parsed.test_frame_anim, None);
    }

    #[test]
    fn test_no_sound_is_a_flag_with_no_value() {
        let parsed = parse_from(argv(&["--no-sound"]));
        assert!(parsed.no_sound);
        assert!(parsed.enable_dbus, "--no-sound must not disable dbus");
    }

    #[test]
    fn test_unknown_flags_fail_loudly() {
        // A daemon autostarted from a systemd unit that silently ignores a
        // mistyped flag runs on defaults and looks healthy while doing nothing
        // the user asked for.
        let Err(err) = try_parse(&["--nonsense"]) else { panic!("--nonsense should be rejected") };
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
        assert!(try_parse(&["--no-soundd"]).is_err());
    }

    #[test]
    fn test_flag_equals_syntax_is_accepted() {
        let parsed = parse_from(argv(&["--test-signal=Charging"]));
        assert_eq!(parsed.test_signal.as_deref(), Some("Charging"));
    }

    #[test]
    fn test_test_variant_range_tracks_the_animation_list() {
        let count = crate::config::Animation::TEST_VARIANTS.len();
        assert!(test_variant("1").is_ok());
        assert!(test_variant(&count.to_string()).is_ok());
        assert!(test_variant(&(count + 1).to_string()).is_err());
        assert!(test_variant("0").is_err());
    }
}
