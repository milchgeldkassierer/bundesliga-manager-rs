# Randomness and core numeric helpers (`rng`)

Port: `crates/bmp-game/src/rng.rs`. All addresses are in `/unpacked/BMMAIN_unpacked.EXE`.
Integer model: MSC large model, `int` = `i16`, `long` = `i32`, `char` signed. All arithmetic wraps.
An integer division by zero raises CPU exception 0, which the MSC runtime reports as
"run-time error R6003 - integer divide by 0" before exiting. The Rust port panics in that case,
and the `checked_*` variants return `None` instead.

## 1. Generator state

| Address | Size | Suggested name | Meaning |
|---|---|---|---|
| DS:9720 | word | `rand_state_lo` | low word of the LCG state; the data image holds **1** |
| DS:9722 | word | `rand_state_hi` | high word of the LCG state; the data image holds 0 |

The RNG state is **not** stored in the save game (it is in none of the 61 blocks of savegame.md).
`Rng::default()` returns the power-on state 1.

## 2. Ported functions

| Address | Ghidra name | Rust | Semantics |
|---|---|---|---|
| 4a01:17ac | `crt_srand` | `Rng::srand`, `Rng::new` | `DS:9720 = seed (u16)`, `DS:9722 = 0` |
| 4a01:17be | `crt_rand` | `Rng::rand` | `state = state * 0x343FD + 0x269EC3` (mod 2^32, computed with `_aFlmul`). It returns `(state >> 16) & 0x7FFF`. The reference sequence after `srand(1)` starts 41, 18467, 6334, 26500, 19169 |
| 176b:0cc7 | `random_range` | `Rng::random_range`, `Rng::checked_random_range` | see §2.1 |
| 176b:0cf0 | `random_seed_from_time` | `Rng::seed_from_time` | `srand((unsigned) time(NULL))`: only the **low 16 bits** of `time_t` are used |
| 3e3a:21ef | `random_chance_pick` | `Rng::chance_pick` | see §2.2 |
| 4091:1009 | `clamp_int` | `clamp_int` | `if (*p < lo) *p = lo; if (*p > hi) *p = hi;` with signed 16-bit compares. The two checks run one after the other, so with `lo > hi` the result is `hi` |
| 4091:1033 | `clamp_char` | `clamp_char` | the same for signed `char` |
| 4091:105d | `text_to_upper_de` | `text_to_upper_de` | `(dst, src)` copies up to NUL. It maps `a..z` to `A..Z`, `0x81` (u umlaut) to `0x9A`, `0x84` (a umlaut) to `0x8E` and `0x94` (o umlaut) to `0x99`. Every other byte is unchanged (`0xE1` sharp s too). It writes NUL and returns `dst` |
| 4091:10d5 | `long_abs_diff` | `long_abs_diff` | `b > a ? b - a : a - b` (signed long compare, wrapping subtraction). The full long goes back in DX:AX, and many callers use only AX |
| 176b:0681 | `format_long_thousands` | `format_long_thousands` | see §2.3 |
| 4091:0ba5 | `money_round_display` (UI value spinner) | `spinner_round_bound` | only the bound-rounding prologue is ported, see §2.4. The widget itself belongs to the UI port |
| 4a01:1a4c | `crt_afldiv` (`_aFldiv`) | `ldiv`, `checked_ldiv` | signed long division that truncates toward zero. It works on magnitudes and fixes the sign afterwards, so `MIN / -1 = MIN`. A zero divisor gives R6003. Callee pops 8 bytes |
| 4a01:1b18 | `crt_aflrem` (`_aFlrem`) | `lrem` | signed long remainder with the sign of the dividend |
| 4a01:1ae6 | `crt_aflmul` (`_aFlmul`) | `lmul` | low 32 bits of the product |
| 4a01:1c10 | `crt_afuldiv` (`_aFuldiv`) | `uldiv` | unsigned long division |
| 4a01:1c70 | `crt_afulrem` (`_aFulrem`) | `ulrem` | unsigned long remainder |
| 4a01:1bb8 | `crt_aflshl` (`_aFlshl`) | `lshl` | DX:AX shifted left by CL, one bit per loop pass (CH is cleared). A count of 32 or more gives 0 |
| 4a01:1bc4 | `crt_aflshr` (`_aFlshr`) | `lshr` | **arithmetic** right shift (`SAR DX; RCR AX` loop). A count of 32 or more gives 0 or -1 |
| 4a01:1bd0 | `crt_afldiv_assign` (`_aFNaldiv`) | use `ldiv` | `*(long near *)p /= d` through 1a4c. The near pointer is pushed as one word |
| 4a01:1bf0 | `crt_aflmul_assign` (`_aFNalmul`) | use `lmul` | `*(long near *)p *= m` |
| 4a01:1a08 / 1a2a | `_aFlldiv` / `_aFllmul` (far-pointer assign forms) | use `ldiv` / `lmul` | the same arithmetic |

There are no `_aN*` (near) long helpers. The program is large model, and all long math goes through
the far `_aF*` routines listed above.

