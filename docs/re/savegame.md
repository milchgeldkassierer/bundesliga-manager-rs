# Save games (`*.MAN`) and high-score tables (`HIGH.XY`)

Reconstructed from `BMMAIN.EXE`. Implementation: `crates/bmp-formats/src/savegame.rs`.

## Addressing conventions

`BMMAIN.EXE` is **EXEPACK-compressed** (stub at CS `4bab`, signature `RB`,
message "Packed file is corrupt"). The Ghidra project imported the *packed*
image at load base `1000:0000` and shows it in 64 KiB chunks. EXEPACK only
expands data from image offset `0x35ad1` upward, so:

* Code below linear image offset `0x35ad1` (including everything in this
  document) is byte-identical in Ghidra; Ghidra address = linear + `0x10000`,
  e.g. linear `0x32aae` = Ghidra `4000:2aae`.
* Everything above it, including the whole of DGROUP, is shifted/garbled in
  Ghidra. Strings such as `WeRnEr_krahe…` appear at Ghidra `5000:ac9b` but
  live at `DS:93bc` in the real program. Data offsets in this document come
  from an unpacked image (throwaway unpacker:
  `out/savegame/unexepack.py`, decompresses to `0x56e40` bytes; real entry
  `3a01:0016`).

Segments of the unpacked image (paragraphs relative to load base):

| Name | Paragraph | Notes |
|------|-----------|-------|
| code segment of the save/load module | `322b` | near calls inside it |
| `DS` = DGROUP | `4cb3` | set by startup `mov di,4cb3` |
| FAR data segment | `4238` | holds most game tables (`FAR:xxxx` below) |
| C runtime I/O | `3930` | `3930:0023` open, `:000c` close, `:0088` read, `:00d6` write, `:010c` far calloc |
| string runtime | `3a01` | `:0e78` strlen, `:0eba` strncmp, `:0e18` strcpy, `:0dd2` strcat |

The code paths use the "segment variables" at `DS:a0d0..a10e` (e.g.
`mov es,[a0da]`); each holds either `4238` or `4cb3`, so every access
below has been resolved to one of the two segments.

## Functions

| Real address | Ghidra | Name (set in Ghidra) | Purpose |
|---|---|---|---|
| `322b:07fe` | `4000:2aae` | `savegame_write_man` | writes a `.MAN` save |
| `322b:120c` | `4000:34bc` | `savegame_read_man` | loads a `.MAN` save |
| `322b:0047` | `4000:22f7` (plate comment, no function object) | savegame_write_encrypt | encrypt + write + restore buffer |
| `322b:013f` | `4000:23ef` | `savegame_read_decrypt` | read + decrypt |
| `322b:0006` | `4000:22b6` | `savegame_io_error` | sets I/O error flag `DS:9440` = 1 and shows a message |
| `322b:21c4` | `4000:4474` | `highscore_load_or_store` | reads/writes `HIGH.XY` |
| `322b:2366` | `4000:4616` | `highscore_insert_entry` | inserts the current manager, sorts, shows the table |

Callers of `savegame_write_man`: linear `0x88e9` (crash save to
`ERROR.MAN`, string `DS:01fc`), `0x9744` (autosave, `autosave.man` at
`DS:026b`), `0xa772` (user save). Loader callers: `0xa741`, `0xb23d`.
The `.MAN` extension is appended by the file-name editor around linear
`0x3585d` (`DS:94b1` / `DS:94b6`).

The error flag `DS:9440` is cleared at the start of a save; once set, every
further encrypted read/write becomes a no-op.

## `.MAN` file layout

All multi-byte values are little-endian. Every file the game wrote is exactly
**34381 bytes** (the string table is always empty in practice, see below).

