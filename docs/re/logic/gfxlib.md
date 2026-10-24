# Graphics and UI library (segments 45ac, 4930, 16c7)

This document covers the drawing foundation of `BMMAIN.EXE`, which every screen uses. It has three layers:

| Layer | Ghidra segment | Language | Content |
|---|---|---|---|
| Library | `45ac` | assembly | Mode X, blits, pixels, lines, rectangles, text, software mouse pointer, timer, palette, EMS, DOS helpers |
| Wrappers | `4930` | C | rastport, screen, mouse open/close, palette arrays, bitmap blits with temporary buffers, circles, file and memory wrappers |
| UI primitives | `16c7` | C | buttons (registered slots), hit test, frames, filled rectangles, text, lines, copy-protection trap |

The Rust port is in `crates/bmp/src/gfx.rs`, and every function there carries an `Origin:` line. All addresses in this document refer to `/unpacked/BMMAIN_unpacked.EXE`. Global variables use these prefixes:

* `CS:xxxx` is library data in segment `45ac`.
* `DS:xxxx` is DGROUP `5cb3`.
* `FAR:xxxx` is far data `5238`.

None of the globals described here belongs to a save-game block (see savegame.md). They are run-time UI state only.

## 1. Video memory model

The library runs the VGA in unchained 320x240 mode ("Mode X"): 4 planes, 80 bytes per row and plane. Pixel `(x, y)` of a page that starts at offset `base` is stored in plane `x & 3` at byte `base + y*80 + x/4`.

| VRAM offset (per plane) | Use |
|---|---|
| `0x0000` | page 0 |
| `0x4B00` | page 1 (`PAGE_SIZE`) |
| `0x9600` | "page 2": cache of the office background and of the 320x240 work bitmap |
| `0xE100` | palette slots: 256 interleaved RGB triplets, one slot in each of planes 0, 1 and 2 (written by `startup_load_palette_and_init` 18bc:0004) |

The model is `Gfx::planes` (4 x 64 KiB). `Gfx::to_vga` copies the page at `crtc_start` into `video::Vga` (mode X, linear) together with the DAC.

### Bitmaps and rastports

* **Bitmap descriptor**: `{u16 w; u16 h; u8 far *data}`. If the data segment is `A000`, the bitmap lives in VRAM and its data offset is the page base. The screen descriptor is `A000:0000`, 320x240. It is created by `screen_open` 4930:0712, and the library keeps its own copy at `CS:0286`, which the pointer uses through `CS:002F`. The Rust handle is `BitmapId`, with `Storage::Mem` or `Storage::Vram(offset)`.
* **Rastport** (16 bytes): `+0` far pointer to the bitmap, `+4` fg pen, `+5` bg pen, `+6` draw mode, `+8/+0A` pen x/y, `+0C` far pointer to a font handle. `rastport_init` (4930:05a4) sets fg 0xFF, bg 0, mode 1 and pen (0,0), with no font.
* **Draw modes**: 0 means normal, i.e. solid fills, solid lines and *transparent* text. 2 means XOR: fills, lines and pixels XOR with `0x1F`. Any other non-zero value (such as the init value 1) draws solid fills and *opaque* text (glyph background = bg pen). The game only passes 0 and 2 to `rastport_set_draw_mode`; there are 28 call sites.
* **Screen struct** (`screen_open`, 0x28 bytes, `FAR:2E70`):
  * `+4` mouse y and `+6` mouse x (the copies the library keeps up to date);
  * `+8` far pointer to the 0x300-byte palette arrays (R at `+0`, G at `+0x100`, B at `+0x200`);
  * `+0C` the screen rastport (`FAR:2F28` points here; this is the main rastport);
  * `+1C` the screen bitmap descriptor.

  The block comes from `mem_alloc` without zero fill, so the game sets the draw mode explicitly.
* **Work bitmap `FAR:56FA`**: main allocates a 1x1 picture (`176b:0356(0,1,1,0,0)`, handle `FAR:57BC`) and then patches its header to 320x240. Its data is therefore never valid. Every copy *from* it goes through `blit_bitmap_or_restore_page2`, which notices `h == 240` and copies from VRAM page 2 instead. So the work bitmap is a token for "page 2".

## 2. Library state (CS variables of 45ac)

