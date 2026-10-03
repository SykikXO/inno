#!/usr/bin/env bash
# Verifies what inno actually renders, in a compositor built for the purpose.
#
# Why not just screenshot the desktop: it contains a wallpaper, panels, a
# terminal and anything else that happens to be lit, so "there are bright pixels
# in the middle of the screen" proves nothing. Three things make a measurement
# mean something, and all three are needed.
#
#   1. An isolated compositor. A nested Hyprland on its own socket, started from
#      scripts/render-hypr.lua, which autostarts nothing and turns off the
#      wallpaper and splash. Nothing of the real desktop is ever in frame.
#   2. A blankness precondition. After startup the surface must be almost
#      uniform. If it is not, something drew on it, and that is either a broken
#      compositor config or an autostarted client. Hyprland renders config errors
#      as an overlay in the middle of the output, where a notification would be,
#      so this check is what stops a broken config from being reported as a
#      rendering result.
#   3. Baseline differencing. Each check captures the surface once with no daemon
#      and again with it, and reports the bounding box of everything that
#      changed. Only pixels inno drew can differ.
#
# The daemon runs detached with its output redirected, so no job-control text or
# shell prompt can appear between a pair of captures. An earlier version measured
# a whole-screen diff for exactly that reason.
#
# Usage:
#   scripts/render-verify.sh                  # every check
#   scripts/render-verify.sh scale fade       # named checks only
#
# Needs: Hyprland, grim, start-hyprland, python3 with Pillow.
set -u

ROOT=$(cd "$(dirname "$0")/.." && pwd)
BIN=${INNO_BIN:-$ROOT/target/release/inno}
WORK=${WORK:-/tmp/inno-render-work}
RUNTIME=${XDG_RUNTIME_DIR:-/run/user/1000}
PARENT=${PARENT_DISPLAY:-${WAYLAND_DISPLAY:-wayland-1}}
SIG=${SIG:-innotest}

mkdir -p "$WORK"
LOG=$WORK/verify.log
: > "$LOG"
say()  { printf '%s\n' "$*" >> "$LOG"; }
out()  { printf '%s\n' "$*"; }
fail() { printf 'render-verify: %s\n' "$*" >&2; exit 1; }

# --- capture ---------------------------------------------------------------

shot() { timeout 15 env WAYLAND_DISPLAY="$DISPLAY" XDG_RUNTIME_DIR="$RUNTIME" \
             grim "$1" >>"$LOG" 2>&1; }

# Number of distinct colours, and how many pixels are not the background colour.
surface_stats() {
    python3 - "$1" <<'PY'
import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
# getcolors is about three times faster than a Counter over every pixel, and
# this runs against a full-screen surface.
colors = im.getcolors(1 << 24)  # each entry is (count, colour)
n = max(colors, key=lambda c: c[0])[0]
print(f"{len(colors)} {im.width * im.height - n}")
PY
}

# How much two captures differ, i.e. whether anything is moving.
frame_delta() {
    python3 - "$1" "$2" <<'PY'
import sys
from PIL import Image, ImageChops
a = Image.open(sys.argv[1]).convert("RGB")
b = Image.open(sys.argv[2]).convert("RGB")
d = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > 12 else 0)
print(d.histogram()[255])
PY
}

# Bounding box and pixel count of what inno drew, relative to a no-daemon
# baseline.
measure() {
    python3 - "$1" "$2" <<'PY'
import sys
from PIL import Image, ImageChops
a = Image.open(sys.argv[1]).convert("RGB")
b = Image.open(sys.argv[2]).convert("RGB")
if a.size != b.size:
    print(f"SIZE MISMATCH {a.size} {b.size}")
    sys.exit()
m = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > 12 else 0)
box = m.getbbox()
# The mask is strictly 0 or 255, so its histogram counts the drawn pixels
# without walking two million of them in Python. getdata() is removed in Pillow 14.
n = m.histogram()[255]
print(f"drawn={n} box={box}" + (f" size={box[2]-box[0]}x{box[3]-box[1]}" if box else ""))
PY
}

# --- compositor ------------------------------------------------------------

# Find our compositor by the signature in its environment.
#
# Not `pkill -f`: that matches the command line, and the signature only ever
# appears in the environment, so the pattern matches nothing and every run leaks
# a compositor. Reading /proc/<pid>/environ is what actually distinguishes ours
# from the real session, and lets a leftover from an interrupted run be reaped.
COMPOSITOR_PID=
compositor_pid() {
    local pid
    for pid in $(pgrep Hyprland 2>/dev/null); do
        if tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null \
            | grep -qx "HYPRLAND_INSTANCE_SIGNATURE=$SIG"; then
            printf '%s' "$pid"
            return 0
        fi
    done
    return 1
}

