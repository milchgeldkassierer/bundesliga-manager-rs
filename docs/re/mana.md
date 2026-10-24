# MANA.DAT – club and player-name database

Status: layout fully derived from code (reader in `BMMAIN.EXE`, reader and writer
in `TEAMED.EXE`) and confirmed against both shipped copies (byte-exact round trip
in `crates/bmp-formats/src/mana.rs`).

## Where the addresses come from

`BMMAIN.EXE` and `TEAMED.EXE` are compressed with Microsoft **EXEPACK** (stub at
`CS:IP = 4bab:0010` in the packed BMMAIN header, signature `"RB"`, error string
`"Packed file is corrupt"`). The packed programs in the Ghidra project
(`/BMMAIN.EXE`, `/TEAMED.EXE`) contain mostly garbage code. All addresses below
refer to the unpacked images in the shared Ghidra project:

* `/unpacked/BMMAIN_UNPACKED.EXE` (data segment `DS = 5cb3`)
* `/unpacked/TEAMED_UNPACKED.EXE` (data segment `DS = 1cb5`)

A throwaway unpacker is in `out/mana/unexepack.py` (it produces images identical to
the `out/funcmap/*_UNPACKED.EXE` ones except for the MZ header).

C runtime calls seen below (BMMAIN): `4000:9323` = `open`, `4000:9388` = `read`,
`4000:930c` = `close`, `4000:ae28` = `strcpy`, `4a01:0e4e` = `strcmp`,
`4000:940c`/`9488` = far alloc/free, `1000:8377(lo, hi)` = random number in a range.
TEAMED: `1a46:0023` = `open`, `1a46:0088` = `read`, `1a46:00d6` = `write`,
`1a46:000c` = `close`.

## Encryption / obfuscation

**There is none.** Every block is read and written verbatim with plain
`read`/`write` calls. Names are plain upper-case CP437 text (umlauts as `0x8e`,
`0x99`, `0x9a`, `0xe1` = `ß`). The bytes at the start of the file
(`58 53 50 56 ...`) only look scrambled: they are picture numbers (see block 1).

The only transformation applied on load is BMMAIN mapping the crest byte to an
index: `index = (b + 0x30) & 0x7f` (8-bit wrapping add), which equals `b - 0x50`
for every shipped value.

## File references

| program | function (Ghidra name)            | address     | access |
|---------|-----------------------------------|-------------|--------|
| BMMAIN  | `mana_load_club_table`            | `362f:36ec` | read: crest, strength, club names (string `"mana.dat"` at `DS:54c0`) |
| BMMAIN  | `mana_load_player_names`          | `422b:035c` | read: whole club block, assigns player names (filename via far pointer `DS:93e8` -> `DS:93d6`) |
| BMMAIN  | `mana_find_player_name`           | `422b:01d0` | locate club block by name, return player slot pointer |
| BMMAIN  | `mana_pick_unused_player_name`    | `422b:022e` | pick a random unused name from a random club |
| TEAMED  | `teamed_init_and_load_mana`       | `1000:1439` (read at `1000:1762`) | read: all blocks |
| TEAMED  | `teamed_save_mana`                | `1579:0030` (write at `1579:04b5`) | write: all blocks |