| CS: | Size | Suggested name | Meaning |
|---|---|---|---|
| 0008/000A | word | `pointer_x/y` | pointer position |
| 000C/000E | word | `pointer_off_x/y` | added to the position to get the sprite origin; 0 by default |
| 0010 | byte | `pointer_state` | 0 = drawn; 1 = drawn, lazy (remove on the first overlapping draw and keep it removed); 2 = not in VRAM. The initial value is 2 |
| 0011 | word | `pointer_hide_count` | hide nesting; `mouse_install` sets 1 |
| 0013/0017/001B/001F | far ptr | `client_pos_ptrs` | where position changes are written: mouse struct `+2` (x) and `+0` (y), screen struct `+6` (x) and `+4` (y) |
| 0023..0026 | byte | `evt_left_down`, `evt_left_up`, `evt_right_down`, `evt_right_up` | latched by the INT33 handler |
| 0027 | far ptr | `pointer_image` | pointer sprite bitmap (0 = transparent) |
| 002B | far ptr | `pointer_bg` | buffer holding the screen under the pointer (its size is the blit size for both) |
| 002F | far ptr | `pointer_screen` | screen bitmap the pointer is drawn to (`CS:0286` after the mode set) |
| 0033 | byte | `draw_page` | non-zero means every VRAM write and read adds `0x4B00` |
| 0034 | word | `ems_frame_seg` | EMS page frame segment |
| 0036 | word | `lib_busy` | 1 while any library routine runs; while it is set the INT33 handler skips the x halving and the pointer move, but still latches button events (see §5) |
| 0038/003A | dword | `timer_deadline` | |
| 003C/00FC/01BC | byte[192] | `fade_out/fade_target/fade_acc` | work arrays of `vga_set_palette_rgb4` |
| 027C | word | `pointer_redraw_flag` | per-call "redraw the pointer afterwards"; saved and restored around blits |
| 0282/0284 | word | `left_plane/right_plane` | `x & 3` of the first and last column |
| 0286 | 8 bytes | `screen_desc` | 320x240, `A000:0000` |
| 028E | word | `mouse_native_320` | 1 if the driver clamps x to 0..319 |
| 0290/0292 | word | default pointer offsets (0, 0) | |
| 0294/0296 | word | near pointers to the built-in 11x11 arrow (`CS:0298`, data `CS:02A8`) and its 11x11 background buffer (`CS:02A0`, data `CS:0321`) | |
| 029A2/29A3 | byte | `left_mask/right_mask` | map masks of the edge bytes in the fill routines |
| 039A | word[17] | CRTC table | |
| 03EB / 03FC / 03FE | | character remap / font count / font descriptor table | see font.md |

## 3. Library routines (45ac)

### Pointer awareness (shared by every VRAM routine)

Pointer hiding uses the prologue below. The pixel routines (`put_pixel`, `xor_pixel`, `get_pixel`) and `draw_text` behave differently; see their entries. The prologue is used by `blit_rect`, `blit_rect_transparent`, `fill_rect`, `fill_rect_xor` and `vga_restore_rect_from_page`, and with a slight difference by `clear_bitmap`:

```
if state != 2 && rect_hits_mouse_pointer(rect):
    restore_bg()                  # state becomes 2
    redraw = (old state != 1)     # lazy state: stays removed
... draw ...
if redraw: mouse_pointer_draw()   # state 0
```

`clear_bitmap` does not test for overlap: it always removes a drawn pointer.

