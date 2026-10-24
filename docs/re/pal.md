# Palettes (`PIC/1.PAL`, DAC programming, fades)

Status: reconstructed from code, verified against the original data files.
Rust implementation: `crates/bmp-formats/src/pal.rs`.

## Addresses used in this document

`BMMAIN.EXE`, `BMLOADER.EXE` and `TEAMED.EXE` are compressed with Microsoft
EXEPACK (16-byte header variant, signature `RB`, no `skip_len` field). The
program `/BMMAIN.EXE` in the Ghidra project is the *packed* image and its code
is meaningless past the first RLE block. All BMMAIN addresses below refer to the
unpacked image `/unpacked/BMMAIN_UNPACKED.EXE` (load segment 0x1000, so a raw
image segment `S` shows up as `S+0x1000`). DGROUP is raw segment 0x4CB3
(Ghidra 0x5CB3); "DGROUP:x" means offset x in that segment.

`BMINTRO.EXE` is not packed; its addresses are raw `segment:offset` of the load
image (code segment 0x0000, data segment 0x00BE).

## 1. File format of `PIC/1.PAL`

* Size: exactly 2304 bytes = 3 palettes x 256 entries x 3 bytes.
* Each palette: 256 entries of `R, G, B` (interleaved), each a 6-bit VGA DAC
  value 0..63 (verified: no byte exceeds 63).
* Palette `p` occupies bytes `p*768 .. p*768+767`.
* Entries 0..31 of every palette are multiples of 4 (4-bit precision, Amiga
  heritage). This matters for the fade, see section 6.
* Entry 255 is stored as (63,63,63) in all three palettes but is forced to
  black by the loader (section 2).

No other palette file exists. `.VGA` and `.CP` pictures carry **no palette**:
the picture loader `FUN_176b_0356` never calls any palette routine (its callees
are file I/O, the RLE decoder `45ac:3120` and plane copies only).

## 2. Loading: `gfx_init_load_palettes` (BMMAIN 18bc:0004)

Called once from the main start-up routine `FUN_18bc_0418`.

1. Copies the file name `pic\1.pal` from DGROUP:0x0206 to the stack.
2. Allocates the screen object (`4930:0712`) and the palette object
   (`4930:077e`); the palette object holds at `+8` a far pointer to a
   768-byte **planar shadow buffer** (`R[256]`, then `G[256]` at +0x100,
   `B[256]` at +0x200). The far pointer to the palette object field is kept in
   global `DGROUP-far 0x4B50` (`= *(0x2E70) + 8`).
3. Opens the file (`4930:0023`), allocates a 0x300-byte buffer.
4. For `p = 0, 1, 2`:
   * reads 0x300 bytes (`4930:0088`),
   * sets buffer bytes 0x2FD, 0x2FE, 0x2FF to 0, i.e. **entry 255 := (0,0,0)**,
   * selects VGA plane `p` for writing: `outp(0x3C4, 2); outp(0x3C5, 1 << p)`,
   * copies the 768 bytes with `45ac:304a` (a plain `rep movsb`) to
     `AE10:0000` = linear 0xAE100 = **video memory offset 0xE100 of plane `p`**.
5. Closes the file and frees the buffer.

So the three palettes live in the fourth off-screen page of the unchained
320x240 VGA mode (3 x 0x4B00 = 0xE100), one palette per bit plane 0/1/2. The
DAC itself is not touched here.

## 3. Which palette is used when

The game selects a palette with `pal_set_palette(pal_obj, src, count)`
(BMMAIN 4930:0624, section 5). `src` is either a palette number (VRAM plane) or
a far pointer to colour words.