# Clear anything a previous run left behind, so repeated or interrupted runs do
# not accumulate compositors.
reap_strays() {
    local pid
    while pid=$(compositor_pid); do
        say "reaping stray test compositor pid=$pid"
        kill "$pid" 2>/dev/null
        sleep 1
    done
}

start_compositor() {
    reap_strays
    # start-hyprland rather than the binary directly, which is what suppresses
    # the "launched without start-hyprland" notice. HYPRLAND_CONFIG rather than
    # XDG_CONFIG_HOME, because the Lua config path is fixed and XDG_CONFIG_HOME
    # is ignored for it.
    setsid env \
        WAYLAND_DISPLAY="$PARENT" \
        XDG_RUNTIME_DIR="$RUNTIME" \
        HYPRLAND_INSTANCE_SIGNATURE="$SIG" \
        HYPRLAND_CONFIG="$ROOT/scripts/render-hypr.lua" \
        WLR_LIBINPUT_NO_DEVICES=1 \
        start-hyprland >>"$WORK/hyprland.log" 2>&1 </dev/null &

    for _ in $(seq 1 25); do
        sleep 1
        for s in "$RUNTIME"/wayland-*; do
            [ -S "$s" ] || continue
            [ "${s##*/}" = "$PARENT" ] && continue
            DISPLAY=${s##*/}
            COMPOSITOR_PID=$(compositor_pid) || return 1
            return 0
        done
    done
    return 1
}

# Kill exactly the compositor we started. Never by name pattern: the real session
# runs the same binary, and guessing wrong there would take the desktop down.
stop_compositor() {
    if [ -n "$COMPOSITOR_PID" ]; then
        say "stopping test compositor pid=$COMPOSITOR_PID"
        kill "$COMPOSITOR_PID" 2>/dev/null
        for _ in 1 2 3 4 5; do
            kill -0 "$COMPOSITOR_PID" 2>/dev/null || break
            sleep 1
        done
        kill -9 "$COMPOSITOR_PID" 2>/dev/null
    fi
    COMPOSITOR_PID=
    DISPLAY=
}

# Refuse to measure on a surface that is not blank and idle. Both checks exist
# because the alternative is reporting compositor furniture as a rendering result.
require_clean_surface() {
    local tries=${IDLE_TRIES:-12}
    local colours odd
    for _ in $(seq 1 "$tries"); do
        sleep 1
        shot "$WORK/probe-a.png" || fail "capture failed on $DISPLAY"
        sleep 1
        shot "$WORK/probe-b.png" || fail "capture failed on $DISPLAY"
        local stats
        stats=$(surface_stats "$WORK/probe-a.png") || fail "could not measure the surface: $WORK/hyprland.log"
        case $stats in
            *" "*) read -r colours odd <<<"$stats" ;;
            *) fail "surface_stats printed no numbers, got '$stats'" ;;
        esac
        local moved
        moved=$(frame_delta "$WORK/probe-a.png" "$WORK/probe-b.png")
        say "surface: $colours distinct colours, $odd non-background px, moved=$moved"
        # A blank surface is a couple of shades of one colour. An error overlay or
        # any client would push this well past.
        if [ "$colours" -gt 8 ] || [ "$odd" -gt 2000 ]; then
            say "surface not clean yet, waiting"
            continue
        fi
        if [ "$moved" != "0" ]; then
            say "surface still animating, waiting"
            continue
        fi
        say "surface is clean and idle"
        return 0
    done
    fail "the test surface never went blank and idle; a compositor config error or an autostarted client is the likely cause. See $WORK/hyprland.log"
}

# --- daemon ----------------------------------------------------------------

PID=
daemon() {
    setsid env WAYLAND_DISPLAY="$DISPLAY" XDG_RUNTIME_DIR="$RUNTIME" \
        sh -c "cd '$CFG' && exec '$BIN' $*" >>"$WORK/daemon.log" 2>&1 </dev/null &
    PID=$!
}
stop_daemon() { [ -n "$PID" ] || return 0; kill "$PID" 2>/dev/null; wait "$PID" 2>/dev/null; PID=; }

# --- checks ----------------------------------------------------------------

# Rendered size must track config.scale linearly, and the pixel count must track
# area. This is the check that catches a buffer sized from the decoded frame
# rather than the source, which pins an animation-only notification to 1x on a
# high-DPI output while its text card grows.
check_scale() {
    say "scale: rendered size must scale linearly"
    for sc in 0.5 1.0 1.5 2.0; do
        set_scale "$sc"
        sleep 0.4
        shot "$WORK/s-base.png"
        daemon --test-signal SCALETEST --no-dbus
        sleep 1.5
        shot "$WORK/s-$sc.png"
        stop_daemon; sleep 0.3
        say "  scale=$sc $(measure "$WORK/s-base.png" "$WORK/s-$sc.png")"
    done
    set_scale 1.0
}

