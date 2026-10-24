#!/usr/bin/env bash
# Visual regression: runs each scenario in tests/visual/ on the port (headless) and
# compares every screenshot with a DOSBox reference of the original game.
#
# Usage: tools/visual-check.sh [scenario...]   (default: all)
# References live outside the repo in $BMP_HOLDOUT/visual/<scenario>/ (default
# ~/Apps/bmp-holdout) because they show original art. Missing references are
# captured with tools/dosbox-capture.sh first. The RNG seed of the original run is
# read from the "# seed: <n>" line of the port script; it has to be recovered once
# for a new reference set when the scenario depends on random data. A "# seed2: <n>"
# line fixes the second seed, read when a new game starts.
# A "# cycles: <setting>" line in the DOSBox script sets DOSBox's CPU speed for its
# capture (e.g. "fixed 1000" for the CPU-paced intro; default max). A "# aim-seed: <n>" line
# aims the capture's RNG seed at <n> (DOSBOX_SEED of tools/dosbox-capture.sh).
# The port runs on a temporary copy of $BMP_DATA, so scenarios may save. A
# "# compare-file: <name>" line compares the file <name> the port wrote with the
# one the original wrote (fetched into the reference directory) byte for byte.
# A "# fixture: <generator> <file> [<seed> [<seed2>]]" line names a file (a save) that both
# games start with: the port runs tests/visual/<generator> (with the seeds) on a fresh copy
# of $BMP_DATA once and keeps the <file> it wrote in $BMP_HOLDOUT/fixtures/<scenario>/; that
# file is then copied into the game directories of both the port and the DOSBox capture.
# A "# fixture-patch: <file> <example> <args...>" line runs the bmp-formats example <example>
# on a fixture <file> right after it was generated (e.g. save_add_messages to add inbox mail).
# A "# fixture-bytes: <file> <source> [<offset>=<byte>|<offset>^<mask>...]" line makes the
# fixture <file> from the file <source> of $BMP_DATA with single bytes set or XORed (offsets
# and values in C notation), e.g. a save with a broken signature, version or checksum.
# A "# fixture-edit: <file> <save> <op>..." line makes the fixture <file> a copy of the save
# <save> of $BMP_DATA with game-state edits (crates/bmp-game/examples/save_patch.rs lists them).
# Fixtures and references belong together: delete both to regenerate them.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
# Default: the read-only pristine copy of the game (see tools/check.sh), never the play directory.
DATA=${BMP_DATA:-${BMP_REFERENCE_DATA:-$HOME/Apps/bmp-pristine}}
[ -d "$DATA" ] || { echo "set BMP_DATA to the game directory" >&2; exit 2; }
HOLDOUT=${BMP_HOLDOUT:-$HOME/Apps/bmp-holdout}
cargo build -q -p bmp
BIN=${CARGO_TARGET_DIR:-target}/debug/bmp
names=("$@")
if [ ${#names[@]} -eq 0 ]; then
  for f in tests/visual/*.port.txt; do names+=("$(basename "$f" .port.txt)"); done
fi
status=0
for name in "${names[@]}"; do
  ref="$HOLDOUT/visual/$name"
  fix="$HOLDOUT/fixtures/$name"
  while read -r gen file fseed fseed2; do
    [ -f "$fix/$file" ] && continue
    mkdir -p "$fix"
    gdata=$(mktemp -d); gout=$(mktemp -d)
    cp -r "$DATA"/. "$gdata" && chmod -R u+w "$gdata"
    "$BIN" --data "$gdata" run --script "tests/visual/$gen" --shots "$gout" \
      ${fseed:+--time "$fseed"} ${fseed2:+--time2 "$fseed2"} </dev/null >/dev/null 2>&1 || true
    cp "$gdata/$file" "$fix/$file" && echo "fixture $name/$file generated"
    while read -r pfile tool pargs; do
      [ "$pfile" = "$file" ] || continue
      # shellcheck disable=SC2086 # the arguments are split on purpose
      cargo run -q -p bmp-formats --example "$tool" -- "$fix/$file" $pargs
    done < <(sed -n 's/^# fixture-patch: *//p' "tests/visual/$name.port.txt")
    rm -rf "$gdata" "$gout"
  done < <(sed -n 's/^# fixture: *//p' "tests/visual/$name.port.txt")
  while read -r file src edits; do
    [ -f "$fix/$file" ] && continue
    mkdir -p "$fix"
    python3 -I - "$DATA/$src" "$fix/$file" $edits <<'PY'
import sys
data = bytearray(open(sys.argv[1], "rb").read())
for e in sys.argv[3:]:
    op = "^" if "^" in e else "="
    off, val = e.split(op)
    off, val = int(off, 0), int(val, 0)
    data[off] = data[off] ^ val if op == "^" else val
open(sys.argv[2], "wb").write(data)
PY
    echo "fixture $name/$file patched from $src"
  done < <(sed -n 's/^# fixture-bytes: *//p' "tests/visual/$name.port.txt")
  while read -r file src ops; do
    [ -f "$fix/$file" ] && continue
    mkdir -p "$fix"
    # shellcheck disable=SC2086
    cargo run -q -p bmp-game --example save_patch -- "$DATA/$src" "$fix/$file" $ops \
      && echo "fixture $name/$file patched"
  done < <(sed -n 's/^# fixture-edit: *//p' "tests/visual/$name.port.txt")
  if [ ! -d "$ref" ]; then
    cycles=$(sed -n 's/^# cycles: *//p' "tests/visual/$name.dosbox.txt")
    aim=$(sed -n 's/^# aim-seed: *\([0-9]*\).*/\1/p' "tests/visual/$name.dosbox.txt")
    env DOSBOX_CYCLES="${cycles:-${DOSBOX_CYCLES:-max}}" BMP_DOS_EXTRA="$([ -d "$fix" ] && echo "$fix")" \
      ${aim:+DOSBOX_SEED=$aim} \
      tools/dosbox-capture.sh "$ref" "tests/visual/$name.dosbox.txt"
  fi
  out=$(mktemp -d)
  data=$(mktemp -d)
  cp -r "$DATA"/. "$data" && chmod -R u+w "$data"
  [ -d "$fix" ] && cp -r "$fix"/. "$data"
  seed=$(sed -n 's/^# seed: *\([0-9]*\).*/\1/p' "tests/visual/$name.port.txt")
  seed2=$(sed -n 's/^# seed2: *\([0-9]*\).*/\1/p' "tests/visual/$name.port.txt")
  "$BIN" --data "$data" run --script "tests/visual/$name.port.txt" --shots "$out" \
    ${seed:+--time "$seed"} ${seed2:+--time2 "$seed2"} 2>/dev/null
  for shot in "$ref"/*.png; do
    base=$(basename "$shot")
    printf '%s/%s: ' "$name" "$base"
    "$BIN" compare "$out/$base" "$shot" --diff "$out/diff-$base" || { status=1; echo "  diff: $out/diff-$base"; }
  done
  for file in $(sed -n 's/^# compare-file: *\([^ ]*\).*/\1/p' "tests/visual/$name.port.txt"); do
    printf '%s/%s: ' "$name" "$file"
    if cmp -s "$data/$file" "$ref/$file"; then echo "identical"; else echo "DIFFERS"; status=1; fi
  done
  rm -rf "$data"
done
exit $status
