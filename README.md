# BMP Remake – Bundesliga Manager Professional (Linux/Rust port)

Goal: a 1:1 port of the DOS game *Bundesliga Manager Professional* (Software 2000, 1993) to Linux in Rust,
based on reverse engineering of the original executables (`BMMAIN.EXE`, `BMINTRO.EXE`, `BMLOADER.EXE`, `TEAMED.EXE`)
with Ghidra. Graphics must match the original pixel for pixel within minimal tolerances
(320×200, 256 colors, original palettes; see `docs/re/TOLERANCES.md`).

**No original game data is shipped.** You need your own copy of the original game
(pass its directory via `--data <dir>` or `BMP_DATA`).

## Layout
- `crates/bmp-formats` – parsers for `.VGA`, `.CP`, `.PAL`, `TORE/*.T|V`, `MANA.DAT`, `*.MAN`, `.CMF`, `.VOC`
- `crates/bmp-game` – game logic reconstructed from BMMAIN.EXE
- `crates/bmp-sound` – OPL2 (YM3812) emulation, SBFMDRV-compatible CMF player, digitised voices
- `crates/bmp` – binary: rendering (320×200 framebuffer), input, sound output (SDL2)
- `docs/re/` – reverse-engineering notes (file formats, function map, memory layout)
- `tools/` – Ghidra scripts and helper tools

Progress: see GitHub issues and milestones.