# A procedural transition must reach the screen for a notification whose content
# is an animation rather than text, which is the one display mode that used to
# ignore the transition entirely. A single-frame asset is used so the animation
# contributes no variation of its own and every difference is the fade.
check_fade() {
    say "fade: transition composes with an animation-only notification"
    shot "$WORK/f-base.png"
    daemon --test-signal FADEONLY --no-dbus
    local prev=0
    for at in 250 700 1300 2100 3200 4600; do
        sleep "$(python3 -c "print(($at-$prev)/1000)")"; prev=$at
        shot "$WORK/f-$at.png"
        say "  t=${at}ms $(measure "$WORK/f-base.png" "$WORK/f-$at.png")"
    done
    stop_daemon
}

# A slide settles: its offset reaches zero and stays there, so most of the
# transition's frames carry exactly the transform the previous one did. The
# daemon skips re-uploading those. That is only safe if the settled frame is
# still on screen afterwards, which is what this checks: the box must move
# early, stop moving once settled, and still be drawn at the end. A guard that
# skipped the wrong frames would leave the surface showing nothing.
check_slide() {
    say "slide: moves, settles, and stays on screen"
    shot "$WORK/sl-base.png"
    daemon --test-signal SLIDEMOVE --no-dbus
    local prev="" at
    for at in 200 400 700 1200 2000 3000; do
        sleep 0.4
        shot "$WORK/sl-$at.png"
        if [ -n "$prev" ]; then
            say "  t=${at}ms moved=$(frame_delta "$WORK/sl-$prev.png" "$WORK/sl-$at.png") $(measure "$WORK/sl-base.png" "$WORK/sl-$at.png")"
        else
            say "  t=${at}ms $(measure "$WORK/sl-base.png" "$WORK/sl-$at.png")"
        fi
        prev=$at
    done
    stop_daemon
}

# The animation box sits above the text card and both are centred.
check_layout() {
    say "layout: animation above text, both centred"
    shot "$WORK/l-base.png"
    daemon --test-signal TEXTMODE --no-dbus
    sleep 1.6
    shot "$WORK/l.png"
    stop_daemon
    say "  $(measure "$WORK/l-base.png" "$WORK/l.png")"
}

# Editing the config mid-playback used to freeze the last frame and spin the
# timer drawing nothing. The animation should still be moving afterwards.
check_reload() {
    say "reload: playback survives a config edit"
    cp "$CFG/inno.toml" "$WORK/inno.orig"
    sleep 0.4
    shot "$WORK/r-base.png"
    daemon --test-signal RELOOP --no-dbus
    sleep 1.2; shot "$WORK/r-before1.png"
    sleep 0.9; shot "$WORK/r-before2.png"
    sed -i 's/fps = 10, loop = true/fps = 4, loop = true/' "$CFG/inno.toml"
    sleep 1.0; shot "$WORK/r-after1.png"
    sleep 1.0; shot "$WORK/r-after2.png"
    stop_daemon
    cp "$WORK/inno.orig" "$CFG/inno.toml"
    say "  while playing $(measure "$WORK/r-base.png" "$WORK/r-before2.png")"
    say "  after reload $(measure "$WORK/r-base.png" "$WORK/r-after2.png")"
    say "  still animating after reload: moved=$(frame_delta "$WORK/r-after1.png" "$WORK/r-after2.png")"
}

# Playback rate, measured from inno's own trace rather than from pixels. Frames
# advance and reach the screen; this checks they advance at the configured rate.
check_timing() {
    say "timing: playback rate from INNO_TRACE"
    local rate
    for fps in 30 60; do
        # Through the same cd as the other checks. Without it inno picks up the
        # repository's own inno.toml and quietly previews a signal that is not
        # in it.
        setsid env WAYLAND_DISPLAY="$DISPLAY" XDG_RUNTIME_DIR="$RUNTIME" INNO_TRACE=1 \
            sh -c "cd '$CFG' && exec timeout 5 '$BIN' --test-signal TIMING$fps --no-dbus" \
            >"$WORK/trace-$fps.log" 2>&1 </dev/null &
        wait $! 2>/dev/null
        rate=$(python3 - "$WORK/trace-$fps.log" "$fps" <<'PY'
import re, sys
t = [float(m.group(1)) for m in
     (re.search(r'TRACE t=([\d.]+)', l) for l in open(sys.argv[1])) if m]
if len(t) < 20:
    print("not enough ticks"); raise SystemExit
w = t[10:]
span = w[-1] - w[0]
print(f"{(len(w)-1)/span:.2f}Hz target={sys.argv[2]} drift={((len(w)-1)/float(sys.argv[2]) - span)*1000:+.1f}ms")
PY
)
        say "  ${fps}fps asset: $rate"
    done
}

