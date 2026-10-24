# Mod 2026: changes to the original behaviour

Every change of the port's v1.0.0 behaviour made for Mod 2026 (`PRD.md`) is listed here and
marked `// MOD2026:` in the code. All of them switch on only when the data directory contains
the marker file `MOD2026.DAT`; without it the port behaves exactly like v1.0.0.

## Generator and mod switch

- **`bmp mod2026 build --from <dir> --out <dir>`** (`crates/bmp/src/main.rs`,
  `crates/bmp-game/src/mod2026.rs`): writes the mod data directory, a deterministic copy of
  every file below `--from` plus the marker `MOD2026.DAT` with fixed content. Into the copy of
  `MANA.DAT` it writes the 64 German club slots from the dataset `docs/mod2026/data/`
  (`clubs.json`, `squads.json`, embedded in the program): the name, the crest picture number,
  the 20 player names (slots without a player all zero bytes, as in the original), the 2025/26
  table as the 2-point pair `[2W+D, 2L+D]`, the goals and the rank, and the strengths (below).
  The 136 foreign clubs keep their bytes; an invalid `MANA.DAT` or a dataset entry that does
  not fit it (a name too long, more than 20 players, a number above 255) is refused before
  anything is written; the dataset's own rules are checked by its tests. `--out` must not exist or be an empty directory, and is never
  the `--from` directory (or inside it) nor `<repo>/BMP` (or inside it); these are refused with a
  non-zero exit code before anything is written. A new command, so the original game is not
  affected.
- **Crest pictures** (`crates/bmp-game/src/mod2026.rs`, `crates/bmp-game/build.rs`, PRD R5):
  the generator encodes every `docs/mod2026/crests/<n>.png` (embedded in the program by
  `build.rs`) with the game's compressor `vga::encode`, in the PNG's size and palette indices,
  and writes it as `PIC/<n>.VGA`, replacing the copied original picture; picture numbers
  without a PNG keep the original. A crest PNG that is not 8-bit indexed is refused before
  anything is written. The game and the team editor load the crests unchanged, so the mod
  clubs show their 2026 crests wherever a crest is drawn.
- **Strength grouping** (`crates/bmp-game/src/strength.rs`, `strength::MOD2026`): the team
  editor's strength recomputation moved unchanged from `bmp` to `bmp-game`, with its league
  grouping as a parameter. The generator runs it with the groups 0..17 / 18..35 / 36..55 /
  56..63 (Bundesliga, 2. Bundesliga, 3. Liga, pool), base strengths 70 / 34 / 20 / 20 and
  league sizes 18 / 18 / 20 / 20 for the rank term `size - rank`, and no random group: unlike
  the original's amateurs (group 2, random strengths), the 3. Liga and the pool get strengths
  from their 2025/26 table. Ranks are used as stored (a promoted club's rank in its 2025/26
  league). The 2. Bundesliga scaling is unchanged; the RNG starts at the MSC start state
  (`srand(1)`), so the output is deterministic. The team editor keeps the original grouping.
- **Mod switch** (`bmp_game::mod2026::is_active`): the game and the team editor read it once at
  start-up into `Host::mod2026`, the flag mod behaviour switches on.

## League structure

- **One owner** (`crates/bmp-game/src/league_layout.rs`, PRD R1): the league tables of the
  original (DS:225e..DS:2278: first club, club count and rounds per tier, relegation and play-off
  places, the club ranges) and the season calendar (DS:2280) are one `LeagueLayout`.
  `game::run_main` selects `MOD2026` when `Host::mod2026` is set and `ORIGINAL` otherwise,
  before any game state is built; `Runtime::default` and `GameState::program_start` copy it,
  every other reader calls `league_layout::layout()`.
