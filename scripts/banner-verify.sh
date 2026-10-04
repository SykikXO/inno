#!/usr/bin/env bash
# Proves a banner covers the screen but does not swallow the screen.
#
# A full-screen surface with an infinite input region eats every click on the
# display. For something that looks like a critical battery alarm, that is worse
# than the problem being reported: you lose the ability to click anything at all
# until you dismiss it.
#
# So this checks four things: the surface really is the size of the output, each
# of its buttons is its own region, clicking the panel away from the buttons
# dismisses it, and clicking anywhere outside the panel reaches the desktop
# instead of being eaten.
#
#   scripts/banner-verify.sh
#
# Needs a live Hyprland session. Kills nothing it did not start.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CLICK="$ROOT/target/release/examples/click"
INNO="$ROOT/target/release/inno"
CFG="${CFG:-/tmp/inno-banner-verify}"
LOG="$CFG/log"
MARK="$CFG/marks"

pass=0
fail=0
say() { printf '%s\n' "$*"; }

cargo build --release --manifest-path "$ROOT/Cargo.toml" --example click >/dev/null 2>&1
cargo build --release --manifest-path "$ROOT/Cargo.toml" >/dev/null 2>&1
[ -x "$CLICK" ] || { say "build it first: cargo build --release --example click"; exit 1; }
hyprctl version >/dev/null 2>&1 || { say "hyprctl did not respond; needs a live Hyprland session"; exit 1; }

mkdir -p "$CFG" "$MARK"

# The current monitor's line is "1920x1200@60.00 at 0x0" on 0.56, which is not the
# "resolution: WxH" form older versions used.
OUT_W=$(timeout 10 hyprctl monitors 2>/dev/null |
    grep -m1 -oE '[0-9]+x[0-9]+' | head -1 | cut -dx -f1)
OUT_H=$(timeout 10 hyprctl monitors 2>/dev/null |
    grep -m1 -oE '[0-9]+x[0-9]+' | head -1 | cut -dx -f2)
[ -n "$OUT_W" ] || { say "could not read the output size from hyprctl monitors"; exit 1; }

{
    printf '[general]\nfont_size = 16\nscale = 1.0\n\n'
    printf '[position]\nanchor = "bottom-center"\nmargin = 90\n\n'
    printf '[appearance]\nbg_color = [0.10, 0.10, 0.12, 1.0]\nborder_radius = 8.0\n\n'
    printf '[colors]\nwhite = [1.0, 1.0, 1.0, 1.0]\ncrit = [1.0, 0.35, 0.25, 1.0]\n\n'
    printf '[[signal]]\nmessage = "your battery is running low, plug your pc in"\n'
    printf 'icon = ""\ncolor = "crit"\nthreshold = 0\nstate = "any"\nbanner = true\n\n'
    for label in Plug Snooze Okay; do
        printf '[[signal.action]]\nlabel = "%s"\ncommand = "touch %s/%s"\n\n' "$label" "$MARK" "$label"
    done
} > "$CFG/inno.toml"

