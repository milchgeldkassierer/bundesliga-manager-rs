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
  start-up into `Host::mod2026`, the flag mod behaviour switches on.

## 2x display layer

- **Display buffer** (`crates/bmp/src/display.rs`, `crates/bmp/src/gfx.rs`, PRD R5b): with
  Mod 2026 data the SDL host keeps a second indexed buffer at twice the resolution (640x480,
  640x484 in the team editor) and presents it instead of the logical frame (`host.rs`,
  `video.rs`). It mirrors VRAM with one 2x2 block per VRAM byte, so every VRAM write of the
  library also draws into it: blits, fills, lines, pixels, text, page copies, clears, the mouse
  pointer and the palette slots; scrolling and blanking show it like the logical page. XOR
  draws apply to each display pixel; page copies and `blit_rect` carry the blocks. Memory
  bitmaps have no display side: `blit_bitmap` from the screen and the pointer background go
  through them and flatten a block to its doubled logical pixel, which the high-resolution
  crests and fonts must address. It uses the same DAC, so fades and palette changes are
  unchanged. For now every block is the doubled logical pixel, so the picture looks exactly as
  before.
  Hit tests, `get_pixel`, the cheat check, the spinners, screenshots and the visual tests read
  the logical frame as before.
- **Off switch** (`crates/bmp/src/main.rs`, `host::display_layer_on`): the layer is off with
  the original data, in the headless (`--script`) host and with the new option `--lowres` of
  `run` and `teamed`.

## Euro sign

- **Euro glyph** (`crates/bmp-formats/src/font.rs`, `crates/bmp-formats/data/euro_glyphs.txt`,
  `crates/bmp/src/gfx.rs`, `crates/bmp/src/host.rs`): the mod's texts write the euro sign as
  the byte code **0xd5** (`font::EURO`). 0xd5 is where IBM code page 858 puts the euro sign; in
  CP437 it is a box-drawing character the game never prints, it is not printable ASCII and
  none of the eight codes `map_char` translates, so v1.0.0 draws it as a space. The three
  glyph fonts **micro4** (5 x 6 pixels), **flow** (7 x 8) and **newbold** (13 x 16) got a
  euro glyph, new art in each font's style (its `C` with two bars), with the font's cell
  height, descent row and spacing column. With the mod flag `Gfx::set_euro_glyphs` installs
  them and text drawing and text widths use them for 0xd5; `map_char` and the font tables are
  unchanged, and without the flag 0xd5 stays a space. `encode_cp437('€')` gives 0xd5.
  `normal.font` stays a stub.
