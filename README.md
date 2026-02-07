# Inno Notification Agent

Inno is a lightweight, event-driven notification agent for Wayland, written in Rust. It listens for configurable DBus events and displays non-intrusive notifications.

<p align="center">
  <img src="pictures/charging.png" width="300" alt="Charging Notification">
  <img src="pictures/discharging.png" width="300" alt="Discharging Notification">
</p>

## 🚀 Quick Start

```bash
# Build
cargo build --release

# Run
./target/release/inno

# Or install
cargo install --path .
```

## Installation

### Arch Linux (AUR)
```bash
yay -S inno
```

### Manual Build
Requirements: `rust`, `cargo`, `wayland`, `cairo`, `dbus`.

```bash
cargo build --release
sudo cp target/release/inno /usr/bin/
```

## Configuration

Inno uses TOML configuration files. Config search order:
1. `./inno.toml` (current directory)
2. `~/.config/inno/inno.toml` (user config)
3. `/etc/xdg/inno/inno.toml` (system config)

### Config File Format (`inno.toml`)

```toml
[general]
font = "Iosevka NFM"
font_size = 18.0
font_slant = "normal"    # normal, italic, oblique
font_weight = "normal"   # normal, bold
position = "center,bottom,10"
format = "{message} {percent}%"

[appearance]
text_color = [1.0, 1.0, 1.0, 1.0]  # RGBA
bg_color = [0.0, 0.0, 0.0, 0.7]
border_radius = 8.0
gradient = true

# Optional
# output = "primary"       # primary, all, or output name
# battery_mode = "first"   # first, combined, highest, lowest

# Named colors for signals
[colors]
green = [0.0, 1.0, 0.0, 1.0]
red = [1.0, 0.0, 0.0, 1.0]
orange = [1.0, 0.65, 0.0, 1.0]

# Notification signals
[[signal]]
message = "Charging"
icon = "󱐋"
icon_size = 28
color = "green"
threshold = 0
state = "charging"      # charging, discharging, full, any
animation = "fadein"    # none, fadein, fadeout, pulse, flicker, slide, bounce
duration = 2
# sound = "/path/to/sound.wav"  # optional
```

### Config Options

| Section | Key | Description |
|---------|-----|-------------|
| `[general]` | `font` | Font family name |
| | `font_size` | Font size in points |
| | `position` | Format: `horizontal,vertical,margin` (e.g., `center,bottom,10`) |
| | `format` | Text format with `{message}`, `{percent}` placeholders |
| `[appearance]` | `text_color` | RGBA array `[R, G, B, A]` (0.0-1.0) |
| | `bg_color` | Background RGBA |
| | `border_radius` | Corner radius in pixels |
| | `gradient` | Enable gradient background |
| `[[signal]]` | `threshold` | Battery percentage trigger point |
| | `state` | Battery state: `charging`, `discharging`, `full`, `any` |
| | `animation` | Animation type (see above) |
| | `duration` | Display duration in seconds |

---

## Custom DBus Event Listeners

Inno can listen for **any** DBus signal, not just battery events. Define custom events in `~/.config/inno/events/*.toml`.

### Event Search Paths
1. `./events/` (current directory)
2. `~/.config/inno/events/`
3. `/etc/xdg/inno/events/`

### Event TOML Format

```toml
# ~/.config/inno/events/bluetooth_volume.toml

name = "Bluetooth Volume"
enabled = true
bus = "session"  # or "system"

[match]
interface = "org.freedesktop.DBus.Properties"
member = "PropertiesChanged"
path_prefix = "/org/bluez"
# arg0 = "org.bluez.MediaTransport1"  # optional filter

[extract]
volume = "Volume"

[state_map]
# Map numeric values to strings (keys must be strings)
"0" = "muted"
"127" = "max"

[format]
message = "Volume: {volume}"

[conditions]
trigger_on = ["Volume"]  # empty = trigger on any change
debounce_ms = 200
require_all = false      # false = OR logic, true = AND logic
```

### Event Configuration Reference

| Section | Key | Description |
|---------|-----|-------------|
| Root | `name` | Event display name |
| | `enabled` | Enable/disable event |
| | `bus` | DBus type: `system` or `session` |
| `[match]` | `interface` | DBus interface to match |
| | `member` | Signal member name |
| | `path` | Exact object path |
| | `path_prefix` | Object path prefix match |
| | `arg0` | First argument filter |
| `[extract]` | `<name> = "<property>"` | Extract properties into variables |
| `[state_map]` | `"<value>" = "<string>"` | Map numeric values to strings |
| `[format]` | `message` | Format string with `{variable}` placeholders |
| `[conditions]` | `trigger_on` | Properties that trigger notification |
| | `debounce_ms` | Minimum ms between triggers |
| | `require_all` | AND (true) or OR (false) logic |

### Example: Battery Event (Default)

```toml
# events/battery.toml
name = "Battery"
bus = "system"

[match]
interface = "org.freedesktop.DBus.Properties"
member = "PropertiesChanged"
path_prefix = "/org/freedesktop/UPower/devices"
arg0 = "org.freedesktop.UPower.Device"

[extract]
percentage = "Percentage"
state = "State"

[state_map]
"1" = "charging"
"2" = "discharging"
"4" = "full"

[format]
message = "{percentage}%"

[conditions]
debounce_ms = 1000
```

---

## DBus Control Interface

Control Inno externally via DBus:

```bash
# Show notification
busctl --user call org.inno.Control /org/inno/Control org.inno.Control Show "st" "Hello World" 5

# Hide notification
busctl --user call org.inno.Control /org/inno/Control org.inno.Control Hide

# Get battery state
busctl --user call org.inno.Control /org/inno/Control org.inno.Control GetState

# Reload config
busctl --user call org.inno.Control /org/inno/Control org.inno.Control Reload
```

## Usage

```bash
inno                    # Run normally
inno -d                 # Daemon mode (detach)
inno -l /path/to.log    # Log to file
inno --no-dbus          # Disable control interface
inno -h                 # Help
```

### Systemd Integration
```bash
systemctl --user enable inno.service
systemctl --user start inno.service
```

## License

MIT