| Call site | Caller | `src` | count | Meaning |
|---|---|---|---|---|
| 18bc:05f6 | `FUN_18bc_0418` (start-up) | DGROUP:0x0234 = **2** | 256 | first screen after the graphics init (title) |
| 18bc:1a5a | `FUN_18bc_0b5f` | DGROUP:0x0794 = **0** | 256 | main game screens |
| 18bc:22c2 | `FUN_18bc_2184` | DGROUP:0x0794 = **0** | 256 | main game screens |
| 3e3a:21e1 | `FUN_3e3a_1b29` | DGROUP:0x0794 = **0** | 256 | restore after the photo screen |
| 362f:2d12 | `FUN_362f_2be8` | DGROUP:0x0798 = **1** | 256 | screen built from pictures 31.VGA and 32.VGA |
| 3e3a:101a | `FUN_3e3a_0fdf` | pointer DGROUP:0x9394 | 16 | grey ramp for the photos 200..229.VGA |
| 176b:1507 | `screen_set_visible_with_fade` | stack words | 32 | restores entries 0..31 after a fade |

The three DGROUP selectors are initialised data and are never written by code
(no store instruction references them).

Additional per-entry writes with `pal_set_color` (4930:05dd):

* `FUN_362f_0af7` at 362f:0c7c: for `k = 0..12` sets entry `31-k` from the
  colour word `DGROUP:0x5498 + 2k` (decoded as
  `R=((w>>8)&15)<<2, G=((w>>4)&15)<<2, B=(w&15)<<2`). The initial image holds a
  green-to-red gradient (0x0C0, 0x2C0, ..., 0xD00), i.e. entries 19..31.
* `screen_set_visible_with_fade` sets entries 0..31 to black before a fade-in.

### Grey ramp (DGROUP:0x9394)

16 words, entry `i` = `reverse4(i) * 0x111`:
`000 888 444 CCC 222 AAA 666 EEE 111 999 555 DDD 333 BBB 777 FFF`.
It is loaded into DAC entries 0..15 immediately before the random photo
`200 + FUN_176b_0cc7(0, 29)` (presumably a random number 0..29) is loaded
(3e3a:1022..103e). Entries 16..255 keep their
previous values (palette 0).

### BMINTRO.EXE

`BMINTRO.EXE` detects the adapter (INT 10h AX=1A00h / AH=12h BL=10h). On VGA
(`DS:0x1B == 0x0F`) the routine at 0000:0b39 sets mode 13h with `INT 10h
AX=0013h` and calls 0000:0aba, which programs all 256 DAC entries directly:

```
for i in 0..256: out 0x3C8, i; out 0x3C9, R; out 0x3C9, G; out 0x3C9, B
```

The source is `DS:0x1382` with `DS = 0x00BE`, i.e. file offset
`header_paragraphs*16 + 0xBE0 + 0x1382`. Entries 0..86 are real colours
(0 = black, 1 = (55,55,55), 2..18 = grey ramp 2,5,8,...,50, 19..86 = an
orange ramp R=45, G=0..63, then G=63,B=1); entries 87..255 are stored as 0xFF
bytes, which the DAC latches as 63 (white). `pal::bmintro_palette` reproduces
this including the 6-bit masking.

`BMLOADER.EXE` and `TEAMED.EXE` link the same graphics library (they contain the
same fade routine and INT 10h AX=1010h code) but load no `.pal` file; they were
not analysed further.

## 4. DAC access primitives (graphics library segment 0x45AC)

All DAC writes go through the video BIOS, never through ports 0x3C8/0x3C9.

| Address | Ghidra name | Operation |
|---|---|---|
| 45ac:2f93 | `vga_screen_disable` | `INT 10h AX=1201h BL=36h` (video refresh off: screen black, DAC untouched) |
| 45ac:2f9b | `vga_screen_enable` | `INT 10h AX=1200h BL=36h` (video refresh on) |
| 45ac:2fa3 | `vga_set_dac_entry(index, r, g, b)` | `INT 10h AX=1010h`, `BX=index, DH=r, CH=g, CL=b` |
| 45ac:2fd8 | `vga_set_dac_from_vram_palette(first, count, plane, shadow far*)` | see below |
| 45ac:33ca | `vga_fade_32_colors(words far*, fade_in)` | section 6 |
| 45ac:34ef | `vga_wait_vretrace` | busy-wait until bit 3 of port 0x3DA is set (returns at once if already in retrace) |

