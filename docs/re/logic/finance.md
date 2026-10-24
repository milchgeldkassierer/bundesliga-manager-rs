# Finances, stadium, loans, training camp, training (area "finance")

Spec for an implementer without Ghidra access. It covers every function of segment **212a**
(finances, loans, training camp, training, season goal messages) and segment **1000** (stadium
screen and construction, sponsor subsidy, status line, fixture list). It also specifies the two
functions in segment 24a4 that compute the **monthly income and expenses** (24a4:2636 and
24a4:2822), because the money flow cannot be ported without them.

All addresses are Ghidra addresses in `/unpacked/BMMAIN_unpacked.EXE`. `DS:xxxx` is DGROUP
(segment `5cb3`), `FAR:xxxx` is the far data segment `5238`. Save-block numbers refer to
[savegame.md](../savegame.md); field names of the manager record follow [state.md](state.md).
Strings are quoted verbatim, in the game's umlaut encoding (`[`=Ä, `\`=Ö, `]`=Ü, `{`=ä, `|`=ö,
`}`=ü, `~`=ß).

Scratch material (gitignored) is in `out/finance/`: `*.c` raw decompiles, `*.asm` disassembly with
selector segments, call arguments and strings resolved (`fmt.py`, `strs.py`, `calls.py`), and
`ptab.py` (dumps a DGROUP far-pointer string table: `python3 ptab.py 28d8 8`).

## 0. Conventions

### 0.1 Integer rules

* MSC large model, `cdecl` far calls; callee-pop helpers for `long` math: `_aFlmul` 4a01:1ae6,
  `_aFldiv` 4a01:1a4c (signed, truncates toward zero), `_aFlrem` 4a01:1b18 (sign of dividend),
  `_aFlmul_assign` 4a01:1bf0 (`*p *= b`), `_aFldiv_assign` 4a01:1a08 (`*p /= b`), `_aFlshr`
  4a01:1bc4 (arithmetic). Every `long` below is `i32` with two's-complement wrap on overflow;
  `/` and `%` are C99 truncating. `u8`/`i8` additions to a byte field wrap mod 256 **before** any
  clamp that follows.
* `m` = `FAR:304a` (`i8`, current manager, sign-extended with `CBW` everywhere), `M = FAR:2242 +
  0x30a*m` (manager record, save block 13). Field `M+0xNNN` is written `M.xxx` with the FAR
  offset for `m = 0` in brackets.
* `random(lo, hi)` = `random_range` 176b:0cc7 = `rand() % (hi-lo+1) + lo` (FUNCTION_MAP §3.4). Every
  RNG call in this area is listed in program order. **Note:** `copy_protection_penalty` 16c7:09f9
  calls `random(0,100)` (and, on a hit, loops forever on `random(0,1)`) **only when `DS:05ac == 0`**
  (protection not passed). With the protection passed it does not touch the RNG. All screens below
  call it once at entry; its arguments are ignored.

### 0.2 Display conversion of money

Money is stored in DM. Every amount shown on screen is converted with the two option longs
`DS:224e` (call it `cur_div`) and `DS:2252` (`cur_mul`), both `1` in the shipped data:

```
display(x)  = (x * cur_mul) / cur_div          /* _aFlmul then _aFldiv, i32 */
internal(v) = (v * cur_div) / cur_mul          /* used for typed-in amounts */
```

Before every displayed amount the code `strcat`s the far string `DS:4f78` (empty) when the char
`DS:224d != 0`. `format_long_thousands` 176b:0681 prints a long with `.` thousand separators; while
the byte `DS:07b2` is 1 it prints **without** separators (used for years).

### 0.3 Mouse

`FAR:6dc0` is a far pointer to the pointer state. In every screen of this area **word `[0]` is the
Y coordinate and word `[1]` the X coordinate** (same finding as [stats.md](stats.md) §0.2, contrary
to MEMORY_MAP §3.1). Below, `mx = ptr[1]`, `my = ptr[0]`. `FAR:2ea2` is the last mouse event (1 =
left click, 3 = both/abort; set by `input_poll_mouse` 176b:0319).

### 0.4 UI and library calls (not part of this area)

| Address | Name | Use here |
|---|---|---|
| 176b:0356 | `vga_load_picture(id, …)` | loads `PIC\<id>.VGA` into a new bitmap |
| 176b:0c91 | `ui_set_font(rp, n)` | font 1/2 |
| 16c7:00a7 | `ui_draw_button(rp, x, y, w, h_or_style, rect_out, b1, b2)` | args quoted verbatim |
| 16c7:0002 | `ui_hit_test_buttons(rects, count, on_click)` | 1-based index or 0 |
| 16c7:07d5 | `ui_draw_text_at(rp, str, x, y, color)` | |
| 16c7:087c | `ui_draw_text_centered_shadow(rp, str, x0, x1, y, color, shadow)` | |
| 16c7:083c | `ui_fill_rect(rp, x0, y0, x1, y1, color)` | |
| 16c7:09b0 | `ui_draw_line(rp, x0, y0, x1, y1, color)` | |
| 16c7:0753 | `ui_draw_frame(rp, x0, y0, x1, y1, color)` | |
| 4930:0991 | `blit_bitmap(src, sx, sy, dst, dx, dy, w, h, minterm)` | |
| 176b:02e4 | `ui_blit_transparent(src, sx, sy, dst, dx, dy, w, h)` | |
| 4930:0849 | `blit_bitmap_scaled_or_copy(...)` | save/restore of the status line strip from the work bitmap `FAR:a6ca` |
| 176b:0e57 | `ui_dialog_box(s1, s2)` | two-line message box |
| 4091:0e3a | `dialog_okay(s1, s2, s3, s4)` | four-line "OKAY" box |
| 4091:146c | `message_broadcast(m, s1..s5)` | yes/no question to manager `m`; returns nonzero for yes |
| 4091:0190 | `message_box(flag, long kind, s1, s2, s3, …)` | builds a message, returns far ptr |
| 4091:0044 | `message_find_owner(ptr)` | |
| 4091:0000 | `team_slot_free_index(row)` | 0..9 or 0x7f |
| 4091:123d / 4091:11cc | `ui_register_icon_slot(x, y, str, id)` / `ui_hit_icon_slot()` | text buttons; `DS:07b3 = 0` resets the slot list |
| 4091:1722 | `ui_wait_with_highlight(0x10c, 0xc1)` | exit-button highlight before leaving a screen |
| 4091:1009 | `clamp_int(int far *p, lo, hi)` | |
| 4091:1109 | `squad_count(group, 0)` | number of used player records of group 0..3 (managers) or 4 (pool) |
| 4091:0ba5 | `ui_input_amount(long init, x, y, long min, long max, long step)` | numeric slider; `-1` = cancel |
| 176b:07d7 | `ui_input_number(ptr_state, buf, x, y, flags, 0xb, a, b)` | typed number entry, returns `long` |
| 18bc:3b92, 4091:185c | `office_show_cached_background`, `ui_draw_manager_header` | |
| 176b:1416 | `screen_fade(pal, 1)` | fade in |
| 3277:000a | `player_reroll_traits(p)` | **RNG**: `p[0x0e]=random(35,65)`, `p[0x14]=random(1,13)`, `p[0x14]|=random(1,3)<<4`, `p[0x14]|=random(0,1)<<7` |
| 3277:0081 / 3277:01a9 / 3277:0225 | training bar helpers | see §2.10 |
| 1f9d:0002 | `manager_ai_weekly(m, 0)` | called when the camp screen closes (uses RNG; spec in ai.md) |
| 1f9d:1122 | training effect estimate (4 int out-params) | |
| 1310:1190 | `matchday_refresh(week)` | used by the fixture list to compute a match date |

## 1. Data layout

### 1.1 Manager record fields used here (`M = FAR:2242 + 0x30a*m`, block 13)

| Off | FAR (m=0) | Type | Suggested name | Meaning |
|---|---|---|---|---|
| 0x000 | 2242 | char[29] | `name` | |
| 0x01e | 2260 | u8 | `club` | club index |
| 0x109+r | 234b+r | u8 | `round_position[r]` | league position (0-based) after round `r` (read by §2.12) |
| 0x10a | 234c | u8 | `ticket_price` | ticket price in DM units (5..25), edited in the stadium screen |
| 0x138 | 237a | u8 | `league` | 0 = Bundesliga, 1 = 2. Liga, 2 = amateur |
| 0x139 | 237b | u8 | `camp_cooldown` | days until the next training camp; set to `random(7,18)` by a booking, decremented daily (the office screen reads it) |
| 0x13f | 2381 | u8 | `training_intensity` | 0..34 (`0x22`) |
| 0x141 | 2383 | u8[4] | `training_a[4]` | Kondition, Spiel, Schuss, Taktik (1..10) |
| 0x145 | 2387 | u8 | `training_a4` | fifth bar of group A (bar id 10) |
| 0x146 | 2388 | u8[4] | `training_b[4]` | Tor, Abwehr, Mittelfeld, Angriff (1..10) |
| 0x15e | 23a0 | i32[7][2] | `stadium[7]` | `{current, pending}` per item, see 1.2 |
| 0x196 | 23d8 | u16[7] | `build_days[7]` | construction countdown per item (days) |
| 0x1dc | 241e | u16 | `fans` | popularity/fan count (grows to 95 max, see §2.1) |
| 0x1de | 2420 | u16 | `extra_costs` | monthly expense item (unsigned) |
| 0x1e0 | 2422 | u16 | `income_base` | low word of the monthly base income (written by 24a4:2636) |
| 0x1e2 | 2424 | i16 | `fatigue_sum` | `+= 4*training_intensity` at every month end |
| 0x1f0 | 2432 | i32 | `balance` | Kontostand |
| 0x1fc | 243e | Credit[5][3] | `credits[lender][slot]` | `lender` 0..3 = human manager, 4 = bank |

### 1.2 Stadium items (`S = FAR:23a0 + 0x30a*m`)

