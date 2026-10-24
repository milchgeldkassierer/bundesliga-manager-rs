# Newspaper, match report and message helpers (segments 3e3a and 4091)

Reverse-engineered from `/unpacked/BMMAIN_unpacked.EXE`. All addresses are Ghidra addresses
(`seg:off`, load segment 0x1000). `DS:xxxx` is DGROUP (5cb3) and `FAR:xxxx` is the far data
segment 5238. Every `MOV ES,[DS:a0xx]` selector used in this area was resolved from the image. The
relocated image was rebuilt from `out/funcmap/BMMAIN_UNPACKED.EXE` (scratch tools under `out/news/`).

Scope:

* Segment **3e3a** holds the post-match newspaper: the per-match event record, the player grading
  after a match, the headline and body template engine, the justified text layout and the
  sidebar. Nothing else lives in 3e3a.
* Segment **4091** holds small helpers: the per-manager message inbox, dialogs, the icon slots and the
  value spinner. `clamp_int`, `clamp_char`, `text_to_upper_de` and `long_abs_diff` are specified
  in [rng.md](rng.md) and are only summarised here.

Conventions in the pseudo-code: `u8`/`i8`/`u16`/`i16`/`i32` are exact widths. `/` and `%` on
signed values truncate toward zero (x86 `IDIV`, MSC `_aFldiv`). "byte compare" means an 8-bit
compare whose signedness is stated. `rp` is the main screen rastport `*(far*)FAR:2f28`. `vp` is the
viewport struct pointer `*(far*)FAR:2e70`. Called library functions use the names from
FUNCTION_MAP.md. Their argument lists are given in C order (first argument first), already
un-reversed from the push order.

Strings are quoted as raw bytes. The game fonts draw `[`=Ä, `\`=Ö, `]`=Ü, `{`=ä, `|`=ö, `}`=ü
and `~`=ß. Most templates use upper-case text and so use `[ \ ]`. Some templates use `|` for Ö and
`~` for ß (`MU~TE`).

## 0. Ghidra state after this pass

* **Created** `3e3a:000e` (`newspaper_draw_sidebar`). It was missing because the last function of
  3a41 runs into the segment boundary (3a41:3f90 = 3e3a:0000).
* **Bogus functions** (plate comments set, kept, not deleted):
  * `3e3a:093e` `str_insert_space_fragment` is the middle of `str_insert_space` 3e3a:0919.
  * `3e3a:0fdf` `newspaper_generate_fragment_a` and `3e3a:1b29` `newspaper_generate_fragment_b`
    are pieces of `newspaper_generate_and_show` (3e3a:0ea3). That function's real body is
    **0ea3..21ee**.

  Their "callers" in 2ecd are `PUSH CS; CALL near` sequences that Ghidra resolved with the wrong
  CS. They really call 2ecd:06af and 2ecd:11f9.
* Because of that split, Ghidra's decompilation of 0ea3 is unusable. The pseudo-code below comes
  from the disassembly.
* Near calls inside a segment are `PUSH CS; CALL rel16` (`0e e8`). Ghidra misses many of them as
  xrefs. The caller lists below come from a byte scan.

## 1. Function list

| Address | Name (Ghidra) | Kind | Summary |
|---|---|---|---|
| 3e3a:000e | `newspaper_draw_sidebar` | game UI | lineup with grades, goals, yellow and red cards (created) |
| 3e3a:060a | `news_draw_label_value_line` | UI | draw "label:" in colour 6 and the rest in colour 0xE |
| 3e3a:06df | `news_text_flow` | UI/layout | word-wrapping, fully justified text layout with state in BSS |
| 3e3a:0919 | `str_insert_space` | string | insert a blank at a position |
| 3e3a:093e | `str_insert_space_fragment` | bogus | part of 0919 |
| 3e3a:0964 | `news_expand_template` | text gen | template expander (recursive) |
| 3e3a:0c0b | `newspaper_draw_body` | text gen | expand and lay out the selected body templates |
| 3e3a:0d33 | `news_flow_words` | layout | split a string into words for `news_text_flow` |
| 3e3a:0dba | `newspaper_draw_headline` | text gen | two-line headline |
| 3e3a:0ea3 | `newspaper_generate_and_show` | game logic + screen | analyse the match, adjust grades, choose templates, show the page |
| 3e3a:0fdf, 1b29 | `newspaper_generate_fragment_a/_b` | bogus | parts of 0ea3 |
| 3e3a:21ef | `random_chance_pick` | RNG helper | see rng.md §2.2 |
| 3e3a:223e | `news_record_match_event` | game logic | log a match event into the newspaper record |
| 3e3a:23aa | `match_rate_players_and_newspaper` | game logic | post-match player grades (1..6) for all managers, then newspaper |
| 4091:0000 | `team_slot_free_index` | game | first free slot in a 10-byte row |
| 4091:0044 | `message_delete` | game | remove a message string from the inbox table |
| 4091:0190 | `message_post` | game | build a dated message from a template and insert it at inbox position 0 |
| 4091:0532 | `messages_free_all` | memory | free all message strings |
| 4091:05c4 | `screen_messages` | UI screen | message inbox viewer (3 per page) |
| 4091:0aaa | `ui_messages_wait_button` | UI | wait for a click on the 3-button strip of the inbox |
| 4091:0b1b | `ui_spinner_draw_value` | UI | draw the value of the spinner |
| 4091:0ba5 | `ui_value_spinner` | UI | +/- value spinner widget (MEMORY_MAP: `money_round_display`) |
| 4091:0e3a | `dialog_okay` | UI | 4-line dialog with an "OKAY" button |
| 4091:1009 | `clamp_int` | lib | rng.md |
| 4091:1033 | `clamp_char` | lib | rng.md (no callers) |
| 4091:105d | `text_to_upper_de` | lib | rng.md |
| 4091:10d5 | `long_abs_diff` | lib | rng.md (returns a **long** in DX:AX) |
| 4091:1109 | `squad_count_by_flag` | game | count squad players by mode |
| 4091:11cc | `ui_hit_icon_slot` | UI | hit-test the registered icon slots |
| 4091:123d | `ui_register_icon_slot` | UI | draw and register a 56x11 text button |
| 4091:146c | `dialog_two_choice` | UI | addressed 3-line dialog with two buttons (FUNCTION_MAP: `message_broadcast`) |
| 4091:1722 | `ui_wait_right_click_button` | UI | draw a button, wait for mouse event 2, redraw it |
| 4091:17b7 | `find_player_by_id` | game | search squads 0..4 for a player id |
| 4091:185c | `ui_draw_manager_header` | UI | manager portrait, name and team badge at the top right |

## 2. The newspaper match record `FAR:90ca + m*0x9a`

One record per human manager `m` (0..3). The array occupies FAR:90ca..9331. It is not saved
(no savegame block). It is created by `2a58:20b2` (the match-day driver, not in this area). That
function zero-fills it (`mem_alloc(0x9a,1)` + `far_memcpy_bytes` + `mem_free`), then sets `+0x0e`,
`+0x0f`, `+0x07`, `+0x12` and `+0x96`. During the match, `match_ticker_events` fills it through
`news_record_match_event`.

| Off | Type | Name | Written by | Meaning |
|---|---|---|---|---|
| 0x00 | u8 | `deficit_own` | 223e | own goals at the moment of the biggest deficit (`%2`) |
| 0x01 | u8 | `deficit_opp` | 223e | opponent goals at that moment (`%3`) |
| 0x02 | u8 | `lead_own` | 223e | own goals at the moment of the biggest lead (`%4`) |
| 0x03 | u8 | `lead_opp` | 223e | opponent goals then (`%5`) |
| 0x04 | u8 | `goals_own` | 223e | own goals (`%0`) |
| 0x05 | u8 | `goals_opp` | 223e | opponent goals (`%1`) |
| 0x06 | i8 | `n_events` | 223e | count of type-0 and type-1 events (chances and goals); clamped by 0ea3 to 3..10 |
| 0x07 | u8 | – | 2a58:20b2 | `FAR:0efa[0x36*opp_team]` (unused here) |
| 0x09 | i8 | `goal_diff` | 223e | `goals_own - goals_opp` after the last goal |
| 0x0a | i8 | `min_diff` | 223e | most negative goal difference reached (<= 0) |
| 0x0b | i8 | `max_diff` | 223e | most positive goal difference reached (>= 0) |
| 0x0c | u8 | `hero` | 0ea3 | 1-based squad index of the "hero" player (`%c`), 0 = none |
| 0x0d | u8 | `loser` | 0ea3 | 1-based squad index of the "loser" player (`%d`), 0 = none |
| 0x0e | u8 | `club_a` | 2a58:20b2 | own club index (`%a`) |
| 0x0f | u8 | `club_b` | 2a58:20b2 | opponent club index (`%b`) |
| 0x11 | u8 | `result` | 223e | 0 = draw, 1 = own team leads/won, 2 = behind/lost (after the last goal) |
| 0x12 | u8 | `sold_out` | 2a58:20b2 | 1 if the home stadium was full |
| 0x13 | u8[20] | `goal_minute[]` | 223e | minute of each goal |
| 0x27 | u8[20] | `goal_player[]` | 223e | 0-based squad index of the scorer, 0x64 = opponent goal |
| 0x3b | u8[20] | `chance_minute[]` | 223e | minute of each type-0 event |
| 0x4f | u8[20] | `chance_player[]` | 223e | 0-based squad index, 0x64 = opponent |
| 0x63 | u8[20] | `goal_arg4[]` | 223e | 4th argument of goal events (assist?) – not read in this area |
| 0x77 | u8[20] | `chance_arg4[]` | 223e | same for type-0 events – not read |
| 0x8b | u8 | `n_goals` | 223e | entries in the goal arrays (no bound check, >20 overflows) |
| 0x8c | u8 | `n_chances` | 223e | entries in the chance arrays |
| 0x8d | u8[6] | `yellow[]` | 223e | 0-based squad indices of booked players |
| 0x93 | u8 | `red` | 223e | 1-based squad index of the player sent off, 0 = none |
| 0x94 | u8 | `n_yellow` | 223e | entries in `yellow[]` (no bound check) |
| 0x96 | i32 | `spectators` | 2a58:20b2 | spectators (`%9`). **0 means away match.** 2a58:20b2 stores `value + 200000` in one case (`[bp-0x24]==2`), and 000e resets values > 200000 to 0 |

## 3. Event recording and player grades

### 3.1 `news_record_match_event` 3e3a:223e

```c
void news_record_match_event(u8 m, u8 type, u8 player, u8 arg4, u8 minute)
// type: 0 = chance, 1 = goal, 2 = red card, 3 = yellow card (meaning inferred from the
// sidebar and newspaper use; the codes come from match_ticker_events param_1).
// player: 0-based squad index of manager m's team, 0x64 = opponent.
{
    rec = &FAR[0x90ca + (u16)(m * 0x9a)];          // MUL byte -> 16-bit
    if (type < 2) rec[0x06]++;                      // u8 wrap
    if (type == 1) {
        if (player != 0x64) rec[0x04]++; else rec[0x05]++;
        i8 d = (i8)(rec[0x04] - rec[0x05]);         // 8-bit SUB
        rec[0x09] = d;
        rec[0x11] = (d > 0) + 2 * (d < 0);          // signed byte compares
        if ((i8)rec[0x0a] > d) { rec[0x0a] = d; rec[0x00] = rec[0x04]; rec[0x01] = rec[0x05]; }
        if ((i8)rec[0x0b] < d) { rec[0x0b] = d; rec[0x02] = rec[0x04]; rec[0x03] = rec[0x05]; }
        k = rec[0x8b];
        rec[0x13 + k] = minute; rec[0x27 + k] = player; rec[0x63 + k] = arg4;
        rec[0x8b]++;
    }
    if (type == 0) {
        k = rec[0x8c];
        rec[0x3b + k] = minute; rec[0x4f + k] = player; rec[0x77 + k] = arg4;
        rec[0x8c]++;
    }
    if (type == 2) rec[0x93] = player + 1;          // u8
    if (type == 3) { rec[0x8d + rec[0x94]] = player; rec[0x94]++; }
}
```

There are no random calls. Callers are five sites in `match_ticker_events` 2a58:0ca3:

* 2a58:1501 and 2a58:1960 call `(m, param_1, 0x64, 0x64, param_7)` (opponent events).
* 2a58:1865 calls `(m, param_1, own_index, [bp-0xa], param_7)`.
* 2a58:1b5a and 2a58:1cee call `(param_8, param_1, param_7, 0, 0)`. These are the card events: the
  first follows a send-off, which sets player `+0x0a = 0` and `+0x15 -= 10`.

### 3.2 `match_rate_players_and_newspaper` 3e3a:23aa

The only caller is 1310:1bc1 in `FUN_1310_15db`, which passes its own second argument (`param`).

```c
void match_rate_players_and_newspaper(u8 param)
{
    for (u8 m = 0; m < DS:07ab; m++) {                 // number of human managers
        if (FAR:1d14[m] == 0) continue;               // manager had no match today
        FAR:5358 = m;
        u8 n = squad_count_by_flag(m, 0);
        for (u8 i = 0; i < n; i++) {
            u8 far *p = &FAR[0x774a + (i16)(0x34 * (u16)(25 * m + i))];   // player record
            if (p[0x0d] == 0x64) { p[0x0d] = 1; p[0x09] |= 1; }
            if (p[0x0a] >= 0x0c || p[0x0a] == 0) continue;               // did not play
            i16 a = FUN_1310_1bd7(m, i);
            i16 b = FUN_1310_1cd3(m, i);
            i8 f = (i8)p[0x15]; if (f < -12) f = -12; if (f > 36) f = 36;
            i16 q = (i16)( long_abs_diff(30L, (i32)p[0x12]) / 6L );     // _aFldiv, low word
            i16 v = ((i16)(p[0x10] + p[0x11]) / -33 - q + 12) * 2;       // IDIV by 0xffdf
            i16 c = (6 + (-(f + 12)) / 6) * 4;
            c += p[0x13] / 33;                                          // 8-bit DIV, unsigned
            c += b / 5;
            c += a;
            v += c;
            i8 g = (i8)(v / 11);                                        // low byte of IDIV
            if (g < 1) g = 1; if (g > 6) g = 6;
            FAR:4d3c[((m << 5) + i) << 3] = g;                          // grade 1 (best)..6
        }
        if (param == 0) {
            newspaper_generate_and_show(m, param /* = 0 */);
            ui_redraw_buttons(0x7f, 0);
        }
    }
}
```

There are no random calls here. The 1310 helpers belong to the match agent.

### 3.3 Grade adjustments inside `newspaper_generate_and_show`

The hero and loser bonuses (and their `random_range` calls) run **only when the newspaper is
enabled** (`DS:060b != 0`, see §4). A port must keep this coupling: the option changes both the
grades and the RNG stream.

## 4. `newspaper_generate_and_show(u8 m, u8 param)` 3e3a:0ea3..21ee

### 4.1 Screen set-up (in this order)

```c
if (DS:060b == 0) return;                           // newspaper option (save block 36, DS:05fe[13])
rec = &FAR[0x90ca + m*0x9a];
i16 gstart[25];                                     // body group start indices (prefix sums)
for (k = 0, s = 0; k < 25; k++) { gstart[k] = s; s += DS:9350[k]; }
club_a = rec[0x0e]; club_b = rec[0x0f];
ui_fill_rect(rp, 0, 0, 0x13f, 0xef, 9);
ui_fill_rect(rp, 5, 1, 0x13a, 0xee, 0x0f);
pic = vga_load_picture(0x2c, 0, 0, 0, 1);           // PIC\44.VGA: masthead
blit_bitmap(*pic, 0, 0, rp, 10, 3, 0x12b, 0x1e, 0xc0);
bitmap_free(pic);
palette_load_rgb4(vp + 8, DS:9394, 16);             // grey palette, see §8
pic = vga_load_picture(200 + random_range(0, 0x1d), 0, 0, 0, 1);    // RNG #1: photo PIC 200..229
bm  = *pic;                    // bitmap descriptor
w   = FAR:a6ce;                // word, used as photo width
h   = bm->word[1];             // bm + 2
x   = (0x6b - w) / 2 + 0xca;   // signed, truncating
y   = (0x6a - h) / 2 + 0x3e;
ui_draw_frame(rp, x - 2, y - 2, x + w + 1, y + h + 1, 0);
blit_bitmap_scaled_or_copy(bm, 0, 0, *(far*)FAR:a6ca, x, y, w, h, 0xc0, 0x0f, 0, 0);
bitmap_free(pic);
```

### 4.2 Match analysis

`W[11] = DS:937e = {3, 4, 5, 7, 9, 10, 9, 7, 5, 4, 3}` (i16) is the time-window weight.

```c
i16 heat[120] = {0};
i16 own_cnt = 0, opp_cnt = 0;                 // [bp-0x36], [bp-0x14e]
bool early_own = 0, early_opp = 0, late_decider = 0;   // [bp-0x4a], [bp-0x15a], [bp-0x3e]
for (i = 0; i < rec[0x8b]; i++) {             // goals, i16 loop counter
    u8 t0 = rec[0x13 + i];
    bool last = (rec[0x8b] - i - 1 == 0);
    if (rec[0x27 + i] != 0x64) {              // own goal
        if (t0 < 8 && !early_opp) early_own = 1;
        if (last && t0 > 0x50 && (own_cnt == opp_cnt || own_cnt - opp_cnt == -1) && param == 0)
            late_decider = 1;
        own_cnt++;
    } else {                                  // opponent goal
        if (t0 < 8 && !early_own) early_opp = 1;
        if (last && t0 > 0x50 && (own_cnt == opp_cnt || opp_cnt - own_cnt == -1) && param == 0)
            late_decider = 1;
        opp_cnt++;
    }
    for (k = 0, t = t0 - 5; t < t0 + 6; t++, k++) {   // i16
        if (t <= 0 || t >= 120) continue;
        if (rec[0x27 + i] == 0x64) heat[t] -= W[k]; else heat[t] += W[k];
    }
}
rec[0x0c] = 0; rec[0x0d] = 0;
i16 n = squad_count_by_flag(m, 0);
for (j = 0; j < n; j++) {                     // i16
    u8 far *p = &FAR[0x774a + (i16)(0x34 * (25 * m + j))];
    i8 f = (i8)p[0x15];
    if (FAR:57fc[0x25 * p[0x0f]] != 0 && f > 25) {     // player table byte +0x1f
        rec[0x0c] = j + 1;                              // last qualifying player wins
        i16 g = (i16)FAR:4d3c[((m<<5)+j)<<3] - random_range(1, 2);   // RNG
        if (g < 1) g = 1;
        FAR:4d3c[((m<<5)+j)<<3] = (u8)g;
    }
    if (f < -15 && rec[0x0d] == 0) {                     // first qualifying player only
        rec[0x0d] = j + 1;
        u8 g = FAR:4d3c[...] + (u8)random_range(1, 2);  // RNG, 8-bit add
        if (g > 6) g = 6;                                // unsigned compare
        FAR:4d3c[...] = g;
    }
}
i16 own_ch = 0, opp_ch = 0;                   // [bp-0x38], [bp-0x150]
for (i = 0; i < rec[0x8c]; i++) {             // type-0 events
    u8 t0 = rec[0x3b + i];
    for (k = 0, t = t0 - 5; t < t0 + 6; t++, k++) {
        if (t <= 0 || t >= 120) continue;
        if (rec[0x4f + i] == 0x64) { heat[t] -= W[k]; if (t == t0) opp_ch++; }
        else                       { heat[t] += W[k]; if (t == t0) own_ch++; }
    }
}
i32 dom = 0; for (i = 0; i < 120; i++) dom += (i32)heat[i];   // sign-extended
FAR:579c = (rec.spectators == 0) + 1;         // selector 1: 1 home, 2 away
dom /= 120;                                   // _aFldiv (signed)
if (dom > 2) dom = 2;                         // no lower clamp
if ((i8)rec[0x06] < 3) rec[0x06] = 3;
if ((i8)rec[0x06] > 10) rec[0x06] = 10;
i8 q = (rec[0x11] == 1) + ((i8)rec[0x06] - 3) / 2 + 1;
FAR:579d = (q > 4) ? 4 : q;                   // selector 2: match quality 1..4
FAR:579e = rec[0x11] + 1;                     // selector 3: 1 draw, 2 win, 3 loss
i16 sa = (team_strength_value(club_a, 1, 0) + team_strength_value(club_a, 0, 0)
          + team_strength_value(club_a, 2, 0)) * 2 / 15;     // 1310:1468, call order as written