`vga_set_dac_from_vram_palette`:

1. Graphics controller index 4 (read map select) := `plane`.
2. For each component `c = 0,1,2`: for `i = 0..count-1`:
   `shadow[c*0x100 + first + i] = vram[0xE100 + 3*(first+i) + c]` (read from
   `A000:` with the selected plane) - de-interleaves into the planar shadow.
3. `INT 10h AX=1012h`, `BX=first`, `CX=count`, `ES:DX = A000:(0xE100 + 3*first)`:
   the BIOS reads the interleaved table straight out of video memory (through
   the selected read plane) and loads `count` DAC registers.

Net effect: DAC entries `first..first+count-1` = palette `plane` of `1.PAL`
(with entry 255 black), and the shadow buffer mirrors them.

## 5. Palette object routines (segment 0x4930)

* `pal_set_color(pal_obj, index, r, g, b)` (4930:05dd): writes `r,g,b` into the
  planar shadow at `index`, `index+0x100`, `index+0x200`, then
  `vga_set_dac_entry`.
* `pal_get_color_12bit(shadow, index)` (4930:06d5): returns the colour word
  `w = ((R << 4 | G) << 2) | (B >> 2)` (16-bit). For 4-bit-exact colours
  (multiples of 4) this is the clean `0x0RGB` word; otherwise the fields overlap
  (e.g. (1,2,3) -> 0x0048) and the round trip is lossy.
* `pal_set_palette(pal_obj, src, count)` (4930:0624):
  * if `(int16)src.hi < 0`, or `src.hi == 0 && src.lo < 4` (unsigned): calls
    `vga_set_dac_from_vram_palette(0, count, src.lo, shadow)`;
  * otherwise `src` points to `count` words; for `i = 0..count-1`:
    `R = (w >> 6) & 0x3C`, `G = (w >> 2) & 0x3C`, `B = (w & 0x0F) << 2`,
    stored to the shadow and set with `vga_set_dac_entry(i, R, G, B)`.

## 6. Showing / hiding a screen and the fade

`screen_set_visible_with_fade(screen, on)` (BMMAIN 176b:1416) is called around
every screen change (about 80 call sites, e.g. 18bc:047b hide and 18bc:0614
show at start-up).

```
if (int8)DGROUP:0x014A == on: return      // initial value 0xFF -> first call always runs
DGROUP:0x014A = on
if DGROUP:0x060C == 0:                     // fade option off
    vga_wait_vretrace()
    on ? vga_screen_enable() : vga_screen_disable()
    return
for i in 0..32:
    words[i] = pal_get_color_12bit(shadow, i)
    if on: pal_set_color(i, 0, 0, 0)       // DAC and shadow 0..31 := black
if !on:
    vga_fade_32_colors(words, 0)           // fade out (screen still visible)
    vga_screen_disable()
else:
    vga_screen_enable()
    vga_fade_32_colors(words, 1)           // fade in
pal_set_palette(shadow, words, 32)         // restore entries 0..31 (DAC + shadow)
```

Only DAC entries **0..31** take part in the fade; entries 32..255 stay at full
brightness during a fade-in/out (they are hidden afterwards by the BIOS screen
disable). The restore through 12-bit words is lossless because entries 0..31
of all `1.PAL` palettes are 4-bit exact.

`DGROUP:0x060C` is checkbox 14 of the options flag array `DGROUP:0x05FE + i`
(i = 0..14, drawn by the options screen `FUN_362f_01b8`). Its value in the EXE
image is 0, i.e. fades are off until the option is enabled; `FUN_1310_2303`
temporarily forces it to 0 around one screen switch (1310:24eb..2518). The
options screen lists the label `Blenden` ("fades"), which is most likely this
checkbox (see open questions).