| Item `i` | UI row `aa = i+1` | `S+8i` current | Unit | DS:0044+4aa price | DS:0063+aa base weeks | Max |
|---|---|---|---|---|---|---|
| 0 | 1 | seats (Sitzplätze) | places | 150 000 per 1000 | 15 | seats+standing+pending ≤ 130 000 (0x1FBD0) |
| 1 | 2 | standing (Stehplätze) | places | 100 000 per 1000 | 15 | same |
| 2 | 3 | covered (überdacht) | places | 160 000 per 1000 | 20 | ≤ seats + standing |
| 3 | 4 | floodlights | level 0..3 (`IS NICH`, `KLEIN`, `MITTEL`, `GRO~`) | 380 000 per level | 10 | 3 |
| 4 | 5 | scoreboard | level 0..3 | 440 000 per level | 10 | 3 |
| 5 | 6 | condition (Zustand) | level 1..6 (`UNGENUEGEND`..`SEHR GUT`) | 550 000 per level | 8 | 6 |
| 6 | 7 | comfort | level 1..6 | 650 000 per level | 4 | 6 |
| — | 8 | ticket price | `M.ticket_price` | — | — | 5..25 |

`S+8i+4` is the pending amount (for levels: the level delta), `S+0x38+2i` = `M.build_days[i]`.
`DS:0044` = `{-1, 150000, 100000, 160000, 380000, 440000, 550000, 650000}` (i32, index = row; DS:0044/0046
also double as the status-line x/width words, see §3.8). `DS:0063` = `{0, 15, 15, 20, 10, 10, 8, 4}`.
`FAR:5780[7]` (block 46) is a per-item **construction-firm lock** shared by all managers (`FAR:577f+aa`).

### 1.3 Credit (0x12 bytes, `FAR:243e + 0x30a*m + 0x12*(3*lender + slot)`)

| Off | Type | Meaning |
|---|---|---|
| 0x00 | i32 | principal; slot is free when 0 |
| 0x04 | i32 | interest paid per payment = `rate*principal/100` |
| 0x08 | u8 | day taken |
| 0x09 | u8 | payment day of month |
| 0x0a | u8 | month taken (0-based) |
| 0x0b | u8 | due month (0-based) |
| 0x0c | u8 | rate in % |
| 0x0d | u8 | unused |
| 0x0e | i16 | year taken |
| 0x10 | i16 | due year |

### 1.4 Global tables (DGROUP, read from the image)

| DS | Type | Value | Meaning |
|---|---|---|---|
| 07b8 | i8[12] | 31,28,31,30,31,30,31,31,30,31,30,31 | days per month (no leap years) |
| 060e | u8[6] | initial 07 06 05 04 03 02 (block 0) | bank interest in % for 3,6,9,12,15,18 months |
| 0630 | i16 | initial 5 | base interest rate, 2..7 |
| 0620 | i16[8] | initial -80,-50,-100,-60,80,50,70,60 (block 51) | training-camp availability counters (>0 closed, <0 open) |
| 5258 | i32[8] | initial 5,6,7,10,11,10,13,15 | camp prices per week; multiplied once by `squad*2000` (§2.3) |
| 2394 | far char*[8] | `Kickfelder Mühle`, `Trlg. Hackstedt`, `Hof Schweißperle`, `Fitnesscenter Krone`, `Scola Ludum Pilae`, `Sportschule Träne`, `Hotel Medium`, `Luxushotel La Preux` (CP437 umlauts here) | camp names |
| 527e | i8[8][5] | {1,2,1,1,1} {1,2,3,1,1} {1,3,2,1,1} {2,4,3,2,2} {2,1,2,4,4} {2,4,3,2,2} {3,2,2,3,4} {5,3,2,4,5} | camp quality: Erholung, Fitness, Platz, Raum, Verpflegung (5th unused) |
| 52a6 | i8[4][3] | {20,20,40} {60,10,20} {20,50,20} {10,40,30} | weight of quality `q` for skill `k` |
| 5278 | u8[5] | 24,1,15,19,18 | colours of the camp legend letters |
| 29a4 | char[5] | `EFPRV` | camp legend letters |
| 066c | i32[4][9] | per manager: 50000, 6000 ×6, 0, 5000 (block 41) | contract incomes `[0..7]` (`[0]` main sponsor, `[1..6]` advertising boards) and `[8]` a monthly cost/fan-promotion item |
| 0644 | i32[4] | 0 | sum of the daily balances of the current month |
| 006c,0070,0078,007c,0080 | u8[] | {23,23,36,36}, {9,111,9,111,9,115,9,115}, {1,0,1,1}, {53,53,55,55}, {3,1,0,0} | stadium picture overlay positions (§3.3) |

## 2. Segment 212a

### 2.1 212a:0a6d `finance_daily_processing` (was FUN_212a_0a6d; FUNCTION_MAP: *finance_weekly_processing*)

`void finance_daily_processing(void)`. Called by `season_main_loop` 2a58:3176 (three call sites)
**once per day for every human manager**, with `FAR:304a = m` (and at two sites `FAR:5358 = m`).
Everything that is not inside the month-end block therefore runs `DS:07ab` times per day.

```c
void finance_daily_processing(void) {
    stadium_construction_daily();                         /* 1000:20e1, §3.5 */

    for (i = 0; i < 8; i++) {                              /* DS:0620 camp counters */
        int v = DS_0620[i];
        int s = (v < 0) - (v > 0);                         /* +1 if negative, -1 if positive, 0 if 0 */
        v += s; DS_0620[i] = v;
        if (v == 0)
            DS_0620[i] = random(20, 70) * s;               /* RNG; called even when s == 0 (result 0 stays 0) */
    }
    if (random(0, 60) == 0) interest_rates_update();       /* 212a:000a, RNG */

    DS_0644[m] += M.balance;                               /* i32 */
    if (M.camp_cooldown != 0) M.camp_cooldown--;

    int dim = (i8)DS_07b8[FAR_2e8e];                       /* FAR:2e8e month (long, low word used as index) */
    if ((i32)dim == FAR_2eba /* day, long */) {            /* --- last day of the month --- */
        if (FAR_2e8e % 4 == 0) DS_5256 = 1;                /* _aFlrem(month, 4) */
        if (FAR_0000[2*m] != 0 && --FAR_0000[2*m] == 0)
            DS_066c[m][0] /= 10;                           /* _aFldiv_assign(&.., 10L) */
        for (i = 0; i < 6; i++)
            if (FAR_4b9a[2*(6*m+i)] != 0 && --FAR_4b9a[2*(6*m+i)] == 0)
                DS_066c[m][1+i] /= 10;
        M.fatigue_sum += (u16)M.training_intensity * 4;    /* i16 add */
        M.balance += finance_monthly_income();             /* 24a4:2636, §4.1 */
        M.balance -= finance_monthly_expenses();           /* 24a4:2822, §4.2 */
        DS_0644[m] = 0;
        if (M.fans < 95) {                                 /* unsigned compare */
            i32 q = DS_066c[m][8] / 2500;
            if (q > (i32)random(0, 22))                    /* RNG only when fans < 95 */
                M.fans += random(1, 3);                    /* RNG */
        }
    }

    /* loans: interest and repayment */
    for (lender = 0; lender < 5; lender++)
      for (slot = 0; slot < 3; slot++) {
        Credit *c = &M.credits[lender][slot];
        if (c->principal == 0) continue;
        bool pay = ((i32)c->pay_day == FAR_2eba);
        if (!pay) {
            i8 d_cur = DS_07b8[FAR_2e8e];
            if (DS_07b8[c->due_month] > d_cur && (i32)d_cur == FAR_2eba) pay = true;
        }
        if (!pay) continue;
        M.balance -= c->interest;
        if ((i32)c->due_year == FAR_a7a0 && (i32)(u8)c->due_month == FAR_2e8e) {
            char who[30];
            strcpy(who, lender == 4 ? DS_4bd8 /* "der Bank" */ : MGR(lender).name);
            copy_protection_penalty(M.name, 0, 0, 0);
            office_show_cached_background();
            ui_draw_manager_header();
            dialog_okay("Ihr Kredit von", who, "wurde heute f{llig.", "");   /* DS:4e50, DS:4e54, DS:52b5 */
            M.balance -= c->principal;
            if (lender != 4) MGR(lender).balance += c->principal;
            c->principal = 0;                              /* interest field is left stale */
        }
      }
}
```

Notes:
* The second payment rule compares `days_in_month[due_month] > days_in_month[current_month]`, not the
  payment day. A loan taken on the 15th with a 31-day due month therefore also pays interest on the
  30th of every 30-day month (two payments in that month). Port it as is.
* `FAR:2e8e`/`FAR:2eba` are longs; the month is used as an array index through its low word.
* Dead code, not ported: 212a:0d47..0d5f computes `local_30 = min((i8)DS_07b8[FAR_2e8e], local_30)`
  (CBW, `CMP AX,[bp-0x30]; JGE; MOV [bp-0x30],AX`). The local is overwritten at 212a:0e9b before any
  read, so the result is unused. It makes no RNG call and has no side effect. In the same way, 212a:0f1b
  sets a "paid" flag `[bp-0x3a] = 1` that is never read.
* Callees: 1000:20e1, 212a:000a, 24a4:2636, 24a4:2822, 16c7:09f9, 18bc:3b92, 4091:185c, 4091:0e3a,
  random_range, CRT. Caller: 2a58:3176.

### 2.2 212a:000a `interest_rates_update` (was FUN_212a_000a)

`void interest_rates_update(void)`. Callers: `start_menu_new_or_load` 18bc:2184, 212a:0a6d.

```c
void interest_rates_update(void) {
    int c = ((i8)DS_4a28 > 2) ? 1 : 0;          /* DS:4a28 = 5 - user level */
    int r = random(0, c + 2);                     /* RNG */
    DS_0630 += r + c - 1;                         /* c=0: -1..+1, c=1: 0..+3 */
    clamp_int(&DS_0630, 2, 7);
    int v = DS_0630, step = 5 - v;
    for (u8 i = 0; i < 6; i++) {
        DS_060e[i] = (u8)v;
        v += step;
        if (step > 1) step--;
        clamp_int(&v, 2, 7);
    }
}
```
Example: base 7 → `07 05 03 02 02 02`; base 5 → `05 ×6`; base 3 → `03 05 06 07 07 07`.

