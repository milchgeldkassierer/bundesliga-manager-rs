#!/usr/bin/env bash
# Runs every quality gate locally: formatting, lints and the full test suite.
# Tests that need original game data use BMP_DATA (defaults to ./BMP when present).
set -euo pipefail
cd "$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
export PATH="$HOME/.cargo/bin:$PATH"
if [ -z "${BMP_DATA:-}" ]; then
  main_root=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")
  [ -d "$main_root/BMP" ] && export BMP_DATA="$main_root/BMP"
fi
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
