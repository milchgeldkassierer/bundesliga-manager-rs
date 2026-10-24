# TEAMED.EXE – crest editor and IFF crest import (package `crest`)

Port: `crates/bmp/src/teamed/crest.rs` (editor, crest file), `crates/bmp/src/teamed/crest_import.rs`
(import, browser), `crates/bmp-formats/src/iff.rs` (IFF reader and decoder). Scenario:
`tests/visual/teamed_crest.*` (18 frames against DOSBox, all 100 %; the two written
`PIC\*.VGA` files are byte-identical to the original's). Addresses are Ghidra addresses in
`/unpacked/TEAMED_unpacked.EXE`; "view" coordinates are relative to the crest view
`DS:1DF6` (screen y - 75), see `overview.md`.

## State (main-loop locals and globals)

| original | meaning | port |
|---|---|---|
| `[BP-58]` | working crest, blank 38x38 picture (rastport) | `Editor::scratch[0]` |
| `[BP-C8]` | Undo copy, blank 38x38 picture | `Editor::scratch[1]` |
| `[BP-04]` | colour to paint with, -1 at start, kept across clubs | `Editor::crest_color` |
| `[BP-DA]` | crest changed (set by painting and the import, cleared by "Ur-Wappen" and on deselect) | `Editor::crest_changed` |
| `DS:5698[i]` / `DS:1E48[i]` | crest picture of club `i` / its bitmap pointer (the same crest) | `Editor::crests` |
| `DS:050C`, `DS:20DC` | browser drive (-1 = not yet) and directory, kept between imports | `ImportState::dir` (`SaveSession`) |
| `DS:1DE2` | `BMHD` of the last picture read (a file without `BMHD` reuses it) | `ImportState::bmhd` |
| `[BP-54]` | browser result / file-name line buffer (stale bytes survive) | `ImportState::name_buf` |

## Selection (`12a5:11b7`, package club) and `crest_editor_draw` (`1000:1d73`)

The selection blits the crest into both boxes (screen (2, 0x2E), (0x73, 0x2E)), into `[BP-C8]`
and `[BP-58]`, then calls `crest_editor_draw(crest[club], 0)` (`12a5:12d8` -> `1fad`).

```
crest_editor_draw(pic, use_copy):
  players = use_copy ? [213E] : [5A02]
  screen off; grid_clear (view (0,0xC)-(0x13F,0xFE) colour 0)
  ui_set_font(view, 2)
  ui_fill_rect(view, 0x9E,0xC, 0x13F,0xA6, 6); ui_draw_frame(view, 0x9D,0xC, 0x13F,0xA6, 0xB)
  if DS:0336 != 0xFF: for i in 0..10 (screen rastport):
      fill (0xA3,(i+2)*7)-(0xEA,7i+0x12) 0xD;  fill (0xF6,(i+2)*7)-(0x13D,7i+0x12) 0xD
      text players[sel*20+i] at (0xA3, 7i+0x12) colour 0xB; players[sel*20+i+10] at (0xF6, ..)
  blit PIC\61 (0,0) -> view (0x9D,0xC) 0xA8 x 0x9B
  for x in 0..38, y in 0..38: colour = get_pixel(pic, x, y)
      fill view (4x+1, 4y+0xD)-(4x+5, 4y+0x11)        (5x5 blocks, 4-pixel pitch)
  ui_draw_frame(view, 0,0xC, 0x99,0xA5, 0xB)
  ui_fill_rect(view, 0x9E,0x7E, 0xC5,0xA5, 0); blit pic -> view (0x9F,0x7F) 38x38
  screen on
```

`PIC\61` holds the 32 colour fields (8 x 4, 0x14 x 0x10 from view (0x9E, 0xD)) and the buttons
"Iff-Bild laden", "Undo", "Ur-Wappen" (rows 4..6 of the same 0x10 grid).

## Crest editor clicks (`12a5:1671..2028`)

Reached for every event that is not a selection, "PROGRAMM-ENDE" or a club button (also the
right button). `x`, `y` = pointer, `y` in view coordinates; all checks need a selected club.

```
A (1671) ev == 3 and 1 < x < 0x99 and 0xC < y < 0xA5:
    old = [BP-4]; [BP-4] = c = rastport_get_pixel(view, x, y)
    if old != -1: ui_draw_frame(field(old), 0)            erase
    ui_draw_frame(field(c), 0x1D); end of iteration
    field(c) = ((c%8)*0x14+0x9F, (c/8)*0x10+0xE, (c%8)*0x14+0xB0, (c/8)*0x10+0x1B)   (C division)
B (17bf) ev == 1 and 1 < x < 0x99 and 0xC < y < 0xA5:
    blit [BP-58] -> [BP-C8]                               Undo = before this stroke
    loop:
      if 1 < x < 0x99 and 0xC < y < 0xA5:
        px = (x-1)>>2, py = (y-0xD)>>2; colour [BP-4] (low byte) in view and work
        fill view (4px+1, 4py+0xD)-(4px+4, 4py+0x10); put_pixel view (px+0x9F, py+0x7F)
        put_pixel [BP-58] (px, py)
      input_poll_mouse; x, y = pointer
      until the event is 2 (left up)
    [BP-DA] = 1, fall through to C with the release position
C (19d7) ev == 1 and x > 0x9E and 0xC < y < 0x9C: row = (y-0xD)/0x10; jump table 12a5:201a
    0..3 (1a49): col = (x-0x9E)/0x14; erase frame of [BP-4] (if != -1); frame field(row*8+col)
                 in 0x1D; [BP-4] = row*8+col   (x up to 319 gives col 8 = next row's colour)
    4 (1b16): import, see below
    5 (1f7d): blit [BP-C8] -> [BP-58]; crest_editor_draw([BP-58], 1)
    6 (1fb8): blit crest[sel] -> [BP-58]; crest_editor_draw([BP-58], 1); [BP-DA] = 0
    every other row: nothing; C always ends the iteration
otherwise (2028): the player-name editing of package club.
```

A click (press and release in one poll) paints one pixel. Undo only takes back the last stroke.

## "AENDERUNG SICHERN" with a changed crest (`12a5:1496..1544`) and `crest_write_vga` (`1000:019c`)

After the copies are committed (package club): if `[BP-DA]`: `sprintf "pic\" + itoa(crest[sel]) +
".vga"`, `crest_write_vga(*[BP-58], name)`, blit `[BP-58]` -> `crest[sel]` (the grid shows it).

`crest_write_vga`: buffer of 0x7D0 bytes (`fatal_error_exit("SPEICHER")` when none), `+0` = 0x26,
`+2` = 0x26, `+4` = `rle_encode(bitmap data, buf+6, 0x5A4)` (`16b7:3195`, the library's encoder,
`bmp_formats::vga::encode_rle`), `file_open(name, 1)`, write `len + 6` bytes, close, free. The
encoder may read one byte past the pixels; with the observed heap that byte never matched
(the scenario's two files are identical to the original's).

## Import (`12a5:1b16..1f7a`)

```
vga_show_page(3)                  start address 0x1B30: rows 87..328 shown, mouse y range 87..328
DS:1E44 (screen rastport) = view rastport, DS:59FE (screen bitmap) = view bitmap
name = crest_import_browser("", buf)      (1579:05b7)
restore DS:1E44 / DS:59FE
if name == "": vga_show_page(1); end
screen off
if !crest_import_load(name) (1000:049e):
    vga_show_page(1); screen on; dialog_message("FALSCHES GRAFIKFORMAT.", "X<=320,Y<=200,256FARBEN"); end
ui_fill_rect(view, 0,0xD6, 0x27,0xFE, 0xB)   preview box;  set_draw_mode(view, 2);  screen on
wait until no mouse event (1000:00fb)
old = (-100, 0)
loop:
    poll; x = pointer x, y = pointer y - 75
    if x > 0x118 or y > 0xD6: clamp both, mouse_set_position(x, y + 75)
    if (x, y) != old: frame(old) colour 1 (xor: erase), frame (x,y)-(x+0x27,y+0x27) colour 1,
                      blit view (x+1, y+1) -> view (1, 0xD7) 38x38; old = (x, y)
    until event 3 or 1
frame(old) (erase); set_draw_mode(view, 0)
if event 1: blit view (x+1,y+1) -> [BP-58]; blit [BP-58] -> [BP-C8];
            blit [BP-58] from (0x73,0x2F) -> view (0,0)   (source position outside the 38x38 picture:
                                                         clipped away; probably meant for the box)
crest_editor_draw([BP-58], 1); vga_show_page(1); [BP-DA] = 1     (also after a cancel)
```

### Reader (`1000:0268`) and loader (`1000:049e`, `04f1`)

`open(name)`; 8 bytes must be `FORM`, then 4 bytes `ILBM` or `PBM ` (the type is not kept).
Chunks (8-byte header, length big-endian via `bytes_reverse4` `1000:0068`):

| chunk | action |
|---|---|
| `BMHD` | 20 bytes into `DS:1DE2` (fixed length) |
| `CMAP` | `length` bytes into a 0x300-byte stack buffer, never used (no pad byte) |
| `CAMG` | `length` bytes into a 4-byte local, never used |
| `BODY` | `alloc(length)` into `DS:4734` (`"SPEICHER"` when none), read, stop |
| other | `seek((length + 1) & ~1, SEEK_CUR)` |

Then the file is accepted when the `BMHD` width <= 320 and height <= 200 (signed,
`bytes_reverse2` `1000:00b4`). `file_read` only fails on DOS errors: at the end of a file without
`BODY` the loop never ends (port: refused, DEVIATION).

`crest_import_convert` (`1000:04f1`): `w`, `h`, planes, compression == 1;
`buf = alloc((w/8) * planes * h)`, `bitmap_init(desc, 1, w, h)`, `desc.data = alloc(w*h, zeroed)`,
`ilbm_decode_body(body, buf, w, h, planes, compressed, desc.data)` (`16b7:382b`), blit desc ->
view at `((320-w)/2, (200-h)/2 + 0xD)` w x h, free everything.

`ilbm_decode_body`: 8 planes = chunky rows of `w` bytes; other plane counts = interleaved
planes of `w >> 3` bytes per row (no word padding). Compressed rows are ByteRun1, where
`n = -128` repeats 129 times. Planar rows then set bit `p` of each pixel for plane `p`.

**Uninitialised descriptor.** `desc` is the far pointer `[BP-10]` of `1000:04f1`, which is never
set: `bitmap_init` writes the descriptor wherever the stack garbage points, and the blit reads it
back from there. That slot is the bytes `0x2F8..0x2FB` of `1000:0268`'s `CMAP` buffer (the
frames differ by the 4 extra argument bytes), i.e. colours 253/254 of a 256-colour `CMAP`. In
DOSBox a file without `CMAP` leaves a pointer that makes the picture invisible (the crop then
takes the editor's own pixels); the scenario's files carry a `CMAP` pointing at the free DOS
data area `0050:0010`, so the original draws them. DEVIATION: the port always draws the
picture with a proper descriptor.

### Browser (`crest_import_browser` `1579:05b7`)

A copy of BMMAIN's `file_selector` (`422b:2bbe`, `game::load_game::file_selector`) drawn in
the view (through the swapped globals), with these differences:

* Box: background saved (0xF2 x 0xB5 from view (6, 0x21)), fill (6,0x21)-(0xF6,0xD5) colour 8,
  frame to (0xF7, 0xD5) colour 1, lines y 0xA3/0xAD/0xB7 colour 0xB. No icon button.
* Eight drive buttons "A:".."H:" (all shown: DOSBox reports 26 drives) through `12a5:0087`
  (= BMMAIN `4091:123d` with font 3 and the label centred between x and x+0x3F,
  `TextButtons::register_on`), hit test `12a5:000a` (= `4091:11cc` in view coordinates).
* Start: `getdrive`/`getcwd` saved; if `DS:050C != -1`: `chdrive(DS:050C)`, `chdir(DS:20DC)`.
* Lines: fill (0xA,0xA5)-(0xF0,0xAA) and (0xA,0xAF)-(0xF0,0xB4); page number fill
  (0xD2,0xAF)-(0xF0,0xB4). Directories colour 3, files colour 1 (with the argument `"k"` a `PIC`
  directory is 0x12; with another non-empty argument `.PIC` files are 0x11; the editor passes "").
* A left click outside (0xA..0xF6, 0x21..0xD5) cancels (empty result).
* Name line fills (0x3F, 0xA5/0xAF)-(0xF0/0xCD, 0xAA/0xB4); `text_input` (package club) at
  (0x3F, 0xAA) limit 0xAF (directory, then `chdir`) and (0x3F, 0xB4) limit 0x8C (name, into the
  result buffer); colours 1 on 8. No `.MAN` suffix, no space conversion.
* End: the background is restored; `getcwd(DS:20DC)`; a non-empty result becomes
  `cwd + ("\" unless the root) + name`; `DS:050C = getdrive()`; the old drive and directory are
  restored.

The pointer can reach view y 12..253 in the scrolled view. DOSBox's absolute mouse maps the
window's 242 rows onto y 0..328 (about `y * 329 / 242`) and clamps to 87, so under DOSBox the
pointer does not follow the window cursor (the top 64 window rows all give y 87). The port's
window adds the scroll offset instead (the pointer stays under the cursor); its scripts give
driver positions, and the DOSBox side of the scenario picks window rows that land on them.

### `crest_import_dialog` (`1579:1215`)

No caller: dead code of an earlier version (it sets the view offset `DS:005A` to 0, asks
"Bitte waehlen Sie den Pfad aus, in dem / sich die Wappen befinden.", repeats the browser in
`"k"` mode until the chosen path can be opened ("FALSCHES VERZEICHNIS !") and then sets
`DS:005A = 0x59`). Not ported.

## Library differences found by this package

* TEAMED's graphics library (`16b7`) computes VRAM rows with a 16-bit `MUL` (`MOV BX,0x50; MUL
  BX` in `put_pixel` `24b2`, `xor_pixel`, `get_pixel`, `blit_rect(_transparent)`, `fill_rect(_xor)`)
  where BMMAIN's (`45ac`) uses `MOV BL,0x50; MUL BL` (`y & 0xFF`): the editor draws below row
  255 of its 329-row screen. The instruction-sequence match (1.00) hides the operand size.
  Port: `Gfx::row_mask`, set by `vga_set_mode_x_teamed` (found by packages uefa and crest at the
  same time).
* `vga_show_page` (`16b7:3231`) mode 3 is used by the import (and the dead dialog): start address
  0x1B30, `INT33/8` y range 0x57..0x148; mode 1 resets the range to 0..0xF1 (the UEFA list
  uses it too). Port: `Gfx::vga_show_page_teamed`, `Gfx::mouse_y_range`.
