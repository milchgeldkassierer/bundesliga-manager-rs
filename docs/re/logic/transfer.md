# Transfer market, offers and contract negotiation (segment 3277)

This spec covers the whole code segment `3277` of `BMMAIN.EXE` (Ghidra `/unpacked/BMMAIN_unpacked.EXE`,
range 3277:000a-3277:3b83, 11 functions, no jump tables, no gaps). It is written so that the logic can be
ported bit-exactly without Ghidra. Segment 3277 also holds three small helpers of `screen_training`
(212a:274c). They are specified here as well (§4.2-4.4).

All function names below were set in Ghidra by this work (they were `FUN_3277_xxxx` before).

| Address | Ghidra name | Kind | Callers |
|---|---|---|---|
| 3277:000a | `player_init_random_attrs` | logic (RNG) | 1cb5:13bd `random_weekly_events`, 212a:072d, 2ecd:37d8 `transfer_list_add` |
| 3277:0081 | `training_draw_cost_bar` | UI | 212a:274c `screen_training` (x2) |
| 3277:01a9 | `training_free_slots` | logic | 212a:274c (x4) |
| 3277:0225 | `training_draw_slot_bar` | UI | 212a:274c (x8) |
| 3277:04a5 | `screen_transfer_market` | screen + logic (RNG) | 18bc:1b09 (office menu 2, case 3; jump table, no Ghidra xref) |
| 3277:1b5f | `transfer_offer_received` | dialog | 3277:0de2, 3277:12c8 (both in `screen_transfer_market`) |
| 3277:1e38 | `transfer_market_refresh` | logic (RNG) | 18bc:08c2 `main` (new game), 2a58:3b81 `season_main_loop` |
| 3277:2171 | `market_fee_offer_accepted` | logic (RNG) | 3277:16db (`screen_transfer_market`) |
| 3277:2270 | `contract_demand_accepted` | logic (RNG) | 3277:38b9 (`contract_negotiation`) |
| 3277:25de | `player_value_calc` | logic (RNG) | 15 sites: 18bc:3609, 24a4:18c7, 24a4:1928, 1cb5:0556, 1cb5:1160, 1cb5:15e5, 1cb5:1f74, 2ecd:3a33, 3277:2032, 3277:34bf, 3277:24f6, 422b:2ab6, 3277:0d87, 3277:125d, 3277:1421 |
| 3277:2a8f | `contract_negotiation` | screen + logic (RNG) | 1cb5:110c (`FUN_1cb5_0012`), 2ecd:2363 (`screen_lineup` 2ecd:18bf), 3277:1768 |

## 1. Conventions

* `i8/u8/i16/u16/i32/u32` are exact C widths. The compiler is MSC large model, so `int` is `i16`.
* Long arithmetic goes through the CRT helpers. Their semantics must be reproduced exactly:
  * `crt_aflmul(a,b)` (4a01:1ae6): `i32` multiply, low 32 bits kept (wrapping).
  * `crt_afldiv(a,b)` (4a01:1a4c): signed `i32` division truncating toward zero.
  * `crt_afuldiv(a,b)` (4a01:1c10): unsigned `u32` division.
  * `crt_aflmul_assign(&a,b)` / `crt_afldiv_assign(&a,b)` (4a01:1bf0 / 1bd0): `a = a*b` / `a = a/b`
    (signed), and they also return the new value in DX:AX.
  * `crt_aflshl(a,n)` / `crt_aflshr(a,n)` (4a01:1bb8 / 1bc4): 32-bit shift; `aflshr` is arithmetic.
  * In the pseudo code, `/` on `i32` is truncating signed division and `*` wraps at 32 bits unless noted.
* Long helper argument order: the code pushes the right operand first and the left operand last
  (`push divisor; push dividend; call afldiv`), so `afldiv(x, y) = x / y`. Many expressions push
  several operands in advance; the pseudo code below already resolves the evaluation order.
* `random_range(lo, hi)` (176b:0cc7) is `lo + (i16)(rand() % (i16)(hi - lo + 1))` with a signed 16-bit
  IDIV; `rand` is the MSC LCG (FUNCTION_MAP §3.4). Both arguments are `i16`. A `random_range` call always
  advances the LCG exactly once. Every call is listed in source order below and marked `RNG#n`.
* `clamp_int(&v, lo, hi)` (4091:1009): `if (v < lo) v = lo; if (v > hi) v = hi;` on `i16`.
* `m` = `current_manager` = `FAR:304a` (`i8`, sign-extended when used as an index). `saved_manager` =
  `FAR:5358`. Many routines temporarily set `FAR:304a = 4` so that the squad helpers operate on the
  **transfer pool** (squad 4), and then restore `FAR:304a = FAR:5358`.
* Mouse: `FAR:2ea2` is the last button event (`input_poll_mouse`); this segment tests the values 0, 1
  (left click), 3 (used for "show player info", presumably right click) and 0xe8 (see open questions).
  The mouse position struct pointed to by `FAR:6dc0` (far ptr) has **`[0]` = y and `[1]` = x**, and the
  struct pointed to by `FAR:2e70` has `+4` = y and `+6` = x (Amiga `MouseY, MouseX` order). This is the
  reverse of what MEMORY_MAP §3.1 states; the hit tests in this segment only make sense this way.
* Money is displayed through a currency conversion: `display(v) = v * currency_mul / currency_div`
  with `currency_mul = DS:2252` (`i32`) and `currency_div = DS:224e` (`i32`), both 1 in the shipped data,
  and an optional prefix string `DS:4f78 -> DS:3df9` (empty) appended when `DS:224d != 0`. Typed-in
  amounts are converted back with `v * currency_div / currency_mul`. These are signed `afldiv`
  except where `afuldiv` is noted. `format_long_thousands(v)` (176b:0681) writes the decimal string
  with `.` separators (`,` when `DS:224e == 4`) only while `DS:07b2 == 0`; `DS:07b2 = 1` suppresses them.
