# Season loop, calendar and season events (`season`)

Area: segment `2a58` (season main loop, special dates, season-end points, match-day helpers that live
in this segment) and segment `1cb5` (season-end messages, weekly random events, player/club
rebalancing). All addresses are Ghidra addresses in `/unpacked/BMMAIN_unpacked.EXE`.

Conventions (same as `rng.md`): MSC large model, `int` = `i16`, `long` = `i32`, `char` is **signed**,
`u8` = `unsigned char`. All arithmetic wraps. `random_range(lo, hi)` (176b:0cc7) =
`rand() % (hi - lo + 1) + lo`, see `rng.md`. Long helpers: `ldiv` = `_aFldiv` (signed, truncating),
`lmul`, `lrem` (sign of dividend), `uldiv` (unsigned). `clamp_int(&v, lo, hi)` = 4091:1009.
`message_post` (4091:0190) always takes 13 argument words:
`void far *message_post(u8 kind, i32 type, void far *a, i32 b, i32 c, i32 d, i32 e)` (callers clean
up with `ADD SP,0x1a`); a far pointer passed in place of an `i32` occupies the same two words.

Segments: `DS:xxxx` = DGROUP 5cb3, `FAR:xxxx` = far data segment 5238. Every global in this document
was resolved from the `MOV ES,[DS:99xx]` selector loads in the disassembly (scratch:
`out/season/*.asm`, `*.cnd`, `selmap.json`), not from the decompiler output, which hides the segment.

Recurring records:

| Name | Base | Stride | Index | Notes |
|---|---|---|---|---|
| `M[m]` manager record | FAR:2242 | 0x30a | 0..DS:07ab-1 | savegame block 13. Offsets below are **relative to FAR:2242** (FAR:2260 = `+0x1e` team id) |
| `T[t]` team season stats | FAR:0ecc | 0x36 | team id 0..63 | savegame block 16 |
| `S[g][i]` squad slot | FAR:774a | 0x34 | g*25+i, g = 0..3 human squads, g = 4 transfer pool (FAR:8b9a) | savegame block 18 |
| `P[p]` person record | FAR:57dd | 0x25 | 1..0x96 | savegame block 17. `P[p]+0x00` name (26 bytes) |
| `C[c]` club record | FAR:3066 | 0x22 | club id | savegame block 15 |

Team ids are **slots ordered by league tier**: ids `DS:2272[2k]..DS:2273[2k]` belong to tier `k`
(tier 0 = Bundesliga 0..17, tier 1 = 2. Liga 18..37, tier 2 = Amateur-Oberliga 38..57, tier 3 =
everything above, 58..255). Promotion/relegation swaps whole team records (1310:0b24), so the id range
check stays valid.

---------------------------------------------------------------------------------------------------

## 1. Calendar model

### 1.1 State

| Global | Type | Meaning |
|---|---|---|
| DS:07dc | i32 | `day`: day counter of the current season. Reset to `(i32)(i8)DS:224c` at the start of every season (=0 in the shipped data). Save block 23 |
| FAR:016e | u8 | `slot`: half-week (match slot) index into DS:2280. Reset to 0 at season start, `+1` per loop pass. Save block 49 (**FAR**, not DS: the MEMORY_MAP entry "DS:016e" is wrong) |
| DS:07e0 | u16 | season start year (`+1` per season). Save block 24 |
| DS:07e2 | i16 | number of completed seasons (`+1` per season, used as "season count"). Save block 48 |
| FAR:2eba | i32 | day of month, **1-based** (1..31). Save block 20 |
| FAR:2e8e | i32 | month, **0-based** (0 = January). Save block 21 |
| FAR:a7a0 | i32 | calendar year of the current date. Save block 22 |
| FAR:4bce | u8 | copy of the low byte of FAR:2e8e taken at the top of each loop pass. Save block 52 |
| DS:07b8 | u8[12] | days per month: 31,28,31,30,31,30,31,31,30,31,30,31 (no leap years) |

### 1.2 `date_from_slot(u16 n)` = 1310:1190 (outside this area, documented here because it defines the calendar)

```c
void date_from_slot(u16 n) {                 // 1310:1190 "matchday_refresh"
    FAR:2e8e = 0;                             // month (long)
    if (n == 0) FAR:2eba = DS:07dc;           // current day counter, no DS:224c offset
    else {
        if (n == 0x78) n = 0;
        FAR:2eba = (u32)((n >> 1) * 7);       // u16 MUL, high word forced to 0
        if (n & 1) FAR:2eba += 4;
        FAR:2eba += (i32)(i8)DS:224c;
    }
    FAR:a7a0 = (i32)(u16)DS:07e0;
    FAR:2eba += 210;                          // day 0 = 29 July
    while (FAR:2eba > 365) { FAR:2eba -= 365; FAR:a7a0++; }        // signed 32-bit compare
    while ((i32)(i8)DS:07b8[FAR:2e8e] < FAR:2eba) {
        FAR:2eba -= DS:07b8[FAR:2e8e];
        if (++FAR:2e8e > 11) FAR:2e8e = 0;
    }
}
```

So match slot `n` is played on day `(n/2)*7 + (n odd ? 4 : 0) + DS:224c`. Day 0 is 29 July, day 322 is
16 June of the next year. Weekday shown on screens: `DS:242c[((day + 4) % 7)]` (0 = "Montag" ...
6 = "Sonntag"; `lrem`, so day 0 is "Freitag"). Month names: `DS:23fc[month]` ("Januar" ...
"Dezember").

### 1.3 Season length and phases

* Normal phase: while `day <= (i8)DS:224c + 0x142` (322). One loop pass = one half-week slot.
* Winter break: slots 37..58 of DS:2280 are 0. The "WINTERPAUSE" screen is shown once when
  `day > (i8)DS:224d*7 + 130 && day < 207 - (i8)DS:224d*14` (all compares signed 32-bit;
  DS:224d = 0 in the shipped data, so 130 < day < 207). The pixel (1,1) of the main rastport is used
  as a "already shown" marker (colour 8).
* Season end: when `day > DS:224c + 0x142` after the weekly events (see §3.1).
* Summer break: the loop advances `day` until the date is 28 July (`FAR:2eba == 28 && FAR:2e8e == 6`),
  then a new season starts with `day = DS:224c`, `slot = 0`, `DS:07e0++`.

### 1.4 Match-type table DS:2280 (95 bytes, save block 50)

One byte per half-week slot (`FAR:016e`). Bit meaning, from the dispatch in the season loop:

| Bit | Mask | Meaning | Action in season loop |
|---|---|---|---|
| 0 | 0x01 | tier 0 (Bundesliga) league round | `1310:0463(slot, 0)`; `DS:225a[0]++` |
| 1 | 0x02 | tier 1 (2. Liga) round | `DS:225a[1]++` |
| 2 | 0x04 | tier 2 (Amateur-Oberliga) round | `DS:225a[2]++` |
| (0-2) | 0x07 | any league round | `1310:0463(slot,1)`, `1310:0463(slot,2)`, `1310:0995(wf)` are called when **any** of bits 0-2 is set |
| 3 | 0x08 | DFB-Pokal round | `1310:0261(1)` |
| 4-6 | 0x70 | European cups (3 competitions). Exactly `0x10` is a special single-cup slot | `== 0x10`: `1310:02bf()`; otherwise if nonzero: `1310:0290(0x0e)` |
| 7 | 0x80 | extra match (only if `1310:000a(slot)` returns nonzero) | `1310:07a2(slot)` |
| value 9 | | "skip this slot" marker: mapped to 0 at the top of the loop (no offices, no matches). Not present in the shipped table, so it is written at run time by another module | |

Shipped values (`index:value`):

```
 0:07  1:70  2:07  3:00  4:07  5:70  6:07  7:06  8:08  9:00 10:07 11:00 12:07 13:06 14:07 15:70
16:07 17:07 18:07 19:70 20:07 21:00 22:08 23:01 24:07 25:06 26:07 27:00 28:07 29:06 30:08 31:70
32:07 33:00 34:07 35:70 36:07 37..58:00 (winter break)       59:07 60:07 61:00 62:07 63:70 64:08
65:07 66:07 67:70 68:07 69:06 70:07 71:00 72:07 73:00 74:07 75:00 76:07 77:70 78:07 79:00 80:07
81:70 82:07 83:00 84:07 85:00 86:08 87:00 88:07 89:00 90:07 91:10 92:10 93:00 94:00
```

Counts: 34 slots with bit 0 (= the 34 Bundesliga rounds, DS:2262[0] = 34), 38 slots with bits 1/2
(DS:2262[1..2] = 38), 5 DFB-Pokal rounds (8, 22, 30, 64, 86), 10 European-cup slots plus the two
`0x10` slots 91/92.

### 1.5 Season configuration block DS:224c..2280 (shipped values)

| DS | Type | Value | Suggested name / meaning |
|---|---|---|---|
| 224c | i8 | 0 | `season_day_offset` (start day; also used in the day-of-week break test) |
| 224d | i8 | 0 | `calendar_shift_weeks`. Shifts the winter window; when nonzero, money strings get DS:4f78 (empty string) inserted. Possibly a country/version switch |
| 224e | i32 | 1 | `money_div`: displayed DM amount = `amount * DS:2252 / DS:224e` |
| 2252 | i32 | 1 | `money_mul`. If `DS:2252 != DS:224e` the birthday screens and the "SCHERZ" message are suppressed (German version check) |
| 2256 | i32 | 0x128e0 (76000) | fixed gate receipt used by 2a58:20b2 for cup round > 4 |
| 225a | u8[3] | 1,1,1 | `league_round[3]`: rounds played per tier (save block 34) |
| 225e | i8[3] | 18,20,20 | `tier_team_count[3]` |
| 2262 | i8[3] | 34,38,38 | `tier_round_count[3]` |
| 2266 | i8[3] | 3,4,4 | `tier_relegated_count[3]` |
| 226a | i8[3] | 1,0,0 | `tier_has_relegation_playoff[3]` |
| 226e | u8[4] | 0,18,38,? | `tier_first_rank[k]` (226f = 18, 2270 = 38) |
| 2272 | u8[4][2] | (0,17),(18,37),(38,57),(58,255) | `tier_team_id_range[k] = {first,last}` |

---------------------------------------------------------------------------------------------------

## 2. Data tables

| Address | Decoded | Used by |
|---|---|---|
| DS:53d0 | i8[3] days `{24, 12, 19}` | special dates |
| DS:53d4 | i8[3] months (1-based) `{12, 11, 4}`: 24 Dec = Christmas, 12 Nov = birthday of Jens Onnen, 19 Apr = birthday of Werner Krahe | special dates |
| DS:0292 | i8[4][3] training mix `{80,5,15}, {30,40,30}, {20,25,55}, {5,65,30}` | random_weekly_events §4.4 |
| DS:5390 | i16[4] `{16,14,10,8}` | 2a58:20b2 neutral-ground ticket price |
| DS:53c7 | `"@@@@@@@@"` | reset value of the 9-byte form strings at T[t]+4 and T[t]+0xd |
| DS:06fc.. | 38 far pointers to 18-letter fixture permutations | game-over patch in season loop |
| DS:22e0+4k | UI string table, see quotes below (`{`=ä, `|`=ö, `}`=ü, `[`=Ä, `]`=ü in `ersch]ttert`/`Torsch]tzen`, `~`=ß; the game font maps these ASCII codes to umlauts) | all |

---------------------------------------------------------------------------------------------------

## 3. Segment 2a58

### 3.1 `season_main_loop` 2a58:3176

`void season_main_loop(u8 resume)` — called once by `main` 18bc:0418 with `resume != 0` after a game
was loaded (the stored current manager FAR:304a then gets the first office turn). Never returns.

Local `wf` (u8, [BP-0x34]) = slot flags; `saved_mgr` ([BP-2]). `n = DS:07ab` (number of human
managers). In all manager loops the loop variable **is** FAR:304a (i8, compared signed against
`(u8)DS:07ab`).

```c
void season_main_loop(u8 resume) {
    u8 saved_mgr;
    if (resume) saved_mgr = FAR:304a;
TOP:                                                    // 2a58:3194
    date_from_slot(0);
    FAR:4bce = (u8)FAR:2e8e;
    wf = DS:2280[FAR:016e]; if (wf == 9) wf = 0;
    for (FAR:304a = 0; FAR:304a < n; FAR:304a++) finance_weekly_processing();   // 212a:0a6d()
    league_init_or_ai(1);                                                       // 1f9d:0697(1)
    if (wf == 0) goto DAY_ADVANCE;
    for (FAR:304a = 0; FAR:304a < n; FAR:304a++) {
        DS:079e[2*m] = DS:079f[2*m];                       // m = FAR:304a
        squad_weekly_update();                             // 2ecd:3360()
        manager_ai_weekly(m, 0);                           // 1f9d:0002(m, 0)
    }
    FAR:304a = 0;
    goto OFFICE_B_TEST;

OFFICE_A_TEST:                                          // 2a58:3abd (season-end office turns)
    while (FAR:304a < n) {
        if (resume) { FAR:304a = saved_mgr; resume = 0; }
        u8 cur = FAR:304a;
        input_wait_release();
        office_menu_dispatcher(0xff);                   // 0xff = "SAISONENDE" mode
        if (day > 0x140) {                              // signed 32-bit
            FAR:304a = cur;
            season_record_manager(FAR:304a);            // block 2a58:3763, see below
            FAR:304a++;
            continue;                                   // back to OFFICE_A_TEST
        }
        FAR:304a++;
        goto OFFICE_B_TEST;                             // falls into the other loop
    }
    goto SEASON_FINAL;                                  // 2a58:3bcf

OFFICE_B_TEST:                                          // 2a58:3b26 (normal office turns)
    while (FAR:304a < n) {
        if (resume) { FAR:304a = saved_mgr; resume = 0; }
        FAR:5358 = FAR:304a;
        input_wait_release();
        if (random_range(0, n + 3) == 0) transfer_random_offer();   // 3277:1e38()
        office_menu_dispatcher(FAR:016e);
        wf = DS:2280[FAR:016e];                         // re-read, NO 9 -> 0 mapping
        if (day > 0x142) {                              // signed 32-bit
            season_record_manager(FAR:304a);
            FAR:304a++;
            goto OFFICE_A_TEST;
        }
        FAR:304a++;
    }
    // ---- matches of this slot (2a58:326a) ----
    input_wait_release();
    for (FAR:304a = 0; FAR:304a < n; FAR:304a++) {
        manager_ai_weekly(m, 1);                        // 1f9d:0002(m, 1)
        DS:079f[2*m] = DS:079e[2*m];
        DS:079e[2*m] = 1;
        FAR:1d14[m] = 0;
        FAR:90c6[m] = 0;
    }
    FAR:304a = 0x1e;
    if (wf & 7) {
        if (wf & 1) league_play_round(FAR:016e, 0);    // 1310:0463(slot, tier)
        league_play_round(FAR:016e, 1);
        league_play_round(FAR:016e, 2);
        league_round_finish(wf);                        // 1310:0995(wf)
    }
    FAR:304a = 0x1e;
    if (wf & 8) cup_play_slot(1);                       // 1310:0261(1)
    if ((wf & 0x70) == 0x10) euro_special_slot();       // 1310:02bf()
    else if (wf & 0x70) euro_play_slot(0x0e);           // 1310:0290(14)
    if ((wf & 0x80) && extra_match_pending(FAR:016e))   // 1310:000a(slot) (u8 result)
        extra_match_play(FAR:016e);                     // 1310:07a2(slot)

DAY_ADVANCE:                                            // 2a58:3397
    for (u8 b = 0; b < 3; b++)
        if ((DS:2280[FAR:016e] >> b) & 1) DS:225a[b]++;          // raw table byte, not wf
    if (day > (i16)((i8)DS:224d*7 + 0x82) &&
        day < (i16)(0xcf - (i8)DS:224d*14)) {                     // both i32 compares
        if (rastport_get_pixel(FAR:2f28, 1, 1) != 8) {           // WINTERPAUSE once
            screen_fade(FAR:2e70, 0);
            show_title_screen(DS:4c00);                          // 1310:01f2("WINTERPAUSE")
            rastport_set_color(FAR:2f28, 8);
            rastport_put_pixel(FAR:2f28, 1, 1);
        }
    }
    for (FAR:304a = 0; FAR:304a < n; FAR:304a++)
        for (u8 i = 0; i < 24; i++)
            if (S[m][i].b[0x0d] != 0) S[m][i].b[0x0a] = 0;      // suspended -> out of lineup
    day++;                                                      // i32
    check_special_dates();                                      // see below
    for (;;) {                                                  // 2a58:355c
        date_from_slot(0);
        for (FAR:304a = 0; FAR:304a < n; FAR:304a++) { FAR:5358 = FAR:304a; finance_weekly_processing(); }
        day++;
        check_special_dates();
        if (FAR:2e8e == 11 && DS:05da != 0) fatal_error_exit(99);   // demo/limited version stop in December
        if (lrem(day - (i32)(i8)DS:224c, 7) == 0) break;
        if (lrem(day - 4, 7) == 0) break;                       // note: no DS:224c here
    }
    for (FAR:304a = 0; FAR:304a < n; FAR:304a++) {
        random_weekly_events();                                 // 1cb5:13bd
        if (lrem(day - (i32)(i8)DS:224c, 14) == 0) top_scorer_weekly_update(); // 24a4:2cb4()
    }
    if (day <= (i32)((i8)DS:224c + 0x142)) { FAR:016e++; goto TOP; }      // 2a58:458b
    // season end reached (2a58:36f9)
    for (FAR:304a = 0; FAR:304a < n; FAR:304a++) {
        DS:079e[2*m] = DS:079f[2*m];
        squad_weekly_update();
        manager_ai_weekly(m, 0);
    }
    FAR:304a = 0;
    goto OFFICE_A_TEST;
```