| Address | Name | Exact semantics |
|---|---|---|
| 45ac:16a0 | `font_map_char` | ported in `bmp_formats::font::map_char` |
| 45ac:16d0 | `rect_hits_mouse_pointer(x,y,w,h)` | `p = pos + off`, size = sprite size. X overlap: `x < px ? px < x+w : x < px+pw`. Y overlap is the same test. Signed 16-bit |
| 45ac:1746 | `mouse_pointer_restore_bg` | state = 2; `blit_rect(bg, 0,0 -> pointer_screen, p, bg.w x bg.h)` |
| 45ac:17a0 | `mouse_pointer_draw` | state = 2; `blit_rect(pointer_screen, p -> bg, 0,0, bg size)`; `blit_rect_transparent(image -> pointer_screen, p, bg size)`; state = 0. The pointer goes to the *draw page* |
| 45ac:1850 | `mouse_pointer_move_internal(x,y)` *(function created)* | save state; restore if drawn; store pos and client copies; if it was drawn, draw it and restore the saved state |
| 45ac:18c2 | `mouse_event_handler` *(function created)* | see §5 |
| 45ac:192d | `mouse_pop_button_event` | returns and clears the first set flag among 1 = left down, 2 = left up, 3 = right down, 4 = right up; 0 if none. FUNCTION_MAP's "2 = right down" is wrong |
| 45ac:1971 | `mouse_set_position(x,y)` | clamp `x >= w -> w-1`, then `< 0 -> 0` (same for y, using the pointer screen size); store and write the client copies; INT33/04 with x*2 unless native; redraw while keeping the state |
| 45ac:1a19 | `mouse_set_pointer_shape(image, bg, off_x, off_y)` | hide if drawn, set, redraw while keeping the state |
| 45ac:1a81 | `mouse_pointer_hide(lazy)` | if state != 2: `lazy == 0` → restore now, otherwise state = `lazy`. Always `hide_count++` |
| 45ac:1ac7 | `mouse_pointer_show` | `--hide_count == 0` → draw if state == 2; state = 0 |
| 45ac:1b08 | `mouse_install(&x,&y,&x2,&y2)` | INT33/0 (returns 0 if no driver). The x-range probe sets x = 600 and reads it back; `<= 350` means native 320. Ranges: y 0..239, x 0..319 or 0..639. Initial position (160,120), offsets 0, built-in arrow, state 2, hide_count 1, events cleared, handler 45ac:18c2 with mask 0x1F |
| 45ac:1c4b | `mouse_uninstall` | restore if drawn; INT33/0 |
| 45ac:1c80 | `vga_set_mode_x_320x240` | `pointer_screen = CS:0286`; INT10/12 BL=36 (screen off), mode 13h, SEQ4 = 06, map mask F, clear `0x9600` bytes of all planes from offset 0 (pages 0 and 1; page 2 survives), misc 0xE3, CRTC table, screen on |
| 45ac:1d08 | `vga_set_text_mode` | INT10 |
| 45ac:1d1c | `blit_rect(sbm, sx, sy, dbm, dx, dy, w, h)` | clip (below), then copy linear→linear, VRAM→linear (source = VRAM if its segment is A000) or linear→VRAM |
| 45ac:20c8 | `blit_rect_transparent` | identical, but source bytes equal to 0 are skipped |
| 45ac:2486 | `put_pixel(x,y,bm,c)` | reject unless `0 <= x < w`, `0 <= y < h` (signed). Linear: `data[y*w+x]`. VRAM: plane `x&3`, offset `(y&0xFF)*80 + x/4 (+0x4B00)`; **the descriptor offset is ignored**. If the pointer is not hidden and covers the pixel, the colour is written into the background buffer at `(y-py)*bg.h + (x-px)` (16-bit `MUL`; the stride is descriptor word `+2` = the background's **height**, 45ac:2507: an original quirk, harmless for the square 11x11/1x1 shipped shapes, reproduced), and reaches VRAM only if the sprite pixel there is 0 |
| 45ac:2586 | `xor_pixel(x,y,bm)` | same as `put_pixel`, but the value is `old ^ 0x1F` (VRAM: GC function XOR, latch loaded by a read) |
| 45ac:2694 | `get_pixel(x,y,bm)` | `0xFFFF` outside. Under the pointer it returns the background buffer pixel at `(y-py)*bg.h + (x-px)` (same height stride, 45ac:2715; no mask test). The descriptor offset is ignored |
| 45ac:278b | `draw_line(x0,y0,x1,y1,c,bm)` | see "Lines" |
| 45ac:2898 | `draw_line_xor(x0,y0,x1,y1,bm)` | the same with `xor_pixel` |
| 45ac:29a4 | `fill_rect(bm, x, y, w, h, c)` | see "Fill clipping"; VRAM uses the descriptor offset plus the draw page; the edge masks give exactly the rectangle |
| 45ac:2b3e | `fill_rect_xor(bm, x, y, w, h)` | the same, XOR `0x1F` |
| 45ac:2cf0 | `clear_bitmap(bm, c)` | linear: `rep stosb` of `(w*h) & 0xFFFF` bytes. VRAM: `0x4B00` bytes of all planes at **`pointer_screen`'s** offset plus the draw page (the argument's offset is ignored); always removes a drawn pointer and redraws it unless the state was lazy |
| 45ac:2d83 | `draw_text(bm, x, y, font, fg, bg, mode, str, len)` | see "Text" |
| 45ac:2eb4 | `text_width(font, str, len)` | 16-bit sum of the advance widths |
| 45ac:2f16 | `font_find_by_name(name, handle*)` *(renamed)* | exact strcmp against the 4 table names (`normal.font`, `micro4.font`, `flow.font`, `newbold.font`). Writes `{index, height, descent}`, or index -1 |
| 45ac:2f93 / 2f9b | `video_screen_off/on` | INT10/12 BL=36 |
| 45ac:2fa3 | `vga_set_dac_color(i, r, g, b)` | INT10/1010 |
| 45ac:2fd8 | `vga_restore_palette_slot(first, n, slot, arrays*)` | read map = slot; for each channel c and each i < n: `arrays[c*0x100 + first + i] = VRAM[slot][0xE100 + 3*first + 3*i + c]`; INT10/1012 loads DAC `first..first+n` from the same VRAM bytes |
| 45ac:304a | `far_memcpy_bytes` | not ported (plain memcpy) |
| 45ac:307a | `timer_set_deadline(n)` | deadline = `0040:006C` + n (32-bit) |
| 45ac:30bc | `timer_deadline_reached` | signed compare of the high words, then unsigned of the low words: `ticks >= deadline` |
| 45ac:327d | `vga_show_page(n)` | 1: draw page 0 and show 0. 2: draw page 1 and show `0x4B00`. 0: flip (draw page 0 → draw 1 and show 0; otherwise draw 0 and show `0x4B00`). Other values: nothing. No retrace wait |
| 45ac:32fe | `vga_set_draw_page(n)` | `CS:0033 = (n-1) as byte` (n = 0 therefore selects page 1) |
| 45ac:332b | `vga_copy_page(src, dst)` | latched forward copy of `0x4B00` bytes in all planes; not pointer-aware |
| 45ac:33ca | `vga_set_palette_rgb4(words*, fade_in)` | see "Fades" |
| 45ac:34ef | `vga_wait_vretrace` | waits for 3DA bit 3 |
| 45ac:34fc | `vga_restore_rect_from_page(cw, ch, x, y, w, h, sel)` *(renamed from `fill_rect_planar`)* | fill clipping against cw x ch; pointer prologue; latched copy of the rect from page 2 (`sel == 3`, `A000:9600`) or page 1 (otherwise, `A000:4B00`) to **page 0** at the same position (the draw page is ignored) |

