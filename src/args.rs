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
    --test <number>         Preview specific animation (1-6)
    --test-animations       Cycle through all animations for testing
    --check-config          Validate config and exit

CONFIG:
    ~/.config/inno/inno.toml   (main config)
    ~/.config/inno/events/     (event definitions)

DBUS CONTROL:
    busctl --user call org.inno.Control /org/inno/Control org.inno.Control Show "st" "Hello" 5
    busctl --user call org.inno.Control /org/inno/Control org.inno.Control Hide
"#;

pub fn parse() -> Args {
    let args: Vec<String> = std::env::args().collect();

    let mut action: Option<Action> = None;
    let mut debug_mode = false;
    let mut enable_dbus = true;
    let mut log_file: Option<PathBuf> = None;
    let mut test_animation: Option<usize> = None;
    let mut test_all_animations = false;
    let mut check_config = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                action = Some(Action::Help);
                break;
            }
            "-v" | "--version" => {
                action = Some(Action::Version);
                break;
            }
            "-d" | "--debug" => {
                debug_mode = true;
            }
            "--daemon" => {
                action = Some(Action::Daemon);
            }
            "--internal-daemon" => {
                action = Some(Action::InternalDaemon);
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
            "--test" => {
                i += 1;
                if i < args.len()
                    && let Ok(idx) = args[i].parse::<usize>()
                    && (1..=6).contains(&idx)
                {
                    test_animation = Some(idx - 1);
                    debug_mode = true;
                }
            }
            "--test-animations" => {
                test_all_animations = true;
                debug_mode = true;
            }
            "--check-config" => {
                check_config = true;
            }
            _ => {}
        }
        i += 1;
    }

    let action = if check_config {
        Action::CheckConfig
    } else {
        action.unwrap_or(Action::InternalDaemon)
    };

    if test_animation.is_some() {
        test_all_animations = true;
    }

    Args { action, debug_mode, enable_dbus, log_file, test_animation, test_all_animations }
}

pub fn help_text() -> &'static str {
    HELP
}