* German texts are quoted verbatim as stored (game umlaut encoding: `[`=Ä `\`=Ö `]`=Ü `{`=ä `|`=ö
  `}`=ü `~`=ß).

## 2. Data used by this segment

### 2.1 Records

`squad(s, i)` = `FAR:774a + (s*25 + i) * 0x34` (save block 18, `SquadPlayer`, 52 bytes). Squads 0..3
belong to the human managers, squad 4 is the **transfer pool / market**. The market list uses slots
0..11 of squad 4 (`market(i)` = `FAR:8b9a + i*0x34`; `FAR:8ba9 + i*0x34` is its `player_id`).

| Off | FAR (squad 0) | Type | Meaning in this segment |
|---|---|---|---|
| 0x00, 0x01, 0x02 | 774a..774c | u8 | rating bytes; `6*b0 + b1` is a value term (§4.5, `player_value_calc`). Copied on a manager-to-manager sale |
| 0x03, 0x04, 0x05 | 774d..774f | u8 | stat bytes (value term `(b3+b4)*30 + b5*40`). **In market records** byte 0x03 is reused as the "bid rejected" mask: bit `m` = manager `m` was rejected, bit 7 = any rejection. Set to 0 for loaned-in players |
| 0x06 | 7750 | u8 | league games (`" LIGASPIELE"`) |
| 0x09 | 7753 | u8 | flags. Bit 6: an offer for this (own squad) player is pending. Bit 7: in a market record, an offer for this listed player is pending; in `player_value_calc` bit 7 switches the 130 % / random factor |
| 0x0a | 7754 | u8 | status; set to 0 when a player is put on the market |
| 0x0b | 7755 | u8 | contract length in seasons |
| 0x0c | 7756 | u8 | loan: 0 = own player, else `0x80 + lending club id` |
| 0x0d | 7757 | u8 | suspension flag (copied on a manager-to-manager sale) |
| 0x0e | 7758 | u8 | random 35..65 (`player_init_random_attrs`), value term |
| 0x0f | 7759 | u8 | `player_id` (index into `players`, 0 = empty slot) |
| 0x10..0x12 | 775a..775c | u8[3] | skills (mirrored into `Player::skills`) |
| 0x14 | 775e | u8 | packed contract/terms byte from `player_init_random_attrs` (low nibble 1..13, bits 4-5 = 1..3, bit 7 random) |
| 0x16 | 7760 | u8 | club the player goes to on sale (copied into `Player::team` and cleared) |
| 0x17 | 7761 | u8 | copied on a manager-to-manager sale |
| 0x18 | 7762 | u8 | negotiation state: 0 = willing; 1..99 = cool-down ("nicht verhandlungsbereit"); 100..127 = player insists on a contract of exactly `value-100` seasons; bit 7 (>= 0x80) = retires when the contract ends |
| 0x1c, 0x1e, 0x20 | 7766, 7768, 776a | u16 | value terms (weights 50/20/30) |
| 0x28 | 7772 | i32 | **squads 0..3: monthly salary; squad 4: asking fee** (both computed by `player_value_calc`) |
| 0x30 | 777a | far ptr | message pointer (cleared with `message_remove`) |

`players[p]` = `FAR:57dd + p*0x25` (block 17, `Player`): `+0x1a` (57f7) age (shown as `" JAHRE ALT, "`),
`+0x1c..0x1e` (57f9..57fb) skills, `+0x21` (57fe) `owner`: 0..3 = human manager, **4 = on the market
(club owned)**, **5 = free pool / unused**; `+0x24` (5801) club id.

`clubs[c]` = `FAR:3066 + c*0x22` (block 15): `+0x00` name, `+0x18..0x1a` strength[3] (`FAR:307e..`),
`+0x1b..0x1d` base_strength[3].

`managers[k]` = `FAR:2242 + k*0x30a` (block 13): `+0x00` name, `+0x1e` (2260) club id, `+0x138` (237a)
league 0..2, `+0x141..0x144` (2383) training A[4], `+0x146..0x149` (2388) training B[4], `+0x1f0` (2432)
`balance: i32`.

### 2.2 Constant tables (DGROUP, values read from the image)

| DS | Type | Name | Values | Used by |
|---|---|---|---|---|
| 543e | i8[2][6] | `value_weights` | row 0 (fee, mode bit0 = 0): `58, 4, 4, 5, 4, 25`; row 1 (salary, bit0 = 1): `70, 8, 7, 5, 8, 2` | `player_value_calc` |
| 2262 | i8[4] | `league_rounds` | `34, 38, 38, 0` (match days per league) | `contract_demand_accepted`, `contract_negotiation` |
| 225a | i8[3] | `league_round` | initial `1, 1, 1`; save block 34; current match day of league 0..2 | same |
| 226e | i8[3] | `tier_first_team` | `0, 18, 38` | `transfer_market_refresh` |
| 225e | i8[3] | `tier_team_count` | `18, 20, 20` | same |
| 2272 | u8[3][2] | `tier_team_range` | `(0,17) (18,37) (38,57)` | same |
| 224c | i8 | `season_start_day` | 0 | (main, before the refresh call) |
| 224d | u8 | `currency_prefix_flag` | 0 | money display |
| 224e | i32 | `currency_div` | 1 (MEMORY_MAP calls it "difficulty"; state.md calls it `option_long_a`) | money display |
| 2252 | i32 | `currency_mul` | 1 | money display |
| 5432 `", "`, 5435 `""`, 5436 `""`, 5437 `": "`, 543a `": "`, 544a `", "`, 544d `":"`, 544f `":"` | char[] | literal fragments | | screens |

The UI string pointers used here are in the tables DS:22e0 and DS:4a68 (far pointers); every one is
listed with its text where it is used.

## 3. Shared helpers called from this segment (other areas, semantics needed)

* `squad_remove_player(slot, force, last)` = 2ecd:10ee (`player_lineup_check` in FUNCTION_MAP). On squad
  `FAR:304a`: if `squad[slot].player_id == 0` return 1. If `players[pid].owner != FAR:304a && !force`
  return 0. Else: if the message pointer is non-NULL call `message_remove(ptr)` (result discarded);
  for `k = slot; k <= last-1; k++` copy record `k+1` over record `k` (52 bytes); set
  `squad[last].player_id = 0`; return 1. **After the call, the record at `slot` is the next player.**
* `transfer_list_add(pid, len, loan_char, i32 arg, last)` = 2ecd:37d8 (owned by the squad agent): inserts
  player `pid` into squad `FAR:304a` sorted by `player_id`, with `+0x0b = len`, `+0x0c = loan_char`
  (`0x63` = `'c'` means loan), calls `random_range(.., 0x50)`, `player_init_random_attrs`,
  `player_value_calc(slot, (loan_char == 'c')*2 + 1)` into `+0x28`, and may change `players[pid].team`.
  Returns the new slot (`u8`) or 0x7f when the squad is full / not allowed (it shows its own dialog).
* `message_remove(ptr)` = 4091:0044 (`message_find_owner` in FUNCTION_MAP): removes and frees the message
  from the per-manager table FAR:5524 and **always returns NULL** (0:0).
* `club_strength_after_sale(club, avg_skill)` = 24a4:23da, 3 RNG calls, see §5.
* `squad_count_by_flag(s, 0)` = 4091:1109: number of non-empty slots in squad `s` (12 slots for s = 4,
  24 for s = 0..3).
* `icon_register(x, y, text, style)` = 4091:123d: draws a 0x38x0xb text button and registers it in the
  next icon slot (`DS:07b3` = count, reset to 0 by the caller). `icon_hit()` = 4091:11cc returns the
  1-based registration index of the slot under the mouse (`x` in `(X, X+0x39)`, `y` in `(Y, Y+0xc)`,
  exclusive), else 0. Style 0x12 is the highlighted look, 1 the normal look.
* `dialog_okay(s1, s2, s3, s4)` = 4091:0e3a: 4-line box with OKAY button, waits for the click.
  `ui_dialog_box(s1, s2)` = 176b:0e57: 2-line message box. `message_broadcast(manager, l1, l2, l3, b1,
  b2)` = 4091:146c: "An Alle:"-style box addressed to manager `manager` with two buttons; returns
  non-zero if the first button was chosen.
* `player_card` = 2ecd:000e, `screen_transfer_market_list` = 2ecd:06af, `FUN_2ecd_29c6`,
  `FUN_2ecd_26e6`, `squad_weekly_update` = 2ecd:3360, `screen_player_info` = 24a4:0906,
  `sponsor_offer_dialog` = 1000:272d, `manager_ai_weekly` = 1f9d:0002, `ui_wait_with_highlight` = 4091:1722:
  other areas; only their arguments are given here. `player_card` returns: 0x7f nothing, 0..24 a squad
  slot, 0x65 click outside the list, 0x66 exit, 0x68 (contract screen only) leave.

## 4. Functions

### 4.1 3277:000a `player_init_random_attrs`

```c
void player_init_random_attrs(SquadPlayer far *p)
{
    p[0x0e] = (u8)random_range(0x23, 0x41);        // RNG#1  35..65
    p[0x14] = (u8)random_range(1, 0x0d);           // RNG#2  1..13
    p[0x14] |= (u8)(random_range(1, 3) << 4);      // RNG#3  8-bit SHL
    p[0x14] |= (u8)(random_range(0, 1) << 7);      // RNG#4
}
```
(The first instruction reads `p[0x0e]` and discards it.) Callees: `random_range`.

### 4.2 3277:0081 `training_draw_cost_bar` (UI, `screen_training`)

```c
void training_draw_cost_bar(Bitmap far *bmp, i16 n)
{
    blit_bitmap(bmp, 0, 0x36, rp, 0xd1, 0xc1, 0x32, 0x29, 0xc0);   // rp = FAR:2f28 (main rastport)
    ui_draw_line(rp, 0xd9, 0xd4, 0xd9 + n, 0xd4, 0x1d);
    ui_draw_line(rp, 0xd9, 0xd5, 0xd9 + n, 0xd5, 0x1d);
    ui_set_font(rp, 2);
    i32 cost = aflshr(((i32)n + 1) * 10000, 4);                 // arithmetic >> 4
    cost = cost * currency_mul / currency_div;
    ui_draw_text_centered_shadow(rp, format_long_thousands(cost), 0xd3, 0x105, 0xdf, 0x1d, 0xff);
}
```

### 4.3 3277:01a9 `training_free_slots`

```c
i8 training_free_slots(i8 kind)
{
    i8 sum = 0;
    for (i8 i = 0; i < 4; i++)
        sum += kind == 0 ? managers[m].training_A[i]   // FAR:2383 + m*0x30a + i
                         : managers[m].training_B[i];  // FAR:2388 + m*0x30a + i
    return (kind != 0 ? 10 : 20) - sum;                // u8 arithmetic in AL
}
```

### 4.4 3277:0225 `training_draw_slot_bar` (UI, `screen_training`)

`blit_bitmap(src, sx, sy, dst, dx, dy, w, h, minterm)`, `ui_blit_transparent(src, sx, sy, dst, dx, dy, w, h)`.
All `i16`, `/` and `%` are signed 16-bit.

```c
void training_draw_slot_bar(Bitmap far *bmp, i16 col, i16 a, i16 b, i16 mode)
{
    if (b == 0) return;
    if (col < 2) {
        i16 x = col * 0x127 + 5;
        i16 t = b * 16 - 0xc4;
        i16 y0 = -t;
        if (a == 0 && mode == 0) {
            for (i16 i = 0; i < (b * 16 - 1) / 15; i++) {
                i16 y = i * 16 + y0;
                if (y + 1 > 0x23) blit_bitmap(bmp, 2, 2, rp, x, y - 0x13, 15, 15, 0xc0);
            }
        } else {
            if (a == 0) a = 1;
            a = 0xc4 - a * 16;
            if (mode == 0) {
                if (a < 0x23) a = 0x23;
                for (i16 i = 0; i < (a + t - 1) / 15; i++) {
                    i16 y = i * 16 + y0;
                    if (y + 1 > 0x23) blit_bitmap(bmp, 2, 2, rp, x, y - 0x13, 15, 15, 0xc0);
                }
            } else {
                for (; y0 - 5 < a && a > 0x22; a -= 16)
                    ui_blit_transparent(bmp, 0x15, 0x15, rp, x, a - 0x13, 15, 15);
            }
        }
    } else {
        col -= 2;
        i16 x0 = (col > 3) ? 0xd2 : 0x1f;
        i16 y = (col % 4) * 0x25 + 0x23;
        if (col == 8) { x0 = 0x1f; y = 0xbd; }
        a = (a - 1) * 16 + x0;
        i16 e = b * 16 + x0 - 6;
        if (mode == 0) {
            if (a < 0x23) a = 0x23;
            for (i16 i = 0; i < ((e - a) - 0xb) / 15; i++)
                blit_bitmap(bmp, 0x17, 2, rp, i * 16 + a, y + 1, 15, 15, 0xc0);
        } else {
            for (; a < e; a += 16) ui_blit_transparent(bmp, 0x15, 0x15, rp, a, y + 1, 15, 15);
        }
    }
}
```

### 4.5 3277:25de `player_value_calc` (fee / salary formula)

`i32 player_value_calc(i8 slot, u8 mode)`; result in DX:AX. Reads `squad(m, slot)` (`m = FAR:304a`, so
callers set `FAR:304a = 4` for market records). Mode bits: bit 0 = salary formula, bit 1 = loan
(fee / 3 and 75..95 %), bit 2 = random asking-price spread. Modes in use: 0 (offer for a player), 4
(market asking price), 5 (salary demand; bit 2 is ignored with bit 0), 1 and 3 (salary set by
`transfer_list_add`; identical results, bit 1 is ignored with bit 0).

```c
i32 player_value_calc(i8 slot, u8 mode)
{
    u8 far *P = squad(FAR:304a, slot);           // index (i16)(i8)m*25 + slot
    u8 b = mode & 1;
    i8 *w = &value_weights[b][0];                // DS:543e + 6*b
    i32 v;

    i16 h = (i16)((u16)P[0x11] + P[0x10]) / 2;     // CWD/SUB/SAR: signed halving
    v  = (i32)h * w[0] / 100;                      // 16x16->32 signed IMUL, full 32-bit product
    i16 r1 = (i16)((((i32)((u16)P[3] + P[4])) * 30 + (u32)((u16)P[5] * 40)) / 100);  // low word kept
    i16 r2 = (i16)(((u32)(20 * *(u16*)&P[0x1e]) + (u32)(30 * *(u16*)&P[0x20])
                    + (u32)(50 * *(u16*)&P[0x1c])) / 100);  // 16x16->32 unsigned MULs, signed /100
    clamp_int(&r1, 0, 20);
    clamp_int(&r2, 0, 200);
    v += ((i32)w[1] * r1) * 5 / 100;                 // w[1]*r1 is a 16x16->32 signed IMUL
    v += ((i32)r2 * 100 / 200) * w[2] / 100;
    v += (i32)(i16)((i16)(w[3] * (u16)P[0x0e]) / 100);   // pure 16-bit IMUL/IDIV, then sign-extended
    i16 r3 = (i16)((u16)(6 * (u16)P[0]) + (u16)P[1]); // MOV AL,6; MUL byte: full 16-bit product, NOT truncated to 8 bit
                                                     // e.g. P[0]=43 -> 258+P[1] -> clamped to 20
    clamp_int(&r3, 0, 20);
    v += ((i32)w[4] * r3) * 5 / 100;
    i16 r4 = 0x2d - players[P[0x0f]].age;            // 45 - age (u8 age, 16-bit)
    clamp_int(&r4, 0, 27);
    v += ((i32)r4 * 100 / 27) * w[5] / 100;

    if (b) {                                         // salary formula
        v = v * v * v / 1000;                        // 32-bit wrapping products
        i32 s = (i32)((u16)P[0x10] + P[0x11]);
        if (s > 100) {
            s = ((s - 100) >> 1) + 100;              // arithmetic 32-bit shift
            if (s > 135) v = v * (s - 35) / 100;
        }
        if (v < 2) v = 2;                            // signed compare
        v *= 85;
    } else {                                         // fee formula
        v *= 10000;
        if (v > 110000) v -= 100000;                 // compares DX > 1 || (DX == 1 && AX > 0xadb0)
        if (mode & 2) v /= 3;
        if (P[0x09] & 0x80) {
            if (P[0x16] > 0x3f) v = v * 130 / 100;
            else                v = v * random_range(0x5f, 0x64) / 100;   // RNG#1 95..100
        }
    }
    v = v * 5 / 3;
    if ((mode & 4) && !b) {
        i32 a = v / 15;
        i16 r = random_range((i16)(v / 20), (i16)(v / 10));   // RNG#2; v/10 evaluated before v/20,
                                                              // both truncated to their low word
        v += (i32)r - a;
    }
    if ((mode & 2) && !b)
        v = v * random_range(0x4b, 0x5f) / 100;     // RNG#3 75..95
    i32 d = b ? 100 : 1000;
    return v / d * d;
}
```
RNG order: RNG#1 only if fee formula and `P[9]` bit 7 and `P[0x16] <= 0x3f`; then RNG#2 if mode bit 2;
then RNG#3 if mode bit 1. Callees: `clamp_int`, `random_range`, CRT long helpers.

### 4.6 3277:1e38 `transfer_market_refresh`

`void transfer_market_refresh(void)`. Called once for a new game (`main` 18bc:08c2, after setting the
day counter `DS:07dc = (i32)DS:224c`) and from `season_main_loop` at 2a58:3b81 when
`random_range(0, DS:07ab + 3) == 0` (that RNG call belongs to the caller). It removes club-owned
entries from the 12 market slots and refills a random number of them.

```c
void transfer_market_refresh(void)
{
    FAR:304a = 4;
    i16 kept = 0, i;
    for (i = 0; i < 12; i++) {
        u8 pid = market(i)[0x0f];
        if (squad_remove_player((u8)i, 0, 12))     // removes club-owned entries (owner == 4),
            players[pid].owner = 5;                // and returns 1 for empty slots (pid 0!)
        else
            kept++;                                // entries listed by human managers
    }
    i16 n = 12 - kept;
    if (n != 0) {
        n -= random_range(0, n);                                   // RNG#1
        for (i = 0; n > i; i++) {                                  // NOTE: i is clobbered below
            i16 s = random_range(0, 11);                           // RNG#2 slot
            i16 pid = market(s)[0x0f];
            if (pid != 0 && players[pid].owner != 4) continue;     // keep human-listed entries
            if (pid != 0) {
                squad_remove_player((u8)s, 1, 12);
                players[pid].owner = 5;
            }
            i16 tries = 0;
            do {
                pid = random_range(1, 0x96);                       // RNG#3 (repeated)
                tries++;
            } while (players[pid].owner != 5 && tries < 0x5dc);    // up to 1500 tries
            players[pid].skills[2] = (u8)random_range(0x2d, 0x37); // RNG#4  FAR:57fb, 45..55
            i16 team = players[pid].team;                          // FAR:5801, zero-extended
            if (manager_index_of_team((i8)team, 0xff) != 0) {      // 1310:32b1: team is a human's club
                // search the tier of `team`; the ORIGINAL REUSES THE OUTER LOOP VARIABLE i HERE
                for (i = 0; i < 3; i++)
                    if (!(tier_first_team[i] > team) &&
                        tier_first_team[i] + tier_team_count[i] > team) break;
                if (i == 3) i = 2;
                do {
                    team = random_range(tier_team_range[i][0], tier_team_range[i][1]);  // RNG#5 (repeated)
                } while (manager_index_of_team((i8)team, 0xff) != 0);
            }
            i16 v = random_range(0, 14) + clubs[team].strength[1] - 10;     // RNG#6, FAR:307f
            clamp_int(&v, 10, 99);
            players[pid].skills[0] = (u8)v;                                 // FAR:57f9
            v = random_range(0, 14) + clubs[team].base_strength[1] - 10;    // RNG#7, FAR:3082
            if (random_range(0, 4) == 0)                                    // RNG#8
                v += random_range(0x0c, 0x14);                              // RNG#9 12..20
            clamp_int(&v, 10, 99);
            players[pid].skills[1] = (u8)v;                                 // FAR:57fa
            u8 slot = transfer_list_add((u8)pid, 0, 0, 0L, 12);             // RNG inside (squad area)
            players[pid].owner = 4;
            players[pid].team  = (u8)team;
            market(slot)->fee = player_value_calc(slot, 4);                 // +0x28; RNG#10 (and #1 if P[9]&0x80)
        }
    }
    FAR:304a = FAR:5358;
}
```
Bit-exact notes: the slot pass removes entries while `i` keeps increasing, so the entry that slides into
slot `i` is skipped. The empty-slot case writes `players[0].owner = 5`. After the tier search `i` is
0..2 and the refill loop continues from `i + 1` (this can make the loop run more or fewer times than
`n`). The `player_value_calc(slot, 4)` uses the slot returned by `transfer_list_add` even if it is
0x7f (no check). Callees: `squad_remove_player`, `random_range`, `manager_index_of_team`,
`clamp_int`, `transfer_list_add`, `player_value_calc`.

### 4.7 3277:2171 `market_fee_offer_accepted`

```c
i16 market_fee_offer_accepted(i8 slot, i32 offer, u8 loan)
{
    i32 v = market(slot)->fee;              // FAR:8bc2 + (i16)(i8)(slot*0x34)  (8-bit IMUL!)
    if (loan) v = v / 3;
    if (v * random_range(0x4b, 0x55) / 100 > offer) return 0;     // RNG#1 75..85
    if (v * random_range(0x78, 0x82) / 100 < offer) return 1;     // RNG#2 120..130
    i32 q = offer * 100 / v;                                      // v == 0 -> divide error in DOS
    return q > (i32)random_range(0x50, 0x78);                     // RNG#3 80..120, signed compare
}
```
The slot index is computed with `MOV AL,0x34; IMUL byte [slot]` (8-bit signed multiply, 16-bit result),
which is correct for slots 0..11.

### 4.8 3277:2270 `contract_demand_accepted`

`i16 contract_demand_accepted(u8 slot, i32 len, i32 salary)`; works on `P = squad(m, slot)` (`m` =
`FAR:304a`, slot zero-extended). Returns 1 when the player accepts the offered contract.

```c
i16 contract_demand_accepted(u8 slot, i32 len, i32 salary)
{
    i8 lg = managers[m].league;                       // FAR:237a + m*0x30a
    i8 round = league_round[lg];                      // DS:225a
    if (len == 0) len = P[0x0b];
    u8 age = players[P[0x0f]].age;
    if (salary == 0) salary = P->salary;              // +0x28
    i32 cur_len = P[0x0b];
    if (cur_len == len && (u32)P->salary < (u32)salary) return 1;
    if (cur_len == len && (u32)P->salary > (u32)salary) return 0;
    if (cur_len > len && (u32)(P->salary * 120) / 100 < (u32)salary) return 1;   // aflmul + afuldiv
    if (cur_len > len && (u32)(P->salary * 105) / 100 >= (u32)salary) return 0;
    i32 rem = (i32)((i16)round * -100) / (i32)(i8)league_rounds[lg] + 100;      // % of season left
    len = (len - 1) * 100 + rem;
    i32 v = player_value_calc(slot, 5);               // salary formula, no RNG
    i32 k = aflshl(len / 6 + 110, 3) / 10;
    v = v * random_range((i16)k - 6, (i16)k + 4) / 100;               // RNG#1
    if (day_counter > 0x141)                                          // DS:07dc (i32) > 321
        v = v * random_range(0x84 - (i8)age, 0x98 - (i8)age) / 100;   // RNG#2  132-age .. 152-age
    if (v >= 100000) return 0;                                        // 0x186a0
    return salary > v;                                                // signed
}
```
The `(i16)round * -100` is a 16x16->32 signed IMUL (`MOV BX,0xff9c; IMUL BX`). `age` is sign-extended
(CBW) in the RNG#2 bounds. Callees: `player_value_calc`, `random_range`, CRT helpers.

### 4.9 3277:1b5f `transfer_offer_received` (BEHALTEN / VERKAUFEN dialog)

`i16 transfer_offer_received(i32 far *fee, i32 len)`; called with `len = (i32)P[0x0b]` (contract
length in seasons). Returns the icon index: 1 = `"BEHALTEN"` (keep), 2 = `"VERKAUFEN"` (sell). It reduces
`*fee` by the compensation it shows.

```c
DS:07b2 = 0;                                                     // thousands separators ON (not restored)
ui_draw_text_at(rp, "Angebot" /*DS:27ec*/, 0xa5, 0x8e, 1);
ui_draw_text_at(rp, "Abl|se"  /*DS:27f0*/, 0xa5, 0x98, 1);
s = ": " /*DS:5437*/ + (DS:224d ? prefix : "") + fmt(*fee * currency_mul / currency_div) + " DM." /*DS:4f68*/;
ui_draw_text_at(rp, s, 0xd9, 0x8e, 1);
i32 cut = (*fee / 7) * len;
*fee -= cut;
s = ": " /*DS:543a*/ + prefix? + fmt(cut * currency_mul / currency_div) + " DM.";
ui_draw_text_at(rp, s, 0xd9, 0x98, 1);
ui_draw_line(rp, 0xa3, 0x9a, 0x13a, 0x9a, 0xb);
DS:07b3 = 0;
icon_register(0xaf, 0x6e, "BEHALTEN" /*DS:27f4*/, 1);
icon_register(0xf5, 0x6e, "VERKAUFEN" /*DS:27f8*/, 0x12);
do { input_poll_mouse(FAR:6dc0); h = icon_hit(); } while (!(FAR:2ea2 == 1 && h != 0));
return h;
```

### 4.10 3277:2a8f `contract_negotiation`

`i16 contract_negotiation(i16 slot, void far *arg2, void far *arg3, Bitmap far *bmp)`. Two modes:

* **single** (`slot != -1`): negotiate with `squad(m, slot)` once (used after buying a player and by
  1cb5:110c). Returns 1 only if a new contract was signed (`hit == 1`), else 0.
* **browse** (`slot == -1`, from the squad screen 2ecd:18bf): a contract screen listing the own squad;
  returns 0 when left with 0x68, 1 otherwise.

Locals: `len` (i32, entered seasons), `sal` (i32, entered salary), `forced` (i16), `hit` (i8),
`m10` (`[bp-0x10]`, sub-mode from `FUN_2ecd_26e6`), `own_bmp` (`bmp` was passed in), `cnt`.

```c
own_bmp = (bmp != NULL);
if (!own_bmp) bmp = vga_load_picture(6, 0, 0, 0, 1);            // PIC 6
if (slot == 0x7f) return 0;
cnt = squad_count_by_flag(FAR:304a, 0) - 1;
single = (slot != -1);
if (single) goto negotiate;                                      // NO loan/retire/cool-down checks
DS:07b2 = 1;
ui_fill_rect(rp, 3, 8, 0xef, 0xb8, 8);
screen_transfer_market_list(6, 0x12, 0, 0x18, 0xec, 0xb0, 0xd146, 0x0003, 3);   // 2ecd:06af
FUN_2ecd_29c6(0,0,0,0,0,0,0,0,0,0,0);
screen_fade(FAR:2e70, 1);
browse:
  len = sal = 0;
  ui_set_font(rp, 2);
  do {
    slot = player_card(6, 0x1e, 0, cnt, 0xef + 0xb5*m10, 0xb8, 0xd146, 0x0003, arg2, 4);
    click = FAR:2ea2; FAR:2ea2 = 0;
    if (m10) FUN_2ecd_29c6(bmp, arg3, arg2, 0, 0, 0xa8, m10, 1);
  } while (slot == 0x7f);
  if (slot == 0x66 && m10 == 0) {                               // open the line-up sub-mode
    tmp = vga_load_picture(0, 0x32, 0x29, 5, 0);                // 50x41 save buffer
    blit_bitmap(FAR:a6ca, 0x10c, 0x29, tmp, 0, 0, 0x32, 0x29, 0xc0);
    blit_bitmap(FAR:a6ca, 0x10c, 0x83, rp, 0x10c, 0x29, 0x32, 0x29, 0xc0);
    m10 = FUN_2ecd_26e6(bmp, &lineup_btn, 0, 0, &rect, 1);   // lineup_btn = u8 [bp-0xde]
  }
  if (slot == 0x65 && m10 != 0) {
    m10 = 0; ui_redraw_buttons(lineup_btn, 0);              // 2d2b: reads u8 [bp-0xde]
    blit_bitmap(tmp, 0, 0, rp, 0x10c, 0x29, 0x32, 0x29, 0xc0); bitmap_free(tmp);
    slot = 100;
  }
  if (slot >= 0x63) goto loop_end;
  P = squad(m, slot); pid = P[0x0f];
  if (click == 3) { screen_player_info(slot); goto loop_end; }
  if (bmp == NULL) ui_fill_rect(rp, 4, 0xb2, 0xec, 0xb8, 8);
  forced = P[0x18];
  if (P[0x0c] != 0) {
    dialog_okay(players[pid].name, "ist leider von", clubs[P[0x0c] & 0x7f].name, "nur ausgeliehen.");
    goto loop_end;
  }
  if (forced & 0x80) {
    dialog_okay(players[pid].name, "wird sich mit", "Ablauf des Vertrages", "zur Ruhe setzen.");
    goto loop_end;
  }
  if (forced > 99) { forced -= 100; goto negotiate; }
  if (forced != 0) {
    dialog_okay(players[pid].name, "ist im Moment", "nicht verhandlungs-", "bereit.");
    delay_ticks(30);
    if (*(i32 far *)FAR:6dc0_ptr != 0) goto loop_end;          // mouse y and x both 0 -> bypass (!)
    forced = 0;
  }
