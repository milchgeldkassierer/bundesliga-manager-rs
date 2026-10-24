# Competitions: DFB-Pokal, European cups, relegation play-off, promotion/relegation, fixtures

Spec for a bit-exact port. Source: `/unpacked/BMMAIN_unpacked.EXE` (Ghidra), segment `27c2` (all 16
functions) plus the cross-area code that the "cups" area depends on (relegation play-off driver in `1310`,
promotion/relegation and championship in `season_main_loop`, league shuffle and fixture generation).

Conventions

* `DS:xxxx` = DGROUP `5cb3`, `FAR:xxxx` = far data segment `5238`. Every address below was resolved
  through the selector table (`MOV ES,[DS:97xx..a1xx]`), not guessed from the decompiler, which mixes
  the two segments up. Save-block numbers refer to [savegame.md](../savegame.md).
* Types: `u8` = unsigned byte, `i8` = signed byte, `u16`/`i16`, `i32` (MSC `long`). All functions are
  `__cdecl far`; byte arguments are pushed as words, the callee reads the low byte only.
* `random_range(lo, hi)` = `176b:0cc7` = `rand() % (hi-lo+1) + lo` (MSC LCG, FUNCTION_MAP §3.4).
  Every call is listed in order. Library/UI calls are marked **[UI]** or **[lib]**; they never use the
  RNG except `copy_protection_penalty` (see §0.3).
* "Slot" = index into a 32-entry competition draw array. Pairs are slots `(2k, 2k+1)`; slot `2k` is the
  home team (first leg home team for two-legged ties).

## 0. Data model

### 0.1 Competitions and team ids

| c | Competition | Display name (DS:24dc+4c) | Pool for first-round draw | Legs |
|---|---|---|---|---|
| 0 | DFB-Pokal | `"DfB-Pokal"` | team ids 0..DS:2277 (=57) | one match, home = slot 2k |
| 1 | European Champion Clubs' Cup | `"Pokal der Landesmeister"` | foreign ids 64..199 + German qualifiers | two legs |
| 2 | Cup Winners' Cup | `"Pokal der Pokalsieger"` | same | two legs |
| 3 | UEFA Cup | `"UEFA-Pokal"` | same | two legs |
| (1) | Relegation play-off (BL 16th vs 2.BL 3rd) | `"Relegationsspiel"` | reuses c=1 storage with `FAR:57dc=1` | two legs |

Team id layout (from `DS:226e/226f/2270` and `DS:225e..2260`):

| ids | league | tier (`league_tier_of_team`) |
|---|---|---|
| 0..17 | Bundesliga (DS:225e = 18 teams, DS:2262 = 34 match days) | 1 |
| 18..37 | Zweite Liga (DS:225f = 20, DS:2263 = 38) | 2 |
| 38..57 | Amateur-Oberliga (DS:2260 = 20, DS:2264 = 38) | 4 |
| 58..63 | German reserve teams that can be promoted into the amateur league (§9) | 4 |
| 64..199 | foreign clubs (European cups only) | 4 |

Team ids are *positional*: promotion/relegation and the yearly shuffle swap the club records of two ids
(`team_swap_slots` 1310:0b24) instead of moving ids between leagues.

### 0.2 Round counter and the round table DS:07d6

`FAR:0008[c]` (save block 30, `u8[8]`, only 0..3 used) is the round index of competition c.
`DS:07d6[]` = `{0x20, 0x10, 0x08, 0x04, 0x02, 0x01}` = teams that are left *after* the round with that
index, which equals the number of pairs being played *in* round `idx` for `idx >= 1`:

| FAR:0008[c] | meaning | pairs played = DS:07d6[idx] | slots used (`n2`) | round name DS:24c4+4*idx |
|---|---|---|---|---|
| 0 | first-round draw in progress (09e0 only) | (32 teams drawn) | | `""` |
| 1 | 1st round | 16 | 32 | `"1.Runde "` |
| 2 | round of 16 | 8 | 16 | `"Achtelfinale "` |
| 3 | quarter-final | 4 | 8 | `"Viertelfinale "` |
| 4 | semi-final | 2 | 4 | `"Halbfinale "` |
| 5 | final | 1 | 2 | `"Endspiel "` |

The three European cups always advance in lockstep; code that needs "the" European round reads
`FAR:0009` (c = 1) for all of them.

### 0.3 Global arrays (all `u8`)

| Address | Shape | Save block | Meaning |
|---|---|---|---|
| FAR:76ca | `[4][32]` | 28 | `cup_slots[c][s]`: team id in slot s of competition c. `FAR:76c9` = `cup_slots[0][-1]` is read (never acted upon) by `cup_amateur_home_swap` for s = 0 |
| FAR:4c7e | `[4][32]` | 33 | `cup_goals[c][s]`: goals of the team in slot s in the match just played. +10 = extra time played ("n.V."), +20 = penalty shoot-out ("n.E.") |
| FAR:21c2 | `[4][32]` | 29 stores rows 1..3 (`FAR:21e2..2241`) | `cup_first_leg[c][s]`: first-leg goals, already re-oriented to the return-leg slot order (row 0 = FAR:21c2..21e1 is never written by the cup code) |
| FAR:1d17 | `[4]`, used for c = 1..3 → `FAR:1d18..1d1a` | 31 (`FAR:1d18[3]`) | `euro_second_leg_pending[c]`: 1 after the first leg was played |
| FAR:57aa | `[3][6]`, 0x80 = empty | 26 | `euro_qualifiers[c-1][q]` German teams qualified for cup c. Read as `FAR:57a4 + c*6 + q` |
| FAR:56fe | `[10]` | 25 | `euro_qualified_list[i]`, length `FAR:2e74` |
| FAR:2e74 | `u8` | **not saved** | length of FAR:56fe |
| FAR:535a | `[3][20]` | 32 | `final_table[L][rank]` = team id at final league position `rank` (0 = champion) of league L |
| DS:07ac | `u8` | 8 | DFB-Pokal winner + 1 (0 = none). Initial value 7 |
| DS:07ad | `u8` | 9 | German winner of the Champions' Cup + 1, 0 = none / foreign winner |
| DS:07ae | `u8` | 10 | German winner of the Cup Winners' Cup + 1 |
| DS:07af | `u8` | 11 | German winner of the UEFA Cup + 1 |
| DS:07b0 | `u8` | 12 | DFB-Pokal runner-up + 1 |
| FAR:2374 + m*0x30a + c | `u8` | 13 (manager record +0x132+c, i.e. +0x114+c from FAR:2260) | `manager_cup_stage[m][c]`: 0x1e = not taking part, 1 = in round 1, k+1 = won round k |
| FAR:2260 + m*0x30a | `u8` | 13 (record +0x1e) | manager m's team id |
| FAR:2e6e | `[2]` | 27 | relegation play-off first-leg goals (re-oriented) |
| FAR:3060 | `u8` | 60 | relegation play-off result: 0 = Bundesliga team won, 1 = 2.Liga team won (0x1e undecided) |
| FAR:016e | `u8` | 49 | current week index into DS:2280 (MEMORY_MAP lists it as DS, it is FAR) |
| DS:2280 | `[95]` | 50 | per-week competition mask, see §0.4 |

Non-saved working globals of this area: `FAR:57c8` (u16, "show the draw" flag), `FAR:57dc` (u8,
relegation mode), `FAR:1d14..1d17` (`manager_plays_now`, per-manager "own team plays now" flags; 27c2:1226 clears
them and sets the byte again at index `manager+1` via `FAR:1d13[r]` for a human team whose DFB-Pokal tie
is level and goes on), `FAR:2e99` (u8, "a human team is in extra time"), `FAR:570e` (u16, lamp
animation phase), bitmaps `FAR:3058/305a`, `FAR:3062/3064`, `FAR:2ebe/2ec0`, `FAR:6db4/6db6` and their
pixel pointers `FAR:a6d0`, `FAR:a6d4`, `FAR:0174`, `FAR:5158`.

### 0.4 Calendar mask DS:2280 (initial data, then save block 50)

Bit 0..2: league match day for league 0..2; bit 3: DFB-Pokal round; bits 4..6: European cups 1..3
(also the two relegation legs use bit 4 only). Initial bytes:

```
2280: 07 70 07 00 07 70 07 06 08 00 07 00 07 06 07 70
2290: 07 07 07 70 07 00 08 01 07 06 07 00 07 06 08 70
22a0: 07 00 07 70 07 00 00 00 00 00 00 00 00 00 00 00
22b0: 00 00 00 00 00 00 00 00 00 00 00 07 07 00 07 70
22c0: 08 07 07 70 07 06 07 00 07 00 07 00 07 70 07 00
22d0: 07 70 07 00 07 00 08 00 07 00 07 10 10 00 00
```

