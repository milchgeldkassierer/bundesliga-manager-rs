# Launcher chain and BMINTRO.EXE (the Software 2000 logo)

Status: fully reconstructed from `BMINTRO.EXE` (not packed; hand-written assembly, no C runtime).
Addresses are offsets in the load image (`0000:xxxx`, Ghidra `1000:xxxx`). Data addresses are
`DS:xxxx` with `DS = 0x00be` (image offset `0x0be0`). The port is `crates/bmp/src/intro/`.

## 1. Launcher chain

`BMP.BAT` is the whole launcher:

```
echo off
bmintro
bmmain %1
```

1. `BMINTRO.EXE` shows the animated Software 2000 logo until a key is pressed (this document).
2. `BMMAIN.EXE` gets the batch argument (`bmp 0` = no sound) and starts with the title picture
   `PIC\47.CP` and `SOUND\BM2TITLE.CMF` (see [flow.md](flow.md)).

`BMLOADER.EXE` is **not** part of the chain. Neither the batch file nor `BMMAIN.EXE` or
`BMINTRO.EXE` names or executes it (no file name, no INT 21h AH=4Bh). Its `main` (`1000:0c1f`)
is a standalone title sequence: the "eine KRON Produktion" picture `PIC\49.CP`, a 200-tick delay,
the title picture `PIC\48.CP` with `BM2TITLE.CMF`, then `exit(99)`. It is probably left over from
another release and is not ported. The Software 2000 logo that DOSBox shows first comes from
`BMINTRO.EXE`.

`bmp run` follows `BMP.BAT`: the intro runs first, then the title. `bmp run --skip-intro`
leaves out the intro.

## 2. Data

BMINTRO reads no files. Everything is in its data segment:

| DS offset | Size | Content |
|---|---|---|
| `0x0e` | word | phase: 0 = logo reveal, non-zero = gradient box |
| `0x10` | word | reveal steps taken (only used by the EGA/Hercules cell data) |
| `0x12` | word | divider: iterations since the last reveal step |
| `0x14` | word | phase 1: current reveal step; phase 2: next gradient line |
| `0x16` | word | pointer to the next gradient colour byte (initially `0x0585`) |
| `0x18` | word | pointer to the next gradient edge-mask word (initially `0x0aa2`) |
| `0x1a` | byte | stage of the cell being drawn |
| `0x1b` | byte | adapter: `0x0f` VGA, `0xff` EGA, `0x00` Hercules |
| `0x1c` | byte | non-zero while the marquee runs |
| `0x35`, `0x41` | 12 bytes each | Hercules CRTC tables (graphics, text) |
| `0x0305` | 192 words | line table: VRAM offset / 4 of each gradient line (line `i` = `i * 80`) |
| `0x0622` | | EGA/Hercules cell pictures (not used on VGA) |
| `0x0ac2` | 214 x 8 bytes | cell records: start step, cell index (`row * 40 + column`), type, stage (0) |
| `0x1172` | | Hercules glyph masks |
| `0x1292` | 9 x 8 bytes | VGA glyph masks, one bit per pixel, MSB = left (types 1..8; type 0 = solid) |
| `0x12da` | 7 x 24 bytes, 0x30 apart | "SOFTWARE 2000" marquee bitmap (section 6) |
| `0x1382` | 768 bytes | palette (see [pal.md](../pal.md), `pal::bmintro_palette`) |

## 3. Start-up and exit

Origin: `0000:0000` (entry).

1. `0000:0b8c` detects the adapter: INT 10h AX=1A00h returns AL=1Ah -> VGA. Otherwise INT 10h
   AH=12h BL=10h: BL unchanged -> Hercules, else EGA. Only the VGA path is ported.
2. `0000:0ad5` -> `0000:0b39`: INT 10h AX=0013h (mode 13h, screen cleared), then `0000:0aba`
   writes all 256 DAC entries from `DS:1382` through ports 3C8h/3C9h. The palette never changes:
   there are no fades. Colour 1 is the grey of the marquee, 2..18 a grey ramp for the cell stages,
   19..86 the red-to-yellow ramp of the gradient box.
