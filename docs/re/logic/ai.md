# Computer clubs, team strength and match AI (segment 1f9d)

Spec for porting segment `1f9d` of `BMMAIN.EXE` bit-exactly, plus the helpers it depends on.
All addresses are Ghidra addresses in `/unpacked/BMMAIN_unpacked.EXE`. `DS:xxxx` is DGROUP `5cb3`,
`FAR:xxxx` is the far data segment `5238`. Selector words `DS:9b16..9b48` were read from the image and
resolve as follows: `9b16 9b18 9b1c 9b1e 9b20 9b26 9b28 9b2a 9b2c 9b2e 9b30..9b3c 9b42..9b48` select
`FAR`; `9b1a 9b22 9b24 9b3e 9b40 9b7a 9b7c 9b7e` select `DS`.

Scratch material (disassembly dumps, the FP-emulator translator) is in `out/ai/`.

## 0. How the game models computer clubs

The FUNCTION_MAP labels in this segment (`manager_ai_weekly`, `league_init_or_ai`, "match engine
helpers") came from callers only. The code shows the following model:

* **There is no per-player AI.** Only the human managers' clubs have player squads (`FAR:774a`, 25
  records of 0x34 bytes per squad, squads 0-3 = managers, squad 4 = 12-slot extra squad).
* Every club (200 records at `FAR:3066`, stride `0x22`) has **three strength arrays of three bytes**,
  one byte per line (0 = defence, 1 = midfield, 2 = attack):

  | Club field | FAR (club 0) | Name used here | Init (mana.md) | Shown by 3a41:1d18 under |
  |---|---|---|---|---|
  | `+0x17` | `307d` | `str_bonus` | `random(0x2d,0x37)` | – (adds `-44` in `team_strength_value`) |
  | `+0x18..+0x1a` | `307e` | `str_a[3]` | MANA.DAT strength | first column, header `"KONDITION"` |
  | `+0x1b..+0x1d` | `3081` | `str_b[3]` | MANA.DAT strength | second column, `"TECHNIK"` |
  | `+0x1e..+0x20` | `3084` | `str_c[3]` | `random(0x2d,0x37)` (-5 if club > 64) | third column, `"FORM"` |
  | `+0x21` | `3087` | `crest_flags` | crest index; bit 7 = "watch this club" flag | |

  The header string is `"PL.     VEREIN               KONDITION   TECHNIK    FORM"` (DS:2f61); the
  A/B/C to header mapping comes from the column order in `3a41:1d18` and is not otherwise confirmed.
* A **computer club's** strength changes only through `league_strength_drift` (1f9d:0697), a random walk
  that runs once per season-loop iteration and once at the start of each season.
* A **human club's** strength is overwritten from its starting XI by `squad_recalc_team_strength`
  (1f9d:0002) whenever the lineup, training, transfers or the week change.
* Matches that are not shown live are decided by `match_simulate_quick_result` (1f9d:08e9) from
  `team_strength_value` (1310:1468). In the live view, `match_goal_chance` (1f9d:0c3b) decides each goal
  chance.
* `match_attendance_estimate` (1f9d:11e0) is a finance function (spectator count) that sits in this
  segment. It is specified here because the segment is assigned to this area.

AI transfers, AI lineups and sacking managers are **not** in this segment; see §4 and the open questions.

## 1. Function list (segment 1f9d, all 6 functions)

| Address | Ghidra name (renamed in this task) | Old FUNCTION_MAP label | Signature | Callers |
|---|---|---|---|---|
| 1f9d:0002 | `squad_recalc_team_strength` | *manager_ai_weekly* | `void far (u8 squad, u8 full)` | 18bc:3940, 2a58:323a, 2a58:327f, 2a58:3728, 2a58:1b77, 2ecd:24c0, 3a41:122f, 3a41:1cfc, 212a:0720, 3277:1b52 |
| 1f9d:0697 | `league_strength_drift` | *league_init_or_ai* | `void far (u8 mode)` | 18bc:086a (main, mode 10), 2a58:31fc (season loop, mode 1), 2a58:40e5 (season loop, new season, mode 10) |
| 1f9d:08e9 | `match_simulate_quick_result` | *match_engine_helper_08e9* | `void far (u8 comp, u8 slot, u8 home, u8 away, i16 t0, i16 t1, u8 store)` | 1310:23c2, 1310:246c (match_live_view), 1310:34a8 |
| 1f9d:0c3b | `match_goal_chance` | *match_engine_substitutions* | `i16 far (u8 comp, u8 slot, u8 home, u8 away, u8 side, i16 minute, u8 ui_arg)` | 1310:26bd, 1310:2710, 1310:289a, 1310:290f (match_live_view) |
| 1f9d:1122 | `squad_lineup_totals` | `FUN_1f9d_1122` | `void far (i16 far *sum_10, i16 far *sum_11, i16 far *sum_13, i16 far *avg_13)` | 212a:2be3 (screen_training) |
| 1f9d:11e0 | `match_attendance_estimate` | `FUN_1f9d_11e0` | `i32 far (u8 mgr, u8 home, u8 away, u8 kind, u8 unused, u8 fixed_cap, u8 pop_override)` | 2a58:2201, 2a58:24c2 (2a58:20b2) |

All functions are MSC large-model `cdecl` far functions (`_chkstk` prologue, `RETF`). Byte parameters
arrive as words; only the low byte is meaningful.

`match_attendance_estimate` uses the MS floating-point emulator (`INT 34h..3Dh`). Ghidra stops its
function body at `1f9d:13c8`; the real body ends with `RETF` at `1f9d:18d9`. The listing in
`out/ai/d11e0_fp.txt` was made by rewriting `CD 34+n` to `9B D8+n` and disassembling with `objdump`.

### 1.1 Helpers called from this segment (outside the area; specified so the port is complete)

