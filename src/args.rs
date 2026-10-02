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

    Args { action, debug_mode, enable_dbus, log_file, test_animation, test_all_animations, test_frame_anim }
}

pub fn help_text() -> &'static str {
    HELP
}
