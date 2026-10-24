# TORE: goal scene animations (`TORE\*.T*`, `TORE\*.V*`, `TORE\ANZAHL`)

Implementation: `crates/bmp-formats/src/tore.rs`.

## Address conventions

* `BMMAIN.EXE` is EXEPACK-compressed. The Ghidra project holds the *packed* image
  loaded at segment 0x1000. Code below image offset 0x35ad2 is stored verbatim, so
  Ghidra `1000:c7c4` = image offset 0xc7c4 and `2000:502c` = image offset 0x1502c.
* "raw `seg:off`" means a far address as it appears in the unpacked image's far calls
  (load segment 0). Routines above image 0x35ad2 (runtime library, blitters, timer)
  are **not** at the corresponding Ghidra address; they were read from the unpacked
  image (any of the `out/*/BMMAIN*unpacked*` files; produced by a standard EXEPACK
  unpacker).
* DGROUP = segment 0x4cb3 (from the C startup `mov di, 0x4cb3`), image offset 0x4cb30.
  `DS:xxxx` below is an offset into DGROUP.

## Which code uses the files

| Ghidra address | Name given | Purpose |
|---|---|---|
| `2000:4e59` | `tore_read_anzahl` | parses `tore\anzahl` at program start (called from `1000:9495`) |
| `2000:502c` | `tore_load_and_play_scene` | chooses a file, loads it, mirrors it, plays it, frees it |
| `2000:4fb4` | `tore_report_read_error` | closes the file and shows a "disk" error box on short reads |
| `2000:4a93` | `tore_mirror_scene` | mirrors positions / swaps team sprites |
| `1000:c7c4` | `tore_play_frames` | the frame loop (draw, sound, timing, page flip) |
| `2000:4c39` | `tore_load_scene_graphics` | loads PIC 26/27/29 and allocates the background strip |
| `2000:4d87` | `tore_free_scene_graphics` | frees them and reloads PIC 8 and PIC 38 |
| `2000:b223` | (caller) | match event report; calls `2000:502c` at `2000:b9a9` |

Strings (DGROUP): `DS:52be` `"BM-Ed1.3-WK"`, `DS:52d0` far pointer to it,
`DS:52fa` `"tore\anzahl"`, `DS:5306` `"tore\"`, `DS:530c` `".v"`, `DS:530f` `".t"`,
`DS:5312` `"e"`, `DS:5314` `"j"`. The file name is assembled in `DS:a374`.

## `TORE\ANZAHL`

Shipped content: `MAXSZENE:43|5|2` followed by a NUL.

Parser (`2000:4e59`):

1. `open("tore\anzahl")`, `read(fd, buf, 250)`; the returned count is kept in a byte
   `len` (fatal error 2 if the open fails).
2. `pos = len - 1` (8-bit, wraps). While `buf[pos]` is not a digit (signed-char
   compares against `'0'` / `'9'`), `pos--`.
3. Three times (k = 0, 1, 2): `acc = 0; mul = 1` (both 32-bit); while
   `'0' <= buf[pos] <= '9'`: `acc += (buf[pos] - '0') * mul; mul *= 10; pos--`.
   Then store and `pos--` (exactly one separator byte is skipped).
   * k = 0 (rightmost number) -> 32-bit `j_count` (far `4bd4`),
   * k = 1 -> 32-bit `e_count` (far `304c`),
   * k = 2 (leftmost) -> low byte `max_scene` (far `57ce`).
4. If `(signed char)max_scene < 1` or `e_count < 1` (32-bit signed) -> fatal error 2.

So `MAXSZENE:43|5|2` gives 43 plain scenes (numbers 2..43), E scenes 2..5 and
J scenes 2..2, which is exactly the set of shipped files (scene number 1 does not
exist; numbers always start at 2).

## Scene file names

