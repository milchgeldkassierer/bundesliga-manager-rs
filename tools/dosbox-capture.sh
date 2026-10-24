#!/usr/bin/env bash
# Runs the original game in DOSBox Staging on a private Xvfb display and captures
# raw 320x200 reference frames.
#
# Usage: tools/dosbox-capture.sh <out_dir> <script>
#   <script> is a file of lines: "wait <seconds>" | "key <xdotool key>" | "type <text>" | "shot <name>"
#   | "click <x> <y> [<mode height 200|240>] [<button>]" (game coordinates, scaled to the window)
#   | "move <x> <y> [<mode height 200|240>]" (pointer motion without a click; a negative
#     coordinate -n means the window's pixel column or row n - 1, i.e. exactly game coordinate 0)
#   | "fetch <file>" (copy a file the game wrote, e.g. a save, from the game directory to <out_dir>)
#   | "audio start" | "audio stop <name>" (toggle DOSBox's WAV recording with ctrl+F6 and copy
#     the recording to <out_dir>/<name>.wav; needs DOSBOX_AUDIO=1)
#   | "opl start" | "opl stop <name>" (toggle the raw OPL register capture and
#     copy it to <out_dir>/<name>.dro; timestamps are emulated time, so unlike a WAV recording
#     it is exact even when the emulation falls behind; needs DOSBOX_AUDIO=1)
# DOSBOX_AUDIO=1 turns sound on (Sound Blaster 2.0 with a single OPL2) with SDL's dummy audio
# driver, so the run is recordable but inaudible; by default sound is off. For recordings use
# fixed cycles (e.g. DOSBOX_CYCLES="fixed 30000") on an otherwise idle machine: when the
# emulation falls behind, the recording's timing becomes uneven.
# The raw OPL capture's default key (ctrl+alt+F7) does not reach DOSBox under Xvfb, so audio
# runs load a mapper file (built from one bundled with DOSBox Staging) that binds it to ctrl+F7;
# that map has no screenshot binding, so "shot" does not work in audio runs.
# BMP_ARGS is passed to BMP.BAT (e.g. 0 = no sound).
# DOSBOX_SEED=<n> aims the game's first RNG seed (the low word of MSC's time(), read about
# DOSBOX_SEED_DELAY seconds, default 12, after this script starts) at <n>: DOSBox reports the
# host's local time, so a TZ offset is chosen from the current time. MSC's time() adds 8 hours
# (PST8PDT) and subtracts one in US daylight saving time (first Sunday in April to last Sunday
# in October). The real seed may still be off by a second or two; recover it afterwards.
# The game runs from a fresh temporary copy of $BMP_DOS_DIR (default ~/Apps/bmp-dos/game), so
# neither real saves nor the files a scenario writes (saves, high scores) persist between runs.
# BMP_DOS_EXTRA names a directory whose files are copied into that copy first (fixture saves).
set -euo pipefail
OUT=$(realpath -m "$1"); SCRIPT=$(realpath "$2")
DOSBOX=${DOSBOX:-$(ls -d "$HOME"/Apps/dosbox-staging*/ | head -1)dosbox}
GAME=${BMP_DOS_DIR:-$HOME/Apps/bmp-dos/game}
DISP=${CAPTURE_DISPLAY:-:77}
WORK=$(mktemp -d); mkdir -p "$OUT" "$WORK/cap"
if [ -n "${DOSBOX_SEED:-}" ]; then
  TZ=$(python3 -I - "$DOSBOX_SEED" "${DOSBOX_SEED_DELAY:-12}" <<'PY'
import datetime, sys, time
seed, delay = int(sys.argv[1]), int(sys.argv[2])
def us_dst(t):
    d = datetime.datetime.fromtimestamp(t, datetime.timezone.utc)
    def sunday(month, last):
        day = datetime.date(d.year, month, 31 if last else 1)
        step = -1 if last else 1
        while day.weekday() != 6:
            day += datetime.timedelta(days=step)
        return day
    return sunday(4, False) <= d.date() < sunday(10, True)
base = int(time.time()) + delay
for dst in (1, 0):
    off = (base + 8 * 3600 - dst * 3600 - seed) % 65536
    if us_dst(base - off) == bool(dst):
        break
print("BMP+%02d:%02d:%02d" % (off // 3600, off // 60 % 60, off % 60))
PY
)
  export TZ
  echo "seed $DOSBOX_SEED: TZ=$TZ"
fi
cp -r "$GAME" "$WORK/game"; GAME="$WORK/game"
if [ -n "${BMP_DOS_EXTRA:-}" ]; then cp -r "$BMP_DOS_EXTRA"/. "$GAME"; fi
MAPPER=
if [ "${DOSBOX_AUDIO:-0}" = 1 ]; then
  sed -e 's/ *"stick_[^"]*"//g' -e '/^hand_decfskip/d' -e 's/^hand_caprawopl .*/hand_caprawopl "key 64 mod1"/' \
    "$(dirname "$DOSBOX")/resources/mapperfiles/xbox/simpbvs.map" > "$WORK/bmp.map"
  MAPPER="mapperfile = $WORK/bmp.map"
fi
cat > "$WORK/dosbox.conf" <<CONF
[sdl]
output = texture
$MAPPER
fullscreen = false
[render]
aspect = off
integer_scaling = off
[cpu]
cycles = ${DOSBOX_CYCLES:-max}
[mouse]
mouse_capture = seamless
[mixer]
$(if [ "${DOSBOX_AUDIO:-0}" = 1 ]; then printf 'nosound = false\n[sblaster]\nsbtype = sb2\noplmode = opl2'; else echo 'nosound = true'; fi)
[capture]
capture_dir = $WORK/cap
default_image_capture_formats = raw
[autoexec]
mount c "$GAME"
c:
BMP.BAT ${BMP_ARGS:-}
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
if [ "${DOSBOX_AUDIO:-0}" = 1 ]; then export SDL_AUDIODRIVER=dummy; fi
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
      # A negative coordinate -n puts the pointer on the window's pixel n - 1 (n small): game
      # coordinate 0, which the centre of game pixel 0 may round past.
      [ "$cx" -lt 0 ] && px=$(( -cx - 1 )); [ "$cy" -lt 0 ] && py=$(( -cy - 1 ))
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
    audio)
      read -r what name <<<"$arg"
      DISPLAY=$DISP xdotool key --window "$WIN" ctrl+F6
      if [ "$what" = stop ]; then
        sleep 1
        wav=$(ls -t "$WORK/cap" | grep -i '\.wav$' | head -1 || true)
        [ -n "$wav" ] && cp "$WORK/cap/$wav" "$OUT/$name.wav" && echo "recorded $name.wav"
        cp "$WORK/dosbox.log" "$OUT/$name.log"
      fi ;;
    opl)
      read -r what name <<<"$arg"
      DISPLAY=$DISP xdotool key --window "$WIN" ctrl+F7
      if [ "$what" = stop ]; then
        sleep 1
        dro=$(ls -t "$WORK/cap" | grep -i '\.dro$' | head -1 || true)
        [ -n "$dro" ] && cp "$WORK/cap/$dro" "$OUT/$name.dro" && echo "recorded $name.dro"
      fi ;;
    ''|'#'*) ;;
  esac
done < "$SCRIPT"
