# Inno Notification Agent

Inno is a lightweight, event-driven notification agent for Wayland, written in Rust. It listens for configurable DBus signals and displays non-intrusive, animated notifications.

<p align="center">
  <img src="assets/images/charging.png" width="300" alt="Charging Notification">
  <img src="assets/images/discharging.png" width="300" alt="Discharging Notification">
</p>

## Features

- **Wayland native** — wlr-layer-shell overlay, no compositor-specific code
- **Any DBus signal** — events are config files, not just battery
- **7 procedural transitions** — none, fade, pulse, blink, slide-left, slide-right, bounce
- **Frame animations** — play a directory of PNG frames, decoded on demand and at display size, so a long sequence costs almost nothing until it plays
- **Composable** — a procedural transition can wrap a frame animation, or stand alone
- **Bring your own frames** — point `source` at a directory of PNGs, any resolution
- **Sound with fallback** — probes `pw-play`, `paplay`, `ffplay`, `mpv` at startup and caches the winner
- **Config reload** — edit `inno.toml` and it applies, including swapping a frame animation's rate mid-playback
- **Click to dismiss**, **HiDPI aware**, **multi-battery aggregation**
- **`--check-config`** — validates config, animations and event files without starting

## Quick Start

```bash
cargo build --release

# Validate config and animations without starting
./target/release/inno --check-config

# See a notification without waiting for a battery event
./target/release/inno --test-signal Charging

# Run
./target/release/inno
```

Config is read from `./inno.toml`, then `~/.config/inno/inno.toml`, then
`/etc/xdg/inno/inno.toml`. Frame animations and sounds are resolved relative to
whichever file was used, so a user config wants its own copies:

```bash
mkdir -p ~/.config/inno/assets/{animations,sounds}
cp -r assets/animations/* ~/.config/inno/assets/animations/
cp    assets/sounds/*.wav  ~/.config/inno/assets/sounds/
```

## Installation

### Arch Linux (AUR)

```bash
yay -S inno
# or
paru -S inno
```

### Manual Build

Requirements: `rust`, `cargo`, `wayland`, `cairo`, `dbus`, `pipewire` (for sounds).

```bash
cargo build --release
sudo cp target/release/inno /usr/bin/
```

## Configuration

Inno uses TOML configuration files. Config search order:

1. `./inno.toml` (current directory)
2. `~/.config/inno/inno.toml` (user config)
3. `/etc/xdg/inno/inno.toml` (system config)

Events are loaded from `events/` in the same search paths.

Every relative path in the config, including animation directories and sound
files, resolves against the directory the config file was found in. Nothing needs
editing when the tree moves.

`inno --check-config` validates without starting the daemon. It reports missing
animation directories, frame directories containing no PNGs, unreadable
directories, zero frame rates, a frame rate high enough to be a typo, a
`loop = false` contradicted by `on_complete = "loop"`, and any signal pointing at
an `animation_ref` that names no such entry. Exit status is 1 if there are
errors.

### Config File Format (`inno.toml`)

The shipped `inno.toml` is commented throughout. The shape:

```toml
# A frame animation: a directory of PNG frames played as the notification body.
[animations]
cube_charge = { source = "assets/animations/cube_charging", fps = 30, loop = true, display = "text" }
ripple = { source = "assets/animations/ripple_charge", fps = 12, loop = true, display = "text" }

[general]
font = "InputMono Nerd Font"
font_size = 18.0
font_slant = "normal"    # normal, italic, oblique
font_weight = "normal"   # normal, bold
position = "center,bottom,0,90,0,0"   # anchor and margins
format = "{message} {percent}%"
fps = 60                 # procedural transitions only
# scale = 1.0            # display scale multiplier
# sound = false          # same as --no-sound

[appearance]
text_color = [1.0, 1.0, 1.0, 1.0]  # RGBA, 0.0-1.0
bg_color = [0.0, 0.0, 0.0, 0.7]
border_radius = 8.0
gradient = true

[colors]
green = [0.0, 1.0, 0.0, 1.0]

[[signal]]
message = "Charging"
icon = "󱐋"
icon_size = 28
color = "green"
threshold = 0
state = "charging"
animation = "fade"            # the transition
animation_ref = "cube_charge"  # the content; they compose
# duration omitted, so this lasts exactly as long as the animation
sound = "assets/sounds/hardware_insert.wav"
```

