#!/usr/bin/env bash
# Runs the original game in DOSBox Staging on a private Xvfb display and captures
# raw 320x200 reference frames.
#
# Usage: tools/dosbox-capture.sh <out_dir> <script>
#   <script> is a file of lines: "wait <seconds>" | "key <xdotool key>" | "type <text>" | "shot <name>"
#   | "click <x> <y> [<mode height 200|240>] [<button>]" (game coordinates, scaled to the window)
#   | "move <x> <y> [<mode height 200|240>]" (pointer motion without a click)
#   | "fetch <file>" (copy a file the game wrote, e.g. a save, from the game directory to <out_dir>)
# The game runs from a fresh temporary copy of $BMP_DOS_DIR (default ~/Apps/bmp-dos/game), so
# neither real saves nor the files a scenario writes (saves, high scores) persist between runs.
set -euo pipefail
OUT=$(realpath -m "$1"); SCRIPT=$(realpath "$2")
DOSBOX=${DOSBOX:-$(ls -d "$HOME"/Apps/dosbox-staging*/ | head -1)dosbox}
GAME=${BMP_DOS_DIR:-$HOME/Apps/bmp-dos/game}
DISP=${CAPTURE_DISPLAY:-:77}
WORK=$(mktemp -d); mkdir -p "$OUT" "$WORK/cap"
cp -r "$GAME" "$WORK/game"; GAME="$WORK/game"
cat > "$WORK/dosbox.conf" <<CONF
[sdl]
output = texture
fullscreen = false
[render]
aspect = off
integer_scaling = off
[cpu]
cycles = ${DOSBOX_CYCLES:-max}
[mouse]
mouse_capture = seamless
[mixer]
nosound = true
[capture]
capture_dir = $WORK/cap
default_image_capture_formats = raw
[autoexec]
mount c "$GAME"
c:
BMP.BAT
CONF
Xvfb "$DISP" -screen 0 1280x1024x24 >/dev/null 2>&1 & XPID=$!
# DOSBox may ignore SIGTERM and the launcher can fork, so SIGKILL every process
# that uses this run's config file.
cleanup() {
  pids=$(pgrep -f "$WORK/dosbox.conf" || true)
  [ -n "$pids" ] && kill -9 $pids 2>/dev/null || true
  kill $XPID 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT
sleep 1
DISPLAY=$DISP "$DOSBOX" --noprimaryconf --nolocalconf -conf "$WORK/dosbox.conf" >"$WORK/dosbox.log" 2>&1 & DPID=$!
sleep 3
WIN=$(DISPLAY=$DISP xdotool search --sync --pid $DPID 2>/dev/null | tail -1)
while read -r cmd arg; do
  case "$cmd" in
    wait) sleep "$arg" ;;
    click)
      read -r cx cy ch cb <<<"$arg"; ch=${ch:-200}; cb=${cb:-1}
      eval "$(DISPLAY=$DISP xdotool getwindowgeometry --shell "$WIN")"
      px=$(( (cx * WIDTH + WIDTH / 2) / 320 )); py=$(( (cy * HEIGHT + HEIGHT / 2) / ch ))
      DISPLAY=$DISP xdotool mousemove --window "$WIN" "$px" "$py" sleep 0.2 mousedown "$cb" sleep 0.35 mouseup "$cb" ;;
    move)
      read -r cx cy ch <<<"$arg"; ch=${ch:-200}
      eval "$(DISPLAY=$DISP xdotool getwindowgeometry --shell "$WIN")"
      px=$(( (cx * WIDTH + WIDTH / 2) / 320 )); py=$(( (cy * HEIGHT + HEIGHT / 2) / ch ))
      DISPLAY=$DISP xdotool mousemove --window "$WIN" "$px" "$py" ;;
    key)  DISPLAY=$DISP xdotool key --window "$WIN" --delay 80 $arg ;;
    type) DISPLAY=$DISP xdotool type --window "$WIN" --delay 120 "$arg" ;;
    shot)
      before=$(ls "$WORK/cap" | wc -l)
      DISPLAY=$DISP xdotool key --window "$WIN" ctrl+F5
      for _ in $(seq 50); do [ "$(ls "$WORK/cap" | wc -l)" -gt "$before" ] && break; sleep 0.1; done
      newest=$(ls -t "$WORK/cap" | head -1)
      # Wait until DOSBox has finished writing the file (size stable).
      prev=-1; for _ in $(seq 30); do cur=$(stat -c %s "$WORK/cap/$newest" 2>/dev/null || echo 0); [ "$cur" -gt 0 ] && [ "$cur" = "$prev" ] && break; prev=$cur; sleep 0.2; done
      [ -n "$newest" ] && cp "$WORK/cap/$newest" "$OUT/$arg.png" && echo "captured $arg" ;;
    fetch) cp "$GAME/$arg" "$OUT/$arg" && echo "fetched $arg" ;;
    ''|'#'*) ;;
  esac
done < "$SCRIPT"