negotiate:
  P = squad(m, slot); pid = P[0x0f];
  s = players[pid].name + ", " /*DS:544a*/ + fmt(players[pid].age) + " JAHRE ALT, " + fmt(P[6]) + " LIGASPIELE";
  btn_id = ui_draw_button(rp, 4, 0xc7, 0xee, 0x1f, &rect, 1, 8);   // btn_id = u8 [bp-0x98] (300f)
  ui_draw_line(rp, 8, 0xd1, 0xb4, 0xd1, 1);
  ui_draw_text_at(rp, s, 8, 0xcf, 1);
  ui_draw_text_at(rp, "VERTRAGSDAUER (IN SAISONS)", 8, 0xd9, 2);
  ui_draw_text_at(rp, ":" /*DS:544d*/, 0x82, 0xd9, 2);
  ui_draw_text_at(rp, fmt(forced ? forced : P[0x0b]), 0x87, 0xd9, 2);
  ui_draw_text_at(rp, "GEHALT PRO MONAT (IN DM)", 8, 0xe1, 2);
  ui_draw_text_at(rp, ":" /*DS:544f*/, 0x82, 0xe1, 2);
  ui_draw_text_at(rp, fmt((u32)(P->salary * currency_mul) / (u32)currency_div), 0x87, 0xe1, 2);
  DS:07b3 = 0;
  icon_register(0xb8, 0xca, "NEUER VERTRAG", 0x12);             // icon 1
  icon_register(0xb8, 0xd8, "ABBRUCH", 1);                       // icon 2
  for (;;) {
    input_poll_mouse(FAR:6dc0);
    if (FAR:2ea2 == 1 && (hit = icon_hit()) != 0) break;
    y = FAR:2e70->y (+4); x = FAR:2e70->x (+6);
    if (!(FAR:2ea2 == 1 && x > 8 && x < 0xb4 && y > 0xd2 && y < 0xe2)) continue;
    f = (y > 0xd9);                                              // 0 = seasons, 1 = salary
    ui_fill_rect(rp, 0x87, 0xd3 + 8*f, 0xb4, 0xd9 + 8*f, 8);
    if (f == 0) {
      if (P[0x0c] != 0) continue;                                // loaned players: no length change
      len = ui_scroll_text_box(FAR:6dc0, buf, 0x87, 0xd9, 0x3e9, 2, 8, 0);   // i32 number
      if (buf[0] == 0) { ui_draw_text_at(rp, fmt(forced ? forced : P[0x0b]), 0x87, 0xd9, 2); continue; }
      ui_fill_rect(rp, 0x87, 0xdb, 0xb4, 0xe1, 8);
      // salary suggestion = the player's MAXIMUM demand for this length (no RNG):
      i32 v = player_value_calc(slot, 5);
      i32 rem = (i32)((i16)league_round[lg] * -100) / (i32)(i8)league_rounds[lg] + 100;
      i32 L = (len - 1) * 100 + rem;
      i32 k = aflshl(L / 6 + 0x7a, 3) / 10;                      // note 0x7a (122), not 110
      v = v * (k + 4) / 100;
      if (day_counter > 0x141) v = v * (i16)(0x98 - age) / 100;  // age zero-extended here
      if ((u32)P->salary > (u32)v) v = P->salary;
      ui_draw_text_at(rp, fmt(v * currency_mul / currency_div), 0x87, 0xe1, 2);
      sal = v;
    } else {
      sal = ui_scroll_text_box(FAR:6dc0, buf, 0x87, 0xe1, 0x3ee, 2, 8, 0) * currency_div / currency_mul;
      if (buf[0] == 0)
        ui_draw_text_at(rp, fmt((u32)(P->salary * currency_mul) / (u32)currency_div), 0x87, 0xe1, 2);
    }
  }
  // ---- decision (3277:3281) ----
  if (len > 4) {
    dialog_okay("Wer wird sich denn SO", "lange verpflichten ?", "(Mal abgesehen", "von Bundi's)");
    hit = 2;
  }
  cur_len = P[0x0b]; cur_sal = P->salary;
  if (!(single && day_counter <= 0x141)) {
    if (len == 0 && (sal == 0 || cur_len == 0) && forced == 0)
      hit = (day_counter > 0x141) ? 2 : 3;                       // 3 = "nothing entered", no effect
  }
  if (len == 0) len = forced ? forced : P[0x0b];
  if (sal == 0) sal = P->salary;
  if (hit == 1 && len > cur_len && cur_sal > sal) {              // signed i32 compares
    dialog_okay("So dumm", "ist", players[pid].name, "leider nicht...");
    hit = 2;
  }
  if (forced != 0 && cur_sal > sal) hit = 2;
  if (forced != 0 && (i32)forced != len) hit = 2;
  if (hit == 1 || (hit == 3 && single)) {
    if (!(hit == 3 && single) && forced == 0 &&
        !contract_demand_accepted((u8)slot, len, sal)) {         // RNG: 1 or 2 calls
      dialog_okay(players[pid].name, "ist nicht an", "Ihrem Angebot", "interessiert.");
      hit = 2;
    } else {
      if (forced) len = forced;
      P[0x0b] = (u8)len;
      P->salary = sal;
      if (!single) {
        ui_fill_rect(rp, 8, 0xc8, 0xf3, 0xe5, 8);
        ui_set_font(rp, 1);
        ui_draw_text_centered_shadow(rp, "Ihr Angebot wurde angenommen !", 6, 0xf4, 0xda, 1, 8);
        delay_ticks(60);
        ui_fill_rect(rp, 4, (slot + 5) * 6, 0xef, slot * 6 + 0x23, 8);
      }
    }
  }
  if (hit != 3) {
    if (P[0x18] > 0x63) P->msg = message_remove(P->msg);         // = NULL
    P[0x18] = (u8)random_range(0x0a, 0x12);                      // RNG cool-down 10..18 (also on success)
    if (!single && arg2 == NULL && (slot < 8 || m10 == 0))
      ui_fill_rect(rp, 4, (slot + 5) * 6, 0xef, slot * 6 + 0x23, 8);
  }
  if (!single) screen_transfer_market_list(6, 0x12, slot, slot, 0xec, 0xb0, 0xd146, 0x0003, 3);
  ui_redraw_buttons(btn_id, 0);                                  // 3ac0: u8 [bp-0x98]