`check_special_dates()` (inlined twice):

```c
for (u8 i = 0; i < 3; i++)
    if ((i32)(i8)DS:53d0[i] == FAR:2eba && (i32)((i8)DS:53d4[i] - 1) == FAR:2e8e)
        special_date_events(i);                 // 2a58:2a06, near call, i16 argument
```

`season_record_manager(m)` (block 2a58:3763..3ab4, inlined), `r = M[m]`, `team = r.b[0x1e]`,
`t = T[team]`:

```c
for (u8 k = 0; k < 2; k++) {                    // k: 0/1 = two sub-records of the team stats
    u8  a = t.b[k+0x00], w = t.b[k+0x1e], g1 = t.b[k+0x16], g2 = t.b[k+0x1a];
    u8  d = t.b[k+0x26], l = t.b[k+0x2a];
    i16 *acc = &FAR:23e6[(k + m*0x185)];        // = r.w[0x1a4 + 2k] etc. (word array, stride 2)
    r.w[0x1a4+2k] += a;           r.w[0x1b0+2k] += 2*w - a;
    r.w[0x1a8+2k] += g1;          r.w[0x1ac+2k] += g2;
    r.w[0x1b4+2k] += w;           r.w[0x1b8+2k] += d;
    r.w[0x1bc+2k] += l;           r.w[0x1c0+2k] += w - l - d;      // all u16 wrap
}
r.l[0x1e8] = 0;  r.b[0x13a] = 0;  r.l[0x1e4] = 0;  r.l[0x1ec] = 99999;   // season records reset
i16 e = DS:07e2;                                // log index = completed seasons
if (e > 0x31) {                                 // history log full: shift down by one entry
    for (u8 j = 0; j < 0x31; j++) far_memcpy_bytes(&r.b[0x42+4*j], &r.b[0x3e+4*j], 4);  // (src,dst,n)
    r.b[0x13b]++;
    e = 0x31;
}
u8 tier = r.b[0x138];
i8 pos  = r.b[0x10a + (i8)DS:2262[tier]];       // final table position (0-based) of this tier
r.b[0x3e+4*e] = pos + 1;
if (pos == 0 && tier == 0) { r.b[0x39]++; championship_winner_message(m, 0); }  // 27c2:274d
r.b[0x3e+4*e] += DS:226e[tier];                 // global rank 1..58
r.b[0x3f+4*e]  = tier;
r.b[0x40+4*e]  = ((u16)r.b[0x1e] - DS:07ac == (u16)-1 ? 0x80 : 0) + r.b[0x132] + 1;   // DFB-Pokal stage, +0x80 = cup winner
if ((u16)r.b[0x1e] - DS:07ac + 1 == 0) r.b[0x3a]++;          // DS:07ac = cup winner team id + 1
for (u8 j = 1; j < 4; j++) {                    // European cups 1..3
    u8 st = r.b[0x132 + j];
    if (st != 0 && st != 0x1e) {
        r.b[0x41+4*e] = (j << 3) + st;
        u16 win = (j == 1) ? DS:07ad : (j == 2) ? DS:07ae : DS:07af;   // winner team id + 1
        if ((u16)r.b[0x1e] - win + 1 == 0) { r.b[0x41+4*e] += 0x80; r.b[0x3a + j]++; }
    }
}
```

`SEASON_FINAL` (2a58:3bcf):

```c
i32 old_pts[4]; u8 scorer_flag[4];              // [BP-0x1a], [BP-0x20]
for (u8 i = 0; i < n; i++) {                    // u8 loop, not FAR:304a
    old_pts[i] = T[M[i].b[0x1e]].l[0x32];
    FAR:4bec[i] = 0;
    FAR:304a = i;
    scorer_flag[i] = screen_top_scorers(1);     // 24a4:1ad5(1), returns u8
}
season_end_eternal_points();                    // 2a58:4597
u8 div = ((u16)DS:07e2 < 4) ? (u8)(DS:07e2 + 1) : 5;
i16 v;                                          // [BP-8], NOT reset per manager (bug kept)
for (u8 i = 0; i < n; i++) {
    i32 gain = T[M[i].b[0x1e]].l[0x32] - old_pts[i];
    u16 q = (u16)uldiv(uldiv(lmul(gain, 100), 108), (i32)(i8)div);
    v = M[i].w[0x1dc] + (q - M[i].w[0x1dc] / (u16)(i8)div);      // u16 DIV
    clamp_int(&v, 0, 100);                      // result only kept in v
}
for (u8 t = 0; t < 0x40; t++)
    for (u8 k = 0; k < 2; k++) {
        T[t].b[k+0x00] = T[t].b[k+0x16] = T[t].b[k+0x1a] = 0;
        T[t].b[k+0x1e] = T[t].b[k+0x26] = T[t].b[k+0x2a] = 0;
        far_strcpy(&FAR:0ed0[(t*6 + k)*9], DS:53c7);              // "@@@@@@@@"
    }
// bankruptcy: forced relegation of tier 0/1 managers below -2,000,000 DM
i8 extra_down[3] = {0,0,0};                     // [BP-0x2e..-0x2c]
for (FAR:304a = 0; FAR:304a < n; FAR:304a++) {
    r = M[m]; r.b[0x140] = 0;                   // season-end flags
    if (r.l[0x1f0] < -2000000 && (i8)r.b[0x138] < 2) {          // balance
        i8 k = r.b[0x138];
        r.b[0x140] = 4;                         // "DFB verweigert die Lizenz"
        for (u8 i = 0; (i16)i < (i8)DS:225e[k]; i++)
            if (FAR:535a[k*20 + i] == r.b[0x1e]) {               // tier table, 20 per tier
                for (; (i16)((i8)DS:225e[k] - 1) > i; i++) FAR:535a[k*20+i] = FAR:535a[k*20+i+1];
                FAR:535a[k*20+i] = r.b[0x1e];  // move team to the last place
                extra_down[k]++;
                r.b[0x138]++;
                break;
            }
    }
}
// promotion / relegation (2a58:3ee0)
for (u8 k = 0; k < 3; k++) {
    if (k == 0 && FAR:3060 == 0) extra_down[0]++;               // no play-off: one more down
    if ((i8)DS:2266[k] < extra_down[k]) extra_down[k] -= DS:2266[k]; else extra_down[k] = 0;
    DS:225a[k] = 1;                                             // round counters restart
    for (u8 j = 0; (i16)j < extra_down[k] + (i8)DS:2266[k]; j++) {
        u8 down = FAR:535a[(i8)DS:225e[k] + k*20 - j - 1];     // = FAR:5359[...]
        u8 up;
        if (k < 2) up = FAR:535a[(k+1)*20 + j];                 // = FAR:536e[k*20+j]
        else {
            up = DS:225f + DS:2260 + DS:225e;                   // i8 sum (=58)
            if (j == 0 && (i8)up < 60) up = random_range(up + 3, up + 4);
            else up += 3 - j;
        }
        if (k == 0 && extra_down[0] + (i8)DS:2266[0] - j == 1 && FAR:3060 == 0 && DS:226a[0] != 0)
            continue;                                           // the play-off place is not swapped
        u8 mi = manager_index_of_team(down, 0xff);              // 1310:32b1, 1+index or 0
        if (mi && M[mi-1].b[0x140] == 0) {
            if (k < 2) { M[mi-1].b[0x140] = 2; M[mi-1].b[0x138]++; }     // relegated
            else { M[mi-1].b[0x140] = 8; M[mi-1].l[0x1f0] = 500000;      // dropped out of tier 2
                   swap_teams(down, up, 1); }                            // 1310:0b24 (extra call)
        }
        mi = manager_index_of_team(up, 0xff);
        if (mi && M[mi-1].b[0x140] == 0) { M[mi-1].b[0x138]--; M[mi-1].b[0x140] = 1; }   // promoted
        swap_teams(down, up, 1);                                // 1310:0b24(a, b, 1)
    }
}
league_init_or_ai(10);                                          // 1f9d:0697(10)
for (u8 i = 0; i < n; i++) {                                    // v from the loop above
    if (v < 30 && M[i].b[0x138] < 2) v = 30;
    if (v < 61 && M[i].b[0x138] == 0) v = 61;
    M[i].w[0x1dc] = v;                                          // board satisfaction 0..100
}
if ((i32)FAR:513c == FAR:a7a0) {                                // last season of the game reached
    *(char far*)DS:0704 = 'a'; *((char far*)DS:070c + 4) = 'a';
    *((char far*)DS:0718 + 8) = 'a'; *((char far*)DS:0720 + 8) = 'a';   // patches fixture permutations
    highscore_load_or_store(0);                                 // 422b:21c4
    for (FAR:304a = 0; FAR:304a < n; FAR:304a++) {
        highscore_prepare_entry(m, 1);                          // 422b:2a2a -> candidate at FAR:4cfe
        for (u8 i = 0; i < 20; i++) {                           // HIGH table FAR:1d52, 58-byte rows
            if (strcmp(FAR:4cfe, row[i].name) == 0 && strcmp(FAR:4d18, row[i].club) == 0) goto put;
            if (i == 19 && row[19].score < (i16)FAR:4d32) { put:
                far_memcpy_bytes(FAR:4cfe, &row[i], 58);
                for (u8 a = 0; a < 19; a++) for (u8 b = a+1; b < 20; b++)
                    if (row[a].score < row[b].score) swap rows via FAR:4cfe;   // 3 copies of 58 bytes
                // no break: the scan goes on with i + 1 and FAR:4cfe as the candidate
            }
        }
    }
    highscore_load_or_store(1); highscore_insert_entry(0);      // 422b:2366(0)
    game_end_screen(0);                                         // 2a58:000c
    fatal_error_exit(99);
}
for (FAR:304a = 0; FAR:304a < n; FAR:304a++) { highscore_insert_entry(2); top_scorer_weekly_update(); }
ui_fill_rect(FAR:2f28, 0, 0, 319, 239, 0);
season_reset_tables();                                          // 1310:09c5()
competition_reset();                                            // 27c2:0ef2()
season_end_messages(scorer_flag);                               // 1cb5:0012(far ptr)
DS:07e2++;
day++;
do {                                                            // summer break 2a58:43d5
    FAR:4bce = (u8)FAR:2e8e;
    date_from_slot(0);
    for (FAR:304a = 0; FAR:304a < n; FAR:304a++) {
        FAR:5358 = FAR:304a;
        finance_weekly_processing();
        for (u8 i = 0; i < squad_count_by_flag(m, 0); i++) player_suspension_countdown(&S[m][i]);
    }
    day++;
} while (!(FAR:2eba == 28 && FAR:2e8e == 6));
date_from_slot(0);
FAR:016e = 0;
day = (i32)(i8)DS:224c;
DS:07e0++;
memset(FAR:6ddc, 0, 0x8e8);           // done as mem_alloc(0x8e8,1) + far_memcpy_bytes + mem_free
for (u8 i = 0; i < n; i++) office_random_event(i);              // 18bc:0a63(i)
competition_count();                                            // 27c2:1b6d()
startup_step_02ef(3);                                           // 18bc:02ef(3)
for (u8 k = 0; k < 4; k++) competition_init(k, 0);              // 27c2:09e0(k, 0)
for (u8 k = 1; k < 4; k++) competition_init(k, 1);
startup_step_034d(3);                                           // 18bc:034d(3)
goto TOP;
```

Note on the high-score loop: after a `put` and the sort (2a58:42ea resets the sort's outer index, not
the row index) the scan goes on with the next row, and the candidate is now the swap buffer
FAR:4cfe, which holds the row the sort's last swap moved out. When the put was over the manager's
own row and the sort moved it up, that buffer row then beats the last row and is put in a second
time (the last row drops out). Port: `bmp_game::highscore::highscore_game_end_insert`.

RNG calls in this function, in order per pass: `random_range(0, n+3)` once per office turn in loop B;
at season end `random_range(up+3, up+4)` (only tier 2, `j == 0`, `up < 60`).

Callers: `main` 18bc:0418. Callees: listed inline (all outside the area except 2a58:2a06, 2a58:4597,
2a58:000c, 1cb5:0012, 1cb5:13bd, 1cb5:2b88).

### 3.2 `special_date_events` 2a58:2a06

`void special_date_events(i16 idx)` — idx 0 = Christmas, 1 = Jens Onnen, 2 = Werner Krahe.

