# Inno — Cleanup Plan

Nothing outstanding.

## Resolved

### 1. [DONE] Systemd service had `--no-dbus`
- `inno.service` passed `--no-dbus`, which skips registering `org.inno.Control`
  on the session bus and so disabled the control interface the README documents.
- Added by an agent while testing, and carried into the shipped unit when the
  file was rewritten for autostart in `451f312`. The earlier unit from `9eabe2d`
  ran plain `inno`.
- It was not guarding against anything. A second instance failing to claim the
  name is caught at `src/main.rs:219`, logged, and startup continues without the
  interface, so no flag was needed to make that safe.
- Flag removed. The four documented commands work against the service again.