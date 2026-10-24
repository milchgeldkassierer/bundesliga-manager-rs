# BMMAIN.EXE memory map (issue #13)

This document gives the segment layout, the data-segment layout and the key globals of `BMMAIN.EXE`.
All addresses are Ghidra addresses in `/unpacked/BMMAIN_unpacked.EXE` (load segment + 0x1000). Read
FUNCTION_MAP.md §0 first: the shipped EXE is EXEPACKed, and the packed Ghidra program `/BMMAIN.EXE`
must not be used for addresses.

Every global was located with the Ghidra script `out/funcmap/Glob.java` (run it on `/unpacked/BMMAIN_unpacked.EXE`; it writes `out/funcmap/globals.tsv`, 9 989 rows). The first `globals.tsv` came from an unsaved earlier version of this script and is kept as `globals_v1.tsv`; `Glob.java` was rewritten to reproduce it and resolves more `ES:` accesses (1 329 instead of 2 001 rows with unknown segment). The script walks all
instructions, tracks the most recent `MOV ES,[DS:99xx]` selector load, and attributes each `[disp]` or
`ES:[disp]` operand to segment `5cb3` (DGROUP) or `5238` (far data). An offset is only meaningful
together with its segment, because both segments use overlapping offsets.

## 1. Load image

| Ghidra seg | File/load seg | Size | Kind |
|---|---|---|---|
| 1000 … 4930 | 0000 … 3930 | see FUNCTION_MAP.md §2 | game code (C, large model) and libraries |
| 4a01 | 3a01 | 0x36b7 | MS C runtime `_TEXT`; entry `4a01:0016` |
| 4d6c, 4dcb, 4e67 | 3d6c, 3dcb, 3e67 | | SB glue, CT-VOICE.DRV, SBFMDRV |
| 4fb2 | 3fb2 | 0x26f0 | `MSEM87` floating-point emulator |
| 5221 | 4221 | 0x0170 | small far data (zero) |
| **5238** | 4238 | **0xa7b0** | **far data segment** (`FAR_DATA`/`FAR_BSS`, zero in the file). Holds most game state |
| **5cb3** | 4cb3 | 0xa310 initialized + BSS | **DGROUP**: DS = SS |

The unpacked image is `0x56E40` bytes long and ends inside DGROUP at DS:a310. The EXEPACK header's
`dest_len` is `0x56e4` paragraphs.

### Stack and heap

`crt_start` sets `SS = DS = 5cb3` and `SP = 0x1000 + 0xA55E = 0xB55E`. The stack therefore occupies
**DS:A560-B55E** (4 KB). BSS spans **DS:A306-A55F** and is zeroed at startup. The stack limit used by
`_chkstk` is stored at `DS:954a`. The far heap (`_fmalloc`) takes DOS memory blocks above the program.
`mem_alloc` (4930:010c) records every block in `5238:0180[200]` (far pointers).

### Video memory

The screen runs in **Mode X 320x240**:
* each plane row is 80 bytes;
* page 0 is at A000:0000 and page 1 at A000:4B00 (`vga_show_page` 45ac:327d sets CRTC start address
  0 or 0x4B00);
* A000:9600 is a cached copy of the office background (`office_show_cached_background` 18bc:3b92).

The screen bitmap descriptor has width 0x140, height 0xF0 and data A000:0000 (`screen_open` 4930:0712).

## 2. DGROUP (segment 5cb3) layout

### 2.1 Overview by range