### Config Options

| Section | Key | Default | Description |
|---------|-----|---------|-------------|
| `[general]` | `font` | monospace | Font family name |
| | `font_size` | `18.0` | Font size in points |
| | `font_slant` | `normal` | `normal`, `italic`, or `oblique` |
| | `font_weight` | `normal` | `normal` or `bold` |
| | `position` | `center,bottom,0,10,0,0` | Anchor and margins (see below) |
| | `format` | `{message} {percent}%` | Text template |
| | `fps` | `30` | Frame rate for procedural transitions. Frame animations use their own `fps` |
| | `scale` | `1.0` | Display scale multiplier. Minimum `0.1` |
| | `output` | primary | Output name. `all` is accepted but not implemented |
| | `battery_mode` | `first` | `first`, `combined`, `highest`, `lowest` |
| | `sound` | `true` | `false` disables all sound, same as `--no-sound` |
| `[appearance]` | `text_color` | white | RGBA array `[R, G, B, A]`, 0.0–1.0 |
| | `bg_color` | black at 0.6 | Background RGBA |
| | `border_radius` | `8.0` | Corner radius in pixels |
| | `gradient` | `true` | Gradient background |
| `[colors]` | `<name>` | — | Named colour referenced by a signal's `color` |
| `[animations]` | `source` | required | Directory of PNG frames, relative to the config file |
| | `fps` | `general.fps` | Playback rate for this animation |
| | `loop` | `true` | Restart at the end instead of finishing |
| | `display` | `anim` | `anim` shows the animation alone, `text` puts it above the text card |
| | `on_complete` | `hold` | `hold`, `hide`, or `loop` (see below) |
| `[[signal]]` | `message` | — | Notification text. `{message}` expands to the event's own text |
| | `icon` | — | Unicode icon character or Nerd Font glyph |
| | `icon_size` | `24` | Icon size in pixels |
| | `color` | — | Name from `[colors]`, or an RGBA array |
| | `threshold` | — | Battery percentage trigger point, 0–100 |
| | `state` | — | `charging`, `discharging`, `full`, `connected`, `disconnected`, `any` |
| | `animation` | `none` | Procedural transition, or the name of an `[animations]` entry |
| | `animation_ref` | — | Name of an `[animations]` entry to play as the content |
| | `duration` | derived | Seconds on screen. `0` stays until dismissed. Omitted derives it from the animation |
| | `sound` | — | Sound file, relative to the config file |

### Signal matching

`charging` and `full` match ascending: the highest `threshold` at or below the
current percentage wins. Everything else matches descending: the lowest
`threshold` at or above it. So a set of thresholds partitions the range without
overlapping, and percentage drifting inside a band does not re-trigger.

`Battery_mode` decides how multiple batteries are combined. `first` takes the
device with the lowest sysfs name, so it is reproducible across runs rather than
depending on hash order. `highest`, `lowest`, and `combined` behave as named. A
percentage that arrives as NaN from a malformed UPower payload is ignored rather
than allowed to win a comparison.

### Position Format

Six comma-separated parts:

```
horizontal, vertical, margin_h, margin_v, offset_x, offset_y
```

| Part | Values | Default |
|------|--------|---------|
| `horizontal` | `left`, `center`, `right` | `center` |
| `vertical` | `top`, `center`, `bottom` | `bottom` |
| `margin_h` | pixels | `10` |
| `margin_v` | pixels (defaults to `margin_h`) | `margin_h` |
| `offset_x` | pixels | `0` |
| `offset_y` | pixels | `0` |

