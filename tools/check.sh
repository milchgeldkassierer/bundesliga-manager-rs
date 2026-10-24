#!/usr/bin/env bash
# Runs every quality gate locally: formatting, lints and the full test suite.
# Tests that need original game data use BMP_DATA. Default: the read-only pristine copy
# ${BMP_REFERENCE_DATA:-~/Apps/bmp-pristine} when present, else ./BMP. ./BMP is the owner's play
# directory and may hold his own saves, which must not break the tests.
set -euo pipefail
# Check the checkout we are run from (the hook may use a copy of this script).
cd "$(git rev-parse --show-toplevel)"
export PATH="$HOME/.cargo/bin:$PATH"
if [ -z "${BMP_DATA:-}" ]; then
  main_root=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")
  reference=${BMP_REFERENCE_DATA:-$HOME/Apps/bmp-pristine}
  if [ -d "$reference" ]; then
    export BMP_DATA="$reference"
  elif [ -d "$main_root/BMP" ]; then
    export BMP_DATA="$main_root/BMP"
  fi
fi
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