i16 sb = (same for club_b) * 2 / 15;
FAR:579f = 1;                                 // selector 4: manager satisfied?
if (rec[0x11] == 1 || (rec[0x11] == 0 && sb > sa)
    || (rec[0x11] == 0 && rec.spectators == 0 && sa + 10 < sb))
    FAR:579f = 2;
FAR:57a0 = 2;                                 // selector 5: 1 deserved, 2 lucky
if (dom > 0) FAR:57a0 = 1;
else if (random_range(0, 1) != 0 && dom == 0) FAR:57a0 = 1;   // RNG only when dom <= 0
```

`team_strength_value(club, k, 0)` is pushed as `(club, k, 0)` (C order). Its result is summed as
i16.

### 4.3 Body template selection (written to `FAR:2e7e[]`, `n_tpl` = `[bp-0x152]`)

`pick(g)` means `FAR:2e7e[n_tpl++] = (u8)random_range(gstart[g], gstart[g] + DS:9350[g] - 1)`.
Group 0 uses the literal `random_range(0, DS:9350[0] - 1)`. Steps 1 to 14 run in this order (RNG
calls in brackets):

| # | Condition | Group (templates) |
|---|---|---|
| 1 | always | g0 (0-7) intro |
| 2 | `spectators != 0` ? g1 : g2 | g1 (8-12) home crowd / g2 (13-16) travelling fans |
| 3 | `random_range(0,1) != 0 && rec[0x94] > 2` (RNG always) | g3 (17-24) hard play |
| 4 | `random_range(0,1) != 0 && rec[0x94] == 0` (RNG always) | g4 (25-29) fair play |
| 5 | `rec[0x0c] != 0 && rec[0x11] == 1` | g10 (66-69) hero |
| 6 | `rec[0x0d] != 0 && rec[0x11] == 2` | g11 (70-73) scapegoat |
| 7 | always | g5 (30-36) manager quote |
| 8 | `late_decider` | g18 (100-101) late result |
| 9 | `own_ch > 3 && (i8)rec[0x09] < 2` | g12 (74-80) poor finishing |
| 10 | `early_opp` | g16 (98) early deficit |
| 11 | `early_own` | g17 (99) early lead |
| 12 | `own_ch < 2 && rec[0x11] == 1` | g13 (81-86) clinical finishing |
| 13 | `rec[0x11] == 2` | g19 (102-108) loss verdict |
| 14 | `rec[0x11] == 1` | g7 (41-47) win verdict |

### 4.4 Headline selection (runs after step 14; some groups add body templates)

`hl` (`[bp-0x162]`, u8) starts at 0, and the flag `pf` (`[bp-0x44]`) starts at 0. Headline group
`k` covers `[hbase, hbase + DS:9322[k] - 1]`. `hbase` starts at 0, and after every group (taken or
not) `hbase += DS:9322[k]; k++`. A candidate is
`hl = random_chance_pick(n, hl, &pf, (u8)hbase, (u8)(hbase + DS:9322[k] - 1))`. It always calls
`random_range(1, n)`. If the result is `> pf`, it sets `pf = 1` and also calls
`random_range(lo, hi)`. So the first candidate always wins, and each later one replaces it with
probability (n-1)/n.

Let `d = (i8)rec[0x09]`, `r = rec[0x11]`, `S5 = FAR:57a0`, `ad = long_abs_diff(0, dom)` (long).
Here `rec[0x09]` is the goal difference after the last goal, so it is 0 when no goal was scored.

| k | idx | Condition | n | Extra body template in the same block |
|---|---|---|---|---|
| 0 | 0-3 | `d > 3` | 5 | |
| 1 | 4-5 | `d > 0 && d < 2` | 5 | |
| 2 | 6-8 | `d > 0 && d < 4` | 5 | |
| 3 | 9-12 | `d < -3` | 5 | |
| 4 | 13-14 | `d < 0 && d > -2` | 5 | |
| 5 | 15-17 | `d < 0 && d > -4` | 5 | |
| 6 | 18 | `d == 0` | 2 | |
| 7 | 19-20 | `rec[0x12] != 0` (sold out) | 3 | then pick(g6) (37-40), inside the same `if` |
| – | – | `rec[0x0a] != 0 && r != 2` | – | pick(g14) (87-92) comeback (no headline) |
| – | – | `rec[0x0b] != 0 && r != 1 && d > -2` | – | pick(g15) (93-97) lead given away (no headline) |
| 8 | 21-24 | `rec[0x0a] != 0 && r == 1` | 5 | |
| 9 | 25-27 | `rec[0x0b] != 0 && r == 2` | 5 | |
| 10 | 28-29 | `(i8)rec[0x0a] < -1 && r == 0` | 4 | (texts talk about a lost lead; original data/condition mismatch, keep it) |
| 11 | 30-31 | `(i8)rec[0x0b] > 1 && r == 0` | 4 | (texts talk about a comeback; swapped with k10 in the original) |
| 12 | 32-38 | `rec[0x0c] != 0 && r == 1` | 4 | |
| 13 | 39-42 | `(i8)rec[0x06] > 7 && ad < 3` | 3 | |
| 14 | 43-47 | `(i8)rec[0x06] < 4 && ad < 3` | 3 | |
| 15 | 48-50 | `rec[0x0d] != 0 && r == 2` | 4 | |
| 16 | 51-53 | `S5 == 1 && r == 2` | 4 | |
| 17 | 54-56 | `S5 == 2 && r == 1` | 4 | |
| 18 | 57-59 | `S5 == 1 && r == 1` | 3 | then pick(g8) (48-55) |
| 19 | 60-62 | `S5 == 2 && r == 2` | 3 | then pick(g9) (56-65) |
| 20 | 63-64 | `ad < 2 && r == 0` | 4 | |
| 21 | 65-66 | `S5 == 2 && r == 0` | 3 | |
| 22 | 67-68 | `S5 == 1 && r == 0` | 3 | |

`ad < 3` is the long compare `hi <= 0 && (hi < 0 || lo < 3)`. Because `long_abs_diff` returns a
positive value, that means `|dom| < 3`. Body groups 20..24 (DS:9350[20..24] = 3, 2, 2, 3, 4) are
never used, and only template 109 exists of group 20.

### 4.5 Output

```c
newspaper_draw_headline(m, hl);
ui_set_font(rp, 2);
newspaper_draw_body(m, (u8)n_tpl);
newspaper_draw_sidebar(m);
screen_fade(vp, 1);
input_wait_click(*(far*)FAR:6dc0);
screen_fade(vp, 0);
palette_load_rgb4(vp + 8, *(far*)DS:0794, 0x100);   // restore the game palette
```

### 4.6 RNG call order of one newspaper (summary)

1. `random_range(0,29)` photo.
2. Per squad slot j (0..n-1): hero `random_range(1,2)` if it qualifies, then loser
   `random_range(1,2)` if it qualifies.
3. `random_range(0,1)` only if `dom <= 0`.
4. Body steps 1..14 in table order (steps 3 and 4 always draw their coin first).
5. Headline groups k0..k22 in order. Each taken group draws 1 or 2 numbers, and the extra
   pick(g6/g14/g15/g8/g9) calls happen at their table positions.
6. Headline expansion (`news_expand_template`, line 1 then line 2): one `random_range(1,M)` per
   `%x0M` met, in text order.
7. Body expansion, template after template, stopping when the page is full (§5.3). If the integrity
   check of §5.3 fails, one `random_range(0,1)` per line instead.
8. The sidebar uses no RNG.

## 5. Template engine

### 5.1 `news_expand_template(u8 m, char far *src, char far *dst)` 3e3a:0964 → `char far *`

The function writes the expansion at `dst` (it first sets `*dst = 0`). It returns the source
pointer where it stopped: on `#`, on NUL, or just after `%e`. `sel[]` = `FAR:579b[]` (u8). Index 0
is never read through `%x0`. `rec` = the record of `m`.