| Address | Suggested name | Signature | Semantics |
|---|---|---|---|
| 176b:0cc7 | `random_range` (existing) | `i16 (i16 lo, i16 hi)` | `rand() % (hi-lo+1) + lo`, MSC LCG (FUNCTION_MAP §3.4). Every call below consumes exactly one `rand()` |
| 4091:1009 | *clamp_i16* | `void (i16 far *v, i16 lo, i16 hi)` | `if (*v < lo) *v = lo; if (*v > hi) *v = hi;` (signed) |
| 4091:1109 | *squad_count_players* | `u8 (u8 squad, u8 what)` | `n = (squad == 4) ? 12 : 24;` counts `i in 0..n-1` of squad records: `what==0`: `id(+0x0f) != 0`; `what==1`: `1 <= slot(+0x0a) <= 11` (starting XI); `what==2`: `slot > 11` |
| 4091:10d5 | *abs_diff_i32* | `i32 (i32 a, i32 b)` | `b >= a ? b - a : a - b` (signed 32-bit compare) |
| 27c2:1bb5 | *league_of_club* | `i16 (u8 club)` | `club < (s8)DS:226f ? 1 : club < (s8)DS:2270 ? 2 : 4` (initial values 18 and 38) |
| 1310:1bd7 | *player_role_mismatch* | `i16 (u8 squad, u8 idx)` | `p = player(squad, idx)`; `return (i16)abs_diff_i32((u8)FAR:57fd[0x25*p.id], (i32)(s8)p.role)` |
| 1310:1cd3 | *player_position_mismatch* | `i16 (u8 squad, u8 idx)` | `x = ((7 - (s8)p.pos_code) * 75) / 7 + 5` (16-bit `idiv`); `return (i16)abs_diff_i32((u8)FAR:57fc[0x25*p.id], x)` |
| 1310:1468 | `team_strength_value` | `i16 (i16 club, i16 kind, i16 minute)` | see §2.7 |
| 1310:32b1 | `manager_index_of_team` | `i16 (u8 a, u8 b)` | `for i in 0..DS:07ab-1: if mgr[i].club == a return i+1; if mgr[i].club == b return i+0x81;` then `return 0` |
| 1310:12d3 | *goal_pick* | `s8 (u8 a, u8 b)` | `if (random_range(0,1) == 0) return b; return (s8)b < (s8)a ? a : b;` |
| 1310:12fe | *goal_minutes_generate* | `void (i16 comp, i16 slot, i16 n, i16 lo, i16 hi)` | see §2.8 |
| 4a01:1a4c / 1ae6 | `crt_afldiv` / `crt_aflmul` | MSC `_aFldiv` / `_aFlmul` | 32-bit signed divide (truncate toward zero) / multiply (wrap) |
| 4a01:3156 | (named `crt_fp_emul_init` in Ghidra) | MSC `_ftol` | used after FP code: converts `ST0` to a 32-bit long, truncating toward zero |

UI and library calls (only in `match_goal_chance`): `ui_set_font` 176b:0c91, `ui_fill_rect` 16c7:083c,
`vga_wait_vretrace` 45ac:34ef, `ui_draw_text_centered_shadow` 16c7:087c, `delay_ticks` 4930:01e7,
`format_long_thousands` 176b:0681, `crt_fstrcpy` 4a01:0e18, `crt_fstrcat` 4a01:0dd2. Match-area
calls: `2a58:0ca3` (goal event/scorer handling), `2a58:009d`, `1310:322c`, `1310:1def` (redraw ticker).

### 1.2 Record layouts used

Squad player record `P = FAR:774a + 0x34*(squad*25 + i)`:

| Offset | FAR (squad 0, i 0) | Type | Use here (suggested name) |
|---|---|---|---|
| `+0x03` | 774d | u8 | `exp` (added as `exp-1` to `sum_b`) |
| `+0x06` | 7750 | u8 | `stamina6` (`/6` added to `sum_a`) |
| `+0x0a` | 7754 | u8 | `slot`: 0 = not in lineup, 1..11 = starting XI, 12..16 = substitutes |
| `+0x0f` | 7759 | u8 | `id`: index into the player table `FAR:57dd`, stride 0x25 |
| `+0x10` | 775a | u8 | `attr_a` (averaged into club `str_a`) |
| `+0x11` | 775b | u8 | `attr_b` (averaged into `str_b`) |
| `+0x12` | 775c | u8 | `attr_c` (averaged into `str_c`) |
| `+0x13` | 775d | u8 | `fatigue` (`/20` subtracted from `sum_a`; raised by `random+4` in 2ecd:18bf) |
| `+0x19` | 7763 | s8 | `role` (tactical role 0..6 in the lineup) |
| `+0x1a` | 7764 | s8 | `pos_code`; position index `pos = 7 - pos_code` (0 = goalkeeper slot … 7 = striker) |

Player table `FAR:57dd + 0x25*id`: `+0x1f` (`FAR:57fc`) = natural position on a 5..80 scale
(`< 25` = goalkeeper); `+0x20` (`FAR:57fd`) = natural role.

Manager record `M = FAR:2242 + 0x30a*m` (save block 13): `+0x1e` (`FAR:2260`) club index;
`+0x10a` (`234c`) u8 popularity; `+0x131` (`2373`) u8 lineup slider 0..34 (default 16, set in
2ecd:18bf from the mouse x position, `x - 0x113` clamped to 0..0x22); `+0x138` (`237a`) u8 league tier
0..2; `+0x13d` (`237f`) u8 match aggression 0..40, or 100 = "fewer than 8 starters"; `+0x15e`
(`23a0`) stadium block (§2.6); `+0x1dc` (`241e`) u16 (initialised to `50 - 20*x` in 18bc:3916).

## 2. Functions

### 2.1 `squad_recalc_team_strength` — 1f9d:0002

`void far squad_recalc_team_strength(u8 sq, u8 full)`

Callers and arguments:

| Call site | Arguments | Context |
|---|---|---|
| 18bc:3940 | `(FAR:304a, 0)` | new-game setup per manager, after `squad_weekly_update` 2ecd:3360 |
| 2a58:323a | `(FAR:304a, 0)` | season loop, per manager on match weeks, after `squad_weekly_update` |
| 2a58:327f | `(i, 1)` for `i = 0..DS:07ab-1` | season loop, after the match day (then sets `DS:079f[2i]=DS:079e[2i]`, `DS:079e[2i]=1`, `FAR:1d14[i]=0`, `FAR:90c6[i]=0`) |
| 2a58:3728 | `(FAR:304a, 0)` | season loop, second per-manager pass |
| 2a58:1b77 | `(arg, 1)` | 2a58:0ca3 (match event handling) after `DS:0664[arg]--` |
| 2ecd:24c0 | `(FAR:304a, mode != 0)` | end of the lineup screen 2ecd:18bf; `mode` = the screen's first argument `[bp+6]` (u8). Code: `CMP byte [bp+6],1; SBB AL,AL; INC AL; PUSH AX`, so full = 1 when mode != 0, else 0 (the full recalculation uses the RNG, so getting this wrong desyncs the stream) |
| 3a41:122f / 3a41:1cfc | `(i, 0)` / `(i, 1)` for all managers | fixtures screen 3a41:120a entry/exit |
| 212a:0720 | `(FAR:304a, 0)` | training camp screen |
| 3277:1b52 | `(FAR:304a, 0)` | transfer market screen |