```c
u8 timed_out = 0;
if (random_range(0, 3) != 0) return;                         // only 25 % chance
if (!(DS:2252 == DS:224e) && idx != 0) return;               // birthdays only in the DM version
screen_fade(FAR:2e70, 0);
ui_fill_rect(rp, 0, 0, 319, 239, 0x10);
ui_draw_frame(rp, 0, 0, 319, 239, 0x13);
bm = vga_load_picture(idx == 0 ? 0x2a : 0x29, 0,0,0, 1);     // PIC\42.VGA Christmas, PIC\41.VGA programmer
blit_bitmap(bm, 0, 0, rp, 0x88, 0x34, 0x30, 0x39, 0xc0);     // 48x57 at (136,52)
ui_draw_frame(rp, 0x86, 0x32, 0xb7, 0x6c, 0x1d);
bitmap_free(bm);
ui_set_font(rp, 1);
buf = DS:242c[lrem(day + 4, 7)] + ", " + fmt(FAR:2eba) + ". " + DS:23fc[FAR:2e8e];
ui_draw_text_centered_shadow(rp, buf, 0, 319, 8, 0x0b, 0);   // e.g. "Freitag, 24. Dezember"
if (idx == 0) {
    ui_draw_text_centered_shadow(rp, "Frohe Weihnachten !", 0, 319, 0x23, 0x13, 0);
    ui_set_font(rp, 1);
    if (random_range(0, 1) != 0) {                           // 50 %: Christmas bonus
        i32 bonus = (i32)random_range(15, 85) * 10000;       // i16*i16 IMUL -> 150000..850000
        DS:07b2 = 0;
        for (i = 0; i < 5; i++) ui_draw_text_centered_shadow(rp, DS:26f0[i], 0, 319, 0x8c + 11*i, 0x0b, 0);
        //  "Ihre Spieler sind }berw{ltigt und" / "zu Tr{nen ger}hrt, als sie Ihre" /
        //  "gro~z}gigen Weihnachtspakete unter" / "dem lammettabehangenen Christbaum" /
        //  "ersp{hen. Was f}r eine Freude !"
        ui_set_font(rp, 2);
        buf = "BUNDESLIGA: " [+DS:4f78 if DS:224d] + fmt(ldiv(lmul(DS:2252, bonus), DS:224e))
            + " DM, 2.LIGA: " [+DS:4f78] + fmt(ldiv(lmul(DS:2252, bonus >> 1), DS:224e)) + " DM,";
        ui_draw_text_centered_shadow(rp, buf, 0, 319, 0xde, 0x0b, 0);
        buf = "AMATEUR-OBERLIGA: " [+DS:4f78] + fmt(ldiv(lmul(DS:2252, ldiv(bonus, 3)), DS:224e))
            + " DM (INCL. MWST.)";
        ui_draw_text_centered_shadow(rp, buf, 0, 319, 0xe6, 0x0b, 0);
        for (i = 0; i < n; i++) M[i].l[0x1f0] -= ldiv(bonus, (i32)(M[i].b[0x138] + 1));
    }
} else {
    ui_draw_text_centered_shadow(rp, "Oh happy day !!!", 0, 319, 0x21, 0x13, 0);
    ui_set_font(rp, 1);
    for (i = 0; i < 8; i++) {                                // DS:2704[i]
        buf = DS:2704[i]; if (i == 3) buf += (idx == 1 ? "JENS ONNEN" : "WERNER KRAHE");
        ui_draw_text_centered_shadow(rp, buf, 0, 319, 0x7d + 11*i, 0x0b, 0);
    }   // "Sehr geehrte Mitspieler !", "Heute ist ein {u~erst denkw}rdiger Tag.", "Es ist der Geburtstag
        // eines der beiden", "glorreichen Programmierer: ", "La~t uns eine Minute in ehrerbietendem",
        // "Schweigen verharren und dem gedenken,", "ohne den dieses affenstarke Spiel nie",
        // "das Licht der Welt erblickt h{tte..."
}
screen_fade(FAR:2e70, 1);
timer_set_deadline(0x438);                                   // 1080 BIOS ticks ~ 59 s
for (;;) {
    if (timer_deadline_reached()) timed_out = 1;
    input_poll_mouse(FAR:6dc0);
    if (FAR:2ea2 == 1 || DS:05d4 != 0) break;                // left click or abort
    if (timed_out && idx != 0) break;                        // birthdays end after one minute
}
if (!timed_out && idx != 0) {                                // clicked away too early
    ui_draw_text_centered_shadow(rp, "Das war keine Minute !", 0, 319, 0xe4, 0x0b, 0);
    ui_draw_text_centered_shadow(rp, "Sch{men Sie Sich !", 0, 319, 0xee, 0x0b, 0);
    delay_ticks(150);                                        // 50 BIOS ticks, about 2.7 s
}
```

Callers: season_main_loop (two sites). The screen is not faded out at the end (the caller's next
screen does it).

### 3.3 `season_end_eternal_points` 2a58:4597

`void season_end_eternal_points(void)` — adds the season's points for the all-time table
(`T[t].l[0x32]`, i32) for team ids 0..63.

```c
show_title_screen(DS:25b8);                                  // 1310:01f2("SAISONENDE")
for (i16 t = 0; t < 0x40; t++) {
    i16 v = (((t < (i8)DS:2270) + (t < (i8)DS:226f)) * 25) - T[t].b[0x2e] + 19;   // b[0x2e] = final rank (0-based)
    if ((u8)DS:2277 < t) v = 0;                              // not in a league
    T[t].l[0x32] += (i32)v;
    for (i16 c = 0; c < 4; c++) {
        FAR:4bec[c] = 0;
        u16 win; i16 bonus;
        switch (c) { case 0: win = DS:07ac; bonus = 20; break;   // DFB-Pokal
                     case 1: win = DS:07af; bonus = 30; break;
                     case 2: win = DS:07ae; bonus = 40; break;
                     case 3: win = DS:07ad; bonus = 50; break; }
        if (win - t == 1) T[t].l[0x32] += bonus;
    }
    i16 b = 0;
    if (t < (i8)DS:226f) { if (T[t].b[0x2e] == 0) b = 12; else if (T[t].b[0x2e] < 5) b = 3; }
    else if (T[t].b[0x2e] == 0 && t <= (u8)DS:2277) b = 3;
    T[t].l[0x32] += b;
}
```

Caller: season_main_loop. No RNG.

### 3.4 `game_end_screen` 2a58:000c

`void game_end_screen(u8 hang)`: `ui_fill_rect(rp,0,0,319,239,0)`, `ui_set_font(rp,3)`,
`ui_draw_text_centered_shadow(rp, "ENDE" (DS:537e), 8, 319, 0x73, 1, 0xff)`,
`input_wait_click(FAR:6dc0)`; then `while (hang) ;` (endless loop when the argument is nonzero).
Caller: season_main_loop (with 0, then `fatal_error_exit(99)`). Port: `season_end::game_end_screen`;
the game-over path writes the table to `HIGH.XY`, shows `highscore_screen(0)` and this screen
(`ScreenSeason::game_end`). The candidate buffer `FAR:4cfe` keeps its bytes between the
high-score calls (`highscore_build_entry` copies the names with `strcpy`, the sorts swap through
it), so `HIGH.XY` carries the tails of earlier names after the NULs (verified: `HIGH.12` of the
one-year game loaded after an endless game's season end holds `"ADI\0ER"` and
`"1.FC N\x9aRNBERG\0NGEN"`). Port: `Runtime::highscore_candidate`.

### 3.5 `match_forfeit_penalty` 2a58:2051

`void match_forfeit_penalty(u8 m)`: `dialog_okay(&M[m] (FAR:2242+m*0x30a, the manager name), "Ihr Spiel
wird mit 0:2", "gewertet. Sie zahlen", "200.000 DM Strafe")` (4091:0e3a), then
`M[m].l[0x1f0] -= 200000`. Callers: 2a58:20b2 (twice).

### 3.6 `match_gate_receipts_and_aftermath` 2a58:20b2

`i16 match_gate_receipts_and_aftermath(u8 home, u8 away, u8 cup_round, i8 neutral, u8 p5, u8 tier_mask)`
— called by 1310:15db (match-day playing) once per played fixture. Returns 0, or 1/2 when the
home/away human manager forfeits.

`gate_div = (cup_round != 0 && neutral == 0) ? 2 : 1` ([BP-0x24]); `att` = i32 attendance
([BP-0x2a]); `fixed` flag ([BP-0x2e]).

```c
u8 fixed = 0; i32 att = 0;
for (u8 m = 0; m < n; m++) {
    if (!(cup_round != 0 || ((tier_mask >> M[m].b[0x138]) & 1) || tier_mask == 0)) continue;
    if (cup_round == 1 && FAR:0008 > 4) { att = DS:2256; fixed = 1; }      // 76000
    u8 tier_away = first k in 0..3 with DS:2272[2k] <= away <= DS:2273[2k];   (4 if none)
    u8 tier_home = same for home;
    if (M[m].b[0x1e] != home && M[m].b[0x1e] != away) last = 0xff;
    if (M[m].b[0x1e] == home) {
        if (M[m].b[0x13d] == 'd' &&                          // 'd' = stadium blocked (100)
            !(FAR:2e8e == 5 && FAR:2eba >= 10)) { match_forfeit_penalty(m); return 1; }
        FAR:1d14[m] = 1;
        u8 mode = 1; if (cup_round > 1) { mode = 2; if (FAR:0009 > 4) mode = 3; }
        if (!fixed) att = compute_attendance(m, home, away, mode, gate_div);  // 1f9d:11e0, i32
        if (cup_round == 0) {
            M[m].b[0x14a + M[m].b[0x13a]] = (u8)ldiv(att, 1000L); // per-home-game attendance in thousands (2a58:2218, low byte kept)
            M[m].l[0x1e4] += att;                                // season attendance sum
            if (att >= M[m].l[0x1e8]) { M[m].l[0x1e8] = att; M[m].l[0x1f4] = away; }   // record high
            if (att <= M[m].l[0x1ec]) { M[m].l[0x1ec] = att; M[m].l[0x1f8] = away; }   // record low
            M[m].b[0x13a]++;                                     // home games played
        } else if (tier_away < M[m].b[0x138] && !fixed) {        // visitor from a higher tier
            i8 d = M[m].b[0x138] - tier_away;
            att += random_range((i16)ldiv(att, 8 - 3*d), (i16)att);
            i32 cap = M[m].l[0x15e] + M[m].l[0x166];             // stadium capacity
            if (cap < att) att = cap;
        }
        last = m;
        if (fixed) M[m].l[0x1f0] += 0x81e20;                     // 532000 DM
        else       M[m].l[0x1f0] += ldiv(lmul((i32)M[m].b[0x10a], att), gate_div);   // ticket price * att
    }
    if (M[m].b[0x1e] == away) {
        (same 'd' forfeit test) -> match_forfeit_penalty(m); return 2;
        FAR:1d14[m] = 1;
        if (gate_div == 2 && !fixed) {
            if (att == 0) {                                      // neutral ground
                i16 price = random_range(0, 1) + DS:5390[(u8)[BP-6]];      // {16,14,10,8}
                att = compute_attendance(m, home, away, 1, 2, 1, price);   // 7-argument call
                M[m].l[0x1f0] += ldiv(lmul((i32)price, att), 2);
            } else
                M[m].l[0x1f0] += ldiv(lmul((i32)M[last].b[0x10a], att), gate_div); // half of home gate
        }
        if (fixed) M[m].l[0x1f0] += 0x81e20;
        last = m;
    }
    // aftermath for the human manager involved (2a58:2530..29ae)
    u8 a = manager_index_of_team(home, 0xfe), b = manager_index_of_team(0xfe, away);
    u8 c = manager_index_of_team(home, away);
    FAR:3052[FAR:4bf0] = c & 0x7f;
    if (FAR:3052[FAR:4bf0] != 0 && (a || (b & 0x7f)) && last == m) {
        DS:0664[m] = 0x0b;
        u8 cnt = squad_count_by_flag(m, 0);
        for (u8 i = 0; i < 12; i++)                              // transfer pool players owned by m
            if (P[S[4][i].b[0x0f]].b[0x21] == m && (S[4][i].b[0x09] & 1) == 1
                && S[4][i].b[0x0d] != 0 && cup_round == 0) S[4][i].b[0x0d]--;
        for (u8 i = 0; i < cnt; i++) {
            s = &S[m][i];
            if ((s->b[0x09] & 1) == 1 && s->b[0x0d] != 0 && cup_round == 0) s->b[0x0d]--;  // ban served
            s->b[0x15] = 0;
            if (s->b[0x0a] != 0 && s->b[0x0a] < 12) {            // played (unsigned compare)
                i16 k = 0;                                       // [BP-8], reset per played player (2a58:2727)
                if (P[s->b[0x0f]].b[0x1c] > P[s->b[0x0f]].b[0x1d]) k = 1;   // u8 compare, JBE at 2a58:2742
                s->b[0x13] += (u8)(random_range(2, 4) - (u8)k + 6);  // 8-bit add, wraps
                if (cup_round == 0) P[s->b[0x0f]].b[0x23]++;     // league appearances
                i16 c2 = (cup_round != 0) + (cup_round > 1);
                s->b[0x06 + c2]++;  s->w[0x1c + 2*c2]++;         // appearances per competition
            }
            if (s->b[0x13] < 50) s->b[0x13] = 50;
            if (s->b[0x13] > 150) s->b[0x13] = 150;
        }
        if (cnt < 6 || cup_round != 0) { FAR:0178[2m] = 0; FAR:0179[2m] = 0; }
        else { FAR:0178[2m] = 6; FAR:0179[2m] = 1; }
        FAR:5396[2m] = 1; FAR:5397[2m] = 2;
        for (i = 0; i < 6; i++) FAR:1d1c[m*6 + i] = 0x7f;
        FAR:2e99 = 1;
        if ((u16)FAR:3052[FAR:4bf0] - m == 1) { FAR:305c[FAR:4bf0] = neutral; FAR:0028[FAR:4bf0] = p5; FAR:4bf0++; }
        if (m - a == (u16)-1 || m - (b & 0x7f) == (u16)-1) {     // m is the human home or away manager
            memset(&FAR:90ca[m*0x9a], 0, 0x9a);                  // last-match report record
            if (M[m].b[0x1e] == home) {
                opp = away; FAR:9160[m*0x9a/4..] (i32 at FAR:9160+m*0x9a) = att;
                if (M[m].l[0x15e] + M[m].l[0x166] == M[m].l[0x1e4]) FAR:90dc[m*0x9a] = 1;   // sold out
                i16 h = (i16)(M[m].l[0x166] - M[m].l[0x15e]) / 1000;   // seats - standing, low words, IDIV
                clamp_int(&h, 0, 30);
                h = 30 - h;
                if (random_range(0, h*4 + 15) == 0) M[m].b[0x13e] |= 1;   // hooligans next week
                me = home;
            } else {
                if (gate_div == 2) FAR:9160[m*0x9a] = att + 200000;
                opp = home; me = away;
            }
            FAR:90d8[m*0x9a] = me; FAR:90d9[m*0x9a] = opp;
            FAR:90d1[m*0x9a] = T[me].b[0x2e];  FAR:90d1[m*0x9a] = T[opp].b[0x2e];   // both writes hit 90d1 (asm 2a58:29dc/29f6); second wins
        }
    }
}
return 0;
```

RNG order per manager iteration: [`random_range(att/(8-3d), att)` (cup, higher-tier visitor)] →
[`random_range(0,1)` (neutral-ground away)] → per played player `random_range(2,4)` → hooligan
`random_range(0, h*4+15)`.

### 3.7 Match-screen helpers in 2a58 (called from the match module 1310/1f9d)

These belong to the live match presentation; they are listed for completeness. Pixel layout is given
as observed (graphics need ~90 %).