DFB rounds (0x08) are weeks 0x08, 0x16, 0x1e, 0x40, 0x56; European legs (0x70) are weeks 0x01, 0x05,
0x0f, 0x13, 0x1f, 0x23, 0x3f, 0x43, 0x4d, 0x51; relegation legs (0x10) are weeks 0x5b, 0x5c. The match-day
driver `matchday_play` 1310:15db receives `mask = DS:2280[w] >> 3` (1 = DFB, 0x0e = Europe, 2 =
relegation) and calls the functions of §4.

### 0.5 Other constants

| Address | Value | Use |
|---|---|---|
| DS:226f, DS:2270 | 0x12, 0x26 | tier limits in `league_tier_of_team` |
| DS:2277 | 0x39 (57) | highest DFB-Pokal team id |
| DS:226d | 4 | UEFA-Cup places from the league table |
| DS:2266[3] | 3, 4, 4 | relegation places per league (BL: 2 direct + play-off) |
| DS:226a[3] | 1, 0, 0 | league has a relegation play-off |
| DS:0654[1..6] | 09 0a 0c 12 22 24 | tier-pair categories, `tierA + 8*tierB` |
| DS:065c[1..6] | 09 11 21 12 14 24 | same, swapped orientation |
| DS:5348/534c | 0x25, 0xa7 (words, 4-byte stride) | draw board x for even / odd slot |
| DS:5356[3] | 0x00, 0x07, 0x1a | label colour for tier index 0/1/2 (colour 0 is drawn as 0xff by the text routine) |
| DS:537c[2] | 0x40, 0x40 | last week (DFB / Europe) for which the next-match date is shown |
| DS:4c24 | `"DFB-Pokal Auslosung"` | pushed to `copy_protection_penalty`, never drawn |

Strings (far pointers in DGROUP; `]` = Ü, `|` = Ö in the game font): DS:24c4 round names (§0.2),
DS:24d8 `"Endspiel "`, DS:24dc..24e8 cup names (§0.1), DS:2388/238c/2390 `"Bundesliga"`, `"Zweite Liga"`,
`"Am.-Oberliga"`, DS:25c4 `"Relegation"`, DS:25d4 `"Minute"`, DS:26a0 `"TASTE DR]CKEN"`, DS:26a4
`" GEGEN "`, DS:26a8 `"Pokalspiele"`, DS:26ac `"Relegationsspiel"`, DS:26b0 `"GEGEN"`, DS:26b4 `"n.E."`,
DS:26b8 `"n.V."`, DS:26bc `"Deutsche Meisterschaft"`, DS:26c0 `"gewinnt die"`, DS:26c4 `"gewinnt den"`,
DS:4da0 `"M|CHTEN SIE DEM UN]BERTROFFENEM"`, DS:4da4 `"SCHAUSPIEL EINER DFB-POKAL-"`, DS:4de4
`"SCHAUSPIEL EINER EUROPA-POKAL-"`, DS:4dd8 `"AUSLOSUNG BEIWOHNEN ?"`, DS:4ddc `"ABER KLAR"`, DS:4de0
`"KEIN GEDANKE"`, DS:299c/29a0 `"."`, literal DGROUP strings DS:5350 `" ("`, DS:5353 `")"`, DS:5359 `":"`,
DS:535b `"    "`, DS:5360 `"    "`, DS:5365 `"("`, DS:5367 `":"`, DS:5369 `")"`, DS:536b `"("`, DS:536d
`":"`, DS:536f `")"`, DS:5371 `"@@@@@@@@@"`.

## 1. Function list (segment 27c2, 16 functions, all renamed in Ghidra)

| Address | Ghidra name | Kind | Callers |
|---|---|---|---|
| 27c2:0006 | `cup_draw_screen_open` | UI + flag | 09e0, 13a2 |
| 27c2:0378 | `cup_draw_animate_slot` | UI | 09e0, 13a2 |
| 27c2:089a | `cup_draw_screen_close` | UI | 0006, 09e0, 13a2 |
| 27c2:09e0 | `cup_draw_first_round` | logic (RNG) | main 18bc:0740/0888/089b, season_main_loop 2a58:4553/456e |
| 27c2:0ef2 | `euro_cup_qualifiers_build` | logic | main 18bc:072a, season_main_loop 2a58:43ab |
| 27c2:1187 | `cup_amateur_home_swap` | logic | 09e0, 13a2 (x2) |
| 27c2:11e7 | `team_is_euro_qualified` | logic | 0ef2, season_end_messages 1cb5:02f1 |
| 27c2:1226 | `cup_check_ties_extra_time` | logic | matchday_play 1310:19e5, 1310:1a42 |
| 27c2:13a2 | `cup_draw_next_round` | logic (RNG) | 16dc |
| 27c2:15e8 | `cup_tie_winner` | logic | 1226, 16dc, matchday_relegation_match 1310:03eb |
| 27c2:16dc | `cup_round_complete` | logic | matchday_play 1310:1b9e |
| 27c2:1b6d | `cup_clear_match_goals` | logic | main 18bc:072f, season_main_loop 2a58:4539, 16dc |
| 27c2:1bb5 | `league_tier_of_team` | logic | 0378, 1187, 1ccb, start menu 18bc:3b0c, 1f9d:08a1, 3a41:02ca, 1000:28ee |
| 27c2:1bf0 | `cup_tier_pairing_label` | UI text | 1ccb |
| 27c2:1ccb | `screen_cup_matches` | screen | 089a, matchday_play 1310:1a6e/1ac9, match_live_period 1310:2c5b/2c86, office_menu_dispatcher 18bc:1b62 |
| 27c2:274d | `screen_title_winner` | screen | 16dc (x2), season_main_loop 2a58:3945 |

Library callees used below: `office_show_cached_background` 18bc:3b92 [UI], `ui_set_font(rp,n)` 176b:0c91
[UI], `ui_draw_text_centered_shadow(rp, str, x0, x1, y, colour, shadow)` 16c7:087c [UI],
`ui_draw_text_at(rp, str, x, y, colour)` 16c7:07d5 [UI], `ui_draw_line(rp, x0, y0, x1, y1, colour)`
16c7:09b0 [UI], `ui_draw_button(rp, x, y, w, h, rect_out, 0, style)` 16c7:00a7 [UI],
`ui_hit_test_buttons(rects, count, on_click)` 16c7:0002 [UI], `ui_redraw_buttons(owner, 0)` 16c7:0563,
`ui_wait_with_highlight(x, y)` 4091:1722, `dialog_two_choice(who, l1, l2, l3, yes, no)` 4091:146c (returns 1
for the first button, 0 for the second), `screen_fade(FAR:2e70, in)` 176b:1416, `pic_load_image(id, w, h,
unused, load)` 176b:0356 (id 0 + load 0 = blank bitmap), `bitmap_free` 176b:0dca, `blit_bitmap(src, sx,
sy, dst, dx, dy, w, h, minterm)` 4930:0991, `rastport_clear(rp, colour)` 4930:0545,
`rastport_set_draw_mode` 4930:052d, `vga_set_draw_page` 45ac:32fe, `vga_wait_vretrace` 45ac:34ef,
`fill_rect_planar` 45ac:34fc, `delay_ticks(long)` 4930:01e7, `mouse_pointer_hide(0)` 45ac:1a81,
`mouse_pointer_show` 45ac:1ac7, `input_poll_mouse(FAR:6dc0)` 176b:0319 (sets FAR:2ea2 = 1 on left
click), `input_wait_release` 176b:0e27, `input_wait_click(FAR:6dc0)` 176b:0d0a, `format_long_thousands(long)`
176b:0681 (returns a far string), `text_to_upper_de(dst, src)` 4091:105d, `clamp_int(&v, lo, hi)`
4091:1009, `crt_fstrcpy/fstrcat` [lib]. `manager_index_of_team(a, b)` 1310:32b1 returns `1+m` if manager m
owns team a, `0x81+m` if he owns team b, else 0. `match_penalty_shootout(c, slot, mask, mgr)` 1310:356d.

`copy_protection_penalty()` 16c7:09f9 takes no arguments (callers push some that are ignored). If
`DS:05ac == 0` (protection not passed) it calls `random_range(0,100)` once and hangs forever on 0. A port
with protection passed (`DS:05ac = 1`) makes no RNG call there. It is called from 0006 and 1ccb.

## 2. Small helpers

### 2.1 `league_tier_of_team` 27c2:1bb5

```c
/// Origin: BMMAIN.EXE 27c2:1bb5 (league_tier_of_team)
i16 league_tier_of_team(u8 team) {
    if ((i16)team < (i16)(i8)DS[0x226f]) return 1;   // 18: Bundesliga
    if ((i16)team < (i16)(i8)DS[0x2270]) return 2;   // 38: Zweite Liga
    return 4;                                        // amateur, reserve, foreign
}
```

### 2.2 `team_is_euro_qualified` 27c2:11e7

```c
/// Origin: BMMAIN.EXE 27c2:11e7 (team_is_euro_qualified)
i16 team_is_euro_qualified(u8 team) {
    for (u8 i = 0; i < FAR[0x2e74]; i++)
        if (FAR[0x56fe + i] == team) return 1;
    return 0;
}
```