```c
char far *news_expand_template(u8 m, char far *p, char far *out)
{
    bool stop = 0;
    *out = 0;
    while (!stop && *p != '#' && *p != 0) {
        if (*p != '%') { *out++ = *p++; *out = 0; continue; }
        i16 base = strlen(out);                  // normally 0 (out points at the NUL)
        p++;
        switch (*p) {
        case '0': cat(out, fmt(rec[4])); break;  // fmt = format_long_thousands((i32)(i8)x)
        case '1': cat(out, fmt(rec[5])); break;
        case '2': cat(out, fmt(rec[0])); break;
        case '3': cat(out, fmt(rec[1])); break;
        case '4': cat(out, fmt(rec[2])); break;
        case '5': cat(out, fmt(rec[3])); break;
        case '9': cat(out, format_long_thousands(rec.spectators)); break;
        case 'a': cat(out, &FAR[0x3066 + (u16)(rec[0x0e] * 0x22)]); break;   // club name
        case 'b': cat(out, &FAR[0x3066 + (u16)(rec[0x0f] * 0x22)]); break;
        case 'c': cat(out, player_name(m, (i8)rec[0x0c])); break;
        case 'd': cat(out, player_name(m, (i8)rec[0x0d])); break;
        case 't': cat(out, &FAR[0x2242 + m * 0x30a]); break;                  // manager name
        case 'e': stop = 1; break;
        case 'x': goto alternatives;
        default: break;                          // unknown code: both chars dropped
        }
        p++;                                     // skip the code char
        out += strlen(out) - base;
        continue;
    alternatives:
        p++; u8 N = *p - '0'; p++;               // p -> M
        u8 s;
        if (N == 0) { u8 M = *p++ - '0'; s = (u8)random_range(1, M); }
        else        { p++;               s = sel[N]; }   // M is ignored
        // p -> first alternative
        for (;;) {
            if (s <= 1) {                         // unsigned: 0 behaves like 1 here
                p = news_expand_template(m, p, out);  // copies the chosen alternative up to '#'
                p++;                              // skip the '#'
                if (--s != 0) continue;           // s==0 before -> 255: runs away (see notes)
                break;
            }
            // skip one alternative, character by character
            do {
                if (p[0] == '%' && p[1] == 'x') { p += 4; while (*p != '%') p++; }
                p++;
            } while (*p != '#');
            p++;                                  // skip '#'
            if (--s == 0) break;                  // never true here (s >= 2 before)
        }
        // skip to the end marker "#%"
        for (;;) {
            if (p[0] == '%' && p[-1] == '#') break;
            if (p[0] == '%' && p[1] == 'x') { p += 4; while (*p != '%') p++; }
            p++;
        }
        p++;                                      // skip the final '%'
        out += strlen(out) - base;
    }
    return p;
}
player_name(m, idx1) = &FAR[0x57dd + (u16)(0x25 * FAR[0x7725 + (i16)(0x34 * ((i16)idx1 + m*25))])]
                     // = player table entry of squad slot idx1-1 (record +0x0f = player id)
```

Notes:

* Grammar: `%xNM` opens a group of `#`-separated alternatives, and the group ends with `#%`.
  `N=0` picks a random alternative from 1..M. `N=1..5` use the selectors of §4.2: 1 venue
  (GEGEN/IN), 2 quality, 3 result, 4 manager mood, 5 deserved/lucky. Groups nest; the
  alternative being copied is expanded recursively.
* Headline 9 contains `%xDEKLASSIERT#...`: `N = 'D'-'0' = 0x14`, so it reads `FAR:57af`
  (inside save block 26, FAR:57aa[5], 0x80 = empty slot). `'E'` is skipped as M. A value of 0 or 1
  gives "KLASSIERT" and 2 gives "]BERROLLT". A larger value (0x80) makes the skip loop run past the
  string, and so does a selector of 0. Both are original defects, so a port must reproduce the
  bytes it would read or define the overrun behaviour. See Open questions.
* `fmt` uses the global `DS:079c` pad width and `DS:07b2` separator flag of
  `format_long_thousands` (rng.md §2.3). `message_post` sets `DS:079c = 1`, so a later `0` prints
  as `0`, not as an empty string.

### 5.2 `newspaper_draw_headline(u8 m, u8 idx)` 3e3a:0dba

```c
ui_set_font(rp, 1);
char buf[100];
char far *rest = news_expand_template(m, DS:4f90[idx], buf);   // stops after %e
ui_draw_text_centered_shadow(rp, buf, 0, 0x13f, 0x29, 0, 0xff);
news_expand_template(m, rest, buf);
ui_draw_text_centered_shadow(rp, buf, 0, 0x13f, 0x33, 0, 0xff);
```

The index < 0x1c test only selects between two selector words that both hold 5cb3.

### 5.3 `newspaper_draw_body(u8 m, u8 count)` 3e3a:0c0b

```c
bool ok = (DS:2252/2254 (i32) == DS:224e/2250 (i32));   // integrity check
if (!ok) count = 8;
news_text_flow(NULL, 0xb4, 0x0d, 0x3c, 1, 0, 0);        // start: x=13, y=60, width 180
char buf[0x1f4];
for (u8 i = 0; i < count; i++) {                          // unsigned byte loop
    if (ok) news_expand_template(m, DS:90d2[(i8)FAR:2e7e[i]], buf);
    else    strcpy(buf, random_range(0, 1) == 0 ? DS:92de : DS:9298);
    if (news_flow_words(buf, 0xb4) > 0x94) i = count;     // page full (u8 of returned y)
}
news_text_flow(NULL, 0xb4, 0, 0, 2, 0, 0);               // flush last line
```

* `DS:92de` = `"xx xxxxxxx xxxx xxx xxx xx xxxxxxxxx xxxx xxxxxx xxx xxxxx xx xxx."`
* `DS:9298` = `"xxxx xxxxxx xx xxxx xxx xxxxxxxxx xxxxx xxxxx xxxx xx xxxxxxx xx xxx."`

DS:2252 is compared with DS:224e in about 40 functions. It looks like a copy-protection or
integrity shadow; see Open questions.

### 5.4 `news_flow_words(char far *s, i16 width)` 3e3a:0d33 → u8

```c
u8 r;                                    // uninitialised if s is empty
while (*s) {
    char w[0x34]; i8 n = 0;
    while (*s && *s != ' ' && *s != (char)0xa0) w[n++] = *s++;
    w[n] = 0;
    r = (u8)news_text_flow(w, width, 0, 0, 0, 0, 0);
    if (*s == (char)0xa0 || *s == ' ') s++;          // exactly one separator
}
return r;
```

### 5.5 `news_text_flow(char far *word, i16 width, i16 x, i16 y, i16 mode, i16 unused, u8 two_col)` 3e3a:06df → i16

State in BSS: `DS:a3a6` line buffer, `DS:a536` x, `DS:a3a4` y, `DS:a538` (i8) word count,
`DS:a504[]` (u8) end offset of every word in the line buffer.

```c
static void draw_line(width, two_col) {
    if (width == 0x127 && two_col) news_draw_label_value_line(DS:a3a6, DS:a536, DS:a3a4);
    else ui_draw_text_at(rp, DS:a3a6, DS:a536, DS:a3a4, 0);
}
i16 news_text_flow(...)
{
    if (mode == 2) {                                        // flush
        n = strlen(line); if (line[n-1] == ',') line[n-1] = 0;   // (n==0 reads DS:a3a5)
        draw_line(width, two_col);
    }
    if (mode != 0) {                                        // 1 = start, 2 = flush+reset
        DS:a536 = x; DS:a3a4 = y; DS:a538 = 0; line[0] = 0;
        return (y & 0xff00);                                // AL = 0
    }
    char tmp[0x15e];
    strcpy(tmp, line); if (tmp[0]) strcat(tmp, " " /*DS:9294*/); strcat(tmp, word);
    if (rastport_text_width(rp, tmp, strlen(tmp)) > width) {   // signed
        i8 i = 0;
        if (DS:a538 != 0) DS:a538--;                       // the last word end is no gap
        while (rastport_text_width(rp, line, strlen(line)) < width) {   // full justification
            u8 pos = DS:a504[i]; i++;
            str_insert_space(line, pos);
            for (i8 j = i; j < DS:a538; j++) DS:a504[j]++;
            if (!(DS:a538 > i)) i = 0;                      // round-robin from the left
        }
        draw_line(width, two_col);
        DS:a3a4 += 7; line[0] = 0; DS:a538 = 0;
    }
    if (line[0]) strcat(line, " " /*DS:9296*/);
    strcat(line, word);
    DS:a504[DS:a538] = (u8)strlen(line);
    DS:a538++;
    return DS:a3a4;                                         // callers use the low byte
}
```

Notes: the trailing comma is only removed on flush, not at a wrap. If the first word of an empty
line is already wider than `width`, the justify loop inserts blanks at a stale `DS:a504[0]`
(original defect). The line height is 7 px.

### 5.6 `str_insert_space(char far *s, i8 pos)` 3e3a:0919

```c
u8 n = (u8)(strlen(s) + 1);
while ((i16)n > (i16)pos) { s[n] = s[n-1]; n--; }
s[pos] = ' ';
```

### 5.7 `news_draw_label_value_line(char far *t, i16 x, i16 y)` 3e3a:060a

```c
i16 i = 0; while (t[i] && t[i] != ':') i++;
if (t[i] == 0) { ui_draw_text_at(rp, t, x, y, 0x0e); return; }
t[i+1] = 0;                                           // overwrites the char after ':'
ui_draw_text_at(rp, t, x, y, 6);                      // "label:"
ui_draw_text_at(rp, t + i + 2, x + rastport_text_width(rp, t, strlen(t)), y, 0x0e);
```

### 5.8 `newspaper_draw_sidebar(u8 m)` 3e3a:000e

Font 2 (set by the caller). Width 0x127 (295) in all calls.

```c
rec = &FAR[0x90ca + m*0x9a];
u8 home_goals = 0, away_goals = 0;                     // [bp-0x7e], [bp-0x80]
if (rec.spectators > 200000) rec.spectators = 0;       // signed long compare
TF(text, mode, x, y, flag) := news_text_flow(text, 0x127, x, y, mode, 0, flag)
TF(NULL, 1, 0x0d, 0xbb, 0);                            // y = 187
buf = club_name(FAR:2260[m*0x30a]) + ": " /*DS:90b7*/;   TF(buf, 0,0,0, 6);
i8 n = squad_count_by_flag(m, 0);
for (u8 i = 0; (i16)n > i; i++) {
    p = player record (m, i);
    if (p[0x0a] == 0 || p[0x0a] >= 0x0c) continue;
    buf = player_name(p[0x0f]) + " (" /*90ba*/ + fmt(grade[m][i]) + ")" /*90bd*/ + "," /*90bf*/;
    TF(buf, 0,0,0, 0x0e);
}
TF(NULL, 2, 0,0, 6);                                   // flush, drops trailing ','
TF(NULL, 1, 0x0d, 0xd1, 6);                            // y = 209
TF("TORE: " /*DS:4bc4 -> 2fc2*/, 0,0,0, 6);
if (rec[0x8b] == 0) TF("FEHLANZEIGE" /*4bc8*/, 0,0,0, 0x0e);
else for (u8 i = 0; i < rec[0x8b]; i++) {
    char name[0x1e] = "";
    if (rec[0x27+i] != 0x64) name = player_name(FAR[0x7759 + 0x34*(25*m + rec[0x27+i])]);
    bool own = rec[0x27+i] != 0x64, home = rec.spectators != 0;
    if (own == home) home_goals++; else away_goals++;  // running score from the home side
    buf = fmt(home_goals) + ":" /*90c1*/ + fmt(away_goals) + " " /*90c3*/ + name
          + " (" /*90c5*/ + fmt(rec[0x13+i]) + ".MIN)" /*90c8*/;
    if (rec[0x8b] - i - 1 != 0) strcat(buf, "," /*90ce*/);
    TF(buf, 0,0,0, 0);
}
TF(NULL, 2, 0,0, 0);
TF(NULL, 1, 0x0d, 0xe6, 6);                            // y = 230
TF("GELBE KARTEN: " /*4bcc*/, 0,0,0, 6);
if (rec[0x94] == 0) TF("KEINE" /*4bd0*/, 0,0,0, 0x0e);
else for (u8 i = 0; i < rec[0x94]; i++) {
    buf = player_name(FAR[0x7759 + 0x34*(25*m + rec[0x8d+i])]);
    if (rec[0x94] - i - 1 != 0) strcat(buf, "," /*90d0*/);
    TF(buf, 0,0,0, 0x0e);
}
TF(NULL, 2, 0,0, 6);
TF(NULL, 1, 0x0d, 0xed, 6);                            // y = 237
TF("ROTE KARTEN  : " /*4bd4*/, 0,0,0, 6);
TF(rec[0x93] == 0 ? "KEINE" : player_name(FAR[0x7725 + 0x34*(25*m + rec[0x93])]), 0,0,0, 0x0e);
TF(NULL, 2, 0,0, 6);
```