- **Mod values**: 18 / 18 / 20 clubs with the ids 0..=17, 18..=35 and 36..=55, the pool
  56..=63 (no league), 34 / 34 / 38 rounds, 2 / 2 / 4 clubs relegated directly and a play-off
  place in the Bundesliga and the 2. Bundesliga. Tables, fixtures, statistics, the top scorers,
  the history graph and the season end follow them; the fixtures of the 18-club 2. Bundesliga
  are the Bundesliga's round permutations. The history graph's bars and the high score's
  position and season terms count from the league clubs (the original's immediates 0x3a, its 58
  league clubs, and 0x39, its last league club id), so the last club of the 3. Liga draws one
  line and scores nothing in both layouts. The save format is unchanged: the 18 / 18 / 20 tiers
  fit the `[tier][20]` and `[tier][38][20]` blocks.
- **Calendar**: the 2. Bundesliga plays on the Bundesliga's days. Week 23 (only a Bundesliga
  round, `0x01`) gets a 2. Bundesliga round (`0x03`), weeks 7, 13, 25, 29 and 69 (2. Bundesliga
  and 3. Liga, `0x06`) keep only the 3. Liga round (`0x04`). Without this the 2. Bundesliga
  would play four more match days that replay round 17 without counting.
- **DFB-Pokal draw** (PRD R2): the draw takes clubs up to the last club of the lowest tier
  (DS:2277, 57 in the original), i.e. 0..=55 in the mod; the pool stays out like the original's
  reserve clubs 58..63.
- **Promotion and relegation**: the 3. Liga's four relegated clubs swap with pool clubs
  (59 or 60, then 58, 57, 56). Only the Bundesliga play-off is played, so for now the
  2. Bundesliga's play-off place goes down directly and three clubs of the 3. Liga go up; the
  tables mark three promotion places in the 3. Liga.
- **Not yet**: the 2. Bundesliga / 3. Liga play-off, the "3. LIGA" texts, which clubs fill the
  pool (#177) and the team editor's tier groups (`crates/bmp/src/teamed/`, still 18 / 20 / 20).

## 2x display layer

- **Display buffer** (`crates/bmp/src/display.rs`, `crates/bmp/src/gfx.rs`, PRD R5b): with
  Mod 2026 data the SDL host keeps a second indexed buffer at twice the resolution (640x480,
  640x484 in the team editor) and presents it instead of the logical frame (`host.rs`,
  `video.rs`). It mirrors VRAM with one 2x2 block per VRAM byte, so every VRAM write of the
  library also draws into it: blits, fills, lines, pixels, text, page copies, clears, the mouse
  pointer and the palette slots; scrolling and blanking show it like the logical page. XOR
  draws apply to each display pixel; page copies and blits carry the blocks. A memory bitmap
  gets a display side (one block per pixel) once a block other than its doubled pixel is
  written into it, so `blit_bitmap` from the screen and the pointer background keep the blocks
  instead of flattening them. Transparent blits are transparent per display pixel: display
  pixels 0 of the source keep the destination's, also where the logical pixel is 0. It uses
  the same DAC, so fades and palette changes are unchanged. Text has 2x detail (**2x fonts**
  below); every other block is the doubled logical pixel.
  Hit tests, `get_pixel`, the cheat check, the spinners, screenshots and the visual tests read
  the logical frame as before.
- **2x fonts** (`docs/mod2026/fonts/`, `crates/bmp-formats/src/hires_font.rs`,
  `crates/bmp/src/gfx.rs`, `crates/bmp/src/host.rs`, PRD R5b): micro4, flow and newbold have a
  2x version, the Scale2x image of every original glyph, generated by
  `cargo run -p bmp-formats --example hires_fonts -- <data dir>` and embedded in the program
  (`docs/mod2026/fonts/README.md`). The host installs them with the display layer
  (`Gfx::set_hires_fonts`); `draw_text` then gives each glyph's temporary bitmap the 2x glyph
  as its display side, in the foreground colour and the background colour (opaque) or 0
  (transparent). The logical frame, every advance, text width and centring are unchanged. The
  euro sign has no 2x glyph yet and is drawn doubled (#205). `normal.font` stays a stub.
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
