# Squad, lineup and tactics board (BMMAIN segment 2ecd)

Status: reconstructed from the disassembly of `/unpacked/BMMAIN_unpacked.EXE`. Every function in segment
2ecd was read instruction by instruction, because the Ghidra decompiler loses most far-data segment
selectors and cdecl arguments here. The throwaway listing with resolved selectors, callee names and
string pointers is `out/squad/seg.asm`, made by `out/squad/dis.py`.

Conventions:
* `DS:xxxx` is DGROUP `5cb3`. `FAR:xxxx` is the far data segment `5238`.
* `m` is the current manager index `FAR:304a` (`i8`, sign-extended with `cbw` before every multiply).
* `SQ(g,i)` is the squad record `FAR:774a + 0x34*(g*25 + i)` (see §1.2). Unless a group is named
  explicitly, `g = m`. The index arithmetic is
  `(i8)(0x19 * (i8)FAR:304a) + i` (`imul byte`, so the result is a 16-bit signed value), multiplied by 0x34
  with a 16-bit `imul`.
* `PL(p)` is the player-table record `FAR:57dd + 0x25*p` (§1.1). The index is always computed with an
  8-bit `mul` (`al=0x25; mul byte`), so `p` is a `u8`.
* `MGR(m)` is the manager record `FAR:2260 + 0x30a*m` (savegame block 13 starts at FAR:2242, which is
  MGR(m) - 0x1e; MEMORY_MAP uses base 2260).
* `random_range(lo,hi)` is 176b:0cc7 = `rand() % (hi-lo+1) + lo` (see rng.md). In the original the call
  pushes `hi` first, so the C argument order is `(lo, hi)`.
* German strings are quoted verbatim in the game's charset: `[`=Ä, `\`=Ö, `]`=Ü, `{`=ä, `|`=ö, `}`=ü,
  `~`=ß. `STR[k]` is entry *k* of the main UI string table, a far pointer at `DS:22e0 + 4*k`.
  `LBL[k]` is a far pointer at `DS:4a14 + 4*k`. Labels are cited by their DS pointer address.

**Important corrections to FUNCTION_MAP.md**
1. `2ecd:3360` (*squad_weekly_update*) is **not** a weekly form/fitness/aging/contract update. It is the
   **automatic lineup** (`auto_lineup`). No function in segment 2ecd changes form, fitness, age,
   injuries or contract length. Those updates live elsewhere (see Open questions).
2. `2ecd:18bf` (*screen_lineup*) is **not a function**. It is the middle of `screen_squad_lineup`
   (2ecd:1560..24cc). The nine "callers" in 3e3a are false xrefs: they are `push cs; call 21ef` near
   calls inside segment 3e3a. A plate comment was set; the bogus function object was left in place.
3. `2ecd:06af`'s Ghidra body is truncated (710 bytes). The real function runs to 2ecd:10ed and contains
   two `jmp cs:[bx+tbl]` jump tables (2ecd:0914 and 2ecd:1001, 18 entries each). Disassemble them as
   data.
4. `2ecd:000e` is not a "player card". It is the hover/click handler of the squad list.
5. `2ecd:10ee` does not check the lineup. It removes a record from a squad group.

Ghidra renames applied: see the function table in §2.

---------------------------------------------------------------------------------------------------

## 1. Data layout

### 1.1 Player table `FAR:57dd`, stride 0x25 (37 bytes), 151 records (savegame block 17)

Record 0 is unused. Players 1..150 exist. The position class is fixed by the index range (written by
`mana_load_player_names` 422b:035c): 1..20 goalkeepers, 21..63 defenders, 64..105 midfielders,
106..150 forwards.

| Off | Abs (p*0x25+) | Type | Meaning | Evidence |
|---|---|---|---|---|
| +0x00 | 57dd | char[26] | name (from MANA.DAT) | list column 2 |
| +0x1a | 57f7 | u8 | **age** in years. Init `random_range(18,33)`; players of human squads are re-rolled with `random_range(19,31)` | tooltip "NAME: x (nn JAHRE)", 2897 |
| +0x1b | 57f8 | u8 | base average strength = (u16)(+0x1c + +0x1d + +0x1e) / 3 (signed `idiv`) | 422b:04e6 |
| +0x1c | 57f9 | u8 | base **Kondition** (fitness). Copied to squad +0x10 | 37d8 |
| +0x1d | 57fa | u8 | base **Technik** (technique). Copied to squad +0x11 | 37d8 |
| +0x1e | 57fb | u8 | base **Form**. Init `random_range(45,55)`. Copied to squad +0x12 | 37d8 |
| +0x1f | 57fc | u8 | **position code** = 25*pos + r, with pos 0=TOR, 1=ABW, 2=MIT, 3=ANG. GK: 0. DEF: 25+`random_range(0,16)`. MID: 50+r. ATT: 75+r. `pos = code / 25` (unsigned 8-bit `div`) | 3635, 06af, 1266 |
| +0x20 | 57fd | u8 | **preferred foot**, `random_range(0,6)`: `<5` adds "L", `>1` adds "R", both gives "L+R" (1310:1c22) | 1310:1c22 |
| +0x21 | 57fe | u8 | **owner** manager index; 4 = transfer market, 5 = nobody | 10ee, 3635 |
| +0x22 | 57ff | u8 | counter, zeroed when the player joins a new club (37d8) | 37d8 |
| +0x23 | 5800 | u8 | counter, zeroed when the player joins a new club; +1 when he is substituted **in** during a league match (1560 at 2147). Probably league appearances for the current club | 1560 |
| +0x24 | 5801 | u8 | **club id** (index into the club table FAR:3066, stride 0x22); compared with `MGR(m)+0x00` | 37d8 |

`mana_load_player_names` generates Kondition and Technik as `10 - (random_range(0,20) - s)`, where
`s = random_range(30,89)`, re-rolled once if `s < 45 && random_range(0,1) != 0`. That function belongs
to another area; it is summarized here only for the field meanings.

### 1.2 Squad record `FAR:774a`, stride 0x34 (52 bytes), 5 groups x 25 (savegame block 18)

Group `g` = 0..3 are the squads of human managers 0..3. Group 4 (`FAR:304a == 4` while it is processed)
is the **transfer list**. `squad_count_by_flag` (4091:1109) scans only the first 12 records of group 4
and the first 24 records of groups 0..3. Records are kept **sorted by player id** (`squad_insert_player`)
and packed: the first record with `pid == 0` ends the list.