No RNG is used when `full == 0`.

```c
void squad_recalc_team_strength(u8 sq, u8 full)
{
    i16 bal[3]  = { -9, -8, -5 };   /* line balance  [bp-24..-20] */
    i16 bonus[3] = {0,0,0};         /* random bonus  [bp-6..-2]   */
    i16 cnt[3]  = {0,0,0};          /* players per line           */
    i16 sum_c[3] = {0,0,0};
    i16 sum_a[3] = {0,0,0};
    i16 sum_b[3] = {8,8,8};
    i16 keeper = 1;                 /* 1 = no goalkeeper found    */
    i16 role_pts = 0;               /* [bp-0c]                    */
    i16 diff_sum = 0;               /* [bp-26]                    */
    i16 n_xi = 0;                   /* [bp-0a]                    */
    i16 rr = 0;                     /* [bp-38]                    */
    i16 n = squad_count_players(sq, 0);

    for (i16 i = 0; i < n; i++) {
        P *p = &squad[sq][i];
        if (p->slot == 0 || p->slot >= 12) continue;      /* unsigned byte compare */
        n_xi++;
        i16 pos = 7 - (i16)(s8)p->pos_code;
        i16 ln  = (i16)(s8)DS_02a2[pos];                  /* 0 0 0 1 1 1 2 2 for pos 0..7 */
        if (full) {
            i16 role = (s8)p->role;
            if (role == 1 || role == 5) role_pts += 1;
            if (role == 0 || role == 6) role_pts += 2;
            if (pos < 3) {
                bal[0] += 2;
                if (pos == 1 && role == 3 && player_role_mismatch(sq, i) < 2
                    && (i16)p->attr_b - 10 > (i16)p->attr_a)
                    sum_b[ln] += 15;
                if (pos == 0 && role == 3)
                    keeper = ((u8)FAR_57fc[0x25 * p->id] / 25 != 0) ? 2 : 0;   /* u8 div */
                /* no random bonus for pos < 3 */
            } else {
                if (pos == 3) {
                    bal[0]++; bal[1]++;
                    if (role > 1 && role < 5 && player_role_mismatch(sq, i) < 2
                        && (i16)p->attr_b - 10 > (i16)p->attr_a)
                        sum_b[ln] += 15;
                } else if (pos < 6) bal[1] += 2;
                else if (pos == 6) { bal[1]++; bal[2]++; }
                else bal[2] += 2;
                if (player_role_mismatch(sq, i) > 2)      bonus[ln] += random_range(2, 6);
                if (player_position_mismatch(sq, i) > 25) bonus[ln] += random_range(2, 6);
            }
            diff_sum  += (i16)p->attr_a - (i16)p->attr_b;
            sum_a[ln] += (p->stamina6 / 6) - (p->fatigue / 20) + 3;   /* u8 divisions */
            sum_b[ln] += (i16)p->exp - 1;
        }
        sum_a[ln] += p->attr_a;
        sum_b[ln] += p->attr_b;
        sum_c[ln] += p->attr_c;
        cnt[ln]++;
    }

    for (i16 j = 0; j < 3; j++)   /* runs even when full == 0 (bonus is 0 then) */
        sum_b[j] += (i16)((i16)(((i16)(s8)DS_4a28 - 5) * bonus[j]) * 20) / 100;  /* 16-bit, idiv */

    if (n_xi == 0) sum_b[0] = sum_b[1] = sum_b[2] = 0;
    M[sq].aggression /* +0x13d */ = (n_xi < 8) ? 100 : 0;

    /* pad the line counts up to 11 players, round robin from line 0 */
    while (n_xi != 0) {
        i16 old = n_xi++;
        if (old >= 11) break;
        cnt[rr]++;
        if (++rr == 3) rr = 0;
    }

    if (full) {
        if (DS_4a28 == 0 && role_pts > 8) sum_b[2] += -100 * cnt[2];
        if (keeper != 0) {
            keeper--;                         /* 1 -> 0 (no keeper), 2 -> 1 (bad keeper) */
            sum_b[0] = (i16)((i32)random_range(10*keeper + 5, 20*keeper + 5) * sum_b[0] / 100);
            sum_b[1] = (i16)((i32)random_range(10, 40) * sum_b[1] / 100);
            sum_a[0] = (i16)((i32)random_range(5*(keeper + 2), 30*keeper + 20) * sum_a[0] / 100);
        }
        clamp_i16(&sum_b[0], 0, 32000);
        clamp_i16(&sum_b[1], 0, 32000);
        clamp_i16(&sum_a[0], 0, 32000);
        if (bal[1] < 1) {
            if (bal[0] > 0) bal[2] += bal[1] - 1;
            if (bal[2] > 0) bal[0] += bal[1] - 1;
        }
        if (bal[0] < -1) {
            if (random_range(7, 10) > bal[0] + 10)
                bal[1] -= random_range(3, 5);
        }
        for (i16 j = 0; j < 3; j++) {
            sum_b[j] += (s8)DS_4a28;
            if (sum_a[j] != 0) sum_a[j] += (i16)M[sq].slider /* +0x131 */ - 16;
            if (sum_b[j] != 0 && (s8)DS_4a28 < 4) {
                i16 t = bal[j] * cnt[j];                         /* 16-bit */
                sum_b[j] += ((i16)FAR_90c6[sq] * 3 + t) * 10;
                sum_a[j] += t * 5;
            }
            if (sum_b[j] < 0) sum_b[j] = 0;
            if (sum_a[j] < 0) sum_a[j] = 0;
        }
        diff_sum /= 9;                         /* idiv, truncate */
        clamp_i16(&diff_sum, -4, 4);
        if (diff_sum < 0) diff_sum = 0;
        diff_sum += M[sq].slider;
        clamp_i16(&diff_sum, 0, 40);
        if (M[sq].aggression != 100) M[sq].aggression = (u8)diff_sum;
    }

    u8 club = M[sq].club;                      /* FAR:2260 + 0x30a*sq */
    for (i16 j = 0; j < 3; j++) {
        if (cnt[j] <= 1) {
            CLUB[club].str_c[j] = 0; CLUB[club].str_b[j] = 0; CLUB[club].str_a[j] = 0;
        } else {
            CLUB[club].str_a[j] = (u8)(sum_a[j] / cnt[j]);   /* signed 16-bit idiv, low byte */
            CLUB[club].str_b[j] = (u8)(sum_b[j] / cnt[j]);
            CLUB[club].str_c[j] = (u8)(sum_c[j] / cnt[j]);
        }
    }
}
```