| Address | Name | Signature | Behaviour |
|---|---|---|---|
| 2a58:009d | `match_draw_score_digit_pair` | `void (u8 x, u8 y, u8 slot, u8 mode)` | Reads two digits for match slot `slot`: if `mode == 0` from `FAR:6dc8/6dc9[((FAR:305c[slot]*0x26 + DS:225a[FAR:305c[slot]])*20 + FAR:0028[slot])]` (two bytes), else from `FAR:4c7e/4c7f[FAR:305c[slot]*32 + FAR:0028[slot]]`; each value is reduced `while (v > 9) v -= 10`; blits digit glyph `v` (11x15 px, source x = `v*0x0b`, y = 0x72 of bitmap FAR:2eb4) to `(x+0x24, y+0x10)` and `(x+0x72, y+0x10)` |
| 2a58:024e | `match_draw_manager_bar_stats` | `void (u8 side, u8 m, u8 flag)` | `m == 0xff`: nothing. `flag` clamped to 1. Fills a 10x? box at `(DS:0632[side] + 0x7e + flag*16, DS:063c[side]+0x4d)` colour 0x0c, stores `FAR:5143[m*?] = squad_count_by_flag(m, 1)`, then draws 5 numbers `FAR:5140[m*?+i]` (i = 0..4, i = 4 replaced by 5 when `flag`) with `ui_draw_text_at` every 7 px from y = DS:063c[side]+0x52 |
| 2a58:0350 | `draw_text_split_two_lines` | `void (char far *s, i16 x0, i16 x1, i16 y)` | If `rastport_text_width(s) > 60` the first space is replaced by NUL and both halves are drawn centred at `y-5` and `y+1` (colour 1, no shadow); otherwise drawn once at `y` |
| 2a58:042f | `match_draw_live_screen` | `void (u8 cup_mode, u8 only_slot)` | Draws the whole live-match screen. Full spec in §3.10 |
| 2a58:0ca3 | `match_ticker_events` | `i16 (u8 type, u8 c, u8 p, u8 home, u8 away, u8 arg6, u8 pl_or_minute, u8 m_or_kind, u8 side, u8 arg10)` | One ticker event (chance/goal, red, yellow, injury) with statistics updates and 2 RNG calls. Full spec in §3.9 |
| 2a58:1f75 | `match_substitution_screen` | `void (u8 m, u8 cup_mode, u8 unused)` | Saves FAR:304a, sets it to `m`, `office_show_cached_background()` (18bc:3b92), `input_wait_release()`, `screen_squad_lineup(DS:0664[m], cup_mode)` (2ecd:1560), fade out, `ui_redraw_buttons(0x7f, 0)`, `ui_fill_rect(0,0,0x13f,0xef,0)`, `match_draw_live_screen(cup_mode, 0xff)`, font 2, wait release, fade in, restore FAR:304a. The third argument pushed by 2a58:0ca3 is never read. The lineup screen's exit runs `manager_ai_weekly(m, 1)` (1f9d:0002 with its random draws), so the rest of the match day depends on it (verified against DOSBox, scenario `in_season`) |

### 3.8 `matchday_info_panel` 1cb5:2bf9 (Ghidra cannot decompile it: address overflow)

UI only, no RNG. Despite the name it is the credits panel behind a click on the office header
(`office_handle_free_click` 18bc:08f2, `y < 0x2c`; the caller fades out before and redraws the
office after it). From the disassembly:

```c
ui_fill_rect(rp, 0, 0x2b, 0x13f, 0xef, 0);                    // the header stays
bm = vga_load_picture(0x27, 0, 0, 0, 1);                       // PIC\39.VGA, the developer logo
blit_bitmap(bm, 0, 0, rp, 0xf3, 0x2d, 0x4c, 0x4c, 0xc0); bitmap_free(bm);
ui_set_font(rp, 1);
for (i = 0; i < 3; i++) ui_draw_text_at(rp, string_xor_copy(DS:50a4 + 0x11*i, 0xc8), 0,    0x51 + 10*i, 0x09);
for (i = 0; i < 2; i++) ui_draw_text_at(rp, string_xor_copy(DS:50d8 + 0x0d*i, 0xc8), 0x7a, 0x55 + 10*i, 0x1d);
for (i = 0; i < 6; i++) ui_draw_text_at(rp, string_xor_copy(DS:50f2 + 0x0c*i, 0xc8), 0,    0x91 + 10*i, 0x1a);
for (i = 0; i < 7; i++) ui_draw_text_at(rp, string_xor_copy(DS:513a + 0x20*i, 0xc8), 0x7a, 0x91 + 10*i, 0x19);
ui_set_font(rp, 2);
ui_draw_text_centered_shadow(rp, string_xor_copy(DS:521a, 0xc8), 0, 0x13f, 0xee, 0x1a, 0);
screen_fade(FAR:2e70, 1); input_wait_click(FAR:6dc0); screen_fade(FAR:2e70, 0);
```

`string_xor_copy` 24a4:000a XORs every byte with the key into the buffer DS:a322. The texts
(scrambled in DS): "Programmidee", "Programmierung", "Digi-Sounds u.a." | "Werner Krahe",
"Jens Onnen" | "Grafik", "Titelmusik", "Torszenen", "", "Testspieler", "" | "Tobias Richter, S.
Dosch", "Sven Appel", "J}rgen Krahe,Werner Krahe", "Markus Bergmann", "Markus Bergmann,J}rgen
Krahe", "Olaf Ernst,J.Lagac,M.Schneider", "J.Haberstock,J.Beck" | "WENN SIE WOLLEN, K|NNEN SIE
JETZT DIE MAUSTASTE DR]CKEN...". Verified 100 % against DOSBox (scenario `in_season`).

The auto-play toggles of the same function: a left click at exactly (1, 1) while automatic play
is off shows `ui_dialog_box("Spiele automatisch", "weiter !")` (DS:0258, DS:024f), sets DS:05d4 =
1 and DS:063a = 0x47; while it is on, the pointer at exactly (0, 0) (no click needed: the
function runs on every poll then) shows `ui_dialog_box("Spiele nicht", "alleine !")` (DS:0242,
DS:0238) and clears DS:05d4. Otherwise every poll ends the turn.

### 3.9 `match_ticker_events` 2a58:0ca3 (logic + UI)

```c
i16 match_ticker_events(u8 type /*BP+6*/, u8 c /*+8*/, u8 p /*+0a*/, u8 home /*+0c*/, u8 away /*+0e*/,
                        u8 arg6 /*+10*/, u8 pl_or_minute /*+12*/, u8 m_or_kind /*+14*/,
                        u8 side /*+16*/, u8 arg10 /*+18*/);
```

Called only for matches shown live: by `match_minute_card_injury_events` 1310:2ee5 (types 2, 3, 4,
with `pl_or_minute` = squad slot, `m_or_kind` = manager), by `match_goal_chance` 1f9d:0c3b and by
`match_penalty_shootout` 1310:356d (types 0/1, with `pl_or_minute` = match minute and `m_or_kind` =
competition index 0 league / 1 DFB / 2 European). `type`: 0 missed chance, 1 goal, 2 red card
(`arg10 != 0`: yellow-red), 3 yellow card, 4 injury. `side` = 0 when the event belongs to the home team,
nonzero for the away team. For types 0/1 `arg10 != 0` means a penalty. The return value is the panel's
y origin + 1 (0 on the early exit). See match.md §4 for the call sites.

Locals: `hm` ([BP-8], u8, 0xff = none), `assist` ([BP-0xa], u8, starts 0xfe), `scene` ([BP-0x48], u8 = 0),
`key` ([BP-2]), `k` ([BP-0x82], u8 row), `ybase` ([BP-0x94], i16), `s` ([BP-0x8e], far squad slot ptr),
`att`/`def` ([BP-0x74]/[BP-0x76], attacking/defending team), `human` ([BP-0x92], u8).

```c
for (k = 0; k < 4 && FAR:5350[k] != home; k++) ;           // ticker row of this fixture (u8 loop)
ybase = ((i16)(3 - k) / 2) * 0x71;                          // CWD/SUB/SAR = trunc toward 0: k=0,1 -> 0x71; k=2,3,4 -> 0
ui_fill_rect(rp, 0x81, 0xe9, 0xc0, 0xef, 0);
for (i16 j = 0; j < n; j++)                                  // n = DS:07ab; the last match wins
    if ((M[j].b[0x1e] == home && side == 0) || (M[j].b[0x1e] == away && side != 0)) hm = j;
if (hm != 0xff && squad_count_by_flag(hm, 1) == 0) return 0;  // nobody on the pitch
if (type > 1) s = &S[m_or_kind][pl_or_minute];               // (25*m + pl)*0x34 + FAR:774a, u8 MUL
att = home; def = away; if (side) { att = away; def = home; }
// ---- panel frame (UI) ----
ui_set_font(rp, 2);
ui_fill_rect(rp, 0, ybase, 0x13f, ybase + 0x7e, 0);
match_ticker_minute(FAR:3057, ybase + 0x77, (arg10 != 0 && type != 2));      // 1310:2086
for (i = 0; i < 2; i++) blit_bitmap(FAR:5798, 0xa3, 0x66, rp, 0x73 + 0x4e*i, ybase + 0x6c, 0x0d, 0x0d, 0xc0);
ui_draw_frame(rp, 0, ybase, 0x13f, ybase + 0x7e, 1);
for (i = 0; i < 2; i++) {                                    // two badge boxes
    ui_fill_rect(rp, 0xf1*i + 0x13, ybase + 4, 0xf1*i + 0x3a, ybase + 0x2b, 0);
    ui_draw_frame(rp, 0xf1*i + 0x12, ybase + 3, 0xf1*i + 0x3b, ybase + 0x2c, 2);
    ui_fill_rect(rp, 0xf1*i + 0x13, ybase + 0x3c, 0xf1*i + 0x3b, ybase + 0x65, 0);
    ui_draw_frame(rp, 0xf1*i + 0x12, ybase + 0x3b, 0xf1*i + 0x3c, ybase + 0x66, 2);
}
for (j = 0; j < n; j++) {                                    // manager portraits (40x40 from FAR:2f24)
    if (M[j].b[0x1e] == home)      blit_bitmap(FAR:2f24, 0x29*j + 1, 0, rp, 0x14,  ybase + 0x3d, 0x28, 0x28, 0xc0);
    else if (M[j].b[0x1e] == away) blit_bitmap(FAR:2f24, 0x29*j + 1, 0, rp, 0x105, ybase + 0x3d, 0x28, 0x28, 0xc0);
}
i16 sy = (ybase == 0) * 0x73 + 0x23, sx = (k & 1) * 0xa1;   // stadium crops from FAR:a6ca
blit_bitmap(FAR:a6ca, sx + 0x16, sy, rp, 0x14,  ybase + 5, 0x26, 0x26, 0xc0);
blit_bitmap(FAR:a6ca, sx + 0x65, sy, rp, 0x105, ybase + 5, 0x26, 0x26, 0xc0);
blit_bitmap(FAR:5798, 0, 0, rp, 0x42, ybase + 2, 0xbc, 0x66, 0xc0);   // pitch picture
ui_set_font(rp, 1);
u8 v = (type == 2 || type == 3) ? 3 : (type == 4) ? 4 : 0;
if (v) { sound_play_voice(v); sound_wait_voice_end(); sound_restart_loop_voice(); }
switch (type) {                                               // jump chain at 2a58:1f21; type > 4: nothing
case 0: case 1: goto chance;                                  // 2a58:11b0
case 2: goto red;  case 3: goto yellow;  case 4: goto injury;
}
goto done;
```

**Types 0/1 (chance, goal)**, 2a58:11b0:

```c
u8 mi = manager_index_of_team(home, 0xff);                  // 1310:32b1: manager index + 1, 0 = none
if (mi) {
    if ((u16)mi - hm - 1 == 0) {                             // 16-bit test; hm == 0xff never matches
        if (hm != 0xff) { FAR:5144[6*hm]++; FAR:5710[hm]++; }
    } else { FAR:513f[6*mi]++; FAR:5777[mi]++; }             // = FAR:5140[6*(mi-1)+5], FAR:5778[mi-1]
}
mi = manager_index_of_team(away, 0xff);
if (mi) {
    if ((u16)mi - hm - 1 == 0) { if (hm != 0xff) { FAR:5145[6*hm]++; FAR:5778[hm]++; } }
    else { FAR:513e[6*mi]++; FAR:570f[mi]++; }               // = FAR:5140[6*(mi-1)+4], FAR:5710[mi-1]
}
if (DS:0607 == 0) {                                           // goal scenes off: blinking text
    ui_fill_rect(rp, 0x0a, ybase + 0x6c, 0x136, ybase + 0x79, 0);
    if (DS:05d4 == 0) for (i16 t = 0; t < 4; t++) {
        vga_wait_vretrace();
        if (arg10 == 0) strcpy(buf, "Chance f]r ");          // DS:4be0
        else { strcpy(buf, "Elfmeter f]r "); scene = arg10; } // DS:4bdc
        strcat(buf, C[att].name); strcat(buf, " !");         // DS:5383
        delay_ticks(6);
        ui_draw_text_centered_shadow(rp, buf, 0, 0x13f, ybase + 0x75, 0x0b, 0);
        vga_wait_vretrace(); delay_ticks(6);
        ui_fill_rect(rp, 0x0a, ybase + 0x6c, 0x136, ybase + 0x79, 0);
    }
} else {                                                      // TORE goal scene
    if (type < 2) match_screen_load_pictures();               // 24a4:01f9
    if (DS:05d4 == 0) delay_ticks(30);
    FAR:549a = 0; u8 prev = arg10;
    do {
        scene = prev = tore_scene_play(type, 1 - side, (u8)(ybase + 5), prev);   // 24a4:05ec
        FAR:549a++;
        if (DS:05d4 == 0) delay_ticks(20);
        key = dos_direct_console_io();
    } while ((i8)FAR:549a < 3 && (key == 'r' || key == 'w'));            // replay keys
    if (type < 2) match_screen_restore_pictures();            // 24a4:0347
}
human = 0;
if (DS:05d4 == 0 && DS:0607 == 0) { v = (type == 1) ? 2 : 5; sound_play_voice(v); sound_wait_voice_end(); sound_restart_loop_voice(); }
for (i16 m = 0; m < n; m++) {
    if (M[m].b[0x1e] != att) continue;
    i16 sc = match_pick_scorer(m, 0, type);                   // 1310:2c9a, 0xfe = nobody on the pitch
    human = 1;
    if (squad_count_by_flag(m, 1) > 1)
        do assist = (u8)match_pick_scorer(m, 1, 1); while ((u16)assist == sc);
    s = &S[m][sc];                                            // i16 IMUL; out of range when sc == 0xfe
    news_record_match_event(m, type, sc, assist, pl_or_minute);          // 3e3a:223e
    for (u8 j = 0; j < n; j++)
        if (M[j].b[0x1e] == def) news_record_match_event(j, type, 100, 100, pl_or_minute);
    if (type == 1) {
        blit_bitmap(FAR:5798, 0x7c, 0x66, rp, 0xf1*side + 0x14, ybase + 0x3d, 0x27, 0x28, 0xc0);
        strcpy(buf, "Torsch}tze: ");                          // DS:4be4
        if (arg10 == 0) {                                     // not a penalty
            if (m_or_kind == 0) P[s->b[0x0f]].b[0x22]++;      // league goals (top-scorer list)
            s->w[0x22 + 2*m_or_kind]++;                       // goals per competition (u16)
            s->b[0x03 + m_or_kind]++;                         // goals per competition (u8)
            s->b[0x15] += 15;                                 // mood, u8 wrap
        }
    } else {
        strcpy(buf, "Chance vergeben: ");                     // DS:4be8
        s->b[0x15] -= 10;
    }
    strcat(buf, sc != 0xfe ? P[s->b[0x0f]].name : "Eigentor !");   // DS:4bec
    if (type == 1 && arg10 == 0) {
        strcat(buf, " (");                                    // DS:5386
        strcat(buf, format_long_thousands((i32)(s->b[3] + s->b[4] + s->b[5])));   // u8s summed in i16
        strcat(buf, ")");                                     // DS:5389
    }
    ui_fill_rect(rp, 0x0a, ybase + 0x6c, 0x136, ybase + 0x7a, 0);
    if (type == 1 || sc != 0xfe) {
        ui_draw_text_centered_shadow(rp, buf, 0, 0x13f, ybase + 0x70 + 5*(assist == 0xfe), 1, 0);
        if (assist != 0xfe && scene == 0 && arg10 == 0) {
            strcpy(buf, "nach Vorlage von ");                 // DS:4bf0
            strcat(buf, P[S[m][assist].b[0x0f]].name);
            S[m][assist].b[0x15] += 10;
            ui_draw_text_centered_shadow(rp, buf, 0, 0x13f, ybase + 0x7a, 1, 0);
        }
    }
}
if (human == 0) for (i16 m = 0; m < n; m++) {                 // the human side conceded / defended
    if (M[m].b[0x1e] != def) continue;
    human = 3;
    news_record_match_event(m, type, 100, 100, pl_or_minute);
    for (i16 i = 0; i < 24; i++) {
        s = &S[m][i];
        if (s->b[0x0a] == 0 || s->b[0x0a] >= 12) continue;    // on the pitch only
        if (type == 1) s->b[0x15] += (P[s->b[0x0f]].b[0x1f] == 0) ? -8 : -5;
        else           s->b[0x15] += (P[s->b[0x0f]].b[0x1f] == 0) ?  8 : -3;
    }
}
if (human == 3 && type == 1)
    blit_bitmap(FAR:5798, 0x7c, 0x66, rp, 0xf1*side + 0x14, ybase + 0x3d, 0x27, 0x28, 0xc0);
if (DS:05d4 == 0) delay_ticks(0x78 - DS:063a);
goto done;
```