### `vga_fade_32_colors(words, fade_in)` (BMMAIN 45ac:33ca)

State in the code segment: current `cur[192]` at CS:0x003C, target `t[192]`
at CS:0x00FC, accumulators `acc[192]` at CS:0x01BC (all bytes). Only the first
96 bytes (32 colours) are ever sent to the DAC; the loop processes 192 bytes
but bytes 96..191 have no visible effect.

1. Convert the 32 words to 6-bit targets with byte arithmetic
   (`AX = word`): `R = AH << 2`, `G = (AL & 0xF0) >> 2`, `B = (AL & 0x0F) << 2`.
2. Fade in: `cur := 0`, `acc := 0`. Fade out: `cur := t`, `acc := 0`.
3. `CX = 60`; loop:
   * for every byte `j`: `acc[j] += t[j]` (8-bit);
     if `(int8)acc[j] >= 60` then `acc[j] -= 60` and `cur[j] += 1` (fade in) or
     `cur[j] -= 1` (fade out);
   * if `CX` is odd: wait for vertical retrace (port 0x3DA bit 3) and upload
     `INT 10h AX=1012h, BX=0, CX=32, ES:DX = CS:0x003C`;
   * `CX -= 1`; repeat while `CX != 0`.

So there are 60 accumulation steps and **30 DAC uploads** (after steps
2, 4, ..., 60). After `k` steps a component with target `T` is exactly
`floor(k*T/60)` (fade in) or `T - floor(k*T/60)` (fade out); the last upload
equals the target / black exactly. With `T <= 60` the accumulator never
exceeds 119, so the signed compare never misbehaves.

Timing: each upload waits for the retrace bit, so the fade runs at most one
upload per display frame. The graphics mode (45ac:1c80: mode 13h, then
unchained with Misc Output 0xE3 and a 480-line CRTC table, i.e. 320x240 "mode
X") refreshes at ~60 Hz, giving a fade of 30 frames (~0.5 s). The wait does not
wait for the *start* of retrace, so on a fast machine two uploads could fall into
the same retrace; a renderer should use one upload per 60 Hz frame.

`pal::fade_frames` returns the 30 uploaded states for a 32-word input.

## 7. Converting to 8-bit RGB

DOSBox expands a 6-bit DAC value `v` (only the low 6 bits are latched) as
`(v << 2) | (v >> 4)`; 63 -> 255, 32 -> 130. `Palette::to_rgb8` and
`pal::dac6_to_8` implement exactly this.

## 8. Rust API summary (`bmp_formats::pal`)

| Item | Purpose |
|---|---|
| `Palette` | 256 x `[r,g,b]` 6-bit; `from_dac_bytes`, `to_dac_bytes`, `to_planar`, `to_rgb8`, `set_color`, `get_color_word`, `set_from_words` |
| `PalFile::parse / load` | `PIC/1.PAL` -> 3 palettes; `vram_palette(p)` applies the entry-255 = black rule |
| `PALETTE_STARTUP / PALETTE_MAIN / PALETTE_ALT` | 2 / 0 / 1 (section 3) |
| `word_to_rgb6`, `fade_word_to_rgb6` | colour word decoders of 4930:0624 and 45ac:33ca |
| `grey_ramp_words()` | DGROUP:0x9394 table, generated by formula |
| `bmintro_palette(exe)` | the BMINTRO.EXE start-up palette |
| `fade_frames(words, dir)` | the 30 DAC states of a fade |

## Open questions

* Confirm that options checkbox 14 (DGROUP:0x060C) is the `Blenden` entry and
  find where its value is loaded/saved (savegame or options file).
* Map `FUN_18bc_2184`, `FUN_18bc_0b5f`, `FUN_362f_2be8`, `FUN_362f_0af7` to the
  concrete in-game screens (needs the screen/menu reverse engineering).
* Verify the fade frame rate against a DOSBox capture.
