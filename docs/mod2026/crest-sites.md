# Crest draw sites

Every place where `BMMAIN.EXE` (the game) and `TEAMED.EXE` (the team editor) draw a club crest, with
the port's code, the original routine, where the picture number comes from and the rectangle the
crest covers. It is the basis for the high-resolution crests of R5b of [the PRD](PRD.md) (issue
#178); nothing here changes behaviour.

## Conventions

- **Crest pictures.** A German club's crest is `PIC/<n>.VGA` with `n` = 80..143, a 38x38 picture
  (format: `docs/re/vga.md`). `n` is block 1 of `MANA.DAT` (`docs/re/mana.md`): `TEAMED.EXE` uses
  the byte as is (`crest[i]`, `DS:5956`); `BMMAIN.EXE` stores it as the club's crest code
  `(b + 0x30) & 0x7f` (`mana_load_club_table` `362f:3759`) and draws `PIC/<(code & 0x7f) + 0x50>.VGA`,
  which is `PIC/<b>.VGA` again. A foreign club (team id 64 and up) in a live match draws
  `PIC/<row + 0x90>.VGA` of its live-match row instead.
- **Caches.** Most game screens do not load the picture; they copy from one of two caches:
  - `FAR:5354` (`DS:05b0` in the start menu, the port's `Pictures::crest_cache`): a blank 164x38
    bitmap with one 41-pixel cell per manager (`x = 41 * m`), filled by X3 or X4 below. Each cell
    holds the 38x38 crest and two black columns.
  - `DS:05b4` (the port's `LiveView::badges`): the 152x38 cache of the computer clubs' crests of
    one live screen, cell `x = 38 * b` (X5).
- **Coordinates.** `x, y, w, h` are decimal pixels in the frame named in the *Frame* column:
  - `screen`: the visible screen. `BMMAIN.EXE` runs mode X 320x240 (the port's `SCREEN_W` x
    `SCREEN_H`); `TEAMED.EXE` shows 242 lines of its 320x329 screen bitmap. Both are taller than
    320x200, so several crests (manager slots, editor preview) lie below row 200.
  - `screen p87`: the team editor's screen while the import shows it scrolled (`vga_show_page`
    mode 3, CRTC start row 87): bitmap row `y + 87` is shown at screen row `y`.
  - `cache …`: an off-screen bitmap (no pixel on screen changes; the crest shows when a later
    row copies it).

  The team editor draws its crest area through a view of the screen with origin `(0, 75)`
  (`DS:1DF6`); the rows give screen coordinates, i.e. view `y + 75`.
- **Kind.** `static`: drawn once when the screen (or the part of it) is drawn and left alone.
  `animated`: moved, scrolled or redrawn per frame or per pointer step while the screen is up.
- **Repeats.** Where one call draws the same crest box for several slots or cells, the row gives
  the first box and the column *Repeats* gives the step.
- **Ghidra.** The address of the original routine (or of the label inside it) from the `Origin:`
  line of the port function and `docs/re/logic/flow.md`, `docs/re/logic/match.md`,
  `docs/re/teamed/*.md`.

Columns of every table: *Row* (id used below) · *Site* · *Rust* `path:line` (function) · *Ghidra*
· *Picture source* · *Frame* · `x` · `y` · `w` · `h` · *Kind* · *Repeats / notes*.

## Start menu carousel

Start menu `start_menu_new_or_load` `18bc:2184` (`docs/re/logic/flow.md` §4): the strip of seven
crests under the header, the centre crest at x 141, and the four manager slots below.

| Row | Site | Rust | Ghidra | Picture source | Frame | x | y | w | h | Kind | Repeats / notes |
|---|---|---|---|---|---|---|---|---|---|---|---|
| C1 | carousel strip, full redraw | `crates/bmp/src/game/start_menu.rs:180` (`Screen::full_redraw`) | `18bc:22ca` | `PIC/<(CLUB[i].crest & 0x7f) + 0x50>.VGA`, clubs 0..6, drawn into the strip bitmap at `start_menu.rs:178` by `draw_club_crest` (`start_menu.rs:116`, `18bc:1cdd`) | screen | 17 | 48 | 287 | 38 | static | crest `k` of the strip at x `18 + 41k`; the centre crest (k 3) at x 141 |
| C2 | carousel strip, scrolling | `crates/bmp/src/game/start_menu.rs:247` (`Screen::scroll`) | `18bc:286a` | as C1; per step the strip shifts by one cell and `draw_club_crest` adds the new crest at strip x 1 (`start_menu.rs:226`, left) or 247 (`start_menu.rs:245`, right) | screen | 17 | 48 | 287 | 38 | animated | one step per 5 ticks while the button is held; second loop label `18bc:2a41` |
| C3 | manager slot crest box (existing managers) | `crates/bmp/src/game/start_menu.rs:154` (`Screen::full_redraw`) | `18bc:22ca` | crest cache `FAR:5354`, cell `41i` | screen | 22 | 114 | 38 | 38 | static | slot `i` at x `22 + 80i`, `i < n0` |
| C4 | manager slot crest box (club chosen) | `crates/bmp/src/game/start_menu.rs:330` (`Screen::slot_clicks`) | `18bc:2a90` | screen copy of the centre crest at (141, 48) | screen | 22 | 114 | 38 | 38 | static | slot `m` at x `22 + 80m`; on a click on the crest box |

## Office

| Row | Site | Rust | Ghidra | Picture source | Frame | x | y | w | h | Kind | Repeats / notes |
|---|---|---|---|---|---|---|---|---|---|---|---|
| O1 | desk panel crest of the current manager | `crates/bmp/src/game/office.rs:500` (`office_menu_dispatcher`) | `18bc:0c1c` | crest cache `FAR:5354`, cell `41 * cur` | screen | 187 | 60 | 38 | 38 | static | office redraw label, re-entered after every screen (`flow.md` §6.2) |

## Tables

None found.

Search: `grep -nE 'crest|crest_cache|picture\(|load_picture\(|\+ 0x50|0x90' crates/bmp/src/game/tables.rs`
returns only `tables.rs:67, 978, 993, 994, 995`, the club info's use of bit 7 of the crest byte as
the colour of the "ANZEIGEN" button (`3a41:0e4b`, `docs/re/audit/tables.md` row 14). No crest
picture and neither cache is drawn on the table screens.

## Match day

The live screen `match_draw_live_screen` `2a58:042f` draws up to four score panels (panel `k` at
`(161 * (k & 1), 115 * (k >> 1))`), each with the home and the away crest; the penalty shoot-out
redraws it with panel 0 only (`penalty_shootout_start`, `1310:356d`). A live ticker event
(`match_ticker_events` `2a58:0ca3`) copies both crests of its panel into the event panel, which
covers the top half (panels 2, 3) or the bottom half (panels 0, 1).

Picture source of M1..M8: the club of a manager `j` copies cell `41j` of the crest cache
`FAR:5354` (`live_match.rs:424`); a computer club loads `PIC/<(crest & 0x7f) + 0x50>.VGA`, or
`PIC/<row + 0x90>.VGA` for a foreign club (`live_match.rs:431`, `:433`), on the first draw of the screen
(`live_match.rs:438`, also cached by X5) and copies the badge cache `DS:05b4` on later draws
(`live_match.rs:445`).

| Row | Site | Rust | Ghidra | Picture source | Frame | x | y | w | h | Kind | Repeats / notes |
|---|---|---|---|---|---|---|---|---|---|---|---|
| M1 | live panel 0, home crest | `crates/bmp/src/game/live_match.rs:424` (`match_draw_live_screen_slots`) | `2a58:042f` | see above | screen | 22 | 35 | 38 | 38 | static | also the shoot-out's only panel |
| M2 | live panel 0, away crest | `crates/bmp/src/game/live_match.rs:424` (`match_draw_live_screen_slots`) | `2a58:042f` | see above | screen | 101 | 35 | 38 | 38 | static | also the shoot-out's only panel |
| M3 | live panel 1, home crest | `crates/bmp/src/game/live_match.rs:424` (`match_draw_live_screen_slots`) | `2a58:042f` | see above | screen | 183 | 35 | 38 | 38 | static | |
| M4 | live panel 1, away crest | `crates/bmp/src/game/live_match.rs:424` (`match_draw_live_screen_slots`) | `2a58:042f` | see above | screen | 262 | 35 | 38 | 38 | static | |
| M5 | live panel 2, home crest | `crates/bmp/src/game/live_match.rs:424` (`match_draw_live_screen_slots`) | `2a58:042f` | see above | screen | 22 | 150 | 38 | 38 | static | |
| M6 | live panel 2, away crest | `crates/bmp/src/game/live_match.rs:424` (`match_draw_live_screen_slots`) | `2a58:042f` | see above | screen | 101 | 150 | 38 | 38 | static | |
| M7 | live panel 3, home crest | `crates/bmp/src/game/live_match.rs:424` (`match_draw_live_screen_slots`) | `2a58:042f` | see above | screen | 183 | 150 | 38 | 38 | static | |
| M8 | live panel 3, away crest | `crates/bmp/src/game/live_match.rs:424` (`match_draw_live_screen_slots`) | `2a58:042f` | see above | screen | 262 | 150 | 38 | 38 | static | |
| M9 | event panel at the top, home crest | `crates/bmp/src/game/live_match.rs:987` (`ticker_event_start`) | `2a58:0ca3` | screen copy of the home crest of panel 2 or 3 (M5, M7) | screen | 20 | 5 | 38 | 38 | static | once per event |
| M10 | event panel at the top, away crest | `crates/bmp/src/game/live_match.rs:989` (`ticker_event_start`) | `2a58:0ca3` | screen copy of the away crest of panel 2 or 3 (M6, M8) | screen | 261 | 5 | 38 | 38 | static | once per event |
| M11 | event panel at the bottom, home crest | `crates/bmp/src/game/live_match.rs:987` (`ticker_event_start`) | `2a58:0ca3` | screen copy of the home crest of panel 0 or 1 (M1, M3) | screen | 20 | 118 | 38 | 38 | static | once per event |
| M12 | event panel at the bottom, away crest | `crates/bmp/src/game/live_match.rs:989` (`ticker_event_start`) | `2a58:0ca3` | screen copy of the away crest of panel 0 or 1 (M2, M4) | screen | 261 | 118 | 38 | 38 | static | once per event |

## Cup draws

None found.

Search: `grep -nE 'crest|crest_cache|picture\(|load_picture\(|\+ 0x50|0x90' crates/bmp/src/game/cup_screens.rs`
returns only `cup_screens.rs:468` and `:475`, the draw board `PIC/47` and its lamps `PIC/48`. The
draw screens show club names only; no crest picture and neither cache is drawn there.

## Team editor

`TEAMED.EXE` keeps all 64 crest pictures loaded (`DS:5698`, `PIC/<crest[i]>.VGA` from `MANA.DAT`
block 1, loaded at `crates/bmp/src/teamed/mod.rs:310`). The grid shows one page of 32 (8 x 4 cells
of 39 pixels); a selected club's crest goes into two boxes at the top and into the crest editor
(`docs/re/teamed/overview.md`, `docs/re/teamed/crest.md`).

| Row | Site | Rust | Ghidra | Picture source | Frame | x | y | w | h | Kind | Repeats / notes |
|---|---|---|---|---|---|---|---|---|---|---|---|
| T1 | crest grid, start-up (page 0) | `crates/bmp/src/teamed/mod.rs:315` (`init`) | `1000:1439` | `DS:5698[i]`, clubs 0..31 | screen | 0 | 87 | 311 | 155 | static | cell `i` (38x38) at `(39 * (i % 8), 87 + 39 * (i / 8))`, `crest_cell` |
| T2 | crest grid, one page | `crates/bmp/src/teamed/mod.rs:362` (`draw_page`) | `1000:2924` | `DS:5698[32p + i]` | screen | 0 | 87 | 311 | 155 | static | cells as T1; drawn on "LIGA" paging (`teamed/mod.rs:503`), deselect (`club.rs:310`) and the league-swap prompt (`club.rs:238`, edge clicks `club.rs:258`) |
| T3 | selected crest, left box | `crates/bmp/src/teamed/club.rs:64` (`select`) | `12a5:11b7` | `DS:5698[club]` | screen | 2 | 46 | 38 | 38 | static | on a click on a grid crest |
| T4 | selected crest, right box | `crates/bmp/src/teamed/club.rs:67` (`select`) | `12a5:11b7` | `DS:5698[club]` | screen | 115 | 46 | 38 | 38 | static | on a click on a grid crest |
| T5 | crest editor, enlarged crest | `crates/bmp/src/teamed/crest.rs:109` (`draw_editor`) | `1000:1d73` | the club's crest on select (`club.rs:82`), else the working copy (Undo, Ur-Wappen, after an import) | screen | 1 | 88 | 152 | 152 | static | 4x4 pixels per crest pixel, frame `(0, 87)-(153, 240)` |
| T6 | crest editor, preview | `crates/bmp/src/teamed/crest.rs:114` (`draw_editor`) | `1000:1d73` | as T5 | screen | 159 | 202 | 38 | 38 | static | real size |
| T7 | crest editor, painting the enlarged crest | `crates/bmp/src/teamed/crest.rs:193` (`click`) | `12a5:1821` | the paint colour, one crest pixel per pointer step | screen | 1 | 88 | 152 | 152 | animated | 4x4 block per step while the left button is held |
| T8 | crest editor, painting the preview | `crates/bmp/src/teamed/crest.rs:201` (`click`) | `12a5:1821` | as T7 | screen | 159 | 202 | 38 | 38 | animated | one pixel per step |
| T9 | crest import, crop preview | `crates/bmp/src/teamed/crest_import.rs:103` (`import`) | `12a5:1b16` | the 38x38 part of the imported IFF picture under the crop frame | screen p87 | 1 | 203 | 38 | 38 | animated | redrawn whenever the pointer moves the crop frame (view `(1, 0xd7)`, bitmap row 290) |

## Other

Crests drawn outside the six screens above: the manager header that several game screens share,
and the copies into the caches and pictures that the rows above draw from.

| Row | Site | Rust | Ghidra | Picture source | Frame | x | y | w | h | Kind | Repeats / notes |
|---|---|---|---|---|---|---|---|---|---|---|---|
| X1 | manager header (top right): crest box | `crates/bmp/src/game/widgets.rs:358` (`ui_draw_manager_header`) | `4091:185c` | crest cache `FAR:5354`, cell `41m` | screen | 267 | 62 | 40 | 38 | static | finance dialogs (`season_events.rs:228`, `:245`, `212a:0a6d`); the two cache columns right of the crest are black |
| X2 | manager header (top right): crest box | `crates/bmp/src/game/season_end.rs:119` (`ui_draw_manager_header`) | `4091:185c` | crest cache `FAR:5354`, cell `41m` | screen | 267 | 62 | 40 | 38 | static | season-end report, separations and contracts via `office_redraw_with_header` (`24a4:260a`) |
| X3 | crest cache fill after the start menu | `crates/bmp/src/game/start_menu.rs:787` (`run_menu`) | `18bc:25cb` | screen copy of slot `i`'s crest box (22 + 80i, 114) | cache `FAR:5354` | 0 | 0 | 40 | 38 | static | cell `i` at x `41i`, new managers `n0..count` |
| X4 | crest cache fill after loading a game | `crates/bmp/src/game/load_game.rs:661` (`screen_load_game`) | `422b:120c` | `PIC/<(CLUB[team].crest & 0x7f) + 0x50>.VGA` of manager `m`'s club | cache `FAR:5354` | 0 | 0 | 40 | 38 | static | cell `m` at x `41m` |
| X5 | badge cache of the live screen | `crates/bmp/src/game/live_match.rs:440` (`match_draw_live_screen_slots`) | `2a58:042f` | the picture M1..M8 load for a computer club | cache `DS:05b4` | 0 | 0 | 38 | 38 | static | cell `b` at x `38b`, `b` counts the computer clubs of the screen |
| X6 | edited crest kept after "AENDERUNG SICHERN" | `crates/bmp/src/teamed/crest.rs:257` (`save_crest`) | `12a5:1496` | the working copy | cache `DS:5698[sel]` | 0 | 0 | 38 | 38 | static | the grid (T2) then shows the edited crest; the file is written by `crest_write_vga` `1000:019c` |
| X7 | crest import, copy to the box (draws nothing) | `crates/bmp/src/teamed/crest_import.rs:129` (`import`) | `12a5:1f13` | the working copy, source position (115, 47) | screen | 0 | 75 | 38 | 38 | static | the source position lies outside the 38x38 picture, so the clipped blit draws nothing (original behaviour) |

## Grep coverage

`grep -rniE 'crest|wappen|PIC/' crates/bmp/src` at the head of this change returns 274 matches. Each
one is listed here, either with the row that covers it or as unrelated with the reason.
Row ids are unique across the document; the letter names the section (C start menu carousel,
O office, M match day, T team editor, X other).

### Matches referenced by a row

| Row | Matches |
|---|---|
| C1 | `crates/bmp/src/game/start_menu.rs:99,100,103,104,177,178` |
| C2 | `crates/bmp/src/game/start_menu.rs:225,226,244,245` |
| C3 | `crates/bmp/src/game/start_menu.rs:155` |
| C4 | `crates/bmp/src/game/start_menu.rs:323` |
| M1–M8 | `crates/bmp/src/game/live_match.rs:375,376,400,421,431,768` |
| M9–M12 | `crates/bmp/src/game/live_match.rs:913` |
| O1 | `crates/bmp/src/game/office.rs:1,501` |
| T1 | `crates/bmp/src/teamed/mod.rs:310,311,313,314,316,322,323,337,338,339,566,567,568,569,570` |
| T2 | `crates/bmp/src/teamed/mod.rs:352,361,363,369,370`; `crates/bmp/src/teamed/club.rs:229,230,263,281,303,314` |
| T3 | `crates/bmp/src/teamed/club.rs:25,52,54,60,61,65,68,74,75,77,78,82` |
| T5 | `crates/bmp/src/teamed/crest_import.rs:131,133`; `crates/bmp/src/teamed/crest.rs:21,26,63,65,66,67,70,71,93,102,103,105,106,227,231,232,234,236` |
| T6 | `crates/bmp/src/teamed/crest.rs:115,121,122` |
| T7 | `crates/bmp/src/teamed/crest.rs:183,185,190,211` |
| T9 | `crates/bmp/src/teamed/crest_import.rs:66,70,103`; `crates/bmp/src/teamed/crest.rs:223` |
| X1 | `crates/bmp/src/game/widgets.rs:335,344,358`; `crates/bmp/src/game/season_events.rs:190,196,228,245,278`; `crates/bmp/src/game/season.rs:262` |
| X2 | `crates/bmp/src/game/season_end.rs:100,106,119,130,131` |
| X3 | `crates/bmp/src/game/start_menu.rs:783,791` |
| X4 | `crates/bmp/src/game/load_game.rs:572,654,657,658,661` |
| X5 | `crates/bmp/src/game/live_match.rs:85,87` |
| X6 | `crates/bmp/src/teamed/crest.rs:243,244,245,247,248,249,253,255,257` |
| X7 | `crates/bmp/src/teamed/crest_import.rs:129` |

### Unrelated matches

| Matches | Reason |
|---|---|
| `crates/bmp/src/teamed/crest_import.rs:1,2,7,23`; `crates/bmp/src/teamed/crest.rs:1,2,4,5,6,7,9,10,19` | module docs and `use` line |
| `crates/bmp/src/teamed/crest_import.rs:122,123,125` | copies the chosen 38x38 part into the off-screen work and undo bitmaps (shown by T5/T6 afterwards) |
| `crates/bmp/src/teamed/crest_import.rs:141` | origin line of the IFF loader (draws the imported picture, not a crest) |
| `crates/bmp/src/teamed/crest_import.rs:201,211` | file browser of the import (text only) |
| `crates/bmp/src/game/start_menu.rs:22,23,28,32` | declares and allocates the blank crest cache `DS:05b0` that X3 fills |
| `crates/bmp/src/game/start_menu.rs:672` | doc comment of the `MenuEnd::Finished` result (the caching is X3) |
| `crates/bmp/src/game/start_menu.rs:732` | comment naming the carousel position variables |
| `crates/bmp/src/game/tables.rs:67,978,993,994,995` | the crest byte's bit 7 (club info "ANZEIGEN" flag, `3a41:0e4b`); no picture is drawn |
| `crates/bmp/src/teamed/dialog.rs:20` | dialog box drawn over the crest view (no crest) |
| `crates/bmp/src/teamed/crest.rs:52,57` | hit test / pixel mapping of the enlarged crest (input, no drawing) |
| `crates/bmp/src/teamed/crest.rs:143,145,146,150` | colour-field choice (frames a colour field, no crest) |
| `crates/bmp/src/teamed/crest.rs:153,157,158,159,160,163,165,173,174` | doc comment and colour pick of the editor's click handler; the painting is T7/T8 |
| `crates/bmp/src/teamed/crest.rs:261,268,269,272,273,279,282,283,284,285,292,323,325` | writes the edited crest to `PIC\<n>.VGA` and its unit test (file output, no drawing) |
| `crates/bmp/src/main.rs:26,86` | `bmp show-pic` developer viewer, draws any `PIC/` file full-frame; not a screen of either program |
| `crates/bmp/src/teamed/mod.rs:3,6,13,19,20,23,24` | module docs and `pub mod` lines |
| `crates/bmp/src/teamed/mod.rs:35,36,38,39,40,41,42,43` | constants of the grid (origin, crest size, pitch, per page) used by T1/T2 |
| `crates/bmp/src/teamed/mod.rs:46,47,48,49` | numbers of the import and crest-editor background pictures (not crests) |
| `crates/bmp/src/teamed/mod.rs:99,107,111,117,119,120,121,122,123,130,137,138,139,144,146,147,148` | fields of the editor state (crest buffers, page, change flag, paint colour, import state) |
| `crates/bmp/src/teamed/mod.rs:256,270,279,280,293,294,295,301,304,305` | start-up: loads the editor pictures and sets up the state (the grid draw is T1) |
| `crates/bmp/src/teamed/mod.rs:342` | doc comment of `clear_grid` (fills colour 0, no crest) |
| `crates/bmp/src/teamed/mod.rs:452,453` | allocates the two blank 38x38 scratch bitmaps (work copy, undo) |
| `crates/bmp/src/teamed/mod.rs:477,478,498,508,515,518,519` | main-loop input handling (hover, page flip, select, editor dispatch); draws go through T2, T3..T8 |
| `crates/bmp/src/teamed/mod.rs:543` | frees the crest bitmaps at exit |
| `crates/bmp/src/game/live_match.rs:285` | doc comment of the event counters drawn under a panel's crests (text, no crest) |
| `crates/bmp/src/game/live_match.rs:614` | hit box of a panel click (input, no drawing) |
| `crates/bmp/src/game/live_match.rs:1390,1392` | unit test of the panel click hit boxes (input, no drawing) |
| `crates/bmp/src/gfx.rs:88` | doc comment on the TEAMED bitmap origin (the view that T1/T2 draw through) |
| `crates/bmp/src/teamed/uefa.rs:65` | message box drawn over the crest view (no crest) |
| `crates/bmp/src/teamed/uefa.rs:137,158` | UEFA screen's way back to the crest grid (the grid is then T2) |
| `crates/bmp/src/game/load_game.rs:472` | doc comment: a save naming a missing crest picture is refused |
| `crates/bmp/src/game/load_game.rs:727,1040,1042,1044,1048,1058,1148,1152,1153` | tests of refusing a save whose crest code has no picture |
| `crates/bmp/src/teamed/club.rs:15` | `use` line |
| `crates/bmp/src/teamed/club.rs:290,300` | league swap of two clubs' data and crest numbers (no drawing) |
| `crates/bmp/src/teamed/club.rs:324,333,334,335,336` | "AENDERUNG SICHERN" button; the crest write is X6 |
| `crates/bmp/src/teamed/club.rs:499,510,515,516` | unit tests of the swap and commit (no drawing) |
| `crates/bmp/src/game/season.rs:102,103,104` | accessor for the crest cache `FAR:5354` used by M1..M8, X2 |
