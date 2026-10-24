# TEAMED.EXE – package `uefa`: the UEFA strength list and the save

Status: ported (`crates/bmp/src/teamed/uefa.rs`, `teamed/save.rs`, `crates/bmp-game/src/strength.rs`; `text_input` in
`teamed/dialog.rs` is shared with package club), scenario `tests/visual/teamed_uefa.*` (11 frames at 100 %, `MANA.DAT`
identical to the file the original writes). Addresses are Ghidra addresses in
`/unpacked/TEAMED_unpacked.EXE` (see `overview.md`); `DS` is DGROUP `2256`.

## `uefa_screen` (`12a5:233d`)

Entered from the main loop by a click on the lower ("UEFA") part of the bar
(`12a5:1157`). All drawing goes through the crest view rastport (`DS:364A`, origin
y 75); "view y" below is screen row - 75.

```
cur = -1                                   ; [BP-2], club under the pointer
vga_show_page(3)                           ; CRTC start 0x1B30: rows 87..328 visible,
                                           ; mouse y range 87..328 (16b7:3231)
page:                                      ; 12a5:2359
  screen_set_visible(0); set_draw_mode(grid, 0); grid_clear()
  blit bar (DS:0116) to (0x138, 0xC) 8x0x9B
  ui_set_font(grid, 3)
  for (x, s) in (0x61 "A", 0x74 "M", 0x89 "S", 0x101 "A", 0x114 "M", 0x129 "S"):
      ui_draw_text_at(grid, s, x, 0x10, 7); ui_draw_text_at(grid, s, x + 1, 0x10, 1)
  x = 5, y = 0x16, first = DS:04CA * 0x44
  for i in first .. first + 0x44:
      club = i + 0x40
      ui_draw_text_at(name[club], x, y, 1)
      ui_draw_text_at(fmt(strength[club][0..3]), x + 0x5A / 0x6E / 0x82, y, 2)
      y += 7; if i - first == 33: x = 0xA5, y = 0x16
  screen_set_visible(1)
loop:
  set_draw_mode(grid, 2)                   ; XOR for the highlight
  repeat:
      ev = input_poll_mouse(); (x, y) = pointer; y -= 75
      if ev == 1 and x > 0x136 and y < 0xA8:
          if y < 0x5A: set_draw_mode(0); vga_show_page(1); return      ; "LIGA"
          DS:04CA ^= 1; goto page                                    ; "UEFA"
      if x <= 5 or x >= 0x113 or y <= 0x11 or y >= 0xFE:
          if cur != -1: xor_row(c, r); cur = -1
      else:
          c = x > 0xA5; r = (y - 0x11) / 7
          club = (c + DS:04CA * 2) * 0x22 + r + 0x40
          if club != cur: (cur != -1: xor_row(old)); xor_row(c, r); cur = club
  until ev == 1 and cur != -1
  xor_row(c, r); set_draw_mode(grid, 0)
  edit (below); cur = -1; goto loop
```

`xor_row(c, r)` = `ui_fill_rect(0xA0c + 5, 7r + 0x11, 0xA0c + 0x96, 7r + 0x16, 1)` in XOR
mode. Note: a page change keeps `cur`, so a highlight that was on screen before the
click is XORed again on the new page when the pointer next leaves the row area
(original behaviour, kept).

### Editing a row (`12a5:28c4`)

`x0 = 0xA0c + 5`, `y0 = 7r + 0x11`, input buffer `[BP-0x50]` (50 bytes, kept between
inputs).

| step | call | data |
|---|---|---|
| name | clear `(x0, y0)-(x0+0x59, y0+5)`, `text_input(.., x0, y0+5, max 0x4D (77 px), cursor 0xB, bg 0, text)`; non-empty: `strncpy(name[club], buf, 0x17)` (zero-padded) | `DS:0042 + 23 * club` |
| | clear `(x0, y0)-(x0+0x6D, y0+5)`, name at `(x0, y0+5)` colour 1 | |
| A | `text_input(.., x0+0x5A, y0+5, 0x3EA (2 chars), 0xB, 0, digits)`; `old = strength[club][0]`; non-empty: store, `uefa_strength_ok()` else restore `old` and `dialog_message_uefa("EINGABE NICHT ANGENOMMEN,", "UEFA-CUP SONST ZU LEICHT.")`; clear `(x0+0x5A, y0)-(x0+0x81, y0+5)`, value colour 2 | `DS:33CA + 3 * club` |
| M | same at `x0+0x6E`, clear to `x0+0x91`; a refused value is replaced by **`old` (the A value)** | `+1` |
| S | same at `x0+0x82`, clear to `x0+0x91`; refused -> `old` (A) | `+2` |

The restore of M and S with the old A value is an original bug; it is kept because it
changes the written `MANA.DAT` (the scenario checks it: club 67 M 95 -> 93).

### `uefa_strength_ok` (`12a5:2ce0`, Ghidra name was `uefa_club_ptr`)

Long sum of the signed bytes `DS:33CA + 0xC0 .. + 0x258` (clubs 64..199), divided by
`0x198` (`crt_aldiv`); returns 1 if the quotient's high word is non-zero (negative) or
its low word is `>= 0x4F` (79), else 0. With the shipped data the average is 80
(sum 32990, limit 32232).