The `flag` of the call that triggers a line draw decides between the two-colour and the plain
drawing. Plain lines are drawn with colour 0, which `ui_draw_text_at` maps to 0xFF.

## 6. Messages (segment 4091)

The per-manager inbox is the save-game string table: `DS:0618[m]` (u8, save block 1) holds the
count and `FAR:5524[m*20 + j]` (far char*, rebuilt by the loader) holds the strings, newest at
`j = 0`. `FAR:56e2[m]` (save block 53) is the "new message" flag. `FAR:5358` (save block 4) is the
manager the message is for.

### 6.1 `message_post(u8 show_now, i32 tpl, i32 a0, i32 a1, i32 a2, i32 a3, i32 a4)` 4091:0190 → `char far *`

```c
i32 arg[6] = {a0, a1, a2, a3, a4, tpl};        // '#'/'$' consume arg[0], arg[1], ...
DS:07b2 = 1;                                    // format_long_thousands: no separators
i16 day = (i16)FAR:2eba - random_range(0, 3);   // RNG: message dated 0..3 days back
i16 mlen = (i8)DS:07b8[(i16)FAR:2e8e];          // month lengths 31,28,31,30,31,30,31,31,30,31,30,31
if (mlen < day) day = mlen;
i8 mon = (i8)FAR:2e8e; i16 year = (i16)FAR:a7a0;
if (day < 1) { mon--; if (mon < 0) { mon = 11; year--; } day += (i8)DS:07b8[mon]; }
buf = fmt(day) + ". " /*93b4*/ + DS:23fc[mon] /*month name*/ + " " /*93b7*/;
DS:079c = 1;                                    // pad width 1 (never restored)
buf += fmt(year) + "^" /*93b9*/;
i16 len = strlen(buf); u8 k = 0;
char far *t = DS:267c[(i16)tpl];
for (i16 i = 0; i < strlen(t); i++) {
    char c = t[i];
    if (c == '$')      { strcat(buf, (char far*)arg[k]); len += strlen((char far*)arg[k]); k++; }
    else if (c == '#') { strcat(buf, fmt(arg[k]));       len += strlen(fmt(arg[k]));       k++; }
    else               { buf[len++] = c; buf[len] = 0; }
}
u8 m = FAR:5358;
DS:0618[m]++;
char far *s = mem_alloc(len + 1, 0); strcpy(s, buf);
for (i16 j = DS:0618[m] - 1; j > 0; j--) FAR:5524[m*20 + j] = FAR:5524[m*20 + j - 1];  // no 20-limit
FAR:5524[m*20] = s;
if (show_now) screen_messages(s);
else if ((i8)m < 4) FAR:56e2[m] = 1;
return s;
```

`^` is the line break. Message templates `DS:267c[0..5]`:

| tpl | text |
|---|---|
| 0 | `$ bietet an,^von # auf #^Jahre zu verl{ngern.^` |
| 1 | `$^ist an $^interessiert.^` |
| 2 | `$^$^$ ^` |
| 3 | `$ h{ngt den^Fu~balljob im Alter von^# Jahren an den Nagel.^` |
| 4 | `$ k}ndigt an,^da~ er seinen Vertrag nicht^mehr verl{ngern wird.^` |
| 5 | `$^$^$ ^` |

The month names are `DS:23fc[0..11]`: `Januar, Februar, M{rz, April, Mai, Juni, Juli, August,
September, Oktober, November, Dezember`. Callers: 1000:20e1, 1000:239e, `random_weekly_events`
1cb5:13bd, `season_end_messages` 1cb5:0012.

### 6.2 `message_delete(char far *s)` 4091:0044 → i32 (always 0)

If `s == NULL`, it returns 0. Otherwise it searches `m = 0..DS:07ab-1`, `j = 0..DS:0618[m]-1` (i8
counters) for `FAR:5524[m*20+j] == s`. If not found, it returns 0. If found, it calls
`mem_free(s, strlen(s)+1)`, sets the entry to NULL, moves entries `j+1 .. count-1` down by one,
does `DS:0618[m]--` and returns 0. Callers: 1000:20e1, 1000:239e, 2ecd:10ee,
`contract_negotiation`, `random_weekly_events`, `screen_transfer_market`, `season_end_messages`.

### 6.3 `messages_free_all(void)` 4091:0532

For every manager `m < DS:07ab` and `j < DS:0618[m]`, it calls
`mem_free(FAR:5524[m*20+j], strlen+1)`. It neither clears the pointers nor the counts. Callers:
`FUN_422b_120c` (save loader), `fatal_error_exit`.

### 6.4 `screen_messages(char far *single)` 4091:05c4 (UI screen)

If `single == NULL`, the screen shows the inbox of `FAR:304a`. Otherwise it shows only `single`.

* PIC 2 (`vga_load_picture(2, 0, 0, 0, 1)`). Its first 0x12 columns hold the normal 3-button strip
  and the columns at sx 0x13 hold the pressed images.
* `ui_fill_rect(rp, 0x3b, 0x72, 0xf5, 0xea, 4)`; `ui_draw_frame(rp, 0x39, 0x70, 0x106, 0xea, 7)`.
* 3 bevelled panels, k = 0..2, `d = 41k`: `line(0x3a, 0x71+d, 0xf2, 0x71+d, 1)`,
  `line(0x3a, 0x71+d, 0x3a, 0x97+d, 1)`, `line(0x3b, 0x98+d, 0xf3, 0x98+d, 7)`,
  `line(0xf3, 0x72+d, 0xf3, 0x98+d, 7)`.
* Button strip `blit_bitmap(bm, 0, 0, rp, 0xf5, 0x71, 0x12, 0x7a, 0xc0)` at x=245, y=113, 18x122.
  It holds three 40-px buttons: 0 = scroll up, 1 = exit, 2 = scroll down.
* Font 1, `screen_fade(vp, 1)`.
* Text: every `^`-terminated line is drawn with
  `ui_draw_text_centered_shadow(rp, line, 0x3a, 0xf5, y, first ? 0x19 : 0x18, 0)`. `y` starts at
  0x7a. After each line `y += 8`, plus 2 more after the first (date) line. Text after the last `^`
  is not drawn. After a message, `y += 7`.
* Empty inbox (`DS:0618[mgr] == 0 && single == NULL`): `"Keine Nachricht vorhanden."` (DS:2694),
  drawn centred at y=0xb1 with shadow 1 and colour 0.
* Paging loop (`i` = index of the last message shown, `row` = messages on this page):

```c
if (row == 3 || count - i - 1 == 0 || single) {
  again:
    u8 sel = ui_messages_wait_button();             // 0..2 (1 if the abort flag is set)
    u16 dy = sel * 0x28 + (sel == 2);
    blit_bitmap(bm, 0x13, dy, rp, 0xf5, 0x71 + dy, 0x12, 0x28 + (sel == 2), 0xc0);   // pressed
    do input_poll_mouse(*(far*)FAR:6dc0); while (FAR:2ea2 != 2 && !DS:05d4);
    blit_bitmap(bm, 0, dy, rp, 0xf5, 0x71 + dy, 0x12, 0x28 + (sel == 2), 0xc0);      // released
    if (sel == 0) { if (i > 2) { i -= row + 1; if (i < -1) i = -1; } else goto again; }
    if (sel == 1) i = 0xff;                          // exit
    if (sel == 2) { if (count > i + 1) i -= 2; else goto again; }
    if (i == 0xff) exit;
    for (k = 0; k < 3; k++) ui_fill_rect(rp, 0x3b, 0x72 + 41*k, 0xf0, 0x97 + 41*k, 4);
    y = 0x7a; row = 0;
}
i++; continue while (count > i && single == NULL);
```

* On exit, the background is restored with
  `blit_bitmap_scaled_or_copy(*(far*)FAR:56fa, 0x39, 0x6e, *(far*)FAR:a6ca, 0x39, 0x6e, 0xce, 0x89, 0xc0, 7, 0, 0)`,
  then `bitmap_free(pic)`.
* `count` is always `DS:0618[FAR:304a]`, also in the `single` case.
* Callers: `message_post`, `office_menu_dispatcher`, 18bc:08f2, `season_end_messages`.

### 6.5 `ui_messages_wait_button(void)` 4091:0aaa → i16

```c
if (DS:05d4) return 1;
for (;;) {
    input_poll_mouse(*(far*)FAR:6dc0);
    if (FAR:2ea2 != 1) continue;
    i16 a = ((i16 far*)*(far*)FAR:6dc0)[0], b = ...[1];
    if (a > 0x6b && a < 0xe8 && b > 0xf5 && b < 0x106) return (a - 0x6c) / 0x29;
}
```

Word 0 of the FAR:6dc0 struct is tested against the **vertical** range of the strip and word 1
against the horizontal range. So the struct is `{y, x}`, not `{x, y}` as MEMORY_MAP says. The same
holds for FAR:2e70: `+4` = y and `+6` = x, see §7.

## 7. Dialogs, icon slots, spinner

### 7.1 Icon slots

`DS:07b3` (i8) is the slot count. The slot positions are kept in `FAR:533c[10]` (i16 x) and
`FAR:90ae[10]` (i16 y).

`ui_register_icon_slot(i16 x, i16 y, char far *label, u8 color)` 4091:123d:

```c
u8 save[2][2];
for (r = 0; r < 2; r++) for (c = 0; c < 2; c++)
    save[r][c] = rastport_get_pixel(rp, x + 0x38*r, y + 0x0b*c);
n = DS:07b3; FAR:533c[n] = x; FAR:90ae[n] = y; if ((i8)n < 10) DS:07b3++;  // slot 10 written once
ui_fill_rect(rp, x, y, x+0x38, y+0x0b, 0);
ui_draw_line(rp, x, y, x+0x38, y, 2);                 // top
ui_draw_line(rp, x+0x38, y, x+0x38, y+0x0b, 7);       // right
ui_draw_line(rp, x, y, x, y+0x0b, 1);                 // left
ui_draw_line(rp, x, y+0x0b, x+0x38, y+0x0b, 6);       // bottom
for (r..) for (c..) { rastport_set_color(rp, save[r][c]); rastport_put_pixel(rp, x+0x38*r, y+0x0b*c); }
ui_set_font(rp, 2);
ui_draw_text_centered_shadow(rp, label, x+1, x+0x39, y+8, color, 0xff);
```

The y arguments of `ui_draw_line` are pushed as bytes (only AL is valid), so y is effectively
`u8`.

`ui_hit_icon_slot(void)` 4091:11cc → i16: with `mx = vp[+6]`, `my = vp[+4]`, it returns `i+1` for
the first `i < DS:07b3` with `FAR:533c[i] < mx < FAR:533c[i]+0x39` and
`FAR:90ae[i] < my < FAR:90ae[i]+0x0c`, and otherwise 0.

### 7.2 `dialog_okay(char far *l1, l2, l3, l4)` 4091:0e3a → 1

```c
u8 b = ui_draw_button(rp, 0x3c, 0x6e, 0xb9, 0x41, save, 1, 0x10);
ui_set_font(rp, 1);
for (k = 0; k < 4; k++) ui_draw_text_centered_shadow(rp, l[k], 0x3c, 0xf5, 0x7b + 10*k, 0x0a, 0xff);
DS:07b3 = 0;
ui_register_icon_slot(0xb2, 0xa0, DS:2698 /*"OKAY"*/, 0x1a);
save mouse (y = w[0], x = w[1]); mouse_set_position(0xbf, 0xa5);
screen_fade(vp, 1);
do input_poll_mouse(..); while (!(FAR:2ea2 == 1 && ui_hit_icon_slot() != 0) && !DS:05d4);
mouse_set_position(saved x, saved y);
ui_redraw_buttons(b, 0);
return 1;
```

The keyboard is not handled; only a click on OKAY or the abort flag DS:05d4 ends the dialog.

### 7.3 `dialog_two_choice(i8 who, char far *l1, l2, l3, char far *left, char far *right)` 4091:146c → i8

* `ui_draw_button(rp, 0x4b, 0x6e, 0x9b, 0x41, save, 1, 0x10)`, then font 1.
* Title: if `who < 5`, the manager name `FAR:2242 + who*0x30a`, otherwise `"An Alle:"` (DS:269c).
  It is drawn centred at x 0x4f..0xe6, y 0x77, shadow 0x0b, colour 0xff.
* `line(0x4d, 0x79, 0xe4, 0x79, 0x0b)`, then font 2.
* `l1`, `l2` and `l3` at y 0x83, 0x8b and 0x93 (x 0x4f..0xe6, shadow 0x0a).
* `line(0x4d, 0x9b, 0xe4, 0x9b, 0x0b)`.
* `ui_register_icon_slot(0x59, 0xa0, left, 0x11)`; `ui_register_icon_slot(0x9f, 0xa0, right, 1)`;
  then `DS:07b3 -= 2` (the slots are drawn but not registered).
* Mouse to (0xb9, 0xa5). Loop: on event 1, for k = 0,1: if
  `0x59+0x46k < x < 0x92+0x46k && 0x9f < y < 0xac`, then `res = 2 - k`. The loop ends when
  `res != 0` or DS:05d4 is set.
* Restore the mouse, `ui_redraw_buttons(b, 0)`, return `res - 1` (left = 1, right = 0, abort = -1).

Callers: 1000:272d, 18bc:1c1d, 422b:07fe, 422b:2bbe, `cup_draw_screen_open`,
`screen_transfer_market`.