### 2.3 212a:0145 `screen_training_camp` (was FUN_212a_0145)

`void screen_training_camp(void)`. Office menu 2 case 5 (jump table 18bc:1b42). Callees: 176b:0356,
16c7:09f9, 4091:1109, 176b:0c91, 4930:0991, 176b:02e4, 4091:105d, 16c7:087c, 16c7:07d5, 176b:0dca,
16c7:00a7, 176b:1416, 16c7:0002, 1000:243f, 176b:0e57, 212a:072d, 176b:0e27, 4091:1722, 1f9d:0002.

Setup:
1. `closed_pic = 0`; if any `DS:0620[i] > 0`: `closed_pic = vga_load_picture(0x2b)`.
2. `copy_protection_penalty("Trainingslager")` (DS:2534).
3. If the `i32` `DS:5258[0] < 10`: for `i = 0..7`: `DS:5258[i] *= (u32)((u16)squad_count(m,0) * 2000)`
   (16-bit unsigned `MUL`, then `_aFlmul_assign`). This happens only once per program run/save
   (the table is global, not per manager, and is not reset).
4. `tiles = vga_load_picture(5)`; font 2.
5. For `i = 0..7` (`col = i>3`, `row = i%4`):
   * `blit_bitmap(tiles, col*0x82, row*0x27, rp, col*0x84+4, row*0x34+7, 0x82, 0x27, 0xc0)`;
   * if `DS:0620[i] > 0`: `ui_blit_transparent(closed_pic, 0, 0, rp, col*0x56+0x34, row*0x34+9, 0x50, 0x21)`;
   * name = upper-case copy of `DS:2394[i]` (4091:105d); `ui_draw_text_centered_shadow(rp, name,
     col*0x56+0x2f, col*0x56+0x8d, row*0x34+0x33, 0x1d, 0)`;
   * for `q = 0..4`: letter `"EFPRV"[q]`: `ui_draw_text_at(rp, letter, col*0xd9 + 8*q + 9,
     row*0x34+0x33, 0)` then `ui_draw_text_at(rp, letter, col*0xd9 + 8*(q+1), row*0x34+0x33,
     DS:5278[q])`.
6. Free both bitmaps; exit button `ui_draw_button(rp, 0x10c, 0xc1, 0, 0x22, rect, 0, 8)`; font 1; fade in.

Loop (`hover = -1`, `booked = 0`):
```c
do {
    hit = ui_hit_test_buttons(rect, 1, 1);
    if (hit == 1) break;
    my = ptr[0]; mx = ptr[1]; sel = 0xff (i8);
    for (col4 = 0; col4 < 4; col4++) {                     /* tile rows */
        if (!(col4*0x34+8 < my && my < col4*0x34+0x2a)) continue;
        if ((5 < mx && mx < 0x2d) || (0xde < mx && mx < 0x106)) {   /* legend strip */
            mx = ((mx - 6) % 0xd9) >> 3;  sel = mx;  price = 0;     /* signed % and >> */
        }
        if ((0x33 < mx && mx < 0x83) || (0x88 < mx && mx < 0xd9)) { /* tile body; mx may have been replaced above */
            mx = (mx > 0x88)*4 + col4 + 5;  sel = mx;              /* = camp index + 5 */
            price = DS_5244[mx];                                    /* = DS:5258[camp] */
        }
        if (sel == -1) continue;
        if (hover != mx) {                                          /* hover is i8, compared sign-extended */
            status_line_show(mx + 10, display(price)); hover = mx;  /* ids 10..14: legend, 15..22: camp price */
        }
        if (FAR_2ea2 != 1 || price == 0) continue;
        if (DS_0620[mx - 5] > 0) { ui_dialog_box(DS_2394[mx-5], "hat nicht ge|ffnet..."); continue; }
        if (booked) { ui_dialog_box("Einmal pro Woche", "reicht doch..."); continue; }
        if (M.balance < price) {                                    /* signed i32 */
            ui_dialog_box("Keine finanzielle", "M|glichkeit gegeben..."); ui_set_font(rp, 1); continue;
        }
        M.balance -= price;
        booked = training_camp_apply(mx - 5);                       /* 212a:072d, returns 1 */
        input_wait_release();
        break;
    }
    if (hover != -1 && sel == -1) { status_line_show(0xff, 0); hover = -1; }
} while (hit != 1 && !booked);
if (!booked) ui_wait_with_highlight(0x10c, 0xc1);
manager_ai_weekly(m, 0);                                            /* 1f9d:0002, RNG */
```

### 2.4 212a:072d `training_camp_apply` (was FUN_212a_072d)

`u8 training_camp_apply(u8 camp)`. Caller: 212a:0145. Callees: 176b:0356, 16c7:00a7, 4930:0991,
176b:0dca, random_range, 3277:000a, 4091:1109, 4930:01e7, 16c7:0563.

```c
u8 training_camp_apply(u8 camp) {
    bmp = vga_load_picture(0x1c);
    btn = ui_draw_button(rp, 0x84, 0x4e, 0x39, 0x3a, tmp, 1, 7);
    blit_bitmap(bmp, 0, 0, rp, 0x85, 0x4f, 0x37, 0x38, 0xc0); bitmap_free(bmp);
    int s[3] = {0, 0, 0};
    for (q = 0; q < 4; q++) for (k = 0; k < 3; k++)
        s[k] += (i16)((i8)DS_527e[camp][q] * (i8)DS_52a6[q][k]);   /* IMUL byte */
    for (k = 0; k < 3; k++) s[k] /= 5;
    int t;
    for (i = 0; i < squad_count(m, 0); i++) {                     /* count re-evaluated each pass */
        u8 *p = &FAR_774a[0x34 * (m*25 + i)];                     /* player record */
        int d = (s[0] + s[1] + s[2]) / -3 + (s[0] - s[2] + 10) * 2 + s[1];
        if (d < 80) d = 80;
        p[0x13] += (u8)random(d / 9, d / 6);                      /* RNG; wraps mod 256 */
        if (p[0x13] < 60) p[0x13] = 60;                           /* unsigned */
        if (p[0x13] > 150) p[0x13] = 150;
        for (k = 0; k < 3; k++) {
            int r = random(2*(i8)DS_4a28 + 87, 2*(i8)DS_4a28 + 117);   /* RNG */
            t = (r + s[k] - 100) / 10;
            int a = random(0, 20 * t / 30);                        /* RNG */
            int b = random(0, t);                                  /* RNG */
            t += b - a;
            p[0x10 + k] += (u8)(t >> 2);                           /* SAR; negative t wraps the byte */
            if (p[0x10 + k] > 99) p[0x10 + k] = 99;                /* unsigned: a wrapped value becomes 99 */
        }
        t += t * s[2] / 200;                                       /* t of k == 2 */
        if (random(0, 4) == 0)                                     /* RNG */
            p[0x0e] += (u8)((i32)(random(155, 300) * t) / 800);    /* RNG; IMUL word -> i32, _aFldiv */
        player_reroll_traits(p);                                   /* 3277:000a, 4 RNG calls, overwrites p[0x0e] */
    }
    delay_ticks(80);
    ui_redraw_buttons(btn, 0);
    M.camp_cooldown = random(7, 18);                               /* RNG */
    delay_ticks(20);
    return 1;
}
```
`random_range` with `hi < lo` (e.g. `t < 0`) uses `rand() % (hi-lo+1)` with a non-positive modulus
exactly like the original (MSC `%` of a positive `rand()` by a negative or zero divisor; zero
divides by zero — `t` is ≥ -1 for normal skill values, so `20*t/30` is 0 and `random(0,-1)` would
fault; it never happened in testing). Port `random_range` verbatim. Player fields: `+0x10..0x12`
three skills (≤ 99), `+0x13` condition 60..150, `+0x0e` see 3277:000a.

### 2.5 212a:0f8d `credit_sum` (was FUN_212a_0f8d)

`i32 credit_sum(u8 which, i8 lender)`: sum over `c = 0..4`, `j = 0..2` of credits of manager `m`
whose principal is nonzero and `lender == c || lender == 99`; adds the principal (`which == 0`) or
the interest field (`which != 0`). Callers: 212a:167f, 212a:23cb, 24a4:2636, 24a4:2822,
362f:0af7, 422b:2a2a.

### 2.6 212a:1039 `credit_take` (was FUN_212a_1039)

`int credit_take(i32 amount, i8 rate, i8 lender, i8 months)`. Caller: 212a:167f.

```c
int credit_take(i32 amount, i8 rate, i8 lender, i8 months) {
    for (j = 0; j < 3; j++) if (M.credits[lender][j].principal == 0) break;
    if (j == 3) { ui_dialog_box("Schon 3 Kredite", "aufgenommen."); return 0; }
    Credit *c = &M.credits[lender][j];
    c->principal = amount;  c->rate = rate;
    c->interest  = ((i32)rate * amount) / 100;
    c->day = (u8)FAR_2eba;  c->month = (u8)FAR_2e8e;
    c->year = c->due_year = (i16)FAR_a7a0;
    i8 dm = (i8)FAR_2e8e + months;
    while (dm > 11) { c->due_year++; dm -= 12; }
    c->due_month = dm;
    i8 d = DS_07b8[(u8)dm];
    c->pay_day = ((i32)d >= FAR_2eba) ? (u8)FAR_2eba : (u8)d;   /* i32 compare (asm 212a:1166) */
    return 1;
}
```

### 2.7 212a:119b `credit_list_draw` (was FUN_212a_119b)

`void credit_list_draw(i8 lender)`. Callers: 212a:167f (5×). Font 2, `DS:07b2 = 0`, lines
`(7,0x84)-(0x82,0x84)` and `(7,0x9f)-(0x82,0x9f)` colour 6. If `lender == -1` skip the list.
With `lender == -1` the list is skipped and all three slots are cleared (the clear loop below runs with
`n = 0`; confirmed against the original). Otherwise `n = 0`; for `j = 0..2` with a nonzero principal: `ui_fill_rect(rp, 9, 0x6a+0x1c*n, 0x81,
0x81+0x1c*n, 7)`; `y = (n+4)*0x1c`, then `n++` and if `n == 2` then `y--`; draw at `x = 9`, colour 1:
* `y`: `"<day>.<month+1>.<year>: " + [DS:4f78] + display(principal) + " DM (" + rate + "%)"` (year printed
  with `DS:07b2 = 1`);
