# `PIC\<id>.VGA` — RLE sprite images

Status: fully reconstructed from `BMMAIN.EXE`, verified against all 154 shipped files
(every file decodes, consumes exactly its bytes, and re-encodes byte-identically with
the game's own compressor). Rust implementation: `crates/bmp-formats/src/vga.rs`.

## 1. Address conventions (read this first)

`BMMAIN.EXE` is **EXEPACK-compressed** (entry `CS:IP = 4bab:0010`, unpacker stub with
`RB` signature; real entry `3a01:0016`, real `SS:SP = 5709:1000`, unpacked image size
0x56e40 bytes). The Ghidra project holds the *packed* file, loaded at segment 0x1000.
Consequences:

- Image offsets `0x00000 .. 0x35ad0` are stored verbatim in the packed stream, so for
  that range **Ghidra address = image offset + 0x10000** (e.g. image 0x7a06 = Ghidra
  `1000:7a06`).
- Code above image 0x35ad1 (the C runtime and the assembler graphics library) is
  compressed in Ghidra. Long literal runs survive, so most routines exist as a
  *shifted copy*; such addresses are given below as "shifted copy at ...". They are
  only useful for reading the disassembly; relative jumps are correct, far calls are
  unrelocated.
- Far-call targets in the listing are unrelocated raw `seg:off` values; image offset
  = `seg * 16 + off`.
- DGROUP (DS) starts at image 0x4cb30 (paragraph 0x4cb3; the MS C copyright string is at
  DGROUP:0x0008). "DGROUP:x" means image `0x4cb30 + x`.

A throwaway unpacker used for this analysis lives in `out/vga/unexepack.py` (not part
of the repo).

## 2. Loader: `pic_load_image`

Ghidra `1000:7a06` (image 0x7a06, called far as raw `076b:0356` from ~70 sites).
Function created and named in the shared Ghidra project; plate comment attached.

Prototype (MS C, far, cdecl):

```c
Sprite far *pic_load_image(u8 id, u16 width, u16 height, u8 unused, u8 load);
struct Sprite { u16 w; u16 h; u8 far *pixels; };   /* 8 bytes */
```

Behaviour:

1. `width == 30000` (`cmp [bp+8],0x7530` at `1000:7a29`) selects a full-screen `.CP`
   file (separate format, see `cp.md`); otherwise a `.VGA` sprite.
2. If `load == 0` no file is opened: a blank sprite of the given `width`/`height` is
   allocated (`1000:7cb5`, raw `3930:0545`).
3. File name: `strcpy(buf, "pic\\")` (DGROUP:0x192), `strcat(buf, itoa(id))`
   (`1000:7d31`), `strcat(buf, ".vga")` (DGROUP:0x19b; `.cp` is DGROUP:0x197). Shipped
   files are upper case: `PIC\<id>.VGA`. `id` is a byte, so ids are 0..255.
4. `open(buf, O_RDONLY)`; failure → fatal error 2 (`1000:7adb`).
5. Three `read(fd, &x, 2)` calls (`1000:7ae3`..`1000:7b11`) fill, in order,
   **`width`, `height`, `packed_len`** — the header values overwrite the caller's
   width/height arguments.
6. `.VGA`: `malloc(packed_len)`, `read(fd, buf, packed_len)` (`1000:7bf1`), `close`.
7. Sprite object initialised by raw `3930:057c` (image 0x3987c: `w = width`,
   `h = height`, `pixels = NULL`); pixel buffer `malloc(width * height)` (16-bit
   `MUL`, so `width * height` must fit 16 bits — max 65535 pixels); failure → fatal
   error 4.
8. `rle_decode(packed, sprite->pixels)` (raw `35ac:3120`, see §4), then the packed
   buffer is freed.
9. If `id > 199` (`cmp byte [bp+6],0xc7` at `1000:7d14`) the width is stored in the
   global at DGROUP-relative `0xa6ce` (used by the photo screen to centre the picture).

The file contains **no palette, no position and no transparency key**. Placement on the
320x200 screen is decided by the caller (blit routine raw `3930:0849` = image 0x39b49,
which copies a clipped rectangle; e.g. photos are centred in a frame at
`x = (0x6b - w)/2 + 0xca`, `y = (0x6a - h)/2 + 0x3e`, Ghidra `3000:f41a`..`3000:f43a`).

## 3. File layout

All values little endian.

| Offset | Type | Field | Notes |
|---|---|---|---|
| 0 | u16 | `width` | 8 .. 320 in shipped files |
| 2 | u16 | `height` | 6 .. 204 in shipped files |
| 4 | u16 | `packed_len` | number of RLE bytes that follow, terminator included |
| 6 | u8[packed_len] | RLE stream | decodes to exactly `width * height` bytes |

`file size == 6 + packed_len` for every shipped file (no trailing bytes). Pixels are
8-bit palette indices, row-major, no row padding.

## 4. RLE decoder: `rle_decode`

Image 0x38be0 = raw `35ac:3120`; shifted copy in Ghidra at `4000:84dc`.
Arguments: `u8 far *src`, `u8 far *dst`.

Read a control byte `c` from `src`:

| `c` | Action |
|---|---|
| `0x00..0x7f` | copy the next `c + 1` bytes verbatim (literal packet, 1..128 bytes) |
| `0x80` | end of stream |
| `0x81..0xff` | read one byte `v`, write it `(c & 0x7f) + 1` times (run, 2..128 bytes) |

There are no bounds checks; the stream ends only at `0x80`. Counts use `CX` with
`CH = 0`, so all counts are plain unsigned 8-bit + 1.

**Planar branch.** If the destination segment equals `0xA000`
(`cmp ax,[bp+0xc]` at shifted `4000:84fd`), the same packets are written into
unchained VGA memory instead (shifted `4000:8523`, image 0x38c27): pixel *i* goes to
plane `i % 4` (Sequencer map mask `0x3c4` index 2 = `1 << plane`) at offset
`dst + i / 4`. The source pointer is advanced by helper image 0x38bc7 (shifted
`4000:84c3`): when SI reaches 0x4b00 it bumps the Graphics Controller read-map plane
(`0x3ce` index 4) and restarts SI at the original offset — i.e. the packed data can
itself be read from off-screen VGA planes in 0x4b00-byte slices. `.VGA` sprites are
always decoded into a linear buffer; the planar path is used for `.CP` screens.

## 5. RLE encoder: `rle_encode`

Image 0x38c96 = raw `35ac:31d6`; shifted copy in Ghidra at `4000:8592`.
Arguments: `u8 far *src`, `u8 far *dst`, `u16 count`; returns the packed length
(terminator included). All 154 `.VGA` files are byte-identical to this encoder's
output, so it is reproduced exactly in `encode_rle`:

```
loop:
  hdr = out++ ; n = 0
  if src[i] == src[i+1]:                       # run candidate
      count -= 1
      if count == 0: count = 1 ; goto literal  # last byte: emit as literal
      *out++ = v = src[i]
      repeat: i++, n++, count--
              if count == 0: *out = 0x80 ; *hdr = n | 0x80 ; done
      until n == 0x7f or src[i+1] != v
      *hdr = n | 0x80 ; i++ ; goto loop
  literal:
  repeat: *out++ = src[i++], n++, count--
          if count == 0: *out = 0x80 ; *hdr = n - 1 ; done
  until n == 0x80 or src[i] == src[i+1]
  *hdr = n - 1 ; goto loop
```

Note the comparisons with `src[i+1]` may read one byte past the input when one byte
remains; only the last packet is affected (`encode_rle_with_past_end` models it).
Maximum packet length is 128 for both kinds.

## 6. Palettes

The files store indices only.

- Sprites `0..147.VGA` use indices 0..31 and are drawn with the current game palette
  (`PIC\1.PAL`, first 768 bytes = 256 x RGB 6-bit DAC values, give correct colours for
  all of them; the second/third 768-byte blocks of `1.PAL` differ only in entries
  0..31, see `pal.md`).
- Photos `200..229.VGA` (newspaper-style grey photos, 104 x <=105) use indices 0..15
  with a dedicated 16-colour palette. Before drawing one, the caller (Ghidra
  `3000:f39f`) installs the table at **DGROUP:0x9394** (image 0x55ec4) into palette
  entries 0..15 via raw `3930:0624` (image 0x39924, shifted copy at `4000:9220`):

  ```
  0x000 0x888 0x444 0xccc 0x222 0xaaa 0x666 0xeee
  0x111 0x999 0x555 0xddd 0x333 0xbbb 0x777 0xfff      (u16 0x0RGB each)
  ```

  Each word is converted to 6-bit DAC as `R = (w >> 6) & 0x3c`,
  `G = (w >> 2) & 0x3c`, `B = (w & 0x0f) << 2`. The grey level of index *i* is the
  4-bit bit-reversal of *i*, so the photo pixel data is bit-reversed brightness;
  drawing them with `1.PAL` produces noise.

## 7. Inventory of shipped files

154 files: ids 0-21, 23-44, 47-54, 60, 61, 63, 64, 80-147, 200-229. Notable groups:
80-147 are 38x38 icons, 9-20 are 139x82 stadium views, 50-54 are
88x88, 200-229 are the grey photos. Index range: 0..31 for ids < 200, 0..15 for photos.

## 8. Ghidra changes made

- Created function `pic_load_image` at `1000:7a06` (was not auto-detected) with a plate
  comment summarising this spec. Nothing else was created in the compressed region
  because the shifted copies are not real code addresses.