**Type 2 (red / yellow-red)**, 2a58:19ed:

```c
strcpy(buf, "Rote Karte: ");                                  // DS:26e0
if (arg10) { strcpy(buf, "Gelb-Rote Karte: "); s->b[2]++; s->b[1]--; }   // DS:26e4
else s->b[0]++;
strcat(buf, P[s->b[0x0f]].name);
if (hm != 0xff) FAR:5141[6*hm]++;
blit_bitmap(FAR:5798, 0x2a, 0x66, rp, 0xf1*side + 0x14, ybase + 0x3d, 0x28, 0x28, 0xc0);
ui_fill_rect(rp, 0x0a, ybase + 0x6c, 0x136, ybase + 0x79, 0);
ui_draw_text_centered_shadow(rp, buf, 0, 0x13f, ybase + 0x75, 0x12, 0x10);
s->b[0x0a] = 0;                                               // off the pitch
s->b[0x09] |= 1;                                              // suspended
s->b[0x0d] = (u8)random_range(1, 7 - 6*arg10);                // 2a58:1b36: red 1..7 games, yellow-red 1
s->b[0x15] -= 10;
news_record_match_event(m_or_kind, type, pl_or_minute, 0, 0);
DS:0664[m_or_kind]--;
squad_recalc_team_strength(m_or_kind, 1);                     // 1f9d:0002
if (DS:05d4 == 0) delay_ticks(0x82 - DS:063a);
goto done;
```

`7 - 6*arg10` is computed as `AL = 6; MUL byte arg10` (16-bit product) then `SUB AX,7; NEG AX`.

**Type 3 (yellow card)**, 2a58:1b93:

```c
strcpy(buf, "Gelbe Karte: "); strcat(buf, P[s->b[0x0f]].name);   // DS:26e8
if (hm != 0xff) FAR:5140[6*hm]++;
blit_bitmap(FAR:5798, 0x01, 0x66, rp, 0xf1*side + 0x14, ybase + 0x3d, 0x28, 0x28, 0xc0);
s->b[0x15] -= 10;  s->b[1]++;                                 // season yellow cards
strcat(buf, " ("); strcat(buf, format_long_thousands((i32)s->b[1])); strcat(buf, ")");   // DS:538b/538e
ui_fill_rect(rp, 0x0a, ybase + 0x6c, 0x136, ybase + 0x79, 0);
ui_draw_text_centered_shadow(rp, buf, 0, 0x13f, ybase + 0x75, 0x17, 0);
news_record_match_event(m_or_kind, type, pl_or_minute, 0, 0);
if (DS:05d4 == 0) delay_ticks(0x82 - DS:063a);
goto done;
```

**Type 4 (injury)**, 2a58:1cf9:

```c
strcpy(buf, "Verletzt: "); strcat(buf, P[s->b[0x0f]].name);    // DS:26ec
if (hm != 0xff) FAR:5142[6*hm]++;
blit_bitmap(FAR:5798, 0x53, 0x66, rp, 0xf1*side + 0x14, ybase + 0x3d, 0x28, 0x28, 0xc0);
player_injure(s, buf2);                                       // 24a4:30cf (other area: sets the injury, may use RNG)
strcat(buf, buf2);
ui_fill_rect(rp, 0x0a, ybase + 0x6c, 0x136, ybase + 0x79, 0);
ui_draw_text_centered_shadow(rp, buf, 0, 0x13f, ybase + 0x75, 0x12, 0x10);
if (DS:05d4 == 0 && random_range(0, 15) == 0) {               // 2a58:1e2c, only drawn when not fast mode
    for (u8 r = 0; r < 4; r++) for (i16 f = 0; f < 4; f++) { // stretcher animation
        i16 alt = (f == 1 || f == 3);
        blit_bitmap(FAR:5798, 0xa3 + 10*alt, 0x73 + 10*(f > 1), rp, 0xdc, ybase + 0x59, 0x0a, 0x0a, 0xc0);
        delay_ticks(13);
    }
} else if (DS:05d4 == 0) delay_ticks(150);
if (DS:05d4 == 0) match_substitution_screen(m_or_kind, arg6, k & 1);   // 2a58:1f75; arg6 = cup mode
```

**Exit** (2a58:1f3d): `ui_set_font(rp, 2); match_draw_manager_bar_stats(k, hm, side); return ybase + 1;`

RNG in this function: only `random_range(1, 7 - 6*arg10)` (red) and `random_range(0, 15)` (injury,
skipped when `DS:05d4 != 0`). `match_pick_scorer`, `tore_scene_play`, `player_injure` and
`squad_recalc_team_strength` consume RNG in their own areas, in the order shown above.

Quirks to keep: the `hm` loop takes the last matching manager; the scorer `s` pointer is computed even
when `sc == 0xfe` (writes into an unrelated slot in the goal branch: unreachable in practice because
the early exit returns when the side has nobody on the pitch); `ybase` is 0x71 for ticker rows 0 and 1
and 0 for rows 2..4.

### 3.10 `match_draw_live_screen` 2a58:042f (UI)

`void match_draw_live_screen(u8 cup_mode, u8 only_slot)`. Callers: 1310 live-match view (with
`only_slot = 0xff`), `match_substitution_screen`. `cup_mode != 0` reads the cup pairing table
FAR:76ca, else the league fixture table FAR:4b5e. No RNG.

```c
u8 badge = 0; i16 row = 0;                                    // [BP-0x56], [BP-0x60]
screen_fade(FAR:2e70, 0); rastport_clear(rp, 0);
for (i32 i = 0; i < 5; i++) FAR:4c6c[2*i] = 0xff;
for (i32 i = 0; i < (i32)FAR:4bf0; i++) {                     // match slots of this half-week (32-bit loop)
    FAR:2e92 = 0; FAR:5794[i] = 0;
    if (only_slot != 0xff && only_slot != i) goto next;
    i32 x = (i16)DS:0632[row], y = (i16)DS:063c[row];        // panel origin
    row++;
    blit_bitmap(*(bitmap far **)FAR:57c4, 0, 0, rp, x + 3, y, 0x9b, 0x72, 0xc0);   // panel picture
    u8 h = cup_mode ? FAR:76ca[FAR:305c[i]*32 + FAR:0028[i]] : FAR:4b5e[FAR:305c[i]*20 + FAR:0028[i]];
    FAR:4c6a[2*row] = h;                                      // == FAR:4c6c[2*(row-1)]
    draw_side(h, x + 0x16, y + 0x23);
    FAR:534f[row] = h;                                        // == FAR:5350[row-1] (ticker row lookup in 0ca3)
    ui_set_font(rp, 2);
    strcpy(buf, C[h].name); draw_text_split_two_lines(buf, x + 5, x + 0x4b, y + 0x0c);
    u8 a = cup_mode ? FAR:76cb[...same index...] : FAR:4b5f[...same index...];
    FAR:4c6b[2*row] = a;
    draw_side(a, x + 0x65, y + 0x23);                         // also sets font 2 before the badge blit
    strcpy(buf, C[a].name); draw_text_split_two_lines(buf, x + 0x55, x + 0x99, y + 0x0c);
    i32 v = 0;
    u8 mi = manager_index_of_team(h, 0xfe); if (!mi) mi = manager_index_of_team(a, 0xfe);
    if (mi) v = *(i32 far *)(FAR:90c6 + mi*0x9a);            // == FAR:9160 + (mi-1)*0x9a: attendance (2a58:20b2)
    if (v > 200000) v -= 200000;                              // signed 32-bit
    u8 col = 1;
    if (v != 0) {
        DS:07b2 = 0; strcpy(buf, format_long_thousands(v)); DS:07b2 = 1;
        if (M[mi-1].l[0x15e] + M[mi-1].l[0x166] == v) col = 0x11;   // sold out
    } else strcpy(buf, "(AUSW.)");                            // DS:26dc
    ui_draw_text_centered_shadow(rp, buf, x + 9, x + 0x33, y + 0x63, col, 0);
    for (i32 j = 0; j < n; j++) {
        if (M[j].b[0x1e] == h) match_draw_manager_bar_stats(row - 1, j, 0);
        if (M[j].b[0x1e] == a) match_draw_manager_bar_stats(row - 1, j, 1);
    }
    match_draw_score_digit_pair((u8)x, (u8)y, row - 1, cup_mode);
next:
    if (FAR:2e92 == 2) FAR:5793[row] = 1;                     // both teams human (== FAR:5794[row-1])
}
FAR:4bf1 = 1;                                                 // badge cache is valid from now on
match_draw_time_bar(FAR:3057);                                // 1310:1def
screen_fade(FAR:2e70, 1);

draw_side(team t, x0, y0):
    for (i32 j = 0; j < n; j++) if (M[j].b[0x1e] == t) {
        FAR:2e92++; blit_bitmap(FAR:5354, 0x29*j, 0, rp, x0, y0, 0x26, 0x26, 0xc0);   // manager portrait
    }
    if (no manager matched) {
        if (FAR:4bf1 == 0) {                                  // first draw: load badge, store in cache
            i16 pic = (t < 0x40) ? (C[t].b[0x21] & 0x7f) + 0x50 : FAR:305c[i] + 0x90;
            bm = vga_load_picture(pic, 0, 0L, 1);             // 176b:0356
            blit_bitmap(bm, 0, 0, rp, x0, y0, 0x26, 0x26, 0xc0);
            blit_bitmap(bm, 0, 0, *(bitmap far **)DS:05b4, 0x26*badge, 0, 0x26, 0x26, 0xc0);
            bitmap_free(bm);
        } else blit_bitmap(*(bitmap far **)DS:05b4, 0x26*badge, 0, rp, x0, y0, 0x26, 0x26, 0xc0);
        badge++;                                              // 8-bit; badge*0x26 is an 8-bit IMUL
    }
```

The `only_slot` path still increments `row`, so a single redrawn fixture always lands in panel 0 and
passes `row - 1` (not `i`) to `match_draw_score_digit_pair`.

---------------------------------------------------------------------------------------------------

## 4. Segment 1cb5

### 4.1 `set_copy_protection_passed` 1cb5:0000

`void (void) { DS:05ac = 1; }` — called by the load-game screen.

### 4.2 `player_suspension_countdown` 1cb5:2b88

```c
void player_suspension_countdown(u8 far *s) {          // s = squad slot (0x34 bytes)
    if ((s[0x09] & 2) == 2 && s[0x0d] != 0 && lrem(day - (i32)(i8)DS:224c, 7) == 0) s[0x0d]--;
    if ((s[0x09] & 3) != 0 && s[0x0d] == 0) { s[0x09] &= 0xfc; squad_weekly_update(); }   // 2ecd:3360
}
```

Callers: random_weekly_events (2x), season_main_loop (summer break).

### 4.3 `random_weekly_events` 1cb5:13bd

`i16 random_weekly_events(void)` — once per manager `m = FAR:304a` per half-week after the day
advance. Return value is unused (garbage on early exit, 0 otherwise).

Locals: `base = 0x25 - (DS:4a28 == 5)` ([BP-4], i16: 37, or 36 on the easiest level — DS:4a28 =
`5 - user level`), `tot[3]` i16 = 0, `processed` i16 = 0.