| DS range | Content |
|---|---|
| 0000-0041 | MSC null-pointer check area. `_nullcheck` XOR-sums 0x42 bytes and expects 0x55. DS:0008 = `"MS Run-Time Library - Copyright (c) 1990, Microsoft Corp"` |
| 00f6-0101 | 3 far pointers: ordinal suffixes `"st"`, `"nd"`, `"rd"` |
| 012a-0139 | font name table, 4 far pointers: `""`, `"flow.font"`, `"micro4.font"`, … (see 176b:0bdb) |
| 014c-0190 | digitized-sound state (§2.2) |
| 0178 | `"sound\digi.voc"` |
| 019b | `".vga"` |
| 01a0 / 01a9 | allowed filename punctuation and its replacement table (176b:0d56) |
| 01d5 | `"FONT-Error"` |
| 01fc | `"ERROR.MAN"` |
| 0206 | `"pic\1.pal"` |
| 021e | `"0"`: `argv[1]` value that disables sound |
| 0220 | `"sound\bm2title.cmf"` |
| 0238-0258 | match-day option texts (`"Spiele nicht"`, `"alleine !"`, `"weiter !"`, `"Spiele automatisch"`) |
| 026b | `"autosave.man"` |
| 06fc-0793 | 38 far pointers to 18-letter permutation strings (`"abcdefghijklmnopqr"`, `"ronklijghefcdabqpm"`, …): **fixture-generation permutations for 18-team leagues** (letters a-r = team slots). Used by `season_main_loop`, 3a41:10e3 and 422b:2366 |
| 07ab-07e2 | core season counters (§2.3) |
| 0f02… | assorted UI strings (`"KONTOSTAND"`, …) |
| 1d34 | 10-byte slot rows (`team_slot_free_index` 4091:0000) |
| 224c-2280 | season/league configuration (§2.3) |
| **22e0-2993** | **main UI string table: 429 far pointers** to German texts. Index *k* is at DS:22e0+4*k. It starts with `"DF0:"`, `"DF1:"` (Amiga drive names) |
| 4a14 | 5 far pointers (`" WOCHE(N)"`, …) |
| 4a2a | 7 far pointers (`"Spiel-Level"`, …), used by the start menu 18bc:2184 |
| **4a68-50a3** | **statistics/label string table: 399 far pointers** (`"G H A"`, `"MEISTERSCHAFTEN"`, `"DFB-POKALE"`, …) |
| 52d0 | far pointer to `"BM-Ed1.3-WK"` (editor/version signature) |
| 53d0 / 53d4 | special-date table: 3 day bytes and 3 month bytes (2a58:2a06: Christmas, developer birthdays) |
| 54c0 | `"mana.dat"` |
| **90d2-9289** | **newspaper/report template table: 110 far pointers** (`"EIN %x20LANGWEILIGES#M[SSIGES#..."`). Templates use `%x<nn>` selectors, `#` separated alternatives, and `%a`/`%b`/`%0`/`%1` substitutions (3e3a:0c0b, 422b:07fe) |
| 93e0 | far pointer to `"WeRnEr_krahe~&~JeNs_onnen"` |
| 93e8 | far pointer to `"mana.dat"` |
| 9440 | flag: when nonzero, save-file encryption is skipped (422b:0047/013f) |
| 9448 / 9471 | V1.x save-game texts (`"Sie haben einen V1.x-Spielstand geladen."`, level prompt 0-5) |
| 949c | `"HIGH.00"` |
| 94a4 | `"(X/X/X)"` |
| 94ad / 94b1 / 94bb | `"*.*"`, `".MAN"`, `"FILE"` |
| 94c0-94cc | Sound Blaster state (§2.2) |
| 94ce-9574 | MS C runtime data (§2.4) |
| 96ca-96e2 | `"TZ"` and timezone data |
| 9720-9722 | **`rand()` state (32-bit)** |
| 9724-97b0 | FP conversion scratch (`_fltout`) |
| 97e4-97f8 | `_cfltcvt_tab` (far pointers to 4a01:30c0, 2ba0, 2c4c, 2c1e, 3630) |
| 9806-981a | FP/emulator hooks (DS:9808 = 0xD6D6 when the FP library is present) |
| **9900-a146** | **far-data segment selector table**: about 0x420 words, each relocated to `5cb3` or `5238`. Code reads a selector with `MOV ES,[sel]` and then accesses `ES:[off]` |
| a156… | `_NMSG` table: runtime error messages (`R6000 stack overflow`, `R6001 null pointer assignment`, …) |
| a306-a55f | BSS (zeroed by startup): a53a `atof` result, a542 tzset-done flag, a544 `_fltout` pointer, a548 `_fltin` result |
| a560-b55e | stack |

