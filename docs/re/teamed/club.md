# TEAMED.EXE – package "club": selecting a club and editing its data

Port: `crates/bmp/src/teamed/club.rs`, the bodies of `dialog::message` and
`dialog::text_input`, and `crest::draw_editor` (needed for the selected screen).
Scenario: `tests/visual/teamed_club.{port,dosbox}.txt` (18 frames, all >= 99.9 %, and
`MANA.DAT` identical to the original's outside the strength block, which package uefa
recomputes). Addresses as in `overview.md`; `DS:` below is DGROUP `2256` for the small
variables (`0336`, `0338..0340`, strings) and `1cb5` for the buffers.

## Data

| buffer | original | copy (edit buffer) | port |
|---|---|---|---|
| club names (200 x 23) | `DS:0042` | `DS:21CA` | `Editor::mana` / `Editor::copy` `.clubs[i].name` |
| points (64 x 2) | `DS:467C` | `DS:57E4` | `.points` |
| goals (64 x 2) | `DS:364E` | `DS:2058` | `.goals` |
| ranks (64) | `DS:1E04` | `DS:123E` | `.rank` |
| player names (64 x 20 x 26) | `[5A02]` | `[213E]` | `.clubs[i].players` |
| crest numbers (64) | `DS:5956` | – | `mana.crest` |
| crest pictures | `DS:5698` (+ bitmap cache `DS:1E48`) | – | `Editor::crests` |
| dead block (0xF56 bytes) | `DS:4742` | `DS:371E` | not modelled |

* `DS:4742..5697` is BSS that is copied to `DS:371E` at start-up (`1000:1941`) and back by
  "AENDERUNG SICHERN" (`12a5:13fe`). Nothing else references either range (searched every
  operand of the program), so it is a dead leftover, not part of `MANA.DAT`.
* `DS:0336` selected club (0xFF none), `DS:0338` first club per league group `0,18,38,58`,
  `DS:033C` highest rank per group `18,20,20,20`, `DS:0340` debug switch, **initially 1**.
* `DS:5996[i]` is set to 1 for both clubs of a league swap and never read.
* `[BP-DA]` "crest changed" (set by the crest editor, `Editor::crest_changed`);
  `[BP-AA]` the 0x52-byte input buffer every input reuses (`Editor::input_buf`).

The panel and the player list edit the **copies**; "AENDERUNG SICHERN" copies all of
them back. Selecting a club refreshes only its player names in the copy
(`[5A02] -> [213E]`, 0x208 bytes).

## Main loop order (pointer `x`, screen `y`, `y_rel = y - 75`, event `ev`)

1. `12a5:04eb` debug switch: `x == 0 && y == 0 && ev == 1`: `DS:0340 ^= 1`, then
   `ui_fill_rect(screen, 0x13D, 0, 0x13F, 2, 3 * (flag + 5))` (15 off, 18 on).
   (The capture never puts the DOSBox pointer at exactly (0, 0), so the scenario does not
   cover it.)
2. `12a5:0521` scroll-view switch (`12a5:2d39`, empty).
3. `12a5:056b` panel editing (below). The league swap overwrites the pointer copies
   `[BP-E2]`/`[BP-EA]` that the hover and bar checks read next.
4. Hover / panel clearing / bar (ported by the skeleton). A hover change jumps to the end
   of the loop, so a click in the same poll as the change is lost (the scripts move first).
5. `12a5:119f` `ev == 1 && hovered != -1 && selected == FF`: select (below).
6. `12a5:12f6` "PROGRAMM-ENDE" (`x 0x2B..0x6F, y 0x47..0x54`, nothing selected).
7. `12a5:134c` club buttons (`x 0x2B..0x6F, y 0x2E..0x54`, selected).
8. `12a5:1671` crest editor (package crest), `12a5:2028` player names.

## Select (`12a5:11b7`)

```
screen off
blit crest[h] 38x38 -> screen (2, 0x2E) and (0x73, 0x2E); -> scratch [BP-C8], [BP-58]
DS:0336 = h
far_memcpy([5A02] + h*0x208 -> [213E] + h*0x208, 0x208)
crest_editor_draw(crest[h], 0)        ; 1000:1d73, screen on at its end
```

