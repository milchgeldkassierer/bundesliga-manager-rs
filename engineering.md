# Engineering conventions

See [factory workflow policy](factory/WORKFLOW_POLICY.md) for factory-specific shared-workflow requirements.

## Language
- **All source code and documentation are English only**: identifiers, comments, doc comments, log/panic messages, test names, commit-relevant code text. No German in `.rs`, scripts, config comments, docs, issues or PR text.
- User-facing game text stays exactly as in the original (German), loaded from or matching the original data/executable.

## Fidelity
- Every reconstructed function carries a doc comment with its origin, e.g. `/// Origin: BMMAIN.EXE 1a2b:0034 (FUN_1a2b_0034)`.
- Reproduce original arithmetic exactly (16-bit wrap-around, signedness, integer division truncation, RNG call order). Use explicit `wrapping_*`/`i16`/`u16` types where the original does.
- Graphics are compared against DOSBox reference frames; tolerance is defined in `docs/re/TOLERANCES.md`.

## Original data
- Never commit original game files or byte dumps of them. Tests needing data read `BMP_DATA` and skip if unset.

## Checks
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