Not ported (other owners or no game effect): `vga_next_plane_helper` 3107, `vga_decode_to_bitmap` 3120 and `vga_encode_rle` 31d6 (VGA agent, `bmp_formats::vga`), `dos_getch` 326b, `dos_direct_console_io` 3272, `install_int_handler` 337e, `dos_delete_file` 3690, the EMS routines 36c7..3820 (the host keeps `DIGI.VOC` in memory).

### Blit clipping (both blitters, signed 16-bit)

```
if sx < 0: w += sx; dx -= sx; sx = 0; if w <= 0 return
if sy < 0: h += sy; dy -= sy; sy = 0; if h <= 0 return
if dx < 0: w += dx; sx -= dx; dx = 0; if w <= 0 return
if dy < 0: h += dy; sy -= dy; dy = 0; if h <= 0 return
if sx >= sw || sy >= sh || dx >= dw || dy >= dh: return
if dx+w > dw: w = dw-dx;  if dy+h > dh: h = dh-dy
if sx+w > sw: w = sw-sx;  if sy+h > sh: h = sh-sy
```

There is no final `w <= 0 || h <= 0` test: with in-range coordinates a caller passing `w <= 0` or `h <= 0` reaches the copy loops (`DEC DX / JNZ` runs 65536 rows for `h == 0`). The port adds that test and draws nothing (deviation D3).

The pointer prologue uses the source rectangle when the source is VRAM, and the destination rectangle otherwise. VRAM addressing: `base + draw_page*0x4B00 + (y0 & 0xFF)*80` (8-bit `MUL`), plus 80 per row, plus `x/4`. The per-plane loops (counts from the `CS:027E` table) copy exactly the rectangle; this was checked by simulating the loop for all start phases and widths 1..39. VRAM→VRAM is not supported by the routine itself, so `4930:0991` uses a temporary buffer. Linear offsets use a 16-bit `MUL` (no bitmap exceeds 64 KiB).

### Fill clipping (`fill_rect`, `fill_rect_xor`, `vga_restore_rect_from_page`)

```
if x < 0: w += x; x = 0
if x+w > W: w -= x+w-W
if w <= 0: return
(the same for y and h)
```

### Lines (45ac:278b)

```
dx = |x1-x0|, sx = x1 >= x0 ? 1 : -1 (same for dy, sy); err = 0 (u16)
plot(x0, y0)
if dy < dx:  repeat dx times: x += sx; err += dy; if err >= dx {err -= dx; y += sy}; plot
else:        repeat dy times: y += sy; err += dx; if err >= dy {err -= dy; x += sx}; plot
```