### 2.2 Sound globals

| DS | Type | Meaning | Evidence |
|---|---|---|---|
| 014c/014e | word | EMS page frame segment | 176b:0006 |
| 0150 + 4*i | word,word | EMS handle and page count of digitized sample *i* (5 samples) | 176b:005f |
| 0164 / 016c | word[] | sample-number to slot maps | 176b:005f, 0182 |
| 018c | byte | sound enabled. Default 1; `argv[1]=="0"` clears it | main |
| 018d | byte | CT-VOICE plus EMS samples initialized | 176b:0006 |
| 018e | byte | sample currently playing | 176b:0182 |
| 018f | byte | looping crowd sample (sample 1) active | 176b:0182/020f |
| 0190 | word | saved EMS map handle (-1 = none) | 176b:0182 |
| 94c0 | byte | saved PIC mask during IRQ detection | 4d6c:00f0 |
| 94c2 | word | CT-VOICE status word (nonzero while playing) | 4d6c:03ab, 176b:024b |
| 94c4 | byte | SBFMDRV music status byte (nonzero while playing) | 4d6c:049c/0580 |
| 94c6 | word | Sound Blaster base port (0x210-0x260) | 4d6c:01fc |
| 94c8 | word | Sound Blaster IRQ | 4d6c:00f0 |
| 94ca/94cc | word | OPL write delay loop counts | 4d6c:02e5 |

### 2.3 Season, league and UI globals in DGROUP

| DS | Type | Meaning (confidence) |
|---|---|---|
| 016e | byte | current week/round index into the per-week match-type table DS:2280 (medium) |
| 05ac | byte | copy protection passed. If 0, `copy_protection_penalty` may hang the game (medium) |
| 05d4 | byte | abort/quit-requested flag checked by all wait loops (medium) |
| 060c | byte | palette-fade mode for `screen_fade` (medium) |
| 079c | byte | "picture loading in progress" flag set by `vga_load_picture` (medium) |
| 079e+2*i | byte pair | per-manager value copied at season end (low) |
| **07ab** | byte | **number of human managers** (loop bound everywhere) (high) |
| 07ac-07af | byte | league sizes / cup round bounds used for table logging (low) |
| 07b2 | byte | message window open flag (low) |
| 07b3 | byte | count of registered icon slots (max 10) (high) |
| **07dc/07de** | long | **day counter** of the season loop (high) |
| 07e2 | word | number of logged rounds (capped at 0x31) (medium) |
| 0210 | byte | office background cached in VGA page 2 (high) |
| 224c | char | season start-day offset (season length check `07dc <= 224c+0x142`) (medium) |
| 224d | char | used in season-loop threshold `224d*7+0x82` (low) |
| 224e/2250 | long | difficulty level. The value 4 is special-cased in 176b:0681 and 4091:0ba5 (medium) |
| 2252/2254 | long | second game-option long (low) |
| 225a[3] | byte | per-competition round counters (medium) |
| 225e/225f, 2262[] | byte | team counts / offsets per league tier (3a41:10e3) (medium) |
| 226e[], 226f, 2270 | byte | league tier boundaries (`league_of_rank` 27c2:1bb5 returns 1, 2 or 4) (high) |
| 2280[] | byte | per-week bitmask of competitions (bits 0-2 increment `225a[]`). The value 9 marks a special week (medium) |
| 4f64-4f7a | words | UI coordinates/state of current screen (low) |

### 2.4 MS C runtime globals