set_scale() {
    sed -i "s/^scale = .*/scale = $1/" "$CFG/inno.toml"
}

# --- test config -----------------------------------------------------------

write_config() {
    mkdir -p "$CFG/events" "$CFG/assets/animations/static"
    cp "$ROOT"/events/*.toml "$CFG/events/" 2>/dev/null
    cp "$ROOT"/assets/animations/ripple_charge/frame_0010.png \
       "$CFG/assets/animations/static/frame_0000.png"
    ln -sfn "$ROOT/assets/animations/ripple_charge" "$CFG/assets/animations/ripple"
    cat > "$CFG/inno.toml" <<'EOF'
[general]
font = "InputMono Nerd Font"
font_size = 16
format = "{message}"
fps = 10
scale = 0.5

[position]
anchor = "center"
margin = 0

[appearance]
bg_color = [0.0, 0.0, 0.0, 0.0]
border_radius = 0.0

[colors]
white = [1.0, 1.0, 1.0, 1.0]

# Single-frame assets. Nothing here moves on its own, so any change between
# captures is the transition or the layout, never the content.
[animations]
dot     = { source = "assets/animations/static", fps = 10, loop = true, display = "anim" }
dottext = { source = "assets/animations/static", fps = 10, loop = true, display = "text" }
ripple  = { source = "assets/animations/ripple",  fps = 10, loop = true, display = "anim" }
timing30 = { source = "assets/animations/ripple",  fps = 30, loop = true, display = "anim" }
timing60 = { source = "assets/animations/ripple",  fps = 60, loop = true, display = "anim" }

EOF

    # One row per preview signal: message, procedural transition, frame
    # animation, seconds on screen. The seven lines every block shared are
    # written once here instead of fifty times across seven signals.
    while read -r msg anim ref secs; do
        [ "${msg#message}" != "$msg" ] && continue
        {
            printf '[[signal]]\nmessage = "%s"\nicon = ""\ncolor = "white"\nthreshold = 0\nstate = "any"\n' "$msg"
            [ "$anim" = '""' ] || printf 'animation = "%s"\n' "$anim"
            # An explicit duration is needed on the single-frame assets: with
            # none set, the display time is derived from the animation's own
            # length, and one frame at 10fps is 0.1s, gone before the first capture.
            printf 'animation_ref = "%s"\nduration = %s\n\n' "$ref" "$secs"
        } >> "$CFG/inno.toml"
    done <<'SIGNALS'
message       animation     frame     secs
SCALETEST     ""            dot        30
FADEONLY      fade          dot        8
SLIDEMOVE     slide_right   dot        8
TEXTMODE      ""            dottext    30
RELOOP        ""            ripple     60
TIMING30      ""            timing30   30
TIMING60      ""            timing60   30
SIGNALS
}

# --- main ------------------------------------------------------------------

CFG=${CFG:-/tmp/inno-render-test}

main() {
    [ -x "$BIN" ] || fail "missing $BIN; run cargo build --release"
    command -v grim >/dev/null || fail "grim is required"
    command -v start-hyprland >/dev/null || fail "start-hyprland is required"
    python3 -c 'import PIL' 2>/dev/null || fail "python3 with Pillow is required"

    write_config
    "$BIN" --check-config >"$WORK/check.log" 2>&1 \
        || { out "test config is invalid:"; cat "$WORK/check.log"; exit 1; }

    say "starting isolated compositor"
    start_compositor || fail "could not start a nested Hyprland"
    say "isolated display: $DISPLAY"
    # Also on interrupt: a leaked compositor outlives the terminal that
    # spawned it, which is how four of them accumulated here.
    trap stop_compositor EXIT INT TERM

    require_clean_surface

    local checks=("$@")
    [ ${#checks[@]} -eq 0 ] && checks=(timing scale fade slide layout reload)
    for c in "${checks[@]}"; do
        case $c in
            timing) check_timing ;;
            scale)  check_scale ;;
            fade)   check_fade ;;
            slide)  check_slide ;;
            layout) check_layout ;;
            reload) check_reload ;;
            *) fail "unknown check: $c" ;;
        esac
    done

    out "log: $LOG"
    out "captures: $WORK"
}

main "$@"