The result is `max(dx, dy) + 1` points. A single point counts as y-major. Plotting goes through `put_pixel`, so each point is clipped on its own.

### Text (45ac:2d83)

1. If the target is VRAM and the pointer state is 0, the state becomes 1 (lazy) for the duration of the call.
2. `top = y - (height - descent - 1)`; `y` is the baseline.
3. For each character, while the remaining length is not 0: stop if `x >= bitmap width` (signed). Otherwise `map_char`, then expand the glyph into the buffer `CS:0406` as `width x height` bytes: `fg` for set bits, and for clear bits `bg`, or 0 when `mode == 0`.
4. Blit the buffer: `blit_rect_transparent` if `mode == 0`, otherwise `blit_rect` (opaque). Then `x += width`. A width-0 glyph is not handled by the original: the expander does `DEC DL` from 0 (45ac:2e2d) and writes 256 bytes per row past the `CS:0406` buffer, then blits with `w = 0`. No shipped font has one; the port skips the glyph (deviation D4).
5. At the end: if the pointer was lazied and got removed (state 2), redraw it; then state = 0.

### Fades (45ac:33ca)

1. Convert 32 `0x0RGB` words to `R*4, G*4, B*4`.
2. Fade in: out = 0, acc = 0. Fade out: out = target.
3. Do 60 steps. In each step, for each of the 192 bytes (only the first 96 are meaningful): `acc += target` (signed byte); `if acc >= 60 { acc -= 60; out ±= 1 }`.
4. After every second step, wait for vertical retrace and load DAC 0..31 from `out` (30 uploads). After step k the value is `floor(k*t/60)`, and the final value is exact.

`Gfx::vga_set_palette_rgb4` returns the 30 frames.

## 4. Wrappers (4930)

| Address | Name | Semantics |
|---|---|---|
| 4930:01e7 | `delay_ticks(long n)` | `timer_set_deadline((n / 3) as u16)` (signed `_aFldiv`, truncating); busy-waits |
| 4930:0216 | `font_handle_invalidate` *(renamed)* | `*handle = -1` |
| 4930:022c | `font_open(name)` *(renamed)* | `mem_alloc(6)` + `font_find_by_name` |
| 4930:0261 | `rastport_set_font(rp, handle)` | returns -1 if `handle[0] == -1`; otherwise sets `rp+0C`, returns 0 |
| 4930:0291 | `rastport_draw_text(rp, s, len)` | `draw_text(bm, pen, font index, fg, bg, mode, s, len)`; the pen is unchanged |
| 4930:02f6 | `rastport_text_width` | `text_width(font index, s, len)` |
| 4930:031c | `rastport_move_to` | pen = (x, y) |
| 4930:033b | `rastport_line_to` | mode 2: `draw_line_xor`; otherwise `draw_line(fg)`; then pen = (x, y). In XOR mode, a frame's corners are XORed twice and cancel out |
| 4930:03ce | `rastport_put_pixel` | mode 2: `xor_pixel`; otherwise `put_pixel(fg)` |
| 4930:043a | `rastport_get_pixel` | `get_pixel` |
| 4930:0471 | `rastport_fill_rect(rp, x0, y0, x1, y1)` | inclusive: `w = x1-x0+1`, `h = y1-y0+1` (16-bit); mode 2: `fill_rect_xor`; otherwise `fill_rect(fg)` |
| 4930:04fd / 0515 / 052d | `rastport_set_color/bg/draw_mode` | bytes `+4/+5/+6` |
| 4930:0545 | `rastport_clear` | `clear_bitmap` |
| 4930:057c | `bitmap_init(bm, w, h)` | w, h, data = 0 |
| 4930:05a4 | `rastport_init` | see §1 |
| 4930:05dd | `palette_set_entry(pal, i, r, g, b)` | arrays `+0/+0x100/+0x200`, DAC |
| 4930:0624 | `palette_load_rgb4(pal, ptr, n)` | `ptr < 4` (signed 32-bit) means `vga_restore_palette_slot(0, n, ptr, arrays)`. Otherwise, for i < n: R = `(w>>6)&0x3C`, G = `(w>>2)&0x3C`, B = `(w&0xF)<<2`, stored in the arrays and the DAC |
| 4930:06d5 | `palette_get_rgb4(pal, i)` | `((R<<4 \| G)<<2) \| (B>>2)` |
| 4930:0712 | `screen_open` | see §1; also allocates the 0x300 palette arrays and calls `vga_set_mode_x_320x240` |
| 4930:077e | `mouse_open(&screen)` | 12-byte mouse struct: `+0` y, `+2` x, `+4` screen pointer, `+8` screen+0x0C (= rastport). `mouse_install(&m+2, &m+0, &screen+6, &screen+4)`. MEMORY_MAP says `FAR:6DC0 [0] = x`, but the install code writes x to `+2` |
| 4930:0819 / 0826 | `screen_close` / `mouse_close` | text mode / `mouse_uninstall` |
| 4930:0849 | `blit_bitmap_or_restore_page2(sbm, sx, sy, dbm, dx, dy, w, h)` *(renamed from `blit_bitmap_scaled_or_copy`)* | If `sbm` is not `A000:0000` and `sbm.h == 240`: `vga_restore_rect_from_page(320, 240, sx, sy, w, h, 3)`, ignoring `dbm/dx/dy`. Otherwise it behaves like `blit_bitmap`, but `dbm` is a bitmap |
| 4930:0991 | `blit_bitmap(sbm, sx, sy, drp, dx, dy, w, h)` | `drp` is a rastport (or a picture handle whose first field is the bitmap pointer). If both are `A000:0000`, it copies through a `w*h` temporary buffer (`mem_alloc` without zero fill): screen→temp, then temp→screen. Otherwise `blit_rect` |
| 4930:0aa5 | `blit_bitmap_transparent(sbm, sx, sy, dbm, dx, dy, w, h)` | the same as `blit_bitmap`, with the second (or only) stage transparent; `dbm` is a bitmap. `ui_blit_transparent` 176b:02e4 passes `*rp` |
| 4930:0bad | `draw_circle_octants(rp, a, b, cx, cy)` | plots `(cx+a,cy+b) (cx+a,cy-b) (cx-a,cy+b) (cx-a,cy-b) (cx+b,cy+a) (cx+b,cy-a) (cx-b,cy+a) (cx-b,cy-a)` in this order with `rastport_put_pixel` |
| 4930:0c90 | `draw_circle(rp, cx, cy, r)` | `x=0, y=r, d=3-2r`; while `y > x`: octants; `d<0 ? d += 4x+6 : (d += 4(x-y)+10, y--)`; `x++`. Then octants once more if `y == x` |

