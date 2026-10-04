#!/usr/bin/env bash
# Checks the banner's appearance and layout from the screen itself.
#
# Separate from banner-verify.sh, which clicks the buttons. This one needs no
# input injection at all, so it keeps working when pointer injection does not, and
# it checks the things a click cannot: that the scrim really covers the screen,
# that the panel is centred rather than merely somewhere, and that the buttons
# are three separate shapes with the primary filled.
#
#   scripts/banner-pixels.sh
#
# Needs a live Hyprland session and grim.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INNO="$ROOT/target/release/inno"
CFG="${CFG:-/tmp/inno-banner-pixels}"
LOG="$CFG/log"
SHOT="$CFG/banner.png"

pass=0
fail=0
say() { printf '%s\n' "$*"; }
check() {
    if [ "$2" = "$3" ]; then
        pass=$((pass + 1)); say "  ok    $1: $2"
    else
        fail=$((fail + 1)); say "  FAIL  $1: got $2, wanted $3"
    fi
}

cargo build --release --manifest-path "$ROOT/Cargo.toml" >/dev/null 2>&1
hyprctl version >/dev/null 2>&1 || { say "hyprctl did not respond"; exit 1; }
mkdir -p "$CFG"

{
    printf '[general]\nfont_size = 16\nscale = 1.0\n\n'
    printf '[position]\nanchor = "bottom-center"\nmargin = 90\n\n'
    printf '[appearance]\nbg_color = [0.10, 0.10, 0.12, 1.0]\nborder_radius = 8.0\n\n'
    printf '[colors]\nwhite = [1.0, 1.0, 1.0, 1.0]\ncrit = [1.0, 0.35, 0.25, 1.0]\n\n'
    printf '[[signal]]\nmessage = "your battery is running low, plug your pc in"\n'
    printf 'icon = ""\ncolor = "crit"\nthreshold = 0\nstate = "any"\nbanner = true\n\n'
    printf '[[signal.action]]\nlabel = "Plug"\ncommand = "true"\n\n'
    printf '[[signal.action]]\nlabel = "Later"\ncommand = "true"\n\n'
    printf '[[signal.action]]\nlabel = "Okay"\ncommand = "true"\n'
} > "$CFG/inno.toml"

say "banner pixels: covers the screen, panel centred, three distinct buttons"

pkill -x inno 2>/dev/null
sleep 1.5
(cd "$CFG" && setsid "$INNO" --internal-daemon \
    --test-signal "your battery is running low" --no-dbus --no-sound \
    >"$LOG" 2>&1 </dev/null &)
sleep 3.5

shot() { grim "$1" 2>/dev/null; }

# The panel and button rectangles the daemon reports, which the pixels below are
# checked against. Reading the geometry from one place and verifying it with the
# other is the point: a log line and a drawing that disagree is exactly the bug
# that matters here.
rects_from_log() {
    # One number per line, so the reader groups them four at a time regardless of
    # how many there are.
    grep 'banner panel' "$LOG" | tail -1 | grep -oE '[0-9]+' 
}

# Baseline with nothing on screen at all. Diffing against another banner would
# find no difference, which is how the first version of this check "passed" a
# banner that drew nothing.
pkill -x inno 2>/dev/null
sleep 2
shot "$CFG/before.png"

(cd "$CFG" && setsid "$INNO" --internal-daemon \
    --test-signal "your battery is running low" --no-dbus --no-sound \
    >"$LOG" 2>&1 </dev/null &)
sleep 3.5
shot "$SHOT"
rects_from_log > "$CFG/rects"
pkill -x inno 2>/dev/null

[ -f "$SHOT" ] || { say "  FAIL  could not capture the screen"; exit 1; }

python3 - "$SHOT" "$CFG/before.png" "$CFG/rects" > "$CFG/report" <<'PYEOF' 
import sys
from PIL import Image

after = Image.open(sys.argv[1]).convert("RGB")
before = Image.open(sys.argv[2]).convert("RGB")
# Numbers, four at a time: the panel first, then each button.
nums = [int(t) for t in open(sys.argv[3]).read().split()]
rects = [tuple(nums[i:i + 4]) for i in range(0, len(nums) - 3, 4)]
panel = rects[:1]
buttons = rects[1:]

