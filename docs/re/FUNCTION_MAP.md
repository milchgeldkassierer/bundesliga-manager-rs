# BMMAIN.EXE function map (issue #13)

Reverse-engineering map of every function Ghidra found in `BMMAIN.EXE`, the main program of
*Bundesliga Manager Professional* (Software 2000, 1993). The same section also covers
`BMINTRO.EXE`, `BMLOADER.EXE` and `TEAMED.EXE` briefly. The memory and data-segment layout is in
[MEMORY_MAP.md](MEMORY_MAP.md).

All addresses are Ghidra `segment:offset` addresses in the **unpacked** program (see the next section).
Ghidra loads the image at paragraph `0x1000`, so a Ghidra segment `S` is file/load segment `S - 0x1000`.

## 0. Read this first: the executables are EXEPACKed

`BMMAIN.EXE`, `BMLOADER.EXE` and `TEAMED.EXE` are compressed with Microsoft **EXEPACK**. All three have
0 MZ relocations, `CS:IP = xxxx:0010/0012`, the `"RB"` signature and the string `"Packed file is corrupt"`.
`BMINTRO.EXE` is not packed: it has 7 relocations and `CS:IP = 0000:0000`.

The original Ghidra programs `/BMMAIN.EXE`, `/BMLOADER.EXE` and `/TEAMED.EXE` are therefore analyses of
the **packed** image:

* The first `0x35AD2` bytes of the BMMAIN image are byte-identical to the unpacked image. EXEPACK only
  rewrites the tail, so code there decompiles. Everything after that point is shifted or RLE-encoded.
* No relocations are applied, so every far call or far pointer has the wrong segment. Ghidra lays the
  image out as one flat `1000:0000-5000:baaf` block, and its "600 functions" mix real and bogus entries.
* Addresses in the packed program are therefore not comparable with the addresses below.

**Use the unpacked programs in the Ghidra project folder `/unpacked/`:**

| Ghidra program | Source file (gitignored, regenerable) | Notes |
|---|---|---|
| `/unpacked/BMMAIN_unpacked.EXE` | `out/mana/BMMAIN_unpacked.EXE` (imported by the MANA agent) | **canonical**, 540 functions; all renames from this issue are applied here. Two relocation bytes were patched in Ghidra (see below) |
| `/unpacked/TEAMED_unpacked.EXE` | `out/mana/TEAMED_unpacked.EXE` | canonical; same two-byte patch |
| `/unpacked/BMLOADER_UNPACKED.EXE` | `out/funcmap/BMLOADER_UNPACKED.EXE` | only unpacked copy of BMLOADER; relocations correct |
| `/unpacked/funcmap_duplicates/BMMAIN_funcmap_dup.EXE`, `.../TEAMED_funcmap_dup.EXE` | `out/funcmap/BMMAIN_UNPACKED.EXE`, `out/funcmap/TEAMED_UNPACKED.EXE` | earlier duplicate imports, renamed and moved out of `/unpacked/` so that name lookups are unambiguous; no renames; do not use for annotation |

**Relocation defect in the `out/mana/*_unpacked.EXE` files.** The MANA agent's unpacker
(`out/mana/unexepack.py`) writes the MZ header as 28 bytes plus 4 zero bytes and puts the relocation
table at file offset `0x20`, but it stores `0x1C` in the header's relocation-table-offset field. Every
loader, Ghidra included, therefore reads the table 4 bytes too early: the first entry it sees is the
4 zero bytes (`0000:0000`), every real entry is shifted by one, and the **last** real entry is lost.
The load images themselves are byte-identical to `out/funcmap/*_UNPACKED.EXE`. Effects in Ghidra:

| Program | Bogus relocation at `0000:0000` | Lost relocation (file seg:off) | Ghidra address of the lost one |
|---|---|---|---|
| BMMAIN | `1000:0000` became `55 9b ec` instead of `55 8b ec` (`PUSH BP; MOV BP,SP`, first instruction of `stadium_expansion_order`) | `5000:6356` | `5cb3:9826` (segment half of the far pointer at DS:9824; `0x3a01` instead of `0x4a01`, i.e. CRT segment `4a01`) |
| TEAMED | `1000:0000` same corruption | `1000:2d7c` | `2256:081c` (`2000:2d7c`; word `0x1256` instead of `0x2256`) |

Both canonical Ghidra programs were patched in place (two bytes each, EOL comments at the four
addresses), and the instruction at `1000:0000` was re-disassembled. After the patch, a byte dump of the
whole 355 904-byte BMMAIN image (and the 77 648-byte TEAMED image) from Ghidra is identical to the
`out/funcmap/*_UNPACKED.EXE` image with all relocations applied at load segment `0x1000`. The files
under `out/mana/` still carry the defect, so anyone who re-imports them must re-apply this fix or import
`out/funcmap/*_UNPACKED.EXE` instead. The header of `out/funcmap/unexepack.py` output has the table offset
and the table position in agreement (`0x1C`).

The unpacker is `out/funcmap/unexepack.py`. Its algorithm:

1. The EXEPACK header sits at `CS:0000`: `real_IP, real_CS, mem_start, exepack_size, real_SP, real_SS,
   dest_len` (paragraphs), then `"RB"`.
2. The packed image is the bytes before `CS:0000`. The unpacker skips trailing `0xFF` padding and then
   decodes **backwards**. Each command is an opcode byte read backwards, followed by a 16-bit count:
   * `0xB0`/`0xB1`: fill. The next byte back is the fill value, written `count` times.
   * `0xB2`/`0xB3`: copy `count` bytes.
   * Bit 0 marks the last command.
3. The relocation table follows `"Packed file is corrupt"`. It has 16 sections, and section *i* is a
   count word followed by that many offset words, all with segment `i*0x1000`.

| EXE | unpacked size | relocs | real CS:IP | real SS:SP | Ghidra CS:IP |
|---|---|---|---|---|---|
| BMMAIN | 355 904 | 6 946 | 3a01:0016 | 5709:1000 | 4a01:0016 |
| BMLOADER | 33 776 | 305 | 0522:0014 | 0840:0800 | 1522:0014 |
| TEAMED | 77 648 | 910 | 0b1f:0012 | 12f7:0800 | 1b1f:0012 |

## 1. Toolchain and program model

* The compiler is **Microsoft C 6.x/7.x, large memory model**. The DGROUP banner reads
  `"MS Run-Time Library - Copyright (c) 1990, Microsoft Corp"`. Nearly every C function starts with
  `_chkstk` (`4a01:02c6`, about 320 callers), uses far calls, and returns with `RETF`. Data is far by
  default.
* Far data is reached through a **segment-selector table at DS:9900-A146**. The compiler emits
  `MOV ES,[DS:99xx]` followed by `ES:[offset]`. The selectors point either to DGROUP `5cb3` or to the far
  data segment `5238` (see MEMORY_MAP.md §3). The decompiler shows such a load as
  `uVar = *(undefined2*)0x99xx`; the following "global" is in the selected segment, not in DS.
* Long arithmetic goes through the MSC helpers `_aFlmul`, `_aFldiv`, `_aFlrem`, `_aFuldiv` and
  `_aFulrem` (`4a01:1a4c..1c70`). Ports must reproduce their truncating semantics.
* Floating point uses the MS FP emulator: INT 34h-3Dh opcodes in code and the `MSEM87` segment `4fb2`.
* The game is an **Amiga port**. The string table starts with `"DF0:"` and `"DF1:"`, the graphics
  library uses rastports and bitmaps, and palettes are stored as 12-bit `0x0RGB` words.
* Screen mode: **320x240 VGA Mode X**, not mode 13h. `vga_set_mode_x_320x240` (`45ac:1c80`) sets mode 13h,
  unchains it, writes `0xE3` to the misc output register and loads the CRTC table at `45ac:039a`
  (vertical display end `0x1DF` = 480 lines, double scan, so 240 lines). Each row is 80 bytes per plane,
  page 0 is at A000:0000 and page 1 at A000:4B00. `screen_open` (`4930:0712`) creates the 320x240 screen
  bitmap, and `main` sets the work bitmap to `0x140 x 0xF0`.

## 2. Segment map of BMMAIN (unpacked, Ghidra addresses)

| Ghidra seg | size | functions | content | category |
|---|---|---|---|---|
| 1000 | 0x3109 | 10 | stadium expansion, sponsor, credit offer, fixture/result list | game (finance) |
| 1310 | 0x3b68 | 38 | match-day presentation: competition headers, live ticker, penalty shoot-out, team strength | game (match) |
| 16c7 | 0x0a45 | 10 | UI primitives: buttons, frames, text, hit test | UI |
| 176b | 0x150e | 25 | services: digitized sound (CT-VOICE+EMS), mouse polling, RNG `random_range`, number formatting, `.VGA` loader, fonts, fatal exit, fades | UI/system |
| 18bc | 0x3bff | 17 | `main`, start menu, office (manager turn) dispatcher, match-day dialog | UI/game |
| 1c7c | 0x038d | 1 | goal-scene sound sync | sound |
| 1cb5 | 0x2e81 | 9 | season-end messages, random weekly events | game (season) |
| 1f9d | 0x18d9 | 6 | weekly manager/AI processing, match engine helpers | game |
| 212a | 0x37a0 | 12 | finances: weekly processing, loans, training camp, training | game (finance) |
| 24a4 | 0x31db | 24 | player info, top scorers, **TORE** goal-scene playback | game/format |
| 27c2 | 0x2966 | 16 | cups (DFB-Pokal draw/rounds), relegation, championship | game (season) |
| 2a58 | 0x4752 | 12 | **season main loop**, match ticker events, special dates, season end | game |
| 2ecd | 0x3a9d | 19 | squad, lineup, auto lineup, transfer list | game (squad) |
| 3277 | 0x3b7a | 11 | transfer market, offers, contract negotiation | game (transfer) |
| 362f | 0x412a | 16 | settings, statistics, eternal table, history, contract, **MANA.DAT** load | game/format |
| 3a41 | 0x3f8f | 13 | opponent info, fixtures, league table, strength table | game (stats) |
| 3e3a | 0x2563 | 14 | newspaper/report text generator, misc | game |
| 4091 | 0x19a5 | 20 | small game helpers (clamp, upper-case, messages, icon slots) | game/UI |
| 422b | 0x3813 | 13 | save/load (`*.MAN`, encrypted), `HIGH.00`, file selector, disk errors | file I/O / format |
| 45ac | 0x3843 | 53 | **assembly graphics/mouse/EMS library** (Mode X blits, lines, text, pointer, timer, EMS). `45ac:0000-169f` is library data (CRTC table, fonts registry, umlaut map, `"wbold.font"`) | gfx/input |
| 4930 | 0x0d0b | 36 | C wrapper layer over 45ac/CRT: file, memory table, rastport, palette, screen, mouse | gfx/file |
| 4a01 | 0x36b7 | 123 | **MS C runtime** (startup, file I/O, heap, strings, long math, time, FP formatting) | CRT |
| 4d6c | 0x05e3 | 32 | Sound Blaster glue (asm): DSP/OPL I/O, port/IRQ detection, CT-VOICE and SBFMDRV call wrappers | sound |
| 4dcb | 0x09c0 | 4 | **embedded CT-VOICE.DRV** ("Copyright (c) Creative Technology Pte Ltd., 1989-1990") | Creative driver |
| 4e67 | 0x14b0 | 6 | **embedded SBFMDRV** ("FMDRV" signature) | Creative driver |
| 4fb2 | 0x26f0 | 0 | `MSEM87` Microsoft 8087 emulator (no functions recovered) | CRT |
| 5221 | 0x0170 | 0 | small far data (zeros) | data |
| 5238 | 0xa7b0 | 0 | **far data segment** (zero-initialised game state) | data |
| 5cb3 | 0xa310 + BSS | 0 | **DGROUP** (DS=SS): strings, tables, CRT data; BSS and stack follow | data |

Function count by category (the table below has all 540 entries):
crt 123, gfx 64, sound 50, game-match 52, game-stats 45, UI 43, game-season 32, game-misc 27,
game-squad 23, game-finance 20, file I/O 17, format-loader 16, input 14, game-transfer 14.

## 3. Program flow

### 3.1 Startup: `crt_start` 4a01:0016 → `main` 18bc:0418

`crt_start` runs the standard MSC C0 sequence: it requires DOS ≥ 2, switches to SS=DS=`5cb3`
(SP=0xB55E), shrinks the memory block, zeroes BSS `DS:A306-A55F`, runs the initializers and calls
`main(argc, argv, envp)`.

`main` then does the following:
1. If `argv[1]` equals `"0"` (DS:021e), it clears the sound flag `DS:018c` (`BMP.BAT` passes `%1` through).
2. It runs `startup_load_palette_and_init` (18bc:0004), which loads `PIC\1.PAL` and initializes screen,
   mouse and fonts. It then fades in and allocates the 320x240 work bitmap (`5238:56fa`).
3. If sound is enabled: `sb_detect_port` → `fmdrv_init`, then read `SOUND\BM2TITLE.CMF` and start it
   (driver clock from the CMF header, instruments, play). It waits for a click or the end of music.