| Off | FAR (g=0,i=0) | Type | Meaning | Written by / read by |
|---|---|---|---|---|
| +0x00 | 774a | u8 | cards counter B (list column 13 prints `+0x01` then `+0x00`; header "GK RK" = yellow, red, so +0x00 = **red cards**) | 06af |
| +0x01 | 774b | u8 | **yellow cards** | 06af |
| +0x02 | 774c | u8 | not used in 2ecd | |
| +0x03 | 774d | u8 | goals, league ("ANZAHL DER TORE IN DER LIGA") | 06af col 9 |
| +0x04 | 774e | u8 | goals, DFB-Pokal | 06af col 10 |
| +0x05 | 774f | u8 | goals, Europapokal | 06af col 11 |
| +0x06 | 7750 | u8 | appearances, league; +1 on substitution in a league match (comp 0) | 06af col 3, 1560 |
| +0x07 | 7751 | u8 | appearances, DFB-Pokal; +1 on substitution with comp 1 | 06af col 4, 1560 |
| +0x08 | 7752 | u8 | appearances, Europapokal; +1 on substitution with comp >= 2 | 06af col 5, 1560 |
| +0x09 | 7753 | u8 | **status bits**: bits 0-1 `st` (0 = available, 1 = suspended "GESPERRT", 2 = injured "VERLETZT", 3 = unavailable, no text); bits 6-7 = transfer-listed / offered flag (name drawn in colour 0xb in transfer modes) | 3635, 3170, 06af, 1560 |
| +0x0a | 7754 | u8 | **lineup slot**: 0 = not selected, 1..11 = starting XI (always renumbered 1..k in record order, see 24cd), 12..`FAR:56ee` = substitutes | everywhere |
| +0x0b | 7755 | u8 | **contract length** in seasons ("V.DAUER", "n JAHR"/"JAHRE") | 37d8, 06af col 16 |
| +0x0c | 7756 | u8 | **loan flag** (" (AUSGEL.)" = on loan); `0x63` ('c') selects price mode 3 in player_value_calc | 37d8, 000e, 06af |
| +0x0d | 7757 | u8 | remaining suspension games ("NOCH n SPIELE", "GESP.(n)") | 000e, 06af, 3170 |
| +0x0e | 7758 | u8 | **tendency**, `random_range(35,65)`; class = (v-30)/13 (signed): 0 "FALLEND."/'-', 1 "KONSTANT."/'O', 2 "STEIGEND."/'+' | 3277:000a, 000e, 06af |
| +0x0f | 7759 | u8 | **player id** (index into the player table; 0 = empty record) | everywhere |
| +0x10 | 775a | u8 | current Kondition | 37d8, 06af col 7 |
| +0x11 | 775b | u8 | current Technik | 37d8, 06af col 7 |
| +0x12 | 775c | u8 | current Form | 37d8, 06af col 7 |
| +0x13 | 775d | u8 | **exhaustion** ("ERSCH\|PFUNG"); `random_range(80,120)` on insert; +`random_range(2,4)+4` on substitution in; >130 draws the tendency cell red (0x12) | 37d8, 1560, 06af |
| +0x14 | 775e | u8 | bit field from 3277:000a: bits 0-3 = `random_range(1,13)`, bits 4-5 = `random_range(1,3)`, bit 7 = `random_range(0,1)` (meaning owned by the transfer area) | 3277:000a |
| +0x15..+0x16 | | | not used in 2ecd | |
| +0x17 | 7761 | u8 | **injury type** k: `STR[53+k]` ("Oberschenkelzerrung" .. "Kreuzbandri~", 18 entries) | 000e |
| +0x18 | 7762 | u8 | transfer-offer state (list mode 3: 1..99 colour 3, bit 7 colour 0x11, `&0x7f > 99` with a message colours the name 0xb) | 06af |
| +0x19 | 7763 | i8 | **tactics-board column** 0..7, `0xff` = not placed | 3360, 1266, 29c6 |
| +0x1a | 7764 | i8 | **tactics-board row** 0..7 (0 = attack, 7 = goal) | 3360, 1266, 29c6 |
| +0x1b..+0x27 | | | not used in 2ecd | |
| +0x28 | 7772 | i32 | **value/salary** (from `player_value_calc`); list column 17 | 37d8, 06af |
| +0x2c..+0x2f | | | not used in 2ecd | |
| +0x30 | 777a | far ptr | message string (savegame relocates it on load) | 10ee, 06af |

### 1.3 Manager record fields used here (`MGR(m)` = FAR:2260 + 0x30a*m)

| Off | FAR | Meaning |
|---|---|---|
| +0x000 | 2260 | club id (byte) |
| +0x113 | 2373 | **tactic slider** 0..34 (`clamp_int(.,0,0x22)`), drawn as a bar; `manager_ai_weekly` adds `value - 16` to strength sums, so 16 is neutral |

### 1.4 Constant tables (DGROUP)

| DS | Bytes | Decoded | Use |
|---|---|---|---|
| 541c | `01 04 04 02  01 03 05 02  01 03 04 03` | formation counts [f][pos] for f=0..2: **4-4-2**, **3-5-2**, **3-4-3** (GK, DEF, MID, ATT) | 3360 (copied to a local) |
| 5428 | `00 03 02 01 00 00` | first board column for n players in a line, indexed by n=0..5 | 3360 |
| 542e | `07 06 03 00` | board row per position (GK 7, DEF 6, MID 3, ATT 0) | 3360 |
| 5410 | `15 25 16` | icon ids of the 3 screen buttons: list, tactics board, contracts | 1560 |
| 07b4 | `15 1a 19 18` | text colour per position (TOR 21, ABW 26, MIT 25, ANG 24) | 06af, 24cd, 3170 |
| 07c4 | `0b 12 39 0d 0c 0c 0e 24 0c 0e 0c 0c 0d 1b 28 0d 19 21` | list column widths in pixels, columns 0..17 | 000e, 06af |
| 53d8.. | `" ("`, `"("`, `")"`, ... | small strings (see code) | |

The formation table DS:541c has only three rows (DS:5428 follows directly). `DS:079e[2m]` holds
`1` = auto lineup off, or `2 + f` with formation f = 0..2. The initial value is 2 (4-4-2).

### 1.5 List columns (index = bit in the 32-bit column mask)

| Col | Header (`DS:4c48+4c`) | Hover text (`STR[20+col]`) | Cell |
|---|---|---|---|
| 0 | `"NR "` | "R]CKENNUMMER DES SPIELERS" | slot +0x0a (blank if 0; colour 4 if >11) |
| 1 | `"ART   "` | position, `STR[38+pos]` + " (AUSGEL.)" | `STR[4+pos]` "TOR/ABW/MIT/ANG" + marker pixel |
| 2 | `"NAME"` + 20 blanks (3 chars cut when mode != 0) | "NAME: " + name + " (" + age + " JAHRE)" + foot | player name |
| 3 | `"SP  "` | "ANZAHL DER LIGASPIELE" | +0x06 |
| 4 | DS:53ea `"      "` | "ANZAHL DER SPIELE IM DFB-POKAL" | +0x07 |
| 5 | DS:53f1 `"      "` | "... IM EUROPAPOKAL" | +0x08 |
| 6 | `"SP  "` (4c58) | "SPIELE GESAMT (LIGA U. POKAL)" | +0x06 + +0x07 + +0x08 |
| 7 | `"ST[RKEN  "` | "KONDITION,TECHNIK,FORM (avg)" | "K T F" = +0x10 " " +0x11 " " +0x12 |
| 8 | `"ST  "` | "DURCHSCHNTTS-ST[RKE" | (K+T+F)/3 |
| 9 | `"TO  "` | "ANZAHL DER TORE IN DER LIGA" | +0x03 |
| 10 | DS:53f8 `"      "` | "... IM DFB-POKAL" | +0x04 |
| 11 | DS:53ff `"      "` | "... IM EUROPAPOKAL" | +0x05 |
| 12 | `"TO  "` | "TORE GESAMT (LIGA U. POKAL)" | +0x03 + +0x04 + +0x05 |
| 13 | `"GK RK  "` | "ANZAHL DER GELBEN UND ROTEN KARTEN" | +0x01 " " +0x00 |
| 14 | `"STATUS      "` | "STATUS: ..." | status text |
| 15 | `"TD "` | "TENDENZ DES SPIELERS: ..." | '-', 'O', '+' |
| 16 | `"V.DAUER  "` | "DAUER DES VERTRAGES ..." | +0x0b " JAHR" ["E"] |
| 17 | `"     WERT"` (modes 1,2) / `"  GEHALT"` | "MONATLICHES GEHALT DES SPIELERS" | value " DM" |