```c
if (day > 0x141) return;                                   // signed 32-bit
// 4.3.1 transfer-pool maintenance, only for manager 0
if (FAR:304a == 0) {
    for (i16 i = 0; i < squad_count_by_flag(4, 0); i++) {      // count re-evaluated every pass
        s = &S[4][i];
        if (s[0x13] > 0x38) s[0x13] -= random_range(3, 9);
        if (random_range(0, 3) == 0 && s[0x09] > 0x1f && s.ptr[0x30] != 0) {
            s[0x09] &= 0x1f; s.ptr[0x30] = message_delete(s.ptr[0x30]);   // 4091:0044, returns far ptr (NULL after delete)
        }
    }
    for (i16 g = 0; g < 3; g++) for (i16 j = 0; j < 10; j++)
        if (FAR:1d34[g*10+j] != 0 && --FAR:1d34[g*10+j] == 0) message_delete(FAR:5664[g*10+j]);
}
// 4.3.2 transfer pool: offers from other clubs for players the manager put on the list
i16 cnt4 = squad_count_by_flag(4, 0);
for (i16 i = 0; i < cnt4; i++) {
    FAR:5358 = FAR:304a;
    s = &S[4][i];
    if ((s[0x03] & 0x7f) == (1 << FAR:304a) && random_range(0, 2) == 0) s[0x03] ^= 1 << FAR:304a;
    if (P[s[0x0f]].b[0x21] != (i8)FAR:304a) continue;           // owner
    player_suspension_countdown(s);
    if (s[0x18] & 0x80) continue;
    FAR:304a = 4;
    i16 q = (i16)ldiv(player_market_value(i, 0), 10000);       // 3277:25de(u8 idx, u8 0) -> i32
    if (random_range(0, 180) < q && random_range(0, 2) != 0 && s.ptr[0x30] == 0) {
        s[0x09] |= 0x80;
        s[0x16] = pick_buying_club(i, (u8)q, 0);               // 24a4:24bf
        s.ptr[0x30] = message_post(0, 1L, &C[s[0x16]], &P[s[0x0f]], (i32)i, 0L, 0L);   // 4091:0190, 13 words (ADD SP,0x1a)
    }
    FAR:304a = FAR:5358;
}
r = M[FAR:304a];
// 4.3.3 hooligans (flag set by 2a58:20b2)
if (r.b[0x13e] & 1) {
    l1 = "Randalierer im Stadion"; l2 = "richteten einen Sachschaden"; l3 = "von ";
    i16 k = random_range(2, 5);
    i32 dmg = lmul(k, (r.l[0x166] << 2) + (r.l[0x15e] >> 1));   // seats*4 + standing/2 (SAR)
    dmg = lmul(ldiv(dmg, 1000), 1000);
    r.l[0x1f0] -= dmg;  r.b[0x13e] ^= 1;
    l3 += [DS:4f78 if DS:224d] + fmt(ldiv(lmul(DS:2252, dmg), DS:224e)) + " DM an.";
    if (r.l[0x186] > 1) {                                   // stadium comfort grade
        if (r.l[0x1f0] > 2000000) { if (random_range(0,1) == 0 || random_range(0,2) == 0) r.l[0x186]--; }
        else if (random_range(0, 2) == 0) r.l[0x186]--;
    }
    show_message(l1, l2, l3, 2, 0);                         // 1000:239e
}
// 4.3.4 comfort downgrade (only Bundesliga)
if (random_range(0, (i16)(0x1ba - 0x34 * (i16)r.l[0x18e])) == 0 && r.l[0x18e] > 1 && r.b[0x138] == 0) {
    show_message("In der neuen Komfortbewertung", "wird ihr Stadion um eine Note", "schlechter beurteilt.", 2, 0);
    r.l[0x18e]--;
}
i16 cnt = squad_count_by_flag(FAR:304a, 0);
// 4.3.5 per-player events during the season
if (day < 0x142) for (i16 i = 0; i < cnt; i++) {
    s = &S[FAR:304a][i];
    i16 risk = (((DS:2280[FAR:016e] == 0) ? 25 : 0) - s[0x13] + 160) * 2;  if (risk < 40) risk = 40;
    if (s[0x0d] == 0 && s[0x09] == 0 &&
        random_range(0, (i16)(i8)DS:4a28 * 30 + risk + 80) == 0) {           // training injury
        player_injure(s, l3);   // 24a4:30cf (other area): applies the injury and writes its text into l3
        show_message("Verletzung im Training:", P[s[0x0f]].name, l3, 2, 0);
        squad_weekly_update();
    }
    if (random_range(0x20, 0x2d) < player_unrest(s) &&                       // 24a4:2588
        !(s[0x18] & 0x80) && s[0x18] < 100) {
        s[0x18] |= 0x80; FAR:5358 = FAR:304a;
        u8 slot = team_slot_free_index(FAR:304a);                            // 4091:0000
        void far *msg = message_post(0, 4L, &P[s[0x0f]], 0L, 0L, 0L, 0L);
        if (slot == 0x7f) message_delete(msg);
        else { FAR:5664[FAR:304a*10 + slot] = msg; FAR:1d34[FAR:304a*10 + slot] = 3; }
    }
    if (s[0x0d] != 0) { s[0x0a] = 0; squad_weekly_update(); }
    if (random_range(0, 3) == 0 && s[0x09] > 0x1f && s.ptr[0x30] != 0) {
        s[0x09] &= 0x1f; s.ptr[0x30] = message_delete(s.ptr[0x30]);
    }
    if (s[0x18] > 99 && !(s[0x18] & 0x80)) {
        if (random_range(0, 5) == 0 && s.ptr[0x30] != 0) {
            s[0x18] = random_range(9, 17); s.ptr[0x30] = message_delete(s.ptr[0x30]);
        }
    } else if (s[0x18] != 0 && !(s[0x18] & 0x80)) s[0x18]--;
    if (s[0x0c] != 0) continue;
    // transfer interest from abroad (only players with s[0x0b] == 1)
    i16 avg = (s[0x10] + s[0x11] + s[0x12]) / 3;                              // IDIV
    i16 a = (i16)long_abs_diff(0, 25 - P[s[0x0f]].b[0x1a]);  if (a > 8) a = 8;   // age distance
    a = (i16)ldiv(-100L * a, 8) + 100;
    i16 b = (u16)(s.w[0x1c] + s.w[0x1e] + s.w[0x20]) / 36;  if (b > 5) b = 5;
    i16 c = (i16)ldiv(40L*a + 60L*avg, 100);
    c += (i16)ldiv((i32)(i16)(c * b) * 10, -100);
    if (random_range(0, c) == 0 && random_range(0, 3) == 0 && s.ptr[0x30] == 0
        && !(s[0x18] & 0x80) && s[0x0b] == 1) {
        i16 r100 = random_range(0, 100);
        i16 w = r100 > 90 ? 4 : r100 > 70 ? 3 : 2;
        s[0x18] = w + 100;
        s.ptr[0x30] = message_post(0, 0L, &P[s[0x0f]], (i32)s[0x0b], (i32)w, 0L, 0L);
    }
}
cnt = squad_count_by_flag(FAR:304a, 0);
// 4.3.6 offers from other clubs for own players (rare)
if (day < 0x142) for (i16 i = 0; i < cnt; i++) {
    s = &S[FAR:304a][i];
    if (s[0x0f] == 0 || s[0x0c] != 0 || (s[0x18] & 0x80)) continue;
    i16 q = (i16)ldiv(player_market_value(i, 0), 10000);
    if (random_range(0, 200) < q && random_range(0, 450) == 0 && s.ptr[0x30] == 0) {
        s[0x09] |= 0x40;
        u8 star = (q > 90 && random_range(0, 12) == 0);
        s[0x16] = pick_buying_club(i, (u8)q, star);
        s.ptr[0x30] = message_post(0, 1L, &C[s[0x16]], &P[s[0x0f]], (i32)i, 0L, 0L);
    }
}
// 4.3.7 training: attribute development
i16 w4[4];  for (i = 0; i < 4; i++) w4[i] = 20 * r.b[0x141 + i];             // training plan (u8*u8)
i16 mix[3]; for (j = 0; j < 3; j++) { mix[j] = 0; for (i = 0; i < 4; i++) mix[j] += (i8)DS:0292[3*i+j] * w4[i]; mix[j] /= 400; }
for (i16 i = 0; i < cnt; i++) {
    s = &S[FAR:304a][i];
    player_suspension_countdown(s);
    i16 inten = r.b[0x145];                                                   // training intensity
    if (s[0x13] > 0x32) s[0x13] -= random_range(3, 7);
    if (in_winter_window(day) && inten < 6) inten = 6;                         // same test as §1.3
    s[0x13] += (u8)((inten - 5) * 2);
    if (s[0x13] > 0x82 && inten < 6) s[0x13] = s[0x13] - random_range(1, 3);
    inten *= 20;
    if (s[0x13] < 0x3c) s[0x13] = 0x3c;  if (s[0x13] > 0x96) s[0x13] = 0x96;
    if (s[0x13] > 0x87) {                                                      // overtrained
        i16 L = (i8)DS:4a28;
        inten -= random_range(70 - 13*L, 5*(18 - L));  if (inten < 10) inten = 10;
    }
    if (P[s[0x0f]].b[0x1f] / 25 == 0) inten += 30;
    for (i16 j = 0; j < 3; j++) {                                              // s[0x10..0x12]
        i16 t = mix[j] * inten / 100;
        t = t * s[0x0e] / 50;
        i16 f = 10 * r.b[0x146 + P[s[0x0f]].b[0x1f] / 25] + 75;                // trainer quality
        f -= random_range(0, 3);
        t = t * f / 100;                                                       // all IDIV i16
        i16 v = s[0x10 + j];
        tot[j] += t;
        i16 target = (random_range(0, 4) == 4) ? 1 : 0;   i16 dir = 0;
        if ((i8)DS:4a28 > 2) {
            if (in_winter_window(day) || day > 0x142) {
                if (random_range(0, 2) == 0) target++;
                target = random_range(base - target - 1, base + 2);
                s[0x13] += random_range(0, 2);
            } else target = random_range(base - target - 2, base + 2);
        } else target = random_range(base - target - 2, base + 2);
        if ((i8)DS:4a28 < 2 && (in_winter_window(day) || day > 0x142)) s[0x13] += random_range(0, 2);
        if (t < target - 8 && random_range(0, 1) != 0) { dir = -1; target = (target - t + 8) / 11; }
        else if (t > target + 8 && random_range(0, 1) != 0) { dir = 1; target = (t - target + 8) / 11; }
        if (random_range(0, 3) == 0 && dir != 0) {
            if (target > 2) target = random_range(2, 3);
            if (target != 0) {
                v += random_range(1, target) * dir;
                if (j < 2) clamp_int(&v, 1, 99); else clamp_int(&v, 45, 55);
                s[0x10 + j] = (u8)v;
            }
        }
    }
    processed++;
    i16 form = s[0x0e];
    if (random_range(0, 3) != 0) {
        if (s[0x14] & 0x80) form += (s[0x14] & 0x70) >> 4; else form -= (s[0x14] & 0x70) >> 4;
    }
    i16 left = (s[0x14] & 0x0f) - 1;
    s[0x14] = (s[0x14] & 0xf0) + (u8)left;
    if (left == 0) form_trend_reroll(s);                                       // 3277:000a(s)
    else { clamp_int(&form, 30, 68); s[0x0e] = (u8)form; }
}
squad_weekly_update();
return 0;
```

Exact branch nesting of the `target` block (asm 2a58:204d..2121): the first `random_range(0,4)` is
always called; the `DS:4a28 > 2` branch contains the `random_range(0,2)` increment and the second
`random_range(0,2)` added to `s[0x13]`; the `DS:4a28 < 2` test follows unconditionally. `in_winter_window`
is `day > (i8)DS:224d*7+130 && day < 207-(i8)DS:224d*14`; the extra `day > 0x142` alternative is
never true here (early exit). `tot[]` and the `[BP-0x12]`/`[BP-0x18]` sums are written but never read.

Callers: season_main_loop. Callees: listed inline.

### 4.4 `squad_fix_position_mismatch` 1cb5:25d5

```c
void squad_fix_position_mismatch(void) {                  // m = FAR:304a
    for (u8 i = 0; i < 24; i++) {
        s = &S[m][i]; u8 p = s[0x0f];
        u8 d = (u8)long_abs_diff((i32)(i8)s[0x19], (i32)P[p].b[0x20]);
        if (p == 0 || s[0x0a] == 0 || s[0x0a] >= 12 || d <= 1) continue;
        u8 best = 99; slot far *bs = 0;
        for (u8 j = 0; j < 24; j++) {
            t = &S[m][j];
            if (j == i || t[0x0f] == 0 || t[0x0a] == 0 || t[0x0a] >= 12 || t[0x1a] != s[0x1a]) continue;
            u8 dj = (u8)long_abs_diff((i32)(i8)s[0x19], (i32)P[t[0x0f]].b[0x20]);
            if (dj < best) { best = dj; bs = t; }
        }
        if (bs && d > best) swap(s[0x19], bs[0x19]);
    }
}
```

No RNG. Caller: one site in segment 1cb5 (FUNCTION_MAP lists 1 caller).

### 4.5 `rank_tier_teams_by_strength` 1cb5:2a3b

```c
void rank_tier_teams_by_strength(i8 k, u8 far *out) {
    u8 cnt = DS:225e[k]; u16 s[?];
    for (u8 i = 0; i < cnt; i++) {
        out[i] = i; s[i] = 0;
        u8 t = DS:226e[k] + i;
        for (u8 j = 0; j < 3; j++) s[i] += C[t].b[0x18+j] + C[t].b[0x1b+j] + C[t].b[0x1e+j];
        s[i] /= 9;                                         // unsigned
    }
    for (u8 i = 0; (i16)i < cnt - 1; i++)
        for (u8 j = i + 1; j < cnt; j++)
            if (s[i] < s[j]) { swap(s[i], s[j]); swap(out[i], out[j]); }   // unsigned compare
}
```

### 4.6 `pick_random_free_team_in_tier` 1cb5:2987

```c
u8 pick_random_free_team_in_tier(i8 k) {
    if (k > 2) return (u8)random_range(DS:2272[2*k], 0xff);
    u8 t;
    do {
        t = random_range(0, 3);
        if (random_range(0, 1) != 0) t = random_range(0, 6);
        if (random_range(0, 5) == 0) t = random_range(0, 12);
        t += DS:226e[k];
    } while (manager_index_of_team(t, 0xff) != 0);
    return t;
}
```

Callers: 1cb5:2756, 24a4:19fb.

### 4.7 `season_rebalance_player_clubs` 1cb5:2756

Moves persons (`P[p]`, e.g. players/coaches of AI clubs) between tiers so that each tier gets the
number requested by 24a4:1798 (`player_pool_season_shuffle`, stats.md §3.3, which also runs the
yearly pool shuffle, so it runs twice here).

```c
void season_rebalance_player_clubs(void) {
    u8 rank[3][20];
    for (i16 k = 0; k < 3; k++) rank_tier_teams_by_strength((i8)k, rank[k]);   // result unused afterwards
    u8 pool[0x97][4]; memset(pool, 0xff, sizeof pool);      // pool[idx][k]
    i16 want[4];  tier_person_quota(want);                   // 24a4:1798(i16 far *), writes want[0..3]
    want[3] = 0;
    i16 have[4], avail[4];
    for (i16 k = 0; k < 4; k++) {
        have[k] = 0;
        for (i16 p = 1; p < 0x97; p++) {
            if (P[p].b[0x24] < DS:2272[2k] || P[p].b[0x24] > DS:2273[2k]) continue;   // club in tier k
            if (want[k] != 0) want[k]--;
            if (P[p].b[0x21] <= 3) continue;                 // owned by a human manager
            pool[have[k]][k] = (u8)p; have[k]++;
        }
    }
    for (k = 0; k < 4; k++) avail[k] = have[k];
    for (i16 k = 0; k < 4; k++) {
        i16 guard = 0;
        while (want[k] > 0 && guard++ < 1000) {
            for (i16 j = 0; j < 4; j++) {
                if (j == k) continue;
                while (avail[j] > 0 && want[k] > 0) {
                    i16 r, tries = 0;
                    do r = random_range(0, have[j] - 1); while (pool[r][j] == 0xff && tries++ < 1000);
                    u8 p = pool[r][j]; pool[r][j] = 0xff;
                    P[p].b[0x24] = pick_random_free_team_in_tier((i8)k);
                    avail[j]--; want[k]--;
                }
            }
        }
    }
    tier_person_quota(want);
}
```

Caller: season_end_messages (end).

### 4.8 `season_end_messages` 1cb5:0012

`void season_end_messages(u8 far *scorer_flag)`. Caller: season_main_loop, once at season end.
`scorer_flag[m] != 0` means manager `m` has the league top scorer. The function shows, per manager,
the "Saisonende" report, the "Trennungen" ageing/transfer-back pass (manager 0 only), and the
retirement/regeneration pass. After that it resets the season counters, recomputes DS:05fe, runs the
"Vertragsverhandlungen" contract negotiations and calls `season_rebalance_player_clubs`. Everything
below was checked against the disassembly (`out/season/1cb5_0012.asm`).

Screens (port `season_end::season_end_event`): `office_redraw_with_header(title)` 24a4:260a is
`office_show_cached_background()`, `copy_protection_penalty(title, 0, 0, 0)` (the title is not
drawn; one `random_range(0, 100)` only when the protection was not passed) and
`ui_draw_manager_header()` 4091:185c: `ui_draw_button(rp, 0x109, 5, 0x2a, 0x2a, &tmp, 0, 8)`,
`blit_bitmap(FAR:2f24, m*0x29, 0, rp, 0x10a, 6, 0x28, 0x28, 0xc0)` (portrait), font 1,
`ui_draw_text_centered_shadow(rp, M[m].name, 0x113, 0x127, 0x39, 0, 1)`,
`ui_draw_button(rp, 0x109, 0x3c, 0x2a, 0x29, &tmp, 0, 0)`,
`blit_bitmap(FAR:5354, m*0x29, 0, rp, 0x10b, 0x3e, 0x28, 0x26, 0xc0)` (crest). The "Trennungen"
screen is that header alone; it stays up for the retirement messages and the 70-tick delay.
`contract_negotiation(sl, 1L, 0, 0)` is the single mode (transfer.md §4.10) with the late-season
rule (`day_counter > 0x141`): "NEUER VERTRAG" with nothing entered counts as "ABBRUCH". In DOSBox
`dialog_okay` puts the pointer back where it was, so a scripted click without host motion lands
there (the scenario `season_end` moves the host pointer before every click).