Notes:

* `random_range` arguments are `(lo, hi)`. The three keeper ranges are `(5,5)`, `(10,40)` and `(10,20)`
  without a goalkeeper (`keeper` 1 → 0), and `(15,25)`, `(10,40)` and `(15,50)` with a misplaced
  goalkeeper. `(32-bit product) / 100` uses `_aFldiv` and keeps the low word.
* `-100 * cnt[2]` is evaluated as `0xff9c * cnt[2]` with `IMUL` (low word).
* `FAR:90c6[sq]` (u8) is incremented in the lineup screen at 2ecd:21b6 and reset to 0 after each match
  day at 2a58:32b8. It is not in a save block (the bytes just after save block 56 `FAR:90c2`).
* `DS:4a28` is the difficulty (save block 42, `5 - user level`).

RNG order for `full != 0`: for each starter at `pos >= 3`, in squad order, `random_range(2,6)` if
`role_mismatch > 2`, then `random_range(2,6)` if `position_mismatch > 25`. Then, if `keeper != 0`,
three calls (B0, B1, A0 ranges). Then `random_range(7,10)` if `bal[0] < -1`, and `random_range(3,5)` if
that check passes.

### 2.2 `league_strength_drift` — 1f9d:0697

`void far league_strength_drift(u8 mode)`; mode 1 = weekly drift, 10 = new game / new season.

```c
void league_strength_drift(u8 mode)          /* 'mode' is a writable parameter copy */
{
    for (u8 t = 0; t < 0x40; t++) {          /* German clubs 0..63, all of them */
        u8 lg = (u8)(league_of_club(t) >> 1);              /* 0, 1 or 2 (SAR then low byte) */
        u8 r0 = (u8)random_range(0, (u16)mode * 2);
        if (mode != 1) {
            CLUB[t].crest_flags &= 0x7f;     /* FAR:3087 + 0x22*t */
            mode = 3;                        /* sticks for every later club */
        }
        for (u8 j = 0; j < 3; j++) {         /* str_a */
            i16 old = CLUB[t].str_a[j];      /* u8, zero-extended */
            i16 k = (mode == 3) ? 3 : 1;
            i16 v = random_range(0, (k + r0) * 2) + old - k - mode;
            if (mode == 3) clamp_i16(&v, DS_05ce[2*lg], DS_05cf[2*lg]);
            else           clamp_i16(&v, 1, 99);
            CLUB[t].str_a[j] = (u8)v;
        }
        for (u8 j = 0; j < 3; j++) {         /* str_b */
            i16 k = (mode == 3) ? 3 : 1;
            i16 v = CLUB[t].str_b[j];
            if (random_range(0, 2) == 0 || mode == 3)        /* call is always made */
                v += random_range(0, (k + r0) * 2) - mode - k;
            if (mode == 3) clamp_i16(&v, DS_05ce[2*lg], DS_05cf[2*lg]);
            else           clamp_i16(&v, 1, 99);
            CLUB[t].str_b[j] = (u8)v;
        }
        for (u8 j = 0; j < 3; j++) {         /* str_c */
            i16 v = CLUB[t].str_c[j] + random_range(0, 6) - 3;
            clamp_i16(&v, 0x2d, 0x37);
            CLUB[t].str_c[j] = (u8)v;
        }
    }
}
```

Bit-exact quirks:

* With `mode = 10`, only club 0 draws `r0 = random_range(0, 20)`. `mode` is already 3 when its loops
  run, so `k = 3` and `- mode` subtracts 3. Every later club draws `r0 = random_range(0, 6)`.
* With `mode = 1`: `v = old + random(0, 2*(1+r0)) - 2`, `r0 ∈ {0,1,2}`, clamp 1..99. `str_b` moves
  only when `random_range(0,2) == 0`.
* Human-managed clubs are included; their values are overwritten later by §2.1.
* RNG calls per club: 1 (`r0`) + 3 (`str_a`) + 3..6 (`str_b`: always `random(0,2)`, plus one more call
  when that call returns 0 or `mode == 3`) + 3 (`str_c`).

Clamp table `DS:05ce` (u8 pairs `lo, hi` per league tier; not in a save block, constant):

| lg | league | DS | lo | hi |
|---|---|---|---|---|
| 0 | Bundesliga (clubs `< DS:226f` = 18) | 05ce/05cf | 70 | 93 |
| 1 | 2. Bundesliga (`< DS:2270` = 38) | 05d0/05d1 | 40 | 74 |
| 2 | lower tier | 05d2/05d3 | 15 | 45 |

### 2.3 `match_simulate_quick_result` — 1f9d:08e9

`void far match_simulate_quick_result(u8 c, u8 s, u8 home, u8 away, i16 t0, i16 t1, u8 store)`

Decides the goals for one fixture played between minute `t0` and `t1` (callers pass 0/45/90-style
ranges). `c` = competition row (callers pass `[bp-0x36]` or `[bp-0x38]+10` for cups), `s` = even slot
(home goals at `s`, away goals at `s+1`).

Result table `R(c,s) = FAR:6dc8 + ((c*0x26 + (s8)DS:225a[c]) * 0x14) + s` (s8 per byte). From
`FAR:6ddc` on it is save block 6. The value `0x1e` marks a fixture that is not to be simulated.

