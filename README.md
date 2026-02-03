# Inno Notification Agent

Inno is a lightweight, event-driven notification agent for Wayland, written in Rust. It listens for DBus events (like Battery Charging/Discharging) and displays non-intrusive notifications on your desktop.

<p align="center">
  <img src="pictures/charging.png" width="300" alt="Charging Notification">
  <img src="pictures/discharging.png" width="300" alt="Discharging Notification">
</p>

**Version 0.2.1** - Now with system-wide configuration and systemd service support.

## 🚀 Quick Start

1. **Install**:
   ```bash
   # Arch Linux
   makepkg -si
   
   # Or manual build
   cargo install --path .
   ```
2. **Launch**:
   ```bash
   inno
   ```
3. **Configure** (Optional):
   The default config is at `/etc/xdg/inno/inno.conf`. Customize it at `~/.config/inno/inno.conf`.

## Installation

### Arch Linux (PKGBUILD)
Build and install using makepkg:
```bash
makepkg -si
```

### Manual Build
Requirements: `rust`, `cargo`, `wayland`, `cairo`, `dbus`.

```bash
cargo build --release
sudo cp target/release/inno /usr/bin/
```

## Configuration

Inno looks for configuration in the following order:
1.  **User Config**: `~/.config/inno/inno.conf`
2.  **System Config**: `/etc/xdg/inno/inno.conf`

### Config Format (`inno.conf`)

```ini
font=Iosevka NFM
text_color=#FF00FF00  # Green (ARGB)
bg_color=#AA000000    # Semi-transparent Black (ARGB)
```

- **font**: Font family name (must be installed).
- **text_color/bg_color**: Hex format `#AARRGGBB` (Alpha, Red, Green, Blue).

## Usage

### Commands
- `inno`: Starts the agent as a **daemon** (detaches from terminal).
- `inno -f`: Runs in foreground (useful for debugging).
- `pkill inno`: Stops the agent.

### Systemd Integration
You can manage Inno as a user service:
```bash
systemctl --user enable inno.service
systemctl --user start inno.service
```

## Migration from C to Rust

The original C implementation has been completely rewritten in Rust for:
- **Memory Safety**: No manual memory management, no segfaults.
- **Modern Tooling**: Easy dependency management with Cargo.
- **Maintainability**: Type-safe DBus and Wayland bindings.

Lines of code reduced by ~30% while maintaining identical functionality.
