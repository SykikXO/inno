#!/usr/bin/env bash
# Proves that a notification only takes clicks where it should.
#
# A surface starts with an infinite input region, so an unconstrained
# notification swallows every click that lands on it, including the transparent
# padding above and below the card where nothing is drawn. That is invisible in a
# screenshot and only shows up as a click that mysteriously does nothing, so it
# needs real pointer events aimed at known pixels.
#
# Clicks go through the wlr virtual pointer protocol (`cargo run --example
# click`). ydotool cannot be used for this: its `mousemove` is relative and its
# absolute mode is subject to pointer acceleration, which its own man page
# warns about. Asked for 400,300 it puts the cursor at 560,420. The virtual
# pointer carries an explicit extent, so it lands exactly where it is told.
#
# Geometry is read from the compositor with `hyprctl layers` rather than assumed,
# because hardcoding it means the test silently stops testing anything the
# moment a margin changes.
#
#   scripts/region-verify.sh
#
# Needs a live Hyprland session. Kills nothing it did not start.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CLICK="$ROOT/target/release/examples/click"
INNO="$ROOT/target/release/inno"
CFG="${CFG:-/tmp/inno-region-verify}"
LOG="$CFG/log"
SCALE=1.0
# Half of V_PADDING from src/draw.rs: the surface is taller than the card by 120,
# split evenly, so the card starts 60px down.
PAD=60

pass=0
fail=0
say() { printf '%s\n' "$*"; }

# "x y w h" of the notification surface, empty if there is not one.
geometry() {
    timeout 10 hyprctl layers 2>/dev/null |
        grep 'namespace: inno_notification' |
        grep -o 'xywh: [-0-9 ]*' |
        tail -1 |
        sed 's/xywh: //'
}

# The notification is showing when its surface exists at a real size. A hidden
# notification is a 1x1 transparent pixel that is still mapped, so testing for the
# surface alone reports a dismissed card as present.
up() {
    local h
    h="$(geometry | awk '{print $4}')"
    [ -n "${h:-}" ] && [ "$h" -gt 1 ] 2>/dev/null
}

clicks() { grep -c 'click at surface-local' "$LOG" 2>/dev/null | head -1; }

# One daemon for the whole run. It lasts long enough for every case, and nothing
# has to be torn down and rebuilt between them.
start() {
    pkill -x inno 2>/dev/null
    sleep 1.5
    (cd "$CFG" && setsid "$INNO" --internal-daemon --test-signal "REGION PROBE" \
        --no-dbus --no-sound >"$LOG" 2>&1 </dev/null &)
    sleep 3
    up
}

# Clicks a surface-local offset from the card's top-left, and reports how many
# presses the compositor routed to the surface.
press() {
    local x="$1" y="$2" before after i
    before="$(clicks)"
    for i in 1 2; do
        "$CLICK" "$((SURF_X + x))" "$((SURF_Y + y))" >/dev/null
        sleep 0.4
    done
    after="$(clicks)"
    echo $((after - before))
}

# want_delivered is 0 when the compositor must not route the press at all, which
# is what makes the click fall through to whatever is underneath.
check_pass_through() {
    local label="$1" ox="$2" oy="$3" got
    got="$(press "$ox" "$oy")"
    if [ "$got" -ne 0 ]; then
        fail=$((fail + 1))
        say "  FAIL  $label: $got presses reached the notification, expected none"
    else
        pass=$((pass + 1))
        say "  ok    $label: 0 presses reached it, still showing: $(up && echo yes || echo no)"
    fi
}

# Offsets are from the surface's top-left, not the card's, so the card case has
# to aim past the padding.
check_dismisses() {
    local got
    got="$(press 0 "$MID_Y")"
    if [ "$got" -lt 1 ]; then
        fail=$((fail + 1))
        say "  FAIL  on the card: nothing arrived, expected the notification to take the click"
    else
        pass=$((pass + 1))
        say "  ok    on the card: $got presses arrived, dismissed: $(up && echo no || echo yes)"
    fi
}

[ -x "$CLICK" ] || { say "build it first: cargo build --release --example click"; exit 1; }
[ -x "$INNO" ] || { say "build it first: cargo build --release"; exit 1; }
hyprctl version >/dev/null 2>&1 || { say "hyprctl did not respond; needs a live Hyprland session"; exit 1; }

cargo build --release --manifest-path "$ROOT/Cargo.toml" --example click >/dev/null 2>&1
cargo build --release --manifest-path "$ROOT/Cargo.toml" >/dev/null 2>&1

mkdir -p "$CFG"
cat > "$CFG/inno.toml" <<EOF
[general]
font_size = 16
scale = $SCALE

[position]
anchor = "top-center"
margin = 200

[appearance]
bg_color = [0.15, 0.15, 0.18, 1.0]
border_radius = 8.0

[[signal]]
message = "REGION PROBE"
icon = ""
color = "white"
threshold = 0
state = "any"
duration = 600
EOF

say "region: a notification should only take clicks on its card"
start || { say "  FAIL  the notification never appeared"; exit 1; }

read -r SURF_X SURF_Y SURF_W SURF_H <<<"$(geometry)"
CARD_H=$((SURF_H - 2 * PAD))
MID_Y=$((PAD + CARD_H / 2))
say "  surface at ($SURF_X, $SURF_Y) ${SURF_W}x${SURF_H}; card is y $PAD..$((PAD + CARD_H))"

# The click-through cases run first and share one daemon. A press that lands on
# the card dismisses the notification and, in test mode, ends the process, so
# testing the card last is what keeps a single daemon enough for everything.
check_pass_through "padding above" 0 $((PAD / 2))
check_pass_through "padding below" 0 $((SURF_H - PAD / 2))
check_pass_through "past the surface" $((SURF_W + 30)) $MID_Y

check_dismisses

pkill -x inno 2>/dev/null
say "  $pass passed, $fail failed"
[ "$fail" -eq 0 ]