```c
void match_simulate_quick_result(u8 c, u8 s, u8 home, u8 away, i16 t0, i16 t1, u8 store)
{
    i16 gh = 0, ga = 0;
    i16 mh = manager_index_of_team(home, 0xff);
    i16 ma = manager_index_of_team(away, 0xff);
    if (*(u8*)R(c,s) == 0x1e) return;
    gh += random_range(0, 1);
    i16 sh = team_strength_value(home, 1, t0);
    i16 sa = team_strength_value(away, 1, t0);
    i16 d  = sa / -3 + sh / 3;                    /* two signed idivs */
    if (d > 0) {
        i16 mx = (d > 19) ? d / 5 : DS_02aa[d];
        i16 r1 = random_range(1, mx);
        i16 r2 = random_range(1, mx);
        if (random_range(0, 1) != 0 && d > 8) gh += (s8)goal_pick((u8)r1, (u8)r2);
        else                                  gh  = r1;
        if (d < 25) ga = random_range(0, 2);
    } else {
        i16 e  = -d;
        i16 mx = (e > 19) ? e / 5 : DS_02aa[e];
        if (e < 25) gh += random_range(0, 2);
        i16 r1 = random_range(1, mx);
        i16 r2 = random_range(1, mx);
        if (random_range(0, 2) == 0 && e > 8) ga += (s8)goal_pick((u8)r1, (u8)r2);
        else                                  ga  = r1;
    }
    ga += random_range(0, 2);
    gh += random_range(0, 3);
    while (gh > 8 || ga > 8) { gh--; ga--; }
    if (gh > 4 && random_range(0, 1) != 0) gh--;
    if (ga > 4 && random_range(0, 1) != 0) ga--;
    if ((gh > 2 || ga > 2) && random_range(0, 1) != 0) { gh--; ga--; }
    if (gh < 0) gh = 0;
    if (ga < 0) ga = 0;
    if (sh == 0) { gh = 0; if (sa > 20) ga = 8; }
    if (sa == 0) { ga = 0; if (sh > 20) gh = 8; }
    i16 span = t1 - t0;
    gh = (i16)(span * gh) / 45;                   /* 16-bit product, idiv */
    ga = (i16)(span * ga) / 45;
    if (t0 > 90) { gh++; ga++; }
    *(i32*)FAR_5788 += (i32)(i16)(gh + ga);       /* total goals statistic */
    if (store) {
        goal_minutes_generate(c, s,     gh, t0, t1);
        goal_minutes_generate(c, s + 1, ga, t0, t1);
        if (mh) { FAR_21d8[mh*2] = (u8)gh; FAR_21d9[mh*2] = (u8)ga; }
        if (ma) { FAR_21d8[ma*2] = (u8)gh; FAR_21d9[ma*2] = (u8)ga; }
    } else {
        FAR_6dbc = (u8)gh; FAR_6dbd = (u8)ga;
    }
}
```

`DS:02aa[0..19]` (maximum goals per strength difference, u8): `1 2 2 2 2 2 3 3 3 3 3 3 3 3 4 4 4 4 4 4`.

With `d == 0` the `else` branch runs with `e = 0`, so `mx = 1`. The function writes no goals into
`R(c,s)` itself. The caller adds them, or `goal_minutes_generate` records the minutes.

### 2.4 `match_goal_chance` — 1f9d:0c3b

`i16 far match_goal_chance(u8 c, u8 s, u8 home, u8 away, u8 side, i16 minute, u8 ui_arg)`;
`side` 0 = the home team attacks, 1 = the away team attacks. Returns 1 if a goal was scored.

```c
i16 match_goal_chance(u8 c, u8 s, u8 home, u8 away, u8 side, i16 minute, u8 ui_arg)
{
    u8 live = 0x1e; i16 goal = 0;
    *(i32*)FAR_6db0 += 1;                                  /* chances statistic */
    i16 adv_away = (team_strength_value(away, 2, minute) - team_strength_value(home, 0, minute)) / 3;
    i16 adv_home = (team_strength_value(home, 2, minute) - team_strength_value(away, 0, minute)) / 3;
    /* call order: home kind 0, away kind 2, away kind 0, home kind 2 */
    i16 gh, ga;
    if (c < 10) { gh = (s8)R(c,s)[0]; ga = (s8)R(c,s)[1]; }
    else        { gh = (u8)FAR_4b3e[c*32 + s]; ga = (u8)FAR_4b3f[c*32 + s]; }
    if (minute > 90 && gh < 10) return 0;
    for (u8 k = 0; k < FAR_4bf0; k++) {                    /* matches shown live */
        u8 t = (c < 10) ? FAR_4b5e[FAR_305c[k]*20 + FAR_0028[k]]
                        : FAR_76ca[FAR_305c[k]*32 + FAR_0028[k]];
        if (t == home) live = k;
    }
    i16 sc = side ? ga : gh;
    if (sc < 9 || (sc >= 10 && sc < 19)) {                 /* 9 and >= 19 block goals */
        i16 adv;
        if (side == 0) { if (random_range(0, 4) <= 1) goto done; adv = adv_home; }
        else           { if (random_range(0, 1) == 0) goto done; adv = adv_away; }
        if (adv > 0) {
            if (random_range(0, (adv >> 2) + 2) != 0) goal = 1;          /* SAR */
        } else {
            if (random_range(0, (-adv) >> 2) == 0 || random_range(0, 6) == 0) goal = 1;
        }
    }
done:
    if (goal) {
        *(i32*)FAR_5496 += 1;                              /* goals statistic */
        if (c < 10) R(c,s)[side]++; else FAR_4b3e[c*32 + side + s]++;
    }
    if (live != 0x1e) {
        u8 kind = (c == 10) ? 1 : (c > 10) ? 2 : 0;
        i16 r = FUN_2a58_0ca3(goal, c, s, home, away, side, (u8)minute, kind, side, 0);
        FUN_2a58_009d(DS_0632[live*2], DS_063c[live*2], live, c > 9);
        FUN_1310_322c(r, ui_arg);
        return goal;
    }
    if (FAR_2e99 && goal && ((CLUB[home].crest_flags | CLUB[away].crest_flags) & 0x80)) {
        if (side) ga++; else gh++;
        while (gh > 9) gh -= 10;
        /* text = home name + " - " + away name + "   " + gh + ":" + ga */
        ui_set_font(rastport, 1);
        ui_fill_rect(rastport, 0, 0xe7, 0x13f, 0xef, 0);
        vga_wait_vretrace();
        ui_draw_text_centered_shadow(rastport, text, 0, 0x140, 0xed, 0x1d, 0xff);
        delay_ticks(70);
        ui_fill_rect(rastport, 0, 0xe7, 0x13f, 0xef, 0);
        FUN_1310_1def((u8)minute);
    }
    return goal;
}
```

