# TEAMED.EXE – the team editor

Status: complete (#158). The whole editor is ported (`crates/bmp/src/teamed/`, `bmp teamed`):
start-up, main screen with the hover display, "LIGA" paging, save question, package club
(`club.md`), package crest with the IFF import (`crest.md`), package uefa with the strength
recomputation of the save (`uefa.md`) and the error screen `fatal_error_exit`.
`coverage.md` classifies every function and lists every input site.

All addresses are Ghidra addresses in `/unpacked/TEAMED_unpacked.EXE` (load segment
`0x1000`, see `docs/re/FUNCTION_MAP.md` §0). DGROUP is `2256`, the far data segment `1cb5`;
`DS:xxxx` below means `1cb5:xxxx` unless the segment table at `DS:0880..` says `2256`
(the code loads `ES` from that table, e.g. `[0x8bc]` -> `2256`). The editor functions were
renamed in Ghidra (`teamed_main_loop`, `grid_draw_page`, `dialog_two_choice`, ...); every
library function that matches BMMAIN carries BMMAIN's name and a comment with the match.

## Program flow

`main` (`1000:111a`):

1. `teamed_init_and_load_mana` (`1000:1439`), see "Start-up".
2. `teamed_main_loop` (`12a5:02bd`, 2365 instructions; the decompiler fails on it
   ("overlapping instruction"), read the listing).
3. `fatal_error_exit("OK.")` (`1000:0ee4`): text mode, message, exit.

There is no sound, no timer use and no `srand` call: `random_range` (`1000:29ec`, the same
code as BMMAIN `176b:0cc7`) runs on the MSC `rand()` start state (seed 1). Only the save
calls it (seven calls in `1579:0030`).

### Start-up (`1000:1439`)

| step | original | port (`teamed/mod.rs init`) |
|---|---|---|
| interrupt handler, EMS check | `install_int_handler(0,0)`, `ems_init` `1000:0d8d` | nothing to do |
| screen | `screen_open(DS:20D8)` (`1a46:0709`): screen struct with the 320x**329** bitmap at `A000:0000`, origin from the argument (0, 0); mode X with TEAMED's CRTC table (242 lines, see below) | `Gfx::vga_set_mode_x_teamed`, screen rastport |
| mouse | `mouse_open` (`1a46:07ad` = BMMAIN `4930:077e`); pointer at (160, 120) | `Gfx::mouse_open` |
| crest view | bitmap `DS:1DF6` = `{320, 329, A000:0000, origin (0, 0x4B)}`, rastport `DS:364A` on it; `DS:123A` is a struct whose `+8` points at that rastport | second VRAM bitmap with `origin = (0, 75)` |
| palette | screen off (`1579:0002(.., 0)`), `palette_load_rgb4(DS:00C0, 32)`, `palette_set_entry(0x20, 0, 0, 0)` | `PALETTE`, same calls |
| pictures | `picture_load` (`1000:1136`, = BMMAIN `176b:0356` without the CP path) of `PIC\63.VGA` (bar, `DS:0116`), `64` (`DS:011A`), `61` (crest editor, `DS:011E`), `60` (background, blitted 320x86 at (0,0), freed) | `Host::load_picture` |
| data | `MANA.DAT` read block by block (`1000:1762`, see `docs/re/mana.md`) | `bmp_formats::mana::ManaDat::parse` |
| crests | `grid_clear` (`1000:0000`), 64 crest pictures `PIC\<crest[i]>.VGA` (`DS:5698`), crests 0..31 blitted 38x38 at `((i%8)*39, (i/8)*39 + 12)` of the view | `crest_cell`, `clear_grid` |
| copies | `far_memcpy`: names `0042->21CA`, `4742->371E` (0xF56 bytes of BSS, a dead buffer: nothing else reads or writes it, see `club.md`), points `467C->57E4`, goals `364E->2058`, ranks `1E04->123E`, players `[5A02]->[213E]` | `Editor::copy` |
| finish | bar blitted (8x155 at (0x138, 0xC) of the view), `mouse_pointer_show`, screen on | same |

### Video: 242 lines and bitmap origins

* `vga_set_mode_x_320x240` (`16b7:1ca0`) is byte-identical to BMMAIN's `45ac:1c80` but its
  CRTC table (`16b7:039E`) has vertical display end `0xE3` instead of `0xDF`: DOSBox shows
  (and captures) **320x242**. The library's screen descriptor (`16b7:028A`) is 320x329.
  Port: `Gfx::vga_set_mode_x_teamed`, `Mode::ModeX242`.
* TEAMED's wrapper layer `1a46` (BMMAIN `4930`) allocates 12-byte bitmap descriptors
  `{w, h, far data, x0, y0}` and **adds the origin** in `rastport_draw_text` (`1a46:0215`),
  `rastport_line_to` (`02d0`), `rastport_put_pixel` (`038d`), `rastport_get_pixel`
  (`040d`), `rastport_fill_rect` (`044e`), `blit_bitmap` (`09a8`, both bitmaps) and the
  two transparent blits (`0878`, `0ae4`). The crest grid, the dialogs and the crest editor
  draw through the view `DS:1DF6` (origin y 75); the mouse code subtracts `DS:005A` (75)
  from the pointer y to get view coordinates. Port: `gfx::Bitmap::origin` (always (0, 0)
  in BMMAIN) and `Gfx::with_origin`.
* `vga_show_page` (`16b7:3231`) has a third mode (start address `0x1B30`, mouse range
  87..328) for a scrolled view: the UEFA list (`12a5:233d`) and the crest import
  (`12a5:1b16`, browser and crop frame) show rows 87..328 with it
  (`Gfx::vga_show_page_teamed`, see `uefa.md`, `crest.md`). Its other caller `12a5:2d39` is
  empty.
* Unlike BMMAIN's (`MUL BL`, rows wrap at 256), TEAMED's library computes VRAM row
  addresses with a 16-bit multiply (`MUL BX`, e.g. `16b7:1f36`, `24b2`): `Gfx::row_mask`.

## Screens and controls

All screens share the 320x242 screen. Coordinates are screen coordinates; "view" means
the crest view (screen y - 75).

### Main screen (ported)

Top 86 rows: `PIC\60.VGA` with the panel labels (MANNSCHAFT, STARTLIGA,
TAB.PLATZ, PUNKTE, TORE), three buttons (NICHT UEBERNEHMEN, AENDERUNG SICHERN,
PROGRAMM-ENDE) between two 38x38 boxes, the frame "ZUGEORDNETE SPIELER" with the position
letters of the 20 player slots. Below: 8x4 crests and the bar "LIGA"/"UEFA" at x 312..319.

| input | condition | effect | original | port |
|---|---|---|---|---|
| pointer over a crest | x < 0x137, view y 12..167, no club selected, club changed | panel (`panel_draw_hovered` `1000:24be`: name, league by group `(i>17)+(i>37)+(i>57)`, rank, `points[0]:points[1]`, `goals[0]:goals[1]`, font 3 colour 0x1D on 0xC) and player list (`12a5:0fd3`, slots 0..9 at x 0xA3, 10..19 at x 0xF6, row y `7i+0x12`, colour 0xB on 0xD) from the `MANA.DAT` buffers | `12a5:07a1` | `draw_hover_panel`, `draw_hover_players` |
| pointer elsewhere | a club was shown, none selected | panel cleared (0x33,3)-(0x95,0x29) colour 0xC; the player list stays | `12a5:10d8` | main loop |
| click bar, view y 12..89 | none selected | other page ("LIGA", clubs 32..63 / 0..31): screen off, `grid_draw_page` (`1000:2924`), screen on | `12a5:115d` | `draw_page` |
| click bar, view y > 89 | none selected | UEFA screen (`12a5:233d`, `uefa.md`) | `12a5:1157` | `uefa::run` |
| click on a crest | none selected | selects the club: crest into both boxes, crest editor in the lower half | `12a5:11b7` | `club::select` |
| click PROGRAMM-ENDE | x 0x2B..0x6F, y 0x47..0x54, none selected | save question, see below | `12a5:12f6`, `22b8` | `end_program` |
| click top-left pixel (0,0) | – | toggles `DS:0340` (initially 1) and paints (0x13D,0)-(0x13F,2) in colour `3 * (flag + 5)` (debug switch that gates the duplicate-name checks) | `12a5:04eb` | `club::toggle_debug` |

Before the loop the player names are upper-cased: `|` -> `\`, `{` -> `[`, `}` -> `]` in all
1280 slots (`12a5:02ca`); the save therefore writes the converted names (`fix_player_names`).

### Save question (ported, `dialog_two_choice` `1000:215b`)

A 0x9B x 0x4D box at view (0x4E, 0x33) (the background is saved in a blank picture and
restored): three centred lines "Daten" / "unwiederbringlich" / "abspeichern ?" (font 2,
colour 0x1D) and the buttons "MACH MAL !" / "OH NEIN !" (font 3, shadow). Returns 1 for the
left, 0 for the right button, 9 for a click outside; `teamed_save_mana` (`1579:0030`) writes
only on 1; the main loop goes on only on 9 (redraws the page) and otherwise ends.

### Club selected: panel, players, crest editor (packages club and crest; club: `club.md`)

Clicking a crest selects the club (`DS:0336`): the crest appears in both boxes, the lower
half becomes the crest editor (`crest_editor_draw` `1000:1d73`, `PIC\61.VGA`): the crest
enlarged 4x4 at the left, 32 colour fields, the buttons "Iff-Bild laden", "Undo",
"Ur-Wappen" and a 38x38 preview. With a club selected:

* panel fields (x 0x34..0x97, y 3..0x27, row `(y-3)/8`): line input (`text_input`
  `1000:0a7d`) for the name (duplicate check against the 200 names with `strcmp`,
  "DIESER TEAMNAME / EXISTIERT BEREITS."), the league (swap with another club: prompt
  "MIT WEM TAUSCHEN ?", "DIESER TABELLENPLATZ / EXISTIERT NICHT."), rank, points and goals
  (`parse_pair` `1000:207d`, "FALSCHE / EINGABE"); the panel then shows the copy
  (`panel_draw_selected` `1000:26f1`).
* player rows: name input in place (`12a5:202e..22ab`), swaps of slots via `mem_swap`
  (`1000:012b`, `12a5:0aba..0d6e`).
* "NICHT UEBERNEHMEN" / "AENDERUNG SICHERN" (x 0x2B..0x6F, y 0x2E..0x54, `12a5:1361`):
  copy back or discard (`far_memcpy` of the copies), write the crest
  (`crest_write_vga` `1000:019c`) and return to the grid.
* crest editor clicks (`12a5:1684..1fad`): picking a colour, painting pixels in the
  enlarged crest (`rastport_get_pixel`/`put_pixel`, frame around the chosen colour),
  "Undo", "Ur-Wappen", "Iff-Bild laden" -> `crest_import_dialog` (`1579:1215`: "Bitte
  waehlen Sie den Pfad aus, in dem sich die Wappen befinden.") with the directory browser
  `crest_import_browser` (`1579:05b7`: VERZEICHNIS, FILENAME, SEITE, "FALSCHES
  VERZEICHNIS !") and the IFF reader (`1000:0268`, `04f1`, `049e`; big-endian helpers
  `1000:0068`/`00b4`; "FALSCHES GRAFIKFORMAT.", "X<=320,Y<=200,256FARBEN").

### UEFA screen (package uefa, ported, `uefa_screen` `12a5:233d`, spec `uefa.md`)

Black screen, two columns of the 136 foreign clubs (64..199) with their three strengths
under "A M S" (`docs/re/mana.md` block 2), the bar stays at the right. Clicking a value
edits the club's name and its three strengths (`text_input`); a value that makes the
cup too easy (average of all foreign strengths below 79) is refused with
"EINGABE NICHT ANGENOMMEN, / UEFA-CUP SONST ZU LEICHT." (`dialog_message_uefa`
`1000:1bd2`, `uefa_strength_ok` `12a5:2ce0`).

## Data model

| buffer | content | file block |
|---|---|---|
| `DS:5956` | crest picture numbers, 64 | 1 |
| `DS:33CA` | strengths 200 x 3 | 2 (clubs 0..63 recomputed on save) |
| `DS:467C` / `DS:57E4` | points 64 x 2 / copy | 3 |
| `DS:364E` / `DS:2058` | goals 64 x 2 / copy | 4 |
| `DS:1E04` / `DS:123E` | ranks 64 / copy | 5 |
| `DS:0042` / `DS:21CA` | club names 200 x 23 / copy | 6 |
| `[5A02]` / `[213E]` | player names 64 x 20 x 26 (0x8200) / copy | 6 |
| `DS:5698` | crest pictures (bitmap handles) | – |
| `DS:0336` | selected club (0xFF none) | – |
| `DS:0340` | debug switch | – |

Files: `mana.dat` (read at start, written by "MACH MAL !"), `pic\<n>.vga` (crests, read;
written by the crest editor), IFF pictures from any directory (crest import).

## Library reuse

Matched by instruction sequence (mnemonics, LCS similarity) against
`/unpacked/BMMAIN_unpacked.EXE`; 1.00 = same instructions (addresses differ).

| TEAMED | BMMAIN | port |
|---|---|---|
| `16b7` (whole segment, 1.00 except below) | `45ac` graphics/mouse library | `crate::gfx::Gfx` |
| `16b7:1ca0` vga_set_mode_x_320x240 | `45ac:1c80` (same code, other CRTC table) | `Gfx::vga_set_mode_x_teamed` |
| `16b7:3231` vga_show_page (0.85, third mode) | `45ac:327d` | `Gfx::vga_show_page_teamed` |
| `16b7` VRAM row address (`MUL BX` instead of `MUL BL`) | `45ac` | `Gfx::row_mask` |
| `16b7:382b` ilbm_decode_body (TEAMED only) | – | `bmp_formats::iff::decode_body` |
| `1a46:000c..01e5`, `028b`, `02b1`, `04ee..051e`, `05c6`, `05ff`, `06cc`, `07ad..0855`, `0c14` | `4930` file, font, rastport, palette, mouse wrappers (1.00) | `Gfx` rastport layer, `Host` |
| `1a46:0215`, `02d0`, `038d`, `040d`, `044e`, `09a8`, `0878`, `0ae4` (0.82..0.90) | same `4930` wrappers plus the bitmap origin | `Gfx::with_origin` in the shared wrappers |
| `1a46:0646` palette_load_rgb4 (0.87) | `4930:0624` without the palette-slot branch | `Gfx::palette_load_rgb4` |
| `1a46:0709` screen_open (320x329, origin) | `4930:0712` | `teamed::init` |
| `1000:0663`, `06ac`, `0909`, `0949`, `0e7d` | `16c7` ui_draw_line, ui_draw_frame, ui_fill_rect, ui_draw_text_centered_shadow, ui_draw_text_at | `gfx::ui_*` |
| `1000:0859`, `13dd`, `29ec`, `0e01` | `176b:02e4` ui_blit_transparent, `0dca` bitmap_free, `0cc7` random_range, `0c4b` | `Gfx`, `bmp_game::rng::Rng` |
| `1000:0e47` ui_set_font (other name table: 1 newbold, 2 flow, 3 micro4) | `176b:0c91` | `teamed::set_font` |
| `1000:072e` format_long_thousands (0.91, no comma variant) | `176b:0681` | `bmp_game::rng::format_long_thousands` (`teamed::format_number`) |
| `1000:088e` / `08c8` input_wait_click / input_poll_mouse (0.83/0.97) | `176b:0d0a` / `0319` | `Host::input_poll_mouse` |
| `1000:1136` picture_load (0.89) | `176b:0356` | `Host::load_picture`, `Host::blank_bitmap` |
| `1000:10e6`, `2446`, `2a2f` | `4091` long_abs_diff, text_to_upper_de, clamp_int | `bmp_game` helpers |
| `12a5:0087` / `000a` (0.92 / 0.79) | `4091:123d` ui_register_icon_slot / `11cc` ui_hit_icon_slot | `game::widgets::TextButtons::register_on` / `hit` |
| `1b1f` (whole segment) | `4a01` MSC CRT | Rust std |

Editor-only code (to port): `1000:0000..0268`, `049e`, `04f1`, `0a7d..0d8c`, `0ee4`,
`1439`, `1a31`, `1bd2`, `1d73`, `207d`, `215b`, `24be`, `26f1`, `2924`, `12a5:*`,
`1579:*` (about 7000 instructions, half of it the main loop).

## Verification

* `tools/dosbox-capture.sh`: a `# exe: TEAMED.EXE` line (or `DOSBOX_EXE`) starts the editor
  instead of `BMP.BAT`; `fetch MANA.DAT` copies the written file.
* `tools/visual-check.sh`: a `# command: teamed` line runs `bmp teamed` on the port side;
  `# compare-file: MANA.DAT` compares the files byte for byte.
* `tests/visual/teamed_e2e.*`: one session across every screen with the save; `MANA.DAT`
  and the crest file are byte-identical to the original's (see `coverage.md`).
* `tests/visual/teamed_fatal.*`: the "DISK" error screen for a missing crest picture.
* `tests/visual/teamed_start.*`: start screen, hover, panel clearing, "LIGA" page 2, save
  question (all 100 %). In this 242-line mode the DOSBox pointer lands about one pixel left
  of the requested x, so the DOSBox script uses slightly different x values.

## Feature packages

Each package owns its source file(s) and spec, plugs in at the `TODO(#158)` stubs and
extends `tests/visual/` with its own scenario (`teamed_<key>.*`). Shared stubs in
`teamed/dialog.rs`: `message` and `text_input` were implemented by package club; the others
call them.

### Package `club` – selecting a club and editing its data (ported, `club.md`)

* Functions: `12a5:11b7..12db` (select), `12a5:056b..07a1` and the swap sub-loop
  `0869..0a8f` (panel editing), `12a5:0aba..0d6e`, `0d9a..0ef2`, `0f18..0fd0` (players,
  rank/points/goals checks), `12a5:1361..1544` (NICHT UEBERNEHMEN / AENDERUNG SICHERN),
  `12a5:202e..22ab` (player name input), `12a5:04eb` (debug switch), `1000:0a7d..0d8c`
  (`text_input`), `1000:1a31` (`dialog_message`), `1000:207d` (`parse_pair`), `1000:26f1`
  (`panel_draw_selected`), `1000:012b` (`mem_swap`), `1000:0068`/`00b4` only if needed;
  `DS:4742` (0xF56 bytes) is a dead buffer.
* Screens: main screen with a selected club (panel, player list, both boxes).
* Data: names, ranks, points, goals, player names and their copies.
* Files: `crates/bmp/src/teamed/club.rs`, `teamed/dialog.rs` (bodies of `message`,
  `text_input`); spec `docs/re/teamed/club.md`.

### Package `crest` – crest editor and IFF import (ported, see `crest.md`)

* Functions: `1000:1d73` (`crest_editor_draw`, already ported by package club as
  `crest::draw_editor`), `12a5:1684..1fad` (painting, colour pick,
  Undo, Ur-Wappen, preview), `1000:019c` (`crest_write_vga`, via `crest::save_crest`),
  `1579:1215` (`crest_import_dialog`), `1579:05b7` (`crest_import_browser`),
  `12a5:0087`/`000a` (icon slots), `1000:0268`, `049e`, `04f1` (IFF reading and conversion),
  `1000:0068`/`00b4` (byte order).
* Screens: lower half while a club is selected, the import dialog and browser.
* Data: crest pictures (`DS:5698`), `PIC\<n>.VGA` files (`bmp_formats::vga` encoder), the
  crest number block of `MANA.DAT`.
* Files: `crates/bmp/src/teamed/crest.rs` (+ `crest_import.rs`); spec
  `docs/re/teamed/crest.md`.

### Package `uefa` – UEFA strengths and the save

* Functions: `12a5:233d` (`uefa_screen`), `12a5:2ce0`, `1000:1bd2`
  (`dialog_message_uefa`), the strength recomputation in `1579:006d..04b5`
  (`save::recompute_strengths`, seven `random_range` calls, `crt_afldiv` `1b1f:1700`),
  `bmp_formats::mana` round trip.
* Screens: UEFA list; the save question is done.
* Data: strength block (all 200 clubs), points/goals/ranks as inputs.
* Files: `crates/bmp/src/teamed/uefa.rs`, `teamed/save.rs`; spec `docs/re/teamed/uefa.md`;
  a scenario that saves with "MACH MAL !" and compares `MANA.DAT` (`# compare-file:`).