| File offset | Size | Encoding | Content |
|---|---|---|---|
| `0x00` | 25 | raw | signature `WeRnEr_krahe~&~JeNs_onnen` (`DS:93bc`, written with `strlen`) |
| `0x19` | 4 | raw | version tag `>V2\0` (`DS:93e4`) |
| `0x1d` | 4 | raw | save id (`u32`) |
| `0x21` | 1 | raw | pad byte = `random(0,255)`; ignored by the loader |
| `0x22` | 1 | raw | initial key `K0` = `random(0,255) + 0x78` (wrapping), stored at `FAR:57cf` and copied to `FAR:56dc` |
| `0x23` | … | enc | 61 fixed blocks, a raw checksum byte after block 26 (table below) |
| `0x8640` | var | enc | string table (empty in all known saves) |
| `0x8640`+ | 1 | raw | checksum #2 (`FAR:56dc`) |
| +1 | 4 | raw | `DS:05f6` |
| +5 | 4 | raw | `FAR:2e7a` |
| +9 | 4 | raw | `FAR:2e94` |

Write sequence at `322b:08c7..0a71`: the save id is
`((r2 << 16) + sign_extend(r1)) + 1` (32-bit) with `r1`, `r2` from
`random(0, 0x8fff)` (`076b:0cc7`); it is also appended to a session
history of 50 ids at `FAR:a6d8` (count `DS:05ae`, shifted with
`movmem` when full). The history itself is not saved.

### Cipher and checksums

State: key `K` (`FAR:57cf`) and checksum `S` (`FAR:56dc`), both start at
`K0`. All arithmetic is on `u8` and wraps.

Encryption of a plaintext byte `p` (`322b:0047`, loop at linear `0x32323`):

```
S = S + p
c = K + (K XOR p)
K = K + 0xd7
```

Decryption (`322b:013f`, loop at linear `0x32436`):

```
p = (c - K) XOR K
K = K + 0xd7
S = S + p
```

The key stream runs continuously through the whole file. The raw checksum
bytes do **not** advance `K` or change `S`.

* **Checksum #1** at `0x6d66` = `S` after block 26 (`FAR:57aa`), i.e.
  `K0 + sum(plaintext of blocks 0..26)`. The loader reads it into
  `FAR:4b54` and compares with `FAR:56dc`.
* **Checksum #2** after the string table = `K0 + sum(all encrypted plaintext)`.

On a mismatch the loader still loads everything but then sets a flag
(`[bp-0xb8] = 5`) and enters an endless warning-screen loop
(linear `0x341ad..0x3434e`), effectively refusing tampered saves.

### Loader checks (`savegame_read_man`)

1. Reads `strlen(signature)` = 25 bytes and compares only the first **24**
   (`strncmp(..., 0x18)`). Mismatch: error message, abort.
2. Reads the 4-byte version and the 4-byte save id and closes the file. Only
   the first two version chars are compared with `>V`. A `>V1` save is
   accepted; afterwards (linear `0x34365`, test `version[2] == '1'`) the game
   prints "Sie haben einen V1.x-Spielstand geladen." / "Bitte geben Sie den
   Spiel-Level (0-5) ein:" and stores `DS:4a28 = 5 - input` (input 3 if the dialog returns -1).
3. If the save id is already in the session history `FAR:a6d8[0..50]` the
   load is refused; otherwise the id is appended.
4. Reopens the file, skips the header by reading `strlen+8` bytes into
   `FAR:2242` (overwritten later), reads the pad byte and `K0`.
5. Decrypts all blocks in the same order as written, checks checksum #1,
   rebuilds the string table, checks checksum #2, reads the raw tail.
   (If the read of `DS:05f6` is short, `0cb5:0000` is called and the two
   remaining tail reads are skipped.)

### Fixed blocks

`enc` = encrypted. "Call" is the call site inside `savegame_write_man`
(segment `322b`); the loader mirrors it in the same order.
Notes marked "observed" come only from the six available saves
(`07072019.MAN`, `2020-JUN.MAN`, `2021-AUG.MAN`, `AUTOSAVE.MAN`,
`BMP/AUTOSAVE.MAN`, `BMP/MICHEL1.MAN`), not from code.