Not ported: `file_*` (000c..00d6) and `mem_alloc`/`mem_free` (010c/0188); the host uses Rust I/O and ownership. The extra word `0xC0` that some callers push after the size is a leftover Amiga minterm argument and is ignored.

## 5. Mouse event handler (45ac:18c2)

```
if CS:0036 != 1:                              # JZ 45ac:18f6 when busy
    x = native_320 ? CX : CX >> 1
    if AX & 1 and (x, DX) != (pos_x, pos_y): mouse_pointer_move_internal(x, DX)
# the button part always runs, also while busy
if   AX & 2:  left_down  = 1
elif AX & 4:  left_up    = 1
elif AX & 8:  right_down = 1
elif AX & 16: right_up   = 1
```

The game reads the position from the client copies (screen struct `+6/+4`) and the events through `mouse_pop_button_event` (via `input_poll_mouse` in 176b, which stores the result in `FAR:2EA2`).

## 6. UI primitives (16c7)

The globals are the button slots `FAR:539E + 12*i` (20 slots), the frame pictures `FAR:548E/5492` (pictures 0x18/0x19, 50x41), the icon sheet `FAR:2F3C`, the alternative icon `FAR:57C0`, the work bitmap `FAR:56FA`, the screen bitmap `FAR:A6CA` and the main rastport `FAR:2F28`.

