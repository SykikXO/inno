# Inno Notification Agent

Inno is a lightweight, event-driven notification agent for Wayland, written in C. It listens for DBus events (like Battery Charging/Discharging) and displays non-intrusive notifications on your desktop.

## Installation

### Arch Linux (PKGBUILD)
Build and install using makepkg:
```bash
makepkg -si
```

### Manual Build
Requirements: `cmake`, `make`, `gcc`, `wayland`, `cairo`, `dbus`, `wayland-protocols`.

```bash
cmake -B build
cmake --build build
sudo cp build/inno /usr/bin/
```

## Configuration

Create a config file at `~/.config/inno/inno.conf`:

```ini
font=Iosevka NFM
text_color=#FF00FF00  # Green (ARGB)
bg_color=#AA000000    # Semi-transparent Black (ARGB)
```

- **font**: Font family name (must be installed).
- **text_color/bg_color**: Hex format `#AARRGGBB` (Alpha, Red, Green, Blue).

## Usage

Run `inno` to start the agent. It runs as a **daemon** by default (detaches from terminal).

```bash
inno
```

To run in foreground (for debugging):
```bash
inno -f
```

To stop it:
```bash
pkill inno
```