BMMAIN never writes `MANA.DAT`. (`422b:21c4` also copies the `"mana.dat"` string
into a local buffer but only as part of building another file name; it does not
access this file's contents.)

## Layout

Total size: **38864 bytes** (0x97d0), fixed. Little endian is irrelevant: every
field is a byte or a byte string.

| offset  | size  | block | TEAMED buffer | content |
|---------|-------|-------|---------------|---------|
| `0x000` | 64    | 1 | `DS:5956` | crest picture number, 1 byte per German club |
| `0x040` | 600   | 2 | `DS:33ca` | strength, 3 bytes per club, 200 clubs |
| `0x298` | 128   | 3 | `DS:467c` | previous season points, 2 bytes per German club |
| `0x318` | 128   | 4 | `DS:364e` | previous season goals, 2 bytes per German club |
| `0x398` | 64    | 5 | `DS:1e04` | previous season final rank, 1 byte per German club |
| `0x3d8` | 37880 | 6 | `DS:0042` (names), far buffer `[5a02]` (players) | club records |

"German club" = club index 0..63; clubs 64..199 are foreign clubs (European cups).

### Block 6: club records

For `i` in `0..200`:

* 23 bytes: club name, NUL terminated (TEAMED keeps names in `DS:0042 + i*23`).
  Bytes after the NUL are **not** guaranteed to be zero: the shipped file contains
  `"SVW MANNHEIM\0" "98\0..."` for club 26 (left over from an earlier, longer
  name). A byte-exact writer must keep the raw field.
* only if `i < 64`: 20 player name slots of 26 bytes (`0x1a`) each, NUL
  terminated, zero padded. An empty slot starts with `0`. Longest shipped name is
  12 characters.

So a German club record is `23 + 20*26 = 543 = 0x21f` bytes, a foreign club
record 23 bytes: `64*543 + 136*23 = 34752 + 3128 = 37880`.

Players carry **only names** in this file. All player attributes (age, skill,
position...) are generated at random by `mana_load_player_names` (`422b:035c`,
e.g. `rand(0x12,0x21)` stored to player `+0x1a`, `rand(0x1e,0x59)` etc.).

### Block 1: crest picture number (`0x000`, 64 bytes)

Value `b` is the number of the club's crest picture `PIC/<b>.VGA`; the shipped
values are a permutation of 80..143 (`0x50..0x8f`).

* TEAMED passes the raw byte to its picture loader `1000:1136`, which formats it
  as a decimal file name (`1000:072e`).
* BMMAIN (`362f:3759`): `team[i].crest = (b + 0x30) & 0x7f` (team table in segment
  `[9f8e]`, base `0x3066`, stride `0x22`, field `+0x21`). Users of the field mask
  `& 0x7f` and add `0x50` again to get the picture (`2a58:05b6`, `422b:1daa`);
  bit 7 is a runtime flag (toggled at `3a41:0e72`, cleared at `1f9d:08d2`).
  Foreign clubs use picture `0x90 + x` from another table instead.

### Block 2: strength (`0x040`, 200 x 3 bytes)

Three bytes per club in club order. Range in the shipped data: 20..99.
BMMAIN (`362f:37e9`) copies byte `j` (j = 0..2) into both team field `+0x18+j`
and `+0x1b+j` (current and base value), and fills `+0x1e+j` with
`rand(0x2d, 0x37) - (i > 0x40 ? 5 : 0)` and `+0x17` with `rand(0x2d, 0x37)`
(note the original compares `i > 64`, so club 64 is not reduced). Those random
fields are not part of the file.

TEAMED recomputes the three bytes of clubs 0..63 on every save (`1579:0030`) from
blocks 3-5 (and random numbers), grouping clubs as
`group = (i > 17) + (i > 37)` (0 = Bundesliga 0..17, 1 = 2. Bundesliga 18..37,
2 = amateur 38..63). For group 2 and for clubs with rank 0 the values are purely
random, which is why the two shipped copies differ in exactly the strength bytes
of clubs 38..63 (`0x0b2..0x0ff`). The three values are written in the order
(points/rank term plus a goals-conceded term, points/rank term only, points/rank
term plus a term computed via the long-division helper `1b1f:1700`, presumably
from goals scored), so they are most likely defence, midfield, attack (not yet
confirmed from a BMMAIN consumer of team fields `+0x18..+0x1d`). Strengths of foreign clubs 64..199 are
written back unchanged.

### Block 3: points (`0x298`, 64 x 2 bytes)

`[plus, minus]` points of the previous season (2-point rule; e.g. club 0 has
48:20). Zero for clubs without a table entry. Only used by TEAMED (BMMAIN reads
this block into a buffer that is immediately overwritten).

### Block 4: goals (`0x318`, 64 x 2 bytes)

`[scored, conceded]` goals of the previous season (club 0: 72:45). TEAMED only.

### Block 5: rank (`0x398`, 64 bytes)

Previous season final table position: 1..18 for clubs 0..17, then
`1,2,2,3,3,4,5,...,18` for clubs 18..37 (the old two-group 2. Bundesliga),
0 for clubs 38..63. TEAMED only.

## How BMMAIN reads the file

`mana_load_club_table` (`362f:36ec`), called once from `18bc:0418`:

1. `open("mana.dat", O_RDONLY)`, read `0x40` bytes -> crest (see above), and
   `strcpy` a placeholder string (`DS:54c9`, `"@@@@@@@@..."`) into a 64-entry
   table at `0xed0 + i*0x36` (not file data).
2. read `0x258` bytes -> strength (200 x 3).
3. read `0x140` bytes (blocks 3-5) and discard them.
4. read `0x93f8` bytes (block 6), then for `i` in `0..200`:
   `strcpy(team[i].name /* +0x00 */, p); p += 0x17; if (i < 0x40) p += 0x208;`.

`mana_load_player_names` (`422b:035c`) reads `0x3d8` bytes (blocks 1-5, discarded)
and then `0x87c0` bytes (the 64 German club records) into a 40000 byte far buffer.
`mana_find_player_name(buf, club, slot)` walks the buffer in `0x21f` steps
(at most 64 clubs) comparing the record name with `team[club].name` and returns
`record + 0x17 + slot * 0x1a`. Player names for the human manager's team are
taken in slot order (empty slots get a random unused name via
`mana_pick_unused_player_name`, which retries until the chosen name is not yet
used by any of the 150 players `1..0x96`).

## How TEAMED writes the file

`teamed_save_mana` (`1579:0030`), after the strength recomputation:
`open("mana.dat", ...)`, then `write` 0x40 from `DS:5956`, 0x258 from `DS:33ca`,
0x80 from `DS:467c`, 0x80 from `DS:364e`, 0x40 from `DS:1e04`, then for
`i = 0..199`: `write(DS:0042 + i*0x17, 0x17)` and, if `i < 0x40`,
`write(players + i*0x208, 0x208)`. The reader at `1000:1762` mirrors this exactly.

## Copies in the original installation

`BMP/MANA.DAT` and `BMP/BMP/MANA.DAT` differ in 87 bytes: the strength bytes of
the amateur clubs 38..63 (re-randomized by TEAMED) and one player name at
`0x1eb5` (`"T.H\x84\xe1LER"` vs `"WESTERBEEK"`), i.e. one copy was re-saved
with the team editor.

## Rust

`bmp_formats::mana::ManaDat::parse` / `ManaDat::to_bytes` implement the reader and
the byte-exact writer; names are kept as raw fixed-size arrays so stale bytes
after the NUL survive a round trip.