| # | File offset | Size | Enc | Variable | Notes | Call |
|---|---|---|---|---|---|---|
| 0 | 0x0023 | 6 | enc | DS:060e | observed `07 05 03 02 02 02` or `05 05 05 05 05 05` | 322b:0b05 |
| 1 | 0x0029 | 4 | enc | DS:0618 | string-table group sizes, `u8[4]` (all 0 observed); the loop reads `DS:0618+i` | 322b:0b1c |
| 2 | 0x002d | 4 | enc | FAR:5350 | observed `xx ff ff ff` (xx = 0x20..0x39) | 322b:0b33 |
| 3 | 0x0031 | 1 | enc | FAR:304a |  | 322b:0b4a |
| 4 | 0x0032 | 1 | enc | FAR:5358 |  | 322b:0b61 |
| 5 | 0x0033 | 8 | enc | DS:079e | `u8[8]`, small values 1..3 | 322b:0b78 |
| 6 | 0x003b | 2280 | enc | FAR:6ddc | table of small values (0..6); differs heavily between saves | 322b:0b8f |
| 7 | 0x0923 | 1 | enc | DS:07ab | string-table group count (1 observed) | 322b:0ba6 |
| 8 | 0x0924 | 1 | enc | DS:07ac | observed 5, 14, 16 | 322b:0bbd |
| 9 | 0x0925 | 1 | enc | DS:07ad |  | 322b:0bd4 |
| 10 | 0x0926 | 1 | enc | DS:07ae |  | 322b:0beb |
| 11 | 0x0927 | 1 | enc | DS:07af |  | 322b:0c02 |
| 12 | 0x0928 | 1 | enc | DS:07b0 | observed 13, 30, 32 | 322b:0c19 |
| 13 | 0x0929 | 3112 | enc | FAR:2242 | **manager table**, 4 x 778 (`0x30a`) records, see below | 322b:0c30 |
| 14 | 0x1551 | 100 | enc | FAR:5714 |  | 322b:0c47 |
| 15 | 0x15b5 | 6800 | enc | FAR:3066 | **club table**, 200 x 34 records, see below | 322b:0c5e |
| 16 | 0x3045 | 3456 | enc | FAR:0ecc | 3456 bytes; mostly bytes < 0x80 (unknown) | 322b:0c75 |
| 17 | 0x3dc5 | 5587 | enc | FAR:57dd | **player table**, 151 x 37 records, see below | 322b:0c8c |
| 18 | 0x5398 | 6500 | enc | FAR:774a | 5 groups x 25 records x 52 bytes; far string pointer at record +0x30 (patched on load) | 322b:0ca3 |
| 19 | 0x6cfc | 60 | enc | FAR:4b5e | 60 bytes, a permutation-like list of small indices | 322b:0cba |
| 20 | 0x6d38 | 4 | enc | FAR:2eba | `u32`, observed 8..25 | 322b:0cd1 |
| 21 | 0x6d3c | 4 | enc | FAR:2e8e | `u32`, observed 1..10 | 322b:0ce8 |
| 22 | 0x6d40 | 4 | enc | FAR:a7a0 | `u32`, observed 0x7ad..0x7b0 (1965..1968); directly follows the unsaved save-id history FAR:a6d8 | 322b:0cff |
| 23 | 0x6d44 | 4 | enc | DS:07dc | `u32`, observed 0x58..0xd2 (increases with play time) | 322b:0d16 |
| 24 | 0x6d48 | 2 | enc | DS:07e0 | `u16`, observed 0x7ac..0x7af (1964..1967) | 322b:0d2d |
| 25 | 0x6d4a | 10 | enc | FAR:56fe | `u8[10]` | 322b:0d44 |
| 26 | 0x6d54 | 18 | enc | FAR:57aa | `u8[18]`, 0x80 = empty slot | 322b:0d5b |
| — | 0x6d66 | 1 | raw | FAR:56dc | checksum #1 (raw, key not advanced) | 322b:0d71 |
| 27 | 0x6d67 | 2 | enc | FAR:2e6e |  | 322b:0d8a |
| 28 | 0x6d69 | 128 | enc | FAR:76ca | 128 bytes | 322b:0da1 |
| 29 | 0x6de9 | 96 | enc | FAR:21e2 | 96 bytes (zero in all samples) | 322b:0db8 |
| 30 | 0x6e49 | 8 | enc | FAR:0008 | `u8[8]`, observed `03 03 03 03 00..` / `04 04 04 04 00..` | 322b:0dcf |
| 31 | 0x6e51 | 3 | enc | FAR:1d18 |  | 322b:0de6 |
| 32 | 0x6e54 | 60 | enc | FAR:535a |  | 322b:0dfd |
| 33 | 0x6e90 | 128 | enc | FAR:4c7e | 128 bytes (zero in all samples) | 322b:0e14 |
| 34 | 0x6f10 | 3 | enc | DS:225a | `u8[3]`, increases with play time | 322b:0e2b |
| 35 | 0x6f13 | 5012 | enc | FAR:9336 | 5012 bytes, starts with "10" then 0xff fill | 322b:0e42 |
| 36 | 0x82a7 | 15 | enc | DS:05fe | `u8[15]` flags | 322b:0e59 |
| 37 | 0x82b6 | 8 | enc | FAR:0000 | `u8[8]` | 322b:0e70 |
| 38 | 0x82be | 48 | enc | FAR:4b9a |  | 322b:0e87 |
| 39 | 0x82ee | 80 | enc | FAR:2ec2 |  | 322b:0e9e |
| 40 | 0x833e | 320 | enc | FAR:002e |  | 322b:0eb5 |
| 41 | 0x847e | 144 | enc | DS:066c |  | 322b:0ecc |
| 42 | 0x850e | 1 | enc | DS:4a28 | **difficulty level** (`5 - user level`) | 322b:0ee3 |
| 43 | 0x850f | 2 | enc | FAR:513c | `u16`, selects the high-score file variant (0x56ce in all samples) | 322b:0efa |
| 44 | 0x8511 | 30 | enc | FAR:1d34 |  | 322b:0f11 |
| 45 | 0x852f | 120 | enc | FAR:5664 | 3 x 10 far string pointers (patched on load) | 322b:0f28 |
| 46 | 0x85a7 | 7 | enc | FAR:5780 | `u8[7]` | 322b:0f3f |
| 47 | 0x85ae | 2 | enc | DS:063a |  | 322b:0f56 |
| 48 | 0x85b0 | 2 | enc | DS:07e2 |  | 322b:0f6d |
| 49 | 0x85b2 | 1 | enc | FAR:016e | `u8`, increases with play time | 322b:0f84 |
| 50 | 0x85b3 | 95 | enc | DS:2280 |  | 322b:0f9b |
| 51 | 0x8612 | 16 | enc | DS:0620 | 8 x `i16` | 322b:0fb2 |
| 52 | 0x8622 | 1 | enc | FAR:4bce |  | 322b:0fc9 |
| 53 | 0x8623 | 4 | enc | FAR:56e2 |  | 322b:0fe0 |
| 54 | 0x8627 | 4 | enc | DS:05fa | `[0]` is copied from DS:05ac right before writing | 322b:1004 |
| 55 | 0x862b | 4 | enc | FAR:76c6 |  | 322b:101b |
| 56 | 0x862f | 4 | enc | FAR:90c2 |  | 322b:1032 |
| 57 | 0x8633 | 4 | enc | DS:05d6 |  | 322b:1049 |
| 58 | 0x8637 | 4 | enc | DS:061c |  | 322b:1060 |
| 59 | 0x863b | 4 | enc | DS:05b8 | `u8[4]` | 322b:1077 |
| 60 | 0x863f | 1 | enc | FAR:3060 |  | 322b:108e |

