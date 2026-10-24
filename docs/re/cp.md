# `PIC/<n>.CP` full-screen pictures

Status: fully reconstructed from `BMMAIN.EXE` and `BMLOADER.EXE` code. All 8
shipped files are decoded without errors (`crates/bmp-formats/src/cp.rs`, test
`cp::tests::decodes_all_original_files`).

## Summary

A `.CP` file holds one **320x240, 256-colour full-screen picture** for the VGA
Mode X screen the game runs in (unchained 320x240, 80 bytes per plane row,
0x4B00 bytes per plane per page). It has a 6-byte header followed by a single
RLE stream. The RLE control codes are **the same as for `.VGA` pictures**: the
game uses one decoder routine for both, which picks a Mode X write path when
the destination segment is `A000`.

CP pictures are static backgrounds and title/logo screens, not scrolling
backgrounds. No palette is stored in the file.

| File  | Body bytes | Content                                              | Loaded from                                  |
|-------|-----------:|------------------------------------------------------|----------------------------------------------|
| 22.CP | 58374 | blue marble background used behind menus/office screens | BMMAIN linear 0xc792 (`pic 0x16`)            |
| 45.CP | 22994 | "BUNDESLIGA-MANAGER HIGHSCORES" table (20 rows)          | BMMAIN linear 0x346ca (`pic 0x2d`)           |
| 46.CP | 38516 | advertising income/expenses screen (Trikotwerbung, ...)  | BMMAIN linear 0x28fa0 (`pic 0x2e`)           |
| 47.CP | 53807 | title picture, players + logo                            | BMMAIN linear 0x9194 (`pic 0x2f`)            |
| 48.CP | 57056 | title picture, stadium + logo                            | BMLOADER linear 0x0cdf (`pic 0x30`)          |
| 49.CP |  5893 | "eine KRON Produktion" logo                              | BMLOADER linear 0x0c82 (`pic 0x31`)          |
| 50.CP | 14092 | replay / video recorder control panel                    | no call with a literal picture number found  |
| 51.CP |  5105 | "Software 2000 präsentiert" logo                         | no call with a literal picture number found  |

## Addressing conventions (important)

All three game executables are **EXEPACK-compressed** (`MZ` header with
`e_crlc = 0`, entry in the unpacker stub, "Packed file is corrupt" string).
The shared Ghidra project contains the *packed* images, loaded at segment
0x1000 with no relocations applied.

* For `BMMAIN.EXE` the packed and unpacked load images are byte-identical up to
  linear offset 0x35ad2. Code below that offset has the Ghidra address
  `1000:0000 + linear` (Ghidra splits into 64 KiB "segments" `1000`, `2000`,
  `3000`, ... by linear address). Far addresses inside the code, e.g.
  `lcall 076b:0356`, are relative to image segment 0 and need +0x1000 for Ghidra.
* The graphics library segment `35ac` starts at linear 0x35ac0, i.e. after the
  identical prefix. In the packed image (and therefore in Ghidra) its bytes
  appear 0x704 bytes earlier: unpacked linear `L` is at Ghidra
  `4000:(L - 0x704 - 0x30000)` for the routines below.
* A throwaway unpacker is at `out/cp/unexepack.py` (writes the flat unpacked
  image and the relocation list; the reloc table parse consumes exactly
  `exepack_size` bytes, which validates it). Unpacked DGROUP of `BMMAIN.EXE` is
  segment 0x4cb3 (the "MS Run-Time Library" string sits at DGROUP:0008).

| Role | Unpacked far | Unpacked linear | Ghidra (packed) | Ghidra name |
|------|--------------|-----------------|-----------------|-------------|
| Picture loader (VGA and CP) | `076b:0356` | 0x07a06 | `1000:7a06` | `load_pic_cp_or_vga` |
| RLE decoder (linear + Mode X) | `35ac:3120` | 0x38be0 | `4000:84dc` | `FUN_4000_84dc` |
| CP source plane-advance helper (near) | `35ac:3107` | 0x38bc7 | `4000:84c3` | `cp_vram_source_advance_plane` |
| VRAM page copy (write mode 1) | `35ac:332b` | 0x38deb | `4000:86e7` | `vga_copy_vram_page_write_mode1` |
| Far memory copy | `35ac:304a` | 0x38b0a | - | - |
| CP loader, BMLOADER copy | `0000:0402` (near) | 0x00402 | - | - |