### 7.4 `ui_wait_right_click_button(i16 a, i16 b)` 4091:1722

```c
ui_draw_button(rp, a, b, 0, 0xa022, save, 0, 8);
do input_poll_mouse(..); while (FAR:2ea2 != 2 && !DS:05d4);
return ui_draw_button(rp, a, b, 0, 0x6022, save, 0, 8);
```

It has 18 callers (screens that wait for "continue").

### 7.5 `ui_value_spinner(i32 v, i16 x, i16 y, i32 lo, i32 hi, i32 step)` 4091:0ba5 → i32

```c
i32 v0 = v;
if (DS:224e (i32) > 1 && v != 0) {               // see rng.md §2.4
    if (lo % 5) lo = lo / 5 * 5;
    if (hi % 5) hi = hi / 5 * 5;
}
pic = vga_load_picture(0x15, 0, 0, 0, 1);        // PIC\21.VGA spinner frame
blit_bitmap(*pic, 0, 0, rp, x, y, 0x46, 0x27, 0xc0);
ui_set_font(rp, 1); DS:07b2 = 0;
ui_spinner_draw_value(v, v0, x, y - 2);
do {
    input_poll_mouse(..); my = w[0]; mx = w[1];
    u8 pix = rastport_get_pixel(rp, mx, my);
    if (FAR:2ea2 == 1 && x < mx && mx < x+0x46 && y < my && my < y+0x27 && pix == 4) {
        if (my > y + 0x12) v -= step; else v += step;   // lower half decrements
        if (v < lo) v = lo;                               // no redraw on clamp
        else if (v > hi) v = hi;                          // no redraw on clamp
        else { ui_fill_rect(rp, x+2, y+0x0f, x+0x43, y+0x17, 0); ui_spinner_draw_value(v, v0, x, y-2); }
    }
} while (FAR:2ea2 != 3);
bitmap_free(pic);
return v == v0 ? -1L : v;
```

`ui_spinner_draw_value(i32 v, i32 v0, i16 x, i16 y)` 4091:0b1b: `buf = fmt(v)`. If
`DS:224e (i32) > 1 && v0 != 0`, it rewrites `buf[2] = buf[1]; buf[1] = '.'; buf[3] = '0';
buf[4] = 0` ("12" becomes "1.20", "5" becomes "5."). It then draws
`ui_draw_text_centered_shadow(rp, buf, x, x+0x46, y+0x18, 9, 0)`. Callers of the spinner: 1000:0602
(stadium), `FUN_422b_120c`.

### 7.6 Small helpers

* `team_slot_free_index(u8 t)` 4091:0000 → u8: the first `i < 10` with `FAR:1d34[t*10 + i] == 0`,
  or 0x7f if there is none. FAR:1d34 is save block 44 (3 x 10 bytes). Callers: 1000:239e,
  `random_weekly_events`.
* `squad_count_by_flag(u8 t, u8 mode)` 4091:1109 → u8: `n = (t == 4) ? 12 : 24`. For `i < n` it
  counts, as independent ifs: mode 0 → player `+0x0f` (id) != 0; mode 1 → `0 < +0x0a < 0x0c`;
  mode 2 → `+0x0a > 0x0b` (unsigned). The record is at FAR:774a + 0x34*(25t+i). Slot 24 is never
  examined. It has 41 callers.
* `find_player_by_id(u8 id, i8 far *out_sq, i8 far *out_i)` 4091:17b7 → far ptr: for `s = 0..4`
  with `s < DS:07ab || s == 4`, it sets `n = squad_count_by_flag(s, 0)`. For `i < n` (i8 compare),
  if record `+0x0f == id`, it stores `*out_sq = s; *out_i = i` and returns the record. If nothing
  matches, it sets `*out_sq = 0xff` and returns NULL (`*out_i` is untouched). Callers:
  `season_end_messages` (2).
* `ui_draw_manager_header(void)` 4091:185c (`mg = FAR:304a`):
  * `ui_draw_button(rp, 0x109, 5, 0x2a, 0x2a, save, 0, 8)`;
    `blit_bitmap(*(far*)FAR:2f24, 0x29*mg, 0, rp, 0x10a, 6, 0x28, 0x28, 0xc0)` (portrait).
  * Font 1. `ui_draw_text_centered_shadow(rp, FAR:2242 + mg*0x30a, 0x113, 0x127, 0x39, 0, 1)`.
  * `ui_draw_button(rp, 0x109, 0x3c, 0x2a, 0x29, save, 0, 0)`;
    `blit_bitmap(*(far*)FAR:5354, 0x29*mg, 0, rp, 0x10b, 0x3e, 0x28, 0x26, 0xc0)` (badge).
  * `0x29*mg` is an 8-bit signed IMUL. Callers: 1000:20e1, 212a:0a6d, 24a4:260a.
* `clamp_int`, `clamp_char`, `text_to_upper_de` and `long_abs_diff`: see rng.md §1. Note that
  `long_abs_diff` returns the full long, and this area uses DX in its compares.

## 8. Data tables

| Address | Type | Content |
|---|---|---|
| DS:90d2 | far char*[110] | body templates (Appendix A.1) |
| DS:9350 | i16[25] | body group sizes: 8,5,4,8,5,7,4,7,8,10,4,4,7,6,6,5,1,1,2,7,3,2,2,3,4 |
| DS:4f90 | far char*[69] | headline templates (Appendix A.2), part of the 4a68 label table |
| DS:9322 | i16[23] | headline group sizes: 4,2,3,4,2,3,1,2,4,3,2,2,7,4,5,3,3,3,3,3,2,2,2 |
| DS:937e | i16[11] | heat weights 3,4,5,7,9,10,9,7,5,4,3 (followed by unrelated words 0, 2184, 1092 ...) |
| DS:9394 | u16[16] | newspaper palette (0x0RGB): 000 888 444 ccc 222 aaa 666 eee 111 999 555 ddd 333 bbb 777 fff |
| DS:9298 / 92de | char[] | "xxxx ..." filler lines (§5.3) |
| DS:9294, 9296 | `" "` | word separators |
| DS:90b7.. | char[] | `": "`, `" ("`, `")"`, `","`, `":"`, `" "`, `" ("`, `".MIN)"`, `","`, `","` at 90b7, 90ba, 90bd, 90bf, 90c1, 90c3, 90c5, 90c8, 90ce, 90d0 |
| DS:4bc4..4bd7 | far char*[5] | `"TORE: "`, `"FEHLANZEIGE"`, `"GELBE KARTEN: "`, `"KEINE"`, `"ROTE KARTEN  : "` |
| DS:267c | far char*[] | UI strings: [0..5] message templates (§6.1), [6] `"Keine Nachricht vorhanden."`, [7] `"OKAY"`, [8] `"An Alle:"` |
| DS:23fc | far char*[12] | month names |
| DS:07b8 | i8[12] | month lengths (February is always 28) |
| DS:93b4/93b7/93b9 | char[] | `". "`, `" "`, `"^"` |

## 9. Globals used

| Address | Size/type | Suggested name | Save block |
|---|---|---|---|
| FAR:90ca | 4 x 0x9a bytes | `news_match_record[4]` (§2) | – |
| FAR:579b..57a0 | u8[6] | `news_selector[]` (1 venue, 2 quality, 3 result, 4 mood, 5 deserved) | – |
| FAR:57af | u8 | read through the `%xD` defect | block 26 (FAR:57aa[5]) |
| FAR:2e7e | u8[~16] | `news_template_list` | – |
| FAR:4d3c | u8 at stride 8, 32 per manager | `player_match_grade[m][i]` | – |
| FAR:1d14 | u8[4] | `manager_played_today` | – |
| FAR:5358 | u8 | `message_manager` / current manager for grading | block 4 |
| FAR:774a | 0x34 x 25 x 5 | player squad records (+0x09, +0x0a, +0x0d, +0x0f, +0x10..+0x15) | block 18 |
| FAR:57dd | 0x25 x 151 | player table (name at +0, flag at +0x1f = FAR:57fc) | block 17 |
| FAR:3066 | 0x22 x 200 | club table (name) | block 15 |
| FAR:2242 | 0x30a x 4 | manager record (+0 name, +0x1e club = FAR:2260) | block 13 |
| FAR:2eba / 2e8e / a7a0 | i32 / i32 / i32 | day, month, year | blocks 20 / 21 / 22 |
| FAR:304a | u8 | current manager | block 3 |
| FAR:5524 | far ptr[4][20] | message strings | string table |
| FAR:56e2 | u8[4] | `new_message_flag[m]` | block 53 |
| FAR:533c / FAR:90ae | i16[10] / i16[10] | icon slot x / y | – |
| FAR:1d34 | u8[3][10] | slot rows for `team_slot_free_index` | block 44 |
| FAR:2e70 | far ptr | viewport/mouse state (+4 y, +6 x, +8 colormap) | – |
| FAR:6dc0 | far ptr | mouse position `{y, x}` | – |
| FAR:2ea2 | i16 | last mouse event (1 left, 2, 3 = spinner exit) | – |
| FAR:2f28 / 2f24 / 5354 / 56fa / a6ca / a6ce | far ptr / far ptr / far ptr / far ptr / far ptr / i16 | rastport, portrait bitmap, badge bitmap, work bitmap, photo/background target, photo width | – |
| DS:060b | u8 | `option_newspaper` | block 36 (DS:05fe[13]) |
| DS:07ab | u8 | number of human managers | block 7 |
| DS:0618 | u8[4] | messages per manager | block 1 |
| DS:07b2 | u8 | `fmt_no_separators` | – |
| DS:079c | i8 | `fmt_pad_width` | – |
| DS:07b3 | i8 | `icon_slot_count` | – |
| DS:05d4 | u8 | abort/quit flag | – |
| DS:224e / DS:2252 | i32 / i32 | game option and its integrity shadow | – |
| DS:0794 | far ptr | game palette | – |
| DS:a3a4, a3a6, a504, a536, a538 | i16, char[0x15e], u8[50], i16, i8 | text-flow state (BSS) | – |

## 10. Open questions

1. The `%xD` code in headline 9 reads selector `FAR:57af`. Typical saves have 0x80 there, which
   makes the alternative skip run past the template into following DGROUP memory. Port option: copy
   the exact DGROUP bytes behind DS:3f91, or treat it as "first alternative". An emulator check is
   needed.
2. Headline groups k10 and k11 have conditions that look swapped relative to their texts. The spec
   keeps the original behaviour.
3. Meaning of DS:2252 versus DS:224e. The body prints "xxx" filler when they differ. It looks like an
   anti-tamper shadow, and its writer is not traced here.
4. Exact semantics of `FUN_1310_1bd7` and `FUN_1310_1cd3` (grade inputs) and of player fields
   +0x10..+0x15 belong to the match agent. The meaning of `FAR:57fc` (player table +0x1f, the hero
   requirement) is unknown.
5. `FAR:a6ce` used as the photo width, and `FAR:a6ca` used as the blit target, need confirmation
   by the VGA/UI agent.
6. The mouse event code 2 waited for in `screen_messages`/`ui_wait_right_click_button`, and code 3
   that ends the spinner, are not decoded (`input_poll_mouse` documents 1 = left and 2 = right).
7. MEMORY_MAP lists the mouse structs as `{x, y}`. This area's code shows `{y, x}` for FAR:6dc0 and
   `+4 = y, +6 = x` for FAR:2e70. The UI agent should verify.
8. 2a58:20b2 stores `spectators + 200000` when its `[bp-0x24] == 2` (neutral venue/cup?). The
   newspaper analysis then counts the match as a home game, but the sidebar resets the value to 0.

## Appendix A. Template texts (raw bytes)

### A.1 Body templates `DS:90d2` (110 far pointers) grouped by `DS:9350`