* `y+8`: `"F[LLIG AM " + pay_day + "." + (due_month+1) + "." + due_year`;
* `y+16`: `"ZINSEN PRO MONAT: " + [DS:4f78] + display(interest) + " DM"`.
Then the empty slots `n..2` are cleared with the same fill rect. Font 1 at the end.

### 2.8 212a:167f `screen_credits` (was FUN_212a_167f)

`void screen_credits(void)`. Office menu 1 case 5 (jump table 18bc:1a15). Callees: 176b:0356,
16c7:09f9, 16c7:00a7, 4930:0991, 212a:119b, 176b:0c91, 16c7:07d5, 176b:0681, 16c7:09b0, 212a:23cb,
176b:1416, 16c7:0002, 212a:0f8d, 1000:243f, 176b:07d7, 4930:0849, 4091:0e3a, 176b:0e57, 212a:1039,
4091:1722, 176b:0dca.

Setup: `DS:07b2 = 0`; `pic7 = vga_load_picture(7)`; `copy_protection_penalty("Kredite")`. Four
frames `ui_draw_button(rp, 6+0x90*a, 0xf+0x59*b, 0x8b-0x1e*a, 0x52, rect, 0, 7)` (`a,b ∈ {0,1}`).
`blit_bitmap(pic7, 0,0, rp, 7,0x10, 0x8a,0x51, 0xc0)`, `blit_bitmap(pic7, 0x8b,0, rp, 0x83,0x69,
0xe,0x51, 0xc0)`. Lender buttons, `n = 0`: for every other human manager `s` (`s < DS:07ab`, `s != m`):
`ui_draw_button(rp, 0x10c, 0x2d*n+0xf, 0, 0, &rects[n], 0, 7)`, face `blit_bitmap(FAR:2f24, s*0x29+5,
8, rp, 0x115, 0x2d*n+0x18, 0x20, 0x17, 0xc0)`, `lender_of[n++] = s`. Bank: the same with style `0x8000`
and face `(pic7, 0x9b, 0)`, `lender_of[n] = 4`, `sel_btn = n`, `n++`. Exit: `ui_draw_button(rp, 0x10c,
0xc1, 0, 0x22, &rects[n], 0, 7)`. `lender = 4`; `credit_list_draw(4)`; font 1;
`"Dauer  Zinssatz"` at (0xa0, 0x18) colour 1. With `DS:079c = 2`: for `r = 0..5`, `y = 0x25 + 11r`:
`"<3(r+1)> Mon."` at `x = 0xa0 - 3*(r>2)`, colour 2; `"<DS:060e[r]>%"` at x 0xdf colour 2; for `r < 5`
line `(0x97, (u8)(0x27+11r))-(0x102, same)` colour 6. `DS:079c = 1`; `credit_summary_draw()`; fade in.

Loop (`hover = -1`, `toggle = 0`), until `SI == DS:07ab + 1`:
```c
SI = ui_hit_test_buttons(rects, DS_07ab + 1, 1);
mx = ptr[1]; my = ptr[0];
if (FAR_2ea2 == 1 && SI != 0 && SI < DS_07ab + 1 && lender_of[SI-1] != lender) {
    ui_draw_button(rp, 0x10c, 0x2d*sel_btn+0xf, 0, 0x2000, tmp, 0, 8);   /* un-highlight */
    sel_btn = SI - 1;
    ui_draw_button(rp, 0x10c, 0x2d*sel_btn+0xf, 0, 0xa000, tmp, 0, 8);   /* highlight */
    lender = lender_of[SI-1]; credit_list_draw(lender); SI = 0;
}
if (FAR_2ea2 == 1 && 0x83 < mx && mx < 0x92) {
    int a = (0x9f < my && my < 0xbb), b = (0x69 < my && my < 0x84);
    if (a - b == 0) {                                   /* click in the strip outside both arrow parts */
        toggle ^= 1;
        if (toggle) {
            credit_list_draw(-1); font 2;
            "SCHULDEN HIER: " + [4f78] + display(credit_sum(0, lender)) + " DM" at (9, 0x8f) colour 1;
            "ZINSEN HIER: "   + [4f78] + display(credit_sum(1, lender)) + " DM" at (9, 0x98) colour 1;
            font 1;
        } else credit_list_draw(lender);
    }
}
if (0xa0 < mx && mx < 0x103 && 0x1e < my && my < 0x5d) {
    int row = (my - 0x1f) / 11;
    if (row != hover) {
        if (lender == 4) status_line_show(0x17, DS_060e[row]);          /* "Kredit zu x% aufnehmen." */
        else if (hover == -1) status_line_show(0x17, (i32)far_ptr(MGR(lender).name)); /* "Kredit von <name> aufnehmen." */
        hover = row;
    }
} else if (hover != -1) { status_line_show(0xff, 0); hover = -1; }

if (FAR_2ea2 == 1 && hover != -1) {
    status_line_show(0x18, 0);                          /* "Wie hoch soll der Kredit sein ?" */
    amount = internal(ui_input_number(ptr, buf, 0xba, 0xe1, 0x3ef, 0xb, 0xff, 0));
    restore_status_strip();                             /* blit work bitmap FAR:56fa (0,0xd9,0x10b,0xa) back */
    SI = 0;
    if (lender != 4) {
        if (MGR(lender).balance < amount) {
            if (cur_div == 1L) SI = dialog_okay("So viel Geld", "besitzt", MGR(lender).name, "gar nicht...");
            else               SI = dialog_okay(MGR(lender).name, "So viel Geld", "besitzt", "gar nicht...");
        }
        if (credit_sum(0, lender) + amount > 1000000)
            SI = dialog_okay("Sie k|nnen nur", "maximal 1 Mio. DM", "Kredit von einem", "Mitspieler aufnehmen.");
    } else if (credit_sum(0, 4) + amount > 4000000)
            SI = dialog_okay("Sie k|nnen nur", "maxmimal 4 Mio. DM", "Kredit von der", "Bank aufnehmen.");
    font 1;
    if (SI == 0 && amount != 0) {
        if (lender != 4) {
            status_line_show(0x19, 0);                  /* "Laufzeit des Kredites (Mon.) ?" */
            months = (int)ui_input_number(ptr, buf, 0xd2, 0xe1, 0x3ea, 0xb, 0, 0); restore_status_strip();
            if (months > 24) SI = ui_dialog_box("Maximal", "24 Monate.");
            if (SI == 0 && months != 0) {
                status_line_show(0x1a, 0);              /* "Prozentsatz des Kredites ?" */
                rate = (int)ui_input_number(ptr, buf, 0xaf, 0xe1, 0x3ea, 0xb, 0, 0); restore_status_strip();
                if (rate > 13) {                        /* no fine is actually charged */
                    dialog_okay("Wegen Wucher werden Sie", "mit einer", "Disziplinarstrafe von", "von 1.000.000 DM belegt.");
                    SI = dialog_okay("Okay...", "War nur ein Scherz...", "Das war aber trotzdem", "kein fairer Zug...");
                }
                if (rate < 3) SI = dialog_okay("Mindest-Prozentsatz:", "l}cherliche", "minimale", "3 Prozent.");
            }
        } else { months = (hover + 1) * 3; rate = DS_060e[hover]; }
        if (SI == 0 && months != 0 && rate != 0 &&
            credit_take(amount, (i8)rate, (i8)lender, (i8)months)) {
            credit_list_draw(lender);
            M.balance += amount;
            if (lender != 4) MGR(lender).balance -= amount;
            credit_summary_draw();
        }
    }
    status_line_show(0xff, 0);
}
```
Exit: `ui_wait_with_highlight(0x10c, 0xc1)`, free `pic7`, `DS:07b2 = 1`.

### 2.9 212a:23cb `credit_summary_draw` (was FUN_212a_23cb)

`void credit_summary_draw(void)`. Callers: 212a:167f (2×). `ui_fill_rect(rp, 0x97,0x69,0x102,0xb9, 7)`,
lines y=0x84 and y=0x9f from x 0x97 to 0x102 colour 6, `DS:07b2 = 0`; centered between 0x97 and
0x103, colour 1, shadow 0: `"Kontostand:"` y 0x72; `[4f78]+display(M.balance)+" DM"` y 0x7e;
`"Gesamtschulden:"` y 0x8d; `display(credit_sum(0,99))+" DM"` y 0x99; `"Zinsen/Monat:"` y 0xa8;
`display(credit_sum(1,99))+" DM"` y 0xb4.

### 2.10 212a:274c `screen_training` (was FUN_212a_274c)

`void screen_training(void)`. Office menu 2 case 4. Callees: 16c7:09f9, 176b:0356, 16c7:00a7,
3277:0081, 4930:0991, 176b:0c91, 16c7:087c, 3277:0225, 3277:01a9, 16c7:083c, 16c7:0753, 1f9d:1122,
16c7:07d5, 176b:1416, 16c7:0002, 4091:1009, 4091:1722, 176b:0dca, CRT.

Helpers outside the area (squad agent): `3277:01a9(group)` returns the remaining free training points
of group 0 (`training_a`) or 1 (`training_b`); `3277:0225(bmp, bar_id, from, to, add)` draws segments
`from..to` of bar `bar_id` (0/1 = free points of group A/B, 2..5 = `training_a[0..3]`, 6..9 =
`training_b[0..3]`, 10 = `training_a4`), filled when `add = 1`, erased when 0; `3277:0081(bmp, v)`
draws the intensity slider.