### 2.1 `random_range(int lo, int hi)` 176b:0cc7

```
SI = lo; DI = hi
AX = rand()                ; consumed first, always
CX = DI - SI + 1           ; 16-bit wrap
CWD                        ; DX = 0 (rand is 0..0x7FFF)
IDIV CX                    ; DX = AX % CX (sign of the dividend, so >= 0)
AX = DX + SI               ; 16-bit wrap, return
```

* `hi < lo` works: the span is negative, and `r % span == r % |span|`. Example: `random_range(5, 2)`
  returns 5 or 6.
* Spans above 0x7FFF wrap negative. `random_range(0, 0x8FFF)` (save-game key, savegame.md) has span
  `-0x7000` and returns **0..=0x6FFF**, not 0..=0x8FFF. Span 0x8000 (`(0, 0x7FFF)`) returns `rand()`
  unchanged.
* A span of 0 (`hi = lo - 1`, for example `(1, 0)` or `(-32768, 32767)`) traps with R6003. The
  `rand()` value has already been drawn by then.
* `IDIV` cannot overflow, because the dividend is never -32768.

### 2.2 `random_chance_pick(u8 n, u8 dflt, u8 far *flag, u8 lo, u8 hi)` 3e3a:21ef

All bytes are zero-extended. The function computes `r = random_range(1, n)`. If `r > *flag` (signed
int compare), it sets `*flag = 1` (the constant 1, **not** `r`) and returns
`(u8) random_range(lo, hi)`, which uses 2 `rand()` calls in total. Otherwise it returns `dflt` after
1 `rand()` call. `n == 0` traps.

### 2.3 `format_long_thousands(long v)` 176b:0681

Globals (the ES selectors DS:99a8, DS:99ae and DS:99b0 all hold `5cb3`, so these are DGROUP):

| Address | Suggested name | Use |
|---|---|---|
| DS:a306[12] | `fmt_digit_buf` | reversed digits and separators (BSS) |
| DS:a312[..] | `fmt_out_buf` | returned string, far pointer `DS:a312` |
| DS:07b2 | `fmt_no_separators` | byte. Non-zero suppresses separators. MEMORY_MAP calls it "message window open flag (low)". the value spinner 4091:0ba5 clears it before drawing |
| DS:224e/2250 | `game_option_224e` | long. The value 4 selects `,` instead of `.` as separator. MEMORY_MAP calls it "difficulty level" (medium), but savegame.md has the difficulty in DS:4a28, so its meaning is unresolved |
| DS:079c | `fmt_pad_width` | signed byte, the minimum width for non-negative values. Both are true: `vga_load_picture` 176b:0356 sets it to 1 on every call, so after any picture load numbers are padded to one digit and `0` prints as `0` (confirmed against DOSBox: level box 0 of the new-game dialog shows "0") |

Algorithm (`d` and `e` are signed bytes):
1. `neg = v < 0`. If so, `v = -v` with wrapping, so `MIN` stays `MIN`.
2. While `v != 0`: if `!no_sep && d in {3, 7, 11}`, then `buf[d++] = sep`. Then
   `buf[d++] = '0' + _aFlrem(v, 10)` and `v = _aFNaldiv(v, 10)`.
3. Negative: `out[e] = buf[d - e]` for `e = d..1`, then `out[0] = '-'` and `out[d + 1] = 0`.
   Negative values get no padding.
4. Non-negative: `for (e = d; width > e; d++, e++) buf[e] = (e == 0) ? '0' : '^';`. Then `d--`,
   `out[e] = buf[d - e]` for `e = d..0`, and `out[d + 1] = 0`. As a result, `0` with width <= 0 gives
   the **empty string**, and `0` with width 3 gives `^^0`.
5. `i32::MIN` gives garbage digits (`'0' + negative remainder`, last char `(`), exactly as the
   original.

The pad character `^` (0x5E) is presumably drawn as a digit-wide blank by the game fonts (not
verified).

### 2.4 Spinner bound rounding (prologue of 4091:0ba5)

If the long DS:224e > 1 (signed) and the initial value != 0, both the minimum and the maximum are
replaced by `b / 5 * 5` when `b % 5 != 0`, using `_aFlrem`, `_aFldiv` and `_aFlmul`, which
truncate toward zero. The rest of the function is an interactive +/- widget (UI).

## 3. Seeding and call order

* `rand` (4a01:17be) has one caller: `random_range`. `srand` (4a01:17ac) has one caller:
  `random_seed_from_time`.
* `random_seed_from_time` is called twice, both times from `main` (18bc:0418):
  * 18bc:0712, after `mana_load_club_table` (362f:36ec, called at 18bc:067b) and before
    `competition_init` (27c2:09e0, called at 18bc:0740);
  * 18bc:0870, on the new-game path, after `league_init_or_ai(10)` (1f9d:0697, called at 18bc:086a)
    and before `24a4:19fb` and `competition_init(0..3)` (18bc:0888 / 089b).