| idx | group | DS addr | text (raw bytes) |
|---|---|---|---|
| 0 | 0 | 5558 | `EIN %x20LANGWEILIGES#M[SSIGES#GUTES#TOLLES#% SPIEL %a %x10GEGEN#BEI#% %b ENDETE %0:%1.` |
| 1 | 0 | 55af | `MIT %x30EINEM UNENTSCHIEDEN#EINEM SIEG#EINER NIEDERLAGE#% ENDETE DAS %x20M]DE#M[SSIGE#RECHT ANSPRECHENDE#BEGEISTERNDE#% SPIEL %a %x10GEGEN#IN#% %b.` |
| 2 | 0 | 5643 | `EIN %0:%1 SAHEN DIE ZUSCHAUER IM SPIEL %a %x10GEGEN#IN#% %b.` |
| 3 | 0 | 5680 | `%0:%1 ENDETE DIE %x20EINSCHL[FERNDE#SCHWACHE#ORDENTLICHE#SEHR GUTE#% PARTIE %a %x10GEGEN#IN#% %b.` |
| 4 | 0 | 56e2 | `ZU %x30EINEM UNENTSCHIEDEN#EINEM SIEG#EINER NIEDERLAGE#% KAM %a IM SPIEL %x10GEGEN#IN#% %b.` |
| 5 | 0 | 573e | `DIE %x20SEHR SCHWACHE#MITTELM[SSIGE#RECHT ANSPRECHENDE#GUTE#% PARTIE %a %x10GEGEN#IN#% %b ENDETE MIT %x30EINEM UNENTSCHIEDEN#EINEM SIEG#EINER NIEDERLAGE#% F]R %t'S MANNSCHAFT.` |
| 6 | 0 | 57ee | `IN EINER %x20EINSCHL[FERNDEN#DURCHSCHNITTLICHEN#DURCHWACHSENEN#MITREISSENDEN#% PARTIE %x30SPIELTE#SIEGTE#UNTERLAG#% %a %x10GEGEN#BEI#% %b MIT %0:%1.` |
| 7 | 0 | 5883 | `DIE %x20LANGWEILIGE#NICHT SEHR ANSEHNLICHE#RECHT GUTE#WIRKLICH ANSPRECHENDE#% PARTIE ZWISCHEN %a %x10GEGEN#BEI#% %b ENDETE %0:%1 %x30UNENTSCHIEDEN#F]R %a#GEGEN %a#%.` |
| 8 | 1 | 5929 | `%9 ZUSCHAUER %x30SAHEN#FEIERTEN#BETRAUERTEN#% IHRE MANNSCHAFT.` |
| 9 | 1 | 5968 | `%9 ZUSCHAUER VERLIESSEN DAS STADION %x30MIT GEMISCHTEN GEF]HLEN#BEGEISTERT VON IHREM TEAM#ENTT[USCHT VON IHRER MANNSCHAFT#%.` |
| 10 | 1 | 59e5 | `DIE %9 ZUSCHAUER ,SOWEIT SIE FANS VON %a WAREN, %x30SAHEN WENIG ANGETAN#WAREN BEEINDRUCKT#MUSSTEN ENTT[USCHT ERLEBEN#%, WIE IHRE MANNSCHAFT SPIELTE.` |
| 11 | 1 | 5a7a | `DIE OFFIZIELLE ZUSCHAUERZAHL LAG NACH ANGABEN DER STADIONLEITUNG BEI %9.` |
| 12 | 1 | 5ac3 | `DIE %9 FANS IM STADION %x30SAHEN EINE %x40SCHWACHE#RECHT GUTE#% VORSTELLUNG#FEIERTEN DEN %x50VERDIENTEN#ETWAS GL]CKLICHEN#% ERFOLG#BETRAUERTEN DIE %x50LEISTUNGSGERECHTE#UNGL]CKLICHE#% NIEDERLAGE#% IHRER MANNSCHAFT.` |
| 13 | 2 | 5b9a | `DIE MITGEREISTEN SCHLACHTENBUMMLER WAREN %x30UNZUFRIEDEN#BEGEISTERT#ENTT[USCHT#%.` |
| 14 | 2 | 5bec | `DIE MITGEREISTEN FANS %x30VERHIELTEN SICH RUHIG#FEIERTEN NOCH LANGE NACH DEM SCHLUSSPFIFF#VERLIESSEN DAS STADION SEHR SCHNELL#%.` |
| 15 | 2 | 5c6d | `DIE ANGEREISTEN FANS %x30WUSSTEN NICHT, OB SIE MIT DEM UNENTSCHIEDEN ZUFRIEDEN SEIN SOLLTEN ODER NICHT#ERLEBTEN EINE ANSPRECHENDE LEISTUNG IHRER MANNSCHAFT, UND FEIERTEN DEN SIEG ]BERSCHW[NGLICH#WAREN ENTT[USCHT ]BER DEN AUSGANG DER PARTIE#%.` |
| 16 | 2 | 5d60 | `DIE MITGEREISTEN FANS IM STADION %x30SAHEN EINE %x40SCHWACHE#RECHT GUTE#% VORSTELLUNG#FEIERTEN DEN %x50DURCHAUS VERDIENTEN#ETWAS GL]CKLICHEN#% ERFOLG#BETRAUERTEN DIE %x50LEISTUNGSGERECHTE#UNGL]CKLICHE#% NIEDERLAGE#% IHRER MANNSCHAFT BEI %b.` |
| 17 | 3 | 5e51 | `DIE HARTE SPIELWEISE DER MANNSCHAFTEN LIESS KEIN TECHNISCH SCH\|NES SPIEL ZU.` |
| 18 | 3 | 5e9e | `BEIDE MANNSCHAFTEN GINGEN SEHR HART, TEILWEISE UNFAIR ZUGANGE.` |
| 19 | 3 | 5edd | `DAS ]BERAUS K\|RPERBETONTE, HARTE SPIEL DER TEAMS LIESS FL]SSIGE KOMBINATIONEN NICHT ZU.` |
| 20 | 3 | 5f35 | `BEIDE TEAMS GINGEN HOCHMOTIVIERT IN DIESES SPIEL. BESONDERS AUFFALLEND WAR DAS ]BERHARTE SPIEL DER MANNSCHAFT %a.` |
| 21 | 3 | 5fa7 | `DAS SPIEL WAR GEPR[GT VON VIELEN KLEINEN FOULS, DER UNPARTEISCHE HATTE TEILWEISE ALLE M]HE, DIE KONTROLLE ZU BEHALTEN.` |
| 22 | 3 | 601e | `F]R DEN UNPARTEIISCHEN WAR DAS SPIEL EIN HARTES ST]CK ARBEIT, DA BEIDE TEAMS OFT ]BER DIE GRENZEN DES ERLAUBTEN HINAUS GINGEN.` |
| 23 | 3 | 609d | `DER ]BERHARTE EINSATZ BEIDER MANNSCHAFTEN MU~TE VOM UNPARTEIISCHEN ALLZU OFT UNTERBROCHEN WERDEN.` |
| 24 | 3 | 60ff | `DIE SPIELER %x03GINGEN MIT ENTSCHIEDEN ZUVIEL EINSATZ ZUR SACHE#BESIEGTEN %b MIT SEHR HARTEM EINSATZ#KONNTEN DIE NIEDERLAGE AUCH NICHT DURCH DEN TEILWEISE ERSCHRECKEND HARTEN EINSATZ ABWENDEN#%.` |
| 25 | 4 | 61c2 | `DAS GESAMTE SPIEL VERLIEF [USSERST FAIR.` |
| 26 | 4 | 61eb | `BEIDE MANNSCHAFTEN BEM]HTEN SICH UM EIN FAIRES SPIEL.` |
| 27 | 4 | 6221 | `%a FIEL DURCH BERSONDERS FAIRE, SAUBERE SPIELWEISE AUF.` |
| 28 | 4 | 6259 | `DIE ZUSCHAUER SAHEN [USSERST FAIREN FUSSBALL BEIDER MANNSCHAFTEN.` |
| 29 | 4 | 629b | `BERSONDERS LOBENSWERT IST DIE SEHR FAIRE SPIELWEISE BESONDERS VON %a, DIE %x30IHREN EHRGEIZ NICHT MIT H[RTE VERWECHSELTEN#SICH BEI IHREM SIEG AUF SPIELERISCHE QUALIT[TEN BESANNEN#TROTZ DER NIEDERLAGE DIE NERVEN WAHRTEN#%.` |
| 30 | 5 | 6379 | `%t %x02ZEIGTE SICH#WAR#% NACH DEM SPIEL %x40UNZUFRIEDEN#ZUFRIEDEN#% MIT DEM ERGEBNIS UND DEM SPIEL SEINER MANNSCHAFT.` |
| 31 | 5 | 63ef | `%t %x40WOLLTE SEINE%x03N UNMUT#N ZORN# WUT#% NICHT VERBERGEN#ZEIGTE SICHTLICHE ZUFRIEDENHEIT ]BER DAS %x02ERGEBNIS#SPIEL#%#%.` |
| 32 | 5 | 646d | `IN EINEM %x02INTERVIEW#GESPR[CH#% NACH %x02DEM SPIEL#DER PARTIE#% %x03MEINTE#SAGTE#ERL[UTERTE#% %t:%x40'ICH WERDE MICH MIT EINIGEN SPIELERN EINMAL UNTER VIER AUGEN UNTERHALTEN M]SSEN#ICH KANN EIGENTLICH ZUFRIEDEN SEIN MIT %x02DER LEISTUNG#DEM SPIEL#% MEINER %x03MANNSCHAFT#JUNGS#M[NNER#%#%.'` |
| 33 | 5 | 6591 | `'%x40EINIGE SPIELER SCHIENEN NICHT MIT DER RICHTIGEN EINSTELLUNG AUF DEM PLATZ GEWESEN ZU SEIN#MAN KONNTE SEHEN, DASS MEINE MANNSCHAFT DURCHAUS GUTEN FUSSBALL SPIELEN KANN#%' %x02MEINTE#KOMMENTIERTE#% %t NACH DEM SPIEL.` |
| 34 | 5 | 666d | `%t %x02SAGTE#MEINTE#% NACH DEM SPIEL: %x40IN MEINER MANNSCHAFT WIRD SICH EINIGES [NDERN M]SSEN. AUF UNS WARTET WOHL NOCH VIEL ARBEIT#IN DIESER FORM MACHT ES SPASS, DER MANNSCHAFT ZUZUSEHEN, OBWOHL HIER UND DORT NOCH VERBESSERUNGEN DRIN SIND#%.'` |
| 35 | 5 | 6762 | `KURZ NACH DEM SPIEL MEINTE %t: '%x40DAS WAR EIN TROSTLOSES SPIEL MEINER MANNSCHAFT, PERSONELLE VER[NDERUNGEN KANN ICH AUFGRUND SOLCH SCHWACHER LEISTUNGEN EINIGER SPIELER NICHT AUSSCHLIESSEN#DIE MANNSCHAFT HAT MEINE TAKTISCHEN ANWEISUNGEN SEHR GUT UMGESETZT UND GEZEIGT, DASS NOCH VIEL VON IHR ZU ERWARTEN IST#%.'` |
| 36 | 5 | 689b | `%t WAR VOM SPIEL SEINER MANNSCHAFT %x40SEHR ENTT[USCHT. 'ES WAR KEIN GROSSES AUFB[UMEN ZU VERSP]REN.#ANGETAN. 'EINIGES IST NOCH ZU VERBESSERN, ABER WENN WIR WEITER SO SPIELEN, SEHE ICH POSITIV IN DIE ZUKUNFT#% ', KOMMENTIERTE ER DAS SPIEL.` |
| 37 | 6 | 698b | `DER SCHATZMEISTER FREUTE SICH ]BER DAS AUSVERKAUFTE STADION.` |
| 38 | 6 | 69c8 | `DIE AUSVERKAUFTEN R[NGE LASSEN GELD IN DIE KASSEN VON %a FLIESSEN.` |
| 39 | 6 | 6a0b | `DER ANBLICK DER VOLLEN R[NGE IM AUSVERKAUFTEN STADION ERFREUTE DENSCHATZMEISTER.` |
| 40 | 6 | 6a5c | `DER ANBLICK DES BIS AUF DIE LETZTEN PL[TZE GEF]LLTEN STADIONS MUSS DEMSCHATZMEISTER VON %a FREUDENTR[NEN IN DIE AUGEN GETRIEBEN HABEN.` |
| 41 | 7 | 6ae3 | `DIE MEISTEN BESUCHER WAREN SICH EINIG, DASS DIESER SIEG %x50ABSOLUT GERECHTFERTIGT#VOLLKOMMEN UNVERDIENT#% WAR.` |
| 42 | 7 | 6b53 | `DER SIEG VON %a WAR %x50VERDIENT#WIRKLICH NICHT VERDIENT#%.` |
| 43 | 7 | 6b8f | `AUCH EIN NEUTRALER BEOBACHTER DER PARTIE MUSS ZU DEM SCHLUSS KOMMEN, DASS DAS RESULTAT %x50MIT DEN GEZEIGTEN LEISTUNGEN ]BEREINSTIMMT#WOHL SEHR SCHMEICHELHAFT F]R DEN SIEGER IST#%.` |
| 44 | 7 | 6c44 | `NACH DEM SPIELVERLAUF IST DAS %0:%1 ALS %x50DEN LEISTUNGEN BEIDER TEAMS ENTSPRECHEND#SEHR GL]CKLICH F]R DEN SIEGER#% ZU BEZEICHNEN.` |
| 45 | 7 | 6cc8 | `ALLE WAREN SICH EINIG, DASS DIE PARTIE MIT %a EINEN %x50VERDIENTEN#GL]CKLICHEN#% SIEGER GEFUNDEN HAT.` |
| 46 | 7 | 6d2e | `DIE BESUCHER %x02DER PARTIE#DES SPIELS#% WAREN %x02EINER MEINUNG#SICH EINIG#%, DASS DER %x02TRIUMPH#SIEG#% VON DER %x02KLAR#WESENTLICH#EINDEUTIG#% %x50BESSEREN#GL]CKLICHEREN#% MANNSCHAFT %x03ERZIELT#ERRUNGEN#ERREICHT#% WURDE.` |
| 47 | 7 | 6e10 | `%a WURDE VON %x02DEN ZUSCHAUERN#ALLEN BESUCHERN#% ALS %x50DER VERDIENTE#GL]CKLICHER#% %x30x#SIEGER#VERLIERER#% DIESER PARTIE %x50AKZEPTIERT#ANGESEHEN#%.` |
| 48 | 8 | 6ea9 | `DIE HAUSHOHE ]BERLEGENHEIT DER %x10GASTGEBER#G[STE#% WAR FAST W[HREND DES GESAMTEN SPIELS NICHT ZU ]BERSEHEN.` |
| 49 | 8 | 6f17 | `%a WAR%x03 DEM GEGNER#DER GEGNERISCHEN MANNSCHAFT# %b#% FAST ]BER DIE GESAMTE SPIEL%x02DAUER#ZEIT#% IN ALLEN %x02BELANGEN#BEREICHEN#% ]BERLEGEN.` |
| 50 | 8 | 6fa8 | `DIE %x10PLATZHERREN#G[STE#% WAREN IN TOLLER %x02VERFASSUNG#SPIELLAUNE#% UND %x02BEHERRSCHTEN DEN GEGNER#BESTIMMTEN DAS SPIEL#% %x02SEHR SOUVER[N#FAST NACH BELIEBEN#%.` |
| 51 | 8 | 704f | `DIE %x02]BERDURCHSCHNITLICH GUTEN#ANSPRECHENDEN#% LEISTUNGEN %x02W[HREND DES GESAMTEN SPIELS#]BER DIE GESAMTE SPIELZEIT HINWEG#% MACHTEN %a ZUM VERDIENTEN SIEGER ]BER %b, DIE DEM %x03STURMLAUF#ANSTURM#ELAN#% DER %x10GASTGEBER#G[STE#% %x02SO GUT WIE#FAST#% NICHTS ENTGEGENZU%x02SETZEN#HALTEN#% HATTEN.` |
| 52 | 8 | 717c | `DEM %x03]BERLEGEN#DURCHDACHTEN#GERADLINIG#% UND %x03SOUVER[N#]BERZEUGEND#SCHNELL#% GEF]HRTEN SPIEL DER %x10PLATZHERREN#G[STE#% HATTEN DIE %x10G[STE#HAUSHERREN#% IN KEINER PHASE DES SPIELS ETWAS ENTGEGENZUSETZEN.` |
| 53 | 8 | 7250 | `DEN SPIELERN VON %b WURDEN DURCH %a IHRE GRENZEN %x02GANZ KLAR#SEHR DEUTLICH#% AUFGEZEIGT. IN %x02VIELEN#EINIGEN#% PHASEN WURDEN DIE %x10G[STE#PLATZHERREN#% VORGEF]HRT WIE IM TRAINING.` |
| 54 | 8 | 7309 | `DER SIEG VON %a WAR DAS %x02ERGEBNIS#RESULTAT#% EINER %x03TOLLEN#SEHR GUTEN#HOMOGENEN#% MANNSCHAFTSLEISTUNG.` |
| 55 | 8 | 7376 | ` %a WAR %x02DER GEGNERISCHEN MANNSCHAFT#DEM GEGNER#% IN PUNKTO KAMPF UND SPIELWITZ %x03UM L[NGEN ]BERLEGEN#EINIGES VORAUS#KLAR IM VORTEIL#%.` |
| 56 | 9 | 7403 | `%a WAR DEM ]BERM[CHTIGEN GEGNER %b AN DIESEM TAG IN JEDER PHASE DES SPIELS UNTERLEGEN.` |
| 57 | 9 | 745a | `DIE %x10G[STE#GASTGEBER#% SCHN]RTEN %a FAST ]BER DIE GESAMTE SPIELZEIT IM DER EIGENEN H[LFTE EIN UND ERSPIELTEN SICH EINE CHANCE NACH DER ANDEREN.` |
| 58 | 9 | 74ed | `AM HEUTIGEN TAG HATTE DER %x10GASTGEBER#GAST#% SEINEM GEGNER NICHTS ENTGEGEN ZU SETZEN.` |
| 59 | 9 | 7545 | `VON DER MANNSCHAFT VON %t HATTE DER GEGNER WEDER IN SPIELERISCHER NOCH IN K[MPFERISCHER ART ERNSTHAFT ETWAS ZU BEF]RCHTEN. ZU GROSS WAREN DIE LEISTUNGSUNTERSCHIEDE AM HEUTIGEN TAG.` |
| 60 | 9 | 75fa | `DIE SPIELER VON %t SCHLICHEN NACH DEM SPIEL VOM PLATZ. MAN WAR SICH EINIG, IM HEUTIGEN SPIEL DEM %x10GAST#GASTGEBER#% IN ALLEN PUNKTEN UNTERLEGEN GEWESEN ZU SEIN.` |
| 61 | 9 | 769d | `GROSSE LEISTUNGSUNTERSCHIEDE WAREN AUCH F]R DIE FANS VON %a NICHT ZU ]BERSEHEN, SO DASS SELBST DIE TREUESTEN ANH[NGER DIESE NIEDERLAGE ALS LEISTUNGSGERECHT ANSEHEN MUSSTEN.` |
| 62 | 9 | 774a | `%x02KEINER#NIEMAND#% H[TTE SICH BE%x02KLAGEN#SCHWEREN#% D]RFEN, WENN DIE NIEDERLAGE GEGEN %b NOCH %x02KLARER#DEUTLICHER#% AUSGEFALLEN W[RE.` |
| 63 | 9 | 77d6 | `DIE %x02EXTREMEN#GROSSEN#% LEISTUNGSUNTERSCHIEDE %x02VON %a UND %b#BEIDER MANNSCHAFTEN#% WAREN %x02URSACHE#AUSSCHLAGGEBEND#% F]R DIE HEUTIGE NIEDERLAGE.` |
| 64 | 9 | 786f | `FUSSBALLEXPERTEN %x02DIE DAS SPIEL SAHEN#UNTER DEN ZUSCHAUERN#% WAREN %x02EINER MEINUNG#SICH EINIG#%, DASS %x02DIE#DIESE#% NIEDERLAGE GEGEN %a %x02ABSOLUT#KLAR#% %x02GERECHTFERTIGT#VERDIENT#% WAR.` |
| 65 | 9 | 7934 | `%x02%b#DER GEGNER#% WAR %x02WESENTLICH ST[RKER#]BERM[CHTIG STARK#% UND %x02IN JEDER PHASE#ZU JEDER ZEIT#% DIE %x03EINDEUTIG#DEUTLICH#KLAR#% %x03BESSERE#SPIELFREUDIGERE#DOMINIERENDE#% MANNSCHAFT.` |
| 66 | 10 | 79f7 | `%c, DER HELD DES SPIELS MEINTE NACH DEM SPIEL ]BERGL]CKLICH:'%x04GEWONNEN HAT DAS GANZE TEAM, ICH BIN FROH, GUT GESPIELT ZU HABEN#ICH GLAUBE, DASS ICH GANZ GUT WAR#NUR WENN ALLE GUT SPIELEN, KANN MAN GEWINNEN#EIN TOLLES GEF]HL, SO GUT GESPIELT ZU HABEN#%.'` |
| 67 | 10 | 7af8 | `%c ZEIGTE EINE ]BERRAGENDE LEISTUNG UND F]HRTE DIE MANNSCHAFT ZUM SIEG.` |
| 68 | 10 | 7b40 | `BEFL]GELT VON %a'S %c'S SUPERLEISTUNG WUCHS DIE GESAMTE MANNSCHAFT ]BER SICH HINAUS.` |
| 69 | 10 | 7b95 | `BESCHEIDEN GAB SICH DER SUPERSTAR DES SPIELS %c NACH SPIELENDE:%x04ICH DENKE OHNE DIE TOLLEN LEISTUNGEN ALLE ANDEREN, W[RE DER SIEG NIE M\|GLICH GEWESEN#ZU EINEM SIEG BRAUCHT MAN ELF GUTE SPIELER, UND WIR HATTEN SIE HEUTE#DAS SPIEL LIEF F]R ALLE GUT, GLAUBE ICH. ICH BIN FROH GUT GESPIELT ZU HABEN#ICH HOFFE WIR SPIELEN IN DEN KOMMENDEN SPIELEN WEITER SO GUT#%.'` |
| 70 | 11 | 7cff | `%d WURDE NACH DEM SPIEL ALS DER VERLIERER DES TAGES DARGESTELLT, ABER AUCH SEINE MITSPIELER ZEIGTEN KEINE ANSPRECHENDEN LEISTUNGEN.` |
| 71 | 11 | 7d83 | `IN EINEM INTERVIEW NACH DEM SPIEL MEINTE DER GROSSE VERLIERER %d:'%x03OKAY, ICH HATTE EINEN SCHWACHEN TAG, ABER ALLE WAREN NICHT SO GUT DRAUF#ICH MUSS VERSUCHEN IM N[CHSTEN SPIEL ALLES VERGESSEN ZU MACHEN#ICH HABE MICH BEI DER MANNSCHAFT SCHON F]R MEINE LEISTUNG ENTSCHULDIGT#%.'` |
| 72 | 11 | 7e9b | `%d BEF\|RDERTE SEINE MANNSCHAFT DURCH SEIN SCHLECHTES SPIEL AUF DIE VERLIERERSTRASSE.` |
| 73 | 11 | 7ef0 | `DER EIGENTLICHE VERLIERER DES SPIELS IST WOHL %d, DER MIT SEINEN AKTIONEN DAS SPIEL DER EIGENEN MANNSCHAFT ZERST\|RTE, UND DURCH KATASTROPHALE FEHLP[SSE IMMER WIEDER F]R GEFAHR VOR DEM EIGENEN TOR SORGTE.` |
| 74 | 12 | 7fbc | `WAS DIE CHANCENAUSBEUTE BETRIFFT, SO KONNTE %a NICHT ]BERZEUGEN.` |
| 75 | 12 | 7ffd | `AN DER SCHWACHEN CHANCENVERWERTUNG SOLLTE NOCH ETWAS GEARBEITET WERDEN.` |
| 76 | 12 | 8045 | `DIE CHANCEN H[TTEN KONSEQUENTER GENUTZT WERDEN SOLLEN.` |
| 77 | 12 | 807c | `%a SOLLTE SICH EINMAL ]BERLEGEN, EINEN SPIELER ZU VERPFLICHTEN, DER EINFACH NUR DIE B[LLE INS TOR TRITT.` |
| 78 | 12 | 80e5 | `MIT EINER SOLCH SCHLECHTEN TORAUSBEUTE WIRD ES SCHWER SEIN, IN N[CHSTER ZEIT ERFOLGREICH ZU SEIN.` |
| 79 | 12 | 8147 | `%a SOLLTE IM N[CHSTEN TRAINING EINMAL DAS VERWERTEN VON TORCHANCEN ]BEN, WILL MAN DIE N[CHSTEN SPIELE ERFOLGREICH BESTEITEN.` |
| 80 | 12 | 81c4 | `WER SO VIELE TORCHANCEN VERGIBT, DARF SICH NICHT WUNDERN, WENN AM ENDE DER SAISON EINIGE PUNKTE WENIGER AUF DEM KONTO SIND, ALS MAN ERHOFFT HATTE.` |
| 81 | 13 | 8257 | `DIE OPTIMALE CHANCENVERWERTUNG SPRICHT F]R %a.` |
| 82 | 13 | 8286 | `MIT DIESER CHANCENVERWERTUNG WIRD %a IN DER N[CHSTEN ZEIT AUCH WEITERE SIEGE ERZIELEN.` |
| 83 | 13 | 82dd | `BEEINDRUCKEND WAR DIE SICHERE VERWERTUNG DER SICH BIETENDEN M\|GLICHKEITEN.` |
| 84 | 13 | 8328 | `DIE ABSOLUT KONSEQUENTE AUSNUTZUNG DER CHANCEN WAR DER GARANT F]R DIESEN SIEG GEGEN %b. F]R DIE ZUKUNFT L[SST DIES EINIGES ERHOFFEN.` |
| 85 | 13 | 83ad | `%a GL[NZTE DURCH TOLLE VERWERTUNG DER CHANCEN. DIE TREFFSICHERHEIT DER %x10GASTGEBER#G[STE#% WAR GERADEZU BE[NGSTIGEND F]R DIE KOMMENDEN GEGNER.` |
| 86 | 13 | 843e | `%a ZEIGTE, DASS EINE KONSEQUENTE AUSNUTZUNG DER CHANCEN DER GRUNDSTEIN F]R EINEN SIEG IST.` |
| 87 | 14 | 8499 | `%a BEWIES STARKE MORAL UND WAR DURCH DEN %2:%3 R]CKSTAND NICHT ZU BEEINDRUCKEN.` |
| 88 | 14 | 84e9 | `%a ERHOLTE SICH SCHNELL VON DEM %2:%3 R]CKSTAND UND GELANG ]BER DEN KAMPF ZU GUTEM SPIEL.` |
| 89 | 14 | 8543 | `DIE TOLLE AUFHOLJAGD VON %a BEEINDRUCKTE SOGAR DIE GEGNERISCHEN FANS.` |
| 90 | 14 | 8589 | `MIT RIESIGEM BEIFALL BEDANKTEN SICH DIE FANS F]R DIE LEISTUNGSSTEIGERUNG IHRER MANNSCHAFT, DIE EIN %2:%3 AUFHOLTE.` |
| 91 | 14 | 85fc | `%a BEEINDRUCKTE NICHT NUR DIE EIGENEN FANS MIT DER TOLLEN AUFHOLJAGD. ES GELANG, EINEN %2:%3 R]CKSTAND NOCH AUFZUHOLEN.` |
| 92 | 14 | 8674 | `DIE MORAL VON %a SCHEINT NICHT ZU BRECHEN ZU SEIN, DER MANNSCHAFT GELANG ES, AUS EINEM %2:%3 NOCH EIN %0:%1 ZU MACHEN.` |
| 93 | 15 | 86eb | `%t ZEIGTE SICH ERBOSST: ES MUSS DOCH AUCH EINMAL M\|GLICH SEIN, EINE %4:%5 F]HRUNG ZU VERTEIDIGEN.` |
| 94 | 15 | 874d | `%a ZEIGTE NERVEN UND LIESS SICH EINE %4:%5 F]HRUNG NOCH NEHMEN.` |
| 95 | 15 | 878d | `ENTT[USCHT [USSERTEN SICH DIE FANS VON %a DAR]BER, WIE LEICHTSINNIG EINE %4:%5 F]HRUNG VON IHRER MANNSCHAFT VERSPIELT WURDE.` |
| 96 | 15 | 880a | `ES DARF EINER MANNSCHAFT EIGENTLICH NICHT PASSIEREN, SICH EINE %4:%5 F]HRUNG NOCH NEHMEN ZU LASSEN.` |
| 97 | 15 | 886e | `SEHR VER[RGERT ZEIGTE SICH %t NACH DEM SPIEL. ER BEM[NGELTE, DASS DER %4:%5 VORSPRUNG NICHT VERTEIDIGT WERDEN KONNTE.` |
| 98 | 16 | 88e4 | `DER %x02SCHNELLE#FR]HE#% R]CKSTAND WURDE VON %a %x30RECHT GUT WEGGESTECKT#AUSGEZEICHNET VERKRAFTET#NICHT VERKRAFTET#%, WAS VOR ALLEM AUF %x30DIE K[MPFERISCHE EINSTELLUNG#TOLLE LEISTUNG DER GESAMTEN MANNSCHAFT#FEHLENDE MORAL DES TEAMS#% ZUR]CKZUF]HREN IST.` |
| 99 | 17 | 89e4 | `DIE %x03SCHNELLE F]HRUNG#FR]HE F]HRUNG#F]HRUNG IN DEN ERSTEN MINUTEN#% F]HRTE ZU%x30 L[SSIGKEITEN IN DER ABWEHR# SICHERHEIT IN ALLEN KOMBINATIONEN. ES GELANG VIELES#M UNTERSCH[TZEN DES GEGNERS, DER IN DER FOLGEZEIT SEHR VON DER NACHL[SSIGEN SPIELWEISE PROFITIERTE#%.` |
| 100 | 18 | 8aef | `%x30%x02DAS UNENTSCHIEDEN#DIE PUNKTETEILUNG#%#DER %x02ERFOLG#SIEG#%#DIE NIEDERLAGE#% GEGEN %b KAM ERST IN DEN %x02SCHLUSSMINUTEN#LETZTEN MINUTEN#% ZUSTANDE, MUSS %x02DAHER#AUS DIESER SICHT#% WOHL ALS %x30ERW[HNENSWERT#SEHR GL]CKLICH#SEHR [RGERLICH#% BEZEICHNET WERDEN.` |
| 101 | 18 | 8bfc | `BESONDERS %x30ERW[HNENSWERT#GL]CKLICH#[RGERLICH#% WAR, DASS %x30%x02DAS UNENTSCHIEDEN#DIE PUNKTETEILUNG#%#DER %x02ERFOLG#SIEG#%#DIE NIEDERLAGE#% GEGEN %b ERST IN DEN %x02SCHLUSSMINUTEN#LETZTEN MINUTEN#% ZUSTANDE KAM.` |
| 102 | 19 | 8cd5 | `DIE MEISTEN BESUCHER WAREN SICH EINIG, DASS DIESE NIEDERLAGE %x50VOLLKOMMEN UNVERDIENT#ABSOLUT GERECHTFERTIGT#% WAR.` |
| 103 | 19 | 8d4a | `DIE NIEDERLAGE VON %a WAR %x50WIRKLICH NICHT VERDIENT#VERDIENT#%.` |
| 104 | 19 | 8d8c | `AUCH EIN NEUTRALER BEOBACHTER DER PARTIE MUSS ZU DEM SCHLUSS KOMMEN, DASS DAS RESULTAT %x50WOHL SEHR SCHMEICHELHAFT F]R DEN SIEGER IST#MIT DEN GEZEIGTEN LEISTUNGEN ]BEREINSTIMMT#%.` |
| 105 | 19 | 8e41 | `NACH DEM SPIELVERLAUF IST DAS %0:%1 ALS %x50SEHR GL]CKLICH F]R DEN SIEGER#DEN LEISTUNGEN BEIDER TEAMS ENTSPRECHEND#% ZU BEZEICHNEN.` |
| 106 | 19 | 8ec5 | `ALLE WAREN SICH EINIG, DASS DIE PARTIE MIT %b EINEN %x50GL]CKLICHEN#VERDIENTEN#% SIEGER GEFUNDEN HAT.` |
| 107 | 19 | 8f2b | `DIE BESUCHER %x02DER PARTIE#DES SPIELS#% WAREN %x02EINER MEINUNG#SICH EINIG#%, DASS DER SIEG VON DER %x02KLAR#WESENTLICH#EINDEUTIG#% %x50GL]CKLICHEREN#BESSEREN#% MANNSCHAFT %x03ERZIELT#ERRUNGEN#ERREICHT#% WURDE.` |
| 108 | 19 | 8fff | `%a WURDE VON %x02DEN ZUSCHAUERN#ALLEN BESUCHERN#% ALS %x50UNGL]CKLICHER#VERDIENTER#% VERLIERER DIESER PARTIE ANGESEHEN.` |
| 109 | 20 | 9077 | `DIES IST HAUS NUMMER 3 (DONNERSTAGE WAREN PROBLEMTAGE F]R IHN).` |