4. It runs `sound_voice_init` and `sound_load_digi_voc` (CT-VOICE plus `SOUND\DIGI.VOC` in EMS), then
   `mana_load_club_table` (362f:36ec, MANA.DAT loader; name set by the MANA agent).
5. It loads common pictures (`vga_load_picture` 0x18/0x19 and others), seeds the RNG
   (`random_seed_from_time` → `srand(time())`) and initializes the competitions (`27c2:09e0` with 0..3).
6. `start_menu_new_or_load` (18bc:2184) returns 0 for a new game. For a new game, main initializes the
   league and transfer market. Either way it calls `18bc:034d(1|2)` and `24a4:0419`
   (`tore_scene_open`, which opens `TORE\ANZAHL`).
7. It calls **`season_main_loop` (2a58:3176)**, which never returns normally.
8. It calls `fatal_error_exit(99)`. Code 99 means normal termination; codes 2 and 4 are file and memory
   errors.

### 3.2 Main loop: `season_main_loop` 2a58:3176

This is the day loop. Its key state:

| Variable | Meaning |
|---|---|
| `DS:07dc` (long) | running day counter |
| `FAR:016e` | season week index into `DS:2280` (match-type bitmask per week); savegame block 49 |
| `DS:07ab` | number of human managers |
| `5238:304a` | current manager index |

For each day, the loop does the following:
1. `matchday_refresh` 1310:1190.
2. Per manager: `finance_weekly_processing` 212a:0a6d.
3. `league_init_or_ai` 1f9d:0697.
4. Special-date check: `special_date_events` 2a58:2a06 (Christmas, developer birthday) via table
   `DS:53d0`, compared with `5238:2eba` (day) and `5238:2e8e` (month).
5. Per manager:
   * `random_weekly_events` 1cb5:13bd;
   * every 14 days `24a4:2cb4`;
   * each manager's office turn `office_menu_dispatcher` 18bc:0b5f;
   * `squad_weekly_update` 2ecd:3360 and `manager_ai_weekly` 1f9d:0002;
   * match days and league table updates (accumulators at `5238:23e6+`, manager record stride `0x30a`).
6. "WINTERPAUSE" and `season_end` 2a58:4597 when the day counter passes `DS:224c + 0x142` (322 days).

### 3.3 Screen and menu dispatcher: `office_menu_dispatcher` 18bc:0b5f

This function is each manager's office turn. It draws the office (`office_show_cached_background`
18bc:3b92 keeps the background in VGA page 2 at A000:9600), shows the competition of the day
(`"Spieltag"`, `"DFB-Pokal"`, `"Europapokal"`, `"Relegation"`, `"Nachholspiel"`, `"Spielfrei"`,
`"SAISONENDE"`) and handles clicks. It writes `AUTOSAVE.MAN`.

The icon menus are `switch` statements whose jump tables Ghidra did **not** follow. The case bodies are
undisassembled bytes, which is why the screen functions below show 0 callers in the table:

| Jump table | case 1 | case 2 | case 3 | case 4 | case 5 | case 6 |
|---|---|---|---|---|---|---|
| `18bc:1a15` (menu 1, `JMP CS:[BX+1a15]` at 18bc:1a10) | 362f:0af7 `screen_statistics` | 362f:18e7 `screen_eternal_table` | 362f:390e `screen_manager_history` | 1000:0602 `screen_stadium` | 212a:167f `screen_credits` | 362f:2be8 `screen_manager_contract` |
| `18bc:1b42` (menu 2, at 18bc:1b3d) | 2ecd:1560 *screen_squad_lineup*`(0, 0xff)` | 2ecd:1560 *screen_squad_lineup*`(0x63, 0xff)` | 3277:04a5 `screen_transfer_market` | 212a:274c `screen_training` | 212a:0145 `screen_training_camp` | 24a4:1ad5 `screen_top_scorers` |

Other direct cases in the same function call 3a41:214c (`screen_league_table`), 3a41:120a
(`screen_fixtures`) and 3a41:3891 (`screen_strength_table`). The script `out/funcmap/jt.py <seg> <lo> <hi>`
decodes `2E FF A7` jump tables from the raw image and follows `JMP` (EB/E9) hops inside a case body up to the first `CALLF`. Menu 2 cases 1 and 2 share one call site: case 1 (18bc:1af5) is `MOV AL,0xFF; PUSH AX; SUB AL,AL; JMP 1b01`, case 2 (18bc:1afc) is `MOV AL,0xFF; PUSH AX; MOV AL,0x63`, and both continue at 18bc:1b01 `PUSH AX; CALLF 2ecd:1560`, so case 1 calls `2ecd:1560(0, 0xFF)` and case 2 calls `2ecd:1560(0x63, 0xFF)` (cdecl, the last push is the first argument).

### 3.4 Randomness

All game randomness goes through `random_range(lo,hi)` 176b:0cc7, which returns
`rand() % (hi-lo+1) + lo`. `rand` (4a01:17be) is the MSC LCG: `state = state*0x343FD + 0x269EC3`
(32-bit, `DS:9720/9722`), returning `(state>>16) & 0x7FFF`. `srand` (4a01:17ac) is seeded once with
`time()`. Bit-exact ports must replicate both the call order and this LCG.

### 3.5 Copy protection

`copy_protection_penalty` 16c7:09f9 runs whenever `DS:05ac == 0` (protection not passed). It has a
1-in-101 chance per call to hang the program forever. `DS:93e0` contains `"WeRnEr_krahe~&~JeNs_onnen"`.

## 4. Other executables

### BMINTRO.EXE (not packed, 11 functions, no CRT, hand-written assembly)

`BMP.BAT` runs `bmintro` and then `bmmain %1`. BMINTRO is the animated Software 2000 logo intro;
the full description is in [logic/intro.md](logic/intro.md). Nothing starts `BMLOADER.EXE`.

| Ghidra addr | Purpose |
|---|---|
| 1000:0000 `entry` | Detect the adapter, set the mode, run animation frames on vertical retrace until done or a key is pressed (INT 21h), restore the text mode, exit |
| 1000:0b8c | Adapter detection: INT10/1A00 returns 0x1A → VGA (`DS:001b=0x0F`); INT10/12 BL=10 → EGA (`0xFF`); otherwise Hercules (`0`) |
| 1000:0ad5 | Set the graphics mode: VGA via INT10 + palette (1000:0aba); EGA via INT10 and GC write mode 2; Hercules via ports 3BF/3B8/3B4 with a 12-register CRTC table at `DS:0035` |
| 1000:0b4a | Restore the text mode (Hercules CRTC table `DS:0041`) |
| 1000:0bbb | Wait for vertical retrace (3DA bit 3, or 3BA bit 7 on Hercules) |
| 1000:0087 | Reveal step: every 3rd iteration advance the step, then draw each of the 0xD6 cell records at `DS:0ac2` (start step, cell index, type, stage) that is due, in colour stage + 2 |
| 1000:01a7 / 1000:0345 | Draw an 8x8 logo cell, masked or solid, for each adapter: VGA linear 320 px, EGA planar 80-byte rows, Hercules 4-bank interleave |
| 1000:0484 | Second phase: one line of the gradient box per iteration (line table `DS:0305`, colours at `DS:0585`, edge masks at `DS:0aa2`) |
| 1000:05c2 | Marquee: shift the interlaced "SOFTWARE 2000" rows 193..199 by one pixel and feed the next bits from `DS:12da` |

### BMLOADER.EXE (EXEPACKed, 178 functions after unpacking)

BMLOADER is a stand-alone title/loader program and is **not** referenced by `BMP.BAT`. Its library code
is the same as BMMAIN's, at different addresses:

| Library | BMLOADER segment | BMMAIN equivalent |
|---|---|---|
| graphics | 10d5 | 45ac |
| wrappers | 1452 | 4930 |
| CRT | 1522 | 4a01 |
| SB glue | 1642 | 4d6c |
| SBFMDRV | 1683 | 4e67 |
| DGROUP | 1806 | 5cb3 |

`main` (called from `entry` 1522:0014):
1. Shows `PIC\49.VGA`, waits 200 ticks, shows `PIC\48.VGA` (palettes from `DS:0340`).
2. Plays `sound\bm2title.cmf` if the sound check passes (argv[1] compared with `DS:0177`).
3. Waits for a click and exits 99.

Its error strings are `LIB-Error`, `GRAFIK-Error`, `DISK-Error`, `FONT-Error`, `SPEICHER-Error` and
`BildLade-Error`.

### TEAMED.EXE (team editor, EXEPACKed, 214 functions)

| Segments | Content |
|---|---|
| 1000, 12a5, 1579 | Editor logic |
| 16b7 | Graphics library (= BMMAIN 45ac) |
| 1a46 | Wrappers (= 4930) |
| 1b1f | CRT (= 4a01); entry 1b1f:0012 |
| 1cb5 | Far data, 0x5a10 bytes |
| 2256 | DGROUP |

