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
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
DATA=${BMP_DATA:?set BMP_DATA to the game directory}
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
  if [ ! -d "$ref" ]; then
    tools/dosbox-capture.sh "$ref" "tests/visual/$name.dosbox.txt"
  fi
  out=$(mktemp -d)
  seed=$(sed -n 's/^# seed: *\([0-9]*\).*/\1/p' "tests/visual/$name.port.txt")
  seed2=$(sed -n 's/^# seed2: *\([0-9]*\).*/\1/p' "tests/visual/$name.port.txt")
  "$BIN" --data "$DATA" run --script "tests/visual/$name.port.txt" --shots "$out" \
    ${seed:+--time "$seed"} ${seed2:+--time2 "$seed2"} 2>/dev/null
  for shot in "$ref"/*.png; do
    base=$(basename "$shot")
    printf '%s/%s: ' "$name" "$base"
    "$BIN" compare "$out/$base" "$shot" --diff "$out/diff-$base" || { status=1; echo "  diff: $out/diff-$base"; }
  done
done
exit $status