Examples:

- `"center,bottom,0,90,0,0"` — bottom centre, 90px up from the bottom edge
- `"center,bottom,0,10,0,0"` — bottom centre, 10px up
- `"right,top,20,30"` — top right, 20px horizontal and 30px vertical margin
- `"center,center,0,0,0,0"` — dead centre

`margin` is the gap from the anchored edge. `offset` then moves the result
further, so a negative `offset_y` on a `bottom` anchor pushes it further up.

Changing `position` takes effect on the next notification for a running daemon
only after a restart. Margin and `scale` changes do apply on reload.

### Animations

| Name | Description |
|------|-------------|
| `none` | Static, no animation |
| `fade` | Fade in → hold → fade out |
| `pulse` | Smooth opacity pulse |
| `blink` | Visible/invisible toggle |
| `slideleft` | Slide in from left, ease out |
| `slideright` | Slide in from right, ease out |
| `bounce` | Parabolic bounce with decay |

### Frame animations

An `[animations]` entry plays a directory of PNG frames as the notification
body.

```toml
[animations]
cube_charge = { source = "assets/animations/cube_charging", fps = 30, loop = true, display = "text" }
```

Frames are decoded on demand into a small ring and straight to display size.
A 216-frame 640x640 set costs about 0.6 MB resident instead of 337 MB, and
loading does not stall the event loop. Measured on the bundled `cube_anim`:

| | frames resident | load | per tick |
|---|---|---|---|
| decoding everything up front | 337.5 MB | 457 ms | — |
| lazy, at display size | 0.61 MB | 18.6 ms | 2.41 ms |

Filenames are ordered naturally, so `frame_9.png` plays before `frame_10.png`.
Any resolution works; anything larger than the display box is decoded down to
fit, and nothing is ever decoded larger than its source, so a high-DPI display
renders slightly soft rather than costing frames x scale² x 4 bytes.

`display` decides what else is on screen:

- `anim` — the animation alone, nothing else
- `text` — the animation in a box above the text card

#### Adding your own

Put a directory of PNGs anywhere and point `source` at it. Nothing needs to be
registered or converted.

```bash
mkdir -p ~/.config/inno/assets/animations/my_anim
cp /path/to/frames/*.png ~/.config/inno/assets/animations/my_anim/
```

```toml
[animations]
my_anim = { source = "assets/animations/my_anim", fps = 24, display = "text" }
```

```bash
inno --check-config          # confirms the directory and frames were found
inno --test-frame my_anim    # plays it on its own
```

To use the sets inno ships with, copy them into your own config directory:

```bash
mkdir -p ~/.config/inno/assets/animations
cp -r assets/animations/* ~/.config/inno/assets/animations/
```

AUR and Debian-style installs already place them under
`/etc/xdg/inno/assets/animations/`, so a system config can reference them
directly.

#### What happens when a frame animation ends

`on_complete` decides what a **non-looping** animation does at its last frame.
Looping animations never reach it.

| Value | Behaviour |
|---|---|
| `hold` | The last frame stays on screen. The surface stops being redrawn, so this costs nothing |
| `hide` | The notification is taken down as soon as the last frame lands |
| `loop` | Playback restarts, whatever `loop` says |

`on_complete = "loop"` overrides `loop = false`, and `--check-config` warns when
both are set so the contradiction is visible.

### Composing transitions with content

`animation` is the transition and `animation_ref` is the content, and a signal
may set both:

```toml
animation = "fade"            # fades in and out
animation_ref = "cube_charge"  # content is the cube
```

### Durations

`duration` is optional on every signal.

- `duration = 0` — stays up until dismissed by click.
- `duration = <seconds>` — explicit, with a half-second tail so a transition
  still playing when the timer expires is not cut off mid-frame.
- omitted, with a frame animation — derived from the frame count and rate, so
  there is nothing to keep in sync by hand. A 216-frame animation at 30fps
  displays for 7.2 seconds.