| DS | Meaning |
|---|---|
| 94ce-94d4 | startup stack bookkeeping (`_atopsp` etc.) |
| 94e8/94ea | heap: DOS block size limits |
| 94ec | `"_C_FILE_INFO="` |
| 94fa/94fc | saved INT 0 vector |
| 9508 | `errno` |
| 950e | PSP segment (`_psp`) |
| 9510 | DOS version (`_osversion`) |
| 9513 | `_doserrno` |
| 9515 | `_nfile` |
| 9517[] | `_osfile[]` flags (0x80 open, 0x40 device, 0x20 append, 0x02 EOF, …) |
| 952b/952d/9531 | `__argc`, `__argv`, `environ` |
| 9535/9537 | environment pointers |
| 9542/9544 | message banner hook |
| 9546 | stack-overflow handler (-1 = default) |
| 954a | stack limit (`_chkstk`) |
| 954c/954e | return address saved by `_setargv` |
| 9550[] | DOS-error to errno map |
| 9565 | `_fmode` high byte (text/binary default) |
| 9566-9570 | far heap rover and segment list |
| 9574 | `_amblksiz` |
| 9720/9722 | `rand()` seed/state |

## 3. Far data segment 5238

Segment 5238 is zero in the EXE and is filled at run time from MANA.DAT, save games and game logic.
Offsets below are `5238:off`. They were reached through selector-table entries that point to `5238`;
see `globals.tsv`.

### 3.1 Engine and UI state

| 5238: | Type | Meaning (confidence) |
|---|---|---|
| 0010 + 4*n | far ptr[4] | font handles 0..3 (`ui_set_font` 176b:0c91) (high) |
| 0180 + 4*i | far ptr[200] | allocation table of `mem_alloc`/`mem_free` (high) |
| 2e70/2e72 | far ptr | pointer/viewport state struct: `+4` = x, `+6` = y of the mouse in `ui_hit_test_buttons` (medium) |
| 2e74 | byte | length of team list 5238:56fe (high) |
| **2e8e/2e90** | long | **current month** (0-based), compared with the special-date table (medium) |
| **2eba/2ebc** | long | **current day of month** (medium) |
| **2ea2** | word | last mouse event: 1 = left click, 2 = right click, 0 = none (`input_poll_mouse`) (high) |
| 2f24/2f26 | far ptr | current picture bitmap (main) (medium) |
| **2f28/2f2a** | far ptr | **main screen rastport**, the first argument of almost every draw call (about 800 uses) (high) |
| 2f3c/2f3e | far ptr | mouse pointer bitmap (main) (medium) |
| 3046/3048 | word | "button held" latch used by wait loops (high) |
| **304a** | byte | **index of the manager whose turn it is** (474 uses) (high) |
| 307e/3081/3084 + 0x22*t | byte[3] | lineup/strength components per team slot (`team_strength_value`) (medium) |
| 4b50/4b52 | far ptr | current palette (`palette_load_rgb4`, `screen_fade`) (medium) |
| 4bce | byte | copy of month at loop start (low) |
| 5358 | byte | saved manager index (low) |
| 53a0 + 12*i | struct[20] | registered UI buttons (`ui_draw_button`) (medium) |
| 548e + 4*i | far ptr[2] | button frame bitmaps loaded in main (pictures 0x18/0x19) (medium) |
| 5524 + 4*(m*0x14+i) | far ptr | per-manager message strings (20 per manager) (medium) |
| 56dc | byte | save-file checksum accumulator (owned by the MAN format agent) (high) |
| 56fa/56fc | far ptr | 320x240 work bitmap (main) (high) |
| 56fe[] | byte | team id list (`team_is_in_list` 27c2:11e7) (high) |
| 57bc | far ptr | work bitmap handle (main) (high) |
| 57cf | byte | save-file encryption rolling key (key -= 0x29 per byte) (MAN agent) (high) |
| 578c + 4*i | far ptr[2] | bitmaps loaded in main (medium) |
| 6db8, 6dc0/6dc2 | far ptr | pointer sprite bitmap and mouse position struct (`[0]` = x, `[1]` = y; 4091:0aaa) (medium) |
| a6ca/a6cc, a7a0/a7a2 | far ptr | further screen bitmaps (low) |

### 3.2 Game-state tables (for the logic agents)