Checksum #2 is at `0x8640` when the string table is empty, the tail at
`0x8641..0x864c`. Total: 35 header + 34331 block bytes + 1 + 0 + 1 + 12 =
34381.

### String table

Written by the loop at linear `0x33344..0x3341b`, read at
`0x33df7..0x33f8d`:

```
for i in 0 .. DS:07ab (u8):                  # group count
    for j in 0 .. DS:0618[i] (u8):           # group size
        enc u32  old far pointer  (FAR:5524[(i*20 + j)])
        enc u8   len = strlen + 1 (wraps; the loader sign-extends it)
        enc len  bytes of the string including NUL
```

The loader allocates each string (`3930:010c`), stores the new pointer in
`FAR:5524[i*20 + j]`, and replaces every occurrence of the old pointer in
the 52-byte records `FAR:774a` (field `+0x30`, 5 groups x up to 25 records
counted by `3091:1109(group)`) and in the 3 x 10 pointer table
`FAR:5664`. In all known saves `DS:07ab = 1` and `DS:0618[0] = 0`, so
the table is empty and the file size is constant. Note that the save loop
uses a signed byte index for `j`, so group sizes above 127 never terminate
in the original.

### Identified records

**Manager record** (`FAR:2242 + i*0x30a`, `i` = 0..3; stride used at linear
`0x3403b`/`0x3411a`):