### `dialog_message_uefa` (`1000:1bd2`)

Saves the view area `(0,0)` 0x78x0x32 in a blank picture, `ui_fill_rect(1, 2, 0x77,
0x28, 0x10)`, `ui_draw_frame(.., 0x13)`, font 2 "Fehler:" (`DS:02DF`) centred at y 0xC,
font 3 the two lines at y 0x18 and 0x20 (colour 0x13, no shadow), `delay_ticks(0x78)`,
restores the area. The backup picture is never freed. In the scrolled UEFA screen the
box's top 10 rows lie above the visible part.

## `text_input` (`1000:0a7d..0d8c`)

`text_input(win, buf, x, y, max, cursor, bg, text)`; `win+8` is the crest view rastport.

```
len = 0; if cursor == 0: cursor = 0x20; range = text ? ' '..'z' : '0'..'9'
set_bg_color(rp, bg); buf[0] = 0; key = 0
loop:
  set_color(cursor); fill(x+2, y-4, x+4, y)                   ; 1000:0ccc
  if key == 13: break
  key = getch(); set_color(bg); fill(x+2, y-4, x+4, y)
  if key == 8 and len: len--; ch = buf[len]; buf[len] = 0; w = width(ch); x -= w
      fill(x, y + 1 + descent - height, x + w, y + descent); continue
  ch = key & 0xDF in 'A'..'Z' ? key & 0xDF : key; ok = 1
  if max > 1000: ok = max - 1000 > len
  else: ok = width(ch) + width(buf, len + 1) <= max        ; one stale byte counted
  accept = text ? key != 13 and ok : key in range and ok
  if accept: move_to(x, y); set_color(cursor); draw(ch); buf[len++] = ch; x += width(ch)
buf[len] = 0; set_color(bg); fill(x+2, y-4, x+4, y)
return text ? AX (unused) : sum of digits as a long (value = value * 10 + d)
```

The port (`dialog::text_input`, package club) ignores a Backspace on an empty text
(documented DEVIATION there); the UEFA inputs reuse one buffer like the original's
`[BP-0x50]`.

## Strength recomputation (`teamed_save_mana` `1579:006d..04b5`)

Runs after "MACH MAL !" for clubs 0..63, group `g = (i > 37) + (i > 17)`; constants
`base[g]` at `DS:0554` = 70, 34, 20 and `size[g]` at `DS:033C` = 18, 20, 20. All
arithmetic is 16-bit signed (C division truncates).

```
for i in 0..64: pts[g] += points[i][0]; gf[g] += goals[i][0]; ga[g] += goals[i][1]
                if rank[i]: n[g]++
for g: each of pts, gf, ga: if n[g]: x /= n[g]; if x == 0: x = 1
for i in 0..64:
  if rank[i] == 0 or g == 2:                       ; seven random_range calls
      s = rr(0, 25)
      a = rr(0, 5) + (base - rr(0, 5)) + s          ; byte 0
      b = rr(0, 5) + (base - rr(0, 5)) + s          ; byte 1
      c = rr(0, 5) + (base - rr(0, 5)) + s          ; byte 2
  else:
      t = points both != 0 ? points[i][0] * -100 / pts[g] + 100 : 0
      f = size[g] - rank[i] - t / 20
      d = goals both != 0 ? goals[i][1] * -100 / ga[g] + 100 : 0
      a = base + d / 20 + f;  a += a / 20
      e = goals both != 0 ? afldiv(goals[i][0] * -100, gf[g]) + 100 : 0
      c = base - e / 20 + f;  c += c / 20
      b = base + f;           b += b / 20
  if g == 1: a, b, c = afldiv(v * 125, 100); clamp 38..74
  clamp a, c, b to 20..95; strength[i] = (a, b, c)
```

`rand()` is never seeded: the generator starts in the MSC state 1 at every program start
(`Editor::rng`). The shipped `MANA.DAT` was written this way: recomputing it from its
own points, goals and ranks reproduces every strength byte of the ranked league clubs
(unit test `shipped_strengths_reproduce`), and a first save from a fresh start even
reproduces the random bytes. Then all blocks are written (`docs/re/mana.md`).

## Video and mouse in the scrolled screen

* `vga_show_page` (`16b7:3231`): mode 1 = page 0, CRTC start 0, `INT 33h` function 8
  y range 0..0xF1; mode 3 = page 0, CRTC start 0x1B30, y range 0x57..0x148. Port:
  `Gfx::vga_show_page_teamed`, `Gfx::mouse_y_range`. The driver clamps the position
  without calling the event handler. The port's window maps a window row to
  `row + range start` (the real, relative mouse); scripts give driver coordinates
  (clamped). DOSBox Staging's seamless mouse maps window row `r` to about
  `(r + 0.5) * ymax / 242` instead, which the DOSBox scenario script accounts for.
* TEAMED's graphics library computes VRAM row addresses with `MUL BX` (`16b7:1f36`,
  `205c`, `22f2`, `241c`, `2561`, `2668`, `275f`, `2b04`, `2ca8`, `3602`), BMMAIN's with
  `MUL BL` (rows wrap at 256). The UEFA list draws down to screen row 328, so the port
  has `Gfx::row_mask` (0xFF for BMMAIN, 0xFFFF after `vga_set_mode_x_teamed`).
