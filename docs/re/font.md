# Fonts and text rendering (BMMAIN.EXE)

Status: reconstructed from code and checked against the real executable.
Implementation: `crates/bmp-formats/src/font.rs`.

## Summary

- The game does **not** use the VGA BIOS font (no INT 10h AX=1130h anywhere). DOSBox
  therefore provides nothing font-related; all glyphs come from the executable.
- Three proportional 1-bit fonts, `micro4.font` (6 px), `flow.font` (8 px) and
  `newbold.font` (16 px, upper case only), are compiled into the data area at the start
  of the assembler graphics library segment. The names are Amiga style (the library
  mimics Amiga `graphics.library`: RastPort, `OpenFont`, `SetFont`, `Text`,
  `TextLength`). There are no `.font` files on disk.
- Text is stored in code page 437. Eight CP437 German characters are remapped into
  ASCII slots that hold their glyphs.
- Glyphs are expanded into a buffer with a foreground/background color and blitted
  either transparently (zero bytes skipped) or as an opaque copy.

## Addressing

`BMMAIN.EXE` is compressed with Microsoft EXEPACK ("RB" header at CS:0 of the packed
image, `CS:IP = 4bab:0010`, unpacked size `0x56e4` paragraphs, real entry `3a01:0016`).
The font data lies inside the compressed part, so offsets in the packed file are of no
use: unpack first (algorithm in `exepack_unpack`, see below). Only the first `0x35ad1`
bytes of the packed and unpacked images match.

All addresses below refer to the Ghidra program `/unpacked/BMMAIN_UNPACKED.EXE`, which is
loaded at segment `0x1000`: Ghidra `S:O` = load image offset `(S - 0x1000) * 16 + O`.
The graphics library segment is Ghidra `45ac` = image segment `0x35ac` (load image
offset `0x35ac0`; far calls in the image are encoded as `9a 83 2d ac 35` =
`CALLF 35ac:2d83`). DGROUP is image segment `0x4cb3` (Ghidra `5cb3`).

### EXEPACK decompression

1. MZ header: `e_cparhdr` (word +8) gives the header size, `e_cs` (word +0x16) the
   segment of the EXEPACK stub inside the load image.
2. Stub header at image `cs*16`: `real_ip, real_cs, mem_start, exepack_size, real_sp,
   real_ss, dest_len` (words), then the signature `"RB"` (at +14, or +16 in the variant
   with an extra `skip_len` word). `dest_len * 16` is the unpacked image size.
3. The packed data is image `[0, cs*16)`. Copy it to the start of a `dest_len*16`
   buffer, skip trailing `0xFF` bytes, then process commands backwards from the end:
   read the opcode byte, then a little-endian 16-bit count (the two bytes before it).
   `op & 0xFE == 0xB0`: read one more byte backwards, write `count` copies of it
   backwards. `op & 0xFE == 0xB2`: copy `count` bytes backwards. `op & 1` = last command.
4. The relocation table follows the string `"Packed file is corrupt"` in the stub
   (16 groups of `count, offsets…`, group `i` for segment `i*0x1000`). Fonts do not need
   relocations.

## Data layout (segment 45ac)

| Address | Content |
|---|---|
| `45ac:03bc` | `"normal.font"`, `"micro4.font"` (`03c8`), `"flow.font"` (`03d4`), `"newbold.font"` (`03de`) |
| `45ac:03eb` | character translation table: count byte `8`, then 8 `(from, to)` byte pairs |
| `45ac:03fc` | font count word `4` |
| `45ac:03fe` | 4 near pointers to font descriptors: `0e5e` (normal), `0e64` (micro4), `11ba` (flow), `07ee` (newbold) |
| `45ac:0406` | glyph expansion buffer, `0x3e8` (1000) bytes, up to `07ee` |
| `45ac:07ee`.. | newbold descriptor + glyphs, then normal, micro4, flow descriptors + glyphs, up to ~`15a8` |

The table index (0..3) is the "font index" the library works with.

### Font descriptor

| Offset | Type | Meaning |
|---|---|---|
| +0 | u16 | near pointer to the name string |
| +2 | u16 | cell height `H` (only the low byte is used per glyph) |
| +4 | u16 | descent: rows below the baseline |
| +6 | u16[95] | near pointers to the glyphs for codes `0x20..0x7e` |

| Index | Name | H | descent | Notes |
|---|---|---|---|---|
| 0 | normal.font | 16 | 4 | **Stub**: no glyph table (its table would overlap the micro4 descriptor). Never opened by the game. |
| 1 | micro4.font | 6 | 1 | widths 2..6 |
| 2 | flow.font | 8 | 1 | widths 3..8; the main UI font |
| 3 | newbold.font | 16 | 1 | widths 5..23; lower-case slots point to the upper-case glyphs; most punctuation is blank |

### Glyph

```
u8  width W            advance width = bitmap width (spacing is part of the bitmap)
u8  bits[H][ceil(W/8)] rows top to bottom, MSB = leftmost pixel; unused low bits ignored
```

Glyphs may be shared (several pointers to one glyph, e.g. the blank glyph).

## Character mapping — `45ac:16a0` `font_map_char_to_glyph`

Input AL = raw byte, output AX = glyph index:

1. `AL < 0x20` → `AL = 0x20`.
2. Otherwise scan the 8 pairs at `03eb`; on the first `from == AL` set `AL = to` and stop
   (no further check):
   `5c→60` (backslash shows the grave slot), `8e Ä→5b`, `99 Ö→5c`, `9a Ü→5d`,
   `84 ä→7b`, `94 ö→7c`, `81 ü→7d`, `e1 ß→7e`.