The strings `"pic\\"`, `".cp"`, `".vga"` are at DGROUP offsets 0x192, 0x197,
0x19b (file offset 0x41d1d).

## File layout

All values little-endian.

| Offset | Size | Field | Notes |
|-------:|-----:|-------|-------|
| 0 | u16 | `width`  | 320 in all files. Read into the loader's `width` argument slot, not used on the CP path. |
| 2 | u16 | `height` | 240 in all files. Read, not used on the CP path. |
| 4 | u16 | `packed_len` | Length of the RLE stream including the terminator. Only steers the chunked copy (see below). |
| 6 | ... | RLE stream | Terminated by byte 0x80. |

`50.CP` carries 96 junk bytes after the terminator (`packed_len` = 13996, body
= 14092); they are copied to VRAM but never read by the decoder.

## Loader: `load_pic_cp_or_vga` (`076b:0356`)

C-like signature (far cdecl):
`void far *load_pic(u8 pic_no, u16 width, u16 height, u8 arg_c, u8 do_load)`.
A CP picture is requested with `width == 30000` (0x7530), `height = 0`,
`arg_c = 0`, `do_load = 1`; e.g. at linear 0xc78b:

```
push 1 ; push 0 ; push 0 ; push 0x7530 ; push 0x16 ; lcall 076b:0356
```

CP path, step by step (linear addresses in the unpacked image):

1. 0x7a29: `width == 30000` skips allocating an image descriptor.
2. 0x7a73..0x7abd: build the path: `strcpy(buf, "pic\\")`, `strcat(buf,
   num_to_str(pic_no))` (number-to-string helper at 0x7d31), `strcat(buf, ".cp")`; flag `is_cp = 1` at `[bp-64h]`.
3. 0x7acd: `open`; failure -> fatal error 2 (third entry of the error-name list `LIB-Error`, `GRAFIK-Error`, `DISK-Error`, ...).
4. 0x7ae3..0x7b16: three 2-byte reads into `[bp+8]` (width), `[bp+0Ah]`
   (height), `[bp-5Ah]` (`packed_len`).
5. 0x7b22: allocate a 0x4B00-byte temporary buffer (`lcall 3930:010c`) (fatal error 4 on failure). Destination
   far pointer = `A4B0:0000` (= `A000:4B00`, the second Mode X page).
6. 0x7b4c..0x7bdb: for plane `p = 0..3`: write Sequencer map mask
   (`out 3C4h, 2; out 3C5h, 1<<p`); if `packed_len == 0x4B00` read
   `packed_len` bytes, copy them to `A000:4B00` and stop; otherwise read
   0x4B00 bytes, copy 0x4B00 bytes to `A000:4B00`, `packed_len -= 0x4B00`
   (16-bit wrap) and continue. Reads at EOF simply return fewer bytes. Net
   effect for every possible header value: **the file body (up to 4 x 0x4B00 =
   76800 bytes) is stored in VRAM page 1, plane 0 first, each plane holding
   0x4B00 consecutive file bytes.**
7. 0x7be7, 0x7c23: free the buffer, close the file.
8. 0x7cc8..0x7ce4: `rle_decode(src = A4B0:0000, dst = *(far*)(*(far*)DS:2E70 + 0x20))`
   via `lcall 35ac:3120`. The destination is the far pointer stored at offset
   0x20 of the screen descriptor whose address lives at DGROUP:2E70; its
   segment is 0xA000 (current draw page), which selects the Mode X branch.
9. Return value 1 (`ax=1, dx=0`).

