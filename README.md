# Inno Notification Agent

Inno is a lightweight, event-driven notification agent for Wayland, written in Rust. It listens for DBus events (like Battery Charging/Discharging) and displays non-intrusive notifications on your desktop.

**Version 0.2.0** - Now rewritten in Rust for improved memory safety and performance.

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

## Migration from C to Rust

The original C implementation has been completely rewritten in Rust for:
- **Memory Safety**: No manual memory management, no segfaults
- **Modern Tooling**: Easy dependency management with Cargo
- **Maintainability**: Type-safe DBus and Wayland bindings

Lines of code reduced by ~30% while maintaining identical functionality.