| Address | Name | Semantics |
|---|---|---|
| 16c7:0002 | `ui_hit_test_buttons(rects*, n, click_only)` | Reads the mouse (x = screen `+6`, y = `+4`) *before* the optional poll. If `click_only` is set, it calls `input_poll_mouse` and requires `FAR:2EA2 == 1`. It returns the 1-based index of the first `[x,y,w,h]` with `x < mx < x+w` and `y < my < y+h` (strict), otherwise 0. The counter is a signed byte stepping by 4 below `n*4`, so n > 31 never terminates without a hit (the model returns 0) |
| 16c7:00a7 | `ui_draw_button(rp, x, y, w, h, rect_out*, save_bg, color)` → slot | See below |
| 16c7:0563 | `ui_redraw_buttons(index, w_filter)` | See below |
| 16c7:0701 | `ui_draw_filled_frame(rp, x0, y0, x1, y1, frame, fill)` | `fill_rect(fill)` then `ui_draw_frame(frame)` |
| 16c7:0753 | `ui_draw_frame(rp, x0, y0, x1, y1, c)` | `move_to(x0,y0)`, `line_to (x1,y0) (x1,y1) (x0,y1) (x0,y0)` |
| 16c7:07d5 | `ui_draw_text_at(rp, s, x, y, c)` | `c == 0` → 0xFF; set colour, `move_to`, `rastport_draw_text(strlen)` |
| 16c7:083c | `ui_fill_rect(rp, x0, y0, x1, y1, c)` | set colour, inclusive `rastport_fill_rect` |
| 16c7:087c | `ui_draw_text_centered_shadow(rp, s, x0, x1, y, c, shadow)` | `x = ((x1-x0)/2 - tw/2) + x0 + 1` (C division, toward 0). If `shadow != 0xFF`: shadow colour (0 → 0xFF) at `(x, y)`. Text colour (0 → 0xFF) at `(x-1, y)`, but if `x+tw > 319` or `x < 0` it uses `x = 1`, i.e. the text is drawn at column 0 (the shadow is not moved) |
| 16c7:09b0 | `ui_draw_line(rp, x0, y0, x1, y1, c)` | **y0 and y1 are bytes** (`MOV AL; SUB AH,AH`) |
| 16c7:09f9 | `copy_protection_penalty` | if `DS:05AC == 0`: `random_range(0,100) == 0` → endless loop of `screen_fade(FAR:2E70, random_range(0,1))` |

### ui_draw_button (16c7:00a7)

1. **Slot search**: `i = 0; while slot[i].w != 0 && i < 20: i++`. With 20 used slots, i = 20; that slot lies at `FAR:548E` and overwrites the frame pointers. The Rust port has a 21st slot instead.
2. `slot[i].pixel = get_pixel(rp, x+2, y+2)` (low byte; 0xFF outside the screen).
3. **`w != 0`, panel**:
   * Store `slot = {x, w+2, (y-2) as byte, (h+2) as byte}` and write the same values to `rect_out`.
   * If `save_bg`: picture handle `176b:0356(0, w+5, h+5, …)`; `blit_bitmap(FAR:A6CA, x, y-2 → handle, 0,0, w+4, h+5)`; the handle goes into `slot.saved`.
   * Colour 0: `fill (x+2, y+h-1)-(x+w+2, y+h+2)` and `fill (x+w, y)-(x+w+2, y+h-2)` (shadows).
   * Colour `color`: `fill (x, y)-(x+w, y+h)`.
   * Lines with byte y: top `(x,y)-(x+w,y)` colour 2, right `(x+w,y)-(x+w,y+h)` colour 7, left `(x,y)-(x,y+h)` colour 1, bottom `(x,y+h)-(x+w,y+h)` colour 6.
4. **`w == 0`, icon button** (`rect_out = {x, y, 50, 41}`, `slot.w = 0`, not registered). Flags in `h`:
   * `0x2000`: `blit_bitmap_transparent(frame[(h>>15)&1], 0,0 → rp, x, y, 50, 41)`, then return.
   * `0x4000` (without 0x2000): no frame. Otherwise `blit_bitmap(frame[(h>>15)&1] …)` (opaque).
   * `code = h & 0xFF`; `0x22/0x23 → +1`; then `100 → 0x22`; `0x96 → 0x26`. The icon source is `((code % 6) * 33, (code / 6) * 24)`, size 32x23, drawn at `(x+9, y+9)`.
   * Original code `0x29`: if `h & 0x1000`, `ui_fill_rect(x+9, y+9, x+40, y+31, 8)`. Then the icon is drawn *transparently* from `FAR:2F3C` (source from code 0x29: (165, 144)).
   * Original code `0xC8`: load picture 37 and `blit_bitmap(pic, 0, 31 → x+9, y+9, 32x21)`, then free it.
   * Remapped code `0x23` (original 0x22 or 100): `blit_bitmap(FAR:57C0, 0, 0, …, 32x23)`.
   * Otherwise `blit_bitmap(FAR:2F3C, icon source, …, 32x23)` (opaque).
5. Return `i`.

### ui_redraw_buttons (16c7:0563)