### 2.3 `cup_clear_match_goals` 27c2:1b6d

```c
/// Origin: BMMAIN.EXE 27c2:1b6d (cup_clear_match_goals)
void cup_clear_match_goals(void) {
    for (u8 s = 0; s < 32; s++) for (u8 c = 0; c < 4; c++) FAR[0x4c7e + c*32 + s] = 0;
}
```

### 2.4 `cup_amateur_home_swap` 27c2:1187

DFB-Pokal rule: an amateur team drawn away against a professional team gets home advantage.

```c
/// Origin: BMMAIN.EXE 27c2:1187 (cup_amateur_home_swap)
void cup_amateur_home_swap(u8 team_s, u8 team_prev, u8 s) {
    if (league_tier_of_team(team_s) != 4) return;
    if (league_tier_of_team(team_prev) >= 4) return;          // signed compare
    if (FAR[0x0008] >= 4) return;                             // DFB round counter, unsigned
    if ((s & 1) == 0) return;
    swap(FAR[0x76c9 + s], FAR[0x76ca + s]);                   // slots s-1 and s of competition 0
}
```

Callers always pass `(cup_slots[0][s], cup_slots[0][s-1], s)` for `s = 0 .. n-1`.

### 2.5 `cup_tie_winner` 27c2:15e8

Decides a two-legged tie (European cups, relegation) after the return leg. `s` is the even slot.

```c
/// Origin: BMMAIN.EXE 27c2:15e8 (cup_tie_winner)
i16 cup_tie_winner(i8 c, i8 s) {
    i16 idx = c*32 + s;                          // signed byte arguments, CBW
    u8 e = FAR[0x4c7e + idx];                    // return leg, slot s goals (home)
    while (e > 9) e -= 10;                       // strip n.V./n.E. markers
    u8 a = FAR[0x21c2 + idx];                    // first leg, team now in slot s (was away)
    u8 b = FAR[0x4c7f + idx];                    // return leg, slot s+1 goals (away)
    u8 d = FAR[0x21c3 + idx];                    // first leg, team now in slot s+1 (was home)
    i32 day = *(i32*)&DS[0x07dc];
    if (day > 315 && (u16)(b + d - a) == (u16)e)  // aggregate level after the season's day 315:
        return 0x1e;                              //   no away-goals rule (relegation)
    if ((i16)(d + b) > (i16)(a + e)) return 1;    // slot s+1 wins on aggregate
    if ((u16)(b + d - e) == a && a < b) return 1; // level, slot s+1 has more away goals
    if ((u16)(b + d - e) == a && a == b) return 0x1e; // fully level: extra time / penalties
    return 0;                                     // slot s wins
}
```

All sums are 16-bit of zero-extended bytes; the original compares are exactly these (`JG` signed for
the aggregate, equality otherwise).

## 3. First-round draw

### 3.1 `cup_draw_first_round(i8 c, u8 redisplay)` 27c2:09e0

Called with `(c, 0)` for the real draw and `(c, 1)` to show an existing European draw.

```c
/// Origin: BMMAIN.EXE 27c2:09e0 (cup_draw_first_round)
void cup_draw_first_round(i8 c, u8 redisplay) {
    u8 click = 0, human_slot[4];
    *(u16*)&FAR[0x57c8] = 0;
    FAR[0x0008 + c] = 0;
    if (c == 0 || redisplay != 0) cup_draw_screen_open(c);           // may set FAR:57c8 = 1
    if (c == 0 || redisplay == 0) {
        u8 euro = (c != 0);
        u8 lo   = euro << 6;                                         // 0 or 64
        u8 hi   = (u8)((0xc7 - DS[0x2277]) * euro + DS[0x2277]);     // 57 or 199
        u8 j0   = euro;                                              // first competition to compare
        u8 j1   = 4 - 3*(c == 0);                                    // 1 or 4 (exclusive)
        input_wait_release();                                        // [UI]
        if (c == 1)                                                  // clear first-leg goals of all
            for (s = 0; s < 32; s++) for (k = 0; k < 3; k++) FAR[0x21e2 + k*32 + s] = 0;
        for (m = 0; m < DS[0x07ab]; m++) FAR[0x2374 + m*0x30a + c] = 0x1e;
        if (c == 0) {                                                // humans get random distinct slots
            for (m = 0; m < DS[0x07ab]; m++) {
                human_slot[m] = 0;
                do {
                    human_slot[m] = random_range(0, 31);
                    dup = 0;
                    for (k = 0; k < m; k++) if (human_slot[k] == human_slot[m]) dup = 1;
                } while (dup);
                FAR[0x2374 + m*0x30a] = 1;
                FAR[0x76ca + human_slot[m]] = FAR[0x2260 + m*0x30a];
            }
        }
        mouse_pointer_hide(0);
        for (u8 i = 0; i < 32; i++) {
            input_poll_mouse(FAR:6dc0); if (*(i16*)&FAR[0x2ea2] == 1) click = 1;
            for (;;) {                                               // retry loop (no new mouse poll)
                dup = 0;
                u8 k; for (k = 0; k < DS[0x07ab]; k++)
                    if (c == 0 && human_slot[k] == i) k = 0x14;      // loop then ends with k = 0x15
                if (k != 0x15) {
                    u8 t = random_range(lo, hi);
                    FAR[0x76ca + c*32 + i] = t;
                    for (u8 j = j0; j < j1; j++) {
                        u8 lim = (c == j) ? i : 32;                  // other cups: all 32 slots,
                        for (k = 0; k < lim; k++)                    //   including last season's
                            if (FAR[0x76ca + j*32 + k] == t) dup = 1;
                    }
                    for (k = 0; k < DS[0x07ab]; k++)
                        if (t == FAR[0x2260 + k*0x30a]) dup = 1;
                }
                if (!dup) break;
            }
            if (c == 0 && *(u16*)&FAR[0x57c8] != 0)
                cup_draw_animate_slot(i, FAR[0x76ca + c*32 + i], click, c);
        }
        mouse_pointer_show();
        if (c == 0) {
            for (u8 s = 0; s < 32; s++) cup_amateur_home_swap(FAR[0x76ca + s], FAR[0x76c9 + s], s);
        } else {                                                     // place German qualifiers
            FAR[0x1d17 + c] = 0;
            for (u8 q = 0; FAR[0x57a4 + c*6 + q] != 0x80; q++) {
                u8 s;
                do s = random_range(1, 31); while (FAR[0x76ca + c*32 + s] < 0x40);
                FAR[0x76ca + c*32 + s] = FAR[0x57a4 + c*6 + q];
                for (m = 0; m < DS[0x07ab]; m++)
                    if (FAR[0x76ca + c*32 + s] == FAR[0x2260 + m*0x30a]) FAR[0x2374 + m*0x30a + c] = 1;
            }
        }
    }
    if (c != 0 && *(u16*)&FAR[0x57c8] != 0) {                       // show the European draw
        mouse_pointer_hide(0);
        for (u8 i = 0; i < 32; i++) {
            input_poll_mouse(FAR:6dc0); if (*(i16*)&FAR[0x2ea2] == 1) click = 1;
            cup_draw_animate_slot(i, FAR[0x76ca + c*32 + i], click, c);
        }
        mouse_pointer_show();
    }
    FAR[0x0008 + c] = 1;
    if (*(u16*)&FAR[0x57c8] != 0) cup_draw_screen_close(c);
}
```

Notes for bit-exactness:
* In the human-slot loop `human_slot[m]` is zeroed once and then re-rolled; the duplicate test compares
  only with earlier managers.
* The European duplicate test reads all 32 slots of the *other* European cups, which still hold the
  previous season's (or a just-made) draw; `FAR:76ca` must persist exactly (save block 28). The cups are
  drawn in order 1, 2, 3.
* RNG order per call: c = 0: `random_range(0,31)` per human (with re-rolls), then per non-human slot
  `random_range(0,57)` until unique. c > 0: per slot `random_range(64,199)` until unique, then per
  qualifier `random_range(1,31)` until the slot holds a foreign team (`>= 0x40`). Slot 0 never receives a
  German qualifier.
* A German qualifier can be drawn into a slot already holding another German qualifier only if the foreign
  check passes, i.e. never; two qualifiers can face each other.
* A human manager's slot skips only the retry loop: `cup_draw_animate_slot` still runs for it, so a shown
  DFB-Pokal draw animates all 32 slots in slot order (checked against DOSBox, scenario `cup_draw`).

### 3.2 `euro_cup_qualifiers_build` 27c2:0ef2

Runs at the end of each season (and in `main` for a new game) *after* the final tables in `FAR:535a`
and the title holders `DS:07ac..07b0` are known, and after `league_shuffle_teams` (§9.1).

