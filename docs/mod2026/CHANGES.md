# Mod 2026: changes to the original behaviour

Every change of the port's v1.0.0 behaviour made for Mod 2026 (`PRD.md`) is listed here and
marked `// MOD2026:` in the code. All of them switch on only when the data directory contains
the marker file `MOD2026.DAT`; without it the port behaves exactly like v1.0.0.

## Generator and mod switch

- **`bmp mod2026 build --from <dir> --out <dir>`** (`crates/bmp/src/main.rs`,
  `crates/bmp-game/src/mod2026.rs`): writes the mod data directory, a deterministic copy of
  every file below `--from` plus the marker `MOD2026.DAT` with fixed content. The original data
  is copied unchanged for now. `--out` must not exist or be an empty directory, and is never
  the `--from` directory (or inside it) nor `<repo>/BMP` (or inside it); these are refused with a
  non-zero exit code before anything is written. A new command, so the original game is not
  affected.
- **Mod switch** (`bmp_game::mod2026::is_active`): the game and the team editor read it once at
  start-up into `Host::mod2026`, the flag later mod behaviour switches on. Nothing reads it yet.