* Range `index..=index`, or `0..=19` when `index == 0x7F` (signed-byte loop).
* A slot is processed when `slot.w != 0` and (`slot.w == w_filter` or `w_filter == 0`).
* No saved background: `ui_fill_rect(FAR:2F28, x, y, x+w, y+h, 0)`. Then, if `slot.pixel != 0`, `blit_bitmap_or_restore_page2(FAR:56FA, x, y → FAR:A6CA, x, y, w+2, h+4)`, which restores from VRAM page 2.
* Saved background: `blit_bitmap(saved, 0,0 → FAR:2F28, x, y, w+1, h+3)`, then `bitmap_free`.
* Finally `slot.w = 0` and `slot.saved = NULL`.

Here `w` and `h` are the stored values (`w+2`, `h+2`).

## 7. Suggested global names

| Global | Suggested name |
|---|---|
| `FAR:2E70` | `g_screen` (screen struct; `+6` mouse x, `+4` mouse y) |
| `FAR:2F28` | `g_main_rastport` |
| `FAR:2F3C` | `g_icon_sheet` (MEMORY_MAP calls it "mouse pointer bitmap"; in `ui_draw_button` it is the icon sheet) |
| `FAR:539E + 12*i` | `g_button_slots[20]` (MEMORY_MAP gives `53A0`, which is the `w` field) |
| `FAR:548E/5492` | `g_button_frames[2]` |
| `FAR:56FA` | `g_work_bitmap` (page-2 token) |
| `FAR:57BC` | `g_work_bitmap_handle` |
| `FAR:57C0` | `g_icon_alt_bitmap` |
| `FAR:6DC0` | `g_mouse` (`+0` y, `+2` x) |
| `FAR:A6CA` | `g_screen_bitmap` (`*FAR:2F28`) |
| `DS:05AC` | `g_protection_passed` |

## 7a. Deviations of the port

| Id | Original | Port | Reason |
|---|---|---|---|
| D1 | `screen_open` gets the 0x28-byte rastport from `mem_alloc` without zero fill: initial draw mode and pens are undefined | mode 0, pens 0 | the game sets the mode before drawing text |
| D2 | `ui_draw_button` with all 20 slots used returns slot 20 and writes over `FAR:548E` (frame picture pointers) | a harmless 21st slot | memory corruption is not modelled |
| D3 | `blit_rect` / `blit_rect_transparent` (45ac:1d1c) have no final `w/h <= 0` test | returns without drawing | avoids a 65536-row loop; no known caller passes `w` or `h <= 0` |
| D4 | `draw_text` with a width-0 glyph overruns `CS:0406` | glyph skipped | no shipped font has width-0 glyphs |
| D5 | `ui_hit_test_buttons` with `n > 31` and no hit never terminates (signed-byte counter) | returns 0 | no known caller passes more than 31 rects |
| D6 | The INT33 handler is asynchronous and sees `CS:0036` set during library calls (it then skips the pointer move but still latches buttons) | the host calls `mouse_event_handler` between library calls, `busy` stays false | timing only; the busy semantics are implemented and tested |

## 8. Ghidra changes made

* Renamed `FUN_45ac_2f16` → `font_find_by_name`, `FUN_4930_0216` → `font_handle_invalidate`, `FUN_4930_022c` → `font_open`, `blit_bitmap_scaled_or_copy` → `blit_bitmap_or_restore_page2`, `fill_rect_planar` → `vga_restore_rect_from_page`.
* Created functions `45ac:1850 mouse_pointer_move_internal` and `45ac:18c2 mouse_event_handler`.
* Added plate comments at 45ac:18c2 (corrected: busy still latches buttons), 45ac:2486 (height stride), 45ac:34fc, 4930:0849 and 16c7:00a7.

## 9. Verification

`cargo test -p bmp gfx` covers:

* Mode X addressing and pages;
* exact fill edges and XOR;
* inclusive rastport fills;
* blit clipping, transparency, and screen↔memory copies;
* Bresenham point sets, and XOR corner cancellation;
* page flips, page copies and the page-2 restore;
* pointer save/restore, drawing under the pointer, events and the lazy hide;
* the busy handler still latching button events, and the height stride of the pointer buffers;
* strict hit testing;
* panel buttons, their registration and restore;
* centered text with shadow, and opaque text;
* circles;
* palette conversion, fades and slots;
* the timer compare;
* the copy-protection RNG call count.

With `BMP_DATA` set, two more tests run: text drawn through the library is compared pixel by pixel against `bmp_formats::font`, and an icon button draws the real `24.VGA` frame picture.