The squad screen uses mask `0x0000f0c7`, i.e. columns 0,1,2,6,7,12,13,14,15.

---------------------------------------------------------------------------------------------------

## 2. Function list (segment 2ecd, all 19 Ghidra entries)

| Address | Ghidra name (now) | Old map name | Kind | Signature |
|---|---|---|---|---|
| 2ecd:000e | `squad_list_hover_info` | player_card | UI | `i16 far (i16 x0, i16 y0, i8 min_row, i8 max_row, i16 x1, i16 y1, u32 col_mask, rect far *btns, i8 nbtn)` |
| 2ecd:06af | `squad_list_draw` | screen_transfer_market_list | UI | `void far (i16 x0, i16 y, i8 first, i8 last, i16 x1, i16 y1, u32 col_mask, u8 mode)` |
| 2ecd:10ee | `squad_remove_player` | player_lineup_check | logic | `i16 far (u8 idx, u8 any_owner, u8 count)` |
| 2ecd:11f9 | `board_find_starter_at` | FUN | logic | `i16 far (u8 col, u8 row)` |
| 2ecd:1266 | `board_auto_place_player` | FUN | logic | `void far (u8 idx)` |
| 2ecd:132a | `board_draw_player_badge` | FUN | UI | `void far (picture far *pic6, u8 idx)` |
| 2ecd:1435 | `squad_draw_tactic_slider` | FUN | UI | `void far (bitmap far *pic37, i16 value)` |
| 2ecd:14c7 | `auto_lineup_disable` | auto_lineup_disable | logic+UI | `void far (u8 board_open)` |
| 2ecd:1560 | `screen_squad_lineup` | FUN | screen | `void far (u8 p1, u8 comp)` |
| 2ecd:18bf | `FUN_2ecd_18bf` | screen_lineup | **bogus entry** (inside 1560) | – |
| 2ecd:24cd | `lineup_renumber_starters` | FUN | logic+UI | `void far (u8 draw)` |
| 2ecd:2658 | `board_draw` | FUN | UI | `void far (picture far *pic6)` (called with a dummy byte first, see below) |
| 2ecd:26e6 | `board_open` | FUN | UI | `u8 far (picture far *pic6, u8 far *frame_btn_id, u8 far *form_btn_ids, rect far *form_rects, u8 full)` |
| 2ecd:2897 | `squad_draw_strength_age_line` | player_strength_line | UI | `void far (u8 idx, u8 board_open)` |
| 2ecd:29c6 | `board_handle_input` | FUN | UI+logic | `i16 far (picture far *pic6, bitmap far *save13, rect far *list_btns, rect far *form_rects, i16 hl_x1, u8 board_open, u8 p1)` |
| 2ecd:3170 | `squad_draw_status_cell` | FUN | UI | `void far (u8 idx)` |
| 2ecd:3360 | `auto_lineup` | squad_weekly_update | **logic** | `void far (void)` |
| 2ecd:3635 | `auto_lineup_pick_best` | FUN | **logic** | `i16 far (i8 pos, i16 far *thr)` |
| 2ecd:37d8 | `squad_insert_player` | transfer_list_add | **logic** | `u8 far (u8 pid, u8 contract, u8 loan, i32 unused, u8 limit)` |

Library and other-area callees: `ui_set_font` 176b:0c91, `ui_hit_test_buttons` 16c7:0002,
`ui_draw_button` 16c7:00a7, `ui_redraw_buttons` 16c7:0563, `ui_draw_text_at` 16c7:07d5
`(rp, str, x, y, colour(0->0xff))`, `ui_fill_rect` 16c7:083c `(rp, x0, y0, x1, y1, colour)`,
`ui_draw_text_centered_shadow` 16c7:087c `(rp, str, x0, x1, y, c_shadow, c_front)` (draws `c_front` at
the centred x unless it is 0xff, 0 means 0xff, then `c_shadow` at x-1), `ui_draw_line` 16c7:09b0
`(rp, x0, (u8)y0, x1, (u8)y1, colour)`, `copy_protection_penalty` 16c7:09f9, `vga_load_picture`
176b:0356 `(id, w, h, depth, flag)`, `format_long_thousands` 176b:0681 (i32 to string; field width from
DS:079c), `ui_dialog_box` 176b:0e57 `(line1, line2)` (always returns 1), `input_wait_release` 176b:0e27,
`bitmap_free` 176b:0dca, `screen_fade` 176b:1416, `ui_blit_transparent` 176b:02e4
`(src_bm, sx, sy, dst_rp, dx, dy, w, h)`, `blit_bitmap` 4930:0991
`(src_bm, sx, sy, dst, dx, dy, w, h, minterm)`, `blit_bitmap_scaled_or_copy` 4930:0849,
`rastport_set_draw_mode` 4930:052d (2 = XOR, 0 = normal), `rastport_put_pixel` 4930:03ce,
`mem_alloc` 4930:010c `(size, flag)` (zero-filled), `mem_free` 4930:0188, `far_memcpy_bytes` 45ac:304a
`(src, dst, n)`, CRT `strcpy/strcat/strlen`, `_aFlmul/_aFldiv/_aFlshr`, `clamp_int` 4091:1009
`(int far *v, lo, hi)`, `text_to_upper_de` 4091:105d, `squad_count_by_flag` 4091:1109,
`message_find_owner` 4091:0044, `ui_wait_with_highlight` 4091:1722, `FUN_212a_00b4` (squad size incl.
owned players), `1310:1c22` (foot suffix), `player_value_calc` 3277:25de,
`player_traits_randomize` 3277:000a, `contract_negotiation` 3277:2a8f, `event_helper_25d5` 1cb5:25d5,
`manager_ai_weekly` 1f9d:0002.

---------------------------------------------------------------------------------------------------

## 3. Game logic (bit-exact)

Port status: §3 is ported as `bmp_game::squad` (`crates/bmp-game/src/squad.rs`), together with
`squad_count_by_flag` 4091:1109 (mode 0 only, as `squad_count_listed`), `count_players_of_owner` 212a:00b4, `player_init_random_attrs`
3277:000a and `event_helper_25d5` 1cb5:25d5 (`squad_fix_position_mismatch`, season.md §4.4).
`squad_insert_player` calls `bmp_game::transfer::player_value_calc` (3277:25de) directly.
Deliberate deviations: `auto_lineup` skips the slot-13 write when `bench_best` was never assigned (§3.1), and `squad_remove_player` does not call
`message_find_owner` (the message table is not modelled yet).
§4 (the screen and its drawing helpers) is ported as `crates/bmp/src/game/squad_screen.rs`;
`ui_dialog_box` and `contract_negotiation` are still stubs there.

