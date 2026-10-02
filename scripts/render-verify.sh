#!/usr/bin/env bash
# Renders notifications in an isolated compositor and reports what was drawn.
#
# Why this exists: verifying a notification daemon by screenshotting the live
# desktop does not work. The frame contains a wallpaper, panels, a terminal, and
# anything else that happens to be lit, so "there are bright pixels in the
# middle" proves nothing. Two things fix that, and this script does both.
#
#   1. An isolated compositor. A nested Hyprland instance on its own socket, with
#      a blank background and no windows, so nothing of the real desktop is in
#      frame.
#   2. Baseline differencing. Every check captures the surface once with no
#      daemon running and again with it running, and reports the bounding box and
#      pixel count of everything that changed. Only pixels that changed can have
#      been drawn by inno, which holds even when the test surface is not
#      perfectly blank.
#
# The daemon is started with setsid and with all output redirected to a file, so
# no job-control text or shell prompt can appear between a pair of captures.
#
# Usage:
#   scripts/render-verify.sh                 # run every check
#   scripts/render-verify.sh scale fade      # run named checks only
#
# Checks read their config from $CFG (default /tmp/inno-render-test) and expect
# signals named SCALETEST, FADEONLY and TEXTMODE to exist. See write_config below
# for the shape.
set -u

ROOT=$(cd "$(dirname "$0")/.." && pwd)
BIN=${INNO_BIN:-$ROOT/target/release/inno}
CFG=${CFG:-/tmp/inno-render-test}
WORK=${WORK:-/tmp/inno-render-work}
SIG=${SIG:-notest}
RUNTIME=${XDG_RUNTIME_DIR:-/run/user/1000}
PARENT_DISPLAY=${PARENT_DISPLAY:-${WAYLAND_DISPLAY:-wayland-1}}

mkdir -p "$WORK"
LOG=$WORK/verify.log
: > "$LOG"
say() { printf '%s\n' "$*" | tee -a "$LOG" >/dev/null; }
say_out() { printf '%s\n' "$*"; }

# --- isolated compositor ---------------------------------------------------

NESTED_SOCKET=

start_compositor() {
    mkdir -p "$WORK/hypr"
    # Deliberately minimal. Every option added here is another chance for a
    # config error, and Hyprland draws config errors as an overlay right across
    # the middle of the output, which is exactly the false positive this script
    # exists to avoid.
    # No .conf extension: that spelling draws a deprecation banner across the
    # output, which is a false positive waiting to happen.
    cat > "$WORK/hypr/hyprtest" <<'EOF'
workspace=1,default:true
animations {
    enabled = false
}
misc {
    disable_hyprland_logo = true
    disable_splash_rendering = true
    focus_on_activate = 0
}
EOF

    # The child connects to the parent compositor through WAYLAND_DISPLAY, and
    # keeps the real XDG_RUNTIME_DIR so that socket is reachable. It publishes
    # its own socket alongside, which is the one everything else uses.
    setsid env \
        WAYLAND_DISPLAY="$PARENT_DISPLAY" \
        XDG_RUNTIME_DIR="$RUNTIME" \
        HYPRLAND_INSTANCE_SIGNATURE="$SIG" \
        WLR_LIBINPUT_NO_DEVICES=1 \
        XDG_CONFIG_HOME="$WORK/hypr" \
        Hyprland --config "$WORK/hypr/hyprtest" \
        > "$WORK/hypr/hyprland.log" 2>&1 < /dev/null &

    for _ in $(seq 1 20); do
        sleep 1
        for sock in "$RUNTIME"/wayland-*; do
            [ -S "$sock" ] || continue
            # The parent's socket already existed; anything else is the child.
            [ "$sock" = "$RUNTIME/$PARENT_DISPLAY" ] && continue
            NESTED_SOCKET=$(basename "$sock")
            return 0
        done
    done
    return 1
}

stop_compositor() {
    pkill -f "Hyprland --config $WORK/hypr/hyprtest" 2>/dev/null
    sleep 1
}

shot() {
    timeout 15 env WAYLAND_DISPLAY="$NESTED_SOCKET" XDG_RUNTIME_DIR="$RUNTIME" \
        grim "$1" >> "$LOG" 2>&1
}

# --- measurement -----------------------------------------------------------