Locals: `hist` ([BP-0x88], i16) = `min((i16)DS:07e2, 0x31)` (signed compare); `any_tier[3]` ([BP-6], i8)
and `tier_cnt[4]` ([BP-0xe], i8), all zeroed; `y` ([BP-0x6c], i16); `yp` ([BP-0x7e], i16, also holds
`flags` earlier); `p` ([BP-0x7c], i16, see quirk Q11); `g`/`sl` ([BP-0x86]/[BP-0x68], u8 outputs of
`find_player_by_id`); `ns` ([BP-0x80], i16).

Callees (other areas): `find_player_by_id(u8 p, u8 *group, u8 *slot)` 4091:17b7 returns the squad slot
pointer (`&S[group][slot]`) or NULL (then `*group = 0xff`), searching groups `0..n-1` and 4;
`squad_insert_player(u8 p, u8 a, u8 b, i32 0, u8 last)` 2ecd:37d8 returns the new slot index in the
squad of `FAR:304a`; `squad_remove_player(u8 slot, i8 force, u8 last)` 2ecd:10ee removes `slot` from the
squad of `FAR:304a` (deletes its message, shifts slots `slot..last-1` down, clears `S[..][last].b[0x0f]`);
`player_value_calc(u8 slot, u8 mode)` 3277:25de returns i32 for the squad of `FAR:304a`;
`count_players_of_owner(u8 m)` 212a:00b4; `team_is_euro_qualified(u8 team)` 27c2:11e7;
`append_ordinal_suffix(buf, u8 n)` 1310:1fe7; `text_to_upper_de(src, dst)` 4091:105d;
`office_redraw_with_header(char far *title)` 24a4:260a; `contract_negotiation(u8 slot, i32, i32, i32)`
3277:2a8f; `squad_list_draw(...)` 2ecd:06af; `screen_messages(msg)` 4091:05c4; `message_delete(msg)`
4091:0044 (returns the new pointer value, stored back).

```c
for (FAR:304a = 0; (i16)(i8)FAR:304a < (u16)DS:07ab; FAR:304a++) {      // m = FAR:304a
    FAR:5358 = m;
    r = &M[m];
    u8 flags = r->b[0x140];
    tier_cnt[r->b[0x138]]++;
    u8 scorer = scorer_flag[m];
    // ---------------- report screen (UI) ----------------
    office_redraw_with_header("Saisonende");                                   // DS:2740
    ui_draw_button(rp, 0x17, 0x19, 0xde, 0x3c, &tmp, 0, 8);
    ui_draw_button(rp, 0x0a, 0x7e, 0x127, 0x52, &tmp, 0, 8);
    ui_set_font(rp, 1);
    ui_draw_text_centered_shadow(rp, "Sie beendeten Ihre Saison", 0x17, 0xf5, 0x21, 0x0b, 0xff);   // DS:2744
    ui_draw_line(rp, 0x18, 0x24, 0xf4, 0x24, 0x0b);
    ui_draw_text_centered_shadow(rp, "Ereignisse", 0x0a, 0x131, 0x86, 0x0b, 0xff);                // DS:2748
    ui_draw_line(rp, 0x0b, 0x89, 0x130, 0x89, 0x0b);
    // history entry hist: r->b[0x3e + 4*hist] = absolute rank, r->b[0x3f + 4*hist] = tier
    u8 tier_h = r->b[0x3f + 4*hist];
    i32 rank = (i32)(i16)((u16)r->b[0x3e + 4*hist] - (i16)(i8)DS:226e[tier_h]);   // 1-based rank in tier
    strcpy(buf, format_long_thousands(rank));
    append_ordinal_suffix(buf, (u8)rank);                                       // adds "."
    strcat(buf, "Platz der ");                                                  // DS:274c
    strcpy(buf2, DS:2388[tier_h]);   // far ptr table: "Bundesliga", "Zweite Liga", "Am.-Oberliga"
    text_to_upper_de(buf2, buf2);
    strcat(buf, buf2);
    ui_draw_text_centered_shadow(rp, buf, 0x17, 0xf5, 0x3c, 1, 0xff);
    y = 0x94;
    if (flags & 1) {                                                            // promoted
        if (r->b[0x138] != 0) {                                                 // now in 2. Liga
            ui_draw_text_centered_shadow(rp, "Gratulation zum Aufstieg (800.000 DM).", 0x0a, 0x131, y, 0x0b, 0xff);   // DS:2754
            r->l[0x1f0] += 800000;
        } else {                                                                // now in Bundesliga
            ui_draw_text_centered_shadow(rp, "Gratulation zum Aufstieg (1.600.000 DM).", 0x0a, 0x131, 0x94, 0x0b, 0xff); // DS:2750
            r->l[0x1f0] += 1600000;
        }
        if (FAR:0000[2*m] == 0) *(i32 *)&DS:066c[m*0x24] /= 10;                 // _aFlldiv, signed
        else {
            FAR:0000[2*m] = 0;
            dialog_okay("Ihre Werbepartner erlauben", "es, die Vertr{ge jederzeit",
                        "zu k}ndigen. Die Einnahmen", "bleiben solange erhalten.");   // DS:4f44..4f50
        }
        for (i16 j = 0; j < 6; j++) {                                           // p ([BP-0x7c]) is the loop variable
            if (FAR:4b9a[(m*6 + j)*2] == 0) *(i32 *)&DS:0670[m*0x24 + 4*j] /= 10;
            FAR:4b9a[(m*6 + j)*2] = 0;
        }
        y += 13;
    }
    ui_set_font(rp, 1);
    if (flags & 2) { draw(y, "Ihr Abstieg ersch]ttert Millionen Fans.");       y += 13; }   // DS:2758
    if (flags & 4) { draw(y, "DFB verweigert die Lizenz. Sie steigen ab.");    y += 13; }   // DS:275c
    if (flags & 8) { draw(y, "Sie beginnen neu mit 500.000 DM.");              y += 13; }   // DS:2760
    if (scorer)    { draw(y, "Torsch]tzenk|nig aus Ihrem Team (250.000 DM)."); y += 13;     // DS:2764
                     r->l[0x1f0] += 250000; }
    if (team_is_euro_qualified(r->b[0x1e])) { draw(y, "Sie sind f}r den Europapokal qualifiziert."); y += 13; }  // DS:2768
    // draw(y, s) = ui_draw_text_centered_shadow(rp, s, 0x0a, 0x131, y, 0x0b, 0xff)
    // ---------------- youth player ----------------
    yp = (i16)((u16)r->w[0x1e2] / 12);                                          // unsigned DIV
    i32 yr = FAR:a7a0;                                                          // signed 32-bit compares
    if ((yr > 1966 && FAR:56f4 == 0) || (yr > 1995 && FAR:56f4 != 0)) {
        yp += yp / -3;                                                          // IDIV, truncates toward 0
        r->w[0x1e2] = (u16)(yp * 12);                                           // 16-bit IMUL, low word
        if (yp > 30 && random_range(0, 2) == 0 && count_players_of_owner(m) < 23) {   // u8 compare
            yp >>= 1;                                                           // SAR
            draw(y, "Ein Jugendspieler steigt in den Kader auf."); y += 13;     // DS:276c
            do p = random_range(1, 0x96); while (P[p].b[0x21] != 5);           // a free person
            clamp_int(&yp, 10, 90);
            P[p].b[0x20] = random_range(0, 6);                                  // position
            P[p].b[0x1a] = random_range(0x11, 0x13);                            // age 17..19
            P[p].b[0x1e] = random_range(0x2d, 0x37);
            P[p].b[0x1d] = random_range(yp - 3, yp + 3);                        // 1cb5:04c5
            P[p].b[0x1c] = random_range(yp - 3, yp + 3);                        // 1cb5:04e3
            P[p].b[0x24] = m;
            u8 slot = (u8)squad_insert_player((u8)p, 0, 0, 0L, 0x18);
            P[p].b[0x21] = m;                                                   // owner
            S[m][slot].l[0x28] = ldiv(lmul(player_value_calc(slot, 1), 50L), 100L);   // slot, not p
            S[m][slot].b[0x0b] = random_range(2, 3);                            // contract years
        }
    }
    if (y == 0x94) draw(y, "Keine besonderen Ereignisse...");                   // DS:2770
    screen_fade(FAR:2e70, 1);
    if (DS:2252 == DS:224e && random_range(0, 4) == 0) {                        // both i32 words compared first
        dialog_okay("Ihre Spieler werden", "{lter und schw{cher...", " ", "Pech gehabt.");             // DS:4e08..
        dialog_okay("Mit freundlichem Gru~", "vom Bundesliga Manager I:", "War nur ein", "SCHERZ !!!"); // DS:4e18..
    }
    input_wait_click(FAR:6dc0);
    u8 saved_m = FAR:5358 = m;                                                  // [BP-0x44]
    screen_fade(FAR:2e70, 0); ui_redraw_buttons(0x7f, 0);
    office_redraw_with_header("Trennungen");                                    // DS:2774, shown for every m
    screen_fade(FAR:2e70, 1);
    // ---------------- "Trennungen": ageing and transfer back (only m == 0) ----------------
    if (m == 0) for (i16 q = 1; q < 0x97; q++) {                                // q = [BP-0x7a]
        s = find_player_by_id((u8)q, &g, &sl);
        P[q].b[0x1a]++;                                                         // every person ages
        if (s == NULL) continue;
        if (s->ptr[0x30] != NULL) s->ptr[0x30] = message_delete(s->ptr[0x30]);
        s->b[0x09] &= 0x3f;
        if (DS:05d4 != 0) { P[s->b[0x0f]].b[0x1a] = random_range(0x12, 0x19); s->b[0x18] = 0; }
        else s->b[0x0b]--;                                                      // contract years, u8 wrap
        if ((u16)P[q].b[0x21] == (u16)(i16)(i8)g) continue;                    // owner == holding squad
        u8 owner = FAR:304a = FAR:5358 = P[s->b[0x0f]].b[0x21];
        if ((i8)owner < 4) {                                                    // owner is a human manager
            ns = (u8)squad_insert_player((u8)q, 1, 0, 0L, 0x19);
            far_memcpy_bytes(&S[g][sl], &S[owner][ns], 0x34);                  // (src, dst, n)
            P[s->b[0x0f]].b[0x21] = owner;
        } else P[q].b[0x21] = 5;                                                // free
        FAR:304a = g;
        squad_remove_player(sl, 1, (u8)((2 - (sl == 4)) * 12));                // tests the SLOT, not the group
        FAR:5358 = FAR:304a = saved_m;                                          // = 0
        S[saved_m][ns].b[0x18] = 0;  S[saved_m][ns].b[0x0c] = 0;               // squad 0, ns may be stale
    }
    // ---------------- retirement and regeneration (every m) ----------------
    for (i16 q = 1; q < 0x97; q++) {
        if ((i16)(i8)m - (u16)DS:07ab + 1 == 0) { P[q].b[0x23] = 0; P[q].b[0x22] = 0; }   // last manager
        s = find_player_by_id((u8)q, &g, &sl);
        if (s != NULL && FAR:304a != g) continue;
        i16 lim = random_range(0x20, 0x22);
        u8 regen;
        if ((i16)P[q].b[0x1a] > lim && s == NULL) regen = 1;                    // free person older than 32..34
        else regen = (s->b[0x0b] == 0 && (s->b[0x18] & 0x80));                  // see Q10 for s == NULL
        if (!regen) continue;
        if (s != NULL) {                                                        // squad player retires
            s->ptr[0x30] = message_post(0, 3L, &P[s->b[0x0f]], (i32)P[s->b[0x0f]].b[0x1a], 0L, 0L, 0L);
            screen_messages(s->ptr[0x30]);
            s->ptr[0x30] = message_delete(s->ptr[0x30]);
            squad_remove_player(sl, 1, 0x18);
            P[q].b[0x21] = 5;
        }
        P[q].b[0x1a] = random_range(0x12, 0x19);                                // new person, age 18..25
        i16 x = random_range(0x1e, 0x5c);
        P[q].b[0x1c] = random_range(x - 5, x + 5);
        P[p].b[0x20] = random_range(0, 6);                                      // index p ([BP-0x7c]), NOT q: Q11
        P[q].b[0x1d] = random_range(x - 5, x + 5);
        FAR:304a = FAR:5358;                                                    // = m
    }
}
// ---------------- after all managers ----------------
for (FAR:304a = 0; FAR:304a < n; FAR:304a++)
    for (i16 i = 0; i < 24; i++) {                                              // slot 24 is not touched
        S[m][i].b[0x02] = 0; S[m][i].b[0x00] = 0; S[m][i].b[0x01] = 0;          // cards
        for (j = 0; j < 3; j++) { S[m][i].b[0x06 + j] = 0; S[m][i].b[0x03 + j] = 0; }   // games, goals per competition
    }
for (i = 0; i < 3; i++) for (j = 0; j < 3; j++) any_tier[j] += DS:05fe[3*i + j];   // i8 column sums
for (i = 0; i < 3; i++) for (j = 0; j < 3; j++) {
    if (DS:05fe[3*i + j] != 0 && tier_cnt[i] == 0) DS:05fe[3*i + j] = 0;
    if (DS:05fe[3*i + j] == 0 && tier_cnt[i] != 0 && any_tier[j] != 0) DS:05fe[3*i + j] = 1;
}
delay_ticks(70L); screen_fade(FAR:2e70, 0); ui_redraw_buttons(0x7f, 0);
// ---------------- "Vertragsverhandlungen" ----------------
if (DS:05d4 == 0) for (FAR:304a = 0; FAR:304a < n; FAR:304a++) {
    screen_fade(FAR:2e70, 0);
    FAR:5358 = m;
    office_redraw_with_header("Vertragsverhandlungen");                         // DS:2778
    u8 any = 0; i16 cnt = (u8)squad_count_by_flag(m, 0); i8 sl = 0;
    for (; sl < cnt; sl++) if (S[m][sl].b[0x0b] == 0) any = 1;
    if (any) {
        ui_draw_button(rp, 2, 7, 0xee, 0xb5, &tmp, 0, 8);
        squad_list_draw(6, 0x12, 0, 0x18, 0xec, 0xb0, 0x0003d146L, 3);          // args as pushed
    }
    ui_set_font(rp, 2); screen_fade(FAR:2e70, 1);
    if (!any) delay_ticks(70L);
    sl = 0;
    for (i16 k = 0; k < cnt; k++) {
        s = &S[m][sl];
        if (s->b[0x0b] != 0) { sl++; continue; }                                // contract still running
        if (contract_negotiation((u8)sl, 1L, 0L, 0L) != 0) {                    // player stays
            dialog_okay(P[s->b[0x0f]].name, "bleibt Ihnen auch", "die n{chste Saison", "erhalten.");  // DS:4e38..
            sl++;
        } else {                                                                // player leaves
            dialog_okay(P[s->b[0x0f]].name, "kehrt Ihrem Verein", "den R}cken...", "");   // DS:4e28, DS:0290 = ""
            i32 fee = lmul(ldiv(player_value_calc((u8)sl, 0) >> 1, 1000L), 1000L);   // SAR/RCR halving
            r->l[0x1f0] += fee;
            buf[0] = 0; if (DS:224d) strcat(buf, DS:4f78);
            strcat(buf, format_long_thousands(ldiv(lmul(DS:2252, fee), DS:224e)));
            strcat(buf, " DM");                                                 // DS:4f64
            dialog_okay("Sie erhalten eine", "Abl|sesumme von", buf, "");        // DS:4e30, DS:4e34, DS:0291 = ""
            squad_remove_player((u8)sl, 1, 0x18);                               // sl not advanced
        }
        ui_fill_rect(rp, 3, 8, 0xef, 0xb8, 8);
        squad_list_draw(6, 0x12, 0, 0x18, 0xec, 0xb0, 0x0003d146L, 3);
    }
}
screen_fade(FAR:2e70, 0); ui_redraw_buttons(0x7f, 0);
season_rebalance_player_clubs();                                                // 1cb5:2756
```