### 3.1 `auto_lineup` 2ecd:3360

Callers: `start_menu_new_or_load` 18bc:2184 (18bc:392f), `season_main_loop` (2a58:3229, 2a58:3717),
`office_menu_dispatcher` (18bc:1186), `random_weekly_events` 1cb5:13bd (1cb5:1a3c, 1c1a, 25c8),
1cb5:2b88 (2bf0), `board_handle_input` (formation change), `screen_transfer_market` 3277:04a5
(084a, 19a4). Callees: `auto_lineup_pick_best`, `far_memcpy_bytes`, `event_helper_25d5`,
`lineup_renumber_starters(0)`. **No RNG** (`event_helper_25d5` is `squad_fix_position_mismatch`, season.md §4.4, which uses none).

```c
void auto_lineup(void) {
    i16  thr   = -1;          /* bp-0x16, passed to pick_best; never changes while it is -1 */
    u8   slot  = 1;           /* bp-0x20 */
    i8   f     = (i8)DS_079e[2*m] - 2;      /* formation 0..2 */
    i8   counts[12];          /* bp-0x0e */
    i8   off[4] = {0,0,0,0};  /* bp-0x12: next column offset per position */
    i8   bench_best;          /* bp-0x1e: UNINITIALIZED (see note) */
    if (f == -1) return;      /* DS:079e[2m] == 1: auto lineup disabled */

    for (i16 i = 0; i < 24; i++) SQ(m,i).slot = 0;      /* record 24 is NOT cleared */
    far_memcpy_bytes(DS:541c, counts, 12);

    for (i16 pos = 0; pos < 4; pos++) {
        DS_a38f = counts[f*4+pos];                        /* dead store, never read */
        if (pos == 2 && DS_0664[m] < 11) DS_a38f--;       /* dead store */
        while (counts[f*4+pos] != 0) {
            i16 idx = auto_lineup_pick_best((i8)pos, &thr);
            if (idx == -1) break;
            REC *r = &SQ(m, idx);
            r->slot = slot++;                              /* u8 */
            counts[f*4+pos]--;
            r->row = DS_542e[pos];
            r->col = DS_5428[ DS_541c[f*4+pos] ] + off[pos];   /* original count, not the local copy */
            if (f == 0) {                                  /* 4-4-2 only: push full-backs up */
                if (slot == 3) { r->row--; r->col++; }     /* starter #2 */
                if (slot == 6) { r->row--; r->col--; }     /* starter #5 */
            }
            off[pos] += 2;
            if (DS_541c[f*4+pos] == 5 && off[pos] != 2 && off[pos] != 6) off[pos]--;
        }
    }

    if (FAR_56ee == 15) {                                  /* 4 substitutes allowed */
        thr = -1; slot = 12;
        for (i16 k = 0; k < 4; k++) {
            i16 idx = auto_lineup_pick_best((i8)k, &thr);
            if (idx == -1) break;
            SQ(m,idx).slot = slot++;                       /* 12 GK, 13 DEF, 14 MID, 15 ATT */
        }
    } else {                                               /* 2 substitutes */
        thr = 0; slot = 12;
        for (i16 k = 0; k < 4; k++) {
            i16 prev = thr;
            i16 idx  = auto_lineup_pick_best((i8)k, &thr);
            if (slot > 12 && prev != thr) bench_best = (i8)idx;
            if (idx == -1) break;
            if (slot == 12) { thr = 0; SQ(m,idx).slot = 12; slot = 13; }
        }
        SQ(m, (i16)bench_best).slot = slot;                /* the strongest DEF/MID/ATT gets 13 */
    }
    event_helper_25d5();
    lineup_renumber_starters(0);
}
```

Resulting board layout for the starters (before any user changes):
* 4-4-2: GK (3,7); DEF cols 0,2,4,6 on row 6, with #2 moved to (1,5) and #5 to (5,5); MID cols
  0,2,4,6 on row 3; ATT cols 2,4 on row 0.
* 3-5-2: DEF 1,3,5; MID 0,2,3,4,6 (because of the 5-player correction); ATT 2,4.
* 3-4-3: DEF 1,3,5; MID 0,2,4,6; ATT 1,3,5.

Notes for a port:
* `bench_best` is an uninitialized stack byte if no pick in the 2-sub branch raises `thr` (for example
  when only goalkeepers are left, or when the first pick already fails). The original then writes slot
  13 to a random record. A port should skip that write when `bench_best` was never assigned, and flag
  this as a deliberate deviation.
* `slot` counts every assigned starter, so after the XI it is 12 only if all 11 were found.

### 3.2 `auto_lineup_pick_best` 2ecd:3635

Returns the index (0..23) of the best unselected, available player for position `pos`. If none is
found, it cycles through the other positions. Callers: `auto_lineup` only.

```c
i16 auto_lineup_pick_best(i8 pos, i16 far *thr) {
    i16 best = -1, best_val = 0;                 /* bp-8, bp-0xc */
    i8  start = pos;  i8 step = 1;               /* bp-6, bp-2 */
    u8  gk_seen = 0, any = 0;                    /* bp-4, bp-0xa */
    if (pos == 3 || (pos == 2 && (i8)DS_079e[2*m] < 4)) step = -1;
    /* 3-4-3 (value 4) searches forward from MID; 4-4-2/3-5-2 search backward */
    goto check;
    for (;;) {
        for (i16 i = 0; i < 24; i++) {
            REC *r = &SQ(m,i);
            if (r->pid == 0 || r->slot != 0) continue;
            u8 st = r->status & 3;
            if (!(st == 0 || (st == 1 && FAR_513e != 0))) continue;
            any = 1;
            u8 ppos = PL(r->pid).poscode / 25;
            if (ppos != (u8)pos) continue;               /* compared as 16-bit, pos sign-extended */
            i16 val = ((u16)r->kond + r->tech + r->form) / 3;   /* signed idiv, operands are u8 */
            if (ppos == 0 && start != 0) {              /* a goalkeeper found while seeking an outfield player */
                if (!gk_seen) { gk_seen = 1; val = -1; } else val = 1;
            }
            if (best_val > val) continue;               /* ties: the later record wins */
            if (*thr != -1 && *thr < val) *thr = val;
            best_val = val; best = i;
        }
        pos += step;  if (pos == 4) pos = 0;  if (pos == -1) pos = 3;
        if (pos == start && gk_seen) break;
        if (!any) break;
    check:
        if (best != -1) break;
    }
    return best;
}
```

Notes:
* "Available" is `st == 0`, or `st == 1` (suspended) while `FAR:513e != 0`. FAR:513e is set elsewhere;
  it probably means "suspensions do not apply to this match".
* Records 24 are never considered.
* When a goalkeeper is the only remaining player for an outfield search, the first one gets value -1
  (never chosen because `best_val` starts at 0) and later ones get value 1.
* The loop cannot hang as long as every position code is below 100: if `any` is set, some position has a
  candidate, and the outfield-to-GK case ends through `gk_seen`. An available player with a code of 100
  or more sets `any` but never matches `pos`, so the original loops forever; the game never generates
  such a code. The port rejects on load a save in which any player holds one, a stricter rule than
  this hang condition (state.md §1.1).