| Base (5238) | Stride | Count | Content (confidence) |
|---|---|---|---|
| **2260 + 0x30a*m** | 0x30a | `DS:07ab` managers | **human manager record**. `+0x00` team id (`manager_index_of_team`). `+0x0ec+DS:2262[+0x11a]` league position byte (read in the season loop). `+0x1b`/`+0x1c` season counters. `+0x20`-`+0xe3` 4-byte per-round result log (0x31 entries, shifted down when full). `+0x114`-`+0x117` cup/competition stage bytes. `+0x11a`-`+0x11d` league/cup state. `+0x140` stadium data (`screen_stadium` uses `+0x23a0`). `+0x186`-`+0x1a2` season statistics words. `+0x1c6`-`+0x1d0` records (initialized to 99999). `+0x1de` finance entries `5 x 3 x 0x12` (`finance_sum_entries`) (medium) |
| 0ecc + 0x36*t | 0x36 | teams | per-team match statistics bytes (wins/draws/goals at +0x00, +0x16, +0x1a, +0x1e, +0x26, +0x2a) accumulated into the manager record (medium) |
| **0x25*t + 57f7..5801** (`t` = team id) | 0x25 | teams | **team records** from MANA.DAT. Fields are accessed as `0x25*t + 0x57fe` (owning manager index, compared with 304a), `+0x5801`, `+0x57f7`, `+0x57f9..57fc`. The record base offset is not yet pinned down (medium; the MANA agent owns the format) |
| **774a + 0x34*(m*0x19+i)** | 0x34 | 25 per squad | **player records** of a squad: `+0x0a` (7754) status/injury (<0x0c counted). `+0x0d` (7757) suspension flag. `+0x0f` (7759) player id. `+0x19`-`+0x1a` (7763/7764) attributes. `+0x28` (7772) word. `+0x30/+0x32` (777a/777c) far pointer to a message. Squads 0-3 belong to human managers; 0x7e9+ is the transfer pool (`find_player_by_id`) (medium) |
| 23e6 + 2*(k + m*0x185) | words | | league table accumulators (games, goals, points…) updated each round in `season_main_loop` (medium) |
| 243e + 0x12*(c*3+j) + 0x30a*m | 0x12 | 5x3 | finance ledger entries (income/expense categories) (medium) |

## 4. Interrupts, ports and hardware

| Resource | User |
|---|---|
| INT 21h | CRT file I/O and memory; 45ac:326b/3272 keyboard (fn 07/06); 45ac:3690 delete file; 45ac:337e set vector |
| INT 33h | mouse: 45ac:1b08 install (reset, ranges 320x240, event handler), 1971 set position, 1c4b remove |
| INT 10h | 45ac:1c80 mode 13h → Mode X; 2f93/2f9b screen off/on (AH=12h BL=36h); 2fa3/33ca DAC (AH=10h) |
| INT 67h | EMS (45ac:36c7-3820), holds `DIGI.VOC` samples |
| Ports 3C4/3C5, 3CE/3CF, 3D4/3D5, 3C2, 3DA | Mode X plane masks, read map, CRTC page flip, misc output, retrace wait |
| Ports SB base+6/+A/+C/+E, base+8/+9 | DSP reset/read/write/status, OPL2 |
| Ports 20h/21h, 40h/43h | PIC mask during IRQ probe, PIT channel 0 calibration |
| BIOS 0040:006C | tick counter for `timer_set_deadline` / `delay_ticks` |

## 5. Files the program touches

| String (DS) | File | Function |
|---|---|---|
| 0206 | `PIC\1.PAL` | 18bc:0004 |
| `pic\<n>` + 019b | `PIC\<n>.VGA` | 176b:0356 |
| 0178 | `SOUND\DIGI.VOC` | 176b:005f |
| 0220 | `SOUND\BM2TITLE.CMF` | main |
| 54c0 | `MANA.DAT` | 362f:36ec |
| 52fa / 5306 | `TORE\ANZAHL`, `TORE\…` | 24a4:0419/05ec |
| 026b | `AUTOSAVE.MAN` | 18bc:0b5f |
| 01fc | `ERROR.MAN` | 176b:114c |
| 94b1 | `*.MAN` | 422b:07fe/120c/2bbe |
| 949c | `HIGH.00` | 422b:21c4 |