`crest_editor_draw(pic, from_copy)` (`1000:1d73`): screen off, `grid_clear`, font 2,
`fill(view 0x9E,0xC,0x13F,0xA6, 6)`, `frame(0x9D,0xC,0x13F,0xA6, 0xB)`; with a club
selected the player list from `[5A02]` (or `[213E]`): rows `i = 0..9`, fills
`(0xA3,(i+2)*7)-(0xEA,7i+0x12)` and `(0xF6,..)-(0x13D,..)` colour 0xD, names at
`(0xA3|0xF6, 7i+0x12)` colour 0xB; `PIC\61.VGA` 0xA8x0x9B at view (0x9D, 0xC); the crest
enlarged: `fill(4x+1, 4y+0xD, 4x+5, 4y+0x11)` in `get_pixel(pic, x, y)` for x, y < 38;
`frame(0,0xC,0x99,0xA5, 0xB)`, `fill(0x9E,0x7E,0xC5,0xA5, 0)`, the crest at (0x9F, 0x7F);
screen on.

## Panel editing (`12a5:056b..07a1`)

Condition `x 0x35..0x97, y 3..0x27, selected, ev == 1`; `row = (y - 3) / 8`,
`group = (s>17)+(s>37)+(s>57)`.

```
if row != 1:
    fill(screen, 0x33, row*8+3, 0x95, (row+1)*8, 0xC)
    v = text_input(screen, buf, 0x35, row*8+7, limit 0x4D, fg 0xB, bg 0xC, text = row != 2)
    if buf[0] == 0: row = 9                     ; nothing
switch row:
 0: if strcmp(buf, any of the 200 names DS:0042) == 0 && DS:0340:  ; includes itself
        dialog_message("DIESER TEAMNAME", "EXISTIERT BEREITS.")
    else strncpy(copy name[s], buf, 0x17)
 1: league swap prompt (below)
 2: if v < 0 || (v == 0 && group <= 1) || v > DS:033C[group]:
        dialog_message("DIESER TABELLENPLATZ", "EXISTIERT NICHT.")
    else:
        if group < 2: first j in DS:0338[g] .. +DS:033C[g] with copy rank[j] == v:
                      copy rank[j] = copy rank[s]
        copy rank[s] = v
 3: if parse_pair(buf, &a, &b) && (a + b) % 2 == 0: copy points[s] = (a, b)
    else dialog_message("FALSCHE", "EINGABE")
 4: if parse_pair(buf, &a, &b): copy goals[s] = (a, b) else dialog_message("FALSCHE", "EINGABE")
12a5:0790: if selected != FF: panel_draw_selected(s)     ; 1000:26f1
```

`panel_draw_selected` (`1000:26f1`) is `panel_draw_hovered` reading the copies
(`teamed::draw_panel`).

### League swap prompt (`12a5:0869..0a8f`)

```
target = -1; screen off; grid_draw_page(page)
fill(screen, 0x35, 0xB, 0x96, 0xF, 0xC); text(screen, "MIT WEM TAUSCHEN ?", 0x35, 0xF, 0x1D)
screen on
loop:
    x, y_rel = pointer            ; read before the poll
    ev = input_poll_mouse()
    if ev == 1 && (x == 0 || x == 0x13F): screen off; page ^= 1; grid_draw_page; screen on
    if x < 0x137 && 0xB < y_rel < 0xA8:
        c = x/39 + (y_rel-0xC)/39*8 + page*32
        if c != target: target = c; panel_draw_hovered(c); continue
    elif target != -1: fill(screen, 0x33,3,0x95,0x29, 0xC); target = -1
    if ev == 1 && target != -1: break
if target != s: swap (12a5:0aba): mem_swap of names 0042 (23), points 467C (2), goals
    364E (2), ranks 1E04 (1), players [5A02] (0x208), names 21CA, points 57E4, goals 2058,
    ranks 123E, crest numbers 5956 (1), players [213E]; crest pictures DS:5698 and the
    bitmap cache DS:1E48; DS:5996[s] = DS:5996[target] = 1. Strengths are NOT swapped.
deselect (12a5:0d4a): screen off; grid_draw_page; DS:0336 = FF; [BP-DA] = 0;
    fill(2,0x2E,0x27,0x53, 0x1D); fill(0x73,0x2E,0x98,0x53, 0x1D);
    rows i=0..9: fill(0xA3,(i+2)*7,0xEA,7i+0x13, 0xD), fill(0xF6,..,0x13D,.., 0xD); screen on
```