The compressed data therefore never occupies conventional memory: VRAM page 1
is used as scratch space. After the load, BMMAIN's callers typically call
`35ac:1ac7` and, for 22.CP, cache the decoded page with
`vga_copy_vram_page_write_mode1(0x0000, 0x9600)` (copy page 0 to the page at
offset 0x9600 using VGA write mode 1; later calls restore it with
`(0x9600, 0x0000)` instead of reloading the file, guarded by DGROUP byte 0x210).

## Decoder: `rle_decode` (`35ac:3120`)

Arguments: `src` far pointer at `[bp+6]`, `dst` far pointer at `[bp+0Ah]`.

Control codes (identical in both branches):

| Byte `c` | Meaning |
|----------|---------|
| `0x00..0x7F` | literal: copy the next `c + 1` bytes (1..128) |
| `0x80` | end of picture |
| `0x81..0xFF` | run: repeat the next byte `(c & 0x7F) + 1` times (2..128) |

* **Linear branch** (`dst` segment != 0xA000, used for `.VGA` into RAM):
  `rep movsb` / `rep stosb`.
* **Mode X branch** (`dst` segment == 0xA000, used for `.CP`):
  * Graphics Controller read map select (`3CEh` index 4) starts at plane 0
    (`bl = 0`); the source is read byte by byte. After every byte the near
    helper `35ac:3107` increments `si`; when `si == 0x4B00` it selects the next
    read plane (`++bl`) and reloads `si` from the source offset argument. This
    reproduces the plane-by-plane order in which the loader stored the file,
    so **the logical source stream equals the file body**.
  * Output pixel counter: `cl` = plane (0..3), `di` = offset. Each pixel sets
    the Sequencer map mask to `1 << cl`, writes one byte at `es:di`, then
    `cl += 1`; when `cl == 4`, `cl = 0` and `di += 1`. So pixel `i` goes to
    plane `i & 3`, offset `dst_off + (i >> 2)`. With 80 bytes per row this is
    exactly row-major order of a 320-pixel-wide image: pixel `i` is at
    `x = i % 320`, `y = i / 320`.
* No bounds checks. All shipped files produce exactly 76800 pixels. A stream
  producing more would write past offset 0x4B00 into VRAM page 1 and corrupt
  its own remaining source; the Rust port reports `CpError::Overflow`
  instead. Running past the 76800 stored bytes would read stale VRAM; the port
  reports `CpError::UnexpectedEnd`.

## Reimplementation recipe

```
body   = file[6 .. min(len, 6 + 76800)]
pixels = []
loop:
    c = body.next()
    if c < 0x80:   pixels += body.next(c + 1)
    elif c == 0x80: break
    else:          pixels += [body.next()] * ((c & 0x7F) + 1)
assert len(pixels) == 320 * 240      # row-major, palette indices
```

## Other executables

* `BMLOADER.EXE` (also EXEPACK) contains its own copy of the loader at
  unpacked linear 0x402 (same `30000` test at 0x428, `.cp` string at file
  offset 0x77b9) and loads 49.CP then 48.CP during start-up, each preceded by
  a palette upload of 0x20 entries from DGROUP 0x66 / 0xa6 (`lcall
  0452:0624`).
* `TEAMED.EXE` contains the same loader (30000 test at unpacked 0x1156) but no
  call with a literal CP picture number.

## Palette

CP files contain no palette. 22/45/46 render correctly with the first 768
bytes of `PIC/1.PAL` (6-bit RGB); the title and logo pictures (47-51) are
shown with palettes set by their callers (see BMLOADER above), which is
outside this format.

## Verification

* `out/cp/dec.py` (throwaway) and the Rust decoder agree: FNV-1a of the
  decoded pixels for every file is asserted in the unit test.
* For every file `consumed bytes == packed_len` and width/height = 320/240.
* PNG renders in `out/cp/*.CP.png` and `out/cp/sheet.png` show coherent
  pictures (UI frames, text, logos) with no row shear, confirming the
  320-pixel row-major mapping.
