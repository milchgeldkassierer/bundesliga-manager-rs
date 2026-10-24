#!/usr/bin/env bash
# Runs the original game in DOSBox Staging on a private Xvfb display and captures
# raw 320x200 reference frames.
#
# Usage: tools/dosbox-capture.sh <out_dir> <script>
#   <script> is a file of lines: "wait <seconds>" | "key <xdotool key>" | "type <text>" | "shot <name>"
# The game runs from a scratch copy ($BMP_DOS_DIR, default ~/Apps/bmp-dos/game) so real saves stay untouched.
set -euo pipefail
OUT=$(realpath -m "$1"); SCRIPT=$(realpath "$2")
DOSBOX=${DOSBOX:-$(ls -d "$HOME"/Apps/dosbox-staging*/ | head -1)dosbox}
GAME=${BMP_DOS_DIR:-$HOME/Apps/bmp-dos/game}
DISP=${CAPTURE_DISPLAY:-:77}
WORK=$(mktemp -d); mkdir -p "$OUT" "$WORK/cap"
cat > "$WORK/dosbox.conf" <<CONF
[sdl]
output = texture
fullscreen = false
[render]
aspect = off
integer_scaling = off
[cpu]
cycles = 20000
[capture]
capture_dir = $WORK/cap
default_image_capture_formats = raw
[autoexec]
mount c "$GAME"
c:
BMP.BAT
CONF
Xvfb "$DISP" -screen 0 1280x1024x24 >/dev/null 2>&1 & XPID=$!
trap 'kill $DPID $XPID 2>/dev/null || true; rm -rf "$WORK"' EXIT
sleep 1
DISPLAY=$DISP "$DOSBOX" --noprimaryconf --nolocalconf -conf "$WORK/dosbox.conf" >"$WORK/dosbox.log" 2>&1 & DPID=$!
sleep 3
WIN=$(DISPLAY=$DISP xdotool search --sync --pid $DPID 2>/dev/null | tail -1)
while read -r cmd arg; do
  case "$cmd" in
    wait) sleep "$arg" ;;
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
    ''|'#'*) ;;
  esac
done < "$SCRIPT"