Setup: `copy_protection_penalty("Training")`; `p1 = vga_load_picture(1)`, `p25 = vga_load_picture(0x25)`;
exit button `(0x10c, 0xc1, 0, 0x22, rect, 0, 8)`; `3277:0081(p25.bmp, M.training_intensity)`;
`blit(p1, 0,0, rp, 3,0xe, 0x15,0xa5)`, `blit(p1, 0,0, rp, 0x12a,0xe, 0x15,0xa5)`; font 1; for `k = 0..3`:
`blit(p1, 0x15,0, rp, 0x1e, 0x22+0x25k, 0x54,0x15)`, `blit(p1, 0x17,0, rp, 0x70, 0x22+0x25k, 0x52,0x15)`,
`DS:255c[k]` (`Kondition`, `Spiel`, `Schuss`, `Taktik`) centered 0x1d..0xbf at y 0x3e+0x25k colour 0xb;
`blit(p1, 0x15,0, rp, 0xd1, 0x22+0x25k, 0x54,0x15)`, `DS:256c[k]` (`Tor`, `Abwehr`, `Mittelfeld`, `Angriff`)
centered 0xd0..0x122 colour 0xb. Row 4: blits at y 0xbc, `"Intensit{t"` centered 0x1d..0xc1 at y 0xd7.
Bars: `0225(bmp, 2+k, 1, training_a[k], 1)`; `freeA = 01a9(0)`, `0225(bmp, 0, 1, freeA, 1)`;
`0225(bmp, 6+k, 1, training_b[k], 1)`; `freeB = 01a9(1)`, `0225(bmp, 1, 1, freeB, 1)`;
`0225(bmp, 10, 1, training_a4, 1)`. Effect box: `ui_fill_rect(3,0xb7,0x16,0xe8, 7)`, frame colour 1;
`1f9d:1122(&e0, &e1, &e2, &e3)`; `sum = e1 + e0`, if 0 then 1; font 2; for `s = 0..2`: label `DS:2580[s]`
(`K`, `T`, `E`) at `(6s+5, 0xe6)` colour `s+0xd`; `v = s<2 ? (100*e_s)/sum : e3 - 50` (i32);
`h = 38 - v*38/100`, `if (h > 38) h = 38`; `ui_fill_rect(rp, 6s+5, 0xb9+h, 6s+8, 0xdf, s+0xd)`; frame colour 0.
Fade in.

Loop until the exit button (`hit == 1`); only clicks (`FAR:2ea2 == 1`) are handled:
```c
if (0xd0 < mx && mx < 0x105 && 0xc0 < my && my < 0xe9) {     /* intensity slider */
    int v = mx - 0xd9; clamp_int(&v, 0, 0x22);
    M.training_intensity = (u8)v; 3277_0081(bmp, v);
}
for (k = 0; k < 4; k++) {
    bar = -1;
    if ((k*0x25+0x21 < my && my < k*0x25+0x33) || (0xbc < my && my < 0xce && mx < 0xbe)) {
        row = (my - 0x21) / 0x25;  my = row;                     /* my is overwritten: later k never match */
        if (0x1e < mx && mx < 0xbe) {                            /* group A */
            bar = row + 2; if (row > 3) { bar = 10; row = 4; }
            i8 v = (i8)(abs(mx - 0x1e) >> 4 with sign) + 1;      /* = (mx-0x1e)/16 + 1, 1..10 */
            u8 cur = M.training_a[row];                          /* row 4 = training_a4 */
            if ((int)cur - v + freeA >= 0 || row == 4) {
                if (cur > v) { from = v + 1; to = cur + 1; add = 0; } else { from = cur; to = v; add = 1; }
                M.training_a[row] = v; 3277_0225(bmp, bar, from, to, add);
            } else goto next_k;                                  /* rejected: no free points */
        }
        if (0xd1 < mx && mx < 0x121) {                           /* group B, same row */
            bar = row + 6; i8 v = (mx - 0xd1)/16 + 1; u8 cur = M.training_b[row];
            if ((int)cur - v + freeB < 0) goto next_k;
            if (cur > v) { from = v + 1; to = cur + 1; add = 0; } else { from = cur; to = v; add = 1; }
            M.training_b[row] = v; 3277_0225(bmp, bar, from, to, add);
        }
    }
    if (bar != -1) {                                             /* refresh the free-point bar */
        if (bar < 6) { d = 3277_01a9(0); if (d < freeA) { from=d; to=freeA; add=0; } else { from=freeA; to=d; add=1; } freeA = d; }
        else         { d = 3277_01a9(1); if (d < freeB) { from=d; to=freeB; add=0; } else { from=freeB; to=d; add=1; } freeB = d; }
        3277_0225(bmp, bar > 5, from, to, add);                 /* bar 10 refreshes group B (bug, keep) */
    }
next_k: ;
}
```
`from/to` of a "cur > v" update use `cur + 1` (the byte increment of `cur`). Exit:
`ui_wait_with_highlight(0x10c, 0xc1)`, free both pictures. The intensity is the only finance-relevant
output (monthly cost `625*(intensity+1)`, `fatigue_sum`).

### 2.11 212a:00b4 `count_players_of_owner` (was FUN_212a_00b4)

`u8 count_players_of_owner(u8 owner)`. Callers: `season_end_messages` 1cb5:0012, `transfer_list_add`
2ecd:37d8. For `s = 0..4` with `s < DS:07ab || s == 4`: for `i < squad_count(s, 0)`: count the player if
`s == owner`, or if `FAR:57fe[0x25 * FAR:7759[0x34*(25*s+i)]] == owner` (the owning manager of the player's
club). Returns the `u8` count.

### 2.12 212a:314d `season_goal_messages` (was FUN_212a_314d)

`void season_goal_messages(void)`, called by `office_menu_dispatcher` 18bc:0b5f. No RNG, no money.
`L = M.league`, `n = (i8)DS:225e[L]` (teams in the league), `own = M.club`, `PTS(t) = FAR:0ecc[0x36t] +
FAR:0ecd[0x36t]`, `LEFT(t) = ((i8)DS:2262[L] - FAR:0eea[0x36t] - FAR:0eeb[0x36t]) * (i8)DS:2271`
(remaining games times points per win), `RANK(r) = FAR:535a[20L + r]` (team at table rank `r`).
`flag4 = (cur_div == 4L)`.

1. Relegation notice: if `DS:226a != 0 && month == 5 && day == 13 && DS:225a[L] - DS:2262[L] == 1` and
   (`L == 0 && M.round_position[DS:225a[0]] == 15` or `L == 1 && M.round_position[DS:225a[1]] == 2`):
   `dialog_okay("Ihre Mannschaft", "bestreitet das", "Relegationsspiel.", "Viel Gl}ck.")` and return.
2. If `month == 5 && day > 13` return.
3. If `day == 2 + flag4 && month == 11`: `dialog_okay("Achtung !", "Dies ist der letzte", "Spieltag vor der", "Winterpause.")`.
4. Rank thresholds `rk[3]`: `rk[0] = 0`; `L == 0`: `rk[1] = 4`, `rk[2] = n - flag4 - 4`; `L == 1`:
   `rk[1] = 2*flag4 + 1`, `rk[2] = n - 5`; `L == 2`: `rk[1] = 3`, `rk[2] = n - 5`. `p[k] = PTS(RANK(rk[k]))`,
   `mine = PTS(own)`, `left = LEFT(own)`.
5. For `k = 0..2`:
   * if `p[k] > left + mine` and bit `k` of `FAR:4bec[m]` is clear: set it; lines `club_name(own) + " hat"`,
     `"die letzte Chance verspielt,"`, and `k=0` `"MEISTER zu werden."`, `k=1` (`L==0`
     `"den UEFA-CUP zu erreichen."` else `"AUFZUSTEIGEN."`), `k=2` `"DIE KLASSE ZU ERHALTEN."`; if
     `L == 0 || k != 0`: `dialog_okay(M.name, line1, line2, line3)`.
   * `best = max(0, max over r = rk[k]+1 .. n-1 of LEFT(RANK(r)) + PTS(RANK(r)))`; if `best < mine` and
     bit `k+3` is clear: set it; lines `club_name + " hat"`, (`k=0` `"die MEISTERSCHAFT"`, `k=1` `L==0` ?
     `"den UEFA-CUP-PLATZ"` : `"den AUFSTIEG"`, `k=2` `"den KLASSENERHALT"`), `"auf jeden Fall gesichert."`;
     if `cur_mul != cur_div` swap lines 2 and 3; if `L == 0 || k != 0` show with `dialog_okay`.

## 3. Segment 1000

### 3.1 1000:0602 `screen_stadium` (was FUN_1000_0602)

`void screen_stadium(void)`. Office menu 1 case 4. Callees: 16c7:09f9, 1000:16e3, 16c7:00a7, 176b:0c91,
1000:1a49, 16c7:09b0, 176b:1416, 16c7:0002, 1000:243f, 176b:0e57, 16c7:083c, 16c7:087c, 16c7:07d5,
4091:0ba5, 1000:0000, 4930:052d, 176b:0319, 4091:1722, CRT.

Setup: `copy_protection_penalty("Stadion")`; `stadium_draw_picture(6, 0xf)`; frames
`ui_draw_button(rp, 0x96,0xf, 0x8b,0x52, rect, 0, 8)`, `(6,0x68, 0x11b,0x52)`, exit `(0x10c,0xc1, 0,0x22)`
(all into the same `rect`, so only the exit rect survives); font 2; `stadium_draw_status_panel(0)`;
line `(0x97,0x54)-(0x120,0x54)` colour 0xb; fade in.

Loop until `hit == 1` (`hover = -1`):
```c
hit = ui_hit_test_buttons(rect, 1, 1); mx = ptr[1]; my = ptr[0];
if (6 < mx && mx < 0x121 && 0x72 < my && my < 0xb9) {
    aa = (my - 0x72) >> 3 (sign-symmetric);            /* 0..8 */
    if (aa != hover) { status_line_show(aa, 0); hover = aa; }   /* DS:2818: "", "SITZPL[TZE AUSBAUEN", ... "EINTRITTSPREIS [NDERN" */
    if (FAR_2ea2 == 1 && aa != 0) handle_row(aa);       /* below */
}
/* 1000:1688, reached on every pass except after the "keine Baufirma" dialog: [bp-0xae] holds the row
   when the pointer is in the table, else the x coordinate (also after the level selection loop of
   rows 4..7, which reloads it with the pointer x); a quirk of the original, keep. */
if (hover != bp_ae && hover != -1) { status_line_show(0xff, 0); hover = -1; font 2; }
```
`handle_row(aa)`:
1. If `FAR:577f[aa] != 0`: `ui_dialog_box("Im Moment keine", "Baufirma aufzutreiben.")`, done.
2. `cap_left = 130000 - S.standing - S.seats_pend - S.standing_pend - S.seats` (i32; constant `0x1FBD0` =
   130000, loaded as `MOV AX,0xfbd0; MOV DX,1` at 1000:0820, then SUB/SBB in the order
   `S+8, S+4, S+0xC, S+0`); if `aa == 3`:
   `cap_left = S.standing - S.covered_pend - S.covered + S.seats`.