Screen side effect (the "goal elsewhere" flash): font 1. The text is built from club names (`FAR:3066 +
0x22*club`, 20 bytes) and the separators `DS:02be = " - "`, `DS:02c2 = "   "` and `DS:02c6 = ":"`, with
numbers formatted by `format_long_thousands`. It is drawn centred in x 0..0x140 at y 0xed, colours
0x1d/0xff, in the strip `(0,0xe7)-(0x13f,0xef)`, held for 70 ticks and then cleared. `FAR:2e99` gates
this flash; it is not an option but `live_match_shown`, set while a human match is shown live
(match.md §6). Bit 7 of the club flags is set by the opponent-info screen (3a41:0e72) and cleared
by `league_strength_drift` when `mode != 1`. The scores 10..18 mean extra time: only `gh` is reduced
modulo 10 for the text.

### 2.5 `squad_lineup_totals` — 1f9d:1122

```c
void squad_lineup_totals(i16 far *sa, i16 far *sb, i16 far *sf, i16 far *avg_f)
{
    i16 n_xi = 0;
    *sf = 0; *sb = 0; *sa = 0;
    u8 n = squad_count_players(FAR_304a, 0);
    for (i16 i = 0; i < n; i++) {
        P *p = &FAR_774a[((i16)(s8)FAR_304a * 25 + i)];   /* IMUL byte: signed */
        if (p->slot == 0 || p->slot >= 12) continue;
        n_xi++;
        *sa += p->attr_a; *sb += p->attr_b; *sf += p->fatigue;
    }
    if (n_xi == 0) n_xi = 1;
    *avg_f = *sf / n_xi;
}
```

No RNG. The only caller is `screen_training` 212a:274c (212a:2be3).

### 2.6 `match_attendance_estimate` — 1f9d:11e0

`i32 far match_attendance_estimate(u8 m, u8 h, u8 a, u8 kind, u8 unused, u8 fixed_cap, u8 pop_override)`

Call sites (both in 2a58:20b2, home-match income): `(mgr, h, a, kind_local, x, 0, 0)` at 2a58:2201 and
`(mgr, h, a, 1, 2, 1, (u8)(random_range(0,…)+DS:5390[2*i]))` at 2a58:24c2. Parameter 5 (`[bp+0xe]`)
is never read.

Stadium block `ST = FAR:23a0 + 0x30a*m` (manager `+0x15e`), all `i32`: `+0x00` and `+0x08` = the two
capacity parts (capacity = sum), `+0x10`, `+0x18`, `+0x20`, `+0x28`, `+0x30` (meanings: see open
questions; `+0x28`/`+0x30` are multiplied by a random count, so they are probably the scoreboard and
floodlight levels).

```c
i32 match_attendance_estimate(u8 m, u8 h, u8 a, u8 kind, u8 unused, u8 fixed_cap, u8 pop_override)
{
    i32 rank_h = (h < 0x40) ? (u8)FAR_0efa[h*0x36] : 5;   /* league position, 0-based */
    i32 rank_a = (a < 0x40) ? (u8)FAR_0efa[a*0x36] : 5;
    i32 str_h = (i16)((i16)(tsv(h,2,0) + tsv(h,1,0) + tsv(h,0,0)) * 2) / 15;
    i32 str_a = (i16)((i16)(tsv(a,2,0) + tsv(a,1,0) + tsv(a,0,0)) * 2) / 15;
    /* call order: tsv(h,1,0), tsv(h,0,0), tsv(h,2,0), tsv(a,1,0), tsv(a,0,0), tsv(a,2,0) */
    i32 pop = (u8)M[m].popularity;             /* +0x10a */
    if (pop_override) pop = pop_override;
    i32 lg  = (u8)M[m].league_tier;            /* +0x138 */
    i32 div = (i32)(i16)(1 << (2 - (u8)lg));   /* SHL by CL = 2-lg: 4, 2, 1 */
    if (div == 4 && rank_h > 14) rank_h = 14;
    if (str_h == 0 && str_a == 0) return 1;

    double x = (1.0 - (double)(rank_a - 1) * 2.0 * 0.01) * (double)str_a   /* FIMUL m32int */
             + (1.0 - (double)(rank_h - 1) * 2.0 * 0.01) * (double)str_h;  /* str_* are i32 (sign-extended i16) */
    x = x * 0.23255813953488372 * 1150.0;      /* DS:9b62, DS:9b6a */
    i16 r = random_range((i16)((pop - 10) * 1050), (i16)(pop * 1550 - 14400));  /* 16-bit wrap */
    i32 att = _ftol((x - (double)r) + 1000.0); /* truncate toward 0 */

    i32 q = ((str_h + str_a) * 100) / 180;     /* _aFlmul then _aFldiv */
    att += (i32)random_range((i16)(5*q), (i16)(9*q)) * ST[+0x30] - q * 20;
    att += (i32)random_range((i16)(4*q), (i16)(12*q)) * ST[+0x28] - q * 31;
    att += q * (ST[+0x18] * 10 - 8);
    att += q * (ST[+0x20] * 10 - 5);
    att += q * (ST[+0x10] / 1000 - 8);
    if ((u16)M[m].f1dc > 50) att += (u32)(u16)(30 * M[m].f1dc - 1500);   /* 16-bit MUL, zero-ext */
    att += lg * -2000;
    if (DS_4a28 != 0) att += 1200;
    if (ST[+0x00] + 2000 > ST[+0x08]) att += (i32)random_range(400, 700);

    i32 week   = (s8)DS_225a[lg];              /* rounds played in this competition */
    i32 nweeks = (s8)DS_2262[lg];              /* rounds per season: 34, 38, 38 */
    i32 cap;
    if (fixed_cap) {
        u8 i;
        for (i = 0; i < 4; i++)
            if (DS_2272[2*i] <= h && h <= DS_2273[2*i]) break;   /* u8 compares */
        i16 T = (i16)DS_02c8[i];               /* low word of the i32 table entry */
        i16 T3 = (i16)(DS_02c8[i] / 3);
        cap = (i32)random_range(T - T3, T + T3) * 10;            /* IMUL: full 32-bit */
    } else {
        cap = ST[+0x00] + ST[+0x08];
    }
    if (kind > 3) kind = 3;
    if (lg == 2) kind = 0;
    if (str_a < 90 && kind > 1) kind--;
    if (pop < 22) att = (att * (kind + 2)) / 3;
    if (week + 5 > nweeks && pop < 22) {
        if (week == nweeks) att = cap;
        else {
            i32 f = (lg != 0);
            if (7 - 2*f > rank_h) { att += cap / 4; if (rank_a < 7) att += cap / 5; }
            if (rank_h + f + 5 > (s8)DS_225e[lg]) att += cap / 4;   /* clubs per league 18,20,20 */
        }
    }
    if (pop > 10) att += (10 - pop) * 900;
    i32 lim = (div * 2 - (div == 4)) * 100000;  /* 700000, 400000, 200000 */
    while (att * pop > lim) att -= 1000;       /* _aFlmul, signed compare */
    while (att <= 500) att += 500;
    if (att > cap) att = cap;
    return att;
}
```