`tore\` + decimal number + `.t` or `.v` + optional `e` or `j` (DOS is case-insensitive;
the shipped files are upper case: `2.T`, `2.VE`, `2.TJ`, ...).

* `.T` = **Tor**: the attack ends with the ball in the net.
* `.V` = **vergeben** (missed chance): same situation but the shot misses or is saved.
  For every number both files exist; the frame data differs (same situation,
  different finish).

The sound triggers are per file data, not tied to the extension. Measured over all
94 shipped files (sound numbers whose trigger byte is not `0xff`):

| set | files | triggers |
|-----|-------|----------|
| `.T` (42) | all 42 trigger sound 2 | 16 also trigger sound 5, 7 sound 3, 7 sound 4 |
| `.TE` (4) | all trigger 2 and 3 | 1 file also 5 |
| `.TJ` (1) | 2, 4, 5 | |
| `.V` (42) | 39 trigger sound 5 | 2 trigger sound 2 (`31.V`: frame 0x55, `43.V`: frame 0x2a, same as `43.T`); `30.V` triggers nothing |
| `.VE` (4) | 3 and 5 | |
| `.VJ` (1) | 4 and 5 | |

So sound 2 is typically the goal sound and sound 5 typically the miss sound, but
the game never derives a sound from the file name; it only plays what the header says.
* `E` suffix: the scene starts with the ball on the penalty spot and only shooter and
  keeper near it, i.e. **Elfmeter** (penalty) scenes. Selected when the caller asks for
  it or by a random chance (below); the flag is returned to the caller.
* `J` suffix: a rare joke scene (1 in 401 chance), e.g. the players lying in a row.

### Selection (`2000:5064..2000:515f`)

`random(lo, hi)` is `1000:8377`: `lo + rand() % (hi - lo + 1)` (C runtime `rand()`,
16-bit signed `idiv`). Arguments of `2000:502c`: `goal` (byte, `[bp+6]`; non-zero =
`.t`), `scoring_side` (`[bp+8]`), `screen_y` (`[bp+0xa]`), `force_e` (`[bp+0xc]`).
`j_flag` is a local initialised to 0 once per call.

```
threshold = goal ? 15 : 25
loop:
  n = random(2, max_scene)
  if random(0, threshold) == 0: force_e = 1          # persists across retries
  if force_e: n = random(2, (int16)e_count)
  else if random(0, 400) == 0: j_flag = 1; n = random(2, (int16)j_count)
  name = "tore\" + ltoa(n) + (goal ? ".t" : ".v")
  if force_e && !j_flag: name += "e"
  if j_flag: name += "j"
  fd = open(name)
  if fd fails: retry (a counter wraps at 50, the loop never gives up)