- omitted, without a frame animation — 5 seconds.

An explicit value always wins over the derived one.

### Previewing

You do not need to wait for a real event to see a notification.

```bash
inno --test-signal Charging          # the signal whose message matches, as a real event would render it
inno --test-frame cube_charge        # one frame animation on its own, no text
inno --test-animations               # cycle the procedural transitions
inno --test 3                        # one procedural transition (1–6)
inno --check-config                  # validate and exit
```

`--test-signal` goes through the same path a DBus event takes, so it exercises
transitions and frame animations together, and shows the real battery percentage
rather than a placeholder. `--test-frame` and `--test-animations` are mutually
exclusive.

`INNO_TRACE=1` logs one line per frame with elapsed time, frame index and
transition alpha, which is how playback rate and drift can be measured without a
compositor in the way:

```
INNO_TRACE=1 inno --test-signal Charging 2>&1 | grep TRACE
```

### Render verification

`scripts/render-verify.sh` checks what actually reaches the screen. It needs
Hyprland, `grim`, `start-hyprland`, and `python3` with Pillow.

```bash
cargo build --release
scripts/render-verify.sh                 # every check
scripts/render-verify.sh scale fade      # named checks only
```

A screenshot of a live desktop contains a wallpaper, panels and a terminal, so
"there are bright pixels in the middle of the screen" proves nothing. Three
things fix that:

- **An isolated compositor.** A nested Hyprland on its own socket, configured by
  `scripts/render-hypr.lua`, which autostarts nothing and disables the wallpaper
  and splash. Lua rather than `.conf` because Hyprland 0.57 removes `.conf` and
  draws deprecation notices as overlays, and because it draws config errors in
  the exact spot a notification would occupy.
- **A blankness precondition.** After startup the surface must be almost uniform
  and idle, or the script refuses to measure. That is what stops a broken
  compositor config from being reported as a rendering result.
- **Baseline differencing.** Each check captures with no daemon and again with
  it, and reports the bounding box of everything that changed, so only pixels
  inno drew are counted.

It leaves nothing running: the compositor it starts is tracked by pid and killed
on exit, and strays from an interrupted run are reaped first.

The checks:

| Check | Asserts |
|---|---|
| `timing` | Playback rate and drift, from `INNO_TRACE` |
| `scale` | Rendered size tracks `scale` linearly and pixels track area |
| `fade` | A transition reaches the screen for an animation-only notification |
| `layout` | Animation above the text card, both centred |
| `reload` | Playback survives a config edit and is still moving after |

### Sounds

Sound files are bundled in `assets/sounds/`, a set of Windows 7 system sounds.
To use them from a user config:

```bash
mkdir -p ~/.config/inno/assets/sounds
cp assets/sounds/*.wav ~/.config/inno/assets/sounds/
```

Then reference any file with a path relative to the config file. Absolute paths
work too, so pointing at your own recordings is just as easy:

```toml
sound = "assets/sounds/hardware_insert.wav"
sound = "/home/you/sounds/charge.ogg"
```

#### How playback is chosen

At startup the daemon tries `pw-play`, then `paplay`, then `ffplay`, then
`mpv`. Each candidate gets a real silent sample played through it, which checks
the binary, the sound server and the audio device together, rather than just
looking for the binary on `PATH` — a player being installed says nothing about
whether a sound server is running. The winner is cached and never re-probed.

If nothing works it warns once and carries on with notifications silent. A
notification daemon that will not start because a cosmetic sound cannot play is
worse than a quiet one, and the probe is bounded by a two-second timeout so a
hung player cannot stop the daemon either.

`aplay` is deliberately absent from the chain. It validates WAV headers
strictly and rejects the bundled files, and without a PulseAudio plugin it would
bypass the sound server and drive the sound card directly, which is the routing
problem the fallback exists to avoid.

Failures are reported once per distinct cause rather than on every notification,
and a failing player is reaped rather than left as a zombie.