loop_end:
  if ((!single && slot != 0x65 && slot != 0x68) || m10 != 0) goto browse;
  if (!own_bmp) bitmap_free(bmp);
  if (single) return hit == 1;
  ui_draw_button(rp, 0x10c, 0x83, 0, 0x2000, &rect, 0, 8);
  ui_draw_button(rp, 0x10c, 0x29, 0, 0xa000, &rect, 0, 8);
  return slot != 0x68;
```
Notes: `hit` is only defined by `icon_hit` (1 = NEUER VERTRAG, 2 = ABBRUCH) and the overrides above.
`day_counter` compares are on the `i32` at DS:07dc. RNG per decision: `contract_demand_accepted`
(1-2 calls) when it is evaluated, then 1 call for the cool-down unless `hit == 3`.

### 4.11 3277:04a5 `screen_transfer_market` (office menu 2, case 3)

`void screen_transfer_market(void)`. Layout: own squad list on the left (`player_card` area), market list
(12 rows) on the right at x 0xa3..0x13a, rows `y = 0x1d + 6*r` (r = 1..12, row r is `market(r-1)`),
an info/offer box at y 0x84..0x9b, the balance at y 0xa4..0xb2, and the KAUFEN/LEIHEN toggle.

Locals: `hover` (row, 0x7f = none), `loan` (u8 0/1, `[bp-8]`), `redraw_own` (`[bp-0x2a]`),
`redraw_market` (`[bp-0x32]`), `first_fade`, `own_cnt`, `offer` (i32, `[bp-6]` low / `[bp-4]` high, persists between
iterations and is NEVER initialised at screen entry: the only writes are at 0d8c, 0dc9, 11ef, 1262,
12af, 13a6 plus `transfer_offer_received(&offer)` at 0ddc/12c2), `reclaim` (`[bp-0xa]`), `from_human` (`[bp-0x78]`), `proceed` (`[bp-0x7a]`),
`refused_msg` (`[bp-0x3a]`).

**Setup**
```c
hover = 0x7f; redraw_own = redraw_market = first_fade = 1; refused_msg = 0; loan = 0;
copy_protection_penalty("Transfermarkt" /*DS:4c18*/, 0, 0, 0);
ui_draw_button(rp, 0,     0x0d, 0x9d, 0xb5, &r1, 0, 8);
ui_draw_button(rp, 0xa1,  0x0d, 0x9b, 0x70, &r1, 0, 8);         // market panel
ui_draw_button(rp, 0x10c, 0xc1, 0,    0x22, &r2, 0, 8);         // exit button
ui_draw_button(rp, 0xa1,  0x83, 0x9b, 0x31, &r3, 0, 8);         // offer/balance panel
ui_set_font(rp, 1);
ui_draw_text_at(rp, "Kontostand:" /*DS:2550*/, 0xa5, 0xa4, 0xb);
ui_draw_line(rp, 0xa3, 0xa6, 0x13a, 0xa6, 0xb);
```
**Main loop** (3277:061f)
```c
for (;;) {
  input_wait_release();
  if (redraw_own || redraw_market) {
    ui_fill_rect(rp, 0xa3, 0x84, 0x13a, 0x9b, 8);
    DS:07b3 = 0;
    icon_register(0xaf, 0x6e, "LEIHEN" /*DS:27c0*/, 0x11*loan + 1);          // 1 or 0x12
    icon_register(0xf5, 0x6e, "KAUFEN" /*DS:27c4*/, (u8)(0xef*loan + 0x12));  // 0x12 or 1
    ui_set_font(rp, 2);
    DS:07b2 = 0;
    s = (DS:224d ? prefix : "") + fmt(managers[m].balance * currency_mul / currency_div);
    DS:07b2 = 1;
    s += " DM" /*DS:4f64*/;
    ui_fill_rect(rp, 0xa5, 0xa9, 0x118, 0xb2, 8);
    ui_set_font(rp, 1);
    ui_draw_text_at(rp, s, 0xa5, 0xb0, 1);
  }
  if (redraw_own) {
    ui_set_font(rp, 2);
    screen_transfer_market_list(4, 0x18, 0, 0x18, 0x99, 0xb6, 0x90c6, 0, 4);   // own squad
    own_cnt = squad_count_by_flag(m, 0) - 1;
    squad_weekly_update();
  }
  if (redraw_market) {
    FAR:304a = 4;
    screen_transfer_market_list(0xa3, 0x17, 0, 0x0c, 0x13b, 0x6b, 0x0086, 0x0002, loan + 1);
    FAR:304a = FAR:5358;
    redraw_market = 0;
    if (first_fade) { screen_fade(FAR:2e70, 1); first_fade = 0; }
  }
  redraw_own = 0;
  // --- input: hover loop (3277:08c8) ---
  for (;;) {
    SI = player_card(2, 0x24, 0, own_cnt, 0x9b, 0xbe, 0x90c6, 0, &r1, 2);
    if (SI != 0x7f) break;
    r = (mouse.y - 0x1d) / 6;  c = mouse.x - 0xa1;              // FAR:6dc0 -> [0]=y, [1]=x; i16 IDIV
    if (c >= 0 && r > 0 && c <= 0x98 && r <= squad_count_by_flag(4, 0)) {
      if (hover != r) {
        rastport_set_draw_mode(rp, 2);                           // XOR
        if (hover != 0x7f) ui_fill_rect(rp, 0xa3, 6*hover + 0x1d, 0x13a, 6*hover + 0x21, 0x1e);
        ui_fill_rect(rp, 0xa3, 6*r + 0x1d, 0x13a, 6*r + 0x21, 0x1e);
        rastport_set_draw_mode(rp, 0);
        hover = r;
        pid = market(r-1)[0x0f];
        s = "VON " + (players[pid].owner == 4 ? clubs[players[pid].team].name
                                             : text_to_upper_de(managers[players[pid].owner].name))
            + ", " + fmt(players[pid].age) + " J.";
        FUN_1310_1c22(s, 4, r - 1);                              // appends the player's strength text
        ui_fill_rect(rp, 2, 0xb7, 0x9b, 0xbf, 8);
        ui_draw_text_centered_shadow(rp, s, 2, 0x9b, 0xbe, 1, 0xff);
      }
      if (FAR:2ea2 == 0xe8 || FAR:2ea2 == 3) { SI = r + 999; break; }   // select (right/0xe8)
    } else if (hover != 0x7f) {
      ui_fill_rect(rp, 2, 0xb7, 0x9b, 0xbf, 8);
      XOR-unhighlight row hover; hover = 0x7f;
    }
  }
  if (SI == 0x65) {                                             // left click outside the own list
    r = (mouse.y - 0x1d) / 6;
    if (r >= 0 && r <= squad_count_by_flag(4, 0) && FAR:2ea2 != 0) SI = r + 999;
    if (mouse.y > 0x6d && FAR:513c != 0x7ac && FAR:513c != 0x7c9) {   // toggle KAUFEN/LEIHEN
      loan = 1 - loan; redraw_market = 1;
    }
  }
  if (SI < 0x19)       own_player_click(SI);                    // below
  else if (SI >= 1000) market_click(SI - 1000);                  // below (SI 999 = header row: ignored)
  if (SI == 0x66) {                                             // exit
    ui_wait_with_highlight(0x10c, 0xc1);
    manager_ai_weekly(m, 0);                                    // 1f9d:0002
    return;
  }
}
```

**`own_player_click(slot)`** (3277:0989)
```c
if (FAR:2ea2 == 3) { screen_player_info(slot); return; }
P = squad(m, slot);
if (P[0x18] & 0x80) { ui_dialog_box("Dieser Spieler h|rt", "am Saisonende auf."); return; }
pid = P[0x0f];
if (P[0x09] & 0x40) {                                           // pending offer for own player
  ui_set_font(rp, 1);
  /* strcpy(buf, "Angebot: ") -- prepared but not drawn */
  offer = player_value_calc(slot, 0);                           // RNG if P[9]&0x80 ...
  offer = offer * random_range(0x55, 0x96) / 10000 * 100;       // RNG 85..150 %, rounded to 100
  h = transfer_offer_received(&offer, (i32)P[0x0b]);            // offer -= offer/7*len
  P[0x09] ^= 0x40;
  P->msg = message_remove(P->msg);
  redraw_market = 1;
  if (h == 1) {                                                 // BEHALTEN
    screen_transfer_market_list(4, 0x18, slot, slot, 0x99, 0xb6, 0x90c6, 0, 4);
  } else {                                                      // VERKAUFEN
    managers[m].balance += offer;
    squad_remove_player(slot, 1, 0x18);
    // QUIRK: P now points to the NEXT player (records shifted)
    u8 to = P[0x16]; P[0x16] = 0;
    players[pid].team = to;
    club_strength_after_sale(to, ((u16)P[0x10] + P[0x11] + P[0x12]) / 3);   // 3 RNG calls
    players[pid].owner = 5;
    redraw_own = 1;
  }
  return;                                                       // SI := 0x7f
}
// put the player on the market
n = 0;
for (i = 0; i < squad_count_by_flag(4, 0); i++)
  if (players[market(i)[0x0f]].owner == m) n++;