W, H = after.size
ap, bp = after.load(), before.load()

def lum(c):
    return 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]

n = 0
for y in range(H):
    for x in range(W):
        if abs(lum(ap[x, y]) - lum(bp[x, y])) > 4:
            n += 1
print(f"CHANGED_PCT={n * 100.0 / (W * H):.1f}")

if panel:
    px, py, pw, ph = panel[0]
    print(f"PANEL={px},{py} {pw}x{ph}")
    print(f"PANEL_CENTRED={int(abs((px + pw / 2) - W / 2) <= 2 and abs((py + ph / 2) - H / 2) <= 2)}")

    # The drawn panel should be flat and the scrim just outside it should be too,
    # and clearly different from each other. That is the boundary being real
    # rather than a rectangle in a log.
    mid = int(py + ph / 2)
    # Background, not the middle: the middle is where the message is.
    # Clear of the rounded corners, or the corner radius is what gets sampled.
    inside = ap[px + 30, mid]
    left_out = ap[px - 8, mid]
    right_out = ap[px + pw + 8, mid]
    # Near the panel's left edge, clear of the centred text.
    flat = all(
        abs(lum(ap[px + 30, py + 22 + i * (ph - 44) // 2]) - lum(inside)) < 3
        for i in range(3)
    )
    print(f"PANEL_FLAT={int(flat)}")
    print(f"PANEL_VS_SCRIM={int(abs(lum(inside) - lum(left_out)) > 3 and abs(lum(inside) - lum(right_out)) > 3)}")

    # Each button's own centre should carry the accent fill, and the gaps between
    # them should not. Three buttons that are really three separate shapes is the
    # whole point of them having separate regions.
    if len(buttons) == 3:
        centres = [(bx + bw // 2, by + bh // 2) for bx, by, bw, bh in buttons]
        gaps = [
            (buttons[i][0] + buttons[i][2] + buttons[i + 1][0]) // 2
            for i in range(len(buttons) - 1)
        ]
        yb = buttons[0][1] + buttons[0][3] // 2
        def accent(x, y):
            r, g, b = ap[x, y]
            return r > 110 and r > g + 40 and r > b + 40
        filled = [accent(cx, cy) for cx, cy in centres]
        gap_hits = [accent(gx, yb) for gx in gaps]
        # Only the first is filled: it is the action you are meant to take. The
        # rest are outlines until hovered.
        print(f"BUTTONS={len(buttons)} PRIMARY_FILLED={int(filled[0])} "
              f"OTHERS_UNFILLED={int(not any(filled[1:]))} "
              f"NO_FILL_IN_GAPS={int(not any(gap_hits))}")
        # Each of the unfilled buttons is a distinct shape: an outline against the
        # panel background, where the filled primary has none to find.
        outlines = []
        for bx, by, bw, bh in buttons[1:]:
            top = ap[bx + bw // 2, by + 1]
            inner = ap[bx + bw // 2, by + bh // 2]
            outlines.append(abs(lum(top) - lum(inner)) > 3)
        print(f"OUTLINED={int(len(outlines) == len(buttons) - 1 and all(outlines))}")
PYEOF
pkill -x inno 2>/dev/null

# Re-read what the check reported and assert on it, so this exits non-zero when a
# property regresses rather than only printing numbers nobody compares.
r() { grep -oE "(^| )$1=[^ ]+" "$CFG/report" 2>/dev/null | tail -1 | sed "s/.*$1=//"; }
check "scrim covers the screen"   "$(r CHANGED_PCT | cut -d. -f1 | awk '{print ($1>=90)?1:0}')" 1
check "panel is centred"          "$(r PANEL_CENTRED)" 1
check "panel edge is real"        "$(r PANEL_VS_SCRIM)" 1
check "panel is flat"             "$(r PANEL_FLAT)" 1
check "three buttons"             "$(r BUTTONS)" 3
check "primary button is filled"  "$(r PRIMARY_FILLED)" 1
check "others are not filled"     "$(r OTHERS_UNFILLED)" 1
check "no fill between buttons"   "$(r NO_FILL_IN_GAPS)" 1
check "each has its own outline"  "$(r OUTLINED)" 1

say "  $pass passed, $fail failed"
[ "$fail" -eq 0 ]
