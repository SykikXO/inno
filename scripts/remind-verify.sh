#!/usr/bin/env bash
# Proves that `remind` repeats a notification, and that it does not when unset.
#
# The complaint behind it was a notification about a full battery appearing once,
# being missed, and the charger staying plugged in for another ten minutes. So
# the property that matters is not "it shows again" but "it keeps showing until
# something says it was seen": a click, or a different state.
#
#   scripts/remind-verify.sh
#
# Needs a live Hyprland session. Kills nothing it did not start.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INNO="$ROOT/target/release/inno"
CFG="${CFG:-/tmp/inno-remind-verify}"
LOG="$CFG/log"
CLICK="$ROOT/target/release/examples/click"

pass=0
fail=0
say() { printf '%s\n' "$*"; }

cargo build --release --manifest-path "$ROOT/Cargo.toml" --example click >/dev/null 2>&1
cargo build --release --manifest-path "$ROOT/Cargo.toml" >/dev/null 2>&1
[ -x "$INNO" ] || { say "build it first: cargo build --release"; exit 1; }
hyprctl version >/dev/null 2>&1 || { say "hyprctl did not respond"; exit 1; }

mkdir -p "$CFG"

# Two signals: one that repeats, one that does not. Both are short lived so a
# test does not sit around for minutes.
write_config() {
    local first="$1" msg="$2"
    # Stop the previous daemon before touching the file. Writing the config while
    # a daemon is still running lets its file watcher fire a reload right across
    # the handover, and the next daemon then starts against a config that is
    # being rewritten underneath it.
    pkill -x inno 2>/dev/null
    sleep 1
    cat > "$CFG/inno.toml" <<EOF
[general]
font_size = 16
scale = 1.0

[position]
anchor = "top-center"
margin = 200

[appearance]
bg_color = [0.15, 0.15, 0.18, 1.0]
border_radius = 8.0

[[signal]]
message = "$msg"
icon = ""
color = "white"
threshold = 0
state = "any"
duration = 2
$first
EOF
}

start() {
    pkill -x inno 2>/dev/null
    sleep 1.5
    (cd "$CFG" && INNO_TEST_PERSIST=1 setsid "$INNO" --internal-daemon --test-signal "$1" \
        --no-dbus --no-sound >"$LOG" 2>&1 </dev/null &)
    sleep 1
}

# Whether a card is on screen right now. The surface is 1x1 when hidden, so size
# is what distinguishes showing from not.
showing() {
    local h
    h="$(timeout 10 hyprctl layers 2>/dev/null |
        grep 'namespace: inno_notification' | grep -o 'xywh: [-0-9 ]*' |
        tail -1 | awk '{print $4}')"
    [ -n "${h:-}" ] && [ "$h" -gt 1 ] 2>/dev/null && echo yes || echo no
}

count() { grep -c "$1" "$LOG" 2>/dev/null | head -1; }

say "remind: a notification repeats while it is still the current state"

# --- with remind set, it comes back after hiding -----------------------------
# Counting the repeats rather than sampling whether a card happens to be up: with
# a short duration the card is only up part of the time, so a single look lands
# in a gap about half the time and proves nothing either way.
write_config 'remind = 3' "REMIND ME"
start "REMIND ME"
sleep 13   # duration 2 + remind 3 cycles through this a few times
reminds="$(count 'Reminding after')"
if [ "${reminds:-0}" -ge 2 ]; then
    pass=$((pass + 1))
    say "  ok    it repeated: $reminds reminders in 13s with duration 2 and remind 3"
else
    fail=$((fail + 1))
    say "  FAIL  only ${reminds:-0} reminders in 13s; expected at least 2"
fi
pkill -x inno 2>/dev/null

# --- a click stops it --------------------------------------------------------
# A long duration here so the card is reliably still up when the click lands;
# sampling for a visible card and then clicking it is a race otherwise.
write_config 'remind = 3' "REMIND ME"
sed -i 's/^duration = 2$/duration = 20/' "$CFG/inno.toml"
start "REMIND ME"
sleep 2.5
read -r OX OY _ <<<"$(timeout 10 hyprctl layers 2>/dev/null |
    grep 'namespace: inno_notification' | grep -o 'xywh: [-0-9 ]*' | tail -1 |
    sed 's/xywh: //')"
if [ -n "${OX:-}" ] && [ -n "${OY:-}" ] && [ "$(showing)" = "yes" ]; then
    # The card sits 60px down from the top of the surface. Two presses, because
    # the first press after a daemon starts is regularly dropped: each virtual
    # pointer is a new device and the press races the compositor working out the
    # pointer is over the surface. The second is harmless once the first has
    # already dismissed the card.
    "$CLICK" $((OX + 20)) $((OY + 75)) >/dev/null
    sleep 0.8
    [ "$(showing)" = "yes" ] && "$CLICK" $((OX + 20)) $((OY + 75)) >/dev/null
    sleep 1
    before="$(count 'Reminding after')"
    sleep 9
    after="$(count 'Reminding after')"
    if [ "$before" = "$after" ] && [ "$(showing)" = "no" ]; then
        pass=$((pass + 1))
        say "  ok    clicking it stopped the reminder and it stayed gone"
    else
        fail=$((fail + 1))
        say "  FAIL  after a click: reminders $before -> $after, showing=$(showing)"
    fi
else
    fail=$((fail + 1))
    say "  FAIL  could not click the card: surface=[${OX:-none}] showing=$(showing)"
fi
pkill -x inno 2>/dev/null

# --- without remind, it stays gone -------------------------------------------
write_config '' "NO REMIND"
start "NO REMIND"
sleep 4
first="$(count 'Reminding after')"
sleep 9
later="$(count 'Reminding after')"
if [ "${first:-0}" = "0" ] && [ "${later:-0}" = "0" ]; then
    pass=$((pass + 1))
    say "  ok    without remind it hid once and never came back"
else
    fail=$((fail + 1))
    say "  FAIL  reminders logged ${first:-0} then ${later:-0}, expected none"
fi
pkill -x inno 2>/dev/null

say "  $pass passed, $fail failed"
[ "$fail" -eq 0 ]