```c
/// Origin: BMMAIN.EXE 27c2:0ef2 (euro_cup_qualifiers_build)
void euro_cup_qualifiers_build(void) {
    u8 uefa = DS[0x226d];          // 4
    u8 src = 1, n0 = 0, n1 = 0, n2 = 0;
    FAR[0x2e74] = 0;
    for (i = 0; i < 3; i++) for (j = 0; j < 6; j++) FAR[0x57aa + i*6 + j] = 0x80;
    #define ADD(t) (FAR[0x56fe + FAR[0x2e74]] = (t), FAR[0x2e74]++)
    // Champions' Cup (row 0): champion, and title holder or runner-up
    FAR[0x57aa + n0++] = FAR[0x535a]; ADD(FAR[0x535a]);
    if (DS[0x07ad] != 0) {
        u8 t = team_is_euro_qualified(DS[0x07ad] - 1) ? FAR[0x535b] : DS[0x07ad] - 1;
        FAR[0x57aa + n0] = t; ADD(t);                       // n0 not incremented
    }
    // Cup Winners' Cup (row 1): DFB cup winner, else runner-up (DS:07ac is not tested for 0)
    if (!team_is_euro_qualified(DS[0x07ac] - 1))      { t = DS[0x07ac] - 1; FAR[0x57b0 + n1++] = t; ADD(t); }
    else if (!team_is_euro_qualified(DS[0x07b0] - 1)) { t = DS[0x07b0] - 1; FAR[0x57b0 + n1++] = t; ADD(t); }
    if (DS[0x07ae] != 0) {                                  // holder of the Cup Winners' Cup
        if (!team_is_euro_qualified(DS[0x07ae] - 1))      { t = DS[0x07ae] - 1; FAR[0x57b0 + n1++] = t; ADD(t); }
        else if (!team_is_euro_qualified(DS[0x07b0] - 1)) { t = DS[0x07b0] - 1; FAR[0x57b0 + n1] = t;   ADD(t); }
    }
    // UEFA Cup (row 2): holder, then the next DS:226d unqualified teams of the Bundesliga table from rank 1
    if (DS[0x07af] != 0) {
        if (!team_is_euro_qualified(DS[0x07af] - 1)) { t = DS[0x07af] - 1; FAR[0x57b6 + n2++] = t; ADD(t); }
        else uefa++;
    }
    while (uefa != 0) {
        t = FAR[0x535a + src];
        if (!team_is_euro_qualified(t)) { FAR[0x57b6 + n2++] = t; ADD(t); uefa--; }
        src++;
    }
}
```

All subtractions are `u8` (a 0 holder gives 0xff). Note the second Cup-Winners'-Cup branch does not
advance `n1`. `src` runs past rank 17 into the 2.Liga part of `FAR:535a` if needed.

## 4. Match-day processing

`matchday_play(…, mask, …)` 1310:15db (match area) does, for a cup day: play all matches, then

```c
n2 = DS:07d6[mask > 1 ? FAR:0009 : FAR:0008] * 2;
FAR:2e99 = 0;                                                    // 1310:19ce
// 1310:19d4..1af0. The result screens are reached ONLY through the extra-time path:
// mask != 0, the first call returned nonzero AND the second (shoot-out) call returned nonzero.
// Every other path jumps straight to 1310:1af2 (via the JMP at 1310:1a4e) and shows no cup result screen.
if (mask != 0                                                    // JZ 1a4e
    && cup_check_ties_extra_time(mask, n2, 0) != 0) {            // 1310:19e5; marks ties +10 ("n.V."), sets flags
    match_live_period(arg6, mask, argC, 0x5b, 0x69, (i8)FAR:2e99);   // 1310:5403, extra time 1st half
    match_live_period(arg6, mask, argC, 0x6a, 0x78, (i8)FAR:2e99);   // extra time 2nd half
    if (cup_check_ties_extra_time(mask, n2, 1) != 0) {           // 1310:1a42; still level: +10 ("n.E.") + shoot-out
        // 1310:1a51: show results (UI only, no RNG)
        if (DS:0608 != 0 && mask == 1) {
            office_show_cached_background();                     // 18bc:3b92 [UI]
            screen_cup_matches(0, 0x78);                         // 27c2:1ccb
            screen_fade(FAR:2e70 /*far ptr, hi word FAR:2e72*/, 0);   // 176b:1416 [UI]
        } else {
            for (u8 c = 1; c < 4; c++)                           // byte at [BP-8], JC on c < 4
                if ((FAR:57dc != 0 || DS:0609 != 0) && (mask & (1 << c))) {   // 16-bit TEST (1<<c) vs (u16)mask
                    office_show_cached_background();             // 18bc:3b92 [UI]
                    screen_cup_matches(c, 0x78);
                    screen_fade(FAR:2e70, 0);                    // 176b:1416 [UI]
                }
        }
    }
}
// 1310:1af2: continues with the match area's own post-processing (FAR:05b4/05b6, FAR:57c4/57c6 ...)
...
if (mask) cup_round_complete(mask - 1);
```

Note on the gating: if the first `cup_check_ties_extra_time` call finds no level tie (return 0), the
match-day ends without any cup result screen, even on a cup day. The screens are also skipped when the
second call returns 0 (no tie still level after extra time). Ports must not show them unconditionally.
(The far-call operands decode as `0x1000:c752` = 18bc:3b92 and `0x1000:8ac6` = 176b:1416 when the
segment bytes of the CALLF instruction are read.)

### 4.1 `cup_check_ties_extra_time(u8 mask, u8 n, u8 shootout)` 27c2:1226 → `u16`

```c
/// Origin: BMMAIN.EXE 27c2:1226 (cup_check_ties_extra_time)
u16 cup_check_ties_extra_time(u8 mask, u8 n, u8 shootout) {
    u8 any = 0;
    for (m = 0; m < DS[0x07ab]; m++) FAR[0x1d14 + m] = 0;
    for (u8 c = 0; c < 4; c++) {
        if (!((1 << c) & mask)) continue;
        for (u8 s = 0; s < n; s += 2) {
            u8 r;
            if (mask == 1) {                                   // DFB-Pokal, single match
                u8 g0 = FAR[0x4c7e + c*32 + s], g1 = FAR[0x4c7f + c*32 + s];
                if (!(g1 == g0 || (u16)(g0 - g1) == 10)) continue;
                any = 1;
                FAR[0x4c7e + s] += 10;                         // index without c*32 (c is 0 here)
                r = manager_index_of_team(FAR[0x76ca + c*32 + s], FAR[0x76cb + c*32 + s]);
                if (r != 0) { FAR[0x1d13 + r] = 1; FAR[0x2e99] = 1; }
            } else {
                if (FAR[0x1d17 + c] == 0) continue;            // only after the return leg
                if (cup_tie_winner(c, s) != 0x1e) continue;
                any = 1;
                FAR[0x4c7e + c*32 + s] += 10;
                r = manager_index_of_team(FAR[0x76ca + c*32 + s], FAR[0x76cb + c*32 + s]);
                if (r != 0) FAR[0x2e99] = 1;                   // no FAR:1d13 write here
            }
            if (shootout) match_penalty_shootout(c, s, mask, r & 0x7f);
        }
    }
    return any;
}
```

`FAR:1d13 + r` uses the raw return value: for an away team owned by a human (`r = 0x81+m`) the byte at
`FAR:1d94+m` (inside the high-score buffer `FAR:1d52`) is set to 1. Keep this side effect.

### 4.2 `cup_round_complete(u8 euro)` 27c2:16dc