Turn sound off with `--no-sound`, or `sound = false` under `[general]` to make it
the default for a config.

## Custom DBus Events

Inno can listen for **any** DBus signal. Define custom events in `~/.config/inno/events/*.toml`.

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

### Example: Battery Event

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

## DBus Control Interface

Control Inno externally via the `org.inno.Control` interface on the session bus.

| Method | Signature | Description |
|--------|-----------|-------------|
| `Show` | `(st)` — message, duration | Show a custom notification |
| `Hide` | — | Hide the current notification |
| `GetState` | — | Returns `(percentage, state)` |
| `Reload` | — | Reload configuration |
| `Version` | — | Returns daemon version string |

```bash
# Show notification for 5 seconds
busctl --user call org.inno.Control /org/inno/Control org.inno.Control Show "st" "Hello World" 5

# Hide notification
busctl --user call org.inno.Control /org/inno/Control org.inno.Control Hide

# Get battery state
busctl --user call org.inno.Control /org/inno/Control org.inno.Control GetState

# Reload config
busctl --user call org.inno.Control /org/inno/Control org.inno.Control Reload
```

## CLI Usage

```
inno [OPTIONS]

OPTIONS:
    -h, --help              Show help
    -v, --version           Show version
    -d, --debug             Run in debug mode (logs to terminal)
    --daemon                Run in background, re-executing itself with
                            --internal-daemon
    -l, --log-file <PATH>   Log output to file (use with --daemon)
    --no-dbus               Disable DBus control interface
    --no-sound              Disable notification sounds
    --test <number>         Preview one procedural transition (1-6)
    --test-animations       Cycle through all procedural transitions
    --test-frame <name>     Preview one [animations] entry on its own
    --test-signal <text>    Preview the signal whose message matches <text>,
                            rendered as a real event would render it
    --check-config          Validate config and exit
```

Flags take their value as a separate argument, so `--test-frame cube_charge`
works and `--log-file --no-dbus` correctly leaves `--no-dbus` alone.
`--internal-daemon` is set by `--daemon` and beats it, so re-spawning cannot
fork-bomb.

Exit status for `--check-config` is 1 when there are errors, 0 otherwise, so it
drops into a package build or a pre-commit hook.

## Known limitations

- `output = "all"` is accepted by the config parser but not implemented. It
  behaves the same as the default. Multi-output notification placement needs a
  layer surface per output.
- Changing `position` or `output` needs a restart. `margin_*`, `scale`, and
  everything else apply on config reload.
- `--flag=value` is not accepted. Values must be a separate argument.
- An animation-only notification (`display = "anim"`) is sized from the frame's
  natural dimensions, so on a high-DPI output it renders softer than the text
  card beside it rather than decoding larger and using the memory.

## Directory Structure

```
inno/
├── inno.toml              # Main config (bundled, installed to /etc/xdg/inno/)
├── events/                # Event definitions (*.toml)
│   ├── bluetooth.toml
│   ├── headset_battery.toml
│   └── laptop_battery.toml
├── assets/
│   ├── animations/        # Frame animations, one directory of PNGs each
│   │   ├── cube_anim/     #   216 frames, 640x640
│   │   ├── cube_charging/ #   216 frames, 128x128
│   │   └── ripple_charge/ #    30 frames, 128x128
│   ├── images/            # Screenshots
│   └── sounds/            # Windows 7 notification sounds (*.wav, 16-bit PCM)
├── scripts/
│   ├── render-hypr.lua    # Config for the isolated verification compositor
│   └── render-verify.sh   # Render verification harness
├── inno.service           # Systemd user service
├── PKGBUILD               # Arch Linux AUR package
└── src/                   # Rust source code
```

A user config directory mirrors that layout:

```
~/.config/inno/
├── inno.toml
├── events/
├── assets/
│   ├── animations/        # Copied here to use the bundled sets, or your own
│   └── sounds/            # Copied here to use the bundled sounds
```

## License

MIT