# Geometry of what inno drew: bounding box and pixel count of the difference
# from the baseline.
measure() {
    python3 - "$1" "$2" <<'PY'
import sys
from PIL import Image, ImageChops
a = Image.open(sys.argv[1]).convert("RGB")
b = Image.open(sys.argv[2]).convert("RGB")
if a.size != b.size:
    print(f"SIZE MISMATCH {a.size} {b.size}")
    sys.exit()
# Ignore small rounding noise; the threshold is in summed RGB.
mask = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > 12 else 0)
box = mask.getbbox()
n = sum(1 for p in mask.getdata() if p)
if box is None:
    print(f"changed=0 (nothing drawn)")
else:
    print(f"changed={n} bbox={box} size={box[2]-box[0]}x{box[3]-box[1]}")
PY
}

DAEMON_PID=
start_daemon() {
    setsid env WAYLAND_DISPLAY="$NESTED_SOCKET" XDG_RUNTIME_DIR="$RUNTIME" \
        sh -c "cd '$CFG' && exec '$BIN' $*" >> "$WORK/daemon.log" 2>&1 < /dev/null &
    DAEMON_PID=$!
}
stop_daemon() {
    [ -n "$DAEMON_PID" ] || return 0
    kill "$DAEMON_PID" 2>/dev/null
    wait "$DAEMON_PID" 2>/dev/null
    DAEMON_PID=
}

# --- checks ----------------------------------------------------------------

# Rendered size must track config.scale linearly. This is what caught a buffer
# sized from the decoded frame instead of the source, which pinned an
# animation-only notification to 1x on a high-DPI output.
check_scale() {
    say "=== scale: rendered size must scale linearly ==="
    local prev=0
    for sc in 0.5 1.0 1.5 2.0; do
        set_scale "$sc"
        sleep 0.5
        shot "$WORK/sc-base.png"
        start_daemon "--test-signal SCALETEST --no-dbus"
        sleep 1.5
        shot "$WORK/sc-$sc.png"
        stop_daemon
        sleep 0.3
        say "  scale=$sc $(measure "$WORK/sc-base.png" "$WORK/sc-$sc.png")"
        prev=$sc
    done
    set_scale 1.0
}

# The procedural transition must reach the screen even when the content is an
# animation rather than text, which is the case that ignored DrawState entirely
# before transitions could compose with frame animations. A static single-frame
# asset is used so the animation contributes no variation of its own.
check_fade() {
    say "=== fade composes with an animation-only notification ==="
    local prev=0
    shot "$WORK/fd-base.png"
    start_daemon "--test-signal FADEONLY --no-dbus"
    for at in 250 700 1300 2100 3200 4600; do
        sleep "$(python3 -c "print(($at-$prev)/1000)")"; prev=$at
        shot "$WORK/fd-$at.png"
        say "  t=${at}ms $(measure "$WORK/fd-base.png" "$WORK/fd-$at.png")"
    done
    stop_daemon
}

# A notification with no matching signal index must survive a scale change.
# Reading only the index used to wipe its animation and leave a text card.
check_layout() {
    say "=== layout: animation above text, both centred ==="
    shot "$WORK/lay-base.png"
    start_daemon "--test-signal TEXTMODE --no-dbus"
    sleep 1.6
    shot "$WORK/lay.png"
    stop_daemon
    say "  $(measure "$WORK/lay-base.png" "$WORK/lay.png")"
}

# Editing the config while an animation plays used to freeze the last frame and
# spin the timer drawing nothing.
check_reload() {
    say "=== reload during playback ==="
    cp "$CFG/inno.toml" "$WORK/inno.toml.orig"
    sleep 0.4
    shot "$WORK/rl-base.png"
    start_daemon "--test-signal RELOOP --no-dbus"
    sleep 1.2; shot "$WORK/rl-before1.png"
    sleep 0.9; shot "$WORK/rl-before2.png"
    sed -i 's/fps = 10, loop = true/fps = 4, loop = true/' "$CFG/inno.toml"
    sleep 1.0; shot "$WORK/rl-after1.png"
    sleep 1.0; shot "$WORK/rl-after2.png"
    stop_daemon
    cp "$WORK/inno.toml.orig" "$CFG/inno.toml"
    say "  before reload $(measure "$WORK/rl-base.png" "$WORK/rl-before2.png")"
    say "  after reload  $(measure "$WORK/rl-base.png" "$WORK/rl-after2.png")"
    say "  still moving after reload: $(frame_delta "$WORK/rl-after1.png" "$WORK/rl-after2.png")"
}

# Whether two consecutive captures differ, i.e. whether animation is still
# running. A static capture means it stopped.
frame_delta() {
    python3 - "$1" "$2" <<'PY'
import sys
from PIL import Image, ImageChops
a = Image.open(sys.argv[1]).convert("RGB")
b = Image.open(sys.argv[2]).convert("RGB")
d = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > 12 else 0)
print(f"moved={sum(1 for p in d.getdata() if p)}")
PY
}