```c
/// Origin: BMMAIN.EXE 27c2:16dc (cup_round_complete)
void cup_round_complete(u8 euro) {
    u8 n2 = DS[0x07d6 + (euro ? FAR[0x0009] : FAR[0x0008])] * 2;   // u8 shift
    u8 w, l, t0, t1;
    if (euro) {
        for (u8 c = 1; (i16)c < ((FAR[0x57dc] != 0) ? 2 : 4); c++) {
            if (FAR[0x1d17 + c] == 0) {                       // first leg was played
                FAR[0x1d17 + c] = 1;
                for (s = 0; s < n2; s += 2) {
                    FAR[0x21c2 + c*32 + s] = FAR[0x4c7f + c*32 + s];
                    FAR[0x21c3 + c*32 + s] = FAR[0x4c7e + c*32 + s];
                    swap(FAR[0x76ca + c*32 + s], FAR[0x76cb + c*32 + s]);  // return leg venue
                }
            } else {                                          // return leg was played
                FAR[0x1d17 + c] = 0;
                for (s = 0; s < n2; s += 2) {
                    t0 = FAR[0x76ca + c*32 + s]; t1 = FAR[0x76cb + c*32 + s];
                    if (cup_tie_winner(c, s) != 0)           // 1 or 0x1e: slot s+1 goes through
                        swap(FAR[0x76ca + c*32 + s], FAR[0x76cb + c*32 + s]);
                    for (m = 0; m < DS[0x07ab]; m++)
                        if (FAR[0x76ca + c*32 + s] == FAR[0x2260 + m*0x30a] && FAR[0x57dc] == 0)
                            FAR[0x2374 + m*0x30a + c] = FAR[0x0008 + c] + 1;
                }
                if (n2 == 2 && FAR[0x57dc] == 0) {            // European final decided
                    u8 *holder = &DS[0x07ad + (c - 1)];       // c=1:07ad, 2:07ae, 3:07af
                    *holder = 0;
                    if (FAR[0x76ca + c*32] < 0x40) *holder = FAR[0x76ca + c*32] + 1;
                    u8 r = manager_index_of_team(FAR[0x76ca + c*32], 0xff);
                    if (r) screen_title_winner(r - 1, c + 1);
                    FAR[0x76ca + c*32] = t0; FAR[0x76cb + c*32] = t1;  // restore pre-swap order
                }
            }
        }
    } else {                                                  // DFB-Pokal
        for (s = 0; s < n2; s += 2) {
            t0 = w = FAR[0x76ca + s]; t1 = l = FAR[0x76cb + s];
            while (FAR[0x4c7e + s] > 9) FAR[0x4c7e + s] -= 10;   // strips markers permanently
            if (FAR[0x4c7f + s] > FAR[0x4c7e + s]) {          // away wins only if strictly more
                FAR[0x76ca + s] = FAR[0x76cb + s];            // assignment, slot s+1 unchanged
                swap(w, l);
            }
            for (m = 0; m < DS[0x07ab]; m++)
                if (FAR[0x76ca + s] == FAR[0x2260 + m*0x30a] && FAR[0x57dc] == 0)
                    FAR[0x2374 + m*0x30a] = FAR[0x0008] + 1;
        }
        if (n2 == 2) {                                        // DFB final
            DS[0x07ac] = w + 1;  DS[0x07b0] = l + 1;
            u8 r = manager_index_of_team(FAR[0x76ca], 0xff);
            if (r) screen_title_winner(r - 1, 1);
            FAR[0x76ca] = t0; FAR[0x76cb] = t1;
        }
    }
    // clear goals for the next match (keep the final's result for display)
    if (euro) {
        if (FAR[0x57dc] == 0 && (FAR[0x1d18] != 0 || FAR[0x0009] < 5)) cup_clear_match_goals();
    } else {
        if (FAR[0x57dc] == 0 && FAR[0x0008] < 5) cup_clear_match_goals();
    }
    // next draws
    for (u8 c = euro ? 1 : 0; c < (euro ? 4 : 1); c++)
        if (FAR[0x57dc] == 0 && (c == 0 || FAR[0x1d17 + c] == 0)) cup_draw_next_round(c);
}
```

The winner of each pair ends up in the even slot `s`; `cup_draw_next_round` relies on that. A DFB draw
after penalties is decided only by the goal values left by `match_penalty_shootout` (match area).

### 4.3 `cup_draw_next_round(i8 c)` 27c2:13a2

```c
/// Origin: BMMAIN.EXE 27c2:13a2 (cup_draw_next_round)
void cup_draw_next_round(i8 c) {
    u8 rd = FAR[0x0008 + c];
    u8 n = DS[0x07d6 + rd];            // teams in the coming round (16, 8, 4, 2)
    u8 click = 0;
    if (rd == 5) return;               // final already played
    if (c != 0)                        // any European cup clears all three first-leg rows
        for (s = 0; s < 32; s++) for (k = 0; k < 3; k++) FAR[0x21e2 + k*32 + s] = 0;
    cup_draw_screen_open(c);
    FAR[0x0008 + c]++;
    for (u8 i = 0; i < n; i++)
        if (i < 16) FAR[0x76ca + c*32 + i] = FAR[0x76ca + c*32 + 2*i];   // winners to the front
    for (u8 k = 1; k < 0x15; k++) {    // 20 random swaps
        u8 a = random_range(1, n) - 1;
        u8 b = random_range(1, n) - 1;
        swap(FAR[0x76ca + c*32 + a], FAR[0x76ca + c*32 + b]);
    }
    if (*(u16*)&FAR[0x57c8] != 0) {
        FAR[0x0008 + c]--;
        mouse_pointer_hide(0);
        for (i = 0; i < n; i++) {
            input_poll_mouse(FAR:6dc0); if (*(i16*)&FAR[0x2ea2] == 1) click = 1;
            cup_draw_animate_slot(i, FAR[0x76ca + c*32 + i], click, c);
        }
        mouse_pointer_show();
        if (c == 0) for (i = 0; i < n; i++) cup_amateur_home_swap(FAR[0x76ca + i], FAR[0x76c9 + i], i);
        FAR[0x0008 + c]++;
        cup_draw_screen_close(c);
    }
    if (c == 0) for (i = 0; i < n; i++) cup_amateur_home_swap(FAR[0x76ca + i], FAR[0x76c9 + i], i);
}
```

With the draw shown, the amateur rule runs twice: first while `FAR:0008[0]` still holds the old round
(test `old < 4` → draws for new rounds 2, 3 and 4), then with the new round (`new < 4` → new rounds 2
and 3). The swap is idempotent, so amateurs get home advantage in round 1 (from 09e0), the round of 16 and
the quarter-final, and also in the semi-final only if the draw was displayed. RNG: exactly 40
`random_range(1,n)` calls.

## 5. Draw screen (UI)

### 5.1 `cup_draw_screen_open(u8 c)` 27c2:0006

```
r = DS:07d6[FAR:0008[c]]                      // teams in the round being drawn
FAR:6db4 = NULL
if DS:0608[c != 0] == 0: FAR:57c8 = 0; return  // settings: "show DFB / European draw" off
if c == 0 && r < 4: FAR:57c8 = 1; return       // DFB final: no title, no question
office_show_cached_background()
ui_set_font(rp, 3)
ui_draw_text_centered_shadow(rp, DS:24dc[c], 0, 0x140, 0x14, 0x0b, 0)   // cup name
copy_protection_penalty()
screen_fade(1)
if (i32)DS:2252 == 3: a = 10; b = 0x17 + 3*(c != 0)    // DS:2252 = text-language option
else:                a = 10 + 16*(c != 0); b = 0x17
if DS:05d4 == 0 && dialog_two_choice(5 /* "An Alle" */, DS:4da0, DS:4d7c[a], DS:4d7c[b], "ABER KLAR", "KEIN GEDANKE") != 0:
    screen_fade(0)
    p = pic_load_image(0x2f, 0, 0, 0, 1)      // PIC\47.VGA: draw board
    blit_bitmap(p->bm, 0, 0, rp, 0x13, 0x1a, 0x11a, 0xcb, 0xc0); bitmap_free(p)
    FAR:570e = 0
    FAR:3058 = pic_load_image(0, 0x113, 0xc6, 5, 0);  FAR:a6d0 = FAR:3058->bm   // board backup
    FAR:6db4 = pic_load_image(0x30, 0, 0, 0, 1);      FAR:5158 = FAR:6db4->bm   // PIC\48.VGA lamps
    FAR:3062 = pic_load_image(0, 0x78, 0x0e, 5, 0);   FAR:a6d4 = FAR:3062->bm   // label compositing
    FAR:2ebe = pic_load_image(0, 0x73, 9, 5, 0);      FAR:0174 = FAR:2ebe->bm   // team label
    ui_set_font(FAR:2ebe, 2); rastport_set_draw_mode(FAR:2ebe, 0); screen_fade(1)
    FAR:57c8 = 1
else:
    FAR:57c8 = 0; cup_draw_screen_close(c)
```

Texts: line 1 `"M|CHTEN SIE DEM UN]BERTROFFENEM"`, line 2 DS:4d7c[10] `"SCHAUSPIEL EINER DFB-POKAL-"` or
[26] `"SCHAUSPIEL EINER EUROPA-POKAL-"`, line 3 [0x17] `"AUSLOSUNG BEIWOHNEN ?"` (in the DS:2252 == 3
variant line 3 is [0x1a] for Europe). Buttons `"ABER KLAR"` (yes) / `"KEIN GEDANKE"` (no).

### 5.2 `cup_draw_animate_slot(u8 s, u8 team, u8 fast, i8 c)` 27c2:0378