* Consequence: the 2 `random_range` calls in `mana_load_club_table` (362f:37cd, 362f:3821) run on
  the **power-on state 1**, so they are deterministic, unless something consumed `rand()` earlier.
* The time seed is the MSC `time()` value (`crt_time` 4a01:1384 → `__loctotime_t` 4a01:1596). It
  includes the `TZ` offset (the MSC default without `TZ` is PST8PDT, +8 h) and the US DST rule
  (`_isindst`). Only the low word matters.

## 4. Callers of `random_range` 176b:0cc7

Ghidra lists 262 xrefs. A byte search for `9A C7 0C 6B 17` (`CALLF 176b:0cc7`) finds **267** call
sites. The 5 extra sites are at 1f9d:13f1, 146b, 14b5, 15d8 and 1666, in floating-point-emulator
code (INT 34h-3Dh) after `FUN_1f9d_11e0`, which Ghidra has not disassembled. There are no near calls.

| Segment | Calls | Functions (FUNCTION_MAP name, number of call sites) |
|---|---|---|
| 1000 | 5 | `stadium_expansion_order` 1000:0000 (3), `sponsor_offer_dialog` 1000:272d (2) |
| 1310 | 21 | 1310:0463 (3), 1310:09c5 (2), 1310:12d3 (1), 1310:12fe (1), 1310:1d45 (3), 1310:2c9a (4), `match_simulate_minute` 1310:2ee5 (3), `match_penalty_shootout` 1310:356d (4) |
| 16c7 | 2 | `copy_protection_penalty` 16c7:09f9 (2) |
| 18bc | 7 | `office_random_event` 18bc:0a63 (2), `start_menu_new_or_load` 18bc:2184 (5) |
| 1cb5 | 59 | `season_end_messages` 1cb5:0012 (16), `random_weekly_events` 1cb5:13bd (36), `event_helper_2756` 1cb5:2756 (1), 1cb5:2987 (6) |
| 1f9d | 36 | `manager_ai_weekly` 1f9d:0002 (7), `league_init_or_ai` 1f9d:0697 (5), `match_engine_helper_08e9` 1f9d:08e9 (14), `match_engine_substitutions` 1f9d:0c3b (5), undisassembled FP code 1f9d:13f1..1666 (5) |
| 212a | 12 | 212a:000a (1), 212a:072d (7), `finance_weekly_processing` 212a:0a6d (4) |
| 24a4 | 32 | `tore_scene_play` 24a4:05ec (4), 24a4:14d4 (7), 24a4:174e (1), 24a4:1798 (2), 24a4:23da (3), 24a4:24bf (1), `player_aging_or_development` 24a4:2cb4 (8), 24a4:30cf (6) |
| 27c2 | 5 | `competition_init` 27c2:09e0 (3), `cup_round_setup` 27c2:13a2 (2) |
| 2a58 | 11 | `match_ticker_events` 2a58:0ca3 (2), 2a58:20b2 (4), `special_date_events` 2a58:2a06 (3), `season_main_loop` 2a58:3176 (2) |
| 2ecd | 2 | `screen_lineup` 2ecd:18bf (1), `transfer_list_add` 2ecd:37d8 (1) |
| 3277 | 24 | 3277:000a (4), `screen_transfer_market` 3277:04a5 (2), `transfer_market_init` 3277:1e38 (9), 3277:2171 (3), 3277:2270 (2), `player_value_calc` 3277:25de (3), `contract_negotiation` 3277:2a8f (1) |
| 362f | 2 | `mana_load_club_table` 362f:36ec (2), which runs before seeding (see §3) |
| 3a41 | 1 | 3a41:1fec (1) |
| 3e3a | 29 | 3e3a:0964 (1), `newspaper_template_text` 3e3a:0c0b (1), `christmas_text` 3e3a:0fdf (20), 3e3a:1b29 (5), `random_chance_pick` 3e3a:21ef (2) |
| 4091 | 1 | `message_box` 4091:0190 (1) |
| 422b | 18 | `savegame_helper_022e` 422b:022e (2), `savegame_write_all` 422b:035c (12), `screen_save_game` 422b:07fe (4) |
| **Total** | **267** | |

`random_chance_pick` has another 14 callers (FUNCTION_MAP), which consume RNG indirectly. Raw
call-site lists are produced by `out/rng/group.py` (reads `out/rng/xrefs.json`, saved from the
GhidraMCP `get_xrefs_to` endpoint).

Porting rule: every port must call `Rng::random_range` in exactly the original order, including
calls whose results are discarded or that only feed UI text (newspaper, Christmas text, save-file
padding). Each call advances the shared state.

## 5. Ghidra changes made

* Renamed: `clamp_int` 4091:1009, `clamp_char` 4091:1033, `text_to_upper_de` 4091:105d,
  `long_abs_diff` 4091:10d5, `random_chance_pick` 3e3a:21ef. `random_range`,
  `random_seed_from_time` and `format_long_thousands` were confirmed.
* Plate comments: 176b:0cc7, 3e3a:21ef, 176b:0681.