if (n >= 3) { ui_dialog_box("Schon 3 Spieler auf", "dem Transfermarkt"); return; }
if (players[pid].owner != m) { ui_dialog_box("Dieser Spieler ist", "nur ausgeliehen"); return; }
FAR:304a = 4;
k = transfer_list_add(pid, 1, 0, 1L, 12);                       // RNG inside
FAR:304a = FAR:5358;
if (k == 0x7f) return;
P[0x0a] = 0;
memcpy(market(k), P, 0x34);                                     // the full record overwrites the new entry
squad_remove_player(slot, 1, 0x18);
redraw_market = redraw_own = 1;
```

**`market_click(idx)`** (3277:1033), `M = market(idx)`, `pid = M[0x0f]`, `own = players[pid].owner`
```c
if (FAR:2ea2 == 3) {
  if (own == m) { FAR:304a = 4; screen_player_info(idx); FAR:304a = FAR:5358; }
  return;
}
from_human = 0; reclaim = 0;
if ((M[3] & 0x80) && (M[3] & (1 << m))) {
  ui_dialog_box("Angebot wurde", "bereits abgelehnt"); goto unhighlight;
}
ui_set_font(rp, 1);
if (((M[3] & (1 << m)) && own == 4) || own == m) {
  if (own == m) {
    if (M[0x09] & 0x80) {                                       // offer for my listed player
      FAR:304a = 4;
      offer = player_value_calc(idx, 0);
      FAR:304a = FAR:5358;
      offer = offer * random_range(0x43, 0x6e) / 10000 * 100;   // RNG 67..110 %
      h = transfer_offer_received(&offer, (i32)M[0x0b]);
      M[0x09] ^= 0x80;
      M->msg = message_remove(M->msg);
      if (h != 1) {                                             // VERKAUFEN
        u8 to = M[0x16]; M[0x16] = 0;
        players[pid].team = to;
        club_strength_after_sale(to, ((u16)M[0x10] + M[0x11] + M[0x12]) / 3);   // before removal here
        managers[m].balance += offer;
        FAR:304a = 4; squad_remove_player(idx, 1, 12);
        players[pid].owner = 5;
        FAR:304a = FAR:5358;
      }
      redraw_market = 1;
      offer = 0;
    } else {
      reclaim = 1;                                              // take my own listed player back
    }
  }
  /* else: (M[3] bit m set, M[3] bit 7 clear, owner 4) -> jumps straight to 13b2 and uses `offer`
     unchanged. If no earlier write happened in this screen session, `offer` is UNINITIALISED
     stack data (whatever the caller left at [bp-6]/[bp-4]); otherwise it is the last value
     written (a previous typed bid, a previous offer, or 0). The bit-exact first-use value is
     stack-dependent (see Open questions). This segment's own rejection always sets bit m together
     with bit 7, but byte 0x03 of a market record doubles as the stat byte b3 (§2.1), so a
     listed record whose stat value has bit m set and bit 7 clear reaches this path without any
     rejection: the click then skips the "Ihr Angebot:" input and bids the stale `offer`. */
} else {
  ui_draw_text_at(rp, "Ihr Angebot:", 0xa5, 0x8c, 1);
  ui_draw_line(rp, 0xa3, 0x8e, 0x13a, 0x8e, 1);
  offer = ui_scroll_text_box(FAR:6dc0, buf, 0xa7, 0x98, 0x3ef, 1, 8, 0) * currency_div / currency_mul;
  ui_set_font(rp, 2);
}
if (!((offer != 0 && managers[m].balance >= offer) || reclaim)) {
  if (offer != 0) ui_dialog_box("Sie haben nicht", "genug Geld");
  goto clear_box;
}
proceed = 1;
if (!reclaim && own != 4) {                                     // listed by another human manager h = own
  FAR:304a = 4; proceed = 0;
  ref = player_value_calc(idx, 0);
  FAR:304a = FAR:5358;
  if (loan) ref /= 3;
  refused_msg = 0;
  if (ref * 60 / 100 > offer || ref * 140 / 100 < offer) {
    message_broadcast(own, "DIESER BETRAG LIEGT", "AU~ERHALB DES ERLAUBTEN !", "", "NIE", "NEIN");
    refused_msg = 1;
  } else {
    l1 = "NEHMEN SIE DAS ANGEBOT (" + prefix? + fmt(offer * currency_mul / currency_div) + " DM)";
    l2 = "F]R " + players[pid].name + " AN ?";
    if (message_broadcast(own, l1, l2, "", "NA GUT.", "NEIN !")) {
      proceed = 1; from_human = 1;
      players[pid].skills[0..2] = M[0x10..0x12];
    }
  }
}
if (!proceed) goto rejected;
if (!from_human && !reclaim && !market_fee_offer_accepted(idx, offer, loan)) goto rejected;   // RNG 1-3
club = players[pid].team;
k = transfer_list_add(pid, 1, (u8)(0x63 * loan), 0L, 0x18 + reclaim);    // into own squad; RNG inside
old_sal = squad(m, k)->salary;                                  // read even if k == 0x7f
if (k == 0x7f) goto clear_box;                                  // no message here
if (!reclaim && !loan && !contract_negotiation(k, 0, 0, 0)) {
  squad_remove_player(k, 1, 0x18);
  players[pid].team = club;
  ui_dialog_box("Der Transfer", "findet nicht statt");
  goto mark_rejected;
}
S = squad(m, k);
if (from_human) { S[9] = M[9]; S[0x17] = M[0x17]; S[0x0d] = M[0x0d]; S[0] = M[0]; S[1] = M[1]; S[2] = M[2]; }
if (reclaim || loan) {
  memcpy(S, M, 0x34);
  if (loan) { S[0x0b] = 1; S[0x0c] = club + 0x80; S->salary = old_sal; S[5] = S[4] = S[3] = 0; }
}
FAR:304a = 4; squad_remove_player(idx, 1, 12); FAR:304a = FAR:5358;
if (from_human) {
  managers[players[pid].owner].balance += offer;
  players[pid].team = managers[m].club;                         // FAR:2260 + m*0x30a
}
if (!loan || reclaim) players[pid].owner = m;
redraw_market = redraw_own = 1;
if (from_human) managers[m].balance -= offer;
if (!reclaim && !from_human) { managers[m].balance -= offer; sponsor_offer_dialog(offer); }
squad_weekly_update();
goto clear_box;
rejected:
  ui_dialog_box("Ihr Angebot", "wurde abgelehnt");