| Offset | Size | Field |
|---|---|---|
| `+0x00` | 29 | name, NUL-terminated (e.g. `OLIVER`); empty for unused slots |
| `+0x1d` | 1 | slot number 1..4 (loader writes `i+1` if zero, linear `0x34125`) |
| `+0x1e` | 1 | club index into the club table; `100` = none |
| `+0x1f` | 747 | not yet decoded |

Anchor: `AUTOSAVE.MAN` manager 0 is `OLIVER` with club `0x23`
= "BAYER 05 UERDINGEN", which matches the high-score entry in `HIGH.00`.

**Club record** (`FAR:3066 + i*34`, 200 entries): `+0` name, 20 bytes,
NUL-padded (CP437, e.g. `FORTUNA DÜSSELDORF`); `+20` 14 attribute bytes
(not yet decoded). Observed order: German clubs first (index 0 =
"BORUSSIA DORTMUND"), foreign clubs at the end (index 199 = "ANDORRA KICKERS").

**Player record** (`FAR:57dd + i*37`, 151 entries): `+0` name, 26 bytes,
NUL-padded; `+26` 11 attribute bytes (not yet decoded). Record 0 is empty.
Field widths of club and player records are inferred from the data, not
yet from code.

## `HIGH.XY` high-score files

`highscore_load_or_store(flag)` (`322b:21c4`):

* File name template `HIGH.00` at `DS:949c`. `X` (`DS:94a1`) =
  `'0'`, or `'1'` if `FAR:513c` is `0x7ac` or `0x7c9`, or `'3'` if it is
  `0x7ae` or `0x7cb`. `Y` (`DS:94a2`) = `'5' - DS:4a28` (the user level).
* `flag = 0`: open for reading; if it exists read `0x488` = **1160** bytes
  into `FAR:1d52`. If it does not exist: check that `mana.dat` exists
  (error screen otherwise), zero the table and create the file. `flag != 0`:
  write the 1160 bytes. No encryption, no checksum.

Layout: 20 entries of `0x3a` = 58 bytes:

| Offset | Size | Field |
|---|---|---|
| `+0x00` | 26 | manager name, NUL-terminated (compared with `FAR:4cfe`) |
| `+0x1a` | 26 | club name, NUL-terminated (compared with `FAR:4d18`) |
| `+0x34` | 2 | score, `i16` |
| `+0x36` | 4 | value copied from `DS:0668` when the entry is created |

`highscore_insert_entry` (`322b:2366`) sorts with an exchange sort
(linear `0x346f8`):
`for i in 0..19 { for j in i+1..20 { if score[j] > score[i] (signed) swap(i, j) } }`,
swapping through the 58-byte buffer `FAR:4cfe` via `movmem` (`35ac:304a`).
The candidate entry is prepared in `FAR:4cfe` (58 bytes). An existing entry
whose manager and club names both match is overwritten; otherwise entry 19 is
overwritten if its score is lower than the new score (`i16` at `FAR:4d32`). The overwrite copies the 58 bytes from
`FAR:4cfe` and then sets `+0x36` from `DS:0668` (linear `0x347ab..0x3485d`). Both sample files are stored sorted. Names may
contain stray control characters typed by the player (e.g. `\x08OLIVER`
in `HIGH.02`).

## Open points

* Semantic meaning of most fixed blocks (finances, calendar, league tables)
  still has to be traced through the game logic that reads them; the table
  above gives the exact storage, which is what a byte-exact reader/writer needs.
* `FAR:513c` holds `0x56ce` in every sample, but the high-score code
  compares it with year-like values (`0x7ac`, `0x7c9`, …); its purpose is unclear.
* Content of the V1 format beyond the header was not checked (no V1 sample).