3. `ui_fill_rect(0x97,0x10,0x120,0x5f, 8)`, font 2, `DS:2818[aa]` centered 0x97..0x120 at y 0x16 colour 0xb,
   line y 0x18 colour 0xb. `"KOSTET " + [4f78] + display(price[aa])` + (`aa<4` `" DM PRO 1000"`, `aa<8`
   `" DM PRO PUNKT"`) at (0x9a, 0x49) colour 1; for `aa == 8` the whole line is the empty string.
4. Rows 1..3 (`aa < 4`): `afford = (M.balance / price) * 1000`, `< 0 → 0`; if 0:
   `ui_dialog_box("Kein Geld f}r", "neue Pl{tze da...")`, result -1, goto 7. `max = min(cap_left, afford)`;
   `"MAX. GR|~E: " + max` at (0x9a,0x50) colour 1; `" KOSTEN: " + [4f78] + display(price * (max/1000)) + " DM."`
   at (0x9a,0x57).
5. `stadium_draw_status_panel(1)`, line y 0x42 colour 0xb. Then:
   * `aa` 1..3: `amt = ui_input_amount(0, 0xb8, 0x1b, 0, max, 1000)`; if `amt != -1`:
     `res = stadium_expansion_order(aa, amt, (amt/1000) * price)`.
   * `aa == 8`: `step, k = (cur_div > 1L) ? (5, 10) : (1, 1)`; `t = cur_mul * k`;
     `v = ui_input_amount((M.ticket_price * cur_mul * k) / cur_div, 0xb8, 0x1b, t*5/cur_div, t*25/cur_div, step)`;
     if `v != -1`: `M.ticket_price = (u8)(((v * cur_div) / cur_mul) / k)`; `res = v`.
   * `aa` 4..7: `cur = (u16)(S.cur[aa-1] + S.pend[aa-1])` (low words); `top = (aa > 5) ? 6 : 3`; if
     `cur == top`: `ui_dialog_box("Was denn...?", "NOCH besser ???")`, res -1. `afford = M.balance / price`;
     if `afford <= 0`: `ui_dialog_box("Da m}ssen Sie erst", "noch sparen...")`, res -1. `lim = min(top,
     afford) + cur`, `if (lim > top) lim = top`. With `DS:079c = 9` list the options, `aa < 6`: levels
     `l = 1..3` at y `7l+0x19`, names `DS:2858[l]`; `aa ≥ 6`: `l = 2..6` at y `7l+0x12`, names `DS:283c[l]`;
     name at x 0x9a, colour `(l > cur && l <= lim) ? 1 : 3`; if `l > cur || cur == 0`:
     `[4f78] + display((l - cur) * price) + " DM"` at x 0xd8. `"MAX. STATUS: " + name(lim)` at (0x9a,0x50),
     `"(KOSTEN: " + [4f78] + display((lim - cur) * price) + " DM)"` at (0x9a,0x57). Selection loop in XOR
     draw mode (mode 2): poll mouse; `FAR:2ea2 == 3` cancels; hovering `0x96 < mx < 0x120`, `0x19 < my <
     0x42` selects `row = (my - 0x1c)/7` (≤ 2 or ≤ 4) and XOR-fills `(0x97, (row+4)*7, 0x11f, 7row+0x20)`;
     a click on a row with `level = row + (aa>5) + 1`, `cur < level <= lim` orders
     `res = stadium_expansion_order(aa, level, (level - cur) * price)`. Draw mode back to 0.
6. —
7. `ui_fill_rect(0x97,0x10,0x120,0x5f, 8)`; `stadium_draw_status_panel(res == -1 ? 1 : 0)`; line y 0x54;
   `hover` row highlight reset.

Exit: `ui_wait_with_highlight(0x10c, 0xc1)`.

### 3.2 1000:0000 `stadium_expansion_order` (was FUN_1000_0000)

`int stadium_expansion_order(i8 aa, i32 amount, i32 cost)`, returns 1 (ordered) or -1. Callers:
1000:0602 (2×). Callees: random_range, CRT, 176b:0e57, 16c7:083c, 176b:0c91, 176b:0681, 16c7:087c,
4091:123d, 176b:0319, 4091:11cc.

```c
int stadium_expansion_order(i8 aa, i32 amount, i32 cost) {
    i32 *it = &S[aa - 1][0];                                  /* jump table 1000:0089 */
    if (M.balance - cost < 0) { ui_dialog_box("Zu wenig", "Kleingeld vorhanden..."); return -1; }
    i32 total = it[1] + it[0];
    i32 rest  = (i32)(u16)M.build_days[aa-1] * random(20, 80) / 100;      /* RNG */
    i32 days  = ((i32)(7 * DS_0063[aa]) + rest) * random(80, 120) / 100;   /* RNG */
    ui_fill_rect(0x97,0x1b,0x120,0x5f, 8); font 2; DS_07b2 = 0;
    if (aa < 4) { l1 = "AUSBAU UM " + fmt(amount) + " AUF "; l2 = fmt(amount + total) + " " + DS_2868[aa]; }
    else if (aa < 6) { l1 = "NEUE GR|~E: ";   l2 = DS_2858[amount]; }
    else             { l1 = "NEUER STATUS: "; l2 = DS_283c[amount]; }
    centered 0x97..0x120: l1 y 0x25 colour 2 shadow 0xff; l2 y 0x2e colour (aa<4 ? 2 : 1) shadow 0xff;
    " KOSTEN: " + [4f78] + display(cost) + " DM" y 0x37 colour 2;
    (rest == 0 ? "BAUZEIT: CIRKA" : "BAUZEIT: INSGESAMT CA.") y 0x40 colour 2;
    fmt(days / 7 + 1) + " WOCHEN" y 0x49 colour 1 shadow 0;
    DS_07b3 = 0;
    ui_register_icon_slot(0x9e, 0x51, "NA KLAR !", 0x12);
    ui_register_icon_slot(0xe3, 0x51, "ACH NEE...", 1);
    do { input_poll_mouse(ptr); r = ui_hit_icon_slot(); } while (!(FAR_2ea2 == 1 && r != 0));
    if (r == 2) { FAR_577f[aa] = random(15, 55); return -1; }   /* RNG; declining locks this item */
    M.balance -= cost;
    it[1] += amount;
    if (aa > 3) it[1] -= total;            /* levels: pending becomes target - current - old pending */
    M.build_days[aa-1] = (u16)days;
    return 1;
}
```

### 3.3 1000:16e3 `stadium_draw_picture` (was FUN_1000_16e3)

`void stadium_draw_picture(int x, int y)`; caller 1000:0602 with `(6, 0xf)`.
`cap = S.seats + S.standing`; `size = (cap > 34000) + (cap > 14000) + (cap > 54000)` (i32 compares);
`cond = (u8)((S.condition - 1) >> 1)` (i32 arithmetic shift); `pic = (u8)(3*size - cond + 2 + 9)`.
`b = vga_load_picture(pic)`; `ui_draw_button(rp, x, y, 0x8b, 0x52, tmp, 0, 7)`;
`blit_bitmap(b, 0,0, rp, x+1, y+1, 0x8a, 0x51, 0xc0)`; free `b`. If `S.floodlights != 0 ||
S.scoreboard != 0`: `ov = vga_load_picture(0x28)` and a 0x28×0x28 scratch bitmap.
Floodlights (`f = S.floodlights` low word, nonzero): `v = 3 - f`; for `j = 0..1`:
`ui_blit_transparent(ov, 0x67 + 0x11*(v - 3*cond), 1, rp, DS:0070[2*size+j] + x, DS:0078[size] + y - 1,
0x11, DS:006c[size])`. Scoreboard (`s = S.scoreboard`, nonzero): `sx = (s-1)*0x16 + 0x29`, `sy = 0x43 -
0x21*cond`, `dx = DS:007c[size] + x`, `dy = DS:0080[size] + y`: copy work bitmap `FAR:a6ca` (dx,dy,0x1f,0x14)
to scratch, transparent-blit `ov (sx,sy)` onto scratch, copy scratch back to `FAR:a6ca` at (dx,dy).

### 3.4 1000:1a49 `stadium_draw_status_panel` (was FUN_1000_1a49)

`void stadium_draw_status_panel(u8 balance_only)`; callers 1000:0602 (3×). Font 2, `DS:07b2 = 0`,
`ui_fill_rect(0x9a,0x59,0x120,0x5e, 8)`; `"IHR KONTOSTAND:" + [4f78] + display(M.balance) + " DM"` at
(0x9a,0x5e) colour 1. If `balance_only` return. Else `ui_fill_rect(7,0x69,0x11f,0xb8, 8)`, `"MOMENTAN"`
(0x76,0x6f) and `"NACH AUSBAU"` (0xc0,0x6f) colour 1, and 9 rows `r` at `y = 0x77 + 8r`: label
`DS:2868[r]` (`GESAMTKAPAZIT[T`, `SITZPL[TZE`, `STEHPL[TZE`, `]BERDACHTE PL[TZE`, `FLUTLICHT`,
`ANZEIGETAFEL`, `ZUSTAND`, `KOMFORT`, `EINTRITTSPREIS`) at x 0xa colour 2; `now` at x 0x76 colour 2;
`after` at x 0xc0 (colour 4 and `after = now` when there is no pending change, else colour 2) and, for
`r != 0` with a change, `fmt(build_days[r-1]/7 + 1) + " WOCHE(N)"` at x 0xf3 (`DS:079c = 2` while
formatting). Values: row 0 `now = seats+standing`, `after = now + seats_pend + standing_pend` only if
`build_days[0] or [1] != 0`; rows 1..3 `now = cur`, `after = cur + pend` if `build_days[r-1] != 0`; rows
4,5 names `DS:2858[cur]` / `DS:2858[cur+pend]` if `pend != 0`; rows 6,7 `DS:283c[...]` likewise; row 8
`[4f78] + fmt(ticket_price * cur_mul * k / cur_div)` (`k = 10` if `cur_div > 1` else 1); when
`cur_div > 1` the code turns the text into a decimal by `buf[4] = buf[3]; buf[3] = '.'; buf[5] = '0';
buf[6] = 0`, then appends `" DM"`.