```

When the replay counter far `549a` is non-zero (a replay) the selection is skipped and
the previous name in `DS:a374` is opened again. The function returns `force_e`.

## Scene file layout

All values little endian.

| Offset | Size | Field |
|---|---|---|
| 0 | 12 | signature `"BM-Ed1.3-WK\0"`; compared with `strcmp` against `DS:52be` (mismatch: file closed, nothing shown) |
| 12 | 4 | `sound_frames[4]` -> `DS:52cc`: frame index at which sound 2, 3, 4, 5 starts; `0xff` = never |
| 16 | 1 | `last` -> `DS:52ca`: index of the last frame (signed char) |
| 17 | `(last+1)*164` | frames |
| ... | 1 | `n`: length of the author block |
| ... | `n` | author name, NUL-terminated, game charset (`]` = U-umlaut) -> far `4238:4bd8` |
| ... | rest | ignored (only `3.T`/`3.V` have 2 extra zero bytes) |

The loader reads exactly these pieces with separate `read()` calls; each must return
the full size, otherwise `2000:4fb4` reports an error and the selection loop retries.
The frame buffer is allocated as `(last + 2) * 164` bytes (`2000:5233`) but only
`(last + 1) * 164` are read.

Signed length bytes (both read with `cbw`):

* Frames (`2000:5259`): `AX = (cbw(last) + 1) * 0xa4` (16-bit `imul`), `DX = 0`;
  the `read()` result is sign-extended with `cwd` and compared with `DX:AX`
  (`2000:528c`). For `last` in `0x80..=0xfe` the count is negative as an i16
  (`-127*164 ..= -164`), `cwd` can never give `DX = 0`, so the file is rejected.
  `last = 0xff` requests 0 bytes and is accepted with zero frames (the frame loop then
  shows nothing, since `cbw(0xff) = -1 < 0`).
* Author (`2000:52aa`, compare at `2000:52ce`): the count passed to `read()` is
  `cbw(n)`; for `n >= 0x80` that is a huge unsigned count, `read()` returns the small
  number of bytes left, which never equals the negative `cbw(n)`, so the file is
  rejected.

No shipped file is affected; the Rust parser returns `ToreError::NegativeLength` for
both cases.

Authors found: `J]RGEN KRAHE`, `WERNER KRAHE`, `W.+J.KRAHE`, `M.BERGMANN`, `FORD PREFECT`.

### Frame (164 bytes)

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | `camera_x`: left edge of the visible window in the pitch picture |
| 1 | 1 | `camera_y`: top edge of the visible window |
| 2 + 6*j | 6 | object j, j = 0..26 |

Object:

| Offset | Size | Field |
|---|---|---|
| 0 | u16 | `x` in pitch-picture pixels |
| 2 | u8 | `y` relative to pitch row 0x23 |
| 3 | u8 | unused (high byte of a y word, always 0) |
| 4 | u16 | `sprite` id |

Sprite ids:

* `0..=0x3a` (0..58): team A cells (red); `0x19..=0x21` are the team's keeper-like
  cells that keep their facing when mirrored.
* `0x3b..=0x75` (59..117): team B cells (blue); `0x54..=0x5c` likewise.
* `0x76..=0x8b` (118..139): neutral cells (referee etc.), mirrored within the range.
* `>= 0x8e` (142..): ball cells (ball and its shadow), drawn 4 pixels wide.
* `1000`: left goal frame; any value `> 1000` (files use 1024): right goal frame.

File order is drawing order; the player never sorts. The objects are *mostly* stored
in ascending y: over all 94 files there are 6839 adjacent object pairs with
decreasing y, and every one of them involves a ball or shadow cell (id `>= 0x8e`), so
the editor apparently sorted only the player sprites. There is no delta
compression: every frame is a complete object list.

## Mirroring (`2000:4a93`)

Called right after loading as `tore_mirror_scene(1 - scoring_side)`; argument `m`.
Loop order: for object j = 0..26, for frame i = 0..last:

```
id   = obj.sprite (signed 16)
ball = id > 0x8d
off  = 7 - 4*ball                  # 7 for players, 3 for the ball
if m: obj.x = (uint16)(off * 0xfffe) - obj.x + 0x140      # = 320 - x - 2*off, wrapping
if obj.x > 1000: obj.x = 0                                  # unsigned compares
if obj.x > 0x134 and id < 0x8e: obj.x = 0x134
if obj.x > 0x13c: obj.x = 0x13c
if id < 0x8e and m:
   if id < 59:            id = (25 <= id <= 33) ? id + 59 : 117 - id
   elif 58 < id <= 117:   id = (84 <= id <= 92) ? id - 59 : 58 - (id - 59)
   else:                  id = 118 - id + 139