```
r = DS:07d6[FAR:0008[c]]
if c == 0 && r < 4: return
blit_bitmap(FAR:a6ca, 0x16, 0x1c, FAR:3058, 0, 0, 0x113, 0xc6, 0xc0)   // save board area
rastport_clear(FAR:2ebe, 0x0b)
buf = club_name(team)                                  // FAR:3066 + 0x22*team
if c == 0: tier = league_tier_of_team(team) >> 1      // 0,1,2
           buf += " (" + format_long_thousands(tier + 1) + ")";  colour = DS:5356[tier]
else:      if team < 0x40: colour = 0x12
           else: colour = ((i16)(FAR:3066+0x22*team)[0x18] + [0x19] + [0x1a]) / 3   // u8 fields, i16 IDIV
                 colour = (colour - 0x4b) * 7 / 20 + 2;  clamp_int(&colour, 2, 8)
ui_draw_text_centered_shadow(FAR:2ebe, buf, 2, 0x70, 5, colour, 0xff)
tx = (s & 1) ? 0xa7 : 0x25
ty = (s & 0xfe)*5 - (((r & 0xfe)*5) >> 1) + 0x76
if !fast:
    x = 0x69; y = 0xd1
    for i = 0..6:                                       // label rises out of the drum
        FAR:570e = (FAR:570e + 2) % 8 (implemented as +2, ==8 -> 0)
        vga_wait_vretrace()
        blit(FAR:0174, 0, 0, rp, x, y, 0x72, i+1)
        blit(FAR:5158, FAR:570e, 0, rp, 0x60, 0xd3, 2, 9); blit(FAR:5158, FAR:570e, 0, rp, 0xe5, 0xd3, 2, 9)
        delay_ticks(12 - i); y--
    toggle = 0
    while !(y <= ty && x == tx):
        if toggle == 0: draw_label_at(x, y)
        toggle ^= 1
        x += sign(tx - x); y += sign(ty - y)
draw_label_at(tx, ty)
draw_label_at(x, y):
    blit(FAR:a6d0, x-0x18, y-0x1c, FAR:3062, 0, 0, 0x78, 0x0e); blit(FAR:0174, 0, 0, FAR:3062, 2, 0, 0x71, 7)
    vga_set_draw_page(2); blit(FAR:a6d4, 0, 0, rp, x-2, y, 0x78, 0x0e); vga_set_draw_page(1)
    vga_wait_vretrace(); fill_rect_planar(0x140, 0xf0, x-2, y, 0x78, 0x0e, 2)
```

All blits use minterm 0xc0. No RNG. `fast` is the sticky "left click seen" flag of the draw loop: the
loop polls the mouse once before each slot, so the label in flight when the player clicks still completes
its animation and every later label is drawn at its place at once. The drum rise takes
`(12 + 11 + ... + 6) / 3` = 19 timer ticks (`delay_ticks` divides by 3); the flight draws the label on every
second step, each after a vertical retrace (verified frame-exactly against DOSBox in scenario `cup_draw`).

### 5.3 `cup_draw_screen_close(i8 c)` 27c2:089a

```
if FAR:57c8 != 0:
    if FAR:6db4 != NULL:
        bitmap_free(FAR:6db4); FAR:6db4 = NULL; bitmap_free(FAR:3058); bitmap_free(FAR:3062); bitmap_free(FAR:2ebe)
        ui_set_font(rp, 2)
        ui_draw_text_centered_shadow(rp, "TASTE DR]CKEN", 0x5a, 0xed, 0xd9, 0x1d, 0)
        input_wait_click(FAR:6dc0)
    office_show_cached_background()
    FAR:016e++; screen_cup_matches(c, 0); FAR:016e--
rastport_clear(rp, 0)
```

## 6. `screen_cup_matches(i8 c, u8 minute)` 27c2:1ccb

Pairings and results of the current round. `minute = 0` before the matches (after a draw, from the
office), `0x78` (120) after the match day, or the live minute from `match_live_period`.

Layout computation (all `u8` unless noted):

```
r = DS:07d6[FAR:0008[c != 0]]; if r == 0: r = 1        // pairs (index 6 would read DS:07dc, the day counter)
title = (day >= 315 && minute != 0) ? "Relegationsspiel" : "Pokalspiele"   // copied, never drawn
copy_protection_penalty()
rows (i16) = (c != 0)*r + 1
if c == 0: for s = 0; s < 2r; s += 2: cat[rows++] = (league_tier_of_team(slot[s+1]) << 3) + league_tier_of_team(slot[s])
cnt = (u8)rows; if c != 0: cnt -= 6
if c == 0: for k = 1..6: if no j in 1..rows-1 has cat[j] == DS:0654[k] || cat[j] == DS:065c[k]: cnt++
h   = (u8)(7*cnt + 0x3a)
top = (u8)(trunc_div((i16)(0xe9 - h), 2) - 0x14)
ui_draw_button(rp, 5, top+5, 0x134, h-8, &rect_list, 0, 8)
ui_draw_button(rp, 0x10c, 0xc1, 0, 0x22 + (minute != 0), &rect_ok, 0, 8)   // OK icon
ui_draw_line(rp, 7, top+0x0f, 0x137, top+0x0f, 0x0b)
ui_set_font(rp, 1)
```

Heading (drawn `ui_draw_text_centered_shadow(rp, buf, 5, 0x139, top+0x0d, 0x0b, 7)`):
* `day < 315 || minute == 0`: `buf = DS:24c4[FAR:0008[c]] + DS:24dc[c]`, e.g. `"Viertelfinale DfB-Pokal"`;
  otherwise `buf = "Relegation"`.
* `minute != 0`: append `"    "`, the minute number, `append_ordinal_suffix(buf, minute)` (1310:1fe7) and
  `"Minute"`.
* `minute == 0 && day < 315 && DS:537c[c != 0] >= FAR:016e`: append `"    "` and the date of the next
  match week: `w = FAR:016e; mask = c ? 2*c : 1; while (!(DS:2280[w] >= 8 && ((DS:2280[w] >> 3) & mask))) w++;`
  then `calendar_compute_date(w)` (1310:1190, **overwrites** the date globals FAR:2eba/2e8e/a7a0), append
  `FAR:2eba`, `"."`, `FAR:2e8e + 1`, `"."`. The loop is unbounded; for c = 3 the mask is 6 (bits of
  cups 1 and 2), an original quirk with no visible effect because all European legs are 0x70.

Body (font 2), `y = top + (3 - (c != 0))*8`, `n = 0`:

```
for k = (c != 0 ? 6 : 1) .. 6:
    if c == 0: ui_draw_text_centered_shadow(rp, cup_tier_pairing_label(k, buf), 0, 0x12b, y, 2, 0)
    y += 7; y_head = (u8)y                     // also for the European cups (27c2:2622): first row at top+0x17
    for j = 1 .. rows-1:
        if c == 0 && cat[j] != DS:0654[k] && cat[j] != DS:065c[k]: continue
        s = 2*(j-1); t0 = slot[c][s]; t1 = slot[c][s+1]
        colour(t) = (c == 0) ? (t is a human manager's team ? 0x0b : 1) : (t < 0x40 ? 0x0b : 1)
        row_y[n] = (u8)y; list0[n] = t0; list1[n] = t1; n++
        ui_draw_text_centered_shadow(rp, club_name(t0), 0, 0x82, y, colour(t0), 7)
        ui_draw_text_centered_shadow(rp, "GEGEN", 0, 0x118, y, 3, 0)
        ui_draw_text_centered_shadow(rp, club_name(t1), 0x8c, 0x10e, y, colour(t1), 7)
        g = goals[c][s]; pen = 0; if g > 9: g -= 10, pen = 1; if g > 9: g -= 10, pen = 2
        txt = "(" + g + ":" + goals[c][s+1] + ")" + (pen == 2 ? "n.E." : pen == 1 ? "n.V." : "")
        ui_draw_text_at(rp, txt, c > 0 ? 0xf9 : 0x10c, y, 1)
        if c != 0: ui_draw_text_at(rp, "(" + first_leg[c][s] + ":" + first_leg[c][s+1] + ")", 0x11d, y, 1)
        y += 7
    if c == 0 && y_head == (u8)y: ui_draw_text_centered_shadow(rp, "@@@@@@@@@", 0, 0x118, y, 2, 7); y += 7
```

Interaction: `screen_fade(1)`, then loop `hit = ui_hit_test_buttons(&rect_list, 2, 1)`. With `p =
*(far*)FAR:6dc0`: if `hit == 1 && p[0] > top+5`: `side = p[1] > 0x82`, find the first `i < 32` with
`row_y[i] - 6 < p[0] <= row_y[i]` (16-bit signed; unused entries are 0), and on a match call
`screen_opponent_info(side ? list1[i] : list0[i], side, 0, 0)` (3a41:000e). Word 0 of `*FAR:6dc0` is the
mouse y and word 1 the x (checked in DOSBox: a click at y 72 opens the club of the row drawn at y 74).
The loop ends on `hit == 2`
(OK icon at 0x10c,0xc1) or the abort flag `DS:05d4`. Exit: `ui_wait_with_highlight(0x10c, 0xc1)`,
`screen_fade(0)`, `ui_redraw_buttons(0x7f, 0)`, `rastport_clear(rp, 0)`. No keyboard shortcuts.

### 6.1 `cup_tier_pairing_label(u8 k, char far *buf)` 27c2:1bf0 → `buf`

```
a = k < 4 ? 1 : k < 6 ? 2 : 3
b = k == 1 ? 1 : (k == 2 || k == 4) ? 2 : 3
text_to_upper_de(buf, DS:2384[a]); strcat(buf, " GEGEN "); text_to_upper_de(tmp, DS:2384[b]); strcat(buf, tmp); strcat(buf, ":")
```