3. If no pair matched and `AL >= 0x7f` (unsigned) → `AL = 0x20`.
4. Return `AL - 0x20`, AH = 0.

So `[ \ ] { | } ~` in a string show the umlaut glyphs, a literal backslash shows the
`` ` `` glyph, and every other non-ASCII byte shows a space.

## Drawing — `45ac:2d83` `font_draw_text`

Far cdecl, arguments (word each, BP offsets):

| BP | Argument |
|---|---|
| +06 | destination width `DW` |
| +08 | destination height `DH` |
| +0A/+0C | destination far pointer (`A000:0000` = Mode X screen) |
| +0E | x (signed) |
| +10 | y = **baseline** row (signed) |
| +12 | font index (0..3) |
| +14 | fg color (low byte) |
| +16 | bg color (low byte) |
| +18 | mode: 0 = transparent, non-zero = opaque (low byte) |
| +1A/+1C | string far pointer |
| +1E | character count (no NUL check) |

Algorithm (all arithmetic 16-bit; comparisons signed):

1. `desc = table[font]`; `y -= desc.H - desc.descent - 1` (this is the cell top).
2. Loop while `count != 0`: `count -= 1`; stop if `x >= DW`.
3. Map the byte, fetch glyph `W` and the bits. Expand row by row into the buffer at
   `45ac:0406` (`W*H` bytes, stride `W`): set bit → fg; clear bit → bg if mode != 0,
   otherwise 0. The bit loop uses an 8-bit counter `DL = W` decremented per pixel; the
   source byte advances after 8 pixels and once more at row end
   (= `ceil(W/8)` bytes per row; `W = 0` would wrap to 256, not present in the data).
4. Blit `W x H` from the buffer at (0,0) to (x, y_top): mode 0 → `45ac:20c8`
   (transparent: skip source bytes equal to 0, so fg 0 also draws nothing), else
   `45ac:1d1c` (opaque copy, bg 0 is written).
5. `x += W`.

When the destination segment is `A000`, the routine hides/restores the mouse pointer
(flags `CS:0010`, `CS:027c`, call `45ac:17a0` at the end); not relevant for output.

### Blit clipping (`45ac:20c8` transparent, `45ac:1d1c` opaque)

Arguments: `src_w, src_h, src far*, sx, sy, dst_w, dst_h, dst far*, dx, dy, w, h`.
Signed 16-bit:

1. `sx<0`: `w+=sx; dx-=sx; sx=0`, return if `w<=0`. Same for `sy` with `h`, `dy`.
2. `dx<0`: `w+=dx; sx-=dx; dx=0`, return if `w<=0`. Same for `dy`.
3. Return unless `sx<src_w && sy<src_h && dx<dst_w && dy<dst_h`.
4. Trim: if `dst_w < dx+w` then `w += dst_w-(dx+w)`; same for height; then the same
   against `src_w`, `src_h`.
5. Copy `h` rows of `w` bytes (linear: `dst[(dy+r)*dst_w + dx + c]`). For `A000` the
   same pixels are written plane by plane in Mode X (80 bytes per row, page offset
   `0x4b00` when `CS:0033` is set); the result is identical to the linear model.

The screen bitmap used by the library is `{320, 240, A000:0000}` (see `45ac:0286`), i.e.
a 320x240 Mode X screen with two pages; text rendering itself is resolution independent.

## Text width — `45ac:2eb4` `font_text_length`

`(font, str far*, count)` → sum of the glyph widths of `count` mapped bytes (16-bit).

## C-side wrappers (segment 4930, "RastPort")

RastPort layout: `+0` far pointer to bitmap `{w, h, far ptr}`, `+4` fg pen (byte),
`+5` bg pen, `+6` draw mode, `+8` x, `+0A` y, `+0C` far pointer to a font handle
`{index, height, descent}` (6 bytes).

| Address | Name | Purpose |
|---|---|---|
| `4930:022c` | `font_open` | allocates a 6-byte handle and fills it via `45ac:2f16` `font_find_by_name` (compares the name with each descriptor name, writes `index, H, descent`, or `index = 0xffff`) |
| `4930:0216` | (close) | sets handle index to `0xffff` |
| `4930:0261` | `rastport_set_font` | `rp+0C = font` unless index is `0xffff` |
| `4930:04fd` / `0515` / `052d` | set fg / bg / draw mode | bytes +4/+5/+6 |
| `4930:031c` | move | `rp+8 = x`, `rp+0A = y` |
| `4930:0291` | `rastport_text` | calls `font_draw_text` with the rastport fields; **does not advance** `rp.x` |
| `4930:02f6` | `rastport_text_length` | calls `font_text_length` |
| `16c7:07d5` | `print_text_at` | `(rp, str, x, y, color)`: color 0 → 0xff, set fg, move, `strlen`, `rastport_text` |
| `176b:0bdb` | `fonts_open_all` | opens game font slots 1..3 from the DS name table at `5cb3:012e` (`flow`, `micro4`, `newbold`), handles stored at DS `0x14/0x18/0x1c` |
| `176b:0d56` | `input_filter_text_char` | keyboard input filter: letters and digits pass; `" ,:.äöüß"` (Latin-1 bytes at DS `0x1a0`) are replaced from `" ,:.[|]~"` (DS `0x1aa`); anything else is rejected |

## Notes for the remake

- Load fonts with `FontSet::load(data_dir)`; glyph bytes are read from the user's
  `BMMAIN.EXE` at runtime and never stored in the repository.
- `Font::draw_text_raw` reproduces `font_draw_text` including clipping and the
  early stop at `x >= DW`; `Font::draw_text` is the transparent 320x200 convenience form.
- `encode_cp437` converts Rust strings to the game's byte encoding.