(`tsv` = `team_strength_value`.) FP constants (doubles in DGROUP): `DS:9b4a` = 1.0, `9b52` = 2.0,
`9b5a` = 0.01, `9b62` = 0.23255813953488372 (= 10/43), `9b6a` = 1150.0 and `9b72` = 1000.0. The FP
sequence is `fild (rank_a-1); fmul 2.0; fmul 0.01; fsubr 1.0; fimul dword [bp-0x12]` (emulator `CD 36`
= `DA /1` = FIMUL m32int on the full i32 `str_a`; for the home team `fimul dword [bp-0x3a]` = `str_h`;
both are sign-extended i16 values, so the 32-bit operand equals the i16 value), the same for the home
team, `faddp; fmul 0.2325…; fmul 1150.0; fstp tmp`, then `call random_range; fild r; fsubr tmp;
fadd 1000.0; _ftol`. A port needs IEEE double arithmetic in this exact order. The x87 works in 80-bit
precision until the `fstp` to a 64-bit double.

`DS:02c8` (i32[4], match-income capacity base for the opponent's tier): `4500, 2200, 1200, 600`. If no
tier matches (`i == 4`), the code reads `DS:02d8`, which holds the bytes `00 61 62 63` (an out-of-range
read in the original). `DS:2272..2279` (tier ranges, u8 pairs): `0..17, 18..37, 38..57, 58..255`.

RNG use: the function always makes 3 `random_range` calls and up to 2 more. In call order:
1. `random_range((pop-10)*1050, pop*1550-14400)` (`r`), always;
2. `random_range(5q, 9q)`, always;
3. `random_range(4q, 12q)`, always;
4. `random_range(400, 700)`, only when `ST[+0x00] + 2000 > ST[+0x08]`;
5. `random_range(T - T3, T + T3)` (capacity draw), only when `fixed_cap != 0`.

The early `return 1` (both strengths 0) happens before call 1, so in that case no RNG call is made.
The six `team_strength_value` calls themselves do not use the RNG.

### 2.7 `team_strength_value` — 1310:1468 (dependency, owned by the match area)

```c
i16 team_strength_value(i16 t, i16 kind, i16 minute)
{
    i16 total = 0;
    for (i16 j = 0; j < 3; j++) {
        i16 b = CLUB[t].str_b[j];
        i16 v = b + (i16)(minute * 10) / (i16)(0xffff - b);   /* divisor = -(b+1) */
        if (v < 0) v = 0;
        v += CLUB[t].str_a[j] + CLUB[t].str_c[j];
        total += (i16)(((i32)v * (u8)DS_008e[kind*3 + j]) / 100);   /* IMUL then _aFldiv */
    }
    return total + (i16)CLUB[t].str_bonus - 0x2c;               /* +0x17 */
}
```

`DS:008e` line weights (u8[3][3]): kind 0 (defence) `80 20 0`; kind 1 (overall) `25 50 25`; kind 2
(attack) `0 20 80`.

### 2.8 `goal_minutes_generate` — 1310:12fe (dependency, RNG consumer)

Slots: `c < 10` → `FAR:515c + ((c*20 + s)*8 + k)`; `c >= 10` → `FAR:433c + ((c*32 + s)*8 + k)`.

```c
for (k = 0; k < 8; k++) slot(k) = 0;
if (n > 8) n = 8;
for (k = 0; k < n; k++) {
    tries = 0;
    for (;;) {
        m = random_range(lo, hi);
        for (j = 0; j < k; j++) {
            if (FAR_515c[(c*20+s)*8 + j] == m) j = 10;           /* always reads the 515c table */
            if (c >= 10) FAR_433c[(c*32+s)*8 + k] = 0;
            else if (FAR_515c[(c*20+s)*8 + k] == m) j = 10;
        }
        tries++;
        if (j == 11 && tries < 100) continue;                    /* collision: draw again */
        if (tries < 100) slot(k) = (u8)m;
        break;
    }
}
```

## 3. Data tables (constants in DGROUP)

| DS | Type | Values | Used by |
|---|---|---|---|
| 008e | u8[3][3] | `80,20,0 / 25,50,25 / 0,20,80` | team_strength_value |
| 02a2 | s8[8] | `0,0,0,1,1,1,2,2` (position → line) | squad_recalc_team_strength |
| 02aa | u8[20] | `1,2,2,2,2,2,3,3,3,3,3,3,3,3,4,4,4,4,4,4` | match_simulate_quick_result |
| 02be/02c2/02c6 | char[] | `" - "`, `"   "`, `":"` | match_goal_chance |
| 02c8 | i32[4] | `4500, 2200, 1200, 600` | match_attendance_estimate |
| 05ce | u8[3][2] | `70,93 / 40,74 / 15,45` | league_strength_drift |
| 2262 | s8[3] | `34, 38, 38` (initial) | match_attendance_estimate |
| 225e | s8[3] | `18, 20, 20` (initial) | match_attendance_estimate |
| 226f/2270 | s8 | `18`, `38` (initial) | league_of_club |
| 2272 | u8[4][2] | `0,17 / 18,37 / 38,57 / 58,255` | match_attendance_estimate |
| 9b4a..9b72 | double | `1.0, 2.0, 0.01, 10/43, 1150.0, 1000.0` | match_attendance_estimate |

## 4. AI decisions outside this segment

* **Strength recalculation**: §2.1 call-site table (lineup, training camp, transfers, fixtures screen,
  season loop) and §2.2 (season loop and `main`).
* **Computer-club results**: §2.3 and §2.4 via `match_live_view` 1310:2303 and 1310:347f.
* **Transfers / lineups / sacking**: no code in segment 1f9d. Candidates for the owning areas:
  `transfer_offer_received` 3277:1b5f (computer clubs bidding for human players), `transfer_market_init`
  3277:1e38, `FUN_3277_000a` (RNG; called from `random_weekly_events` 1cb5:13bd) and
  `random_weekly_events` 1cb5:13bd. No string about sacking a manager ("entlassen", "gefeuert") exists in
  DGROUP. Computer clubs have no squads, so there is no AI lineup code.

## 5. Globals used

| Address | Size/type | Suggested name | Save block |
|---|---|---|---|
| DS:008e | u8[9] | `line_weight_table` | const |
| DS:02a2 | s8[8] | `position_line_table` | const |
| DS:02aa | u8[20] | `quick_goal_max_table` | const |
| DS:02c8 | i32[4] | `attendance_tier_base` | const |
| DS:05ce | u8[6] | `league_strength_bounds` | const |
| DS:0632 / DS:063c | u8 pairs | `live_match_param_a/b` (passed to 2a58:009d) | not saved (DS:063a-063b is block 47) |
| DS:07ab | u8 | `human_manager_count` | 7 |
| DS:225a | s8[3] | `competition_round` | 34 |
| DS:225e | s8[3] | `league_club_count` | – |
| DS:2262 | s8[3] | `league_round_count` | – |
| DS:226f/2270 | s8 | `league_tier_bound_1/2` | – |
| DS:2272 | u8[8] | `tier_club_ranges` | – |
| DS:4a28 | s8 | `difficulty` | 42 |
| DS:9b4a..9b72 | double[6] | FP constants | const |
| FAR:0028[k] | u8[] | `live_match_slot` | not saved (block 40 starts at FAR:002e) |
| FAR:0efa + 0x36*t | u8 | `club_league_position` | 16 (FAR:0ecc) |
| FAR:2242 + 0x30a*m | struct | manager record (`+0x1e` club, `+0x10a` popularity, `+0x131` slider, `+0x138` league tier, `+0x13d` aggression, `+0x15e` stadium i32[...], `+0x1dc` u16) | 13 |
| FAR:21d8 + 2*k | u8[2] | `manager_last_score` (k = 1+manager) | 29 (FAR:21e2) neighbour; not saved |
| FAR:2e99 | u8 | `live_match_shown` (set by 2a58:20b2, see match.md §6) | 39 (FAR:2ec2) neighbour; not saved |
| FAR:304a | u8 | `current_manager` | 3 |
| FAR:305c[k] | u8[] | `live_match_row` | not saved (see below) |
| FAR:3066 + 0x22*t | struct[200] | club table (`+0x17` bonus, `+0x18` A[3], `+0x1b` B[3], `+0x1e` C[3], `+0x21` flags) | 15 |
| FAR:433c / FAR:515c | u8[] | goal-minute tables (cups / leagues) | – |
| FAR:4b3e | u8[] | cup result table (`c*32 + s`) | – |
| FAR:4b5e | u8[] | league fixture team table | 19 |
| FAR:4bf0 | u8 | `live_match_count` | – |
| FAR:5496 | i32 | `stat_goals_from_chances` | – |
| FAR:5788 | i32 | `stat_quick_sim_goals` | – |
| FAR:57dd + 0x25*id | struct | player table (`+0x1f` natural position, `+0x20` natural role) | 17 |
| FAR:6db0 | i32 | `stat_goal_chances` | – |
| FAR:6dbc/6dbd | u8 | `quick_sim_result_home/away`; the Rust port's `match_simulate_quick_result` returns it (`Some` for store = 0, `None` when skipped or stored), and only `match_reschedule_chances` keeps it in `Runtime::quick_sim_result` (the previous value is read when the fixture's result cell is 0x1e) | – |
| FAR:6dc8 | s8[] | league result table (`(c*38+round)*20 + s`) | 6 (from FAR:6ddc) |
| FAR:76ca | u8[] | cup fixture team table | 28 |
| FAR:774a + 0x34*(sq*25+i) | struct | squad player record | 18 |
| FAR:90c6[m] | u8[4] | `lineup_change_count` | not saved |

FAR:305c lies inside the club table range (`FAR:3066` minus 10), so it is a separate 10-byte array
directly in front of block 15. It is not saved.

## 6. Open questions

1. The A/B/C to KONDITION/TECHNIK/FORM mapping comes only from the column order in 3a41:1d18.
   `team_strength_value` decays B with the match minute, which fits "Kondition" better than
   "Technik".
2. Semantics of player fields `+0x03`, `+0x06`, `+0x19` (role) and of the player-table bytes
   `+0x1f`/`+0x20` are inferred from use, not confirmed against the player screen.
3. Stadium block fields `ST+0x10..+0x30` and manager field `+0x1dc` need the stadium/finance spec
   (1000:0602, 212a) to be named.
4. `match_attendance_estimate` reads `DS:02d8` when the home club id matches no tier range (only
   possible for ids above 255, so unreachable in practice).
5. AI transfers and offers (3277:1b5f, 3277:1e38, 3277:000a) and weekly events (1cb5:13bd) are not
   specified here; they belong to the transfer/season areas. No manager-sacking logic was found.
6. The exact meaning of the result-table scores 9 and 10..18 in `match_goal_chance` (extra-time or
   penalty encoding) belongs to the match area.