start() {
    pkill -x inno 2>/dev/null
    sleep 1
    rm -f "$MARK"/*
    (cd "$CFG" && setsid "$INNO" --internal-daemon \
        --test-signal "your battery is running low" --no-dbus --no-sound \
        >"$LOG" 2>&1 </dev/null &)
    sleep 3
}

surface() {
    timeout 10 hyprctl layers 2>/dev/null |
        grep 'namespace: inno_notification' |
        grep -o 'xywh: [-0-9 ]*' | tail -1 | sed 's/xywh: //'
}

# The panel and the buttons, as "x y w h" lines, out of the daemon's own log.
#  Matching the Rust debug formatting is brittle, so pull the numbers and group
#  them four at a time instead.
logged_rects() {
    grep -oE "$1" "$LOG" | tail -1 |
        grep -oE '\-?[0-9]+' |
        awk 'NR % 4 == 1 { x = $1 } NR % 4 == 2 { y = $1 }
             NR % 4 == 3 { w = $1 } NR % 4 == 0 { print x, y, w, $1 }'
}

# "banner panel Rect { x: .., y: .., w: .., h: .. }"
panel_rect() { logged_rects 'banner panel Rect \{[^}]*\}'; }
# "buttons [Rect { .. }, Rect { .. }, Rect { .. }]"
button_rects() { logged_rects 'buttons \[.*\]'; }

# Banner presses are logged under their own line; the card's counter would always
# read zero here and every button would look undelivered.
banner_clicks() { grep -c 'banner click at' "$LOG" 2>/dev/null | head -1; }
clicks() { banner_clicks; }

check() {
    local label="$1" got="$2" want="$3"
    if [ "$got" = "$want" ]; then
        pass=$((pass + 1))
        say "  ok    $label: $got"
    else
        fail=$((fail + 1))
        say "  FAIL  $label: got $got, wanted $want"
    fi
}

say "banner: covers the screen, clicks only on the panel"

# --- the surface is the whole output ----------------------------------------
start
read -r SX SY SW SH <<<"$(surface)"
say "  surface at ($SX, $SY) ${SW}x${SH}, output ${OUT_W}x${OUT_H}"
if [ "$SW" -ge "$((OUT_W - 2))" ] && [ "$SH" -ge "$((OUT_H - 2))" ]; then
    pass=$((pass + 1))
    say "  ok    it covers the screen"
else
    fail=$((fail + 1))
    say "  FAIL  ${SW}x${SH} is not the ${OUT_W}x${OUT_H} output"
fi

panel=($(panel_rect))
mapfile -t buttons < <(button_rects)
if [ "${#buttons[@]}" -ne 3 ]; then
    say "  FAIL  expected 3 buttons, the daemon reported ${#buttons[@]}"
    exit 1
fi
say "  panel: ${panel[*]}"

# --- each button is its own region ------------------------------------------
# The banner has to survive a press on a button, so all three can be tried
# against one daemon. Clicking the panel instead would dismiss it.
i=0
for want in Plug Snooze Okay; do
    read -r bx by bw bh <<<"${buttons[$i]}"
    rm -f "$MARK"/*
    before="$(clicks)"
    "$CLICK" $((SX + bx + bw / 2)) $((SY + by + bh / 2)) >/dev/null
    sleep 0.6
    after="$(clicks)"
    ran=""
    for m in Plug Snooze Okay; do [ -f "$MARK/$m" ] && ran="$ran $m"; done
    if [ "$ran" = " $want" ] && [ $((after - before)) -ge 1 ]; then
        pass=$((pass + 1))
        say "  ok    $want: only its own command ran"
    else
        fail=$((fail + 1))
        say "  FAIL  $want: ran[$ran] delivered=$((after - before))"
    fi
    i=$((i + 1))
done

# --- clicking off the panel reaches the desktop -----------------------------
# The far corners are outside a centred panel on any sane screen, and they are
# the places a real click is most likely to be aimed at something else.
for corner in "5 5" "$((OUT_W - 6)) 5" "5 $((OUT_H - 6))"; do
    read -r cx cy <<<"$corner"
    before="$(banner_clicks)"
    "$CLICK" "$cx" "$cy" >/dev/null
    sleep 0.5
    after="$(banner_clicks)"
    if [ $((after - before)) -eq 0 ]; then
        pass=$((pass + 1))
        say "  ok    click at ($cx, $cy) passed through to the desktop"
    else
        fail=$((fail + 1))
        say "  FAIL  click at ($cx, $cy) was swallowed by the banner"
    fi
done

# --- clicking the panel away from the buttons dismisses ---------------------
if [ "${#panel[@]}" -ge 4 ]; then
    read -r px py pw ph <<<"${panel[*]}"
    "$CLICK" $((SX + px + pw / 2)) $((SY + py + 8)) >/dev/null
    sleep 0.5
    h="$(surface | awk '{print $4}')"
    if [ -n "$h" ] && [ "$h" -gt 1 ]; then
        "$CLICK" $((SX + px + pw / 2)) $((SY + py + 8)) >/dev/null
    fi
    sleep 0.8
    h="$(surface | awk '{print $4}')"
    if [ -z "$h" ] || [ "$h" -le 1 ]; then
        pass=$((pass + 1))
        say "  ok    clicking the panel dismissed it"
    else
        fail=$((fail + 1))
        say "  FAIL    still showing, height $h"
    fi
fi

pkill -x inno 2>/dev/null
say "  $pass passed, $fail failed"
[ "$fail" -eq 0 ]