set_scale() {
    python3 - "$CFG" "$1" <<'PY'
import pathlib, re, sys
p = pathlib.Path(sys.argv[1]) / "inno.toml"
p.write_text(re.sub(r'^scale = .*$', f"scale = {sys.argv[2]}", p.read_text(), flags=re.M))
PY
}

# Two consecutive identical captures mean nothing on the surface is moving.
wait_for_idle() {
    local tries=${IDLE_TRIES:-15}
    for _ in $(seq 1 "$tries"); do
        sleep 1
        shot "$WORK/idle-a.png" || return 1
        sleep 1
        shot "$WORK/idle-b.png" || return 1
        [ "$(frame_delta "$WORK/idle-a.png" "$WORK/idle-b.png")" = "moved=0" ] && return 0
    done
    return 1
}

write_config() {
    mkdir -p "$CFG/events" "$CFG/assets/animations/static"
    cp "$ROOT"/events/*.toml "$CFG/events/" 2>/dev/null
    cp "$ROOT"/assets/animations/ripple_charge/frame_0010.png \
       "$CFG/assets/animations/static/frame_0000.png"
    ln -sfn "$ROOT/assets/animations/ripple_charge" "$CFG/assets/animations/ripple" 2>/dev/null
    cat > "$CFG/inno.toml" <<'EOF'
[general]
font = "InputMono Nerd Font"
font_size = 16
position = "center,center,0,0,0,0"
format = "{message}"
fps = 10
scale = 0.5

[appearance]
bg_color = [0.0, 0.0, 0.0, 0.0]
border_radius = 0.0

[colors]
white = [1.0, 1.0, 1.0, 1.0]

# Static single-frame assets. Nothing here animates on its own, so any change
# between captures is the transition or the layout, never the content.
[animations]
staticdot = { source = "assets/animations/static", fps = 10, loop = true, display = "anim" }
statictext = { source = "assets/animations/static", fps = 10, loop = true, display = "text" }
ripple    = { source = "assets/animations/ripple", fps = 10, loop = true, display = "anim" }

[[signal]]
message = "SCALETEST"
icon = ""
color = "white"
threshold = 0
state = "any"
animation_ref = "staticdot"
# An explicit duration is required on the static assets. With none set, the
# display time is derived from the animation's own length, and a one-frame
# animation is 0.1s, which is gone before the first capture.
duration = 30

[[signal]]
message = "FADEONLY"
icon = ""
color = "white"
threshold = 0
state = "any"
animation = "fade"
animation_ref = "staticdot"
duration = 8

[[signal]]
message = "TEXTMODE"
icon = ""
color = "white"
threshold = 0
state = "any"
animation_ref = "statictext"
duration = 30

[[signal]]
message = "RELOOP"
icon = ""
color = "white"
threshold = 0
state = "any"
animation_ref = "ripple"
duration = 60
EOF
}

# --- main ------------------------------------------------------------------

main() {
    [ -x "$BIN" ] || { echo "missing $BIN; run cargo build --release" >&2; exit 1; }
    command -v grim >/dev/null || { echo "grim is required for capture" >&2; exit 1; }
    python3 -c 'import PIL' 2>/dev/null || { echo "python3 with Pillow is required" >&2; exit 1; }

    write_config
    "$BIN" --check-config > "$WORK/check.log" 2>&1 || {
        echo "test config is invalid:" >&2; cat "$WORK/check.log" >&2; exit 1; }

    say "starting isolated compositor"
    if ! start_compositor; then
        echo "could not start a nested Hyprland; is Hyprland installed?" >&2
        exit 1
    fi
    say "isolated display: $NESTED_SOCKET"

    # Hyprland draws startup banners that fade over several seconds. Baseline
    # and test captures taken either side of that fade differ for reasons that
    # have nothing to do with inno, so poll until the surface is genuinely idle
    # rather than guessing a delay.
    if ! wait_for_idle; then
        echo "warning: compositor surface never went idle, measurements may be noisy" >&2
    fi

    trap stop_compositor EXIT

    local checks=("$@")
    [ ${#checks[@]} -eq 0 ] && checks=(scale fade layout reload)
    for c in "${checks[@]}"; do
        case $c in
            scale) check_scale ;;
            fade)  check_fade ;;
            layout) check_layout ;;
            reload) check_reload ;;
            *) echo "unknown check: $c" >&2 ;;
        esac
    done

    say_out "log: $LOG"
    say_out "captures: $WORK"
}

main "$@"