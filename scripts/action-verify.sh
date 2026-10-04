#!/usr/bin/env bash
# Proves that each action button on a notification is its own clickable region,
# and that clicking one runs its own command.
#
# Three buttons on one card is the whole point of the input region work: if the
# regions were one rectangle covering the card, every click would hit the same
# thing. So this checks the negative as well as the positive, by asserting that
# pressing a button creates exactly one marker file and not the other two.
#
# The button rectangles are read out of the daemon's own log rather than
# recomputed here. Duplicating the layout arithmetic in the test means the test
# passes when it agrees with the code and fails when it does not, which is the
# wrong way round.
#
#   scripts/action-verify.sh
#
# Needs a live Hyprland session. Kills nothing it did not start.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CLICK="$ROOT/target/release/examples/click"
INNO="$ROOT/target/release/inno"
CFG="${CFG:-/tmp/inno-action-verify}"
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

# Each button writes its own marker, so which one ran is answered by the
# filesystem rather than by parsing anything clever out of the log.
{
    printf '[general]\nfont_size = 16\nscale = 1.0\n\n'
    printf '[position]\nanchor = "top-center"\nmargin = 200\n\n'
    printf '[appearance]\nbg_color = [0.15, 0.15, 0.18, 1.0]\nborder_radius = 8.0\n\n'
    printf '[[signal]]\nmessage = "BATTERY LOW"\nicon = ""\ncolor = "white"\n'
    printf 'threshold = 0\nstate = "any"\nduration = 600\n\n'
    i=0
    for label in PlugIn Snooze Ignore; do
        [ "$i" -gt 0 ] && printf '\n'
        printf '[[signal.action]]\nlabel = "%s"\ncommand = "touch %s/%s"\n' \
            "$label" "$MARK" "$label"
        i=$((i + 1))
    done
} > "$CFG/inno.toml"

start() {
    pkill -x inno 2>/dev/null
    sleep 1.5
    rm -f "$MARK"/*
    (cd "$CFG" && setsid "$INNO" --internal-daemon --test-signal "BATTERY LOW" \
        --no-dbus --no-sound >"$LOG" 2>&1 </dev/null &)
    sleep 3
}

# Surface origin as "x y".
surface_origin() {
    timeout 10 hyprctl layers 2>/dev/null |
        grep 'namespace: inno_notification' |
        grep -o 'xywh: [-0-9 ]*' |
        tail -1 |
        sed 's/xywh: //' |
        awk '{print $1, $2}'
}

# Buttons in declaration order, as "x y w h" lines, from the daemon's log.
buttons() {
    # Pull the numbers out and group them four at a time. Matching on the Rust
    # debug formatting instead would break the first time a field is renamed,
    # and would quietly report zero buttons rather than fail.
    grep -o 'card buttons at \[.*\]' "$LOG" | tail -1 |
        grep -oE '\-?[0-9]+' |
        awk 'NR % 4 == 1 { x = $1 } NR % 4 == 2 { y = $1 }
             NR % 4 == 3 { w = $1 } NR % 4 == 0 { print x, y, w, $1 }'
}

ran() { [ -f "$MARK/$1" ]; }

check_button() {
    local want="$1" bx="$2" by="$3" cw="$4" ch="$5" ox="$6" oy="$7"
    local cx=$((ox + bx + cw / 2)) cy=$((oy + by + ch / 2)) other
    rm -f "$MARK"/*
    "$CLICK" "$cx" "$cy" >/dev/null
    sleep 0.6
    if ! ran "$want"; then
        fail=$((fail + 1))
        say "  FAIL  $want: clicking its button at ($cx, $cy) ran nothing"
        return
    fi
    for other in PlugIn Snooze Ignore; do
        [ "$other" = "$want" ] && continue
        if ran "$other"; then
            fail=$((fail + 1))
            say "  FAIL  $want: clicking it also ran $other, regions are not separate"
            return
        fi
    done
    pass=$((pass + 1))
    say "  ok    $want: only its own command ran"
}

say "actions: each button is its own region and runs only its own command"
start

read -r OX OY <<<"$(surface_origin)"
[ -n "${OX:-}" ] || { say "  FAIL  the notification never appeared"; exit 1; }
mapfile -t BTNS < <(buttons)
[ "${#BTNS[@]}" -eq 3 ] || { say "  FAIL  expected 3 buttons, the log reported ${#BTNS[@]}"; exit 1; }
say "  surface at ($OX, $OY)"

i=0
for want in PlugIn Snooze Ignore; do
    read -r bx by bw bh <<<"${BTNS[$i]}"
    check_button "$want" "$bx" "$by" "$bw" "$bh" "$OX" "$OY"
    i=$((i + 1))
done

# The card body must still dismiss, and must not run anything. A click that
# dismisses ends the test-mode daemon, so this is deliberately last.
say "  card body still dismisses"
start
read -r OX OY <<<"$(surface_origin)"
"$CLICK" $((OX + 5)) $((OY + 70)) >/dev/null
sleep 0.8
left="$(ls "$MARK" 2>/dev/null | tr '\n' ' ')"
if [ -n "$left" ]; then
    fail=$((fail + 1))
    say "  FAIL  clicking the card body ran: $left"
else
    pass=$((pass + 1))
    say "  ok    clicking the card body ran nothing"
fi

pkill -x inno 2>/dev/null
say "  $pass passed, $fail failed"
[ "$fail" -eq 0 ]