3. Main loop (`0000:000b`):

   ```
   loop {
       wait_vretrace();                         // 0000:0bbb
       if DS:1c != 0 { marquee(); }             // 0000:05c2
       if DS:0e == 0 { reveal_step(); }         // 0000:0087
       else          { gradient_line(); }       // 0000:0484
       if INT 21h AH=06h DL=FFh returns a key { break; }
   }
   restore_text_mode();                         // 0000:0b4a: INT 10h AX=0003h
   INT 21h AH=4Ch
   ```

The code after the exit call (`0000:0037..0086`, hooks for INT 1Ch and INT 1Bh) is never reached.

**Skip:** any key ends the intro at once, in every phase (INT 21h AH=06h also returns extended
keys). Mouse buttons are not read. There is **no sound**, and the intro never ends on its own:
the finished logo stays until a key is pressed.

## 4. Timing

The only clock is the vertical retrace. On VGA, `0000:0be4` polls port 3DAh until bit 3 is
set. It does not first wait for the retrace to end, so it returns at once while the retrace
pulse (about 2 of 449 lines, 64 us) is still active. One iteration per refresh therefore holds
only while an iteration takes longer than the pulse and shorter than a frame:

* On the machines of the time the iteration, with its DOS keyboard call, outlasts the pulse:
  one iteration per refresh, 70.086 Hz (25.175 MHz / (800 x 449)).
* In DOSBox at `cycles=max` the INT 21h call is native and nearly free. Many iterations run in
  each pulse, and the whole animation is over within a fraction of a second.
* In DOSBox at `cycles=fixed 1000` an iteration (up to about 3 200 cycles for a reveal step) fits
  in a frame and outlasts the pulse: one iteration per frame, as in the port. Below about
  300 cycles a reveal step spans several frames, and screenshots catch half-drawn steps.

The port runs exactly one iteration per refresh (`intro::wait_vretrace`). The animation state
depends only on the number of iterations `n`. Frame `n` is visible from `n / 70.086` s on, and
the logo is complete after **343** iterations (4.89 s).

| Iterations | What happens |
|---|---|
| 3, 6, ..., 210 | reveal steps 1..70 (step 70 draws nothing and switches to phase 2) |
| 91 .. 343 | marquee, 253 shifts (started during step 30) |
| 211 .. 342 | gradient lines 0..131 |
| 343 | line counter at 132: marquee off; the picture is final |

## 5. Phase 1: logo reveal

Origin: `0000:0087` (VGA path `0000:0122`), cells `0000:0345` / `0000:01a7` (VGA paths
`0000:0410` / `0000:0305`).

Every call increments the divider. On the third call it resets the divider, increments
`DS:10` and the step `DS:14`, and walks all 214 records. A record is drawn when
`start <= step` and `stage < 17`:

* Position: `index / 40` (8-bit `div`) gives the row (AL) and the column (AH). The cell's top-left
  pixel is at `column * 8 + row * 2560`.
* Colour: `stage + 2` (`DS:1a + 2`). After 17 stages a cell has gone from colour 2 to colour 18.
* Type 0 fills the 8x8 cell. If the step is 30 or later this also sets the marquee flag `DS:1c`.
  Types 1..8 set only the pixels of glyph `DS:1292 + type * 8` (rounded corners and the ends
  of the frame strokes). Other pixels keep their colour.
* Then the record's stage is incremented.

If no record was drawn in a step (detected by DI still being 0), the phase becomes 2
(`DS:0e = 0xffff`) and `DS:14` is reset. The latest start step is 53, so step 70 is the first
step without work. The records draw two strokes of the "S"-shaped frame at once, one cell per
step each, which makes the frame grow along its outline.

## 6. Marquee: "SOFTWARE 2000"

Origin: `0000:05c2` (VGA path `0000:09dc`). This runs on every iteration while `DS:1c != 0`:

1. Rows 193, 195, 197 and 199 move one pixel to the right. `std; rep movsw` of 159 words from
   `x = 318` down, with the source one byte to the left, so `x = 2..319` takes the old `x = 1..318`.
2. Rows 194, 196 and 198 move one pixel to the left (`cld; rep movsw`, `x = 0..317` takes the old
   `x = 1..318`).
3. Each odd row takes its next bit from a 192-bit row at `DS:12da + k * 0x30` (k = 0..3).
   `rcr` runs through its twelve words, the first word most significant. The bit shifted out
   of the last word sets `x = 3` to colour 1, otherwise to colour 0.
4. Each even row takes its next bit from `DS:12f2 + k * 0x30` (k = 0..2). `rcl` runs through
   the twelve words from the last one up, and the bit shifted out of the first word sets
   `x = 316`.

The bitmaps are consumed: zeros shift in, so after 192 calls only blank pixels follow and the
text keeps moving. The marquee stops after 253 shifts, when phase 2 ends, and the text stays
at that position.

## 7. Phase 2: gradient box

Origin: `0000:0484` (VGA path `0000:054d`). One line per iteration, for `line = DS:14`:

* `line >= 132`: clear the marquee flag and return (from then on every iteration does only this).
* Otherwise `DS:14 += 1`. `di = table[line] * 4 + 0x16ca`, which is screen row `18 + line`
  starting at `x = 74`.
* Edge mask: `0xffff`, except for lines `< 8` and `> 122`, which read the next word at `DS:18`.
  There are 17 such lines, but only 16 mask words precede the records. The last one
  (line 131) therefore reads the first record's start step, `0x0001`. That line gets no
  right end and a one-pixel left end (an original quirk, reproduced).
* Colour: the next byte at `DS:16`.
* The 16 mask bits are taken with `rcl` from the top. Bits 15..8 set `x = 230..237` (the right
  end, `di + 0x9c`), and bits 7..0 set `x = 82..89` (the left end). Then 70 words fill
  `x = 90..229`. The box spans `x = 82..237`, `y = 18..149`, with rounded corners.

## 8. Port

| Original | Port |
|---|---|
| main loop `0000:000b`, entry `0000:0000` | `intro::run` |
| `0000:0bbb` vertical retrace wait | `intro::wait_vretrace` (one iteration per 70.086 Hz refresh) |
| one loop iteration | `intro::anim::Intro::iterate` |
| `0000:0087` reveal step | `Intro::reveal_step` |
| `0000:0345` / `0000:01a7` solid / glyph cell | `Intro::draw_solid_cell` / `Intro::draw_glyph_cell` |
| `0000:05c2` marquee | `Intro::marquee` |
| `0000:0484` gradient line | `Intro::gradient_line` |
| `0000:0aba` palette | `bmp_formats::pal::bmintro_palette` |
| INT 21h AH=06h | `Host::key_hit` (any key; a key press without a character also counts) |

`Intro` keeps a mutable copy of the data segment and runs the routines on it. Every quirk above
(edge-mask overrun, consumed bitmaps, wrapping offsets) is therefore reproduced. During the
intro the host presents the mode 13h framebuffer directly (`Host::set_raw_video`). Afterwards
the screen is black until BMMAIN sets its mode, like the text-mode screen between the two
programs.

## 9. Verification

`tests/visual/intro.*.txt` captures the original at `cycles=fixed 1000` (`# cycles:` line read by
`tools/visual-check.sh`) and shoots iterations 196 (reveal), 252 (gradient box and marquee)
and the finished logo. All three match 100 %. Because the original's pace depends on the CPU,
the port shot times are aligned once per reference set to the iterations the references show.

Additional captures at 100, 300 and 1000 fixed cycles and at `max` were matched against every
port iteration 1..359:

* Every frame that DOSBox captured between two iterations matches one port iteration 100 %.
* Frames captured during a slow reveal step (100 and 300 cycles) consist only of pixels from the
  frames before and after that step.