```

Then for every frame: `v = clamp(0x89 - camera_x, 0, 0x89)`; if `m`: `camera_x = v`.
(0x89 = 320 - 182 - 1.) Constants come from `DS:52d6..52ee`:
`3a 3b 75 76 8b 8e 90 91 91 19 21 54 5c` (words).
The clamps are applied even when `m == 0`.

## Graphics used (`2000:4c39`)

`pic_load_image` (`1000:7a06`) returns a far pointer to an object whose first field is
a far pointer to a bitmap `{u16 w; u16 h; u8 far *pixels}`.

* PIC 26 (`26.VGA`, 320x112): stadium and pitch. Its bitmap is the **canvas** the
  frames are drawn into (far `56ea`; object far `577c`).
* A new 320x77 bitmap (far `0x20`) receives a copy of PIC 26 rows 0x23..0x6f
  (`blit_opaque(pic26, 0, 0x23 -> strip, 0, 0, 320x77)`): the background strip.
* PIC 27 (`27.VGA`, 288x77): sprite sheet, 24 columns x 7 rows of 12x11 cells (far `56e6`).
* PIC 29 (`29.VGA`, 46x20): the two goal frames (far `2ea4`).

The palette is whatever the match screen has loaded; the scene code does not touch it.

## Blitters

Both take `(src_bitmap, sx, sy, dst_bitmap, dx, dy, w, h)`; wrappers at raw
`3930:0991` (opaque, an extra unused byte argument) and `3930:0aa5` (transparent);
cores at raw `35ac:1d1c` and `35ac:20c8`. Clipping (16-bit signed, identical in both):

```
if sx < 0: w += sx; dx -= sx; sx = 0; if w <= 0: return
if sy < 0: h += sy; dy -= sy; sy = 0; if h <= 0: return
if dx < 0: w += dx; sx -= dx; dx = 0; if w <= 0: return
if dy < 0: h += dy; sy -= dy; dy = 0; if h <= 0: return
if sx >= src.w or sy >= src.h or dx >= dst.w or dy >= dst.h: return
if dx + w > dst.w: w = dst.w - dx
if dy + h > dst.h: h = dst.h - dy
if sx + w > src.w: w = src.w - sx
if sy + h > src.h: h = src.h - sy
```

The transparent core skips source pixels equal to 0. When the destination segment is
0xA000 the cores write planar (mode X) video memory instead; geometry is the same.

## Playback (`1000:c7c4`, called from `2000:52fb`)

Argument: `screen_y`. The caller (`2000:b223`) computes it as
`((3 - slot) / 2) * 113 + 5` (C division), i.e. 118 or 5.

```
start = 0; ticks = 2
if replay_counter == 1: start = max(last - 18, 0)
if replay_counter != 0: ticks = 5
for i = start .. last:
   frame = buf + i*164
   for k in 0..=3: if (signed char)sound_frames[k] == i: play_sound(k + 2)   # 1000:7832
   timer_start(ticks)                                  # raw 35ac:307a: deadline = BIOS tick count 0040:006c + ticks
   blit_opaque(strip, 0,0 -> canvas, 0, 0x23, 320, 77)
   for j in 0..26:
      o = frame.objects[j]
      if o.sprite > 999:
         if o.sprite == 1000: blit_transparent(goals, 0,0    -> canvas, 0,     0x36, 24, 20)
         else:                blit_transparent(goals, 0x18,0 -> canvas, 0x128, 0x36, 23, 20)
      else:
         ofs, w = (4, 4) if o.sprite >= 0x8e else (0, 12)
         blit_transparent(sprites, (o.sprite % 24)*12 + ofs, (o.sprite / 24)*11
                          -> canvas, o.x, o.y + 0x23, w, 11)
   blit_opaque(canvas, camera_x, camera_y -> screen, 0x45, screen_y, 182, 96)
   if i == 0 ([bp-0xa] == 0, 1000:c9ae): copy page 0 -> page 0x4b00   # raw 35ac:332b
   flip display page                                   # raw 35ac:327d(0)
   wait until BIOS ticks >= deadline                   # raw 35ac:30bc
   if i == 0 ([bp-0xa] == 0, 1000:c9db): timer_start(10); wait   # 1000:c9e1
```

Note: both `i == 0` checks test only the frame index, never the replay counter
`549a`. They therefore fire on every showing that starts at frame 0: the first
showing, the full slow-motion replay (counter 2), and also the tail replay
(counter 1) when `last < 18` makes `start` clamp to 0. They never fire when
`start > 0`. After the loop: if `(last - start)` is odd, page 0x4b00 is
copied back to page 0; page 0 is shown (`35ac:327d(1)`); a 7-tick pause follows; if a
sound was triggered and far `18e != 1` `1000:78bf` is called (sound related, not analysed).

Frame rate: the BIOS tick is 18.2065 Hz, so normal playback is 2 ticks (~110 ms,
~9.1 fps) per frame and replays 5 ticks (~275 ms) per frame.

### Replays (caller `2000:b986..2000:b9f4`)

The caller clears the replay counter far `549a`, plays the scene, increments the
counter, then reads a key; while the counter is below 3 and the key is `r` or `w`
the scene is played again (same file). Showing 2 (counter 1) starts 18 frames before
the end in slow motion, showing 3 (counter 2) replays everything in slow motion.

## Reimplementation notes

* `tore::parse_scene` / `Scene::to_bytes` round-trip all 94 shipped files byte for byte.
* `tore::ScenePlayer` reproduces the drawing; `render_screen` places the 182x96 window
  on a 320x200 frame (with `screen_y = 118` the window would extend to row 213; the
  game's screen bitmap height there was not determined).
* Example renders: `out/tore/png/` (generated by
  `BMP_TORE_EXPORT=out/tore/idx cargo test -p bmp-formats tore` plus
  `out/tore/topng.py`, palette from the first 768 bytes of `PIC\1.PAL`).