k = 1..6 gives `"BUNDESLIGA GEGEN BUNDESLIGA:"`, `"BUNDESLIGA GEGEN ZWEITE LIGA:"`,
`"BUNDESLIGA GEGEN AM.-OBERLIGA:"`, `"ZWEITE LIGA GEGEN ZWEITE LIGA:"`, `"ZWEITE LIGA GEGEN AM.-OBERLIGA:"`,
`"AM.-OBERLIGA GEGEN AM.-OBERLIGA:"`.

## 7. `screen_title_winner(i16 m, u8 comp)` 27c2:274d

`comp` 0 = German championship, 1 = DFB-Pokal, 2..4 = European cups (c+1).

```
screen_fade(0); match_screen_clear_background()                        // 1310:00fa
ui_draw_text_centered_shadow(rp, club_name(FAR:2260[m*0x30a]), 0, 0x13f, 0x21, 0x1d, 0)
if comp == 0: draw "Deutsche Meisterschaft" (0..0x13f, y 0xdd, 0x1d, 0); ui_set_font(rp,1); draw "gewinnt die" (y 0x3c)
else:         draw DS:24d8[comp] (y 0xdd);                              ui_set_font(rp,1); draw "gewinnt den" (y 0x3c)
p = pic_load_image(0x32 + comp, 0, 0, 0, 1)                           // PIC\50..54.VGA trophy
blit_bitmap(p->bm, 0, 0, rp, 0x74, 0x54, 0x58, 0x58, 0xc0); bitmap_free(p)
screen_fade(1); input_wait_release(); input_wait_click(FAR:6dc0); rastport_clear(rp, 0)
```

The championship call is in `season_main_loop` 2a58:38f0-394c: for each manager, when the final
league position byte `FAR:2260[m*0x30a] + 0xec + DS:2262[league]` is 0 and the manager's league
`+0x11a` is 0, it increments the championship counter `+0x1b` and calls `screen_title_winner(m, 0)`.

## 8. Relegation play-off (cross-area: `matchday_relegation_match` 1310:02bf)