Afterwards the main loop continues with the prompt's last pointer position: the hover
shows the target slot (now holding the selected club's data). Clicking the selected
club itself deselects without swapping, and the main loop's select step then selects it
again (`hovered` is still that club and the click is still pending).

## Club buttons (`12a5:1361..1671`)

`row = (y - 0x2E) / 0xD`:

* row 1 "AENDERUNG SICHERN": `far_memcpy` copy -> original for names (0x11F8, all 200),
  `371E -> 4742` (dead), points (0x80), goals (0x80), ranks (0x40), players (0x8200, all 64
  clubs). If `[BP-DA]`: `crest_write_vga([BP-58], "pic\" + crest[s] + ".vga")` and the
  scratch is blitted into `crest[s]` (package crest, `crest::save_crest`).
* every row (0 "NICHT UEBERNEHMEN", 1, and 2 = the "PROGRAMM-ENDE" button): deselect
  (`12a5:1544`, as above).

Quirk kept: "NICHT UEBERNEHMEN" does not restore the copies; the discarded panel edits
stay there and the next "AENDERUNG SICHERN" of *any* club writes them (scenario frame
c18). The player names are safe because selecting a club refreshes its copy, but the
start-up fix-up of `| { }` (`12a5:02ca`) only touched `[5A02]`, so a commit writes the
unconverted names of every club from the copy back.

## Player names (`12a5:2028..22ab`)

Condition `ev == 1, selected, x > 0xA1, 0xD < y < 0x53` (no upper x bound):

```
font 3; row = (y - 0xE) / 7; col = x > 0xEF; slot = col*10 + row; x0 = col*0x53 + 0xA3
fill(screen, x0, (row+2)*7, col*0x53+0xEA, row*7+0x13, 0xD)
text_input(screen, buf, x0, row*7+0x12, limit 0x34, fg 0xB, bg 0xD, text)
if buf[0]:
    if DS:0340 && strcmp(buf, any of the 1280 copy slots [213E]) == 0:
        dialog_message("DIESER NAME", "EXISTIERT BEREITS.")
    else strncpy(copy players[s][slot], buf, 0x1A)
fill(same rect); text(screen, copy players[s][slot], x0, row*7+0x12, 0xB)
```

## Helpers

* `text_input` (`1000:0a7d..0d8c`, Ghidra parts `0ae9`, `0c0e`):
  `(holder, buf, x, y, limit, fg, bg, text)`; `holder+8` is the rastport (`DS:57B2` -> the
  screen rastport). `fg 0 -> 0x20`; `set_bg_color(bg)`; `buf[0] = 0`. Loop: cursor
  `fill(x+2, y-4, x+4, y)` in fg; stop on Enter; `getch`; erase cursor in bg. Backspace
  with text: `len--`, `x -= w(c)`, `fill(x, y+descent-height+1, x+w(c), y+descent)` in bg.
  Else `c = key & 0xDF` if that is `A..Z`, else key; `ok = len < limit-1000` when
  `limit > 1000`, else `w(c) + text_width(buf, len+1) <= limit` (one stale byte measured);
  text mode accepts every key but Enter if ok, digit mode only `'0'..'9'` (key
  sign-extended) if ok; accepted: `move_to(x,y)`, draw c in fg, `buf[len++] = c`,
  `x += w(c)`. On Enter: `buf[len] = 0`, erase the cursor; digit mode returns the decimal
  value (long). DEVIATION in the port: Backspace on an empty text would append a 0x08 in
  text mode; ignored.
* `dialog_message` (`1000:1a31`): backup of the screen (0,0) 0x78x0x32 into a blank
  picture (never freed); `fill(1,2,0x77,0x28, 0x10)`, `frame(.., 0x13)`, font 2 "Fehler:"
  centred (1..0x77, y 0xC, colour 0x13, shadow 0xFF), font 3 line 1 at y 0x18, line 2 at
  y 0x20; `delay_ticks(0x5A)`; restore.
* `parse_pair` (`1000:207d`): from the end of the string, digits until ':' -> `b`, then
  digits before it -> `a`; any other character -> 0; `a >= 255 || b >= 255` -> 0. Without
  a colon the second scan runs into the bytes before the buffer (a far pointer on the
  stack): practically invalid; the port returns invalid.
* `mem_swap` (`1000:012b`): `(a, b, len)` through a temporary allocation (`far_memcpy` is
  `(src, dst, len)`).

## Coordinates summary

| element | screen area |
|---|---|
| panel rows (input) | x 0x35..0x97, row `r` y `8r+3 .. 8r+10` |
| boxes | (2,0x2E)-(0x27,0x53), (0x73,0x2E)-(0x98,0x53) |
| NICHT UEBERNEHMEN / AENDERUNG SICHERN / PROGRAMM-ENDE | x 0x2B..0x6F, y 0x2E..0x3A / 0x3B..0x47 / 0x48..0x54 |
| player rows | left x 0xA2..0xEF, right x 0xF0.., row `r` y `0xE+7r .. 0x14+7r` |
| message box | (1,2)-(0x77,0x28) |
| debug square | (0x13D,0)-(0x13F,2) |