TEAMED edits `MANA.DAT` (team names, league placement `BUNDESLIGA` / `2.BUNDESLI.` / `REGIONALLIGA` /
`OBERLIGA`, swaps). It imports club crests ("Bitte waehlen Sie den Pfad aus, in dem sich die Wappen
befinden", format check `"X<=320,Y<=200,256FARBEN"`) and contains the UEFA-cup difficulty guard
`"UEFA-CUP SONST ZU LEICHT"`.

## 5. Conventions in the table

| Column | Meaning |
|---|---|
| Address | Ghidra address in `/unpacked/BMMAIN_unpacked.EXE` |
| Name | Name set in Ghidra (all `crt`, `sound`, `gfx`, `input`, `fileio` and UI-primitive entries marked high/medium, plus `main`, `season_main_loop`, `office_menu_dispatcher` and some services). A proposed name not applied in Ghidra is shown in *italics*. Raw `FUN_` names are unclassified helpers |
| Category | `crt`, `sound` (incl. Creative drivers), `gfx`, `input`, `fileio`, `ui`, `format-loader` (owned by the format agents, deliberately **not** renamed), `game-season`, `game-match`, `game-finance`, `game-transfer`, `game-squad`, `game-stats`, `game-misc` |
| Confidence | **high**: behaviour verified in decompilation. **medium**: inferred from code and strings. **low**: inferred from strings, callers or segment only |

Texts in quotes are the German in-game strings the function references, reproduced verbatim
(umlauts as stored by the game: `[`=Ä, `\`=Ö, `]`=Ü, `{`=ä, `|`=ö, `}`=ü, `~`=ß).

## 6. Function table (BMMAIN, 540 functions)

| Address | Name | Category | Purpose | Conf. | Size | Callers |
|---|---|---|---|---|---|---|
| 1000:0000 | *stadium_expansion_order* | game-finance | stadium expansion order dialog ("AUSBAU UM ... AUF", cost, build time, "NA KLAR!/ACH NEE") | medium | 1524 | 2 |
| 1000:0602 | *screen_stadium* | game-finance | office menu 1/case 4: Stadion screen (capacity/comfort/floodlights/scoreboard, prices per 1000 seats) | medium | 4321 | 0 |
| 1000:16e3 | *stadium_draw* | game-finance | draw stadium picture/state | low | 870 | 1 |
| 1000:1a49 | *stadium_cost_panel* | game-finance | show account balance, current vs after-expansion values | medium | 1670 | 3 |
| 1000:20e1 | *stadium_construction_progress* | game-finance | weekly: report finished construction (Flutlichtmasten/Anzeigetafel/Komfort) | medium | 687 | 1 |
| 1000:239e | *stadium_construction_helper* | game-finance | helper of construction progress | low | 161 | 4 |
| 1000:243f | *credit_offer_text* | game-finance | "Kredit zu x% aufnehmen" text/DM per week display | medium | 750 | 10 |
| 1000:272d | *sponsor_offer_dialog* | game-finance | sponsor subsidy offer ("NEHMEN SIE EINEN SPONSOR-ZUSCHUSS VON ... DM AN?") | medium | 360 | 1 |
| 1000:2895 | *fixture_info_helper* | game-stats | helper for opponent/fixture info list | low | 47 | 2 |
| 1000:28c4 | *screen_fixture_results* | game-stats | fixture list with Hin/Rueck results (GEGNER, PLATZ, DATUM, ERGEBNIS) | medium | 2117 | 1 |
| 1310:000a | *matchday_header* | game-match | match-day header drawing | low | 240 | 2 |
| 1310:00fa | `FUN_1310_00fa` | game-match | called by 1310:01f2, match_penalty_shootout, championship_winner_message; draws UI | low | 248 | 4 |
| 1310:01f2 | `FUN_1310_01f2` | game-match | called by matchday_title_dfb_cup, matchday_title_european_cup, matchday_title_relegation; draws UI | low | 111 | 7 |
| 1310:0261 | *matchday_title_dfb_cup* | game-match | title "DFB-Pokal" | medium | 47 | 1 |
| 1310:0290 | *matchday_title_european_cup* | game-match | title "Europapokal" | medium | 47 | 1 |
| 1310:02bf | *matchday_title_relegation* | game-match | title "Relegationsspiel" | medium | 420 | 1 |
| 1310:0463 | `FUN_1310_0463` | game-match | called by season_main_loop; uses RNG | low | 398 | 3 |
| 1310:05f1 | *matchday_list_helper* | game-match | match list helper (team names) | low | 433 | 1 |
| 1310:07a2 | *matchday_title_rescheduled* | game-match | title "Nachholspiele" | medium | 499 | 1 |
| 1310:0995 | *matchday_title_league* | game-match | title "Ligaspiel" | medium | 48 | 1 |
| 1310:09c5 | `FUN_1310_09c5` | game-match | called by main, season_main_loop; uses RNG | low | 291 | 2 |
| 1310:0ae8 | `FUN_1310_0ae8` | game-match | called by 1310:0b24 | low | 60 | 5 |
| 1310:0b24 | `FUN_1310_0b24` | game-match | called by 1310:09c5, start_menu_new_or_load, season_main_loop | low | 1056 | 5 |
| 1310:0f44 | `FUN_1310_0f44` | game-match | called by 1310:0b24 | low | 114 | 1 |
| 1310:0fb6 | *matchday_screen_setup* | game-match | match day screen setup | low | 474 | 1 |
| 1310:1190 | *matchday_refresh* | game-match | called every loop iteration of season loop; match-day state refresh | low | 323 | 9 |
| 1310:12d3 | `FUN_1310_12d3` | game-match | called by match_engine_helper_08e9; uses RNG | low | 43 | 2 |
| 1310:12fe | `FUN_1310_12fe` | game-match | called by 1310:347f, match_engine_helper_08e9; uses RNG | low | 362 | 3 |
| 1310:1468 | *team_strength_value* | game-match | compute team strength from 3 lineup parts (5238:307e/3081/3084, stride 0x22) weighted by tactic table DS:008e | medium | 198 | 18 |
| 1310:152e | `FUN_1310_152e` | game-match | called by match_live_view | low | 173 | 2 |
| 1310:15db | *matchday_play* | game-match | play one match day: header by competition then live ticker | low | 1532 | 5 |
| 1310:1bd7 | `FUN_1310_1bd7` | game-match | called by 1310:2c9a, manager_ai_weekly, 3e3a:23aa | low | 75 | 6 |
| 1310:1c22 | `FUN_1310_1c22` | game-match | called by player_card, player_strength_line, screen_transfer_market | low | 177 | 3 |
| 1310:1cd3 | `FUN_1310_1cd3` | game-match | called by 1310:2c9a, manager_ai_weekly, 3e3a:23aa | low | 114 | 4 |
| 1310:1d45 | `FUN_1310_1d45` | game-match | called by match_simulate_minute; uses RNG | low | 170 | 3 |
| 1310:1def | `FUN_1310_1def` | game-match | called by match_live_view, 1310:322c, match_engine_substitutions; draws UI | low | 504 | 4 |
| 1310:1fe7 | `FUN_1310_1fe7` | game-match | called by matchday_screen_setup, match_ticker_minute, office_menu_dispatcher | low | 159 | 10 |
| 1310:2086 | *match_ticker_minute* | game-match | "Minute"/"ELFMETER" ticker line | medium | 461 | 3 |
| 1310:2253 | `FUN_1310_2253` | game-match | called by match_live_view | low | 50 | 2 |
| 1310:2285 | `FUN_1310_2285` | game-match | called by match_live_view | low | 126 | 1 |
| 1310:2303 | *match_live_view* | game-match | live match view: ticker, goal scenes ("Torszene war"), substitutions | low | 2455 | 4 |
| 1310:2c9a | `FUN_1310_2c9a` | game-match | called by match_ticker_events; uses RNG | low | 587 | 2 |
| 1310:2ee5 | *match_simulate_minute* | game-match | per-minute match simulation step (RNG) | low | 839 | 2 |
| 1310:322c | `FUN_1310_322c` | game-match | called by match_simulate_minute, match_penalty_shootout, match_engine_substitutions; draws UI | low | 133 | 3 |
| 1310:32b1 | *manager_index_of_team* | game-season | return 1+manager index controlling team a, or 0x81+i if team b, else 0 (manager table 5238:2260 stride 0x30a) | high | 99 | 30 |
| 1310:3314 | `FUN_1310_3314` | game-match | called by match_live_view; mouse input | low | 363 | 2 |
| 1310:347f | `FUN_1310_347f` | game-match | called by match_live_view, 1310:3314 | low | 238 | 3 |
| 1310:356d | *match_penalty_shootout* | game-match | "Elfmeterschiessen" penalty shoot-out and "Endergebnis ... n.E." | medium | 1540 | 1 |
| 16c7:0002 | `ui_hit_test_buttons` | ui | return 1-based index of button rect (x,y,w,h array) under mouse (mouse state at 5238:2e70), optionally only on click | high | 165 | 21 |
| 16c7:00a7 | `ui_draw_button` | ui | draw/register clickable button or icon (button table 5238:53a0, 20 entries x 12 bytes) with frame styles from flags | medium | 1212 | 78 |
| 16c7:0563 | `ui_redraw_buttons` | ui | redraw registered buttons (all when index 0x7f) of a given owner | medium | 414 | 20 |
| 16c7:0701 | `ui_draw_filled_frame` | ui | fill rect in one color then outline in another | high | 82 | 0 |
| 16c7:0753 | `ui_draw_frame` | ui | rectangle outline via move_to/line_to | high | 130 | 19 |
| 16c7:07d5 | `ui_draw_text_at` | ui | draw string at (x,y) in color (0 -> 0xFF) | high | 103 | 144 |
| 16c7:083c | `ui_fill_rect` | ui | fill rect (x0,y0,x1,y1) in color | high | 64 | 132 |
| 16c7:087c | `ui_draw_text_centered_shadow` | ui | draw string centered between x0..x1 at y, optional shadow color, clamps to 320 px | high | 308 | 160 |
| 16c7:09b0 | `ui_draw_line` | ui | line (x0,y0)-(x1,y1) in color | high | 73 | 74 |
| 16c7:09f9 | `copy_protection_penalty` | game-misc | if protection flag DS:05ac==0: 1% chance (random(0,100)==0) to hang forever toggling pointer - anti-crack trap | medium | 77 | 21 |
| 176b:0006 | `sound_voice_init` | sound | if sound enabled (DS:018c) and >6 EMS pages: probe SB, detect IRQ, init CT-VOICE, get EMS page frame; sets DS:018d | high | 69 | 1 |
| 176b:004b | `sound_voice_shutdown` | sound | CT-VOICE terminate if voice active | high | 20 | 1 |
| 176b:005f | `sound_load_digi_voc` | sound | allocate EMS, read SOUND\\DIGI.VOC (DS:0178) in chunks into EMS pages (5 samples, table DS:0150) | medium | 291 | 1 |
| 176b:0182 | `sound_play_voice` | sound | play digitized sample n from EMS via CT-VOICE (n==1 loops crowd noise, flag DS:018f) | medium | 141 | 8 |
| 176b:020f | `sound_stop_voice` | sound | stop CT-VOICE output and restore EMS map | high | 60 | 2 |
| 176b:024b | `sound_wait_voice_end` | sound | busy-wait until voice status word DS:94c2 is 0, restore EMS map | high | 50 | 2 |
| 176b:027d | `sound_restart_loop_voice` | sound | restart looping sample 1 when previous output finished | medium | 38 | 9 |
| 176b:02a3 | `ui_set_pointer_shape` | ui | select one of two mouse pointer images (DS:085e.. / 0876..) | medium | 65 | 3 |
| 176b:02e4 | `ui_blit_transparent` | ui | wrapper for blit_bitmap_transparent with bitmap handle | high | 53 | 13 |
| 176b:0319 | `input_poll_mouse` | input | poll latched mouse event into 5238:2ea2 (1=left,2=right) and click flag 3046/3048 | high | 61 | 32 |
| 176b:0356 | *vga_load_picture* | format-loader | load PIC\\<n>.VGA (".vga" DS:019b) into new or existing bitmap, optional palette; owned by VGA agent | high | 811 | 68 |
| 176b:0681 | `format_long_thousands` | ui | convert long to decimal string with "." thousand separators (DE style) into buffer 5238:a306.. | medium | 342 | 171 |
| 176b:07d7 | `ui_scroll_text_box` | ui | text box with scrolling (uses font metrics, draws lines) | low | 1028 | 9 |
| 176b:0bdb | *font_load_all* | format-loader | load fonts 1..3 by name from table DS:012a (flow/micro4/...) (font agent owns) | high | 112 | 1 |
| 176b:0c4b | *font_free_all* | format-loader | release font handles 1..3 | high | 70 | 1 |
| 176b:0c91 | `ui_set_font` | ui | attach font n (0..3, handles at 5238:0010) to rastport | high | 54 | 118 |
| 176b:0cc7 | `random_range` | game-misc | random(lo,hi) = rand() % (hi-lo+1) + lo  (MSC rand at 4a01:17be). All game randomness goes through here | high | 41 | 262 |
| 176b:0cf0 | `random_seed_from_time` | game-misc | srand(time(NULL)) | high | 26 | 2 |
| 176b:0d0a | `input_wait_click` | input | wait for release, then until left click (or abort flag DS:05d4) | high | 76 | 10 |
| 176b:0d56 | `text_filter_filename_char` | ui | accept [A-Za-z0-9] or char from table DS:01a0, remapping via DS:01a9 | medium | 116 | 0 |
| 176b:0dca | `bitmap_free` | gfx | free bitmap data, header and descriptor (mem_free x3) | high | 93 | 69 |
| 176b:0e27 | `input_wait_release` | input | poll until no mouse button held | high | 48 | 15 |
| 176b:0e57 | `ui_dialog_box` | ui | modal message/question box (draws frame, text lines, buttons, waits) | low | 757 | 25 |
| 176b:114c | `fatal_error_exit` | game-misc | shutdown: on code 99 normal exit, else error message (LIB/GRAFIK/DISK/FONT/SPEICHER/BildLade-Error), writes ERROR.MAN/autosave, frees sound | medium | 657 | 25 |
| 176b:1416 | `screen_fade` | gfx | fade/blank screen in (1) or out (0): palette fade loop over the 5238:4b50 palette or INT10 screen on/off | medium | 254 | 81 |
| 18bc:0004 | *startup_load_palette_and_init* | format-loader | load PIC\\1.PAL (DS:0206) and init screen/mouse/fonts (PAL agent owns the palette format) | medium | 503 | 1 |
| 18bc:01fb | *startup_clear_state* | game-season | zero-initialise game tables (memset) | low | 244 | 1 |
| 18bc:02ef | *office_free_bitmaps* | ui | free office screen bitmaps | low | 94 | 3 |
| 18bc:034d | *office_load_background* | ui | load office background picture | low | 203 | 4 |
| 18bc:0418 | `main` | game-season | main(argc,argv): argv[1]=="0" disables sound; init graphics; detect SB+FMDRV and play SOUND\\BM2TITLE.CMF; init voice/EMS; load MANA.DAT (362f:36ec); start menu (18bc:2184: new/load); run season loop 2a58:3176; exit(99) | high | 1242 | 1 |
| 18bc:08f2 | *matchday_options_dialog* | ui | dialog "Spiele automatisch / alleine / weiter / Spiele nicht" before a match day | medium | 369 | 2 |
| 18bc:0a63 | *office_random_event* | game-season | random office event helper | low | 252 | 2 |
| 18bc:0b5f | `office_menu_dispatcher` | ui | per-manager office turn: draws office, handles icon menus via two 6-entry jump tables (18bc:1a15, 18bc:1b42) -> screens; autosave AUTOSAVE.MAN; returns when manager ends turn | medium | 4139 | 2 |
| 18bc:1c1d | *quit_game_confirm* | ui | "MOECHTEN SIE DAS SPIEL WIRKLICH BEENDEN?" confirmation | medium | 192 | 2 |
| 18bc:1cdd | `FUN_18bc_1cdd` | ui | called by start_menu_new_or_load; draws UI | low | 278 | 3 |
| 18bc:1df3 | `FUN_18bc_1df3` | ui | called by start_menu_new_or_load | low | 130 | 2 |
| 18bc:1e75 | `FUN_18bc_1e75` | ui | called by start_menu_new_or_load | low | 33 | 2 |
| 18bc:1e96 | `FUN_18bc_1e96` | ui | called by start_menu_new_or_load; draws UI | low | 302 | 1 |
| 18bc:1fc4 | `FUN_18bc_1fc4` | ui | called by start_menu_new_or_load | low | 157 | 2 |
| 18bc:2061 | `FUN_18bc_2061` | ui | called by start_menu_new_or_load; draws UI | low | 291 | 4 |
| 18bc:2184 | *start_menu_new_or_load* | game-season | title/start menu: number of managers, names, teams, level (string table DS:4a2a "Spiel-Level"), or load game; returns 0 for new game | low | 6670 | 2 |
| 18bc:3b92 | *office_show_cached_background* | ui | show office background: load picture once into VGA page 2 (offset 0x9600) and page-copy afterwards | medium | 113 | 18 |
| 1c7c:0004 | `tore_play_frames` | sound | plays a TORE goal scene frame by frame and starts voices 2..5 on the frames listed in the TORE header (see voc.md §5; called by 24a4:05ec) | medium | 908 | 1 |
| 1cb5:0000 | `FUN_1cb5_0000` | game-season | called by screen_load_game | low | 18 | 1 |
| 1cb5:0012 | *season_end_messages* | game-season | promotion/relegation/licence/sponsor/top-scorer/European-cup qualification messages at season end | medium | 5035 | 1 |
| 1cb5:13bd | *random_weekly_events* | game-season | random events: hooligan damage to stadium, comfort downgrade, training injuries | medium | 4632 | 1 |
| 1cb5:25d5 | *event_helper_25d5* | game-season | helper of random events | low | 385 | 1 |
| 1cb5:2756 | *event_helper_2756* | game-season | helper of season end messages | low | 561 | 1 |
| 1cb5:2987 | `FUN_1cb5_2987` | game-season | called by event_helper_2756, 24a4:19fb; uses RNG | low | 180 | 2 |
| 1cb5:2a3b | `FUN_1cb5_2a3b` | game-season | called by event_helper_2756, 24a4:19fb | low | 333 | 2 |
| 1cb5:2b88 | `FUN_1cb5_2b88` | game-season | called by random_weekly_events, season_main_loop | low | 113 | 3 |
| 1cb5:2bf9 | `FUN_1cb5_2bf9` | game-season | called by matchday_options_dialog; draws UI; mouse input | low | 648 | 1 |
| 1f9d:0002 | *manager_ai_weekly* | game-season | per-manager weekly processing (AI teams + RNG), called from season loop | low | 1685 | 10 |
| 1f9d:0697 | *league_init_or_ai* | game-season | league processing called from main and season loop | low | 594 | 3 |
| 1f9d:08e9 | *match_engine_helper_08e9* | game-match | match engine helper (RNG) | low | 850 | 3 |
| 1f9d:0c3b | *match_engine_substitutions* | game-match | match engine: substitutions/lineup during match | low | 1255 | 4 |
| 1f9d:1122 | `FUN_1f9d_1122` | game-match | called by screen_training | low | 190 | 1 |
| 1f9d:11e0 | `FUN_1f9d_11e0` | game-match | called by 2a58:20b2; floating point | low | 495 | 2 |
| 212a:000a | `FUN_212a_000a` | game-finance | called by start_menu_new_or_load, finance_weekly_processing; uses RNG | low | 170 | 2 |
| 212a:00b4 | `FUN_212a_00b4` | game-finance | called by season_end_messages, transfer_list_add | low | 145 | 2 |
| 212a:0145 | *screen_training_camp* | game-finance | office menu 2/case 5: Trainingslager (training camp) booking | medium | 1512 | 0 |
| 212a:072d | `FUN_212a_072d` | game-finance | called by screen_training_camp; uses RNG; draws UI | low | 832 | 1 |
| 212a:0a6d | *finance_weekly_processing* | game-finance | per-manager weekly finances incl. due loans ("Ihr Kredit von der Bank wurde heute faellig") | medium | 1312 | 3 |
| 212a:0f8d | *finance_sum_entries* | game-finance | sum income/expense entries of current manager (5 x 3 x 0x12 at 5238:243e) | medium | 172 | 10 |
| 212a:1039 | *credit_take* | game-finance | take a credit ("Schon 3 Kredite") | medium | 206 | 1 |
| 212a:119b | *credit_list_entry* | game-finance | credit list row (DM, due date, interest/month) | medium | 1252 | 5 |
| 212a:167f | *screen_credits* | game-finance | office menu 1/case 5: Kredite (bank/co-player loans, max 1 Mio / 4 Mio DM, 24 months) | medium | 3404 | 0 |
| 212a:23cb | *credit_summary_panel* | game-finance | Kontostand / Gesamtschulden / Zinsen pro Monat panel | medium | 897 | 2 |
| 212a:274c | *screen_training* | game-squad | office menu 2/case 4: Training intensity | medium | 2561 | 0 |
| 212a:314d | *season_goal_warning* | game-season | "Achtung! Dies ist der letzte Spieltag..." championship/promotion/relegation status message | medium | 1629 | 1 |
| 24a4:000a | `FUN_24a4_000a` | game-stats | called by 1cb5:2bf9 | low | 73 | 5 |
| 24a4:0053 | `FUN_24a4_0053` | game-stats | called by tore_scene_play | low | 422 | 1 |
| 24a4:01f9 | *list_empty_marker* | ui | "FEHLANZEIGE" (no entries) display | low | 334 | 1 |
| 24a4:0347 | `FUN_24a4_0347` | game-stats | called by match_ticker_events | low | 210 | 1 |
| 24a4:0419 | *tore_scene_open* | format-loader | open TORE\\ANZAHL and goal-scene files (TORE agent owns) | medium | 347 | 1 |
| 24a4:0574 | *tore_disk_error* | format-loader | "Ihre Torszenen-Diskette ist offenbar defekt" error | medium | 120 | 1 |
| 24a4:05ec | *tore_scene_play* | format-loader | play a TORE\\ goal scene animation (TORE agent owns) | medium | 794 | 1 |
| 24a4:0906 | *screen_player_info* | game-squad | "Info ueber" player detail (age, status, games, goals, tendency, exhaustion) | medium | 2854 | 4 |
| 24a4:14d4 | `FUN_24a4_14d4` | game-stats | called by 24a4:1662; uses RNG | low | 398 | 2 |
| 24a4:1662 | `FUN_24a4_1662` | game-stats | called by match_live_view | low | 236 | 2 |
| 24a4:174e | `FUN_24a4_174e` | game-stats | called by 24a4:1798; uses RNG | low | 74 | 2 |
| 24a4:1798 | `FUN_24a4_1798` | game-stats | called by event_helper_2756, 24a4:19fb; uses RNG | low | 611 | 3 |
| 24a4:19fb | `FUN_24a4_19fb` | game-stats | called by main | low | 218 | 1 |
| 24a4:1ad5 | *screen_top_scorers* | game-stats | office menu 2/case 6: Torschuetzenliste | medium | 2309 | 1 |
| 24a4:23da | `FUN_24a4_23da` | game-stats | called by screen_transfer_market; uses RNG | low | 229 | 2 |
| 24a4:24bf | `FUN_24a4_24bf` | game-stats | called by random_weekly_events, 24a4:1798; uses RNG | low | 201 | 4 |
| 24a4:2588 | `FUN_24a4_2588` | game-stats | called by random_weekly_events | low | 130 | 1 |
| 24a4:260a | `FUN_24a4_260a` | game-stats | called by season_end_messages; draws UI | low | 44 | 3 |
| 24a4:2636 | `FUN_24a4_2636` | game-stats | called by finance_weekly_processing, 24a4:2b15, screen_statistics | low | 492 | 3 |
| 24a4:2822 | `FUN_24a4_2822` | game-stats | called by finance_weekly_processing, 24a4:2b15, screen_statistics | low | 755 | 3 |
| 24a4:2b15 | `FUN_24a4_2b15` | game-stats | no direct callers found (reached via unrecovered switch case or pointer) | low | 165 | 0 |
| 24a4:2bba | `FUN_24a4_2bba` | game-stats | called by player_aging_or_development, expenses_panel | low | 250 | 2 |
| 24a4:2cb4 | *player_aging_or_development* | game-squad | per-manager periodic player update (RNG) | low | 1051 | 3 |
| 24a4:30cf | `FUN_24a4_30cf` | game-stats | called by random_weekly_events, match_ticker_events; uses RNG | low | 278 | 2 |
| 27c2:0006 | *cup_draw* | game-season | "DFB-Pokal Auslosung" cup draw screen | medium | 882 | 2 |
| 27c2:0378 | `FUN_27c2_0378` | game-season | called by competition_init, cup_round_setup; draws UI | low | 1314 | 3 |
| 27c2:089a | *press_key_prompt* | ui | "TASTE DRUECKEN" prompt and wait | medium | 326 | 3 |
| 27c2:09e0 | *competition_init* | game-season | initialise league/cup competition n (called with 0..3 from main) | low | 1298 | 5 |
| 27c2:0ef2 | *competition_reset* | game-season | reset competitions at startup | low | 661 | 2 |
| 27c2:1187 | *competition_helper_1187* | game-season | cup helper | low | 96 | 3 |
| 27c2:11e7 | *team_is_in_list* | game-season | true if team id is in list 5238:56fe (count 5238:2e74) | high | 63 | 8 |
| 27c2:1226 | *cup_round_helper* | game-season | cup round helper | low | 380 | 2 |
| 27c2:13a2 | *cup_round_setup* | game-season | cup round setup | low | 582 | 1 |
| 27c2:15e8 | *cup_pairing_helper* | game-season | cup pairing helper | low | 244 | 3 |
| 27c2:16dc | *cup_play_round* | game-season | play cup round | low | 1169 | 1 |
| 27c2:1b6d | *competition_count* | game-season | competition helper used by main | low | 72 | 3 |
| 27c2:1bb5 | *league_of_rank* | game-season | map league position to tier using limits DS:226f/2270 (returns 1,2,4) | high | 59 | 9 |
| 27c2:1bf0 | *match_pairing_text* | game-match | "X GEGEN Y" text | medium | 219 | 1 |
| 27c2:1ccb | *screen_cup_matches* | game-match | Pokalspiele / Relegation results (n.V., n.E.) | medium | 2690 | 6 |
| 27c2:274d | *championship_winner_message* | game-season | "Deutsche Meisterschaft ... gewinnt die/den" | medium | 542 | 3 |
| 2a58:000c | `FUN_2a58_000c` | game-match | called by season_main_loop; draws UI; mouse input | low | 145 | 1 |
| 2a58:009d | `FUN_2a58_009d` | game-match | called by match_penalty_shootout, match_engine_substitutions, 2a58:042f; draws UI | low | 433 | 3 |
| 2a58:024e | `FUN_2a58_024e` | game-match | called by 2a58:042f, match_ticker_events; draws UI | low | 258 | 3 |
| 2a58:0350 | `FUN_2a58_0350` | game-match | called by 2a58:042f; draws UI | low | 223 | 2 |
| 2a58:042f | `FUN_2a58_042f` | game-match | texts: "(AUSW.)"; called by match_live_view, 1310:322c, match_penalty_shootout; draws UI | low | 2164 | 5 |
| 2a58:0ca3 | *match_ticker_events* | game-match | live match ticker: chances, penalties, goals, own goals, assists, red/yellow cards, injuries | medium | 4818 | 5 |
| 2a58:1f75 | `FUN_2a58_1f75` | game-match | called by 1310:3314, match_ticker_events; draws UI; mouse input | low | 220 | 2 |
| 2a58:2051 | *match_forfeit_penalty* | game-match | "Ihr Spiel wird mit 0:2 gewertet. Sie zahlen 200.000 DM Strafe" | medium | 97 | 2 |
| 2a58:20b2 | `FUN_2a58_20b2` | game-match | called by matchday_play; uses RNG | low | 2388 | 2 |
| 2a58:2a06 | *special_date_events* | game-misc | date-triggered screens (table DS:53d0 day/month): "Frohe Weihnachten" bonus, developer birthday "Oh happy day" (WERNER KRAHE, JENS ONNEN) | medium | 1904 | 2 |
| 2a58:3176 | `season_main_loop` | game-season | main game loop: per day advance date (5238 day counter DS:07dc long), process all managers (offices via 18bc:0b5f), matches, finances (212a:0a6d), season end (2a58:4597), WINTERPAUSE | medium | 5153 | 1 |
| 2a58:4597 | *season_end* | game-season | SAISONENDE processing | medium | 454 | 1 |
| 2ecd:000e | *player_card* | game-squad | player card (name, age, status, reserve, tendency, exhaustion) | medium | 1697 | 2 |
| 2ecd:06af | *screen_transfer_market_list* | game-transfer | TRANSFERMARKT / IHRE MANNSCHAFT list | medium | 710 | 7 |
| 2ecd:10ee | *player_lineup_check* | game-squad | check player of current manager squad (5238:7759 stride 0x34) for transfer/loan state | low | 267 | 10 |
| 2ecd:11f9 | `FUN_2ecd_11f9` | game-squad | called by 2ecd:1266 | low | 109 | 1 |
| 2ecd:1266 | `FUN_2ecd_1266` | game-squad | no direct callers found (reached via unrecovered switch case or pointer) | low | 196 | 0 |
| 2ecd:132a | `FUN_2ecd_132a` | game-squad | no direct callers found (reached via unrecovered switch case or pointer); draws UI | low | 267 | 0 |
| 2ecd:1435 | `FUN_2ecd_1435` | game-squad | called by 2ecd:1560, screen_lineup; draws UI | low | 146 | 2 |
| 2ecd:14c7 | *auto_lineup_disable* | game-squad | "Auto-Aufstellung ist deaktiviert" | medium | 153 | 2 |
| 2ecd:1560 | `FUN_2ecd_1560` | game-squad | proposed *screen_squad_lineup*(mode, sel): squad/lineup screen over the player records 5238:774a+ (stride 0x34, byte +0x0a = lineup slot: 1..11 starting XI, 12..16 substitutes, 0 not selected); mode 0x63 ('c') switches to an internal variant (flag 2); called by 2a58:1f75 and by office menu 2 cases 1-2 (18bc:1af5/1afc, not visible as xrefs) | medium | 863 | 1 |
| 2ecd:18bf | *screen_lineup* | game-squad | team lineup screen (Auto-Aufstellung, substitutions limits, reserve limits) | medium | 3086 | 9 |
| 2ecd:24cd | `FUN_2ecd_24cd` | game-squad | called by screen_lineup, 2ecd:29c6, squad_weekly_update; draws UI | low | 395 | 5 |
| 2ecd:2658 | `FUN_2ecd_2658` | game-squad | called by screen_lineup, 2ecd:29c6; draws UI | low | 142 | 2 |
| 2ecd:26e6 | `FUN_2ecd_26e6` | game-squad | called by screen_lineup, contract_negotiation; draws UI | low | 433 | 2 |
| 2ecd:2897 | *player_strength_line* | game-squad | "STAERKE: ... JAHRE" line | medium | 303 | 1 |
| 2ecd:29c6 | `FUN_2ecd_29c6` | game-squad | called by 2ecd:1560, screen_lineup, 2ecd:29c6; draws UI; mouse input | low | 1962 | 6 |
| 2ecd:3170 | `FUN_2ecd_3170` | game-squad | called by screen_lineup, 2ecd:29c6; draws UI | low | 496 | 5 |
| 2ecd:3360 | *squad_weekly_update* | game-squad | per-manager squad update from season loop | low | 725 | 11 |
| 2ecd:3635 | `FUN_2ecd_3635` | game-squad | called by squad_weekly_update | low | 419 | 3 |
| 2ecd:37d8 | *transfer_list_add* | game-transfer | put player on transfer market ("Schon 12 Mann auf dem Transfermarkt", 24 max) | medium | 722 | 7 |
| 3277:000a | `FUN_3277_000a` | game-transfer | called by random_weekly_events, 212a:072d, transfer_list_add; uses RNG | low | 119 | 3 |
| 3277:0081 | `FUN_3277_0081` | game-transfer | called by screen_training; draws UI | low | 296 | 2 |
| 3277:01a9 | `FUN_3277_01a9` | game-transfer | called by screen_training | low | 124 | 4 |
| 3277:0225 | `FUN_3277_0225` | game-transfer | called by screen_training; draws UI | low | 640 | 8 |
| 3277:04a5 | *screen_transfer_market* | game-transfer | office menu 2/case 3: Transfermarkt (buy/loan, offers, allowed amount) | medium | 5818 | 0 |
| 3277:1b5f | *transfer_offer_received* | game-transfer | offer for own player: BEHALTEN / VERKAUFEN | medium | 729 | 2 |
| 3277:1e38 | *transfer_market_init* | game-transfer | transfer market (re)initialisation | low | 825 | 2 |
| 3277:2171 | `FUN_3277_2171` | game-transfer | called by screen_transfer_market; uses RNG | low | 255 | 1 |
| 3277:2270 | `FUN_3277_2270` | game-transfer | called by contract_negotiation; uses RNG | low | 878 | 1 |
| 3277:25de | *transfer_value_calc* | game-transfer | player value/price computation (RNG) | low | 1201 | 15 |
| 3277:2a8f | *contract_negotiation* | game-transfer | contract negotiation (duration in seasons, salary per month) | medium | 4341 | 3 |
| 362f:0004 | `FUN_362f_0004` | game-stats | called by start_menu_new_or_load, screen_settings; draws UI | low | 130 | 5 |
| 362f:0086 | `FUN_362f_0086` | game-stats | called by screen_settings; draws UI | low | 306 | 2 |
| 362f:01b8 | *screen_settings* | game-misc | Einstellungen: what to show (half-time, results, table, goal scenes, cups, newspaper, fades), level, speed | medium | 2279 | 2 |
| 362f:0a9f | `FUN_362f_0a9f` | game-stats | called by screen_player_info, screen_statistics | low | 88 | 7 |
| 362f:0af7 | *screen_statistics* | game-stats | office menu 1/case 1: Statistik (red cards etc.) | medium | 3055 | 0 |
| 362f:18e7 | *screen_eternal_table* | game-stats | office menu 1/case 2: Ewige Tabelle / Ewige Bilanz | medium | 2298 | 0 |
| 362f:21e1 | `FUN_362f_21e1` | game-stats | called by screen_statistics, screen_opponent_info | low | 515 | 3 |
| 362f:23e4 | `FUN_362f_23e4` | game-stats | called by screen_manager_contract; draws UI | low | 166 | 4 |
| 362f:248a | `FUN_362f_248a` | game-stats | called by 362f:29a8, screen_manager_contract; draws UI | low | 99 | 3 |
| 362f:24ed | `FUN_362f_24ed` | game-stats | called by 362f:29a8; draws UI | low | 100 | 1 |
| 362f:2551 | *contract_info_line* | game-transfer | "VERTRAG: n JAHR(E) n MONAT(E) KUENDBAR" line | medium | 718 | 7 |
| 362f:281f | *job_offer_no_interest* | game-misc | "KEIN INTERESSE..." job offers | low | 393 | 2 |
| 362f:29a8 | `FUN_362f_29a8` | game-stats | called by screen_manager_contract; draws UI | low | 576 | 3 |
| 362f:2be8 | *screen_manager_contract* | game-misc | office menu 1/case 6: own contract ("Sie stehen noch unter Vertrag") | medium | 2820 | 0 |
| 362f:36ec | `mana_load_club_table` | format-loader | load MANA.DAT (DS:54c0) team/player database (MANA agent owns) | high | 546 | 1 |
| 362f:390e | *screen_manager_history* | game-stats | office menu 1/case 3: "Damals... Ihre Erfolge" history | medium | 2080 | 0 |
| 3a41:000e | *screen_opponent_info* | game-stats | opponent scouting info (home/away record, last games, remaining fixtures) | medium | 3840 | 4 |
| 3a41:0f0e | *list_no_entries* | ui | "NOCH KEINE EINTRAGUNG." | low | 469 | 4 |
| 3a41:10e3 | *league_team_range* | game-stats | compute team index range for league tier (DS:225e/225f/2262) | medium | 295 | 12 |
| 3a41:120a | *screen_fixtures* | game-stats | Spielplan (fixtures by match day, Nachholspiele) | medium | 2830 | 3 |
| 3a41:1d18 | *table_row_strength* | game-stats | "PLATZ, STAERKE" row | low | 476 | 3 |
| 3a41:1ef4 | `FUN_3a41_1ef4` | game-stats | called by 3a41:2d33 | low | 188 | 4 |
| 3a41:1fb0 | `FUN_3a41_1fb0` | game-stats | called by 3a41:2d33 | low | 60 | 4 |
| 3a41:1fec | `FUN_3a41_1fec` | game-stats | called by 3a41:2d33; uses RNG | low | 135 | 2 |
| 3a41:2073 | `FUN_3a41_2073` | game-stats | called by 3a41:2d33 | low | 217 | 2 |
| 3a41:214c | *screen_league_table* | game-stats | Tabelle (league table) | medium | 3047 | 2 |
| 3a41:2d33 | `FUN_3a41_2d33` | game-stats | called by matchday_play, match_live_view, office_menu_dispatcher | low | 2716 | 6 |
| 3a41:37cf | `FUN_3a41_37cf` | game-stats | called by 3a41:2d33 | low | 194 | 8 |
| 3a41:3891 | *screen_strength_table* | game-stats | Staerketabelle (team strength ranking) | medium | 1804 | 1 |
| 3e3a:060a | `FUN_3e3a_060a` | game-misc | called by 3e3a:06df; draws UI | low | 213 | 2 |
| 3e3a:06df | `FUN_3e3a_06df` | game-misc | called by screen_statistics, newspaper_template_text, 3e3a:0d33; draws UI | low | 570 | 6 |
| 3e3a:0919 | `FUN_3e3a_0919` | game-misc | called by 3e3a:06df | low | 37 | 1 |
| 3e3a:093e | `FUN_3e3a_093e` | game-misc | called by 2ecd:1560 | low | 38 | 2 |
| 3e3a:0964 | `FUN_3e3a_0964` | game-misc | called by 3e3a:0964, newspaper_template_text, newspaper_helper; uses RNG | low | 679 | 4 |
| 3e3a:0c0b | *newspaper_template_text* | game-misc | newspaper/report text generator over template table DS:90d2 (%x escapes, # alternatives) | medium | 296 | 0 |
| 3e3a:0d33 | `FUN_3e3a_0d33` | game-misc | called by newspaper_template_text | low | 135 | 1 |
| 3e3a:0dba | *newspaper_helper* | game-misc | newspaper helper | low | 233 | 0 |
| 3e3a:0ea3 | `FUN_3e3a_0ea3` | game-misc | no direct callers found (reached via unrecovered switch case or pointer); draws UI | low | 314 | 0 |
| 3e3a:0fdf | *christmas_text* | game-misc | Christmas message text ("dem lammettabehangenen ...") | low | 2890 | 2 |
| 3e3a:1b29 | `FUN_3e3a_1b29` | game-misc | called by 2ecd:29c6; uses RNG; draws UI; mouse input | low | 1732 | 2 |
| 3e3a:21ef | *random_chance_pick* | game-misc | if random(1,n) > *p: set *p and return random(lo,hi), else return default | high | 79 | 14 |
| 3e3a:223e | `FUN_3e3a_223e` | game-misc | called by match_ticker_events | low | 364 | 5 |
| 3e3a:23aa | `FUN_3e3a_23aa` | game-misc | called by matchday_play; draws UI | low | 454 | 1 |
| 4091:0000 | *team_slot_free_index* | game-squad | first free slot (0..9) in 10-byte row DS:1d34+n*10, 0x7f if full | medium | 68 | 2 |
| 4091:0044 | *message_find_owner* | game-misc | find message pointer in per-manager message table (5238:5524 stride 0x50) | low | 332 | 13 |
| 4091:0190 | *message_box* | ui | message window with text lines and OK | low | 928 | 7 |
| 4091:0532 | *messages_free_all* | game-misc | free all message strings of all managers | medium | 146 | 2 |
| 4091:05c4 | *screen_messages* | ui | message inbox ("Keine Nachricht vorhanden.") | medium | 1254 | 4 |
| 4091:0aaa | *ui_choose_column* | ui | wait for click in strip x 0x6c..0xe7, y 0xf6..0x105; return column (x-0x6c)/0x29 | medium | 113 | 1 |
| 4091:0b1b | *ui_draw_label* | ui | draw label text helper | low | 138 | 2 |
| 4091:0ba5 | *money_round_display* | game-finance | round DM amount to multiples of 5 depending on difficulty level (DS:224e/2250) | low | 661 | 3 |
| 4091:0e3a | *dialog_okay* | ui | "OKAY" dialog | medium | 463 | 26 |
| 4091:1009 | *clamp_int* | game-misc | *p = clamp(*p, lo, hi) (int) | high | 42 | 31 |
| 4091:1033 | *clamp_char* | game-misc | *p = clamp(*p, lo, hi) (signed char) | high | 42 | 0 |
| 4091:105d | *text_to_upper_de* | ui | uppercase incl. German umlauts (cp437 0x81->0x9a, 0x84->0x8e, 0x94->0x99) | high | 120 | 16 |
| 4091:10d5 | *long_abs_diff* | game-misc | /a-b/ of two longs, returned as int | high | 52 | 13 |
| 4091:1109 | *squad_count_by_flag* | game-squad | count players of team n by flag (mode 0/1/2) in 5238:7754/7759 (stride 0x34, 25 per team) | medium | 195 | 40 |
| 4091:11cc | *ui_hit_icon_slot* | ui | hit-test registered icon slots (DS:533c/90ae) | medium | 113 | 9 |
| 4091:123d | *ui_register_icon_slot* | ui | save background and register icon slot (max 10) | medium | 559 | 24 |
| 4091:146c | *message_broadcast* | ui | "An Alle:" broadcast message to all managers | low | 694 | 7 |
| 4091:1722 | *ui_wait_with_highlight* | ui | highlight rect until click/abort | medium | 149 | 18 |
| 4091:17b7 | *find_player_by_id* | game-squad | search squads 0..4 for player id; returns record pointer | medium | 165 | 2 |
| 4091:185c | *ui_draw_manager_header* | ui | draw current manager header (name/team) | low | 329 | 3 |
| 422b:0006 | *disk_error_dialog* | fileio | "Achtung! Fehler beim Diskettenzugriff! Vorgang abgebrochen." | medium | 65 | 4 |
| 422b:0047 | *savegame_write_encrypted* | format-loader | write block XOR/add-encrypted with rolling key DS:57cf (key -= 0x29 per byte), checksum DS:56dc (MAN agent owns) | high | 248 | 64 |
| 422b:013f | *savegame_read_decrypted* | format-loader | read block and decrypt (inverse of 422b:0047) (MAN agent owns) | high | 145 | 64 |
| 422b:01d0 | *savegame_helper_01d0* | format-loader | save/load helper | low | 94 | 2 |
| 422b:022e | *savegame_helper_022e* | format-loader | save/load helper | low | 302 | 2 |
| 422b:035c | *player_table_generate* | game-state | generate the player table and deal players to managers by position band (see logic/state.md) | medium | 1186 | 1 |
| 422b:07fe | *screen_save_game* | format-loader | Spielstand Speichern (overwrite confirmation) (MAN agent owns) | medium | 2574 | 3 |
| 422b:120c | *screen_load_game* | format-loader | Spielstand Laden (version check, V1.x level prompt) (MAN agent owns) | medium | 4024 | 2 |
| 422b:21c4 | *highscore_load_save* | format-loader | HIGH.00 hall of fame file (DS:949c) | medium | 418 | 4 |
| 422b:2366 | *highscore_screen* | game-stats | hall of fame display "(X/X/X)", PUNKTE | low | 1278 | 4 |
| 422b:2864 | *highscore_helper* | game-stats | hall of fame helper | low | 454 | 1 |
| 422b:2a2a | *expenses_panel* | game-finance | "AUSGABEN (GEHAELTER, REISEKOSTEN)" panel | medium | 404 | 2 |
| 422b:2bbe | *file_selector* | fileio | file requester: directory listing *.* / *.MAN, pages, delete ("WIRKLICH LOESCHEN?") | medium | 3151 | 2 |
| 45ac:16a0 | `font_map_char` | gfx | map character AL to glyph index (umlaut remap table CS:03eb, <0x20 or >0x7e -> space) minus 0x20 | high | 46 | 2 |
| 45ac:16d0 | `rect_hits_mouse_pointer` | gfx | test whether rect (x,y,w,h) overlaps the software mouse pointer rectangle | high | 115 | 10 |
| 45ac:1746 | `mouse_pointer_restore_bg` | gfx | remove software pointer: blit saved background back to screen | high | 90 | 13 |
| 45ac:17a0 | `mouse_pointer_draw` | gfx | save background under pointer and draw pointer sprite (masked blit) | high | 176 | 12 |
| 45ac:192d | `mouse_pop_button_event` | input | return and clear latched mouse event: 1=left down,2=right down,3,4 (flags CS:0023..0026 set by INT33 handler) | medium | 65 | 1 |
| 45ac:1971 | `mouse_set_position` | input | clamp and set pointer position (INT33/04), redraw software pointer | high | 168 | 4 |
| 45ac:1a19 | `mouse_set_pointer_shape` | input | set pointer image/mask bitmaps and hotspot | high | 104 | 1 |
| 45ac:1a81 | `mouse_pointer_hide` | input | hide software pointer (nesting counter CS:0011) | high | 69 | 7 |
| 45ac:1ac7 | `mouse_pointer_show` | input | decrement hide counter; redraw pointer when it reaches 0 | high | 64 | 8 |
| 45ac:1b08 | `mouse_install` | input | INT33/00 reset, set ranges (320x240, x scaled), install event handler, init pointer state; returns 0 if no mouse | high | 321 | 1 |
| 45ac:1c4b | `mouse_uninstall` | input | INT33 reset/remove handler | high | 53 | 1 |
| 45ac:1c80 | `vga_set_mode_x_320x240` | gfx | INT10 mode 13h then unchain (SEQ 4=06), clear 4 planes, misc 0xE3, CRTC table CS:039a (17 regs) -> 320x240 Mode X, 80 bytes/row, page size 0x4B00 | high | 136 | 1 |
| 45ac:1d08 | `vga_set_text_mode` | gfx | restore text mode via INT10 | high | 20 | 1 |
| 45ac:1d1c | `blit_rect` | gfx | clipped rectangle copy between bitmaps (linear or planar VGA A000), pointer-aware | high | 936 | 10 |
| 45ac:20c8 | `blit_rect_transparent` | gfx | clipped rectangle copy skipping color 0 (masked blit) | high | 954 | 4 |
| 45ac:2486 | `put_pixel` | gfx | set pixel (linear bitmap or Mode X plane write via SEQ map mask), pointer-aware | high | 254 | 5 |
| 45ac:2586 | `xor_pixel` | gfx | XOR pixel with 0x1F (GC function select 3=XOR) | high | 268 | 5 |
| 45ac:2694 | `get_pixel` | gfx | read pixel (Mode X read map select) | high | 233 | 1 |
| 45ac:278b | `draw_line` | gfx | Bresenham line with put_pixel | high | 269 | 1 |
| 45ac:2898 | `draw_line_xor` | gfx | Bresenham line with xor_pixel | high | 266 | 1 |
| 45ac:29a4 | `fill_rect` | gfx | clipped solid rectangle fill | high | 409 | 1 |
| 45ac:2b3e | `fill_rect_xor` | gfx | clipped XOR rectangle fill | high | 433 | 1 |
| 45ac:2cf0 | `clear_bitmap` | gfx | clear whole bitmap/screen page to color | high | 146 | 1 |
| 45ac:2d83 | `draw_text` | gfx | render string with proportional bitmap font (font table CS:03fe) in color/bg/mode | high | 303 | 1 |
| 45ac:2eb4 | `text_width` | gfx | sum glyph advance widths of a string for a font | high | 97 | 1 |
| 45ac:2f16 | *font_find_by_name* | gfx | look up registered font by name in table CS:03fe (fonts owned by font agent) | medium | 124 | 1 |
| 45ac:2f93 | `video_screen_off` | gfx | INT10/12 BL=36 AL=1: disable video refresh (blank) | medium | 8 | 2 |
| 45ac:2f9b | `video_screen_on` | gfx | INT10/12 BL=36 AL=0: enable video refresh | medium | 8 | 2 |
| 45ac:2fa3 | `vga_set_dac_color` | gfx | INT10/1010: set one DAC register (6-bit RGB) | high | 53 | 2 |
| 45ac:2fd8 | `vga_restore_palette_slot` | gfx | restore palette slot saved in off-screen VRAM: read map = slot, source A000:E100+3*first (interleaved RGB), split into R/G/B arrays at +0/+0x100/+0x200, then INT10/1012 sets the DAC block; called by palette_load_rgb4 when ptr<4 | high | 114 | 1 |
| 45ac:304a | `far_memcpy_bytes` | gfx | byte-wise far copy src->dst n (returns CS); used widely as memcpy | high | 48 | 52 |
| 45ac:307a | `timer_set_deadline` | gfx | deadline = BIOS tick count 0040:006C + n (CS:0038/003a) | high | 66 | 6 |
| 45ac:30bc | `timer_deadline_reached` | gfx | return 1 when BIOS tick count >= deadline | high | 74 | 6 |
| 45ac:3107 | `vga_next_plane_helper` | gfx | near helper: advance read plane when SI hits 0x4AFF | low | 25 | 3 |
| 45ac:3120 | *vga_decode_to_bitmap* | gfx | decode packed picture stream into bitmap/Mode X planes (called by .VGA loader 176b:0356; owned by VGA agent) | medium | 181 | 2 |
| 45ac:31d6 | *vga_encode_rle* | gfx | RLE encoder counterpart (no callers in BMMAIN) | low | 148 | 0 |
| 45ac:326b | `dos_getch` | input | INT21/07: read key without echo | high | 7 | 1 |
| 45ac:3272 | `dos_direct_console_io` | input | INT21/06: direct console I/O (non-blocking key poll) | high | 11 | 4 |
| 45ac:327d | `vga_show_page` | gfx | set CRTC start address to page 0 (0) or page 1 (0x4B00); flag CS:0033 | high | 126 | 2 |
| 45ac:32fe | `vga_set_draw_page` | gfx | select page for drawing (CS:0033 = n-1) | high | 45 | 4 |
| 45ac:332b | `vga_copy_page` | gfx | latched copy of 0x4B00 bytes between VGA pages (GC mode 1) | high | 79 | 4 |
| 45ac:337e | `install_int_handler` | gfx | store handler far pointer and set interrupt vector via INT21/25 | low | 58 | 1 |
| 45ac:33ca | `vga_set_palette_rgb4` | gfx | convert 32 Amiga 12-bit RGB words (0x0RGB) to 6-bit DAC values and upload with INT10/1012 after vretrace | medium | 292 | 2 |
| 45ac:34ef | `vga_wait_vretrace` | gfx | wait for vertical retrace (3DAh bit3) | high | 13 | 10 |
| 45ac:34fc | `fill_rect_planar` | gfx | clipped planar rectangle operation (read+write planes) | low | 402 | 3 |
| 45ac:3690 | `dos_delete_file` | fileio | INT21/41 unlink(path); 0 on success | medium | 47 | 1 |
| 45ac:36c7 | `ems_detect` | fileio | check INT67 vector for "EMMXXXX0", INT67/40 status, 67/42 free pages; returns free page count | medium | 78 | 1 |
| 45ac:3716 | `ems_get_page_frame` | fileio | INT67/41: page frame segment (CS:0034) | medium | 57 | 1 |
| 45ac:3750 | `ems_alloc_pages` | fileio | INT67/43: allocate n pages, return handle or -1 | medium | 54 | 1 |
| 45ac:3787 | `ems_map_page` | fileio | INT67/44: map logical page to physical page | medium | 49 | 2 |
| 45ac:37b8 | `ems_map_or_free` | fileio | INT67/44 (n<4) or 67/45 deallocate | low | 60 | 2 |
| 45ac:37f5 | `ems_save_map` | fileio | INT67/47: save page map | medium | 43 | 1 |
| 45ac:3820 | `ems_restore_map` | fileio | INT67/48: restore page map | medium | 43 | 2 |
| 4930:000c | `file_close` | fileio | close(fd) wrapper | high | 23 | 17 |
| 4930:0023 | `file_open` | fileio | open file: mode 0 -> _open read, else _dos_creat; returns 0 on failure instead of -1 | high | 101 | 16 |
| 4930:0088 | `file_read` | fileio | _read(fd, far buf, n) wrapper | high | 31 | 35 |
| 4930:00a7 | `file_seek` | fileio | _tell + _lseek(fd, off, SEEK_SET) wrapper | high | 47 | 0 |
| 4930:00d6 | `file_write` | fileio | _write wrapper; returns -1 if short write | high | 43 | 12 |
| 4930:010c | `mem_alloc` | fileio | _fmalloc wrapper, records block in table 5238:0180 (selector DS:a144 = 5238; 200 far pointers; a full table overflows into slot 200), optional zero-fill (flag==1) | high | 124 | 30 |
| 4930:0188 | `mem_free` | fileio | free block registered in the 5238:0180 table (_ffree + _fheapmin) | high | 95 | 26 |
| 4930:01e7 | `delay_ticks` | ui | busy-wait n/3 BIOS ticks via timer_set_deadline/timer_deadline_reached | medium | 47 | 26 |
| 4930:0216 | *font_handle_invalidate* | gfx | set font handle to -1 | medium | 22 | 1 |
| 4930:022c | *font_open* | gfx | allocate font handle and look font up by name (font agent owns) | medium | 53 | 1 |
| 4930:0261 | `rastport_set_font` | gfx | attach font to rastport (+0x0c) | high | 48 | 1 |
| 4930:0291 | `rastport_draw_text` | gfx | draw string at pen position with rastport colors/mode | high | 101 | 11 |
| 4930:02f6 | `rastport_text_width` | gfx | width in pixels of string in rastport font | high | 38 | 15 |
| 4930:031c | `rastport_move_to` | gfx | set pen position (+8,+10) | high | 31 | 16 |
| 4930:033b | `rastport_line_to` | gfx | draw line from pen to (x,y) (XOR in mode 2), update pen | high | 147 | 8 |
| 4930:03ce | `rastport_put_pixel` | gfx | plot pixel in pen color (XOR in mode 2) | high | 108 | 17 |
| 4930:043a | `rastport_get_pixel` | gfx | read pixel color | high | 55 | 5 |
| 4930:0471 | `rastport_fill_rect` | gfx | fill rect (x0,y0)-(x1,y1) inclusive in pen color | high | 140 | 10 |
| 4930:04fd | `rastport_set_color` | gfx | set foreground pen (+4) | high | 24 | 27 |
| 4930:0515 | `rastport_set_bg_color` | gfx | set background pen (+5) | high | 24 | 1 |
| 4930:052d | `rastport_set_draw_mode` | gfx | set draw mode (+6; 2 = XOR) | high | 24 | 28 |
| 4930:0545 | `rastport_clear` | gfx | clear bitmap of rastport to color | high | 55 | 9 |
| 4930:057c | `bitmap_init` | gfx | init bitmap header (w,h, data ptr = 0) | high | 40 | 1 |
| 4930:05a4 | `rastport_init` | gfx | init rastport defaults (fg 0xFF, bg 0, mode 1) | high | 57 | 1 |
| 4930:05dd | `palette_set_entry` | gfx | store R,G,B in palette arrays (+0,+0x100,+0x200) and program DAC | high | 71 | 2 |
| 4930:0624 | `palette_load_rgb4` | gfx | load n palette entries from 12-bit 0x0RGB words (or from VGA when ptr<4), expand 4->6 bit | high | 177 | 7 |
| 4930:06d5 | `palette_get_rgb4` | gfx | return palette entry as 12-bit 0x0RGB | high | 61 | 1 |
| 4930:0712 | `screen_open` | gfx | allocate screen rastport 320x240 at A000:0000 and set Mode X | high | 108 | 1 |
| 4930:077e | `mouse_open` | input | allocate pointer state and call mouse_install | high | 155 | 1 |
| 4930:0819 | `screen_close` | gfx | restore text mode | high | 13 | 1 |
| 4930:0826 | `mouse_close` | input | mouse_uninstall and clear handle | high | 35 | 1 |
| 4930:0849 | `blit_bitmap_scaled_or_copy` | gfx | copy rect between bitmaps via temporary buffer when both are on screen | medium | 328 | 12 |
| 4930:0991 | `blit_bitmap` | gfx | copy rect (sx,sy)->(dx,dy) w x h between bitmaps (temp buffer for screen->screen) | high | 276 | 132 |
| 4930:0aa5 | `blit_bitmap_transparent` | gfx | masked copy (color 0 transparent) between bitmaps | high | 264 | 3 |
| 4930:0bad | `draw_circle_octants` | gfx | plot 8 symmetric circle points | high | 227 | 2 |
| 4930:0c90 | `draw_circle` | gfx | midpoint circle using draw_circle_octants | high | 134 | 1 |
| 4a01:0016 | `crt_start` | crt | MSC C0 startup: DOS version check, set SS=DS=DGROUP 5cb3, shrink memory (INT21/4A), zero BSS DS:a306..a560, run initializers, call main(argc,argv,envp) at 18bc:0418, then exit | high | 203 | 0 |
| 4a01:00f1 | `crt_amsg_exit` | crt | _amsg_exit: print runtime error banner/message (R60xx) and terminate with code 255 | high | 36 | 3 |
| 4a01:0116 | `crt_inherit_file_info` | crt | __cinit part: save INT 0 vector, parse _C_FILE_INFO= from environment, probe handles 0-4 with IOCTL (INT21/44) to flag devices | medium | 197 | 1 |
| 4a01:01db | `crt_exit` | crt | exit(): run atexit/terminator tables (_initterm), flush, _nullcheck, restore vectors, INT21/4C | high | 105 | 2 |
| 4a01:025e | `crt_ctermsub` | crt | _ctermsub: restore INT 0 / user vectors before terminating | medium | 45 | 1 |
| 4a01:028b | `crt_initterm` | crt | _initterm: call a table of far function pointers [SI..DI) in reverse order | high | 19 | 8 |
| 4a01:029e | `crt_ff_msgbanner` | crt | _FF_MSGBANNER: write "run-time error" banner (message 252/255) | high | 34 | 3 |
| 4a01:02c0 | `crt_amsg_exit_jump` | crt | tail jump to _amsg_exit (stack overflow path) | medium | 6 | 1 |
| 4a01:02c6 | `crt_chkstk` | crt | _chkstk: allocate AX bytes of stack frame, R6000 stack overflow check against DS:954a (called by ~320 functions) | high | 36 | 319 |
| 4a01:02ea | `crt_nullcheck` | crt | _nullcheck: XOR-checksum DS:0000..0041 must equal 0x55, else R6001 null pointer assignment | high | 36 | 1 |
| 4a01:030e | `crt_setargv` | crt | _setargv: parse PSP command tail (PSP:81) into argv[], build argc | medium | 428 | 1 |
| 4a01:04bc | `crt_setenvp` | crt | _setenvp: build envp[] from environment block, skipping _C_FILE_INFO | medium | 160 | 1 |
| 4a01:055c | `crt_nmsg_text` | crt | _NMSG_TEXT: look up runtime message text by number in table DS:a156 | high | 43 | 1 |
| 4a01:0587 | `crt_nmsg_write` | crt | _NMSG_WRITE: write runtime message to stderr (INT21/40) | high | 54 | 5 |
| 4a01:05be | `crt_alloc_or_die` | crt | allocate with _amblksiz forced to 0x400; on failure _amsg_exit (used for stdio/env buffers) | medium | 37 | 2 |
| 4a01:05e4 | `crt_dosret0` | crt | _dosret0: CF set -> map DOS error, return -1; else 0 | high | 8 | 2 |
| 4a01:05ec | `crt_dosretax` | crt | _dosretax: CF set -> map DOS error, return -1; else return AX | medium | 13 | 2 |
| 4a01:05f9 | `crt_dosreturn` | crt | _dosreturn: CF set -> map DOS error (errno) | medium | 13 | 7 |
| 4a01:0606 | `crt_dosmaperr_far` | crt | far wrapper of _dosmaperr | medium | 6 | 1 |
| 4a01:060c | `crt_dosmaperr` | crt | _dosmaperr: translate DOS error AL into errno (DS:9508) via table DS:9550; _doserrno at DS:9513 | high | 45 | 3 |
| 4a01:063a | `crt_close` | crt | _close(fd): INT21/3E, clear _osfile[fd] (DS:9517) | high | 32 | 1 |
| 4a01:065a | `crt_lseek` | crt | _lseek(fd, long off, whence): INT21/42 | high | 122 | 5 |
| 4a01:06ee | `crt_open` | crt | _open/_sopen(path, oflag, pmode): INT21/3D/3C, O_CREAT/O_TRUNC/O_APPEND handling, fills _osfile | high | 417 | 1 |
| 4a01:088f | `crt_nop_088f` | crt | empty helper (share/lock stub) | low | 17 | 1 |
| 4a01:08a0 | `crt_read` | crt | _read(fd, buf, n): INT21/3F with text-mode CR/LF and ^Z translation | high | 254 | 1 |
| 4a01:099e | `crt_write` | crt | _write(fd, buf, n): INT21/40 with text-mode LF->CRLF expansion via stack buffer | high | 192 | 1 |
| 4a01:0a5e | `crt_write_flush_chunk` | crt | _write helper: flush translated chunk | medium | 102 | 3 |
| 4a01:0ac4 | `crt_write_raw` | crt | _write helper: binary-mode direct write | medium | 57 | 1 |
| 4a01:0afe | `crt_stackavail` | crt | _stackavail: bytes between SP and stack limit DS:954a | high | 20 | 1 |
| 4a01:0b12 | `crt_malloc_thunk` | crt | thunk to _fmalloc (malloc in large model) | high | 5 | 2 |
| 4a01:0b18 | `crt_ffree` | crt | _ffree: mark far heap block free (set bit 0 of length word) | high | 19 | 1 |
| 4a01:0b2b | `crt_fmalloc` | crt | _fmalloc(size): far heap first-fit search over heap segments (DS:9566/956a), grows via DOS 48h | high | 125 | 2 |
| 4a01:0ba8 | `crt_heap_init_segment` | crt | far heap: initialize a new heap segment descriptor | medium | 52 | 1 |
| 4a01:0bdc | `crt_heap_link_segment` | crt | far heap: link heap segment into rover list | medium | 54 | 1 |
| 4a01:0c12 | `crt_heap_search_segment` | crt | far heap: first-fit search/coalesce inside one segment | medium | 123 | 2 |
| 4a01:0d1d | `crt_heap_grow_segment` | crt | far heap: grow segment with INT21/4A | medium | 80 | 1 |
| 4a01:0d6e | `crt_heap_find_end` | crt | far heap: walk to end sentinel 0xFFFE | medium | 32 | 1 |
| 4a01:0d8e | `crt_heap_new_segment` | crt | far heap: allocate new DOS block (INT21/48) and add as heap segment | medium | 67 | 1 |
| 4a01:0dd2 | `crt_fstrcat` | crt | _fstrcat(dst, src) | high | 70 | 394 |
| 4a01:0e18 | `crt_fstrcpy` | crt | _fstrcpy(dst, src) | high | 54 | 237 |
| 4a01:0e4e | `crt_fstrcmp` | crt | _fstrcmp(a, b) | high | 41 | 10 |
| 4a01:0e78 | `crt_fstrlen` | crt | _fstrlen(s) | high | 23 | 39 |
| 4a01:0e90 | `crt_fstrncpy` | crt | _fstrncpy(dst, src, n) (pads with NUL) | high | 42 | 2 |
| 4a01:0eba | `crt_fstrncmp` | crt | _fstrncmp(a, b, n) | high | 59 | 4 |
| 4a01:0ef6 | `crt_atoi_thunk` | crt | thunk to atoi | high | 3 | 1 |
| 4a01:0efa | `crt_atof` | crt | atof(s): skip spaces, _fltin, copy double to DS:a53a | high | 87 | 2 |
| 4a01:0f52 | `crt_atoi` | crt | atoi(s) (also used as atol low word) | high | 86 | 1 |
| 4a01:0fa8 | `crt_fptostr` | crt | _fptostr: copy/round mantissa digits for _cftoX | high | 162 | 3 |
| 4a01:104e | `crt_getenv` | crt | getenv(name) over environ (DS:9531) | high | 141 | 1 |
| 4a01:10dc | `crt_stub_10dc` | crt | tiny far stub (*p=1, return 0); referenced as far pointer from code seg 3e3a area - likely FP/emulator hook table | low | 23 | 0 |
| 4a01:10f3 | `crt_stub_10f3` | crt | tiny far stub (*p=0) | low | 21 | 0 |
| 4a01:1108 | `crt_stub_1108` | crt | empty far stub | low | 9 | 0 |
| 4a01:1111 | `crt_stub_1111` | crt | tiny far stub (*p=v) | low | 22 | 0 |
| 4a01:1127 | `crt_stub_1127` | crt | empty far stub | low | 9 | 0 |
| 4a01:1130 | `crt_stub_1130` | crt | far stub doing one INT21 call, returns 0 | low | 26 | 0 |
| 4a01:114a | `crt_huge_ptr_diff` | crt | difference of two huge (seg:off) pointers as long (used by memmove overlap test) | medium | 43 | 1 |
| 4a01:1176 | `crt_tell` | crt | _tell(fd) = _lseek(fd,0,SEEK_CUR) | medium | 23 | 1 |
| 4a01:118e | `crt_filelength` | crt | _filelength(fd): seek end, restore position | high | 134 | 1 |
| 4a01:1214 | `crt_fheapmin` | crt | _fheapmin: release free heap segments to DOS | medium | 56 | 1 |
| 4a01:124c | `crt_heapmin_segment` | crt | heapmin helper: shrink one segment | medium | 109 | 1 |
| 4a01:12ba | `crt_heap_unlink_free_segment` | crt | heapmin helper: unlink segment and INT21/49 free it | medium | 89 | 1 |
| 4a01:1314 | `crt_intdosx` | crt | intdosx(inregs, outregs, sregs): generic INT21 call | high | 97 | 1 |
| 4a01:1376 | `crt_outp` | crt | outp(port, value) | high | 14 | 4 |
| 4a01:1384 | `crt_time` | crt | time(t): INT21/2A+2C, convert via __loctotime_t (used to seed srand) | high | 85 | 1 |
| 4a01:13da | `crt_tzset_once` | crt | call __tzset once (DS:a542 flag) | high | 17 | 1 |
| 4a01:13ec | `crt_tzset` | crt | __tzset: parse TZ env (DS:96ca) into timezone/daylight/tzname | high | 207 | 1 |
| 4a01:14bc | `crt_isindst` | crt | _isindst: US DST rule (April..October) | high | 218 | 1 |
| 4a01:1596 | `crt_loctotime` | crt | __loctotime_t: y/m/d h:m:s -> seconds since 1970 (+timezone, DST) | high | 259 | 1 |
| 4a01:169a | `crt_fmemmove` | crt | _fmemmove/_fmemcpy with huge-pointer segment wrap and overlap handling | high | 202 | 1 |
| 4a01:1764 | `crt_fmemset` | crt | _fmemset(p, c, n) | high | 72 | 2 |
| 4a01:17ac | `crt_srand` | crt | srand(seed): DS:9720 = seed, DS:9722 = 0 | high | 17 | 1 |
| 4a01:17be | `crt_rand` | crt | rand(): state = state*0x343FD + 0x269EC3 (32-bit, DS:9720), return (state>>16)&0x7FFF  -- the game RNG core | high | 40 | 1 |
| 4a01:1809 | `crt_chdir` | crt | chdir(path): INT21/3B | high | 15 | 3 |
| 4a01:1842 | `crt_getdrive` | crt | _getdrive(): INT21/19 + 1 | high | 8 | 4 |
| 4a01:184a | `crt_chdrive` | crt | _chdrive(n): INT21/0E then verify with 19 | high | 27 | 3 |
| 4a01:1866 | `crt_getcwd` | crt | getcwd(buf, n) = _getdcwd(0, buf, n) | high | 24 | 3 |
| 4a01:187e | `crt_getdcwd` | crt | _getdcwd(drive, buf, n): INT21/47, malloc if buf==NULL | high | 259 | 1 |
| 4a01:1984 | `crt_dos_handle_call_1984` | crt | _dos_xxx returning handle in *p (INT21, CF->errno) | low | 7 | 0 |
| 4a01:198b | `crt_dos_creat` | crt | _dos_creat(path, attr, &handle): INT21/3C (used by file_open for write mode) | medium | 38 | 1 |
| 4a01:19b2 | `crt_dos_findnext` | crt | _dos_findnext: set DTA (1A), find next (4F), restore DTA | medium | 11 | 1 |
| 4a01:19bd | `crt_dos_findfirst` | crt | _dos_findfirst: set DTA (1A/2F), find first (4E) | medium | 51 | 1 |
| 4a01:19f0 | `crt_dos_setdrive` | crt | _dos_setdrive(drive, &ndrives): INT21/0E | medium | 23 | 1 |
| 4a01:1a08 | `crt_aflldiv` | crt | _aFlldiv: *plong /= long (assign form) | high | 34 | 4 |
| 4a01:1a2a | `crt_afllmul` | crt | _aFllmul: *plong *= long (assign form) | high | 34 | 1 |
| 4a01:1a4c | `crt_afldiv` | crt | _aFldiv: signed 32-bit divide (truncates toward zero) | high | 154 | 142 |
| 4a01:1ae6 | `crt_aflmul` | crt | _aFlmul: 32-bit multiply | high | 50 | 132 |
| 4a01:1b18 | `crt_aflrem` | crt | _aFlrem: signed 32-bit remainder (sign of dividend) | high | 160 | 11 |
| 4a01:1bb8 | `crt_aflshl` | crt | _aFlshl: 32-bit shift left by CL | medium | 11 | 3 |
| 4a01:1bc4 | `crt_aflshr` | crt | _aFlshr: 32-bit shift right by CL | medium | 11 | 4 |
| 4a01:1bd0 | `crt_afldiv_assign` | crt | _aFNaldiv: *plong /= long (signed) | medium | 32 | 7 |
| 4a01:1bf0 | `crt_aflmul_assign` | crt | _aFNalmul: *plong *= long | medium | 32 | 6 |
| 4a01:1c10 | `crt_afuldiv` | crt | _aFuldiv: unsigned 32-bit divide | high | 95 | 6 |
| 4a01:1c70 | `crt_afulrem` | crt | _aFulrem: unsigned 32-bit remainder | high | 106 | 1 |
| 4a01:1cda | `crt_fmemset_2` | crt | second copy of _fmemset (different object module) | medium | 72 | 1 |
| 4a01:1d70 | `crt_fltout_core` | crt | $I8_OUTPUT/_fltout core: convert double to decimal digits using FP emulator (INT 34h-3Dh) | medium | 383 | 1 |
| 4a01:2031 | `crt_fltout_pow10` | crt | scale by powers of ten for _fltout | medium | 62 | 1 |
| 4a01:2086 | `crt_ld12cvt` | crt | __ld12cvt/$I10_OUTPUT: long double -> decimal string struct | medium | 526 | 3 |
| 4a01:254c | `crt_fp_helper_254c` | crt | FP conversion helper (decompile fails: overlapping instructions) | low | 145 | 2 |
| 4a01:2621 | `crt_ld12mul` | crt | 12-byte extended multiply for decimal conversion | medium | 394 | 1 |
| 4a01:27a6 | `crt_atoldbl_mantissa` | crt | __atoldbl helper: accumulate decimal mantissa | medium | 174 | 1 |
| 4a01:2854 | `crt_shl96` | crt | 96-bit shift-left-by-1 helper | medium | 11 | 3 |
| 4a01:2860 | `crt_strgtold_wrap` | crt | _fltin helper: __strgtold12 + FP emulator store | medium | 42 | 1 |
| 4a01:28c8 | `crt_strgtold12` | crt | __strgtold12: parse decimal string into 12-byte long double | medium | 616 | 2 |
| 4a01:2ba0 | `crt_forcdecpt` | crt | _forcdecpt / _cropzeros style string fixup | medium | 123 | 0 |
| 4a01:2c1e | `crt_positive` | crt | _positive(double): sign test via FP emulator | medium | 46 | 0 |
| 4a01:2c4c | `crt_fassign` | crt | _fassign: string -> float/double for scanf | medium | 107 | 0 |
| 4a01:2cba | `crt_cftoe` | crt | _cftoe: double -> e-format string | high | 390 | 2 |
| 4a01:2e42 | `crt_cftoe_g` | crt | _cftoe with g-format flag | medium | 42 | 1 |
| 4a01:2e6c | `crt_cftof` | crt | _cftof: double -> f-format string | high | 339 | 2 |
| 4a01:2fc2 | `crt_cftof_g` | crt | _cftof with g-format flag | medium | 39 | 1 |
| 4a01:2fea | `crt_cftog` | crt | _cftog: double -> g-format string | high | 213 | 1 |
| 4a01:30c0 | `crt_cfltcvt` | crt | _cfltcvt(e/f/g dispatcher); pointer stored in _cfltcvt_tab DS:97e4 | high | 96 | 0 |
| 4a01:3122 | `crt_shift_string` | crt | _shift: move string right by n chars | medium | 52 | 4 |
| 4a01:3156 | `crt_fp_emul_init` | crt | FP emulator/8087 control-word init (INT 35h/3Dh) | low | 51 | 0 |
| 4a01:318a | `crt_fltin` | crt | _fltin: string -> double struct (DS:a548) | high | 89 | 1 |
| 4a01:31e4 | `crt_fltout` | crt | _fltout: double -> STRFLT struct (DS:979e) | high | 75 | 3 |
| 4a01:3288 | `crt_cftoe_ld` | crt | long-double e-format (2nd generation _cftoe) | medium | 76 | 1 |
| 4a01:32d4 | `crt_cftoe_ld_fmt` | crt | format helper for e-format | medium | 368 | 2 |
| 4a01:344a | `crt_cftof_ld` | crt | long-double f-format | medium | 71 | 1 |
| 4a01:3492 | `crt_cftof_ld_fmt` | crt | format helper for f-format | medium | 288 | 2 |
| 4a01:35b6 | `crt_cftog_ld` | crt | long-double g-format | medium | 122 | 1 |
| 4a01:3630 | `crt_cfltcvt_ld` | crt | second _cfltcvt dispatcher (DS:97f8) | medium | 102 | 0 |
| 4a01:3698 | `crt_fp_tail_3698` | crt | FP helper; Ghidra body runs past segment end into 4d6c sound code (bogus extent) | low | 38 | 1 |
| 4d6c:000e | `sb_dsp_read` | sound | read DSP data port (base+0Ah) after polling base+0Eh bit7, 0x200 tries, CF on timeout | high | 39 | 2 |
| 4d6c:0035 | `sb_dsp_write_timeout` | sound | write AL to DSP (base+0Ch) when bit7 clear, 0x200 tries, CF on timeout | high | 40 | 2 |
| 4d6c:007b | `sb_dsp_write` | sound | write AL to DSP (base+0Ch), wait forever for ready | high | 31 | 1 |
| 4d6c:009a | `dos_set_vector_raw` | sound | write far pointer DX:AX into IVT slot BX (CLI, direct 0000:BX*4) | high | 21 | 10 |
| 4d6c:00af | `dos_get_vector_raw` | sound | read IVT slot BX into DX:AX | high | 19 | 5 |
| 4d6c:00f0 | `sb_detect_irq` | sound | hook IRQ 2/3/5/7 vectors (INT 0Ah,0Bh,0Dh,0Fh), unmask PIC, send DSP F2h (raise IRQ) and record which IRQ fired in DS:94c8 | high | 199 | 1 |
| 4d6c:01fc | `sb_detect_port` | sound | scan base ports 0x220..0x260 step 0x10 with sb_probe_port, fallback 0x210; result in DS:94c6 | high | 46 | 1 |
| 4d6c:022a | `sb_probe_port` | sound | probe card at DS:94c6: SB mixer/DSP E0h test and OPL2 timer test; returns capability bits (bit2 = DSP present) | medium | 154 | 3 |
| 4d6c:02c4 | `opl_wait_status` | sound | poll OPL status port (base+8) until (status&0xE0)==AH&0xE0, 0x40 tries | high | 33 | 2 |
| 4d6c:02e5 | `opl_calibrate_delay` | sound | calibrate OPL register-write delay loops (DS:94ca/94cc) using PIT channel 0 at 0x1b58 and IRQ0 hook | medium | 125 | 1 |
| 4d6c:0362 | `opl_write_reg` | sound | write OPL register AH=index AL=value via base+8/base+9 with calibrated busy-wait delays | high | 41 | 7 |
| 4d6c:038b | `pit_set_channel0_divisor` | sound | program PIT channel 0 mode 3 with divisor AX (ports 43h/40h) | high | 13 | 2 |
| 4d6c:03ab | `ctvoice_init` | sound | CT-VOICE driver: fn1 set base port (DS:94c6), fn2 set IRQ (DS:94c8), fn3 init, fn5 set status word address DS:94c2 | high | 56 | 1 |
| 4d6c:03e3 | `ctvoice_speaker` | sound | CT-VOICE fn4: speaker on/off | high | 16 | 0 |
| 4d6c:03f3 | `ctvoice_set_status_addr` | sound | CT-VOICE fn5: set voice status word address | high | 20 | 0 |
| 4d6c:0407 | `ctvoice_stop` | sound | CT-VOICE fn8: stop voice output | high | 9 | 2 |
| 4d6c:0410 | `ctvoice_terminate` | sound | CT-VOICE fn9: terminate driver | high | 9 | 1 |
| 4d6c:0419 | `ctvoice_input` | sound | CT-VOICE fn7: start voice input (sampling rate/length/ES:DI buffer) | high | 29 | 0 |
| 4d6c:0436 | `ctvoice_output` | sound | CT-VOICE fn6: start voice output from ES:DI (.VOC data block) | high | 20 | 1 |
| 4d6c:045c | `ctvoice_break_loop` | sound | CT-VOICE fn12: break voice repeat loop | high | 16 | 0 |
| 4d6c:046c | `sb_dsp_reset` | sound | DSP reset: write 1/0 to base+6, wait for 0xAA on read port (32 tries); AX=0 ok, 1 fail | high | 47 | 1 |
| 4d6c:049c | `fmdrv_init` | sound | SBFMDRV: fn -3 set base port, fn -1 init/detect, fn1 set status byte address DS:94c4 (old address saved in CS:04d7) | medium | 59 | 1 |
| 4d6c:04ee | `fmdrv_play_music` | sound | SBFMDRV fn6: play CMF music block DX:AX | high | 19 | 1 |
| 4d6c:0501 | `fmdrv_set_instruments` | sound | SBFMDRV fn2: set instrument table (DX:AX, CX count) | high | 22 | 1 |
| 4d6c:0517 | `fmdrv_reset` | sound | SBFMDRV fn8: reset driver | high | 13 | 1 |
| 4d6c:0524 | `fmdrv_set_system_clock` | sound | SBFMDRV fn3: set system clock rate | high | 16 | 0 |
| 4d6c:0534 | `fmdrv_set_driver_clock` | sound | SBFMDRV fn4: set driver clock rate (song tempo, from CMF header ticks/sec) | high | 16 | 1 |
| 4d6c:0544 | `fmdrv_stop_music` | sound | SBFMDRV fn7: stop music | high | 9 | 1 |
| 4d6c:0566 | `fmdrv_pause_music` | sound | SBFMDRV fn9: pause music | high | 13 | 0 |
| 4d6c:0573 | `fmdrv_resume_music` | sound | SBFMDRV fn10: resume music | high | 13 | 0 |
| 4d6c:0580 | `fmdrv_poll_status` | sound | read music status byte DS:94c4 (nonzero = playing) and reset it to 0xFF atomically | high | 26 | 1 |
| 4d6c:059a | `fmdrv_shutdown` | sound | SBFMDRV fn -2 (uninstall) and restore previous status byte address | high | 26 | 1 |
| 4dcb:0000 | `ctvoice_entry_jmp` | sound | embedded Creative CT-VOICE.DRV (c) Creative Technology 1989-1990: entry JMP to dispatcher | high | 3 | 11 |
| 4dcb:05e6 | `ctvoice_internal_05e6` | sound | CT-VOICE.DRV internal: normalize far pointer of voice block | medium | 70 | 1 |
| 4dcb:06fe | `ctvoice_internal_06fe` | sound | CT-VOICE.DRV internal: start new voice block | low | 32 | 1 |
| 4dcb:0754 | `ctvoice_dispatch` | sound | CT-VOICE.DRV dispatcher: BX=function 0..13 via table CS:0085 | high | 65 | 1 |
| 4e67:0000 | `fmdrv_entry_jmp` | sound | embedded Creative SBFMDRV ("FMDRV") entry JMP to dispatcher | high | 3 | 13 |
| 4e67:093b | `fmdrv_internal_pit_out` | sound | SBFMDRV internal: OUT 40h | low | 3 | 1 |
| 4e67:093e | `fmdrv_internal_opl_write` | sound | SBFMDRV internal: OPL register write with delays | medium | 38 | 3 |
| 4e67:0aea | `fmdrv_internal_0aea` | sound | SBFMDRV internal: multi-register OPL write | low | 53 | 1 |
| 4e67:0bd3 | `fmdrv_internal_0bd3` | sound | SBFMDRV internal fragment | low | 1 | 1 |
| 4e67:110f | `fmdrv_dispatch` | sound | SBFMDRV dispatcher: BX>=0 fn 0..14 via CS:0261, BX<0 fn ~BX via CS:027f; reentrancy flag CS:01e4 | high | 94 | 1 |