Runs in weeks 0x5b and 0x5c. It borrows competition 1 (Champions' Cup) storage and restores it:

```
FAR:0009 = 5; FAR:57dc = 1
match title DS:26ac "Relegationsspiel"; if DS:05d4 == 0: delay_ticks(50)
for k in 0,1: save slot[1][k], goals[1][k], first_leg[1][k]; goals[1][k] = 0
              first_leg[1][k] = FAR:1d18 ? FAR:2e6e[k] : 0
if FAR:1d18 (return leg): slot[1][0] = FAR:5369 (BL rank 15), slot[1][1] = FAR:5370 (2.Liga rank 2)
else (first leg):         slot[1][1] = FAR:5369,             slot[1][0] = FAR:5370
matchday_play(0, 2, 0, 0)            // mask 2 -> 1226/16dc with c = 1 only (FAR:57dc)
if FAR:1d18 == 0: FAR:3060 = cup_tie_winner(1, 0)   // after the return leg: 0 = Bundesliga team won
else:             FAR:2e6e[0..1] = first_leg[1][0..1]
restore the saved slot/goals/first_leg bytes; FAR:57dc = 0
```

The day counter is past 315 here, so a level aggregate is decided by extra time and penalties
(`cup_tie_winner` returns 0x1e), never by away goals.

## 9. Season rollover (cross-area, in `season_main_loop` and `main`)

### 9.1 Promotion and relegation (2a58:3d99-40e2, season end)

```
extra[0..2] = 0
for m = 0; m < DS:07ab; m++:                                  // FAR:304a is the loop variable
    rec = FAR:2260 + m*0x30a
    rec[0x122] = 0                                            // season-end status
    if *(i32*)&rec[0x1d2] < -2000000 && (i8)rec[0x11a] < 2:   // licence withdrawn
        L = rec[0x11a]; rec[0x122] = 4
        find first p < DS:225e[L] with FAR:535a[L*20+p] == rec[0]; if none: continue
        for i = p; i < DS:225e[L]-1; i++: FAR:535a[L*20+i] = FAR:535a[L*20+i+1]
        FAR:535a[L*20 + DS:225e[L]-1] = rec[0]; extra[L]++; rec[0x11a]++
for L = 0..2:
    if L == 0 && FAR:3060 == 0: extra[0]++                    // Bundesliga team won the play-off
    extra[L] = (extra[L] > DS:2266[L]) ? extra[L] - DS:2266[L] : 0        // signed byte compare
    DS:225a[L] = 1
    for k = 0; k < extra[L] + DS:2266[L]; k++:
        down = FAR:535a[L*20 + DS:225e[L] - 1 - k]
        if L < 2: up = FAR:535a[(L+1)*20 + k]
        else:     n = DS:225f + DS:2260 + DS:225e             // 58 (u8)
                  up = (k == 0 && (i8)n < 60) ? random_range(n+3, n+4) : n + 3 - k
        if L == 0 && extra[0] + DS:2266[0] - k == 1 && FAR:3060 == 0 && DS:226a[0] != 0: continue  // play-off slot kept
        r = manager_index_of_team(down, 0xff)
        if r && mgr(r-1)[0x122] == 0:
            if L < 2: mgr[0x122] = 2; mgr[0x11a]++
            else:     mgr[0x122] = 8; *(i32*)&mgr[0x1d2] = 500000; team_swap_slots(down, up, 1)
        r = manager_index_of_team(up, 0xff)
        if r && mgr(r-1)[0x122] == 0: mgr[0x11a]--; mgr[0x122] = 1
        team_swap_slots(down, up, 1)
```

(`mgr(r-1)` = record at `FAR:2260 + (r-1)*0x30a`.) For an amateur-league manager the swap is executed
twice and therefore cancels out. The only RNG call is `random_range(61, 62)` once per season (L = 2,
k = 0).

`team_swap_slots(a, b, 1)` 1310:0b24 (match/season area) returns at once if `a > 0x3f` or `b > 0x3f`; it
renames `a`↔`b` in the player table (`FAR:57dd + 0x25*p + 0x24`, p = 1..150), the manager team ids, the
title holders `DS:07ac..07b0` (via `swap_team_ref_byte`), and swaps the club records `FAR:3066` (0x22
bytes), `FAR:0ecc` (0x36), `FAR:9d36`, `FAR:9336` and further per-team tables.

### 9.2 League shuffle `league_shuffle_teams` 1310:09c5 (main 18bc:0725, season_main_loop 2a58:43a6)

Called right before `euro_cup_qualifiers_build`. Randomizes which team plays which fixture slot:

```
for L = 0..2:
    lo = DS:226e[L]; hi = lo + DS:225e[L] - 1          // 0..17, 18..37, 38..57
    repeat 55 times:
        a = random_range(lo, hi); b = random_range(lo, hi)
        team_swap_slots(a, b, 1)
        for i < DS:225e[L]: if FAR:535a[L*20+i] == a:
            for k < DS:225e[L]: if FAR:535a[L*20+k] == b: swap(FAR:535a[L*20+i], FAR:535a[L*20+k]); goto next
```

### 9.3 Fixture generation (3a41:10e3, suggested name `league_fixtures_for_matchday`, stats area)

Fixtures are not stored; they are derived per match day from the fixed permutation strings DS:06fc
(38 far pointers, MEMORY_MAP §2.1) plus the shuffled team ids:

```
/// Origin: BMMAIN.EXE 3a41:10e3 (FUN_3a41_10e3)
void league_fixtures_for_matchday(u8 L, u8 md /*1-based*/) {
    u8 base = L == 0 ? 0 : L == 1 ? DS:225e : DS:225e + DS:225f;   // 0, 18, 38
    u8 half = (i8)DS:2262[L] >> 1;                                  // 17, 19, 19
    u8 flip = 0;
    if (half < md) { md -= half; flip = 1; }                        // second half: venues swapped
    if (half < md) md = half;
    for (u8 i = 0; i < ((i8)DS:225e[L] >> 1); i++) {
        char far *p = (DS:225e[L] == 18) ? DS:06fc[md] : DS:0744[md];   // 18-team strings 1..17, 20-team 19..37
        u8 h = p[2*i] - 'a', g = p[2*i+1] - 'a';
        if (flip) swap(h, g);
        FAR:4b5e[(L*10 + i)*2]     = h + base;                     // home team
        FAR:4b5f[(L*10 + i)*2]     = g + base;                     // away team
    }
}
```

Detail: both the loop count and the table choice use `DS:225e[L]`, the size of the selected league
(correct behaviour, nothing special to emulate). String indices 0 and 18 are empty strings and are never used,
because `md` is 1-based.

Strings 1..17 (18 teams): `abcdefghijklmnopqr`, `ronklijghefcdabqpm`, `afchejglinkpmrbdqo`, `rknglejchafbdqompi`,
`ajclengpirkodfbhqm`, `rgnclajbhdfqmkoipe`, `fhancpergoimdjblqk`, `rcnbldjfhqkimgoepa`, `arcoemgkhjfldnbpqi`,
`rbnflhjqigkemcoapd`, `amckeijlhnfpdrboqg`, `rfnjlqgeickambodph`, `aicglnjphrfodmbkqe`, `rjnqecgaibkdmfohpl`,
`aecqlrjohmfkdibgpn`, `rncaebgdifkhmjolqp`, `aqnolmjkhifgdebcpr`.
Strings 19..37 (20 teams): `abcdefghijklmnopqrst`, `dahcnklibetqropmjgfs`, `caglinkpqfsbedhjmrot`,
`dstmlhbqrkpingjcaefo`, `gpceobsairktmfqdjlhn`, `phaqdotirgnjesfklcbm`, `oahrcsgtifkbqelnjpmd`,
`bithrjampleofgdksqnc`, `mecqlrgbkaosjtnpidhf`, `tlaibhqofjdgrnpcsmek`, `mqhdcoientjbksgaprlf`,
`sitpegdjrcomahfnblqk`, `hecmnbgsiqkopfldrtja`, `mktcejbpoiqgaldnshfr`, `nagoimrbjsckpdhqtfle`,
`slendrkiapfcqjbtmgoh`, `hmpetdnsgkralqicjofb`, `mjdfqnatcbkholspiger`, `lmbdrsnojkfahigctepq`.

### 9.4 Call order at a season change (season_main_loop) and in `main`

Season end (after the play-off and §9.1): `league_shuffle_teams()` (2a58:43a6), `euro_cup_qualifiers_build()`
(43ab). Season start (date 28 July, 2a58:44ae..4588): week FAR:016e = 0, day DS:07dc = DS:224c, year
DS:07e0++, …, `cup_clear_match_goals()`, `cup_draw_first_round(c, 0)` for c = 0, 1, 2, 3, then
`cup_draw_first_round(c, 1)` for c = 1, 2, 3 (display of the European draws).

`main` (new game): `league_shuffle_teams()` (18bc:0725), `euro_cup_qualifiers_build()` (072a),
`cup_clear_match_goals()` (072f), `cup_draw_first_round(c, 0)` for c = 1..3 (silent, before the start menu);
later (18bc:0888) `cup_draw_first_round(0, 0)` and `cup_draw_first_round(c, 1)` for c = 1..3.

## 10. Globals used

| Address | Type | Suggested name | Save block |
|---|---|---|---|
| FAR:0008[4] | u8 | `cup_round` | 30 |
| FAR:76ca[4][32] (FAR:76c9 read) | u8 | `cup_slots` | 28 |
| FAR:4c7e[4][32] | u8 | `cup_match_goals` | 33 |
| FAR:21c2[4][32] (rows 1..3 = FAR:21e2) | u8 | `cup_first_leg_goals` | 29 |
| FAR:1d17[c] (c=1..3 → 1d18..1d1a) | u8 | `euro_second_leg_pending` | 31 |
| FAR:1d14..1d17 (via FAR:1d13+r) | u8 | `manager_plays_now`: manager m's team plays now; set by 2a58:20b2, cleared by 27c2:1226 (which also writes FAR:1d13 + r) and by the season loop, read by 1310:2ee5 and the news code | not saved |
| FAR:57aa[3][6] (read as 57a4+c*6) | u8 | `euro_qualifiers` | 26 |
| FAR:56fe[10] | u8 | `euro_qualified_list` | 25 |
| FAR:2e74 | u8 | `euro_qualified_count` | not saved |
| FAR:535a[3][20] | u8 | `final_table` | 32 |
| FAR:2260+m*0x30a | u8 | `manager_team` | 13 (+0x1e) |
| FAR:2374+m*0x30a+c | u8 | `manager_cup_stage` | 13 (+0x132) |
| FAR:237a+m*0x30a | i8 | `manager_league` | 13 (+0x138) |
| FAR:2382+m*0x30a | u8 | `manager_season_status` (1 up, 2 down, 4 licence, 8 out) | 13 (+0x140) |
| FAR:2432+m*0x30a | i32 | `manager_balance` | 13 (+0x1f0) |
| FAR:227b+m*0x30a | u8 | `manager_championships` | 13 (+0x39) |
| FAR:2e6e[2] | u8 | `relegation_first_leg` | 27 |
| FAR:3060 | u8 | `relegation_result` | 60 |
| FAR:57dc | u8 | `relegation_mode` | not saved |
| FAR:016e | u8 | `week_index` | 49 |
| FAR:2e99 | u8 | `human_in_extra_time` | not saved |
| FAR:57c8 | u16 | `cup_draw_visible` | not saved |
| FAR:570e | u16 | `draw_lamp_phase` | not saved |
| FAR:3058, 3062, 2ebe, 6db4 (far ptrs) | sprite* | `draw_board_backup`, `draw_label_comp`, `draw_label`, `draw_lamps` | not saved |
| FAR:a6d0, a6d4, 0174, 5158 | bitmap* | pixel pointers of the four above | not saved |
| FAR:a6ca | bitmap* | screen bitmap (source of the board backup) | not saved |
| FAR:3066+0x22*t | club record | `club_table` (name +0, strength bytes +0x18..+0x1a) | 15 |
| FAR:2eba, FAR:2e8e, FAR:a7a0 | i32 | date (day, month 0-based, year) | 20, 21, 22 |
| FAR:6dc0 | far ptr | mouse position | not saved |
| FAR:2ea2 | i16 | last mouse event | not saved |
| DS:07ac..07b0 | u8 | `dfb_cup_winner_p1`, `champions_cup_holder_p1`, `cup_winners_cup_holder_p1`, `uefa_cup_holder_p1`, `dfb_cup_runner_up_p1` | 8..12 |
| DS:07ab | u8 | `human_manager_count` | 7 |
| DS:07dc | i32 | `day_counter` | 23 |
| DS:07d6[6] | u8 | `cup_teams_after_round` = 32,16,8,4,2,1 | constant |
| DS:2280[95] | u8 | `week_competition_mask` | 50 |
| DS:225a[3] | u8 | `league_matchday` | 34 |
| DS:225e[3], DS:2262[3], DS:2266[3], DS:226a[3], DS:226d, DS:226e[3], DS:2277 | i8 | league sizes, match days, relegation places, play-off flag, UEFA places, id bases, max DFB id | constant |
| DS:0608, DS:0609 | u8 | `show_dfb_cup`, `show_euro_cups` (settings) | 36 (DS:05fe[15]) |
| DS:05d4 | u8 | `abort_requested` | not saved |
| DS:05ac | u8 | `copy_protection_passed` | 54 (copied to DS:05fa) |
| DS:2252 (i32) | | text-language option (3 changes dialog lines, see 1310:1fe7) | constant / option |
| DS:0654[7], DS:065c[7], DS:5348, DS:5356[3], DS:537c[2] | u8/u16 | display tables (§0.5) | constant |

## 11. Open questions

1. How `match_penalty_shootout` (1310:356d) records the shoot-out winner in `FAR:4c7e/4c7f`. The DFB
   code only compares goals after stripping the +10/+20 markers (home team wins on equality), and the
   European code calls `cup_tie_winner` again; both rely on the shoot-out adjusting the goal bytes.
   Resolved: `match_penalty_shootout` overwrites both bytes, home = shoot-out score + 0x14, away =
   shoot-out score (`crates/bmp-game/src/matchsim.rs`).
2. `FAR:2e74` (length of the European-qualified list) is not in the save file, so after loading a game
   `team_is_euro_qualified` returns 0 until the next season end. `season_end_messages` (1cb5:02f1) uses
   it at season end, after `euro_cup_qualifiers_build` has rebuilt it, so this is probably harmless.
3. `cup_check_ties_extra_time` writes `FAR:1d13 + r` with an unmasked `r = 0x81+m` for human away teams
   (lands in the high-score buffer `FAR:1d94+m`). Whether it is ever visible depends on when `HIGH.XY` is
   written from `FAR:1d52`.
4. Threshold mismatch: `cup_tie_winner` uses day > 315, `screen_cup_matches` uses day >= 315 for the
   "Relegation" heading. Kept as-is.
5. DS:224e/DS:2252 appear to be language/text-variant options (see `append_ordinal_suffix` 1310:1fe7),
   not difficulty as MEMORY_MAP says; the `DS:2252 == 3` dialog variant in `cup_draw_screen_open`
   prints the DFB line together with the Europe line.
6. MEMORY_MAP lists `016e` under DGROUP; every access in this area goes through a selector for `5238`,
   matching save block 49 (`FAR:016e`).
7. Word 0 of the mouse-position struct `*FAR:6dc0` is compared against row y values in
   `screen_cup_matches` while MEMORY_MAP calls it x. Resolved: word 0 is y, word 1 is x (DOSBox
   scenario `cups`, ported as `crates/bmp/src/game/cup_screens.rs`).
8. `team_swap_slots` 1310:0b24 was only skimmed (owned by the match/season area); the full list of
   swapped per-team tables must come from that spec. Resolved: full spec in match.md
   "1310:0b24 `team_swap_slots`", ported as `GameState::team_swap_slots`.