RNG calls in program order, per manager iteration: [youth: `random_range(0,2)`; if taken:
`(1,0x96)` repeated until a free person, `(0,6)`, `(0x11,0x13)`, `(0x2d,0x37)`, `(yp-3,yp+3)` twice,
`(2,3)`] → joke `random_range(0,4)` (DM version only) → [m == 0 and DS:05d4 != 0: `(0x12,0x19)` per
squad player in the Trennungen pass] → per person q = 1..150 not held by another squad:
`random_range(0x20,0x22)`, and on regeneration `(0x12,0x19)`, `(0x1e,0x5c)`, `(x-5,x+5)`, `(0,6)`,
`(x-5,x+5)`. After the loop: none here (contract_negotiation and season_rebalance_player_clubs have
their own).

---------------------------------------------------------------------------------------------------

## 5. Function list (area complete)

| Address | Ghidra name (renamed) | Kind | Callers |
|---|---|---|---|
| 2a58:000c | `game_end_screen` | UI | season_main_loop |
| 2a58:009d | `match_draw_score_digit_pair` | UI | 1310 match code (3) |
| 2a58:024e | `match_draw_manager_bar_stats` | UI | 2a58:042f, 2a58:0ca3 |
| 2a58:0350 | `draw_text_split_two_lines` | UI | 2a58:042f |
| 2a58:042f | `match_draw_live_screen` | UI (§3.10) | 1310 live view, 2a58:1f75 |
| 2a58:0ca3 | `match_ticker_events` | logic+UI (§3.9) | 1310:2ee5, 1310:356d, 1f9d:0c3b |
| 2a58:1f75 | `match_substitution_screen` | UI | 1310:3314, 2a58:0ca3 |
| 2a58:2051 | `match_forfeit_penalty` | logic | 2a58:20b2 |
| 2a58:20b2 | `match_gate_receipts_and_aftermath` | logic | 1310:15db |
| 2a58:2a06 | `special_date_events` | logic+UI | season_main_loop |
| 2a58:3176 | `season_main_loop` | logic | main |
| 2a58:4597 | `season_end_eternal_points` | logic | season_main_loop |
| 1cb5:0000 | `set_copy_protection_passed` | logic | load-game screen |
| 1cb5:0012 | `season_end_messages` | logic+UI (§4.8) | season_main_loop |
| 1cb5:13bd | `random_weekly_events` | logic | season_main_loop |
| 1cb5:25d5 | `squad_fix_position_mismatch` | logic | (1 caller) |
| 1cb5:2756 | `season_rebalance_player_clubs` | logic | season_end_messages |
| 1cb5:2987 | `pick_random_free_team_in_tier` | logic | 1cb5:2756, 24a4:19fb |
| 1cb5:2a3b | `rank_tier_teams_by_strength` | logic | 1cb5:2756 |
| 1cb5:2b88 | `player_suspension_countdown` | logic | 1cb5:13bd, season_main_loop |
| 1cb5:2bf9 | `matchday_info_panel` | UI (not decompilable) | match-day options dialog |

Library/UI callees used: `random_range`, `ui_*`, `blit_bitmap`, `vga_load_picture` (176b:0356),
`screen_fade`, `timer_*`, `delay_ticks`, `input_*`, `format_long_thousands`, `crt_f*` string
functions, `_aFl*` long helpers, `mem_alloc/mem_free`, `far_memcpy_bytes(src, dst, n)`.

---------------------------------------------------------------------------------------------------

## 6. Globals used

| Address | Size/type | Suggested name | Save block |
|---|---|---|---|
| DS:05ac | u8 | `copy_protection_passed` | (54 copies it) |
| DS:05d4 | u8 | `fast_mode` (skips delays/animations/contract talks; in 1cb5:0012 it also re-rolls ages) | — |
| DS:0607 | u8 | `goal_scenes_enabled` (TORE scenes in 2a58:0ca3) | — |
| DS:063a | i16 | `ticker_delay_reduction` (40 shipped; subtracted from 0x78/0x82 ticks) | — |
| DS:066c | i32[4][9] | `sponsor_income` (m*0x24; [0] at 066c, [1..6] at 0670; divided by 10 on promotion) | 41 |
| DS:05b4 | far bitmap* | `badge_cache_bitmap` (38x38 badges side by side) | — |
| DS:2388 | far char*[3] | league names "Bundesliga", "Zweite Liga", "Am.-Oberliga" | const |
| DS:05da | u8 | `demo_stop_in_december` | — |
| DS:05fe | u8[3][3] | `tier_competition_flags` | 36 |
| DS:0632/063c | i16[] | `match_panel_x/y` | — |
| DS:0664 | u8[4] | `mgr_players_on_pitch` (decremented on a red card; used as lineup mode by 2ecd:1560) | — |
| DS:079e/079f | u8[4][2] | `mgr_week_flags` (copied each week) | 5 |
| DS:07ab | u8 | `num_managers` | 7 |
| DS:07ac | u8 | `dfb_pokal_winner_plus1` | 8 |
| DS:07ad/07ae/07af | u8 | `euro_cup_winner_plus1[3]` | 9-11 |
| DS:07b2 | u8 | `message_window_open` | — |
| DS:07b8 | u8[12] | `days_per_month` | — (const) |
| DS:07dc | i32 | `day` | 23 |
| DS:07e0 | u16 | `season_year` | 24 |
| DS:07e2 | i16 | `seasons_played` | 48 |
| DS:0292 | i8[4][3] | `training_mix_table` | const |
| DS:224c | i8 | `season_day_offset` | const |
| DS:224d | i8 | `calendar_shift_weeks` | const |
| DS:224e/2252 | i32 | `money_div`, `money_mul` | const |
| DS:2256 | i32 | `fixed_gate_receipts` | const |
| DS:225a | u8[3] | `league_round[3]` | 34 |
| DS:225e/2262/2266/226a/226e/2272 | see §1.5 | league configuration | const |
| DS:2280 | u8[95] | `slot_match_flags` | 50 |
| DS:4a28 | u8 | `difficulty` (5 - user level) | 42 |
| DS:4c00 | far char* | "WINTERPAUSE" | const |
| DS:25b8 | far char* | "SAISONENDE" | const |
| DS:53d0/53d4 | i8[3] | `special_date_day/month` | const |
| DS:5390 | i16[4] | `neutral_ticket_price` | const |
| FAR:0000 | u8[4][2] | `sponsor_cancel_flag` | 37 |
| FAR:0008/0009 | u8 | `cup_round_index` (DFB / euro) | 30 |
| FAR:0028[] | u8 | `match_slot_fixture` (fixture index within the round) | 40 (002e region) |
| FAR:2e92 | u8 | `panel_human_count` (managers drawn in the current panel) | — |
| FAR:2f24 | far bitmap* | manager portraits 40x40 at x = 0x29*m+1 | — |
| FAR:3057 | u8 | current match minute (ticker) | — |
| FAR:4b5e | u8[..][20] | league fixtures (home,away pairs), index `round*20 + k` | — |
| FAR:4bf1 | u8 | `badge_cache_valid` | — |
| FAR:4c6c | u8[5][2] | `panel_teams` (home, away per live panel; 0xff = empty) | — |
| FAR:5140+6*m | u8[4][6] | `match_side_stats`: +0 yellow, +1 red, +2 injuries, +3 players on pitch, +4/+5 chance counters | — |
| FAR:5350 | u8[4] | `ticker_slots` (home team per panel row) | — |
| FAR:5354 | far bitmap* | manager portraits 38x38 at x = 0x29*m | — |
| FAR:549a | u8 | `tore_replay_count` | — |
| FAR:5710/5778 | u8[4] | per-manager chance counters (home/away side) | — |
| FAR:5793/5794 | u8[] | `panel_is_derby` (both teams human) | — |
| FAR:5798 | far bitmap* | match event sprites (pitch 0xbc x 0x66 at (0,0); cards/goal/injury 40x40 at y 0x66; ball 13x13; stretcher 10x10) | — |
| FAR:57c4 | far bitmap** | live panel background 0x9b x 0x72 | — |
| FAR:76ca | u8[..][32] | cup pairings (home,away), index `comp*32 + k` | — |
| FAR:a6ca | far bitmap* | stadium picture used for the ticker crops | — |
| FAR:016e | u8 | `slot` | 49 |
| FAR:0178/0179 | u8[4][2] | `mgr_sub_limits` | — |
| FAR:0ecc+0x36*t | struct[64] | `team_stats` (T) | 16 |
| FAR:1d14 | u8[4] | `manager_plays_now`: manager m's team plays now; set by 2a58:20b2, cleared by 27c2:1226 (which also writes FAR:1d13 + r) and by the season loop, read by 1310:2ee5 and the news code | — |
| FAR:1d1c | u8[4][6] | `mgr_sub_slots` (0x7f = empty) | — |
| FAR:1d34 | u8[3][10] | `message_timer` | 44 |
| FAR:1d52 | 20 x 58 bytes | high-score table | HIGH.XY |
| FAR:2242+0x30a*m | struct[4] | `manager` (M) | 13 |
| FAR:2e70 | far ptr | current palette/fade target | — |
| FAR:2e8e | i32 | `month` | 21 |
| FAR:2e99 | u8 | `squad_dirty` | — |
| FAR:2ea2 | i16 | `mouse_event` | — |
| FAR:2eba | i32 | `day_of_month` | 20 |
| FAR:2f28 | far ptr | main rastport | — |
| FAR:3052/305c | u8[] | `match_slot_manager`, `match_slot_neutral` | 39 (2ec2 region) |
| FAR:3060 | u8 | `relegation_playoff_disabled` | 60 |
| FAR:3066+0x22*c | struct | club (C) | 15 |
| FAR:304a | u8 | `current_manager` | 3 |
| FAR:4b9a | u8[4][6][2] | `sponsor_contract_flags` | 38 |
| FAR:4bce | u8 | `month_at_slot_start` | 52 |
| FAR:4bec | u8[4] | `top_scorer_shown` | — |
| FAR:4bf0 | u8 | `match_slot_count` | — |
| FAR:4cfe | 58 bytes | high-score candidate | — |
| FAR:513c | u16 | `last_game_year` | 43 |
| FAR:5358 | u8 | `saved_manager` | 4 |
| FAR:535a | u8[3][20] | `tier_table_order` (team ids by rank) | 32 |
| FAR:5664 | far ptr[3][10] | `pending_messages` | 45 |
| FAR:56f4 | u8 | `youth_rule_switch` (0: youth from 1967 on; nonzero: only after 1995) | — |
| FAR:57dd+0x25*p | struct[151] | person (P) | 17 |
| FAR:6ddc | u8[0x8e8] | per-season table, zeroed at season start | 6 |
| FAR:774a+0x34*(g*25+i) | struct | squad slot (S) | 18 |
| FAR:90ca+0x9a*m | u8[0x9a] | last-match report records (+0x96 = FAR:9160 attendance i32, read by 042f as FAR:90c6+0x9a*(m+1)) | 56 region (90c2) |
| FAR:a7a0 | i32 | `calendar_year` | 22 |

Manager-record fields used here (offsets relative to FAR:2242): `+0x1e` team id, `+0x39` titles,
`+0x3a..0x3d` cup wins, `+0x3e+4e` season history log (0x31 entries: rank, tier, cup stage, euro stage),
`+0x10a` ticket price / `+0x10a+rounds` final position, `+0x132..0x135` cup stage per competition
(0x1e = not entered), `+0x138` tier, `+0x13a` home games, `+0x13b` log overflow count, `+0x13d` stadium
blocked ('d'), `+0x13e` bit0 hooligans, `+0x140` season-end flags (1 promoted, 2 relegated, 4 licence
refused, 8 restart with 500.000 DM), `+0x141..0x144` training plan, `+0x145` training intensity,
`+0x146..` trainer quality, `+0x15e` standing capacity (i32), `+0x166` seating capacity (i32),
`+0x186` comfort grade (i32), `+0x18e` second comfort grade (i32), `+0x1a4..0x1c3` career statistics
(u16 x2 per field), `+0x1dc` board satisfaction (i16 0..100), `+0x1e2` youth budget (u16),
`+0x1e4` attendance sum (i32), `+0x1e8` record attendance (i32), `+0x1ec` lowest attendance (i32),
`+0x1f0` balance (i32), `+0x1f4`/`+0x1f8` opponents of the records.

---------------------------------------------------------------------------------------------------

## 7. Open questions

1. **DS:2280 value 9**: written at run time by another module (probably 1310 when a slot is cancelled);
   the season loop maps it to 0 only at the loop top, while the office loop re-reads the raw byte.
2. **Day-of-week break test** uses `(day - DS:224c) % 7` and `(day - 4) % 7`; with a nonzero DS:224c
   the two match days per week would drift. DS:224c is 0 in all data seen.
3. (resolved) The youth test of 1cb5:0012 is `(year > 1966 && FAR:56f4 == 0) || (year > 1995 &&
   FAR:56f4 != 0)`, checked in the asm at 1cb5:035b..039e.
4. (resolved) 2a58:20b2: the attendance byte is `(u8)ldiv(att, 1000)`; `k = (P.b[0x1c] > P.b[0x1d])`.
5. (resolved) 1cb5:0012 is now specified at instruction level (§4.8).
6. (resolved) 2a58:0ca3 and 2a58:042f are now specified (§3.9, §3.10). The RNG inside
   `match_pick_scorer`, `tore_scene_play`, `player_injure` and `squad_recalc_team_strength` belongs to
   the match/stats areas.
7. (resolved) The high-score insertion loop at the game end (2a58:41c9..42fa) goes on scanning
   with the swap buffer after a put (see the note after §3.1).
8. `rank_tier_teams_by_strength` results are computed in 1cb5:2756 but the buffer is never read
   there; confirm whether 24a4:1798 reads it through the stack frame (unlikely).
9. 1cb5:0012 Trennungen: `squad_remove_player(sl, 1, (2 - (sl == 4)) * 12)` compares the **slot**
   with 4, almost certainly meant to be the group (the transfer pool has 12 slots). The following
   writes `S[0][ns].b[0x18] = S[0][ns].b[0x0c] = 0` use manager 0 rather than the new owner and a stale
   `ns` when the owner is >= 4. Keep both for bit-exactness.
10. 1cb5:0012 retirement test: when `s == NULL` and the age test fails, the code still reads
    `s->b[0x0b]` and `s->b[0x18]` through the NULL far pointer, i.e. bytes 0000:000b and 0000:0018 of
    the real-mode interrupt vector table. A port should treat this as "not regenerated" (regen = 0)
    unless a DOS trace shows `[0:000b] == 0 && ([0:0018] & 0x80)`.
11. 1cb5:0012 regeneration writes the new position to `P[p].b[0x20]` where `p` is the stale `[BP-0x7c]`
    (last youth person index, or 6 after the sponsor loop, or uninitialised stack for the first
    manager when neither ran), not to the regenerated person `q`. Port: keep the stale-index write when
    `p` is defined; when it is undefined, an uninitialised value cannot be reproduced (suggest:
    initialise to 0 and write `P[0].b[0x20]`, which is the unused person 0).
12. 2a58:0ca3: `ybase = ((3 - k)/2) * 0x71` puts ticker rows 0 and 1 in the lower half and rows 2..4
    in the upper half; `k == 4` (row not found) also maps to the upper half.