### 3.5 1000:20e1 `stadium_construction_daily` (was FUN_1000_20e1)

`void stadium_construction_daily(void)`; caller 212a:0a6d (daily, per manager). Callees: CRT, 18bc:3b92,
4091:185c, 4091:0190, 4091:0044, 176b:1416, 16c7:0563, 1000:239e. No RNG.

```c
for (i = 0; i < 7; i++) {
    if (FAR_5780[i] != 0) FAR_5780[i]--;                 /* global lock, ticks once per manager per day */
    if (M.build_days[i] == 0 || --M.build_days[i] != 0) continue;
    l2 = ""; l3 = ""; named = 0;
    switch (i) {                                         /* jump table 1000:21d2 */
      case 3: l2 = "Flutlichtmasten";  named = 1; break;
      case 4: l2 = "Anzeigetafel";     named = 1; break;
      case 6: l2 = "Stadion-Komforts"; named = 1; break;
    }
    S[i].cur += S[i].pend; S[i].pend = 0;
    if (named) l3 = "ist abgeschlossen."; else l2 = "ist abgeschlossen.";
    l1 = DS_28d8[i];  /* "Der Ausbau der Sitzpl{tze", "Der Ausbau der Stehpl{tze", "Die ]berdachung des Stadions",
                         "Die Errichtung der neuen", "Die Errichtung der neuen", "Die Stadionrenovierung", "Die Verbesserung des" */
    i32 day = DS_07dc; int A = (i8)DS_224d*7 + 0x80, B = 0xcf - 14*(i8)DS_224d;
    if ((A < day && day < B) || day > 0x142) {           /* show now */
        office_show_cached_background(); ui_draw_manager_header(); FAR_5358 = m;
        message_find_owner(message_box(1, 5L, l1, l2, l3, 0, 0, 0, 0));
        screen_fade(pal, 0); ui_redraw_buttons(0x7f, 0);
    } else message_queue_add(l1, l2, l3, 1, 0);          /* 1000:239e */
}
```

### 3.6 1000:239e `message_queue_add` (was FUN_1000_239e)

`void message_queue_add(char far *s1, char far *s2, char far *s3, u8 row, u8 kind)`; callers 1000:20e1,
`random_weekly_events` 1cb5:13bd. `FAR:5358 = m`; `slot = team_slot_free_index(row)`;
`p = message_box(kind, (i32)(3*kind + 2), s1, s2, s3, 0, 0, 0, 0)`; if `slot != 0x7f`:
`FAR:5664[10*row + slot] = p` (far ptr, block 45) and `DS:1d34[10*row + slot] = 3`; else
`message_find_owner(p)`.

### 3.7 1000:272d `sponsor_subsidy_offer` (was FUN_1000_272d)

`void sponsor_subsidy_offer(i32 base)`; caller `screen_transfer_market` 3277:04a5.
```c
if (random(0, 6) != 0) return;                               /* RNG */
i32 amt = ((base / 100) * random(20, 65) / 10000) * 10000;    /* RNG */
DS_07b2 = 0;
text = [4f78] + display(amt) + " DM AN ?";
if (message_broadcast(m, "NEHMEN SIE EINEN SPONSOR-", "ZUSCHU~ VON", text, "ABER IMMER!", "LIEBER NICHT."))
    M.balance += amt;
```

### 3.8 1000:243f `status_line_show` (was FUN_1000_243f; FUNCTION_MAP: *credit_offer_text*)

`void status_line_show(i8 id, i32 value)`; callers 1000:0602, 212a:0145, 212a:167f. Font 1. If
`DS:0044 != -1` restore the old strip: `blit_bitmap_scaled_or_copy(FAR:56fa, DS:0044, 0xd9, FAR:a6ca,
DS:0044, 0xd9, DS:0046, 0xa, 0xc0, 7, 0, 0)`. If `id == -1` return. Text:
* `0 ≤ id ≤ 9`: `DS:2818[id]` (stadium row help);
* `10..14`: `DS:28d0[id]` = `DS:28f8[id-10]` (`Erholungswert`, `Fitnessausr]stung`, `Platz-Qualit[t`,
  `Raumausstattung`, `Verpflegung`);
* `15..22`: `DS:2358[id]` = camp name, and if `value != 0`: `+ ", " + [4f78] + fmt(value) + " DM/Woche."`;
* `23`: `value < 50` → `"Kredit zu " + fmt(value) + "% aufnehmen."`; else `"Kredit von " + (char far*)value
  + (DS:224d == 0 ? " aufnehmen." : "")`;
* `> 23`: `DS:28ac[id]` (24..26 = `"Wie hoch soll der Kredit sein ?"`, `"Laufzeit des Kredites (Mon.) ?"`,
  `"Prozentsatz des Kredites ?"`).
`DS:0046 = text_width`. Ids 24..26: `DS:0044 = 2`, `ui_draw_text_at(rp, text, 2, 0xe1, 1)`. Others:
`DS:0044 = 0x85 - width/2` (signed `/2`), `ui_draw_text_centered_shadow(rp, text, 0, 0x10a, 0xe1, 1, 0)`.

### 3.9 1000:2895 `fixture_week_of_nth_match` (was FUN_1000_2895)

`u8 fixture_week_of_nth_match(u8 mask, i8 n)`: `w = 0; while (n != 0) { if (DS:2280[w] & mask) n--; w++; }
return w - 1;`. Callers 1000:28c4, `screen_fixtures` 3a41:120a.

### 3.10 1000:28c4 `screen_team_fixtures` (was FUN_1000_28c4)

`u8 screen_team_fixtures(u8 team, u8 right, i8 offset)`; caller 3a41:000e (opponent info). Returns the
clicked icon id (0..2: `REICHT MIR`, `INFO`, round toggle). Read-only except two side effects below.
`T = league_of_rank(team) >> 1` (27c2:1bb5), `round = DS:225a[T] + offset`, `second = round >
DS:2262[T] >> 1`. Frame x `SI = right ? 0x5b : 0x59`, `DI = 0x37`:
`ui_fill_rect(SI+1, DI+1, SI+0xdf, DI+0xad, 8)`; title `("R}ck" or "Hin") + "spiele " + club_name(team)`
(word order swapped when `cur_mul == 3`) centered SI..SI+0xe0 at y 0x3f colour 0xb; line y 0x41;
icons `"REICHT MIR"` (SI+4, 0xd6, id 10), `"INFO"` (SI+0x54), `"HINRUNDE "`/`"R]CKRUNDE "` (SI+0xa4)
(the other half); headers `GEGNER`, `PLATZ (ST[RKE)`, `DATUM` (centered), `ERGEBNIS` at y 0x48. For each
round of the half, after `league_team_range(T, r)` (3a41:10e3), every pair `FAR:4b5e[20T + 2j]` /
`+1` containing `team` prints `"(H) "`/`"(A) "` + opponent name, `3a41:1d18` rank/strength, and either the
result `FAR:6dc8[20*(0x26T + r) + 2j (+1)]` (`a:b`) or, if not played (`== 0x1e`) or in the future, the
date: the week comes from `fixture_week_of_nth_match(league_bit, r)` (or the cup table `FAR:5715`), then
**`matchday_refresh(week)` (1310:1190) is called, which overwrites the date globals `FAR:2eba`/`FAR:2e8e`**,
and `day + "." + (month+1) + "."` is printed. Clicking icon 3 toggles the half; clicking 0..2 clears the
frame, calls `league_team_range(T, DS:225a[T])` and returns the id.

## 4. Monthly income and expenses (segment 24a4, spec'd here; Ghidra names `finance_weekly_income` / `finance_weekly_expenses`, set by another agent; they run **monthly**, not weekly)

### 4.1 24a4:2636 `finance_monthly_income` (Ghidra `finance_weekly_income`)

`i32 finance_monthly_income(void)`; callers 212a:0a6d, 24a4:2b15, `screen_statistics` 362f:0af7.

```c
i32 finance_monthly_income(void) {
    u16 players = squad_count(m, 0);
    u8  c = M.club;
    i32 str = 0;
    for (k = 0; k < 3; k++) str += FAR_307e[0x22*c + k] + FAR_3081[0x22*c + k];   /* club strength now + base */
    str *= 15;                                                  /* _aFlmul_assign */
    i32 inc = str * (i32)players / 3 + (u16)M.fans / 5 + 25;   /* unsigned DIV for fans */
    M.income_base = (u16)inc;
    for (i = 0; i < 8; i++) inc += DS_066c[m][i];
    FAR_5358 = m;
    for (FAR_304a = 0; FAR_304a < DS_07ab; FAR_304a++)
        if (FAR_304a != FAR_5358) inc += credit_sum(1, 99);    /* ALL interest paid by every other manager */
    FAR_304a = FAR_5358;
    if (DS_0644[m] > 0) inc += (DS_0644[m] / FAR_2eba) / 75;   /* interest on a positive average balance */
    return inc < 0 ? 0 : inc;
}
```
The interest loop credits manager `m` with every interest payment of the other managers, including
those to the bank and to third managers. Keep it.

### 4.2 24a4:2822 `finance_monthly_expenses` (Ghidra `finance_weekly_expenses`)

`i32 finance_monthly_expenses(void)`; same callers.