### 3.3 `squad_insert_player` 2ecd:37d8

Inserts player `pid` into group `FAR:304a` (a manager's squad or, with 304a = 4, the transfer list) in
sorted position. Callers: `start_menu_new_or_load` (18bc:32ac: `(pid, 1, 0, 15000L, 24)`),
`random_weekly_events` 1cb5:0012 (0521: `(pid,0,0,0L,24)`; 07c9: `(pid,1,0,0L,25)`),
`screen_transfer_market` 3277:04a5 (0f82, 1716), 3277:1e38 (200b), `mana_load_player_names` (0645).

```c
u8 squad_insert_player(u8 pid, u8 contract, u8 loan, i32 unused, u8 limit) {
    i8 idx = 0;
    if (limit == 12 && SQ(m,11).pid != 0) {          /* FAR:7995 + 0x514*m */
        ui_dialog_box(LBL@4d2c "Schon 12 Mann auf", LBL@4d30 "dem Transfermarkt");
        return 0x7f;
    }
    if ((i8)FAR_304a != 4 && PL(pid).club != MGR(m).club
        && (u8)FUN_212a_00b4(m) > (i16)limit - 1) {
        ui_dialog_box(LBL@4d34 "Schon 24 Mann", LBL@4d38 "im Team");
        return 0x7f;
    }
    void far *tmp = mem_alloc(0x34, 1);               /* zeroed buffer */
    while (SQ(m,idx).pid != 0 && SQ(m,idx).pid < pid && idx < 24) idx++;   /* u8 compares */
    if (SQ(m,idx).pid != 0)
        for (i8 k = 23; k >= idx; k--) far_memcpy_bytes(&SQ(m,k), &SQ(m,k+1), 0x34);
    far_memcpy_bytes(tmp, &SQ(m,idx), 0x34);           /* clear the record */
    mem_free(tmp, 0x34);
    REC *r = &SQ(m,idx);
    r->pid  = pid;
    r->exhaustion = (u8)random_range(0x50, 0x78);     /* RNG #1 */
    r->tendency   = 0;
    player_traits_randomize(r);                        /* 3277:000a, RNG #2..#5, see below */
    r->contract = contract;
    r->loan     = loan;
    r->kond = PL(pid).kond; r->tech = PL(pid).tech; r->form = PL(pid).form;
    r->value = player_value_calc(idx, (loan == 0x63 ? 1 : 0) * 2 + 1);  /* (u8 idx, u8 mode) -> i32 */
    if ((i8)FAR_304a != 4 && PL(pid).club != MGR(m).club) {
        PL(pid).cnt_22 = 0; PL(pid).cnt_23 = 0;
        PL(pid).club = MGR(m).club;
    }
    return (u8)idx;
}
```

`player_traits_randomize` (3277:000a, transfer area) does, in this order:
`r->tendency = random_range(35,65); r->b14 = random_range(1,13); r->b14 |= random_range(1,3) << 4;
r->b14 |= random_range(0,1) << 7;`. `player_value_calc` draws no random numbers in the salary modes used here (1 and 3).
Note that the 12-player check looks at record 11 of the current group whatever the group is. The "24"
check counts with `FUN_212a_00b4(m)`: the number of records in squads 0..`DS:07ab`-1 and group 4 whose
player is owned (`+0x21`) by `m`, plus every record of group `m`. The `i32` argument is never read; the
compiler reuses its stack slot for the returned value.

### 3.4 `squad_remove_player` 2ecd:10ee

```c
i16 squad_remove_player(u8 idx, u8 any_owner, u8 count) {
    u8 pid = SQ(m,idx).pid;
    if (pid == 0) return 1;
    if (PL(pid).owner != (i8)FAR_304a && any_owner == 0) return 0;
    if (SQ(m,idx).msg != NULL) message_find_owner(SQ(m,idx).msg);
    for (u8 k = idx; (i16)count - 1 >= k; k++) far_memcpy_bytes(&SQ(m,k+1), &SQ(m,k), 0x34);
    SQ(m,count).pid = 0;      /* only the id is cleared; the rest of the record keeps stale data */
    return 1;
}
```
Callers pass `count` = 24 for a squad, or `(2 - (g == 4)) * 12` (1cb5:0854). With count 24 the last copy
reads record 24 (the 25th record of the group).
Callers: 1cb5:0012 (0864, 0a54, 1263), 3277:1e38 (1e93, 20c7), 3277:04a5 (0e6d, 0fde, 137a, 18a2, 19bd).

### 3.5 `lineup_renumber_starters` 2ecd:24cd

```c
void lineup_renumber_starters(u8 draw) {
    DS_079c = 2;  u8 n = 1;
    for (u8 i = 0; i < 24; i++) {
        if (draw) ui_fill_rect(rp, 6, (i+5)*6, 16, i*6+0x23, 8);
        u8 s = SQ(m,i).slot;
        if (s == 0) continue;
        if (s <= 11) {
            if (draw) ui_draw_text_at(rp, ltoa(n), 6, i*6+0x22, DS_07b4[PL(pid).poscode/25]);
            SQ(m,i).slot = n++;
        } else if (draw) ui_draw_text_at(rp, ltoa(s), 6, i*6+0x22, 4);
    }
    DS_079c = 1;
}
```
Starters are therefore always numbered 1..k in record (player-id) order.

### 3.6 Tactics-board helpers

`board_find_starter_at(col,row)` 2ecd:11f9: returns the first `i` in 0..23 with slot in 1..11,
`(i16)(i8)r->col == col` and `(i16)(i8)r->row == row` (col/row zero-extended), else 0x7f.

`board_auto_place_player(idx)` 2ecd:1266: only if `SQ(m,idx).col == 0xff`:
```c
u8 row = (u8)((i16)(7 * PL(pid).poscode) / -100) + 7;   /* 8-bit mul gives u16, idiv by -100 */
if (row == 7) row = 6;
for (;;) {
    for (u8 c = 0; c < 8; c++)
        if (board_find_starter_at(c, row) == 0x7f) { r->col = c; r->row = row; return; }
    if (++row == 8) row = 0;
}
```
(GK: row 6; DEF: rows 6..5; MID: 4..3; ATT: 2..1.)

---------------------------------------------------------------------------------------------------

## 4. Screen `screen_squad_lineup(p1, comp)` 2ecd:1560

Callers:
* office menu 2, case 1 (18bc:1af5 → 18bc:1b01): `(0, 0xff)` = squad / lineup.
* office menu 2, case 2 (18bc:1afc): `(0x63, 0xff)` = contracts (opens contract negotiation directly).
* `2a58:1f75` (match substitution, called from the live match): `(DS:0664[m], comp)`, where
  `DS:0664[m]` is the number of players allowed on the field (11 minus send-offs) and `comp` is the
  competition (0 league, 1 DFB-Pokal, 2 Europapokal). 2a58:1f75 sets `FAR:304a` for the call.

Locals: `maxp = p1 ? p1 : 11` (bp-6); `tab = (p1 == 0x63) ? 2 : 0` (bp-0xb6; p1 is then set to 0);
`board_open = 0` (bp-0xa); `first = 1` (bp-0x74); `saved_col = 0xff`, `saved_row` (bp-2/bp-4, the board
spot a removed starter vacated); `next_res` (bp-0xba, uninitialized on the first pass).

### 4.1 Setup
1. `pic37 = vga_load_picture(0x25, 0,0,0, 1)` (PIC\37.VGA, background with palette); `bm37 = *pic37`.
   `save13 = vga_load_picture(0, 13, 13, 5, 0)` (empty 13x13 bitmap). `pic6 = vga_load_picture(6, 0,0,0,1)`
   (tactics-board sheet).
2. `copy_protection_penalty(&CLUB(MGR(m).club).name, 0, 0, 0)`.
3. Frame button (2,7,w 0xee,h 0xb5) into `btn[0]` (overwritten next, so it is not clickable).
4. If `tab == 0`: `squad_list_draw(6, 0x12, 0, 24, 0xec, 0xb0, 0x0000f0c7, 0)`.
5. Icon buttons `btn[i]`, i = 0..2: `ui_draw_button(rp, 0x10c, 0x29 + 45*i, 0, DS:5410[i] | (tab==i ? 0x8000 : 0), &btn[i], 0, 8)`:
   0x15 = squad list, 0x25 = tactics board, 0x16 = contracts. `btn[3]` = exit icon at (0x10c, 0xb0),
   icon `0x22` (comp == 0xff) or `0x23` (match). The exit icon is the fourth iteration of the same
   formula, so it sits at (0x10c, 0xb0).
6. `squad_draw_tactic_slider(bm37, MGR(m).slider)`; `nmax = squad_count_by_flag(m,0) - 1`;
   `squad_list_hover_info(-1,...)` (reset), `board_handle_input(NULL, ...)` (reset), `input_wait_release()`,
   `FAR:2ea2 = 0`.
7. If `comp != 0xff`: font 3, club name centred between x 2..0xf0 at y 0xe4 (`c_shadow` 0xb, `c_front` 0 → 0xff).
8. If `tab == 0`: `screen_fade(FAR:2e70, 1)`.

### 4.2 Main loop (2ecd:185a)
```
loop:
  if (first) goto renumber;
poll:
  si = 0x7f;
  if (tab == 0) si = squad_list_hover_info(6, 0x1e, 0, nmax, board_open ? 0xaf : 0xec, 0xb8, 0xf0c7, btn, 4);
  if (board_open) { di = board_handle_input(pic6, save13, btn, form_rects, 0xb3, board_open, p1); if (di) saved_col = 0xff; }
  mouse = FAR:6dc0 -> {y = [0], x = [1]};
  if (FAR:2ea2 == 1 && 0x10a < x < 0x13f && 4 < y < 0x24) {           /* tactic slider */
      v = x - 0x113; clamp_int(&v, 0, 0x22); MGR(m).slider = v; squad_draw_tactic_slider(bm37, v);
  } else if (FAR:2ea2 == 1 && si == 0x7f && di == 0) auto_lineup_disable(board_open);
  if (tab == 0 && si == 0x7f) goto poll;
  next_res = 12;  if (tab) si = 0x67;
renumber:                     /* 2ecd:19a0 */
  font 2;
  do {
     changed = 0;  cnt = squad_count_by_flag(m,0);
     for (di = 0; di < cnt; di++) {
        if (SQ(di).slot > FAR:56ee && p1 == 0) {           /* slot beyond the allowed bench */
           t = 12; while (t < 16 && exists j in 0..cnt-1 with SQ(j).slot == t) t++;   /* the scan reads 0..cnt+1, but a hit at cnt or later does not count */
           if (t == FAR:56ee + 1) t = 0;
           SQ(di).slot = t;  ui_fill_rect(rp, 6, (di+5)*6, 16, di*6+0x23, 8);
           lineup_renumber_starters(1);  if (di < 9 || !board_open) squad_draw_status_cell(di);
        }
        if (SQ(di).slot == next_res) { changed = 1; next_res++; }
     }
  } while (changed);
  if (first) { first = 0; goto poll; }
  -> click handling (4.3), then button handling (4.4), then back to loop
```
`next_res` ends as the first free substitute number (>= 12, consecutive scan). On the first pass it
starts uninitialized. That only changes how many times the idempotent loop repeats.

### 4.3 Clicking a player row (si = 0..23)
```
msg = 0;  set_draw_mode(0);
if (DS:079e[2m] > 1) { ui_dialog_box("Auto-Aufstellung", "ist aktiviert !"); goto loop; }   /* LBL 4d00/4d08 */
st = SQ(si).status & 3;
if ((st == 1 && FAR:513e == 0) || st == 2) msg = ui_dialog_box("Spieler ist", STR[133 - st]);  /* "GESPERRT." / "VERLETZT " */
if (p1 && SQ(si).slot == 0) msg = ui_dialog_box("Spieler ist", "nicht aufgestellt");
if (p1 && FAR:5396[2m + (PL(pid).poscode != 0)] == 0) msg = ui_dialog_box("Keine Auswechslung", "mehr m|glich");
if (msg) goto buttons;
slot = SQ(si).slot;
if (((p1 == 0 && slot != 0) || (p1 != 0 && slot < 12)) && FAR:2ea2 == 1) {         /* REMOVE (2ecd:1d2d) */
    n = #records 0..23 with slot 1..11;
    if (maxp > n && slot in 1..11) { msg = ui_dialog_box("Nicht weniger als " + ltoa(maxp-1), "Spieler einsetzen !"); goto buttons; }
    if (slot < 12) { saved_col = SQ(si).col; saved_row = SQ(si).row; }
    SQ(si).slot = (p1 != 0) ? next_res : 0;       /* in a match the player goes to the bench */
    ui_fill_rect(rp, 6, (24+5)*6, 16, 24*6+0x23, 8);   /* BUG: uses the loop counter (24), not si */
    lineup_renumber_starters(1); if (si < 9 || !board_open) squad_draw_status_cell(si);
}
else if (p1 == 0 && FAR:2ea2 == 3 && comp == 0xff && slot == 0) {               /* ADD AS SUBSTITUTE (1ec3) */
    if (FAR:56ee < next_res) { msg = ui_dialog_box("Maximal " + ltoa(FAR:56ee - 11), "Reservespieler !"); goto buttons; }
    SQ(si).slot = next_res; SQ(si).col = 0xff;
    ui_fill_rect(rp, 6, (si+5)*6, 16, si*6+0x23, 8); ui_draw_text_at(rp, ltoa(next_res), 6, si*6+0x22, 4);
    if (si < 9 || !board_open) squad_draw_status_cell(si);
}
else if (!((slot != 0 || p1 != 0) && slot <= 11) && FAR:2ea2 == 1) {             /* BRING INTO XI (2042) */
    n = #starters;
    if (maxp - 1 < n) { msg = ui_dialog_box("Nicht mehr als " + ltoa(maxp), "Spieler einsetzen !"); goto buttons; }
    if (p1) {                                   /* match substitution */
        FAR:5396[2m + (PL(pid).poscode != 0)]--;
        if (comp == 0) PL(pid).cnt_23++;
        SQ(si).games[(comp >= 1) + (comp > 1)]++;      /* +0x06/+0x07/+0x08 */
        SQ(si).exhaustion += (u8)(random_range(2,4) + 4);   /* the only RNG call of the screen */
        FAR:90c6[m]++;                           /* substitutions made (not in the savegame) */
    }
    if (saved_col != 0xff) { SQ(si).col = saved_col; SQ(si).row = saved_row; saved_col = 0xff; }
    else SQ(si).col = 0xff;
    SQ(si).slot = 11;  board_auto_place_player(si);
    lineup_renumber_starters(1); if (si < 9 || !board_open) squad_draw_status_cell(si);
}
```
Mouse event codes (`FAR:2ea2`): 1 = left button; 3 is the other button used for "add as substitute"
(MEMORY_MAP lists 2 = right; the button code mapping of `mouse_pop_button_event` should be checked).

### 4.4 Buttons (si = 0x65..0x68)
After a row click, and also on every pass:
* if `!msg && si < 100`: if board open, `board_draw(pic6)`; if `si < 8 || !board_open`, `squad_draw_status_cell(si)`
  (rows 8+ are hidden behind the board while it is open).
* `si == 0x66` (tactics board) and the board is closed: `board_open = board_open(pic6, &frame_id,
  form_ids, form_rects, comp == 0xff)` (returns 1); `board_handle_input(NULL…)`; `input_wait_release()`.
* `si == 0x67` (contracts), board closed and `p1 == 0`: redraw icon 2 pressed (`0xa000`) and icon 0
  released (`0x2000`); `r = contract_negotiation(-1, btn, save13, pic6)`; if `r == 0` then `si = 0x68`
  (leave). Otherwise clear (3,8)-(0xef,0xb8) colour 8 and redraw the list; `tab = 0`.
* `si == 0x65` (list) and the board is open: `board_open = 0; ui_redraw_buttons(frame_id, 0)`; if
  `comp == 0xff`, restore the bottom area from the work bitmap (`blit_bitmap_scaled_or_copy(FAR:56fa,
  0,0xc0, FAR:a6ca, 0,0xc0, 0xbb,0x2a, 0xc0,7,0,0)`); redraw the status cells of rows 8..23 with pid != 0.
* `si == 0x68` (exit) and the board is closed: `ui_wait_with_highlight(0x10c, 0xb0)`, free pic6,
  pic37 and save13, `manager_ai_weekly(m, p1 != 0)`, return. Any other case goes back to the loop.

### 4.5 `squad_list_hover_info` 2ecd:000e (UI)
* `x0 == -1`: reset `DS:53e6 = 0` (last column), `DS:53e8 = -1` (last highlighted row); returns garbage.
* The list polls the mouse inside `ui_hit_test_buttons(btns, nbtn, 1)`; hover changes therefore only
  show after a pointer motion has been polled (a scripted click without a prior move skips the hover
  of the target row).
* Font 2. `h = ui_hit_test_buttons(btns, nbtn, 1)`; if nonzero, return `h + 0x64`.
* `row = (i16)(mouse_y - y0) / 6` (truncating); `px = mouse_x - x0`. Column: `acc = 0; for col in
  0..17: if (mask >> col) & 1 { acc += width[col]; if acc > px break }`. The cell is valid if
  `0 <= mouse_x <= x1`, `min_row <= row <= max_row` and `col != 18`; otherwise row = col = 0x7f.
* Click (`2ea2 == 1 or 3`) on a valid cell: un-highlight the last row (XOR fill
  `(x0, 6*last+y0)-(x1, 6*last+y0+4)` colour 0x1e if `6*last+y0 > 27`), set `53e8 = -1`, **return row**.
* Hover: if the column or row changed, clear the info line `(x0, y1-7)-(x1, y1+1)` colour 8 and print
  the hover text (§1.5) centred x0..x1 at y1 (`c_shadow` 1). Move the XOR row highlight. Return 0x7f.
* Status hover text: "STATUS: " + ("RESERVE" if slot > 11; else "IM TEAM"/"H[NGT HALT SO RUM..." if
  available; else ["NOCH n SPIELE "] + "GESPERRT."/"VERLETZT " + ["(" + upper(injury) + ")"]).
  Tendency text appends " (ERSCH|PFUNG:" + exhaustion + ")" only when `x1 == 0xec`.

### 4.6 `squad_list_draw` 2ecd:06af (UI; also used by the transfer screens)
`mode`: 0 = own squad, 1/2 = transfer market (title "TRANSFERMARKT", value column "WERT", mode 2 shows
value/3), 3 = offers view, 4 = like 0 but with the name highlight flag. Header: if `first != last`,
draw the title ("IHRE MANNSCHAFT" or "TRANSFERMARKT") centred at y-2 (colours 0xb/7), `y += 6`, clear
`(x0, y+3)-(x1, y1-1)` colour 8 if `y+3 > 27`. Then for `si = first-1 .. last`: row -1 (the header) is
the concatenation of the column headers drawn at (x0,y) colour 0xb, with lines at y+2 and at y1, then
`y += 10`. Each data row draws the cells of the mask columns at x = x0 + sum of the widths of the
previous drawn columns, in colour `DS:07b4[pos]` (overrides: slot > 11 in NR → 4; status not available →
0x12; exhaustion > 130 → tendency 0x12; mode 3 offer colours), then `y += 6`. Empty records
(`pid == 0`) draw nothing. Column 17 value = `value * (i32)DS:2252 / (i32)DS:224e` (MSC long mul/div)
+ " DM"; if `DS:224d != 0`, x += 8 and the empty prefix string LBL@4f78 is used. In modes 1/2, for a
player not owned by group 4, the value is recomputed with `FAR:304a` temporarily 4:
`player_value_calc(si, 0)`. `DS:079c` is set to the number field width (2, 1 or `7 - DS:224d`).

### 4.7 Other UI helpers
* `squad_draw_status_cell(i)` 2ecd:3170: clear `(0xb1, (i+5)*6)-(0xd8, i*6+0x23)` colour 8; text at
  (0xb6, i*6+0x22): available → `STR[8 + c]` with c = 2 (slot 0, " "), 3 (starter, "IM TEAM"),
  4 (slot > 11, "RESERVE"), colour by position; otherwise `STR[7+st]` ("GESP.", "VERL.", " ") with
  "(n)" for suspended players, colour 0x12.
* `squad_draw_strength_age_line(i, board_open)` 2ecd:2897: "ST[RKE:" + (K+T+F)/3 + ", " + age +
  " JAHRE" + foot suffix, centred x 6..(board_open ? 0xaf : 0xec) at y 0xb8.
* `squad_draw_tactic_slider(bm37, v)` 2ecd:1435: blit bm37 (0,0) 0x33x0x1f to (0x10b,5); lines from
  x=0x113 to 0x113+v at y 0x19 and y 0x1a, colour 0x1d.
* `auto_lineup_disable(board_open)` 2ecd:14c7: only if `DS:079e[2m] > 1`. If the board is open, redraw
  the pressed formation button at `x = 60*DS:079e[2m] - 0x74`, y 0xc0, released (icon flag 0x2000).
  Show "Auto-Aufstellung" / "ist deaktiviert !", then set `DS:079e[2m] = 1`.
* `board_draw(pic6)` 2ecd:2658 (the call site pushes an extra leading byte 0xff, harmless): blit pic6
  (0,0) 0x78x0x78 to (0xbc,0x5d), then draw a badge for each starter.
* `board_draw_player_badge(pic6, i)` 2ecd:132a: if slot != 0, copy pic6 (13,121) to (0,121), 13x13
  (clean template); print the slot number into pic6 at (slot < 10 ? 5 : 2, 0x81) colour 0xb; blit pic6
  (0,121) 13x13 transparently to the screen at (col*16 + 0xc2, row*14 + 0x63).
* `board_open(...)` 2ecd:26e6: frame button (0xb5,0x56,0x84,0x84) → id stored to `*frame_btn_id`; board
  blit as in board_draw; font 2, draw mode 0; if `form_rects` and `full`: 3 formation buttons at
  (60k+4, 0xc0), pressed if `DS:079e[2m] - k == 2`, each with the label graphic from pic6 (0x79, 24k)
  0x20x0x17 drawn at (60k+0xd, 0xc9); badges of all starters; returns 1.
* `board_handle_input(...)` 2ecd:29c6: with `pic6 == NULL`, reset (`FAR:54a0 = 1` (nothing held),
  `DS:a38e = 0xff` (highlighted list row), `DS:5416 = -1` (cursor x), `DS:a38d = 0`) and return 0.
  Formation click (`p1 == 0`, left click, `k = ui_hit_test_buttons(form_rects,3,0)`): redraw the old and
  new buttons, `DS:079e[2m] = k+1`, run `auto_lineup()`, `lineup_renumber_starters(1)`, redraw the status
  cells of rows 0..7 and the board, reset, and return 1. Cursor: if the mouse is inside
  (0xc2,0x63)-(0x134,0xd5) exclusive, `col = min(trunc((x-0xc2)/16), 6)`,
  `row = min((y-0x63)/14, 7)`, cursor centre (col*16+0xc9, row*14+0x6a) kept in DS:5416/DS:a38a, with
  the background saved in `save13` from FAR:a6ca and the cursor sprite pic6 (0x34,0x79) 13x13. The
  starter under the cursor (`board_find_starter_at`) gets its list row XOR-highlighted
  (`(4,(r+5)*6)-(hl_x1, r*6+0x22)` colour 0x1e) and its strength/age line shown. Left click:
  `auto_lineup_disable(board_open)`; if nothing is held, pick up the starter under the cursor
  (`DS:a38c`, origin DS:a388/a389, sprite pic6 (0x1a,0x79)), `FAR:54a0 = 0`. If a player is held:
  `FAR:54a0 = 1`; drop on an empty spot (move col/row, redraw the badge, restore the old spot from pic6
  (col*16+6, row*14+6)) or on another starter (swap the two col/row pairs, redraw both badges). Returns
  1 after a pick, drop or formation change, otherwise 0.

---------------------------------------------------------------------------------------------------

## 5. Globals used

| Address | Type | Suggested name | Save block |
|---|---|---|---|
| FAR:304a | i8 | `cur_manager` | 3 |
| FAR:2260+0x30a*m (+0x000, +0x113) | struct | `managers[m].club_id`, `.tactic_slider` | 13 |
| FAR:3066+0x22*c | struct[200] | `clubs[c]` (name at +0) | 15 |
| FAR:57dd+0x25*p | struct[151] | `players[p]` (§1.1) | 17 |
| FAR:774a+0x34*(g*25+i) | struct[5][25] | `squads[g][i]` (§1.2) | 18 |
| FAR:513e | u8 | `suspended_players_allowed` | – (not in a block; set by 18bc:0b5f / 2a58:0ca3) |
| FAR:56ee | i8 | `max_lineup_slot` (11 + allowed substitutes: 13 or 15) | – (set by 18bc:0b5f) |
| FAR:5396+2m+k | u8[4][2] | `subs_remaining[m][is_outfield]` | – |
| FAR:90c6+m | u8[4] | `subs_made[m]` | – (block 56 FAR:90c2 ends before it) |
| FAR:54a0 | u8 | `board_nothing_held` | – |
| FAR:2ea2 | i16 | `mouse_event` | – |
| FAR:6dc0 | far ptr | `mouse_pos` ([0] = y, [1] = x as used here) | – |
| FAR:2e70 | far ptr | `pointer_state` (+4 = y, +6 = x) | – |
| FAR:2f28 | far ptr | `screen_rp` | – |
| FAR:56fa, FAR:a6ca | far ptr | `work_bitmap`, `screen_backup_bitmap` | – |
| FAR:5358 | i8 | `saved_manager` (owner test in the transfer list) | 4 |
| DS:079e+2m | u8[4] | `auto_lineup_formation` (1 = off, 2..4) | 5 |
| DS:0664+m | u8[4] | `players_on_field_limit` (init 11) | – (not saved; block 41 starts at DS:066c) |
| DS:07ab | u8 | `num_managers` | 7 |
| DS:079c | u8 | `number_field_width` (MEMORY_MAP calls it "picture loading" – doubtful) | – |
| DS:07b4 | u8[4] | `position_colour` | const |
| DS:07c4 | u8[18] | `list_column_width` | const |
| DS:224d | u8 | `currency_mode` (?) | – |
| DS:224e / DS:2252 | i32 | value display divisor / multiplier | – |
| DS:5410 | i8[3] | `squad_screen_icons` | const |
| DS:5416 | i16 | `board_cursor_x` | – |
| DS:53e6 / DS:53e8 | i16 | `hover_last_col` / `hover_last_row` | – |
| DS:541c / 5428 / 542e | u8[] | formation tables | const |
| DS:a388 / a389 | u8 | `board_held_from_col/row` | BSS |
| DS:a38a | i16 | `board_cursor_y` | BSS |
| DS:a38c | u8 | `board_held_idx` | BSS |
| DS:a38d | u8 | reset only | BSS |
| DS:a38e | i8 | `board_hl_row` | BSS |
| DS:a38f | u8 | `auto_lineup_dead_count` (write-only) | BSS |
| DS:22e0, DS:4a14 tables | far ptr[] | UI strings | const |

## 6. Open questions

1. The weekly squad update (form, fitness, injuries, aging, contract countdown) that the task attributes
   to 2ecd:3360 is not in segment 2ecd. Candidates: `random_weekly_events` 1cb5:13bd, `manager_ai_weekly`
   1f9d:0002, the season end (2a58:4597). Ruled out: `event_helper_25d5` 1cb5:25d5 (called at the end of
   auto_lineup) is `squad_fix_position_mismatch`, season.md §4.4.
2. `auto_lineup` writes through an uninitialized `bench_best` in some cases (§3.1). Decided in the port:
   the slot-13 write is skipped when `bench_best` was never assigned (see the port status at §3);
   `TODO(re)` in `squad.rs`.
3. Mouse event code 3 (add a substitute) against MEMORY_MAP's "2 = right".
4. Exact meanings of squad +0x14 (bit field), +0x18 (offer state) and the player fields +0x22/+0x23
   belong to the transfer and match areas.
5. FAR:513e and FAR:56ee are set in 18bc:0b5f (office dispatcher); their exact rules (cup vs league,
   season) are not traced here.
6. `ui_draw_button` icon flags (0x8000 pressed, 0x2000 redraw state only, 0xa000) are inferred from use.