### A.2 Headline templates `DS:4f90` (69 far pointers) grouped by `DS:9322`

| idx | group | DS addr | text (raw bytes) |
|---|---|---|---|
| 0 | 0 | 3e49 | `%a }BERROLLT%e%b MIT %0:%1` |
| 1 | 0 | 3e64 | `%a %x02DEMORALISIERT#DEKLASSIERT#%%e%b MIT %0:%1` |
| 2 | 0 | 3e95 | `%a FERTIGT%e%b MIT %0:%1 AB` |
| 3 | 0 | 3eb1 | `%a MACHT %x03KLASSE #SUPER #RIESEN#%SPIEL%e%x10GEGEN#IN#% %b` |
| 4 | 1 | 3eee | `%a %x02SCHL[GT#BESIEGT#%%e%b KNAPP MIT %0:%1` |
| 5 | 1 | 3f1b | `%0:%1 ZITTERSIEG%e%x10GEGEN#IN#% %b` |
| 6 | 2 | 3f3f | `%a BESIEGT%e%b MIT %0:%1` |
| 7 | 2 | 3f58 | `%a SCHL[GT%e%b %0:%1` |
| 8 | 2 | 3f6d | `%a GEWINNT%e%x10GEGEN#IN#% %b %0:%1` |
| 9 | 3 | 3f91 | `%a WIRD VON%e%b %0:%1 %xDEKLASSIERT#]BERROLLT#%` |
| 10 | 3 | 3fc1 | `%0:%1 PLEITE %x10GEGEN#IN#%%e%b` |
| 11 | 3 | 3fe1 | `%0:%1 PACKUNG F]R %a%e%x10GEGEN#IN#% %b` |
| 12 | 3 | 4009 | `BITTERE %0:%1 %x02NIEDERLAGE#SCHLAPPE#%%e%x10GEGEN#IN#% %b` |
| 13 | 4 | 4044 | `%a %x02VERLIERT %x10GEGEN#IN#%#UNTERLIEGT#%%e%b KNAPP MIT %0:%1` |
| 14 | 4 | 4084 | `KNAPPE %0:%1 NIEDERLAGE%e%x10GEGEN#IN#% %b` |
| 15 | 5 | 40af | `%a UNTERLIEGT%e%b MIT %0:%1` |
| 16 | 5 | 40cb | `%a %x10GEGEN#IN#%%e%b MIT %0:%1 UNTERLEGEN` |
| 17 | 5 | 40f6 | `%a VERLIERT%e%x10GEGEN#IN#% %b %0:%1` |
| 18 | 6 | 411b | `%a UND%e%b TRENNEN SICH %0:%1` |
| 19 | 7 | 4139 | `%0:%1 %x10GEGEN#IN#% %b%eVOR AUSVERKAUFTEM HAUS` |
| 20 | 7 | 4169 | `%9 ZUSCHAUER SAHEN%eEIN %0:%1 %x10GEGEN#IN#% %b` |
| 21 | 8 | 4199 | `%a MACHT AUS %2:%3%eNOCH EIN %0:%1` |
| 22 | 8 | 41bc | `%2:%3 %x02TOLL#GUT#% %x02VERKRAFTET#WEGGESTECKT#%-%e%0:%1 %x10GEGEN#IN#% %b` |
| 23 | 8 | 4208 | `NACH %x03GROSSER#TOLLER#RIESEN#% LEISTUNGSSTEIGERUNG%e%0:%1 %x10GEGEN#IN#% %b` |
| 24 | 8 | 4256 | `%a SCHAFFT NOCH%eWENDE %x10GEGEN#IN#% %b` |
| 25 | 9 | 427f | `TROTZ %4:%5 %x02VORSPRUNG#F]HRUNG#% %0:%1%e%x10GEGEN#IN#% %b` |
| 26 | 9 | 42bc | `%a VERLIERT %0:%1 NACH%e%4:%5 %x02VORSPRUNG#F]HRUNG#% %x10GEGEN#IN#% %b` |
| 27 | 9 | 4304 | `%0:%1-PLEITE NACH %4:%5%eF]HRUNG %x10GEGEN#IN#% %b` |
| 28 | 10 | 4337 | `NUR %0:%1 %x10GEGEN#BEI#% %b%eTROTZ %4:%5 F]HRUNG` |
| 29 | 10 | 4369 | `TROTZ %4:%5 KEIN SIEG%e%x10GEGEN#BEI#% %b` |
| 30 | 11 | 4393 | `TOLL GEK[MPFT, AUS %2:%3%eEIN %0:%1 GEMACHT` |
| 31 | 11 | 43bf | `%a ERREICHTE NACH %2:%3%eNOCH %0:%1 %x10GEGEN#BEI#% %b` |
| 32 | 12 | 43f6 | `%c SCHOSS%e%a ZUM SIEG` |
| 33 | 12 | 440d | `%c ERSCHOSS%e%b` |
| 34 | 12 | 441d | `%0:%1 DANK EINES%e]BERRAGENDEN %c` |
| 35 | 12 | 443f | `%c MACHT RIESENSPIEL%e%x10GEGEN#BEI#% %b` |
| 36 | 12 | 4468 | `%c BESTER SPIELER%e%x10GEGEN#BEI#% %b` |
| 37 | 12 | 448e | `%a SIEGT DANK%eWELTKLASSELEISTUNG VON %c` |
| 38 | 12 | 44b7 | `%c GARANT F]R DEN SIEG%e%x10GEGEN#BEI#% %b` |
| 39 | 13 | 44e2 | `TOLLES %0:%1 IM SPIEL %a%e%x10GEGEN#BEI#% %b` |
| 40 | 13 | 450f | `TOLLES SPIEL VON %a%eUND %b - %0:%1` |
| 41 | 13 | 4533 | `%0:%1 %x10GEGEN#BEI#% %b-%eEIN SUPERSPIEL` |
| 42 | 13 | 455d | `TOLLES FUSSBALLSPIEL%e%0:%1 %x10GEGEN#BEI#% %b` |
| 43 | 14 | 458c | `LANGWEILIGES %0:%1%e%x10GEGEN#BEI#% %b` |
| 44 | 14 | 45b3 | `ZUSCHAUER VER[RGERT ]BER%eLANGWEILIGES %0:%1` |
| 45 | 14 | 45e0 | `M]DES GECKICKE IM SPIEL%e %x10GEGEN#BEI#% %b` |
| 46 | 14 | 460d | `GRAUENHAFTES BALLGESCHIEBE%eZUSCHAUER PFIFFEN TEAMS AUS` |
| 47 | 14 | 4645 | `EIN TRAUERSPIEL%eIN ZWEI AKTEN - %0:%1` |
| 48 | 15 | 466c | `%d DER S]NDENBOCK%e%x10GEGEN#BEI#% %b` |
| 49 | 15 | 4692 | `SCHWACHE LEISTUNG%eVON %d` |
| 50 | 15 | 46ac | `RABENSCHWARZER TAG F]R%e%d %x10GEGEN#BEI#% %b` |
| 51 | 16 | 46da | `UNGL]CKLICHES %0:%1%e%x10GEGEN#BEI#% %b` |
| 52 | 16 | 4702 | `VIEL PECH BEIM %0:%1%e%x10GEGEN#BEI#% %b` |
| 53 | 16 | 472b | `%a VERZWEIFELT AN DER%eABWEHR VON %b` |
| 54 | 17 | 4750 | `%x02SCHMEICHELHAFTES#GL]CKLICHES#% %0:%1%e%x10GEGEN#BEI#% %b` |
| 55 | 17 | 478d | `ABWEHR VON %a%eSTAND %x02GUT#SICHER#%` |
| 56 | 17 | 47b3 | `VIEL GL]CK BEIM %0:%1%e%x10GEGEN#BEI#% %b` |
| 57 | 18 | 47dd | `HOCHVERDIENTES %0:%1%e%x10GEGEN#BEI#% %b` |
| 58 | 18 | 4806 | `%a MIT SUPERSPIEL%e%0:%1 %x10GEGEN#BEI#% %b` |
| 59 | 18 | 4832 | `SOUVER[NER %0:%1%eERFOLG %x10GEGEN#BEI#% %b` |
| 60 | 19 | 485e | `%a OHNE CHANCE IM SPIEL%e%x10GEGEN#BEI#% %b` |
| 61 | 19 | 488a | `%b DIE KLAR BESSERE%eMANNSCHAFT` |
| 62 | 19 | 48aa | `VERDIENTE %0:%1 NIEDERLAGE%eF]R %a` |
| 63 | 20 | 48cd | `LEISTUNGSGERECHTES %0:%1 IM SPIEL%e%x10GEGEN#BEI#% %b` |
| 64 | 20 | 4903 | `%0:%1 IN AUSGEGLICHENEM SPIEL%eTEAMS WAREN MIT EINEM PUNKT ZUFRIEDEN` |
| 65 | 21 | 4948 | `GL]CKLICHES %0:%1 F]R %a%e%x10GEGEN#BEI#% %b` |
| 66 | 21 | 4975 | `PUNKT MIT VIEL GL]CK%e%0:%1 %x10GEGEN#BEI#% %b` |
| 67 | 22 | 49a4 | `NUR %0:%1 TROTZ KLARER%e]BERLEGENHEIT %x10GEGEN#BEI#% %b` |
| 68 | 22 | 49dd | `%0:%1 - PUNKT VERSCHENKT%e%a NUTZTE CHANCEN NICHT AUS` |