```c
i32 finance_monthly_expenses(void) {
    i32 e = 150000;                                              /* 0x249f0 */
    for (i = 0; i < squad_count(m, 0); i++) e += *(i32*)&FAR_774a[0x34*(25*m+i) + 0x28];   /* salaries */
    for (i = 0; i < squad_count(4, 0); i++) {                    /* pool players owned via club */
        u8 *p = &FAR_8b9a[0x34*i];
        if (FAR_57fe[0x25 * p[0x0f]] == m) e += *(i32*)&p[0x28];
    }
    i32 x = S.comfort * 3000 + (S.floodlights + S.scoreboard) * 1375 + (i32)(3 - M.league) * 43750 + S.seats;
    x = x * 2 + S.standing;
    x = x * 2 + S.covered;
    e += x;                                                      /* = 4 seats + 2 standing + covered + ... */
    e += (u16)M.extra_costs;
    e += (((i32)M.training_intensity + 1) * 10000 >> 4) + DS_066c[m][8];
    e += credit_sum(1, 99);                                      /* own interest */
    if (DS_0644[m] < 0) e += (DS_0644[m] / FAR_2eba) / -10;      /* 10 % of a negative average balance */
    if (M.league != 2 && M.balance > 2000000) {
        i32 t = (M.balance - 2000000) / 500000 * 30000 + 20000;
        if (M.balance >= 4000000) t = t * 3 / 2;
        e += t;
    }
    return e < 0 ? 0 : e;
}
```
All stadium fields are the `current` values (`S+0`, `S+8`, `S+0x10`, `S+0x18`, `S+0x20`, `S+0x30`); the
condition item `S+0x28` costs nothing. Ticket income is not computed here (see Open questions).

## 5. Function index

| Address | Name (Ghidra) | Signature | Kind |
|---|---|---|---|
| 212a:000a | interest_rates_update | `void(void)` | logic, RNG |
| 212a:00b4 | count_players_of_owner | `u8(u8 owner)` | logic |
| 212a:0145 | screen_training_camp | `void(void)` | screen + logic |
| 212a:072d | training_camp_apply | `u8(u8 camp)` | logic, RNG |
| 212a:0a6d | finance_daily_processing | `void(void)` | logic, RNG |
| 212a:0f8d | credit_sum | `i32(u8 which, i8 lender)` | logic |
| 212a:1039 | credit_take | `int(i32, i8 rate, i8 lender, i8 months)` | logic |
| 212a:119b | credit_list_draw | `void(i8 lender)` | UI |
| 212a:167f | screen_credits | `void(void)` | screen + logic |
| 212a:23cb | credit_summary_draw | `void(void)` | UI |
| 212a:274c | screen_training | `void(void)` | screen + logic |
| 212a:314d | season_goal_messages | `void(void)` | logic + UI |
| 1000:0000 | stadium_expansion_order | `int(i8 aa, i32 amount, i32 cost)` | logic, RNG |
| 1000:0602 | screen_stadium | `void(void)` | screen + logic |
| 1000:16e3 | stadium_draw_picture | `void(int x, int y)` | UI |
| 1000:1a49 | stadium_draw_status_panel | `void(u8)` | UI |
| 1000:20e1 | stadium_construction_daily | `void(void)` | logic |
| 1000:239e | message_queue_add | `void(s1, s2, s3, u8 row, u8 kind)` | UI/message |
| 1000:243f | status_line_show | `void(i8 id, i32 value)` | UI |
| 1000:272d | sponsor_subsidy_offer | `void(i32 base)` | logic, RNG |
| 1000:2895 | fixture_week_of_nth_match | `u8(u8 mask, i8 n)` | logic |
| 1000:28c4 | screen_team_fixtures | `u8(u8 team, u8 right, i8 offset)` | screen |
| 24a4:2636 | finance_weekly_income (misnamed; suggested *finance_monthly_income*) | `i32(void)` | logic (outside segment) |
| 24a4:2822 | finance_weekly_expenses (misnamed; suggested *finance_monthly_expenses*) | `i32(void)` | logic (outside segment) |

## 6. Globals used

| Address | Size/type | Suggested name | Save block |
|---|---|---|---|
| FAR:0000 + 2m | u8 | `sponsor_contract_months[m]` (countdown; 0 → contract income /10) | 37 |
| FAR:0ecc + 0x36t | struct | team season stats (points, games) | 16 |
| FAR:2242 + 0x30a*m | struct | manager record (§1.1) | 13 |
| FAR:2e8e | i32 | `month` (0-based) | 21 |
| FAR:2eba | i32 | `day_of_month` | 20 |
| FAR:2ea2 | u16 | `mouse_event` | — |
| FAR:304a | i8 | `current_manager` | 3 |
| FAR:3066 + 0x22c | struct | club record (`+0x18..0x1d` strength) | 15 |
| FAR:4b5e | u8[] | fixture pairs | 19 |
| FAR:4b9a + 2*(6m+i) | u8 | `ad_contract_months[m][i]` | 38 |
| FAR:4bec + m | u8 | `goal_message_flags[m]` (bits 0..5) | — (not in the save; see Open questions) |
| FAR:535a + 20L + r | u8 | `league_table_order[L][r]` | 32 |
| FAR:5358 | i8 | `saved_manager` | 4 |
| FAR:5664 | far ptr[3][10] | queued messages | 45 |
| FAR:577f + aa (= 5780[aa-1]) | u8[7] | `construction_lock_days[7]` | 46 |
| FAR:57fe + 0x25p | u8 | owning manager of player `p`'s club | 17 |
| FAR:6dc0 | far ptr | pointer state (`[0]` y, `[1]` x) | — |
| FAR:6dc8 | u8[] | match results | 6 |
| FAR:774a + 0x34*(25g+i) | struct | player records (`+0x0e`, `+0x10..0x13`, `+0x28` salary i32) | 18 |
| FAR:a7a0 | i32 | `year` | 22 |
| FAR:2f24, 2f28, 56fa, a6ca, 2e70 | far ptr | picture, rastport, work bitmap, palette | — |
| DS:0044 / 0046 | i32[8] / words | stadium unit prices; also status-line x / width | — |
| DS:0063 | u8[8] | `stadium_build_weeks` | — |
| DS:006c..0083 | u8[] | stadium overlay layout | — |
| DS:05ac | u8 | `copy_protection_passed` | — |
| DS:060e | u8[6] | `bank_interest_rates` | 0 |
| DS:0620 | i16[8] | `camp_availability` | 51 |
| DS:0630 | i16 | `base_interest_rate` | — (not saved) |
| DS:0644 + 4m | i32 | `month_balance_sum[m]` | — (not saved) |
| DS:066c + 0x24m + 4i | i32[9] | `contract_income[m][i]` (`[8]` = cost item) | 41 |
| DS:079c | u8 | number-format mode flag (set 2/9/1 around formatting) | — |
| DS:07ab | u8 | `human_managers` | 7 |
| DS:07b2 | u8 | `format_no_separator` | — |
| DS:07b3 | u8 | `icon_slot_count` | — |
| DS:07b8 | i8[12] | `days_in_month` | — |
| DS:07dc | i32 | `day_counter` | 23 |
| DS:1d34 + 10row + slot | u8 | message slot state | 44 |
| DS:224d | i8 | currency-prefix flag / season offset | — |
| DS:224e | i32 | `cur_div` (difficulty/currency long; 4 is special) | — |
| DS:2252 | i32 | `cur_mul` | — |
| DS:225a, 225e, 2262, 226a, 2271 | i8 | league round counters, team counts, rounds, relegation flag, points per win | 34 / — |
| DS:2280 | u8[] | per-week competition mask | 50 |
| DS:4a28 | i8 | `difficulty` (5 − user level) | 42 |
| DS:5256 | u8 | `quarter_flag` (set at month end when month % 4 == 0) | — |
| DS:5258 | i32[8] | `camp_prices` | — |
| DS:527e, 52a6, 5278, 29a4 | tables | camp quality, weights, colours, letters | — |
| string tables | far ptr[] | DS:2818, 28d0, 2358, 28ac, 2868, 2878, 283c, 2858, 28d8, 255c, 256c, 257c, 2534, 2538, 2540, 2548, 2550, 258c, 4c80, 4ce0, 4d44, 4d58, 4d8c, 4dc0, 4e50, 4eb0, 4ec0, 4f64, 292c | — |

## 7. Open questions

1. **Ticket income.** No function in this area turns attendance × `M.ticket_price` into money; the match
   income is presumably booked in `FUN_2a58_20b2` (match area, reads FAR:234c). The monthly "base
   income" `15*str*players/3 + fans/5 + 25` (stored in `M.income_base`) may be what the statistics
   screen calls merchandising/attendance; its meaning is unconfirmed.
2. `DS:0630`, `DS:0644`, `DS:5258`, `FAR:4bec` are not in the 61 save blocks: after loading a game the
   base interest rate, the month's balance sum, the scaled camp prices and the goal-message flags come
   from whatever the running program holds (a fresh start: 5, 0, unscaled 5..15, 0). Verify against
   the loader before relying on it.
3. `FAR:577f+aa` (lock) and `DS:0620` (camp counters) tick once per manager per day, so their speed
   depends on the number of human managers. Assumed intentional-by-accident; kept as is.
4. `screen_credits` keeps the return value of `dialog_okay`/`ui_dialog_box` in the same register as the
   hit-test result; the screen closes if that value equals `DS:07ab + 1`. The return values of
   4091:0e3a and 176b:0e57 are not specified here.
5. The credit-strip toggle fires when the click is inside x 0x84..0x91 but **outside** both y ranges
   (`a - b == 0`). It looks inverted but is what the code does.
6. `screen_team_fixtures` leaves `FAR:2eba`/`FAR:2e8e` set to the last printed match date (via
   1310:1190). If the caller does not restore them, finance day/month tests on the same day see the
   wrong date. Check 3a41:000e.
7. `training_camp_apply`: `random(0, 20*t/30)` and `random(0, t)` with `t < 0` give a non-positive
   modulus in `random_range`; behaviour then depends on the MSC `%` (and `t == -1` → modulus 0 → divide
   error). Not observed; needs a decision for the port.
8. `DS:066c[m][0]` was called "salary" in state.md; here it is added to the monthly **income** and cut to
   1/10 when `FAR:0000[2m]` expires, which fits a sponsor contract better, so state.md and the port now
   call it main-sponsor income. The owner of `screen_manager_contract` 362f:2be8 should confirm the name.