mark_rejected:
  if (!from_human && !refused_msg) M[3] |= (1 << m) | 0x80;    // 8-bit
  refused_msg = 0;
clear_box:
  ui_fill_rect(rp, 0xa3, 0x84, 0x13a, 0x9b, 8);
unhighlight:
  if (!redraw_own) XOR-unhighlight row hover (fill 0x1e, mode 2 then 0);
  hover = 0x7f;
```
Notes for porting: on a loan (`loan && !reclaim`) the player's `owner` is **not** changed; the loan fee
is charged like a purchase and `sponsor_offer_dialog(offer)` is called. A purchase from the club pool
skips `contract_negotiation` when `loan` is set. A purchase from another human skips
`market_fee_offer_accepted` but not the negotiation. `market(i)` byte 3 doubles as a stat byte of
records copied in from a squad, so a freshly listed player can already carry "rejected" bits.

**Hit areas / keys summary**

| Area | Action |
|---|---|
| own list (`player_card`) row, left click | own player: sell offer dialog / put on market |
| own list row, event 3 | `screen_player_info(slot)` |
| market row r (y = 0x1d+6r.., x 0xa1..0x139), hover | highlight + "VON ..." line at y 0xbe |
| market row, left click (via SI 0x65 path) or event 3 / 0xe8 | `market_click(r-1)` (event 3 on a row = info only if it is the own listed player) |
| any click below y 0x6d outside the list (SI 0x65) | toggle KAUFEN/LEIHEN unless `FAR:513c` is 0x7ac or 0x7c9 |
| exit (`player_card` 0x66, button at 0x10c,0xc1) | `ui_wait_with_highlight`, `manager_ai_weekly(m, 0)`, return |

RNG order in the screen: per action as given above; hover and drawing use no RNG.

## 5. `club_strength_after_sale` (24a4:23da, other segment, only called from 3277:04a5)

```c
void club_strength_after_sale(u8 club, i16 avg)
{
    Club far *c = &clubs[club];
    i16 sum = 0, X /* uninitialised local [bp-4] */;
    for (i16 i = 0; i < 3; i++) sum += c->strength[X] + c->base_strength[X];   // BUG: X, not i
    i16 s = (avg - sum / 6 + 10) / 3;
    i16 k = random_range(0, 2);                       // RNG#1
    clamp_int(&s, 1, 10);
    c->strength[k]      += (u8)random_range(s - 1, s + 1);    // RNG#2
    c->base_strength[k] += (u8)random_range(s - 1, s + 1);    // RNG#3
}
```
`X` is stack garbage (byte reads at `club + 0x18 + X` and `club + 0x1b + X` in FAR). It does not change the
RNG sequence (always 3 calls), only the deltas. Analysis of the two call sites: at 3277:0ec9 the slot
holds the high word of the message pointer that `squad_remove_player` read, which `message_remove` had
just set to NULL, so **X = 0**. At 3277:133a the slot belongs to the frame of the preceding
`message_remove` call: low byte = its last loop index if the pointer was non-NULL, otherwise the saved
BP of `screen_transfer_market` left there by `transfer_offer_received`; see open questions.

## 6. Globals used

| Address | Size/type | Suggested name | Save block | Use |
|---|---|---|---|---|
| FAR:304a | i8 | `current_manager` | 3 | index `m`; set to 4 temporarily |
| FAR:5358 | i8 | `saved_manager` | 4 | restore value for FAR:304a |
| FAR:774a | SquadPlayer[5][25] | `squads` | 18 | squads; `FAR:8b9a` = squad 4 = market |
| FAR:57dd | Player[151] | `players` | 17 | age, skills, owner, team |
| FAR:3066 | Club[200] | `clubs` | 15 | names, strength (FAR:307e/307f), base_strength (FAR:3081/3082) |
| FAR:2242 | Manager[4] | `managers` | 13 | name (+0), club FAR:2260, league FAR:237a, training FAR:2383/2388, balance FAR:2432 |
| FAR:513c | u16 | `scenario` (state.md) | 43 | 0x7ac / 0x7c9 disable loans |
| FAR:2ea2 | i16 | `mouse_event` | - | 1 = left, 3 = info, 0xe8 = select |
| FAR:6dc0 | far ptr | `mouse_pos_ptr` | - | `[0]` = y, `[1]` = x |
| FAR:2e70 | far ptr | `pointer_state_ptr` | - | `+4` = y, `+6` = x |
| FAR:2f28 | far ptr | `main_rastport` | - | `rp` |
| FAR:a6ca | far ptr | `screen_bitmap_2` | - | blit source/target (contract screen) |
| DS:07dc | i32 | `day_counter` | 23 | `> 0x141` = late season |
| DS:07ab | u8 | `manager_count` | 7 | (`manager_index_of_team`, refresh probability in caller) |
| DS:07b2 | u8 | `no_thousands_sep` | - | `format_long_thousands` separator switch |
| DS:07b3 | i8 | `icon_slot_count` | - | reset before `icon_register` |
| DS:224c | i8 | `season_start_day` | - | |
| DS:224d | u8 | `currency_prefix_flag` | - | |
| DS:224e | i32 | `currency_div` | - | 1 |
| DS:2252 | i32 | `currency_mul` | - | 1 |
| DS:225a | i8[3] | `league_round` | 34 | |
| DS:225e | i8[3] | `tier_team_count` | - | 18, 20, 20 |
| DS:2262 | i8[4] | `league_rounds` | - | 34, 38, 38, 0 |
| DS:226e | i8[3] | `tier_first_team` | - | 0, 18, 38 |
| DS:2272 | u8[3][2] | `tier_team_range` | - | (0,17) (18,37) (38,57) |
| DS:543e | i8[2][6] | `value_weights` | - | §2.2 |
| DS:22e0.., DS:4a68.. | far ptr[] | UI string tables | - | texts quoted inline |

## 7. Open questions

1. `FAR:2ea2` value 3 (used for "player info" and market selection) and 0xe8 (also selects a market row)
   come from `mouse_pop_button_event`; their exact button/key meaning is not verified here (MEMORY_MAP only
   lists 1 and 2).
2. `club_strength_after_sale` (24a4:23da) indexes with an uninitialised local. Site 1 is provably `X = 0`.
   Site 2 (selling an own listed player from the market) depends on the stack: if `M->msg` was NULL,
   X = `screen_transfer_market`'s BP (a fixed stack address, deterministic for a fixed call chain but not
   computed here); otherwise its low byte is `message_remove`'s final index. A port may need an emulator
   run to pin the value, or accept a deviation that does not affect the RNG sequence.
3. The exact semantics of `squad +0x00..0x05`, `+0x0e`, `+0x14`, `+0x16`, `+0x17`, `+0x1c..0x20` beyond
   their use here (names are descriptive only).
4. The constants `0x0003d146`, `0x000090c6` and `0x00020086` passed to `player_card` /
   `screen_transfer_market_list` look like 32-bit column masks; they belong to the squad area (2ecd).
5. `vga_load_picture(0, 0x32, 0x29, 5, 0)` is assumed to allocate an empty 50x41 bitmap (picture 0 =
   none); the VGA agent owns the signature.
6. `ui_scroll_text_box` arguments `0x3e9`, `0x3ee`, `0x3ef` (probably a right x limit or a max value) and
   the colour pair (2/1, 8) are passed through unexplained.
7. `transfer_offer_received` shows a deduction `fee/7 * contract_len` labelled `"Abl|se"` and pays only
   the remainder; whether this models a contract buy-out is an interpretation.
8. MEMORY_MAP calls DS:224e a difficulty level; here it is only used as the currency divisor (with
   DS:2252 as multiplier). The real meaning of the pair (state.md: `option_long_a/b`) should be settled
   by the start-menu owner.
9. `screen_transfer_market`'s `offer` ([bp-6]/[bp-4]) is not initialised at entry. On the path
   "M[3] bit m set, bit 7 clear, owner 4" (§4.11) it is used as is, so the first use in a session reads
   stack garbage left by the caller (office menu dispatcher). Needs an emulator run to pin; a port could
   initialise it to 0 at screen entry (a 0 bid then fails the `offer != 0` check and shows nothing)
   and record this as a known deviation. How the initial byte 0x03 of a fresh market record is filled
   (stat copy vs. 0) decides how often this path is hit in practice.
