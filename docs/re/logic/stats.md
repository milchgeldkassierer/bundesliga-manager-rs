# Statistics, info screens, high scores (area "stats")

Spec for an implementer without Ghidra access. It covers the segments **3a41** (opponent info,
fixtures, league table, strength table, league-round processing), **362f** (settings,
statistics, eternal table, manager history, advertising contracts), **24a4** (player info,
top scorers, AI goal scorers, player pool, finance estimates, advertising offers, injuries) and
the **422b** save/load UI and high-score logic.

All addresses are Ghidra addresses in `/unpacked/BMMAIN_unpacked.EXE`. `DS:xxxx` is DGROUP
(segment `5cb3`), `FAR:xxxx` is the far data segment `5238`. The save block numbers refer to
[savegame.md](../savegame.md). String quotes are verbatim game data, with the game's umlaut
encoding (`[`=Ä, `\`=Ö, `]`=Ü, `{`=ä, `|`=ö, `}`=ü, `~`=ß). `LBL[n]` is the far pointer at
`DS:4a68+4*n`, `UI[n]` the far pointer at `DS:22e0+4*n` (both tables point into DGROUP).

Scratch material (regenerable, gitignored) lives in `out/stats/`: `dec/*.c` raw decompiles,
`dec/*.s` annotated disassembly (selector segments resolved), `traces.txt` (every call with its
argument list and resolved string-table entries), `strtab.txt` (all far string tables decoded),
`dgroup.bin` (DGROUP image read from Ghidra).

## 0. Conventions and shared facts

### 0.1 Calling convention, integer helpers

* MSC 6/7 large model, `cdecl` far calls. Byte parameters are pushed as words; only the low
  byte is used. Signed `char` parameters are sign-extended with `CBW` where the code does so
  (noted below).
* `long` math goes through `_aFlmul` (4a01:1ae6), `_aFldiv` (4a01:1a4c, signed, truncates toward
  zero), `_aFlrem` (signed remainder), `_aFlmul_assign`/`_aFldiv_assign` (`*p op= b`, result also
  returned), `_aFlshr` (arithmetic shift right). All are callee-pop. In the pseudo code below
  `/` and `%` on `int32` are C99 truncating division.
* `random_range(lo, hi)` (176b:0cc7) = `rand() % (hi - lo + 1) + lo` with the MSC LCG
  (FUNCTION_MAP §3.4). Its arguments are `int16`. The order of the calls below is the order in
  the binary; every call consumes one LCG step, also when the result is unused or the range is
  empty (`random_range(0, 0)` still calls `rand()`).
* `clamp_int(int16 *v, lo, hi)` = 4091:1009: `if (*v < lo) *v = lo; if (*v > hi) *v = hi;`.
* `long_abs_diff(a, b)` = 4091:10d5 with two `long` arguments, returns `(int16)|b - a|`.
* `format_long_thousands(int32)` (176b:0681) returns a pointer to a **single static buffer**
  (`FAR:a306`); every caller copies the result before the next call. The byte `DS:079c` is the
  minimum field width it pads to (callers set it to 1, 2, 3, 4, 5 or 10 before calling).
* `squad_count(m, flag)` = 4091:1109 (owned by the squad agent): number of used records in squad
  group `m` (`m` = manager 0..3, 4 = transfer pool). Called with `flag = 0` everywhere here.

### 0.2 Mouse position

`FAR:6dc0` is a far pointer to the pointer state. In every screen of this area **word `[0]` is
compared with row (Y) coordinates and word `[1]` with column (X) coordinates** (rows of the
league table, the fixtures grid and the strength table use `[0]`). MEMORY_MAP §3.1 states the
opposite; see Open questions.

### 0.3 Data structures used throughout

**Manager record** `M(m)` = `FAR:2242 + 0x30a*m` (save block 13). Offsets used here:

| Offset | FAR (m=0) | Type | Meaning |
|---|---|---|---|
| +0x000 | 2242 | char[29] | manager name |
| +0x01e | 2260 | u8 | team id (club index) |
| +0x039 | 227b | u8 | championships won (shown as first "X" of "(X/X/X)") |
| +0x03a | 227c | u8 | DFB cups won |
| +0x03b..+0x03d | 227d..227f | u8[3] | European cups won (3 competitions) |
| +0x03e + 4*k | 2280 + 4k | u8[4] | per-round log (k < `DS:07e2`, max 50 used): `[0]` overall rank (table position + first team id of the league, so 0..57), `[2]`, `[3]` cup results (bit 7 = won, else stage in bits 0..2) |
| +0x10a | 234c | u8 | (used by 24a4:2b15 as divisor) |
| +0x10b + r | 234d + r | u8 | league position (0-based) after round index `r` (written by `league_apply_round_and_sort`) |
| +0x132..+0x135 | 2374..2377 | u8[4] | cup stage bytes (1..6 count in the high score) |
| +0x138 | 237a | u8 | league tier of the team (0 = Bundesliga, 1 = 2. Liga, 2 = Oberliga) |
| +0x13a | 237c | u8 | league home games of the season, the entry count of +0x14a (written by 2a58:20b2, reset at the season end) |
| +0x14a | 238c | u8[20] | attendance of each league home game of the season in thousands (low byte; written by 2a58:20b2) |
| +0x15e | 23a0 | int32[] | stadium block (`S` below): `S+0x00, S+0x08, S+0x10, S+0x18, S+0x20, S+0x28, S+0x30` are `int32` stadium values (standing places, seats, covered places, floodlights, score board, comfort...; owned by the stadium agent) |
| +0x1dc | 241e | u16 | used by income and advertising offers (`km` below) |
| +0x1de | 2420 | u16 | weekly expense item |
| +0x1e0 | 2422 | u16 | written by `finance_weekly_income` (low word of the income) |
| +0x1f0 | 2432 | int32 | **account balance** (Kontostand) |

`DS:0644 + 4*m` (int32) and `DS:066c + 0x24*m + 4*i` (int32, `i` 0..8; save block 41) are
per-manager finance accumulators read by the finance estimates.

**Club record** `C(t)` = `FAR:3066 + 0x22*t`, `t` = 0..199 (save block 15; MANA.DAT fills it,
see [mana.md](../mana.md)):

| Offset | FAR (t=0) | Meaning |
|---|---|---|
| +0x00 | 3066 | name, 20 bytes |
| +0x17 | 307d | morale, initial `rand(45,55)`, clamped to 47..53 by `team_morale_adjust` |
| +0x18..+0x1a | 307e..3080 | strength, current, 3 parts (defence, midfield, attack); "KONDITION" column |
| +0x1b..+0x1d | 3081..3083 | strength, base, 3 parts; "TECHNIK" column |
| +0x1e..+0x20 | 3084..3086 | form, 3 parts (initial `rand(45,55)`); "FORM" column |
| +0x21 | 3087 | crest index (`& 0x7f`); bit 7 = "show in fixture list" flag toggled by the opponent info screen |

**Team season statistics** `T(t)` = `FAR:0ecc + 0x36*t`, `t` < 64 (save block 16, 3456 bytes =
64 x 54). Every counter is a byte pair `[home, away]`; the pair two bytes further is a
**snapshot** of the pair (see `team_stats_copy_column`):

| Offset | FAR (t=0) | Meaning |
|---|---|---|
| +0x00/+0x01 | 0ecc/0ecd | points (home / away), win adds `DS:2271` (= 2), draw adds 1 |
| +0x02/+0x03 | 0ece/0ecf | snapshot of points |
| +0x04 | 0ed0 | form string, 8 chars + NUL: last results, newest at index 7 |
| +0x0d | 0ed9 | snapshot of the form string |
| +0x16/+0x17 | 0ee2/0ee3 | goals scored (home / away) |
| +0x18/+0x19 | 0ee4/0ee5 | snapshot of goals scored |
| +0x1a/+0x1b | 0ee6/0ee7 | goals conceded (home / away) |
| +0x1c/+0x1d | 0ee8/0ee9 | snapshot of goals conceded |
| +0x1e/+0x1f | 0eea/0eeb | games played (home / away) |
| +0x26/+0x27 | 0ef2/0ef3 | wins (home / away) |
| +0x28 | 0ef4 | snapshot of home wins |
| +0x2a/+0x2b | 0ef6/0ef7 | losses (home / away) |
| +0x2c | 0ef8 | snapshot of home losses |
| +0x2e | 0efa | current table position, 0-based |
| +0x32 | 0efe | int32 eternal-table points |

Draws are never stored; screens compute `games - wins - losses`.

**Fixtures and results.** `FAR:4b5e + 2*(L*10 + i)` (save block 19) holds the pairings of the
current round of league `L` (`[home, away]` team ids, pair `i` < 10). `FAR:6ddc + ((L*0x26 + r) *
0x14) + 2*i` (save block 6, 3 leagues x 38 rounds x 10 pairs x 2 bytes) holds the **result** of
pair `i` in round `r` (0-based): `[home goals, away goals]`; home goals `0x1e` (30) means "not
played yet" to the code, although the saves hold 0:0 for rounds not played yet (open question
2). Note: code that indexes by the 1-based round counter `DS:225a[L]` uses the base
`FAR:6dc8 = 6ddc - 0x14`. A home team id `0x46` ('F') ends a pair list.

**Table order.** `FAR:535a + 0x14*L + p` (save block 32) = team id at table position `p`.

**Streak table** `FAR:9d36 + 0x15*t + 3*k + c` (part of save block 35), team `t` < 64, category
`k` 0..6, column `c` (0 = total, 1 = home, 2 = away). Current streak lengths:

| k | meaning (statistics label) |
|---|---|
| 0 | won in a row ("GEWONNEN") |
| 1 | lost in a row ("VERLOREN") |
| 2 | drawn in a row ("UNENTSCHIEDEN") |
| 3 | not won ("NICHT GEWONNEN") |
| 4 | not lost ("NICHT VERLOREN") |
| 5 | no goal conceded ("OHNE GEGENTOR") |
| 6 | no goal scored ("OHNE TORERFOLG") |

**Best streaks per manager** `FAR:a276 + 3*(7*m + k) + c` (84 bytes, part of save block 35):
maximum ever reached by the manager's current team.

**Team records** `FAR:a2ca + 8*t + s` (value) and `FAR:a4ca + 8*t + s` (opponent team id), team
`t` < 64, slot `s` 0..7 (both inside save block 35). The value byte holds two nibbles
`hi = v >> 4`, `lo = v & 15`:

| s | label (UI[101+s]) | stored as | kept when |
|---|---|---|---|
| 0 | "H\|CHSTER H-SIEG" | hi = own goals, lo = opp goals | own - opp is larger |
| 1 | "H\|CHSTE H-NIEDERL." | hi = own goals, lo = opp goals | own - opp is smaller |
| 2 | "ERZIELTE H-TORE" | hi = own goals, lo = 0 | own goals larger |
| 3 | "KASSIERTE H-TORE" | hi = 0, lo = opp goals | opp goals larger |
| 4 | "H\|CHSTER A-SIEG" | hi = home(opp) goals, lo = own goals | home - own smaller |
| 5 | "H\|CHSTE A-NIEDERL." | hi = home(opp) goals, lo = own goals | home - own larger |
| 6 | "ERZIELTE A-TORE" | hi = 0, lo = own goals | own goals larger |
| 7 | "KASSIERTE A-TORE" | hi = opp goals, lo = 0 | opp goals larger |

**Head-to-head log** `FAR:9336 + 2*((m*0x40 + opp)*5 + k) + side` (part of save block 35):
for each human manager `m`, opponent team `opp` < 64, the last 5 results `k` per side
(`side` 0 = at home, 1 = away). Byte `= (opponent goals << 4) | own goals`, `0xff` = empty.

**Player pool** `P(p)` = `FAR:57dd + 0x25*p`, `p` = 1..150 (save block 17; record 0 unused):

| Offset | FAR (p=0) | Meaning |
|---|---|---|
| +0x00 | 57dd | name, 26 bytes |
| +0x1a | 57f7 | age |
| +0x1b | 57f8 | average of the three skills |
| +0x1c | 57f9 | skill A |
| +0x1d | 57fa | skill B |
| +0x1e | 57fb | fitness / condition (45..55 at start) |
| +0x1f | 57fc | goal-scoring talent (0 for keepers) |
| +0x20 | 57fd | `rand(0,6)` at creation (character) |
| +0x21 | 57fe | owner manager (5 = none) |
| +0x22 | 57ff | league goals this season |
| +0x23 | 5800 | league games this season |
| +0x24 | 5801 | team id; `0x40..0xfe` foreign club, `0xff` none |

**Squad record** `Q(m,i)` = `FAR:774a + 0x34*(25*m + i)` (save block 18): `+0x03` goals,
`+0x09` flags (bit 1 injured), `+0x0a`, `+0x0d` injury weeks, `+0x0e`..`+0x13` attributes,
`+0x0f` player id (into `P`), `+0x17` injury type, `+0x28` int32 salary.

### 0.4 DGROUP constant tables (decoded from the image)

| DS | Size | Value | Use |
|---|---|---|---|
| 224e / 2252 | int32 x2 | 1 / 1 | money display scale: shown value = `v * DS:2252 / DS:224e` (equal = no conversion; 362f:2551, statistics) |
| 225a | u8[3] | runtime | current round per league (1-based) |
| 225e | u8[3] | 18, 20, 20 | teams per league |
| 2262 | u8[3] | 34, 38, 38 | rounds per league |
| 2266 | u8[3] | 3, 4, 4 | table colour band sizes (relegation zone) |
| 226e | u8[3] | 0, 18, 38 | first team id of each league |
| 2271 | u8 | 2 | points for a win |
| 2272 | u8[3][2] | (0,17) (18,37) (38,57) | team id range per league tier (top scorers) |
| 227a..227f | char[6] | `s S n N u U` | form characters: win home/away, loss home/away, draw home/away |
| 06fc | far ptr[18] | | 18-team round permutations (`"abcdefghijklmnopqr"` = round 0 ...) |
| 0744 | far ptr[20] | | 20-team round permutations (DS:06fc+0x48) |
| 05bc | u8[18] | 1,2,1,2,2,4,2,1,1,6,12,5,7,10,11,9,20,14 | injury duration base per injury type |
| 5331 | u8[11] | 0,2,3,4,5,6,7,8,9,10,46 | scorer selection weights (index = candidate count) |
| 54b2 | u8[3] | 2, 0, 1 | column map G/H/A -> `goals_per_game` side parameter |
| 54d8 | u8[3] | 1, 2, 0 | opponent info `side` -> streak column |
| 54dc | u8[2] | 10, 17 | colour of the "ANZEIGEN" icon (flag off / on) |
| 5454 | u8[15] | 122,122,122,180,180,180,238,238,238,122,122,122,122,238,238 | settings toggle X |
| 5464 | u16[15] | 35,54,73,35,54,73,35,54,73,100,119,138,157,100,119 | settings toggle Y |
| 5498 | u16[13] | 0x0c0,0x2c0,...,0xd00 (green..red) | statistics bar palette, set into entries 0x1f+ |
| 9432/9433 | u8 pairs | 0,2,8,15,20 | slot ranges of player names per position (keeper 0-1, defence 2-7, midfield 8-14, attack 15-19) |
| 9438 | u8[4] | 2,5,8,5 | players per position group in a new human squad |
| 943c | u8[4] | 19,44,42,45 (static, decremented) | free-player counters per group |
| 07a7 | u8[4] | 20,64,106,151 | position group boundaries in the player pool |

## 1. Function list

Every function of the four segments. "Lib" marks UI/library calls that are only referenced.
Names in **bold** were applied in Ghidra by this agent.

| Address | Name | Kind | Section |
|---|---|---|---|
| 3a41:000e | **screen_opponent_info** | screen | §2.10 |
| 3a41:0f0e | **format_team_record** | helper | §2.4 |
| 3a41:10e3 | **fixtures_build_round_pairings** | logic | §2.1 |
| 3a41:120a | **screen_fixtures** | screen | §2.8 |
| 3a41:1d18 | **format_position_strength** | helper | §2.5 |
| 3a41:1ef4 | **team_stats_copy_column** | logic | §2.2 |
| 3a41:1fb0 | **team_form_push** | logic | §2.2 |
| 3a41:1fec | **team_morale_adjust** | logic (RNG) | §2.2 |
| 3a41:2073 | **head_to_head_log_result** | logic | §2.2 |
| 3a41:214c | **screen_league_table** | screen | §2.7 |
| 3a41:2d33 | **league_apply_round_and_sort** | logic (RNG) | §2.3 |
| 3a41:37cf | **team_record_update** | logic | §2.2 |
| 3a41:3891 | **screen_strength_table** | screen | §2.9 |
| 24a4:000a | **string_xor_copy** | helper | §3.1 |
| 24a4:0053 | **tore_scene_adjust_positions** | TORE helper | §3.1 |
| 24a4:01f9 | **match_screen_load_pictures** | gfx | §3.1 |
| 24a4:0347 | **match_screen_restore_pictures** | gfx | §3.1 |
| 24a4:0419 | **tore_scene_open** | TORE (see tore.md) | §3.1 |
| 24a4:0574 | **tore_disk_error** | TORE | §3.1 |
| 24a4:05ec | **tore_scene_play** | TORE | §3.1 |
| 24a4:0906 | **screen_player_info** | screen | §3.9 |
| 24a4:14d4 | **match_assign_ai_scorers** | logic (RNG) | §3.2 |
| 24a4:1662 | **matchday_assign_ai_scorers** | logic | §3.2 |
| 24a4:174e | **player_skill_drift** | logic (RNG) | §3.3 |
| 24a4:1798 | **player_pool_season_shuffle** | logic (RNG) | §3.3 |
| 24a4:19fb | assign_pool_players_to_teams (named by another agent) | logic (RNG) | §3.3 |
| 24a4:1ad5 | **screen_top_scorers** | screen + logic | §3.8 |
| 24a4:23da | **club_strength_boost** | logic (RNG) | §3.4 |
| 24a4:24bf | **pick_team_for_player** | logic (RNG) | §3.3 |
| 24a4:2588 | **player_value_percent** | logic | §3.4 |
| 24a4:260a | **office_redraw_with_header** | UI | §3.1 |
| 24a4:2636 | **finance_weekly_income** | logic | §3.5 |
| 24a4:2822 | **finance_weekly_expenses** | logic | §3.5 |
| 24a4:2b15 | **finance_weekly_balance_per_round** | logic (no caller) | §3.5 |
| 24a4:2bba | **stadium_attraction** | logic | §3.5 |
| 24a4:2cb4 | **advertising_offers_generate** | logic (RNG) | §3.6 |
| 24a4:30cf | **player_injure** | logic (RNG) | §3.7 |
| 362f:0004 | **settings_draw_toggle** | UI | §4.1 |
| 362f:0086 | **settings_draw_speed_slider** | UI | §4.1 |
| 362f:01b8 | **screen_settings** | screen | §4.1 |
| 362f:0a9f | **format_long_or_blank** | helper | §4.2 |
| 362f:0af7 | **screen_statistics** | screen | §4.2 |
| 362f:18e7 | **screen_eternal_table** | screen | §4.3 |
| 362f:21e1 | **format_goals_per_game** | helper | §4.2 |
| 362f:23e4 | **contract_draw_tab** | UI | §4.5 |
| 362f:248a | **contract_blit_sponsor_large** | UI | §4.5 |
| 362f:24ed | **contract_blit_sponsor_small** | UI | §4.5 |
| 362f:2551 | **contract_info_line** | UI | §4.5 |
| 362f:281f | **advertising_offer_panel** | UI | §4.5 |
| 362f:29a8 | **contract_draw_board** | UI | §4.5 |
| 362f:2be8 | **screen_manager_contract** | screen | §4.5 |
| 362f:36ec | mana_load_club_table | format (see mana.md) | §4.6 |
| 362f:390e | **screen_manager_history** | screen | §4.4 |
| 422b:0006 | **disk_error_dialog** | UI | §5.1 |
| 422b:0047 | **savegame_write_encrypted** | format (savegame.md) | §5.1 |
| 422b:013f | **savegame_read_decrypted** | format (savegame.md) | §5.1 |
| 422b:01d0 | mana_team_player_name_ptr (named by another agent) | logic | §5.5 |
| 422b:022e | mana_pick_unused_player_name (named by another agent) | logic (RNG) | §5.5 |
| 422b:035c | newgame_init_player_pool (named by another agent) | new-game logic (RNG) | §5.5 |
| 422b:07fe | **screen_save_game** | screen | §5.2 |
| 422b:120c | **screen_load_game** | screen | §5.2 |
| 422b:21c4 | **highscore_load_save** | file | §5.3 |
| 422b:2366 | **highscore_screen** | screen + logic | §5.3 |
| 422b:2864 | **highscore_compute_score** | logic | §5.3 |
| 422b:2a2a | **highscore_build_entry** | logic | §5.3 |
| 422b:2bbe | **file_selector** | screen | §5.4 |

(66 functions. FUNCTION_MAP's guesses "list_empty_marker" for 24a4:01f9, "player_aging_or_development"
for 24a4:2cb4 and "expenses_panel" for 422b:2a2a were wrong; see the sections.)

Screen draw sequences (exact coordinates, colours, fonts and strings of every call) are in
Appendix A, generated from the disassembly. The sections below explain the logic, the inputs and
the click handling.

## 2. Segment 3a41: league tables, fixtures, opponent info

### 2.1 `fixtures_build_round_pairings` 3a41:10e3

`void fixtures_build_round_pairings(uint8 league, uint8 round)` (round is 1-based).
Callers: `screen_fixtures`, `screen_fixture_results` 1000:28c4, match-day code 1310:000a,
1310:05f1, 1310:07a2, 1310:15db, `office_menu_dispatcher`. Callees: none.

```c
uint8 offset = 0;                       // first team id of the league
bool  second_half = false;
if (league != 0) { offset = DS:225e[0]; if (league == 2) offset += DS:225e[1]; }
int8 half = (int8)DS:2262[league] >> 1;           // SAR on the signed byte
if (half < round) { round -= half; second_half = true; }   // u8 arithmetic
if (half < round) round = half;                   // clamp (never hit for valid input)
for (uint8 i = 0; i < ((int8)DS:225e[league] >> 1); i++) {
    char far *perm = (DS:225e[league] == 18) ? DS:06fc[round] : DS:0744[round];
    uint8 a = perm[2*i] - 'a', b = perm[2*i + 1] - 'a';
    if (second_half) swap(a, b);
    FAR:4b5e[2*(league*10 + i)]     = a + offset;   // home
    FAR:4b5e[2*(league*10 + i) + 1] = b + offset;   // away
}
```

The 18-team and 20-team permutation strings are listed in MEMORY_MAP §2.1 (`DS:06fc`, 38 far
pointers; the 20-team set starts at pointer 18 = `DS:0744`).

### 2.2 Small state helpers used by the round processing

**`team_stats_copy_column` 3a41:1ef4** `void (uint8 t, uint8 src, uint8 dst)`. Copies the
column `src` of `T(t)` into column `dst` (`src`/`dst` are 0 = current, 2 = snapshot):

```c
uint8 far *T = FAR:0ecc + 0x36*t;
T[0x00+dst] = T[0x00+src];  T[0x01+dst] = T[0x01+src];
T[0x16+dst] = T[0x16+src];  T[0x17+dst] = T[0x17+src];
T[0x1a+dst] = T[0x1a+src];  T[0x1b+dst] = T[0x1b+src];
T[0x26+dst] = T[0x26+src];  T[0x2a+dst] = T[0x2a+src];   // only the home byte of wins/losses
strcpy(FAR:0ed0 + 9*(6*t + (dst>>1)), FAR:0ed0 + 9*(6*t + (src>>1)));   // form string
```

Games (`+0x1e/+0x1f`), away wins `+0x27` and away losses `+0x2b` are **not** copied.

**`team_form_push` 3a41:1fb0** `void (char far *form, char c)`:
`for (i = 1; i < 8; i++) form[i-1] = form[i]; form[7] = c;` (u8 loop counter).

**`team_morale_adjust` 3a41:1fec** `void (int16 t, int16 goal_diff)` (RNG):

```c
int16 v = C(t)[0x17];                              // u8 zero-extended
int16 d = random_range(0, 2 * long_abs_diff(0, goal_diff));   // ALWAYS called, also for 0
if (goal_diff > 0) v += d; else v -= d;
clamp_int(&v, 47, 53);
C(t)[0x17] = (uint8)v;
```

**`head_to_head_log_result` 3a41:2073** `void (uint8 m, uint8 opp, uint8 side, uint8 own,
uint8 their)`:

```c
uint8 far *e = FAR:9336 + 2*((m*0x40 + opp)*5) + side;   // e[2*k] = entry k
uint8 k = 0;
while (k < 5 && e[2*k] != 0xff) k++;
if (k == 5) { for (j = 0; j < 4; j++) e[2*j] = e[2*j + 2]; k = 4; }   // drop oldest
e[2*k] = their * 16 + own;                                            // u8 truncation
```

**`team_record_update` 3a41:37cf** `void (int16 t, int16 s, uint16 a, uint16 b, int16 minimize,
int16 wa, int16 wb, uint8 opp)`. All products are signed 16-bit (`IMUL`):

```c
uint8 v = FAR:a2ca[8*t + s];
int16 hi = v >> 4, lo = v & 15;
int16 old_score = hi*wa - lo*wb;
int16 new_score = a*wa - b*wb;
bool take = minimize ? (new_score < old_score) : (old_score < new_score);
if (take) {
    if (wb < 0) wb = 1;
    FAR:a2ca[8*t + s] = (uint8)(((uint8)(a*wa) << 4) + (uint8)(wb*b));   // 8-bit IMULs
    FAR:a4ca[8*t + s] = opp;
}
```

The records start at 0 (save block 35 is zero for a new game), so `old_score` is 0 for an
empty slot.

### 2.3 `league_apply_round_and_sort` 3a41:2d33 (core table update)

`void league_apply_round_and_sort(uint8 league, int8 mode, uint8 sort_cols)`. Callers:
`matchday_play` 1310:15db, `match_live_view` 1310:2303, `screen_league_table`,
`office_menu_dispatcher`. Callees: `manager_index_of_team` 1310:32b1, the helpers of §2.2.

* `mode == 0`: apply the results of round `DS:225a[league]-1` (0-based index `r`) to the team
  statistics, streaks, records and head-to-head logs, then sort.
* `mode == 1`: copy the current columns of every team that has a result in round `r` into the
  snapshot columns, then sort (no results are added).
* `sort_cols`: 0 = home columns only, 1 = home + away (normal table), 2 = away only.

```c
int8 r = DS:225a[league] - 1;
if (r > (int8)DS:2262[league] - 1 || league > 2) return;     // signed compare, then u8
for (int16 i = 0; i < (int8)DS:225e[league]; i += 2) {      // i = byte offset of the pair
    uint8 far *res = FAR:6ddc + (league*0x26 + r)*0x14 + i;
    if (res[0] == 0x1e) continue;                           // not played
    uint8 h = FAR:4b5e[league*0x14 + i], a = FAR:4b5e[league*0x14 + i + 1];
    if (mode == 0) {
        int16 mh = manager_index_of_team(h, 0xff);          // 1+manager or 0
        int16 ma = manager_index_of_team(0xff, a) & 0x7f;     // arguments swapped in the binary:
                                    // a 2nd-arg match returns m+0x81, &0x7f -> m+1 (same result)
        if (mh) head_to_head_log_result(mh-1, a, 0, res[0], res[1]);
        if (ma) head_to_head_log_result(ma-1, h, 1, res[1], res[0]);
    }
    if (h == 0x46) { i = 0x14; continue; }                  // end marker: leave the loop
    if (mode == 1) { team_stats_copy_column(h, 0, 2); team_stats_copy_column(a, 0, 2); }
    if (mode != 0) continue;
    team_stats_copy_column(h, 2, 0); team_stats_copy_column(a, 2, 0);   // restore snapshot
    uint8 gh = res[0], ga = res[1];
    T(h)[0x16] += gh;  T(a)[0x17] += ga;   // goals scored
    T(h)[0x1a] += ga;  T(a)[0x1b] += gh;   // goals conceded
    T(h)[0x1e]++;      T(a)[0x1f]++;       // games
    team_record_update(h, 2, gh, ga, 0, 1, 0, a);    // most goals scored at home
    team_record_update(a, 7, gh, ga, 0, 1, 0, h);    // most goals conceded away
    team_record_update(h, 3, gh, ga, 0, 0, -1, a);   // most goals conceded at home
    team_record_update(a, 6, gh, ga, 0, 0, -1, h);   // most goals scored away
    S(h,k,c) = FAR:9d36[0x15*h + 3*k + c];          // streak cells (see 0.3)
    if (ga == 0) { S(h,5,1)++; S(h,5,0)++; } else S(h,5,1) = S(h,5,0) = 0;
    if (gh == 0) { S(h,6,1)++; S(h,6,0)++; } else S(h,6,1) = S(h,6,0) = 0;
    if (gh == 0) { S(a,5,2)++; S(a,5,0)++; } else S(a,5,2) = S(a,5,0) = 0;
    if (ga == 0) { S(a,6,2)++; S(a,6,0)++; } else S(a,6,2) = S(a,6,0) = 0;
    team_morale_adjust(h, gh - ga);                 // RNG call 1 (int16 difference)
    team_morale_adjust(a, ga - gh);                 // RNG call 2
    if (ga < gh) {                                   // home win
        T(h)[0x00] += DS:2271;
        S(h,0,1)++,S(h,0,0)++; S(h,1,1)=S(h,1,0)=0; S(h,2,1)=S(h,2,0)=0;
        S(h,3,1)=S(h,3,0)=0;   S(h,4,1)++,S(h,4,0)++;
        team_record_update(h, 0, gh, ga, 0, 1, 1, a);
        team_record_update(a, 5, gh, ga, 0, 1, 1, h);
        S(a,0,2)=S(a,0,0)=0; S(a,1,2)++,S(a,1,0)++; S(a,2,2)=S(a,2,0)=0;
        S(a,3,2)++,S(a,3,0)++; S(a,4,2)=S(a,4,0)=0;
        T(h)[0x26]++; T(a)[0x2b]++;
        team_form_push(T(h)+4, DS:227a /*'s'*/); team_form_push(T(a)+4, DS:227d /*'N'*/);
    } else if (ga == gh) {                           // draw
        T(h)[0x00]++; T(a)[0x01]++;
        S(h,0,1)=S(h,0,0)=0; S(h,1,1)=S(h,1,0)=0; S(h,2,1)++,S(h,2,0)++;
        S(h,3,1)++,S(h,3,0)++; S(h,4,1)++,S(h,4,0)++;
        S(a,0,2)=S(a,0,0)=0; S(a,1,2)=S(a,1,0)=0; S(a,2,2)++,S(a,2,0)++;
        S(a,3,2)++,S(a,3,0)++; S(a,4,2)++,S(a,4,0)++;
        team_form_push(T(h)+4, DS:227e /*'u'*/); team_form_push(T(a)+4, DS:227f /*'U'*/);
    } else {                                         // away win
        T(a)[0x01] += DS:2271; T(a)[0x27]++; T(h)[0x2a]++;
        S(a,0,2)++,S(a,0,0)++; S(a,1,2)=S(a,1,0)=0; S(a,2,2)=S(a,2,0)=0;
        S(a,3,2)=S(a,3,0)=0;   S(a,4,2)++,S(a,4,0)++;
        S(h,0,1)=S(h,0,0)=0; S(h,1,1)++,S(h,1,0)++; S(h,2,1)=S(h,2,0)=0;
        S(h,3,1)++,S(h,3,0)++; S(h,4,1)=S(h,4,0)=0;
        team_record_update(h, 1, gh, ga, 1, 1, 1, a);
        team_record_update(a, 4, gh, ga, 1, 1, 1, h);
        team_form_push(T(h)+4, DS:227c /*'n'*/); team_form_push(T(a)+4, DS:227b /*'S'*/);
    }
}
if (mode == 0)                                       // best streaks of the human teams
    for (m = 0; m < DS:07ab; m++) { t = M(m)[0x1e];
        for (k = 0; k < 7; k++) for (c = 0; c < 3; c++)
            if (FAR:a276[3*(7*m+k)+c] < S(t,k,c)) FAR:a276[3*(7*m+k)+c] = S(t,k,c); }
// exchange sort of the table order (i < j, swap moves the better team up)
uint8 far *ord = FAR:535a + 0x14*league;
for (i = 0; i < (int8)DS:225e[league] - 1; i++)
  for (j = i + 1; j < (int8)DS:225e[league]; j++) {
    x = ord[i]; y = ord[j];
    #define COL(o, t) ((sort_cols < 2 ? T(t)[o] : 0) + (sort_cols != 0 ? T(t)[o+1] : 0))
    int16 px = COL(0x00,x), py = COL(0x00,y);
    bool swap;
    if (px != py) swap = px < py;                    // more points first
    else {
        int16 gx = COL(0x1e,x), gy = COL(0x1e,y);    // games played
        if (DS:224d != 0) gx = gy = 0;
        if (gx != gy) swap = gx > gy;                // FEWER games ranks higher
        else {
            int16 dx = (sort_cols<2 ? T(x)[0x16]-T(x)[0x1a] : 0) + (sort_cols ? T(x)[0x17]-T(x)[0x1b] : 0);
            int16 dy = (same for y);
            if (dx != dy) swap = dx < dy;            // goal difference
            else swap = COL(0x16,x) < COL(0x16,y);   // goals scored
        }
    }
    if (swap) { ord[i] = y; ord[j] = x; }
  }
for (p = 0; p < (int8)DS:225e[league]; p++) T(ord[p])[0x2e] = p;
for (m = 0; m < DS:07ab; m++) M(m)[0x10b + r] = T(M(m)[0x1e])[0x2e];
```

`COL` sums are `uint8` values widened to 16 bits. The streak `S(...)++` operations are `u8`
increments (wrap at 256). The pseudo code keeps the binary's statement order, which matters
only for the two RNG calls (home team first). The end-marker check (`h == 0x46`) comes after
the head-to-head logging, as in the binary.

### 2.4 `format_team_record` 3a41:0f0e

`uint8 format_team_record(uint8 t, uint8 s, char far *out)`; returns the raw record byte.
Callers: `screen_opponent_info`, `screen_statistics`.

```c
uint8 v = FAR:a2ca[8*t + s];
if (v == 0) strcpy(out, LBL[69] "NOCH KEINE EINTRAGUNG.");
else {
    DS:079c = 1;
    if (s == 0 || s == 1 || s == 4 || s == 5) {       // results: "x:y"
        x = (s <= 3) ? v >> 4 : v & 15;  y = (s <= 3) ? v & 15 : v >> 4;
        strcpy(out, fmt(x)); strcat(out, ":"); strcat(out, fmt(y));
    } else {                                           // goal counts
        x = v >> 4; if (x == 0) x = v & 15;
        strcpy(out, fmt(x));
    }
    strcat(out, " ("); strcat(out, C(FAR:a4ca[8*t + s]).name); strcat(out, ")");
}
return v;
```

### 2.5 `format_position_strength` 3a41:1d18

`void (char far *out, uint8 t, uint8 league /*unused*/, int8 detailed)`. Callers: `screen_fixtures`
(detailed = 1), 1000:28c4.
Builds `"<pos+1><ordinal> (<strength>)"` and, if `detailed`, `"... (PLATZ, ST[RKE <s>
(<d>,<m>,<a>)"`. Exactly:

```c
DS:079c = 2;
int16 a = Σ_{j<3} C(t)[0x18+j], b = Σ C(t)[0x1b+j], c = Σ C(t)[0x1e+j];
strcpy(out, fmt(T(t)[0x2e] + 1));
append_ordinal(out, T(t)[0x2e] + 1);        // 1310:1fe7 (adds "." style suffix)
strcat(out, " ");
strcat(out, detailed ? LBL[74] "PLATZ, ST[RKE " : "(");
strcat(out, fmt((a + b + c) / 9));          // int16 division
if (detailed) {
    strcat(out, " ("); strcat(out, fmt(a / 3)); strcat(out, ",");
    strcat(out, fmt(b / 3)); strcat(out, ","); strcat(out, fmt(c / 3));
}
strcat(out, ")");
DS:079c = 1;
```

### 2.6 Shared screen elements

* `copy_protection_penalty(title)` (16c7:09f9, lib) is called at the start of every screen with
  the window title string; it draws nothing (with the protection passed it returns at once), the
  screens draw their titles themselves. Titles: `LBL[112] "Spielplan"`,
  `LBL[113] "St{rketabelle"`, `LBL[117] "Statistik"`, `LBL[118] "Ewige Tabelle/Bilanz"`,
  `LBL[119] "Damals... Ihre Erfolge"`, `LBL[116] "Einstellungen"`, `LBL[110] "Torsch}tzenliste"`.
* Every screen draws its panels with `ui_draw_button(x, y, w, h, buttons, 0, 8)` and an exit
  button at **(0x10c, 0xc1)** (button index 2 in `ui_hit_test_buttons`); clicking it, the right
  mouse button or the abort flag `DS:05d4` leaves the screen. `ui_wait_with_highlight`
  (4091:1722, lib) animates the button release.
* Icon buttons are registered with `ui_register_icon_slot(x, y, text, colour)` (4091:123d) and
  hit-tested with `ui_hit_icon_slot()` (4091:11cc, returns 1-based slot or 0); `DS:07b3` counts
  the slots.
* Mode selectors: league table `LBL[75..77] "HEIM" "GESAMT" "AUSW[RTS"` (pointer table
  `DS:4b94`); strength table `LBL[82..85] "GESAMT" "ABWEHR" "MITTELFELD" "STURM"` (`DS:4bb0`).
* League names come from `UI[42+L]`: `"Bundesliga"`, `"Zweite Liga"`, `"Am.-Oberliga"`
  (`DS:2388 + 4*L`).

### 2.7 `screen_league_table` 3a41:214c

`void screen_league_table(uint8 league, uint8 arg)`. Callers: `office_menu_dispatcher` (arg 1),
season loop. Font 1 title, font 2 body.

1. Title `LBL[79] "Tabelle " + UI[42+league]` centered 0..0x10c at y 0xdc (colour 0xb, shadow 0),
   frame (5,8,0x134,0xb4), exit icon `0x22 + (arg < 1)` at (0x10c,0xc1), lines at y = 0x19 and
   0xa9 (colour 1), header `LBL[78]` at (9, 0x17).
2. `cols = 1` (GESAMT). Draw loop (on entry and after each mode change): `screen_fade(0)`, register
   the three mode icons `ui_register_icon_slot(0x1e + 0x66*i, 0xae, LBL[75+i], i == cols ? 0x12 : 1)`,
   `DS:079c = 1`, fill (6,9,0x137,0x11,8) and print `"Tabelle <mode>   <round>.SPIELTAG"` centered
   5..0x139 at y 0x10 (font 1, colour 1, shadow 7). The shown round is `DS:225a[league] - 1` when
   `arg == 1` and the counter is > 1, else the counter. Temporarily clamp `DS:225a[league]` to
   `DS:2262[league]-1` if it is past the last round, call `league_apply_round_and_sort(league, 1,
   cols)` and restore it.
3. Fill (9,0x1a,0x137,0xa8,8). Colour bands in the position column x 9..0x13: league 0: row 0
   (y 0x1c..0x22) colour 0xf and rows 1..`DS:226d` (= 4; y 0x23..0x22+7*4) colour 0xc; every
   league: relegation band y `7*(teams - DS:2266[L] + 4)`..`7*teams + 0x1b` colour 0x11; leagues
   1, 2: promotion band y 0x1c..`7*DS:2265[L] + 0x1b` (`DS:2265 + L` = 3 / 4 rows) colour 0xc.
4. Rows `p` = 0..teams-1 at baseline y = 0x21 + 7*p: `"<p+1>."` (field width 2) at x 9 in colour
   0x1d; club name at x 0x15 and the numbers string at x 0x67 in colour 0xb for human teams, else 1.
   Numbers string: the form string `T(t)+4` + `"   "` + games (width 2) + `"     "` + wins + `" "` +
   draws + `" "` + losses + `"    "` + points + (`DS:2271 < 3`: `":"` + `2*games - points`, else
   `" "`, with `"     "` before the points when `DS:2271 > 2`) + `"    "` + goals for (width 3) +
   `":"` + goals against + `"      "` + (width 2) sign (`"#"` for 0, `"+"`, `"-"`) + `|difference|`.
   Every number uses the columns selected by `cols`.
5. Clicks (`ui_hit_test_buttons` on frame and exit): frame click with Y > 0xa9 selects the mode
   by X (< 0x87 HEIM = 0, < 0xd7 GESAMT = 1, else AUSWÄRTS = 2) and redraws if it changed. A frame
   click with `0x1a < y < 0xa9` (row = (y - 0x1a)/7 < teams) opens
   `screen_opponent_info(team, DS:5538[cols] /* 0,2,1 */, 0, arg == 0)` (no redraw afterwards: the
   info window restores the background). Exit: button 2 or `DS:05d4`.
6. On exit it clamps the round again and calls `league_apply_round_and_sort(league, 1, 1)`, so
   the stored order is always the full table.

### 2.8 `screen_fixtures` 3a41:120a

`void screen_fixtures(uint8 league, int8 mode, uint8 arg, int8 by_round)`. Callers:
`office_menu_dispatcher` (mode 0, arg 0, by_round 1), `match_live_view` (mode 2 = live minute
display, `arg` = minute). `by_round == 0` lists the rescheduled matches (`LBL[72]`).

* If `mode != 0`: call `squad_recalc_team_strength(m, 0/1)` (1f9d:0002, FUNCTION_MAP
  *manager_ai_weekly*) for every manager before and after.
* Frame (1,2,0x13a,0xb9), exit icon `0x22 + (mode == 2)` at (0x10c,0xc1); in mode 0 the arrow
  icons 0x28 at (1,0xc1) and 0x27 at (0x3b,0xc1) (button count 4, else 2); line (3,0xe)-(0x139,0xe)
  colour 0xb.
* `round = DS:225a[league]`, set to 1 if past the last round.
* Header (fill (6,3,0x13a,0xd,8), font 1, centered 7..0x139 at y 0xb, colour 1, shadow 7): if
  `by_round`: `LBL[70] "SPIELE  " + league name + "   " + round + ordinal + LBL[71] "SPIELTAG  "`,
  then (mode != 2) the date: `w = fixture_week_of_nth_match(1 << league, round)` (1000:2895,
  0x78 if 0), `calendar_compute_date(w)` (1310:1190, overwrites the date globals), day `FAR:2eba` +
  `"."` (`DS:299c`) + month+1 `FAR:2e8e` + `"."` (`DS:29a0`). Else `LBL[72] "NACHHOLSPIELE  "`.
  Mode 2 appends `"  " + arg + ordinal + LBL[73] "MINUTE"`.
* If `by_round`: `fixtures_build_round_pairings(league, round)`. Fill (3,0xf,0x13a,0xb9,8).
* One pair per row `i` (byte offset `2i`, stop at home id 0x46), baseline y = 0x1a + 0x11*i,
  colour 0xb if a human manager runs either club, else 1: font 1 home name at (3, y-2), `"-"` at
  (0x88, y-2), away name at (0x92, y-2); font 2 `format_position_strength(.., detailed = 1)` of
  both clubs at (5, y+5) and (0x94, y+5) in colour 4; font 1 again. Result `FAR:6dc8[20*(0x26*L +
  round) + 2i]`: if home goals == 0x1e, search the 20 rescheduled records `FAR:5714 + 5k` for
  `[2] == round && [3] == L && 2*[4] == 2i`; when found, `calendar_compute_date([1])`,
  `DS:079c = 2`, `day + "." + month+1 + "." + W[2] " (N)"` centered at x 0x128 (font 2, y-3,
  no shadow), `DS:079c = 1`; when not found nothing is drawn. Otherwise, if `round <
  DS:225a[L] + (mode == 2)`, `gh` at x 0x120, `":"` at 0x129, `ga` at 0x12e (baseline y), else
  `"-:-"` at 0x120.
* Clicks (mode 0): arrow buttons 3/4 step the round down/up with wrap (1..rounds) and
  `ui_wait_with_highlight` on the arrow, then redraw. A frame click with y > 0xe selects
  `side = x > 0x8c`, `pair = ((y - 0xe)/0x11)*2 + side`; if valid it saves `FAR:4b5e` (60 bytes),
  calls `screen_opponent_info(team, side, 1, mode == 2 && arg == 0x5a)` and restores the buffer.
  Exit: button 2 or `DS:05d4`; if `by_round` it rebuilds the pairings for the real current round.

### 2.9 `screen_strength_table` 3a41:3891

`void screen_strength_table(uint8 league)`. Caller: `office_menu_dispatcher`.

* Frame (0x3d,0x11,0xbb,0xb2), exit icon 0x22 at (0x10c,0xc1), lines y 0x23 and 0xb2 from x 0x3f
  to 0xf6, title `LBL[86] "ST[RKEN " + league name` centered 0x3d..0xf8 at y 0x19 (font 1, colour
  1, shadow 7), header `LBL[81] "PL.     VEREIN               KONDITION   TECHNIK    FORM"` at
  (0x41,0x21) (font 2).
* `part = 0` (GESAMT). Draw: `ui_register_icon_slot(0x85, 0xb5, LBL[82+part], 0x12)`, fill
  (0x3e,0x24,0xf6,0xb0,8). Range of parts: `part == 0` → 0..2, else `part-1` only; `n = 3` or 1.
* Score per team `k` (team id `DS:226e[league] + k`): sum over the parts of
  `C[0x1e+j] + C[0x18+j] + C[0x1b+j]` (int16). Exchange sort descending
  (`for i, for j > i: if score[ord[j]] > score[ord[i]] swap`).
* `DS:079c = 2`. Rows at baseline y = 0x2a + 7*row (colour 0xb for human teams, else 1):
  `"<row+1>. <name>"` at x 0x41, then at x 0xa2 the three columns each `clamp_int(Σ parts / n, 0,
  99)` for `+0x18` (KONDITION), `+0x1b` (TECHNIK), `+0x1e` (FORM), separated by 12 spaces.
* Clicks: a frame click with y > 0xb1 cycles `part` 0→1→2→3→0 and redraws; a frame click with
  0x25 < y < 0xb2 opens `screen_opponent_info(DS:226e[league] + ord[(y-0x25)/7], 2, 0, 0)`.
  Exit with button 2.

### 2.10 `screen_opponent_info` 3a41:000e

`void screen_opponent_info(uint8 t, uint8 side, int8 from_list)`; `side` 0 = home view,
1 = away view, 2 = both. Callers: `screen_league_table`, `screen_fixtures`,
`screen_strength_table`, `cup_draw` 27c2:1ccb.

* Frame at x = 0x59 (+2 if `from_list`), y = 0x37, w = 0xe0, h = 0xae. Title
  `LBL[43] "Info ]ber " + name` (font 1, colour 0xb) with a line below. Icons: `UI[408]
  "REICHT MIR"` (exit, slot 1).
* The frame is drawn with a saved background (restored by `ui_redraw_buttons` on exit); the
  allocation goes through `vga_load_picture`, which resets `DS:079c` to 1.
* Foreign team (`t >= 0x40`): three lines `LBL[44..46]` ("IHRE SP[HER K|NNEN LEIDER NUR" /
  "AUF DEUTSCHEM BODEN OPERIEREN..." / "WILL SAGEN: KEINE INFORMATIONEN VERF]GBAR.") centered at
  y 0x7b, 0x83, 0x8d, the exit icon and "ANZEIGEN" (slot 2).
* Otherwise icons `LBL[47] "RESTPROGRAMM"` (slot 2, opens `screen_fixture_results(t, ...)`
  1000:28c4) and `LBL[48] "ANZEIGEN"` (slot 3, colour `DS:54dc[C(t)[0x21] >> 7]`; a click
  toggles bit 7 of `C(t)[0x21]` and redraws the icon). Lines:
  1. `"<pos+1>. PLATZ IN DER <LEAGUE UPPER>, ST[RKE: <(Σ C[0x18..0x20])/9>"` (league via
     `league_of_rank` 27c2:1bb5 and `text_to_upper_de` 4091:105d).
  2. `"HEIMBILANZ:"` / `"<W> SIEGE, <D> UNENT., <L> NIED."` from the home columns.
  3. `"AUSW[RTSBILANZ:"` / same from the away columns.
  4. `"DIE LETZTEN SPIELE:"` + the 8 form characters drawn one by one.
  5. `"TORE PRO SPIEL " + LBL[36+col]` and `format_goals_per_game(t, col, 2)` where
     `col = DS:54d8[side]`.
  6. `"SERIEN " + LBL[36+side]`, then up to 4 lines for streak categories `k` whose cell
     `S(t,k,col) > 1`: `"<n> SPIELE IN FOLGE " + X + LBL[29+k]` at (x+3) where, with
     `DS:224e == DS:2252` (shipped data), `X` is the far pointer `FAR:2f2c[col]`. That table is
     **never written**: the null pointer reads the interrupt vector table at 0000:0000, which
     DOSBox shows as one glyph (0x7e). With different options the line is `LBL[29+k] +
     LBL[26+col]` (" ", "ZUHAUSE ", "AUSW[RTS "). `FAR:2e6a` counts the lines. If none:
     `LBL[60] "KEINE BEMERKENSWERTE SERIE VORHANDEN."` at (x+0xa) in colour 2.
  7. Records: for slots `s` 0..3 (side 0) or 4..7 (side ≠ 0): label `UI[...]` and
     `format_team_record(t, s)`. For side 2 the best of the home and away record is shown with
     `" (H)"` / `" (A)"`.
  8. If `t` is not the current manager's team: `LBL[63] "HISTORISCHE ERGEBNISSE GEGEN " +`
     the name of the **manager's own club**, and `"H: g:g,g:g... -- A: ..."` from the
     head-to-head log of manager `FAR:304a` (`LBL[64] "H: "`, `LBL[66] " -- A: "`, `LBL[65]
     " NOCH KEINE"` for an empty side). The "H:" group reads the side-1 bytes (`FAR:9337 + 2i`),
     the "A:" group the side-0 bytes; a `","` precedes every non-empty entry `k != 0`.
  9. `LBL[67] "PUNKTE IN DER EWIGEN TABELLE: " + T(t)[0x32] (int32)` or `LBL[68] "NOCH KEINEN
     EINZIGEN..."` when 0.
* Loop: slot 1 = exit; slot 2 = RESTPROGRAMM (redraws the whole info afterwards unless it
  returned 1); slot 3 toggles the flag.

## 3. Segment 24a4: players, scorers, finance estimates, offers

### 3.1 Helpers, graphics and TORE functions

* **`string_xor_copy` 24a4:000a** `char far *(char far *s, uint8 key)` (caller 1cb5:2bf9):
  copies `s[i] ^ key` into the static buffer `DS:a322` up to and including the source NUL
  (the terminator is copied XORed, then `DS:a322[len] = 0` is written explicitly); returns
  `DS:a322`. Used to de-obfuscate hidden texts.
* **`tore_scene_adjust_positions` 24a4:0053** `void (int8 mirror)` — helper of `tore_scene_play`
  that walks the sprite position records of the loaded goal scene (`FAR:57d8` far pointer,
  records of 0xa4 bytes, `DS:52ca` = last index, bounds `DS:52d6..52ee`) and mirrors/clamps the
  coordinates. Format and semantics belong to [tore.md](../tore.md).
* **`tore_scene_open` 24a4:0419**, **`tore_disk_error` 24a4:0574** (`LBL[307..310]` "Ihre
  Torszenen-Diskette ist offenbar defekt." ...), **`tore_scene_play` 24a4:05ec**: TORE goal
  scenes, specified in [tore.md](../tore.md).
* **`match_screen_load_pictures` 24a4:01f9** `void (void)` (match ticker): frees the bitmaps at
  `FAR:2e9e` and `FAR:57c4`, loads `PIC 0x1a` into `FAR:577c` (data ptr copy `FAR:56ea`),
  allocates a 320x77 bitmap (`FAR:04a0`, data `FAR:0020`) and copies rows 0x23..0x6f of picture
  0x1a into it (minterm 0xc0), loads `PIC 0x1b` (`FAR:9332`, data `FAR:56e6`) and `PIC 0x1d`
  (`FAR:4bca`, data `FAR:2ea4`).
* **`match_screen_restore_pictures` 24a4:0347** `void (void)` (caller 2a58:0ca3): frees
  `FAR:04a0`, `FAR:9332`, `FAR:577c`, `FAR:4bca` and loads `PIC 8` into `FAR:2e9e` (data
  `FAR:5798`) and `PIC 0x26` into `FAR:57c4` (data `FAR:2eb4`).
* **`office_redraw_with_header` 24a4:260a** `void (char far *title)` (caller
  `season_end_messages` 1cb5:0012): `office_show_cached_background()`,
  `copy_protection_penalty(title, 0, 0, 0)`, `ui_draw_manager_header()`.

### 3.2 AI goal scorers: `matchday_assign_ai_scorers` 24a4:1662 and `match_assign_ai_scorers` 24a4:14d4

`void matchday_assign_ai_scorers(uint8 league)` (caller `match_live_view`):

```c
uint8 r = DS:225a[league]; if ((int8)DS:2262[league] < r) r = 1;
for (uint8 i = 0; i < (int8)DS:225e[league]; i += 2) {      // u8 counter
    uint8 far *res = FAR:6dc8 + (league*0x26 + r)*0x14 + i;   // = round index r-1
    if (res[0] == 'F' /*0x46*/) { i = 0x14; continue; }
    if (res[0] == 0x1e) continue;
    match_assign_ai_scorers(FAR:4b5e[league*0x14 + i],     res[0]);
    match_assign_ai_scorers(FAR:4b5e[league*0x14 + i + 1], res[1]);
}
```

Note: the `'F'` test reads the *result* byte here, not the team id.

`void match_assign_ai_scorers(uint8 team, uint8 goals)` — distributes the goals of a team that
is **not** managed by a human over the pool players `P(p)` of that team:

```c
uint8 n = 0; uint8 cand[151];                  // cand[1..150] zeroed first
if (goals == 0 || manager_index_of_team(team, 0xff) != 0) return;
for (uint8 p = 1; p < 151; p++) {
    if (P(p)[0x24] != team) continue;
    if (random_range(0, 100) < 94) {           // RNG
        P(p)[0x23]++;                          // league games
        int16 x = random_range(10, 99);        // RNG
        int16 y = random_range(5, 25) + P(p)[0x1f];   // RNG, talent
        if (y > x) cand[n++] = p;
    }
}
if (n < 2) return;                             // unsigned: n <= 1 gives no scorer at all
for (uint8 g = 0; g < goals; g++) {
    uint8 k = (n < 10) ? DS:5331[n] - 1 : 10;
    if (k > 2) k--;                            // u8
    if (random_range(0, 4) != 0) {             // RNG; 0 = goal without a pool scorer
        do {
            if (random_range(0, k) != 0) {     // RNG
                uint8 idx = random_range(0, n - 1);   // RNG
                P(cand[idx])[0x22]++;          // league goals
                break;
            }
        } while (random_range(0, 1) + 2 < n);  // RNG
    }
}
```

### 3.3 Player pool reshuffle

**`player_skill_drift` 24a4:174e** `void (uint8 far *v)`:
`int16 x = *v; x += random_range(1, 7) - 4; clamp_int(&x, 30, 99); *v = (uint8)x;`

**`pick_team_for_player` 24a4:24bf** `uint8 (uint8 p, uint8 value, int8 foreign)` (`p` unused):

```c
uint8 lo = foreign ? 0x40 : 0, hi = foreign ? 0xc7 : 0x3f, result = 0xff;
int16 tries = 0;
do {
    int16 t = random_range(lo, hi);                       // RNG
    if (manager_index_of_team((uint8)t, 0xff) == 0) {
        int16 s = 0;
        for (j = 0; j < 3; j++) s += C(t)[0x18+j] + C(t)[0x1b+j] + 50;
        s /= 9;
        if (tries > 500 || (value < s + 20 && value + 2 > s)) result = (uint8)t;
    }
    tries++;
} while (result == 0xff);
return result;
```

**`player_pool_season_shuffle` 24a4:1798** `void (int16 far *out /*[3]*/)` (callers
`season events` 1cb5:2756 and `assign_pool_players_to_teams`):

```c
uint8 per_tier[3], total = 0, foreign = 0;
for (tier = 0; tier < 3; tier++) { per_tier[tier] = 0;
    for (m = 0; m < DS:07ab; m++) if (M(m)[0x138] == tier) per_tier[tier] += squad_count(m, 0);
    total += per_tier[tier]; }
for (p = 1; p < 151; p++) {
    player_skill_drift(&P(p)[0x1c]); player_skill_drift(&P(p)[0x1d]);       // 4 RNG
    if (P(p)[0x24] > 0x3f && P(p)[0x24] != 0xff) foreign++;
}
uint8 moves = random_range(5, 15);                                          // RNG
for (uint8 i = 0; i < moves; i++) {
    uint8 p, old;
    do {
        p = random_range(1, 150);                                           // RNG
        old = P(p)[0x24];
        if (foreign > 4 && old > 0x3f && old != 0xff) {        // send a foreigner home
            foreign--;
            P(p)[0x24] = pick_team_for_player(p, transfer_value(p, 0) / 10000, 0);
            p = 0;
        }
    } while (manager_index_of_team(old, 0xff) != 0 || p == 0 || foreign > 4);
    P(p)[0x24] = pick_team_for_player(p, transfer_value(p, 0) / 10000, 0);
}
for (p = 1; p < 151; p++) if (P(p)[0x24] > 0x3f && P(p)[0x24] != 0xff) total++;
uint8 rest = 150 - total; if (total == 0) total = 1;
for (tier = 0; tier < 3; tier++)
    out[tier] = (int16)(((int32)per_tier[tier] * 100 / total * rest) / 100);
```

`transfer_value(p, 0)` is `3277:25de` (transfer agent; returns `int32`); the quotient is
truncated to its low byte when passed. The value check in `pick_team_for_player` uses this
byte.

**`assign_pool_players_to_teams` 24a4:19fb** `void (void)` (caller `main`): calls
`player_pool_season_shuffle(quota)`, then `1cb5:2a3b(k, list_k)` for `k` = 0..2 (builds 20-byte
candidate team lists per tier, season agent), then for each pool player `p` = 1..150 whose team is
not human-managed and (`P(p)[0x24] == 0xff` or `FAR:a7a0` (int32) != 0): for `k` = 0..2 with
`quota[k] != 0`: `P(p)[0x24] = 1cb5:2987(k, list_k)`, `quota[k]--` (verified in the disassembly
24a4:1a5d..1a76; 1cb5:2987 never reads `list_k`). `transfer_value(p, 0)` in
`player_pool_season_shuffle` gets the pool index as a squad slot, so it reads record
`FAR:304a * 25 + p` of the flat table FAR:774a, past the 125 squad records for large `p`.
The slot is **signed** (3277:25eb `MOV AL,[BP+6]; CBW`): the record offset is
`(25 * (i8)FAR:304a + (i8)p) * 0x34 + 0x774a`, 16-bit throughout, so a pool index `p >= 128`
reads record `25 * FAR:304a + p - 256`, before the squad table (in the player table FAR:57dd for
`FAR:304a <= 3`). The age is then `P[rec[0x0f]].b[0x1a]` (unsigned id). Verified with the
`season_two` scenario: with an unsigned slot the move of pool player 138 read a record of 0xff
bytes (value byte 138, no club matches, 502 draws), while the original read a player-table record
(value byte 34..54, an early match), which shifted every later random draw of the new season.
Such a record overlaps the club bytes `P[q][0x24]` of the player table (for example the status
byte `+0x09` of slot -118 of manager 1 is player 86's club, and bit 7 of it draws the offer
percent). The reshuffle writes each move's club at once (24a4:18e5, 24a4:194f) and
`player_value_calc` reads the live far data, so a later value sees the clubs of earlier moves; the
port rebuilds its far image for every value instead of taking one snapshot before the moves.

### 3.4 Club strength and player value

**`club_strength_boost` 24a4:23da** `void (uint8 t, int16 v)` (caller `screen_transfer_market`):

```c
int16 k;                         // NOT initialised before the loop (stack garbage, see Open questions)
int16 s = 0;
for (int16 j = 0; j < 3; j++) s += C(t)[0x18 + k] + C(t)[0x1b + k];   // original indexes with k, not j
int16 d = (v - s / 6 + 10) / 3;
k = random_range(0, 2);                                           // RNG
clamp_int(&d, 1, 10);
C(t)[0x18 + k] += random_range(d - 1, d + 1);                     // RNG, u8 add
C(t)[0x1b + k] += random_range(d - 1, d + 1);                     // RNG
```

**`player_value_percent` 24a4:2588** `int32 (Q far *q)` (caller `random_weekly_events`):

```c
int16 diff = q[0x11] - q[0x10];
int16 pct = 100 - 15 * (P(q[0x0f])[0x1f] < 25 ? 1 : 0);
if (diff > 0) pct -= diff >> 1;            // SAR
return (int32)(P(q[0x0f])[0x1a] * pct) / 100;     // 16-bit IMUL, sign-extended
```

### 3.5 Finance estimates (current manager `m = FAR:304a`)

`S` = stadium block `M(m)+0x15e` (int32 fields). All sums are `int32`.

**`stadium_attraction` 24a4:2bba** `int32 (void)` (callers `advertising_offers_generate`,
`highscore_build_entry`):

```c
int32 v = (S[0x10] >> 1) + S[0x08] + S[0x00];       // >> is arithmetic
if (v > 80000) v = 80000;
v += S[0x20] * 1900 + S[0x18] * 1500 + (S[0x30] * 2 + S[0x28]) * 1000;
if (v > 100000) v = 100000;
return v / 1000;
```

**`finance_weekly_income` 24a4:2636** `int32 (void)` (callers `finance_weekly_processing`,
`screen_statistics`, 24a4:2b15):

```c
int32 acc = 0; int32 n = squad_count(m, 0);
uint8 t = M(m)[0x1e];
for (j = 0; j < 3; j++) acc += C(t)[0x18+j] + C(t)[0x1b+j];
acc = acc * 15 * n / 3;
acc += (uint16)M(m)[0x1dc] / 5 + 25;                  // unsigned 16-bit division
M(m)[0x1e0] = (uint16)acc;                            // low word stored
for (i = 0; i < 8; i++) acc += DS:066c[9*m + i];      // int32 entries
FAR:5358 = m;
for (j = 0; j < DS:07ab; j++) {                       // FAR:304a is used as the loop variable!
    FAR:304a = j; if (j != FAR:5358) acc += finance_sum_entries(1, 0x63);   // 212a:0f8d
}
FAR:304a = FAR:5358;
if (DS:0644[m] != 0) acc += DS:0644[m] / FAR:2eba / 75;
return acc < 0 ? 0 : acc;
```

**`finance_weekly_expenses` 24a4:2822** `int32 (void)`:

```c
int32 acc = 150000;
for (i = 0; i < squad_count(m, 0); i++) acc += Q(m, i)[0x28];          // salaries
for (i = 0; i < squad_count(4, 0); i++)                                 // transfer pool
    if (P(Q(4, i)[0x0f])[0x21] == m) acc += Q(4, i)[0x28];
int32 x = S[0x30] * 3000 + (S[0x18] + S[0x20]) * 1375 + (3 - M(m)[0x138]) * 43750;
acc += ((x + S[0x00]) * 2 + S[0x08]) * 2 + S[0x10];
acc += (uint16)M(m)[0x1de];
acc += (((int32)M(m)[0x13f] + 1) * 10000) >> 4;        // byte at FAR:2381, arithmetic shift
acc += DS:066c[9*m + 8];
acc += finance_sum_entries(1, 0x63);
if (DS:0644[m] < 0) acc += DS:0644[m] / FAR:2eba / -10;
if (M(m)[0x138] != 2 && M(m)[0x1f0] > 2000000) {
    int32 t = (M(m)[0x1f0] - 2000000) / 500000 * 30000 + 20000;
    if (M(m)[0x1f0] >= 4000000) t = t * 3 / 2;
    acc += t;
}
return acc < 0 ? 0 : acc;
```

**`finance_weekly_balance_per_round` 24a4:2b15** `int16 (void)` (no caller found):
`d = finance_weekly_expenses()*12 - finance_weekly_income()*12;`
`r = d / ((int8)DS:225e[M(m)[0x138]] - 1) / M(m)[0x10a]; return r < 0 ? 0 : (int16)r;`

### 3.6 `advertising_offers_generate` 24a4:2cb4

`void (void)` for the current manager `m`. Callers: `start_menu_new_or_load` 18bc:2184 and
`season_main_loop` (every 14 days, and at season end after `highscore_screen(2)`). Fills the
offer amounts `O(k,i)` = `FAR:002e + 4*((2*m + k)*10 + i)` (int32, save block 40) and the
contract lengths `Y(k,i)` = `FAR:2ec2 + (2*m + k)*10 + i` (u8 1..3, save block 39) for the two
offer kinds `k` (0, 1) and ten offers `i`:

```c
int32 km = (uint16)M(m)[0x1dc];
for (k = 0; k < 2; k++) for (i = 0; i < 10; i++) {
    int32 s = stadium_attraction();
    if (s < 50 && km > 29) s += 20;
    int16 r1 = random_range(15, 20);                                     // RNG 1
    int32 base = (s >> 1) + 50;
    int32 thr  = base + r1 - 50;
    int16 q = (int16)(base / 5) + 2*i;
    O(k,i) = random_range(q*11 + 58, q*12 + 62);                         // RNG 2
    if (k == 0) base *= 5;
    int16 p = ((i*7)/10 + 3) * 3;
    int16 v = 10*i + random_range(3, 20);                                // RNG 3
    if (!(v < thr) && random_range(0, p) != 0) { O(k,i) = 0; continue; } // RNG 4 (only if v >= thr)
    int16 r = random_range((int16)(km/7) + 100, (int16)(km/5) + 100);    // RNG 5
    O(k,i) = O(k,i) * base * r / 100;
    int16 d = random_range(0, 2);                                        // RNG 6
    Y(k,i) = d + 1;
    int16 tier2 = 2 - M(m)[0x138];
    if (tier2 == 1) O(k,i) = O(k,i) * 107 / 100;                         // 2. Liga team
    O(k,i) = O(k,i) * random_range((d + 9)*10, d*15 + 92) / 100;         // RNG 7
    O(k,i) = O(k,i) * random_range((tier2 + 5)*20, tier2*23 + 102) / 100 // RNG 8
             + (int8)DS:4a28 * 800 + 16500;
    if (k == 0) O(k,i) += (int8)DS:4a28 * 2000 - 11000;
}
```

All multiplications are `int32` (`_aFlmul`), the divisions truncate. `O(k,i) = 0` means "KEIN
INTERESSE...". `DS:4a28` is the difficulty value (`5 - level`).

### 3.7 `player_injure` 24a4:30cf

`void (Q far *q, char far *out)` (callers `random_weekly_events`, `match_ticker_events`):

```c
P_t far *pl = P(q[0x0f]);
if (pl[0x1e] > 20) pl[0x1e] -= random_range(12, 19);              // RNG (only if > 20)
int16 type = random_range(0, 17);                                  // RNG
if (random_range(0, 1) != 0 &&                                     // RNG
    DS:05bc[type] > random_range(2, 4))                            // RNG (only if previous != 0)
    type = random_range(0, 17);                                    // RNG (re-roll serious injuries)
uint8 base = DS:05bc[type];
q[0x0d] = random_range(0, (base >> 2) + 1) + base;                 // RNG, injury duration
q[0x09] |= 2;  q[0x17] = (uint8)type;  q[0x0a] = 0;
strcpy(out, " ("); strcat(out, UI[53 + type]); strcat(out, ")");
```

Injury names `UI[53..70]`: Oberschenkelzerrung, Prellung d. Kn|chels, Platzwunde am Kopf,
Stauchung d. Fu~gelenkes, Sprunggelenkprellung, B{nderdehnung, Bauchmuskelzerrung, Bluterguss
am Knie, Nasenbeinbruch, Au~enbandanri~, Schienbeinbruch, Muskelfaserri~, Meniskusverletzung,
Innenbandabri~, Wadenbeinbruch, Kapselri~, Leistenbruch, Kreuzbandri~.

### 3.8 `screen_top_scorers` 24a4:1ad5

`u8 screen_top_scorers(i16 no_display)`. Callers: office menu 2 case 6 (`no_display = 0`) and the
season end (`no_display = 1`, with `FAR:304a` = the manager asked). Ported as
`crates/bmp/src/game/scorers.rs`.

```c
for (m = 0; m < DS:07ab; m++) for (i = 0; i < (u8)squad_count(m, 0); i++)
    P(Q(m,i).player_id).goals /*+22*/ = Q(m,i).goals[0];
for (i = 1; i < 151; i++) ord[i] = i;
for (i = 1; i < 151; i++) { cur = ord[i];                         // exchange sort, not stable
    for (j = i+1; j < 151; j++) { o = ord[j];
        if (P(o).goals > P(cur).goals || (== && P(o).games /*+23*/ < P(cur).games))
            { ord[i] = o; ord[j] = cur; cur = o; } } }
if (!no_display) {
    ui_draw_button(rp, 0x23, 0x08, 0xfa, 0xb5, &rects[0], 0, 8);
    ui_draw_button(rp, 0x10c, 0xc1, 0, 0x22, &rects[1], 0, 8);
}
REDRAW:
ids[0..19] = 0; cnt[0..3] = 0;
if (!no_display) {
    ui_draw_line(rp, 0x24, 0x1a, 0x11c, 0x1a, 1); font 1;
    ui_draw_text_centered_shadow(rp, "Die Besten der " + UI[225+mode] /*"Liga"/"Spieler"*/, 0x23, 0x11d, 0x10, 1, 7);
    font 2;
    ui_draw_text_at(rp, "PL. SPIELER                 VEREIN", 0x26, 0x18, 1);
    ui_draw_text_at(rp, "TORE       TORE/SPIEL", 0xd2, 0x18, 1);
}
DS:079c = 2; y = 0x22; rank = 0; rcap = 0; rows = 0;
for (si = 1; si < 0x97; si++) {
    p = ord[si]; team = (i8)P(p).team;
    if (team < DS:2272[2*tier] || team > DS:2273[2*tier]) continue;   // tier = M(FAR:304a).league
    if (P(p).goals < 2) break;
    human = 0; col = 1;
    for (m = 0; m < DS:07ab; m++) if (M(m).team == team) {
        if (cnt[m] != 0 && mode == 0) continue;
        human = 1; cnt[m]++; break; }
    if (manager_index_of_team(team, 0xff) != 0) col = 0x0b;
    rank++; if (rcap < 0x13) rcap++;
    if (rcap == 1 && col == 0x0b && no_display && M(FAR:304a).team == team) { ret = 1; break; }
    if (!no_display && rows < 20 && (mode == 0 || human)) {
        ui_draw_text_at(rp, fmt(rank) + ". " + P(p).name, 0x26, y, col);
        ui_draw_text_at(rp, CLUB(team).name, 0x72, y, col);
        ui_draw_text_at(rp, fmt(P(p).goals), 0xd4, y, col);
        ids[rows] = p;
        DS:079c = 1; q = 10*goals; if (games) q /= games;              // no games: goals.0
        s = fmt(q/10) + "." + fmt(q%10); DS:079c = 2;
        ui_draw_text_at(rp, s, 0xfc, y, col);
        rows++; y += 7;
        if (mode == 0 && si == 12) {                 // si is the ORDER index, not the row
            y += 0x0e; rastport_set_color(rp, 1);
            for (si = y-0x12; si < y-7; si += 2) rastport_put_pixel(rp, 0x2d, si);   // reuses si (bug)
        }
    }
    last_y = (u8)y;                                   // uninitialised before the first listed row
}
if (no_display) return ret;
if (y == 0x22) ui_draw_text_centered_shadow(rp, "NOCH KEINE EINTR[GE VORHANDEN...", 0x23, 0x11d, 0x28, 1, 0);
ICONS:
DS:07b3 = 0;
ui_register_icon_slot(0x36, 0xaf, UI[16] "LIGA", DS:5340[mode]);       // 0x11 / 0x0a
ui_register_icon_slot(0xd4, 0xaf, UI[17] "SPIELER", DS:5341[-mode]);   // 0x0a / 0x11
screen_fade(1);
for (;;) {
    h = ui_hit_test_buttons(rects, 2, 0); input_poll_mouse(); my = mouse y;
    if ((FAR:2ea2 == 1 || FAR:2ea2 == 3) && h == 1 && my > 0x1d && last_y > my) {
        k = (my - 0x1e) / 7;
        if (k < 20) for g in 0..4 with (g < DS:07ab || g == 4), j < squad_count(g, 0):
            if (Q(g,j).player_id == ids[k]) { FAR:304a = g; screen_player_info(j); FAR:304a = FAR:5358; goto ICONS; }
        FAR:2ea2 = 0;                                  // AI clubs' players: no info
    }
    if (FAR:2ea2 == 1 && ui_hit_icon_slot() != 0) { mode ^= 1; ui_fill_rect(rp, 0x24, 9, 0x11b, 0xbb, 8); goto REDRAW; }
    if (FAR:2ea2 == 1 && h == 2) { ui_wait_with_highlight(0x10c, 0xc1); return ret; }
}
```
`DS:2272` = `{0,17} {18,37} {38,57}`. Either icon toggles the mode (also the active one); the sort is
not redone. Closing the player info does not redraw the list (the window restores its background).

### 3.9 `screen_player_info` 24a4:0906

`void screen_player_info(u8 slot)` for `R = Q(FAR:304a, slot)`, `PL = P(R.player_id)`. Callers:
`screen_top_scorers`, `screen_transfer_market` 3277:04a5, `contract_negotiation` 3277:2a8f.
`COLX = DS:532c = {0x89, 0xc4, 0xff}`, `BAR = DS:5324 = 0c 0d 0e 0f 13 12 11 10` (followed by
`89 00 c4 00 ff 00 02 03 ..`, read by the unclamped exhaustion marker).

```c
id = ui_draw_button(rp, 0x59, 0x37, 0xe0, 0xae, &rect, 1, 8);   // saves the background: DS:079c = 1
font 1; ui_draw_text_centered_shadow(rp, "Info ]ber " + PL.name, 0x59, 0x139, 0x3f, 0x0b, 0);
ui_draw_line(rp, 0x5a, 0x41, 0x138, 0x41, 0x0b);
DS:07b3 = 0; font 2; ui_register_icon_slot(0xac, 0xd6, "AHA !", 0x0a);
k = (u8)((i16)(PL.age - 18) / 5); if (k > 3) k = 3;            // unsigned: age <= 13 -> 3
ui_draw_text_at(rp, PL.name + " IST " + fmt(PL.age) + " JAHRE ALT, " + UI[202+k], 0x5c, 0x49, 1);
    // "(GR}NSCHNABEL)", "(NOCH GANZ FRISCH)", "(MITTELALTER)", "(FAST AM ENDE)"
s = "STATUS: "; st = R.status & 3; lp = R.lineup_position;
if (lp != 0 && lp < 12) s += "IST F]R'S N[CHSTE SPIEL EINGEPLANT (NR." + fmt(lp) + ")";
else if (lp > 11)       s += "H}TET DIE ERSATZBANK";
else if (st == 0)       s += "KANN SPIELEN, DARF ABER SCHEINBAR NICHT...";
else { DS:079c = 1;
       if (st == 1) s += "NOCH " + fmt(R.suspended) + " SPIELE ";
       s += UI[133-st];                       // "GESPERRT.", "VERLETZT ", st 3: "UEFA-Pokal" (quirk)
       if (st == 2) s += "(" + upper(UI[53 + R[0x17]]) + ")"; }
ui_draw_text_at(rp, s, 0x5c, 0x51, 1);
for k < 3: ui_draw_text_centered_shadow(rp, UI[190+k] /*LIGA, DFB-POKAL, EUROPACUP*/, COLX[k], COLX[k]+0x3a, 0x5c, 1, 0);
DS:079c = 2;
ui_draw_text_at(rp, "TORE:", 0x5c, 0x64, 1);
for k < 3: ui_draw_text_at(rp, blank(R.goals[k]) + "(" + blank(R.w[0x22+2k]) + ")", COLX[k]+0x0c, 0x64, 1);
ui_draw_text_at(rp, "SPIELE:", 0x5c, 0x6c, 1);
for k < 3: ui_draw_text_at(rp, blank(R.games[k]) + "(" + blank(R.w[0x1c+2k]) + ")", COLX[k]+0x0c, 0x6c, 1);
ui_draw_text_at(rp, "DATEN:", 0x5c, 0x77, 1);
DS:07b2 = 0;
for (i = 0; i < 9; i++) { ry = (i/3)*16 + 0x77; x0 = COLX[i%3];
    ui_draw_text_centered_shadow(rp, UI[193+i], x0, x0+0x3a, ry, 1, 0);
        // KONDITION TECHNIK FORM / ROTE KARTEN GELBE KARTEN GELB-ROTE K. / SEITE(N) GEHALT/M. VERTRAGSDAUER
    v = i<3 ? R.skills[i] : i<6 ? R.cards[i-3] : i==6 ? PL[0x20] : i==7 ? R.salary*mul/div (unsigned) : R.contract;
    s = blank(v);
    if (i == 6) s = UI[206 + (v>1) + (v>4)];  // LINKS / MITTE / RECHTS replaces the number
    else if (i == 7) s += " DM";
    else if (i == 8) { s += " JAHR"; if (v > 1) s += "E"; }
    ui_draw_text_centered_shadow(rp, s, x0, x0+0x3a, ry+7, 1, 0xff); }
ui_draw_text_at(rp, "TENDENZ", 0x7f, 0xb1, 1);
ui_fill_rect(rp, 0x5f,0xb4,0xc1,0xbe, 1); ui_fill_rect(rp, 0x60,0xb5,0xc0,0xbd, 0);
for i < 8: ui_fill_rect(rp, (i+8)*12, 0xb7, (i+9)*12, 0xbb, BAR[7-i]);
t = (u8)(((i32)(i16)(R.tendency - 30) * 100) / 38); if (t > 0x5e) t = 0x5e;
ui_draw_line(rp, 0x5f+t, 0xb5, 0x5f+t, 0xbd, BAR[7 - t/12]);
ui_draw_text_at(rp, "ERSCH|PFUNG", 0xea, 0xb1, 1);
ui_fill_rect(rp, 0xd0,0xb4,0x132,0xbe, 1); ui_fill_rect(rp, 0xd1,0xb5,0x131,0xbd, 0);
for i < 8: ui_fill_rect(rp, 12*i+0xd1, 0xb7, 12*i+0xdd, 0xbb, BAR[i]);
e = (u8)(((i32)(i16)(R.exhaustion - 50) * 0x5e) / 100);      // no clamp
ui_draw_line(rp, 0xd0+e, 0xb5, 0xd0+e, 0xbd, BAR[e/12]);
do { input_poll_mouse(); h = ui_hit_icon_slot(); } while (!(FAR:2ea2 == 1 && h == 1));
ui_redraw_buttons(id, 0); DS:07b2 = 1;
```
`blank` = `format_long_or_blank` 362f:0a9f. The bracketed values (`R.w[0x22..]`, `R.w[0x1c..]`) are
presumably career totals.

## 4. Segment 362f: settings, statistics, eternal table, history, contracts

### 4.1 `screen_settings` 362f:01b8 and its helpers

`void screen_settings(void)` (callers `office_menu_dispatcher`, `start_menu_new_or_load`).

* Loads `PIC 0x21` (toggle graphics); title `LBL[116] "Einstellungen"`.
* Fifteen on/off toggles `DS:05fe[i]` (save block 36), drawn by
  **`settings_draw_toggle` 362f:0004** `void (bitmap, int16 x, int16 y, int16 on)`: two
  24x17 blits from the toggle picture, source X `on*0x30 + 100` and `0xac - on*0x30`, to
  `(x, y)` and `(x + 0x1a, y)`. Toggle `i` sits at `x = DS:5454[i]`, `y = DS:5464[i]`.
  Row labels: `UI[417] "Halbzeitst{nde"`, `UI[418] "Ergebnisse"`, `UI[419] "Tabelle"` (rows
  y 0x2e/0x40/0x53, columns `UI[425..427] "1.Liga" "2.Liga" "3.Liga"`), `UI[420] "Torszenen"`,
  `UI[127] "DfB-Pokal"`, `UI[421] "Europapokale"`, `UI[422] "Nachholspiele"`, `UI[423]
  "Zeitung"`, `UI[424] "Blenden"`. If `FAR:513c < 2000` an extra line `UI[416] " NIE."` is
  added.
* Click on toggle `i` (X in `[x, x+0x31)` and Y in `[y, y+0x12)`): `DS:05fe[i] = (X < x + 0x1a)`
  (left half = on) and redraw.
* Speed slider `UI[428] "Spielgeschwindigkeit"`: **`settings_draw_speed_slider` 362f:0086**
  `void (int16 pos)` clears (0xca,0xa1)-(0x115,0xa8) in colour 5 and draws the knob at
  `0xca + pos`. A click with `0xbe < X < 0x122` and `0x97 < Y < 0xab` sets
  `DS:063a = clamp(X, 0xca, 0x110) - 0xca` (0..70).
* **Keyboard cheats** (only while the pixel under the pointer has colour 0x0b; keys read with
  DOS function 06):
  * `'0'..'5'`: `DS:4a28 = '5' - key` (difficulty).
  * `'s'` or `'a'`: for every used squad slot of the current manager (`i < 24`, player id
    `!= 0`): `Q[0x0e] = 0x44`, `Q[0x13] = 0x32`; with `'a'` also `Q[0x10] = Q[0x11] =
    Q[0x12] = 99` and the balance `M(m)[0x1f0] = 0x05f5e0ff` (99,999,999).
  * `'d'`: `DS:05ad = 0`, clears the 50 save ids `FAR:a6d8[0..49]` (int32) and `DS:05ae = 0`
    (allows loading a save again in the same session).
  * `'g'`: balance `+= 200000`.
  * `'p'`: `DS:05f5 = 1`.
* Exit with button 2 (frees the picture).
* Header line (font 2, colour 3 at (0x10, 0x17)): `"V2.0 - ENDE: "` + the scenario year
  (`FAR:513c < 2000`) or `UI[416] " NIE."`, then `" (LEVEL "` + `5 - DS:4a28` + `")"`. There
  is no title. The rows are centred with colour 0x1d, shadow 0; lines at y 0x11 (colour 1),
  0x62 and 0xb4 (colour 0x1d, the current colour). The slider background is a 0x64 x 0x13 cut
  of the toggle picture at (0xbe, 0x9b).
* Cheat keys are read with DOS function 06 on every loop pass and discarded unless the pixel
  under the pointer has colour 0x0b; an accepted cheat flashes a frame (0,0)-(0x13f,0xff) in
  colour 1 while the keyboard buffer is drained.

### 4.2 `screen_statistics` 362f:0af7 and helpers

`void screen_statistics(int16 far *unused, uint8 unused2)` (office menu 1 case 1; port:
`crates/bmp/src/game/stats_screens.rs`). Ghidra's function body for 362f:0af7 is corrupt (it
swallows the following functions); the real code is 362f:0af7..18e6. The switch for items
20..27 is a jump table at 362f:11b7 (targets 0f0b, 0f52, 0f90, 0f99, 1018, 11c7, 1094, 111c)
whose cases the annotated disassembly does not list.

`m = FAR:304a`, `t = M(m)[0x1e]` (own team).

* Panels (5,8,0xc3,0xb4), (0xcd,8,0x6c,0xb4), (5,0xc0,0xc3,0x27) and the exit icon
  (0x10c,0xc1, code 0x22). All four write their rect to the same buffer, so the hit test
  (`n = 1`) only knows the exit icon.
* `debt = -finance_sum_entries(0, 0x63)` (int32, minus the principal still owed, all lenders).
* Palette: for `i` 0..12, entry `0x1f - i` = `DS:5498[i]` (`0x0RGB`, each nibble * 4):
  green (0x1f) .. red (0x13).
* 31 items `UI[90+i]`, `x = 8`, `y = 0x10`; at `i == 19` `x = 0xd1, y = 0x10`, at `i == 28`
  `x = 8, y = 0xc5`.
  * Header (text starts with `'!'`): font 1, `y += 6`, text+1 at `(x, y-2)` colour 0xb, line
    `(x, y)-(x + width(text+1), y)` colour 0xb (width measured in font 1), font 2,
    `y += 0xb`; next item.
  * Otherwise the label at `(x, y)` colour 1; if `i > 0x1c` `x += 0x8c`.
  * `i` 1..7 (`DS:079c = 2`): for column `c` 0..2 (x `0x22*c + 0x54`, colour 2):
    `format_long_or_blank(S(t, i-1, c)) + "(" + format_long_or_blank(best(m, i-1, c)) + ")"`
    (`best` = `FAR:a276[3*(7m + i-1) + c]`).
  * `i` 8, 9: `"  " + format_goals_per_game(t, DS:54b2[c], i - 8)` at the same places.
  * `i` 11..18: `format_team_record(t, i - 11)` at x `(7 + (DS:2252 == 3)) * 12` = 0x54,
    colour 2.
  * `i` 20..27 (`DS:07b2 = 0`, separators on; `n = M(m)[0x13a]` home games):
    | i | text (at `(x, y+6)`, colour 2) |
    |---|---|
    | 20 BESUCHERZAHL | `n == 0 ? "(NOCH KEINE...)" : fmt(M[0x1e4])` (total attendance) |
    | 21 SCHNITT/SPIEL | `n == 0 ? "(NOCH KEINER...)" : fmt(M[0x1e4] / n)` |
    | 22 BEN\|TIGTER SCHNITT | `fmt(finance_weekly_balance_per_round())` (24a4:2b15, §3.5) |
    | 23 ZUSCHAUERREKORD | `v = M[0x1e8]`; `v == 0 ? "(NOCH KEINER...)" : "(" + C(M.w[0x1f4]).name + ")"` |
    | 24 MINUSKULISSE | `v = M[0x1ec]`; `v == 99999 ? "(NOCH KEINE...)" : "(" + C(M.w[0x1f8]).name + ")"` |
    | 26 KONTOSTAND | `[LBL[324] if DS:224d] + fmt(M[0x1f0] * DS:2252 / DS:224e) + " DM"` |
    | 27 SCHULDEN GESAMT | `fmt(-(debt * DS:2252 / DS:224e)) + " DM"` (a `strcpy` that drops the prefix) |
    For 23 and 24, when `v != 0 && v != 99999`, `fmt(v)` is drawn right after the label at
    `(x + width(label), y)`. Strings: `DS:4a46 "(NOCH KEINE...)"`, `DS:4a56 "(NOCH KEINER...)"`.
  * `i` 29, 30 (`DS:079c = 7`, `DS:07b2 = 0`): `fmt(finance_weekly_income() * DS:2252 /
    DS:224e) + " DM"` and `fmt(finance_weekly_expenses() * DS:2252 / DS:2252) + " DM"` (the
    expenses are divided by the wrong constant, a leftover divisor on the stack) at `(x, y)`
    colour 2; then `DS:079c = 1`, `x -= 0x8c`.
  * Step: `i > 0x1b ? y += 8 : y += (i > 0x12 ? 15 : 8)`, plus 0x12 after item 24 (room for
    the attendance graph).
* Font 1, `LBL[0] "G       H       A"` at (0x5c,0x14) colour 0xb, font 2.
* Graph frames: fill (0xd1,0x69)-(0x130,0x76) colour 0, frame (0xd0,0x68)-(0x131,0x77) colour
  6; fill (0xd1,0xab)-(0x130,0xb8) colour 0, frame (0xd0,0xaa)-(0x131,0xb9) colour 6.
* `debt += M(m)[0x1f0]` (now the net balance).
* Attendance graph of the `n` bytes `M(m)[0x14a..]`: `max = (int8)-127`, `min = 127`; for each
  byte `v` (unsigned) `if (v > max) max = v; if (min > v) min = v;` with `max`/`min` signed
  bytes (so a byte above 127 stored as maximum turns negative). `range = (int8)(max - min)`,
  0 becomes 1. Bar `i`: `h = (v - min) * 11 / range + 2` (16-bit), frame `(5i+0xd1, 0x76-h)
  -(5i+0xd6, 0x76)` colour 0x15, fill `(5i+0xd2, 0x77-h)-(5i+0xd6, 0x76)` colour 0x11.
* Balance gauge: `thr = -2,375,000` (`0xffdbc2a8`), colour index `k = 0`; for `x` 0xd1..0x130:
  a line at `x` colour `k + 0x13`, tall `(x,0xab)-(x,0xb8)` when `debt < thr && debt + 50000 >=
  thr`, else short `(x,0xae)-(x,0xb5)`; `x++`; `k++` when `(x - 0xd1) % 7 == 0 && k < 12`;
  `thr += 50000`. Then a tall line at 0xd1 colour 0x13 if `debt <= -2,375,000` and at 0x130
  colour 0x1f if `debt > 2,325,000` (`0x237a08`).
* `screen_fade(1)`, wait for button 1 (exit), `ui_wait_with_highlight(0x10c, 0xc1)`,
  `DS:079c = 1`. `DS:07b2` is left at 0.

**`format_long_or_blank` 362f:0a9f** `char far *(int32 v)`: copies `fmt(v)` to `DS:a390`, or,
for `v == 0`, `DS:5485 "^@"` (`DS:5482 "^\xfc"` when `DS:224d != 0`); returns `DS:a390`.

**`format_goals_per_game` 362f:21e1** `void (char far *out, uint8 t, uint8 side, int16 what)`;
`side` 0 = home, 1 = away, 2 = both; `what` 0 = scored, 1 = conceded, 2 = both
(`"x.y:x.y"`):

```c
out[0] = 0;
for (w = (what == 2 ? 0 : what); w < (what == 2 ? 2 : what + 1); w++) {
    int32 v = 0;
    uint16 g = side < 2 ? T(t)[0x1e+side] : T(t)[0x1e] + T(t)[0x1f];
    if (g != 0) {
        uint16 goals = side < 2 ? T(t)[(w ? 0x1a : 0x16) + side]
                                : T(t)[w ? 0x1a : 0x16] + T(t)[(w ? 0x1a : 0x16) + 1];
        v = (int32)goals * 10 / g;
    }
    DS:079c = 1; strcat(out, fmt(v / 10)); strcat(out, ".");
    strcat(out, fmt(v % 10));
    DS:079c = 2;
    if (what == 2 && w == 0) strcat(out, ":");
}
```

### 4.3 `screen_eternal_table` 362f:18e7

`void (void)` (office menu 1 case 2). Order: sort, `vga_load_picture(0x24)` (trophy strip),
`copy_protection_penalty(LBL[118])`.

* Sort: `ord[i] = i` for the 64 domestic teams; for `i` 0..62, `j` from `i` to 63: swap if
  `(uint32)T(ord[i])[0x32] < (uint32)T(ord[j])[0x32]` (high words first, unsigned).
* Panels (5,8,0x87,0xde) and (0x91,8,0xa8,0xb4), exit icon (0x10c,0xc1, 0x22); like the
  statistics only the exit icon's rect is tested.
* Font 1: `LBL[11] "Ewige Tabelle"` centred (5..0x8c, y 0x11, colour 0xb, no shadow), line
  (6,0x13)-(0x8b,0x13) colour 0xb; `LBL[12] "Ewige Bilanz"` centred (0x91..0x139, 0x11), line
  (0x92,0x13)-(0x138,0x13).
* Trophy strips: `blit_bitmap(pic, 0, 0, screen, 0x9b, 0x16, 0x15, 0x71)` and the same to x
  0x119.
* Font 2. For `k` 0..4: `LBL[1+k]` centred (0xb0..0x11e, y `0x17k + 0x1e`, colour 1, shadow
  0); `M(m)[0x39+k]` (titles won) as `fmt(n)` colour 0xb or `LBL[13] " KOMMT VIELLEICHT
  NOCH..."` colour 1, centred (0xb0..0x119, `0x17k + 0x26`, shadow 0); `LBL[6+k]` at (0x94,
  `7k + 0x9c`) colour 1.
* `LBL[14] "GESAMT HEIM AUSW[RTS"` at (0xb7,0x93) colour 1; `DS:079c = 4`.
* Career counters: word pairs `[home, away]` at `M(m)+0x1a4` (points), `+0x1a8` (goals for),
  `+0x1ac` (goals against), `+0x1b0` (points against), `+0x1b8` (wins), `+0x1bc` (losses),
  `+0x1c0` (draws). Row `r` 0..4 (`PUN.`, `TORE`, `SIEGE`, `NIED.`, `UNEN.`), column `c`
  (0 = home + away as a 16-bit sum, 1 = home, 2 = away): row 0 shows `points:points against`
  (only `points` when `DS:224d != 0`), row 1 `goals for:goals against`, rows 2..4 one number.
  Pairs are drawn at `(0x30c + 0xa9, 7r + 0x9c)`, single numbers at x `0x30c + 0xb3`, colour 1.
* Table (`DS:07b2 = 0`), `y = 0x1b`, `shown = 0` (signed byte): for `si` 0..63, `t = ord[si]`:
  human = some manager `k < DS:07ab` has `M(k)[0x1e] == t` (then `shown++` if `si < 23`,
  colour 0xb, else colour 1). Drawn when `shown + 23 > si` or human: `DS:079c = 2`,
  `fmt(si+1) + ". " + name` at (9, y), `DS:079c = 4`, `fmt((uint32)T(t)[0x32] % 100000)` at
  (0x6f, y), `y += 7`. If then `shown - si == -22` and `DS:07ab != shown` (human teams still
  below): colour 1, `y += 0xe`, dots `put_pixel(0xd, p)` for byte `p` from `y - 0x13` while
  `p < y - 7`, step 2. So every human team inside the top 23 extends the top block by a row.
* `DS:079c = 1`, `DS:07b2 = 1` (left at 1), free the picture.
* Circles `draw_circle(0xa5, 0x17k + 0x20, 0xb)` for `k` 0..4, colour 0xd when the team plays
  in that competition this season, else 0x12: `k == 0`: bit 0 of `FAR:4bec[m]` clear and
  `M(m)[0x138] == 0`; `k > 0`: `M(m)[0x131+k] == FAR:0007+k` (state `event_state[k-1]`).
* `screen_fade(1)`, wait for button 1, `ui_wait_with_highlight(0x10c, 0xc1)`.

### 4.4 `screen_manager_history` 362f:390e

`void (void)` (office menu 1 case 3). `M = M(FAR:304a)`, `L = M[0x3e + 4k]` the season log
(`L[0]` overall rank: table position + first team id of the league, 1-based; `L[1]` league;
`L[2]` DFB-Pokal byte; `L[3]` European cup byte; `0xff` = no season yet).

* `copy_protection_penalty(LBL[119])`. Panels: graph (5,0x9d,0x102,0x49) in colour **0**, list
  (5,8,0x134,0x8f) colour 8, exit icon (0x10c,0xc1, 0x22). The graph panel writes its rect to
  the buffer the list panel then overwrites, so hit-test button 1 is the **list**, button 2 the
  exit icon.
* Font 1: `LBL[19]` at (`10 - 3*(DS:2252 == 3)`, 0x11) colour 0xb, line (5,0x13)-(0x138,0x13)
  colour 0xb; `LBL[20] "Gesamtentwicklung"` centred (6..0x106, 0xa5, colour 0xb, no shadow),
  line (6,0xa7)-(0x106,0xa7) colour 0xb.
* Graph: `n = min(DS:07e2, 50)` (signed); for season `k < n`, `v = L[0]`, colour 0x1b, for `h`
  from 0x3a down to 1: colour 0x1a once `h < DS:2270 + 1` (39), 0x19 once `h < DS:226f + 1`
  (19); if `v <= h` a line `(5k+7, h-0x56)-(5k+0xb, h-0x56)` (byte y: 0xe4 for `h = 58` up to
  0xab) in that colour. So each bar reaches from the bottom up to the rank.
* Font 2, `y = 0x1d`, `later = (FAR:a7a0 - DS:07e2) > 1970` (int32).
* `DS:07e2 == 0`: `LBL[21] "NOCH HABEN SIE KEINE ..."` centred (0..0x140, 0x1d, colour 1,
  shadow 7); wait for button 2.
* Else for `k` = 0, 1, ...:
  * `fmt(0x1c*later + M[0x13b] + k + 1963)` at (0xc, y) colour 1; `DS:079c = 2`.
  * `L[0] == 0xff`: `LBL[22] "EXISTIERTEN SIE NOCH GAR NICHT ALS MANAGER..."` at (0x2a, y).
  * Else `fmt(L[0] - (int8)DS:226e[L[1]]) + "."` at (0x2a, y), colour 0xb for the champion
    (`L[0] == 1 && L[1] == 0`) else 1; `upper(UI[42 + L[1]])` centred (0x55..0x55, colour 1, no
    shadow).
  * DFB-Pokal: bit 7 = `LBL[23] "SIEGER"` colour 0xb, else `upper(UI[120 + (L[2] & 7)])`
    colour 1 (stage 2 = "1.Runde ", 3 "Achtelfinale ", ..., stage 0 would read `UI[120]`),
    centred (0x90..0x90).
  * European cup (a scratch buffer keeps the last string copied into it: the league name, or
    the raw DFB-Pokal stage): bit 7 = `LBL[24] "SIEGER "` colour 0xb; else if `L[3] & 7` the
    stage `UI[121 + (L[3] & 7)]` (one entry later than the DFB-Pokal stages). Then if
    `(L[3] & 0xf8) == 0` the text is `LBL[25] "nicht im wettbewerb"`, else it is followed by
    the competition `UI[127 + ((L[3] >> 3) & 3)]` ("Pokal der Landesmeister", "Pokal der
    Pokalsieger", "UEFA-Pokal"), cut to `"Pokal der Landesm."` / `"Pokal der Pokals."` when
    `DS:224e == DS:2252`. Upper case, centred (0xb4..0x139) in that colour.
  * `k++`; unless `k % 16 == 0` a line (6, y+2)-(0x138, y+2) colour 6; `y += 8`.
  * After every 16th season and after the last: `screen_fade(1)`, wait for a click on button
    1 or 2 (on the last page only button 2 counts). Button 2 leaves; button 1 shows the next
    page: `y = 0x1d`, `screen_fade(0)`, fill (6,0x18)-(0x138,0x96) colour 8.
* `ui_wait_with_highlight(0x10c, 0xc1)`, `DS:079c = 1`. Read-only screen; draw sequence in
  Appendix A.

### 4.5 `screen_manager_contract` 362f:2be8 (advertising contracts) and helpers

`void (void)` (office menu 1 case 6). Data of the current manager `m = FAR:304a`:

| Where | Meaning |
|---|---|
| `FAR:0000 + 2m` / `+1` | shirt contract: months left (0 = free) / logo `0..9` |
| `FAR:4b9a + 2(6m + i)` / `+1` | board `i` (0..5): months left / logo |
| `DS:066c + 0x24m + 4j` (int32) | `j = 0` shirt, `1..6` boards, `7` TV, `8` advertising expenses |
| `O(k,i)`, `Y(k,i)` | offers of `advertising_offers_generate` (§3.6), kind `k` 0 = shirt, 1 = boards; offer `i` shows logo `i` |

**Entry.** `pic31 = vga_load_picture(0x1f)` (buttons, bitmap `FAR:2e76`), `pic32 =
vga_load_picture(0x20)` (logos, `FAR:4b5a`), `screen_fade(0)`, pointer hidden while `PIC\46.CP`
(0x2e) is decoded to the screen, font 2, `contract_draw_board`, `palette_load_rgb4(DS:0798 = 1)`
(palette slot 1), `screen_fade(1)`. Locals: `offer_mode` (`BP-0x22`, long) = 0, `kind` (`BP-4`)
= 100 (nothing selected), `tab` (`BP-2`) = 0, `prev_tab` (`BP-0x26`) = -1, `offer` (`BP-6`,
uninitialised, set before it is read).

**Loop.** `input_poll_mouse`; event 3 (**right button down, anywhere**) frees both bitmaps and
returns (the office redraws itself). Only event 1 (left down) is handled; `x = mouse[1]`,
`y = mouse[0]` are read once. All comparisons are strict. The blocks run in this order:

1. *Accept / decline* (`offer_mode`, `0xd9<x<0x139`, `0x5a<y<0x6d`, `O(kind, offer) != 0`):
   blit pressed `pic31 (0x30, x<0x108 ? 0x3f : 0x2f)` 48x16 to `(x>0x108 ? 0x10a : 0xd9, 0x5b)`,
   wait for event 2, blit `pic31 (0, 0x2f)` to the same place. If `x < 0x109` (accept):
   * kind 0: if `FAR:0000[2m] != 0` -> `ui_dialog_box("Sie stehen noch", "unter Vertrag")`;
     else `[2m+1] = offer`, `[2m] = (u8)(Y(0,offer) * 12)`, `DS:066c[m][0] = O(0,offer)`,
     `offer_mode = 0` (the shirt offer stays).
   * kind 1: if `FAR:4b9a[2(6m+tab)] != 0` -> the same dialog; else logo = `offer`, months =
     `Y(1,offer) * 12`, `DS:066c[m][1+tab] = O(1,offer)`, **`O(1,offer) = 0`**, `offer_mode = 0`.
   * `contract_draw_board` (also after the dialog).

   Then, if `!offer_mode || x > 0x108` (signed, or decline): `offer_mode = 0`, grey arrows
   `pic31 (0x3c,0)`/`(0x3c,0x17)` 30x23 to `(0xb5,0x3c)`/`(0xb5,0x54)`, grey buttons
   `pic31 (0x60,0x2f)`/`(0x60,0x3f)` 48x16 to `(0xd9,0x5b)`/`(0x109,0x5b)`.
2. *Expenses* (`0xd8<x<0x135`, `0xb9<y<0xc5`): `e = DS:066c[m][8] + (x < 0x107 ? -2500 :
   +2500)`; `e > 50000` -> 2500, else `e < 2500` -> 50000; `contract_draw_board`.
3. *Select* (`3<x<100 && 0x50<y<0xd2 && prev_tab != 10` (player picture) or
   `3<x<0x13c && 0x12<y<0x33` (boards)): `tab = (x-4)/0x34`, `kind = y < 0x50`; kind 0 ->
   `tab = 10`. If `prev_tab == tab` the rest of the iteration is skipped. Else: clear
   (0xd9,0x3c)-(0x139,0x5a) colour 0, `offer_mode = 0`, grey buttons (as above; the arrows are
   **not** reset), fill (0x7f,0x43)-(0xa4,0x60) colour 9, `contract_draw_tab(tab, 0x1d)`, if
   `prev_tab != -1` `contract_draw_tab(prev_tab, 0x1c)`, `prev_tab = tab`,
   `contract_info_line(months, amount, 0xd9, 0x3e, 0, 0)` with the months and amount of the
   selected contract.
4. *Open the offers* (`kind != 100`, `0x7e<x<0xa0`, `0x43<y<0x6c`, `!offer_mode`):
   `offer_mode = 1`, `offer` = logo byte of the selected contract (also when it is free),
   arrows `pic31 (0,0)`/`(0,0x17)` to `(0xb5,0x3c)`/`(0xb5,0x54)`,
   `contract_blit_sponsor_large(offer, 0x7f, 0x43)`, `advertising_offer_panel(kind, offer)`.
5. *Arrows* (`offer_mode`, `0xb4<x<0xd3`, `0x3b<y<0x6c`): `d = y > 0x52`; pressed
   `pic31 (0x1e, 0x17d)` to `(0xb5, 0x3c + 0x18d)`, wait for event 2, normal `pic31 (0, 0x17d)`;
   upper (`y < 0x53`): `offer++` up to 9; lower (`y > 0x52`): `offer--` down to 0; big logo and
   `advertising_offer_panel` again.

**`contract_draw_board` 362f:29a8** `void (bitmap logos, int16 unused)`: if the shirt runs
(`FAR:0000[2m] != 0`) `contract_blit_sponsor_small(logo)`; boards `i` = 0..5 at
`x = 0x34 i + 8`: free -> `ui_fill_rect(x, 0x13, x+0x26, 0x31, 9)`, else
`contract_blit_sponsor_large(logo, x, 0x13)`. Then `contract_info_line(.., 0xf0, y, 1, 0)` for
shirt (`y` 0x80), the sum of the boards (0x8e), TV (0x9c), expenses (0xb9) and
`shirt + (TV + boards) - expenses` (0xd2), all int32 wrapping.

**`contract_blit_sponsor_large` 362f:248a** `void (bitmap, uint16 n, int16 x, int16 y)`: 38x30
from `((n>4 ? n-5 : n) * 0x27, (n>4 ? 0x21 : 0) + 1)`. **`contract_blit_sponsor_small` 362f:24ed**
`void (bitmap, uint16 n)`: 26x16 from `((n>4 ? n-5 : n) * 0x1c, n>4 ? 0x55 : 0x43)` to
(0x33,0x6e), the shirt of the player. **`contract_draw_tab` 362f:23e4** `void (int16 tab,
uint8 colour)`: tab 10 = `ui_draw_frame(5,0x3b,0x6e,0xe5)`; else, with `r = 0x34 tab + 0x36`,
`ui_draw_line(0x34 tab + 5, 0x32, r, 0x32)` and `ui_draw_line(r, 0x13, r, 0x32)`.

**`advertising_offer_panel` 362f:281f** `void (uint8 kind, uint8 i)`: clears
(0xd9,0x3c)-(0x139,0x5a) colour 0; `g = O(kind,i) == 0 ? 0x60 : 0`; blits `pic31 (g,0x2f)` to
(0xd9,0x5b) ("OK") and `(g,0x3f)` to (0x109,0x5b) ("NEIN"), 48x16; then either
`ui_draw_text_centered_shadow(LBL[18] "KEIN INTERESSE...", 0xd9, 0x139, 0x4e, 1, 0xff)` or
`contract_info_line(Y(kind,i), O(kind,i), 0xd9, 0x3e, 0, 1)`.

**`contract_info_line` 362f:2551** `void (uint8 n, int32 amount, int16 x, int16 y, uint8 left,
uint8 years)`:

* `left != 0`: `ui_fill_rect(x, y, 0x134, y+10, 0)`, no head line.
* `left == 0`: head line centred in 0xd9..0x139 at `y+7`, colour 1, no shadow:
  `n == 0` -> `LBL[17] "K}NDBAR"`; else `LBL[15] "VERTRAG: "` + `format_long_thousands(n)`
  (current `DS:079c`, normally 1) + unit + `UI[222] "E"` when `n > 1`. Unit: `years == 0` ->
  `LBL[16] " MONAT"`; else `UI[221] " JAHR"` **+ 1 - DS:224d** (`DS:2252 != 3`), i.e. `"JAHR"`
  without the space in the shipped data ("VERTRAG: 1JAHR", confirmed in DOSBox), or the full
  `" JAHR"` when `DS:2252 == 3`. Then `y += 12`.
* Amount: `DS:079c = 10`, `DS:07b2 = 0`; a negative amount is negated and `"-"` (`DS:54be`) drawn
  at `(x, y+7)` colour 1; text = (`DS:224d` ? `LBL[324]` "" : "") +
  `format_long_thousands(amount * DS:2252 / DS:224e)` + `LBL[319] " DM"` (so padded to 10 with
  `^`, with separators); afterwards `DS:07b2 = 1`, `DS:079c = 1`. Leading `'%'` characters are
  skipped (none occur). `left`: `ui_draw_text_at(x, y+7, 1)`; else centred in 0xd9..0x139 at
  `y+7`, colour 1, no shadow.

### 4.6 `mana_load_club_table` 362f:36ec

MANA.DAT loader; fully specified in [mana.md](../mana.md) ("How BMMAIN reads the file").

## 5. Segment 422b: save/load UI, file selector, high scores

### 5.1 Error dialog and encryption

* **`disk_error_dialog` 422b:0006** `void (void)`: `DS:9440 = 1` (all further encrypted reads
  and writes become no-ops), then the four-line message box 4091:0e3a with `LBL[139..142]`
  ("Achtung !", "Fehler beim", "Diskettenzugriff !", "Vorgang abgebrochen.").
* **`savegame_write_encrypted` 422b:0047**, **`savegame_read_decrypted` 422b:013f**: see
  [savegame.md](../savegame.md) (cipher, checksums, block order).

### 5.2 `screen_save_game` 422b:07fe and `screen_load_game` 422b:120c

Both open the file selector (`file_selector(".MAN")`) titled `LBL[106] "Spielstand
Speichern"` / `LBL[107] "Spielstand Laden"`; the file format and the block sequence are in
savegame.md.

* Save `void screen_save_game(char far *fixed_name)`: callers `fatal_error_exit` (name
  `"ERROR.MAN"`; not for exit code 99, the quit question), `office_menu_dispatcher` 18bc:0b84
  (autosave `"autosave.man"` at DS:026b when the request DS:5256 `autosave_pending` is set by month end 212a:0a6d; cleared after the save) and 18bc:1bb2 (menu
  "save", empty name DS:0278, only while `DS:05ad == 0`). Port: `crates/bmp/src/game/disk.rs`.

  ```c
  DS:9440 = 0;
  if (*fixed_name == 0) {
      copy_protection_penalty(LBL[106]);                       // one random_range when not passed
      font 3; title = upper(LBL[106]) centred x 0..0x140, y 0x14, colour 0x0b, shadow 0;
      file_selector(0, 0, name, 0);  if (!name[0]) return;
      path = FAR:4af6 (cwd) + (cwd[3] ? "\" : "") + name;
      if (file_open(path, 0)) {                                // exists
          file_close();
          if (!dialog_two_choice(FAR:304a, LBL[208], LBL[209], LBL[210], LBL[218], LBL[219]))
              return;                                          // "BLO~ NICHT !" (-1 continues)
      }
      strcpy(FAR:54a2, name);                                  // offered again by the selector
  } else path = fixed_name;
  if (!(fd = file_open(path, 1))) { disk_error_dialog(); DS:9440 = 0; return; }
  write(signature DS:93e0, strlen); write(">V2\0", 4);
  lo = random_range(0, 0x8fff); hi = random_range(0, 0x8fff); // 0..0x6fff each (span wraps)
  id = (long)lo + ((long)hi << 16) + 1;
  if (DS:05ae < 50) FAR:a6d8[DS:05ae++] = id;                 // the session's save-id history:
  else { shift FAR:a6dc.. down by 4; FAR:a79c = id; }          //   this save cannot be loaded again
  write(&id, 4);
  FAR:57cf = FAR:56dc = random_range(0, 255) + 0x78;          // cipher key
  pad = random_range(0, 255); write(&pad, 1); write(&FAR:57cf, 1);
  blocks (DS:05fa[0] = DS:05ac right before block 54), strings, tail as in savegame.md;
  file_close(fd);
  if (*fixed_name == 0 && DS:224e/2250 == DS:2252/2254) DS:05ad = 1;   // both options are 1
  ```
  `DS:05ad` then blocks the menu save and ending the turn by a click (flow.md §6.6). A save the
  port writes loads in the original (checked in DOSBox). The port's save path reproduces all six
  shipped saves byte for byte from the RNG state before their header draws.
* Load `void screen_load_game(...)`: callers `start_menu_new_or_load` 18bc:2184 and
  `office_menu_dispatcher`. Signature mismatch: `LBL[285..288] "Kein" / "Bundesliga Manager
  Prof." / "Spielstand." / "!!!"`; wrong version: `UI[18..19] "Spielstand aus" / "falscher
  Version !"`; save id already loaded this session: `LBL[192..193] "Keinen heutigen" /
  "Spielstand laden !"`; V1 save: `"Sie haben einen V1.x-Spielstand geladen."` and `"Bitte
  geben Sie den Spiel-Level (0-5) ein:"`; checksum mismatch: endless `UI[136..139]` "DER GROSSE
  MOGELGURU" screen. Disk prompt `LBL[143] "Spielstanddisk"`.

  ```c
  copy_protection_penalty(LBL[107]); font 3; title; DS:9440 = 0;
  file_selector(cwd, 0, name, icon);  if (!name[0]) return 0;
  path = FAR:4af6 + (FAR:4af6[3] ? "\\" : "") + name;
  if (!(fd = file_open(path, 0))) { disk_error_dialog(); goto out; }  // 4-line dialog_okay
  read(fd, sig, strlen(DS:93e0));
  if (strncmp(sig, DS:93e0, 0x18)) {
      dialog_okay(LBL[285..288]); file_close(fd); return 0;          // "Kein ... !!!"
  }
  messages_free_all();                                               // 4091:0532
  read(fd, version, 4); read(fd, &id, 4); file_close(fd);
  if (strncmp(version, ">V", 2)) { ui_dialog_box(UI[18], UI[19]); goto out; }
  if (id in FAR:a6d8[0..50]) { ui_dialog_box(LBL[192], LBL[193]); goto out; }
  append id (as in screen_save_game); strcpy(FAR:54a2, name);
  while (!(fd = file_open(path, 0))) ui_dialog_box(LBL[143], "");   // "Spielstanddisk"
  read all blocks; a checksum mismatch sets bad = 5;  DS:05ac = DS:05fa[0];
  if (DS:9440 == 0) {                                                // no read error
      crests into DS:05b0, screen_fade(0), portraits into DS:4d38;
      if (bad) the endless MOGELGURU screen (fade, title font 3 colour 0x12,
               two lines font 2, a frame flashing 0 / 0x12 every 20 ticks);
      DS:9440 = 0; DS:05ad = 0;                                      // may save again
      if (version[2] == '1') {
          screen_fade(0); rastport_clear(8); font 1;
          text_centered_shadow(DS:9448, 0..0x13f, y 0x14, colour 1, shadow 0);
          text_centered_shadow(DS:9471, 0..0x13f, y 0x1e, colour 1, shadow 0);
          screen_fade(1);
          v = ui_value_spinner(3, x 0x7d, y 0x32, 0, 5, step 1);  if (v == -1) v = 3;
          DS:4a28 = 5 - v;                                           // level
      }
      return 1;
  }
  out: DS:9440 = 0; return 0;
  ```
  `sig` and `version` share the stack buffer `[BP-0xf4]`, and a short `read` leaves the rest of
  it stale: a file shorter than 24 bytes fails the signature check ("Kein ... !!!", not a disk
  error), and a file with the signature but fewer than two version bytes fails the version
  check (the buffer still starts with the signature's `"We"`).
  The `ui_dialog_box` boxes are timed (no click). Port: `crates/bmp/src/game/load_game.rs`
  (`screen_load_game`); the deviations are listed in §7a.

### 5.3 High scores (`HIGH.XY`)

File format: savegame.md "HIGH.XY high-score files". Table in memory: `FAR:1d52`, 20 entries
of 0x3a bytes (`+0x00` manager name, `+0x1a` club name, `+0x31..+0x33` the three "(X/X/X)"
digits, `+0x34` int16 score, `+0x36` int32 copied from `DS:0668`). Candidate entry:
`FAR:4cfe` (same layout, score at `FAR:4d32`).

**`highscore_load_save` 422b:21c4** `uint8 (int8 write)`: builds the name `HIGH.XY`
(`Y = '5' - DS:4a28`, `X = '0' + variant`, `variant` = 1 if `FAR:513c` is 0x7ac or 0x7c9,
3 if 0x7ae or 0x7cb, else 0). `write == 0`: read 1160 bytes if the file exists; otherwise check
that `mana.dat` exists (fade out if not), zero the table (copy of a zeroed buffer) and create
the file. `write != 0`: write the table. **Returns the variant** (0, 1 or 3).

**`highscore_compute_score` 422b:2864** `int32 (uint8 m)`:

```c
int32 s = 57 - T(M(m)[0x1e])[0x2e] - (int8)DS:226e[M(m)[0x138]];   // overall rank term
if (s < 0) s = 0;
s /= 2;
for (k = 0; k < 4; k++) { uint8 c = M(m)[0x132+k]; if (c != 0 && c < 7) s += 2*c; }
uint8 n = DS:07e2; if (n > 50) n = 50;                              // logged rounds
for (i = 0; i < n; i++) {
    uint8 far *e = M(m) + 0x3e + 4*i;
    s += (int16)(58 - e[0]) / 2;                                      // C division
    uint8 b = e[2];
    if (b & 0x80) {
        s += 20;
        uint8 b3 = e[3];
        s += (b3 & 0x80) ? 25 : 3 * (b3 & 7);
    } else s += 2 * (b & 7);
}
if (n != 0) s /= (n + 1);
return s * 10;
```

**`highscore_build_entry` 422b:2a2a** `void (uint8 m, uint8 variant)` (callers
`highscore_screen`, `season_main_loop` game-over path with `variant = 1`):

```c
strcpy(FAR:4cfe, M(m).name);  strcpy(FAR:4d18, C(M(m)[0x1e]).name);
int16 score = (int16)highscore_compute_score(m);
int32 v = 0;
for (i = 0; i < squad_count(m, 0); i++) v += transfer_value(i, 0);   // 3277:25de
v += stadium_attraction() * 135000;
v += M(m)[0x1f0] - finance_sum_entries(0, 0x63);                      // balance - debts
score += (int16)(v / 80000);
FAR:4d2f = M(m)[0x39]; FAR:4d30 = M(m)[0x3a];
FAR:4d31 = M(m)[0x3b] + M(m)[0x3c] + M(m)[0x3d];                    // u8 sum
score += FAR:4d2f * 40 + FAR:4d30 * 20 + FAR:4d31 * 60 - 500;        // 8-bit MULs, see note
if (variant == 0) score += 300;
if (score < 1) score = 1;
FAR:4d32 = score;
```

Note: `FAR:4d2f*40` and `FAR:4d30*20` are 8-bit `MUL`s (16-bit result), `FAR:4d31*-60` is a
16-bit `IMUL` of the zero-extended byte.

**`highscore_screen` 422b:2366** `void (int8 mode)`; mode 0 = show the table only, 1 = show
with the current manager's preview, 2 = insert the current manager silently and save.
Callers: `office_menu_dispatcher` (mode 1, menu entry), `season_main_loop` (mode 2 for every
manager at each season end, followed by `advertising_offers_generate`; mode 0 after the
game-over insertion), itself (mode 2).

```c
if (mode != 2) screen_fade(0);
void far *tmp = mem_alloc(0x488, 1);                      // 422b:715c, zero-filled block
far_copy(tmp, FAR:1d52, 0x488);                           // 422b:685a(src, dst, n): LDS SI=[bp+6],
                                                          //   LES DI=[bp+0xa], REP MOVSB -> the
                                                          //   table FAR:1d52 is CLEARED here
mem_free(tmp, 0x488);                                     // 422b:71d8
// (highscore_load_save(0) below refills the table from HIGH.XY or zeroes it anyway)
uint8 variant = highscore_load_save(0);
if (mode != 0) highscore_build_entry(FAR:304a, variant);
if (mode != 2) { load PIC 0x2d (pointer hidden); font 2; }
int8 hit = -1; int16 mark = 0;
for (i = 0; i < 20; i++) {                                // insertion of the candidate
    if (hit != -1) continue;
    bool same = strcmp(E(i).name, FAR:4cfe) == 0 && strcmp(E(i).club, FAR:4d18) == 0;
    if (!same && !(i == 19 && E(19).score < FAR:4d32)) continue;
    hit = i; E(i) = candidate (58 bytes); E(i)[0x36] = DS:0668 (int32);
    for (a = 0; a < 19; a++) for (b = a+1; b < 20; b++)   // exchange sort, descending
        if (E(a).score < E(b).score) swap(E(a), E(b));    // via the buffer FAR:4cfe!
    if (mode == 2) { highscore_load_save(1); return; }
    i = 19;
}
if (mode == 2) return;
for (i = 0; i < 20; i++) {                                // display
    if (mode != 0 && mark == 0 && E(i).score <= FAR:4d32) {   // preview insert
        for (j = 19; j > i; j--) E(j) = E(j-1);
        E(i) = FAR:4cfe; mark = i + 1;                    // highlighted row
    }
    if (mode == 0) memset(DS:06fc[i], 'a', 20);           // overwrites permutation string i!
    if (E(i).score != 0) {
        int16 hl = (variant != 0) ? (i + 1 == mark)                       // 422b:2668..26ac
                                  : (*(int32*)(E(i)+0x36) == DS:0668);   // int32 compare
        int16 colour = hl * 2 + 1;                         // 1 normal, 3 highlighted
        all four columns use `colour`:
        draw name at x 0x38, club at x 0x68, UI[140] "PUNKTE:" + score
        (field width 5) at x 0xbb, "(X/X/X)" with the digits E(i)[0x31..0x33] + '0' at x 0xf3;
    }
}
screen_fade(1);
if (variant == 0) highscore_screen(2);                    // re-load, insert, save
input_wait_click();
```

Consequences for a port:

* `mark` is the local `[bp-0x70]`, which the insertion's sort also uses as its outer counter: after
  an insertion it is 19, so mode 1 then shows no preview row and, in the one-year variants, row 18
  (`i + 1 == 19`) would be highlighted. In the normal game every entry whose `+0x36` equals
  `DS:0668` is highlighted; `DS:0668` is 0 in the data image (writer not traced), so after loading a
  game all entries are drawn in colour 3. Verified in DOSBox with `07072019.MAN`: the candidate
  replaces OLIVER / BAYER 05 UERDINGEN (score 131 -> 96) and mode 2 writes `HIGH.00` with exactly
  that change.
* `HIGH.XY` is written (`highscore_load_save(1)`) from exactly two call sites
  (xrefs to 422b:21c4: 2a58:41b6 load, 2a58:432a save, 422b:23de load, 422b:24e4 save):
  1. `highscore_screen(2)` at 422b:24e4, after a successful insertion: called by
     `season_main_loop` 2a58:4359 at every season end for every human manager, and by
     `highscore_screen` itself (422b:2845) after viewing the table from the office menu
     (mode 1) when `variant == 0`. Mode 2 saves only when the candidate was inserted
     (`hit != -1`); otherwise it returns without writing.
  2. Directly by `season_main_loop` on the game-over path: `highscore_load_save(0)` at
     2a58:41b6, the inline insertion with `highscore_build_entry(m, 1)` for every manager
     (2a58:4314), then `highscore_load_save(1)` at 2a58:432a, then `highscore_screen(0)` at
     2a58:4333 (display only, its own `highscore_load_save(0)` re-reads the file just
     written), then exit code 99 (2a58:4341).
* The candidate swap during the sort goes through `FAR:4cfe`, so after a sort the candidate
  buffer holds the last swapped entry; the display loop then compares with that buffer.
* Mode 0 (game over) fills the first 20 fixture permutation strings with `'a'` (harmless
  because the game exits afterwards with code 99). Verified: the only mode-0 caller is
  2a58:4333 on the game-over path (xrefs to 422b:2366: 2a58:4333 mode 0, 2a58:4359 mode 2,
  18bc:1bcc office menu mode 1, 422b:2845 self mode 2).
* The game-over path in `season_main_loop` (2a58:4249..4324, season agent) repeats the
  insertion inline for every manager with `highscore_build_entry(m, 1)`, saves with
  `highscore_load_save(1)` and then shows `highscore_screen(0)`.

### 5.4 `file_selector` 422b:2bbe

`char far *file_selector(...)` (callers `screen_save_game`, `screen_load_game`): directory
listing of `"*.*"` filtered to `".MAN"`, pages (`UI[146] "SEITE: "`), header `UI[144]
"VERZEICHNIS: "`, name input `UI[145] "FILENAME      :"` (no character filter:
`text_filter_filename_char` 176b:0d56 has no caller, see audit/uncited.md; `".MAN"` appended), delete with confirmation `LBL[211]
"WIRKLICH L|SCHEN ?"` / `LBL[212] "MACH SCHON!"` / `LBL[213] "OH NEIN!"`. Pure UI; draw
details in Appendix A.

Buttons below the list (`ui_register_icon_slot`, slots 1..8): seven drive labels `DS:93ec`
(`"A:"`..`"G:"`, shown only while the slot is below the number of logical drives that
`_dos_setdrive` reports) and `"L|SCHEN"` (`DS:2994`). On a left click:

```c
if (slot == 8 && name[0] &&
    dialog_two_choice(FAR:304a, "FILE" /* DS:94bb */, name, LBL[211], LBL[212], LBL[213])) {
    dos_delete_file(name);  name[0] = 0;  goto relist;               // also on -1 (auto play)
}
if (slot <= logical_drives) { _chdrive(slot); goto relist; }        // slot 8 = drive H: too
```

`relist` resets the page, calls `getcwd(cwd)` for the `"VERZEICHNIS: "` line and lists
`findfirst("*.*", 0x10)` again. On leaving, `DS:942c = _getdrive()` and `getcwd(FAR:4af6)`
are stored and the drive and directory of the start are restored; the next call starts with
`_chdrive(DS:942c); chdir(FAR:4af6)` when `DS:942c != -1`. Under DOSBox (the reference setup,
only C: mounted) `_chdrive` succeeds for A: and B: even without a disk: `findfirst` then
finds nothing (an empty list, not even `.`/`..`), `getcwd` fails and leaves the caller's
buffer as it was, and the selector stays on that drive until C: is clicked. Loading and
saving still use `FAR:4af6`, which keeps the C: directory. D:..G: fail and change nothing.
Port: `file_selector` with `SaveSession::drive`.

### 5.5 New-game player pool from MANA.DAT

**`mana_team_player_name_ptr` 422b:01d0** `char far *(char far *buf, uint8 t, int16 slot)`:
walks the 64 German club records (0x21f bytes) in `buf` until the record name equals
`C(t).name` (at most 64 steps; if none matches the pointer is past the 64th record) and returns
`record + 0x17 + 0x1a*slot`.

**`mana_pick_unused_player_name` 422b:022e** `char far *(char far *buf, int16 unused, int8 pos,
uint8 p)` (RNG):

```c
for (;;) {
    int16 slot = random_range(DS:9432[pos], DS:9433[pos] - 1);         // RNG
    uint8 t;
    do {
        t = random_range(0, (int8)DS:225e[0] - 1);                       // RNG, Bundesliga team
    } while (t is the team of any human manager);
    char far *name = mana_team_player_name_ptr(buf, t, slot);
    char tmp[26]; strcpy(tmp, name);
    if (tmp[0] == 0) continue;
    for (q = 1; q < 151 && strcmp(tmp, P(q).name) != 0; q++);
    if (q != 151) continue;                                              // already used
    if (p != 0xff) P(p)[0x24] = t;
    return name;
}
```

**`newgame_init_player_pool` 422b:035c** (mana.md calls it `mana_load_player_names`) `void (void)` (caller `start_menu_new_or_load`, new
game). Allocates 40000 bytes (`fatal_error_exit(4)` on failure), reads MANA.DAT bytes 0..0x3d7
(discarded) and the 64 German club records (0x87c0 bytes), then:

```c
for (m = 0; m < 4; m++) for (i = 0; i < 25; i++) Q(m,i)[0x0f] = 0;
for (p = 1; p < 151; p++) {
    P(p)[0x1a] = random_range(18, 33);                         // age
    int16 a = random_range(30, 89);
    if (a < 45 && random_range(0, 1) != 0) a = random_range(30, 89);
    P(p)[0x1d] = a - random_range(0, 20) + 10;                 // u8
    P(p)[0x1e] = random_range(45, 55);
    P(p)[0x1c] = a - random_range(0, 20) + 10;
    P(p)[0x1b] = (P(p)[0x1c] + P(p)[0x1d] + P(p)[0x1e]) / 3;
    P(p)[0x20] = random_range(0, 6);
    P(p)[0x21] = 5;
}
for (m = 0; m < DS:07ab; m++) {                                // human squads: 20 players each
    FAR:5358 = m;  uint8 slot = 0;
    for (g = 0; g < 4; g++) for (j = 0; j < DS:9438[g]; j++) {
        uint16 p = DS:07a7[g] - DS:943c[g] + 1;  DS:943c[g]--;
        char far *name = mana_team_player_name_ptr(buf, M(FAR:304a)[0x1e], slot);
        strcpy(P(p).name, name);
        if (name[0] == 0) strcpy(P(p).name, mana_pick_unused_player_name(buf, 0, g, 0xff));
        slot++;
        P(p)[0x21] = FAR:304a;
        P(p)[0x1a] = random_range(19, 31);
        transfer_list_add(p, 2, 0, 15000L, 0x18);              // 2ecd:37d8, squad agent
        P(p)[0x24] = M(FAR:304a)[0x1e];
    }
}
uint8 g = 0;
for (p = 1; p < 151; p++) {                                    // remaining names
    if (DS:07a7[g] <= p) g++;
    if (P(p).name[0] == 0) {
        strcpy(P(p).name, mana_pick_unused_player_name(buf, 1, g, p));
        if (FAR:513c > 2000) P(p)[0x24] = 0xff;
    }
}
for (p = 1; p <= 20; p++)  P(p)[0x1f] = 0;
for (p = 21; p <= 63; p++) P(p)[0x1f] = random_range(0, 16) + 25;
for (p = 64; p <= 105; p++) P(p)[0x1f] = random_range(0, 16) + 50;
for (p = 106; p <= 150; p++) P(p)[0x1f] = random_range(0, 16) + 75;
free(buf);
```

Note: the human-squad loop uses `FAR:304a` (the current manager) for the team and owner,
while the loop variable `m` is stored in `FAR:5358`; `FAR:304a` is set to 0 before the loop and
incremented by the loop itself (it *is* the loop variable: 422b:052d sets `FAR:304a = 0`,
422b:067b increments it). `DS:943c` are static counters that are never reset, so a second new
game in the same session continues where the first stopped.

## 6. Globals used

| Address | Size / type | Suggested name | Save block | Used by |
|---|---|---|---|---|
| DS:0644 | int32[4] | mgr_finance_carry | — | income/expense estimates |
| DS:0668 | int32 | highscore_stamp | — | copied into high-score entries `+0x36` |
| DS:066c | int32[4][9] | mgr_monthly_items | 41 | income (0..7), expenses (8), contracts |
| DS:05ad | u8 | load_blocked_flag | — | autosave guard, cheat 'd' |
| DS:05ae | u8 | save_id_count | — | cheat 'd' |
| DS:05bc | u8[18] | injury_base_weeks | const | `player_injure` |
| DS:05d4 | u8 | abort_flag | — | all screens |
| DS:05f5 | u8 | cheat_p_flag | — | cheat 'p' |
| DS:05fe | u8[15] | display_options | 36 | settings toggles |
| DS:063a | u16 | game_speed | 47 | settings slider |
| DS:06fc | far ptr[38] | round_permutations | const (mutated by highscore mode 0) | fixtures |
| DS:0798 | far ptr | contract_palette | — | contract screen |
| DS:079c | u8 | number_field_width | — | `format_long_thousands` |
| DS:07a7 | u8[4] | pool_group_bounds | const | new game |
| DS:07ab | u8 | manager_count | 7 | everywhere |
| DS:07b2 | u8 | text_box_open | — | contract lines |
| DS:07b3 | u8 | icon_slot_count | — | icon screens |
| DS:07e2 | u8 (read as a byte at 422b:292e `MOV AL,ES:[0x7e2]`) | logged_rounds | 48 | high score |
| DS:224d | char | ignore_games_tiebreak | — | table sort, contract text |
| DS:224e / DS:2252 | int32 | money_scale_den / money_scale_num | — | money display |
| DS:225a | u8[3] | current_round | 34 | all league code |
| DS:225e | u8[3] | league_team_count | const | |
| DS:2262 | u8[3] | league_round_count | const | |
| DS:2266 | u8[3] | relegation_band | const | league table |
| DS:226e | u8[3] | league_first_team | const | |
| DS:2271 | u8 | points_per_win | const | |
| DS:2272 | u8[3][2] | tier_team_range | const | top scorers |
| DS:227a | char[6] | form_chars | const | |
| DS:2388 | far ptr[3] | league_names (UI[42..44]) | const | |
| DS:4a28 | int8 | difficulty (5 - level) | 42 | offers, high score file name, cheats |
| DS:5331 | u8[11] | scorer_weights | const | AI scorers |
| DS:5454 / DS:5464 | u8[15] / u16[15] | settings_toggle_x / _y | const | settings |
| DS:5498 | u16[13] | stats_palette | const | statistics |
| DS:54b2, 54d8, 54dc | u8[] | column maps / icon colours | const | statistics, opponent info |
| DS:9432..943f | u8[] | name_slot_ranges, group_quota, group_free | const / mutable | new game |
| DS:9440 | u8 | io_error_flag | — | save/load |
| DS:949c | char[8] | "HIGH.00" (bytes 5/6 patched) | — | high score |
| DS:a322 | char[] | xor_text_buffer | — | `string_xor_copy` |
| DS:a390 | char[] | long_or_blank_buffer | — | `format_long_or_blank` |
| FAR:002e | int32[4][2][10] | ad_offer_amount | 40 | offers |
| FAR:0ecc | 64 x 0x36 | team_season_stats | 16 | everything in 3a41 |
| FAR:1d52 | 20 x 0x3a | highscore_table | — (HIGH.XY) | high score |
| FAR:2242 | 4 x 0x30a | manager_records | 13 | |
| FAR:2e6a | u16 | streak_line_count | — | opponent info |
| FAR:2eba | int32 | day_of_month | 20 | finance divisors |
| FAR:2ec2 | u8[4][2][10] | ad_offer_years | 39 | offers |
| FAR:304a | u8 | current_manager | 3 | |
| FAR:3066 | 200 x 0x22 | club_records | 15 | |
| FAR:4b5e | u8[3][10][2] | round_pairings | 19 | |
| FAR:4cfe | 0x3a bytes | highscore_candidate | — | high score |
| FAR:513c | u16 | edition_year | 43 | high-score variant, settings, pool |
| FAR:5358 | u8 | saved_manager | 4 | finance loop, new game |
| FAR:535a | u8[3][20] | table_order | 32 | |
| FAR:5716 | 5-byte records | rescheduled_matches | 14 | fixtures |
| FAR:57dd | 151 x 0x25 | player_pool | 17 | |
| FAR:6dc0 | far ptr | mouse_state | — | |
| FAR:6ddc | u8[3][38][10][2] | round_results | 6 | |
| FAR:774a | 5 x 25 x 0x34 | squads | 18 | |
| FAR:9336 | 4 x 64 x 5 x 2 | head_to_head_log | 35 | |
| FAR:9d36 | 64 x 21 | team_streaks | 35 | |
| FAR:a276 | 4 x 21 | manager_best_streaks | 35 | |
| FAR:a2ca / FAR:a4ca | 64 x 8 | team_records / team_record_opponents | 35 | |
| FAR:a6d8 | int32[50] | loaded_save_ids | — | cheat 'd' |
| FAR:a7a0 | int32 | (pool flag, block 22) | 22 | new-game pool |

Block 35 (`FAR:9336`, 5012 bytes) therefore is: head-to-head log (2560) + streaks (1344) +
best streaks (84) + records (512) + record opponents (512) = 5012.

## 7. Open questions

1. **Mouse word order** (§0.2): these screens compare word `[0]` of `*FAR:6dc0` with row (Y)
   coordinates; MEMORY_MAP §3.1 says `[0]` = x. Needs a check in `input_poll_mouse`/the INT 33h
   handler.
2. **Snapshot semantics of `league_apply_round_and_sort`**: mode 0 restores the snapshot before
   adding a result but increments games, away wins/losses, streaks, records and head-to-head
   logs without restoring them, so mode 0 must run exactly once per round. Which caller runs
   mode 1 and when (before the live ticker?) has to be confirmed with the match-day agent.
   Evidence from the saves: rounds not played yet hold 0:0, not `0x1e`, and every snapshot
   column equals the current one. Replaying the saved results with mode 1 then mode 0 per round
   from empty tables reproduces the saved totals and table order of all three leagues
   (`bmp_game::league` test `real_saves_replay_the_league_tables`).
3. `club_strength_boost` 24a4:23da sums with the uninitialised local `[BP-4]` as index (the loop
   counter is `[BP-6]`), so the result depends on stack garbage left by the caller. A port
   must reproduce the stack content of `screen_transfer_market` or pick a documented value;
   decide with the transfer agent.
4. ~~`assign_pool_players_to_teams` 24a4:19fb: the inner loop needs an asm pass.~~ Resolved: see
   §3.3 (checked against the disassembly; `bmp_game::newgame::assign_pool_players_to_teams`).
5. `transfer_list_add(p, 2, 0, 15000, 0x18)` (2ecd:37d8) and `transfer_value(i, 0)` (3277:25de)
   semantics belong to the squad/transfer agents; in `highscore_build_entry` the loop passes the
   squad index `i` (not a player id).
6. The draw sequences of `screen_manager_history`, `screen_manager_contract`,
   `screen_player_info`, `file_selector`, `screen_save_game` and `screen_load_game` are only
   given as call traces (Appendix A); their click areas were not re-derived by hand.
7. `FAR:513c` (`edition_year`): compared with 2000 and with 0x7ac/0x7c9/0x7ae/0x7cb; savegame.md
   observed 0x56ce in all saves. Meaning unclear.
8. `24a4:2b15` has no caller (dead code or reached through an unrecovered pointer).
9. The two-point table display in `screen_league_table` (minus points) was read from the trace
   only (`DS:2271 < 3` branches); verify the exact formula when porting.
10. `DS:5485` (`"^@"`) is printed for zero values; the font maps these characters to special
    glyphs (to be confirmed with font.md).

## 7a. Deviations of the port

Obvious bugs of the original that the port fixes (MISSION invariant 1); RNG order and save
files are unchanged.

1. `screen_load_game` frees the message list (`messages_free_all` 4091:0532) right after the
   signature check, before the version and the save id are checked. A load from the office
   that is then refused ("Spielstand aus falscher Version !", "Keinen heutigen Spielstand
   laden !") loses the running game's messages. The port keeps them: a refused load never
   touches the running game (`crates/bmp/src/game/load_game.rs`, test
   `refused_loads_show_their_dialogs_and_load_nothing`).
2. A read error after the header (`DS:9440` set by `savegame_read_decrypted`) leaves the blocks
   read so far in memory and returns 0, so the running game continues with half-loaded data.
   The port loads nothing (`LoadError::Disk`, `disk_error_dialog`).
3. `file_selector` on an empty floppy drive draws `"VERZEICHNIS: "` with the caller's
   uninitialised path buffer after `getcwd` fails (seen in DOSBox: the last directory, an
   empty string, or garbage such as `"U.07"`). The port shows the C: directory. Expected
   difference in `tests/visual/disk_dialogs` frames k09 and k13 (the line only).
4. `screen_load_game` loads a save whose tier rows (save block 32) are the Mod 2026 layout's
   (18 / 18 / 20 clubs, ids 0..=17, 18..=35, 36..=55), rows the original never writes: league
   tier 1 holds club 0 twice (its slots 18 and 19 are 0) and clubs 36 and 37 play in tier 2.
   The game then plays on with a broken league. The port refuses such a save on the original
   data like a save of the other layout on Mod 2026 data (`LoadError::OtherLayout`, a
   `dialog_okay` box; `bmp_game::league_layout::saved_layout`, #218) and forgets its id again.
   Every save the original or the port writes on the original data still loads (test
   `a_mod_save_is_refused_in_an_original_run`; `real_saves_play_to_the_next_season` checks the
   layout of every real save before and after its season change).

## Appendix A. Screen call traces

Generated from the annotated disassembly (`out/stats/calls2.py`). One line per call: offset in the
function's segment, callee, then the arguments in C order (first argument first). The rastport
arguments `FAR:[0x2f28], FAR:[0x2f2a]` are omitted. `LBL[n]"..."`/`UI[n]"..."` are string-table
entries, `0xNNNN="..."` immediates that point to DGROUP strings (some are false positives: far
data offsets that happen to hit a string). Register arguments show the last value loaded into
the register; `&[BP + x]` are stack buffers. Argument lists are reconstructed from pushes and may
contain stray prologue pushes at the start of a function.

### 3a41:000e screen_opponent_info

```
006b ui_draw_button ( 0, 0x37, 0xe0, 0xae, &[BP + 0xff78], 0x1, 0x8, SI, DI )
0087 ui_set_font ( 0x1 )
00a2 crt_fstrcpy ( &[BP + -0x56], LBL[43]"Info ]ber " )
00bc crt_fstrcat ( &[BP + -0x56], 0x22, 0x5238 )
00e7 ui_draw_text_centered_shadow ( &[BP + -0x56], AX, &[SI + 0xe0], &[DI + 0x8], 0xb, 0 )
010e ui_draw_line ( &[SI + 0x1], &[DI + 0xa], &[SI + 0xdf], &[DI + 0xa], 0xb )
0131 ui_set_font ( 0x2 )
0153 ~ui_register_icon_slot ( &[SI + 0x4], &[DI + 0x9f], UI[408]"REICHT MIR", 0xa )
0190 ui_draw_text_centered_shadow ( LBL[44]"IHRE SP[HER K|NNEN LEIDER NUR", AX, &[SI + 0xe0], &[DI + 0x44], 0x1, 0 )
01c4 ui_draw_text_centered_shadow ( LBL[45]"AUF DEUTSCHEM BODEN OPERIEREN...", AX, &[SI + 0xe0], &[DI + 0x4c], 0x1, 0 )
01f8 ui_draw_text_centered_shadow ( LBL[46]"WILL SAGEN: KEINE INFORMATIONEN VERF]GBAR.", AX, &[SI + 0xe0], &[DI + 0x56], 0x1, 0 )
0221 ~ui_register_icon_slot ( &[SI + 0x54], &[DI + 0x9f], LBL[47]"RESTPROGRAMM", 0xa )
025c ~ui_register_icon_slot ( &[SI + 0xa4], &[DI + 0x9f], LBL[48]"ANZEIGEN", b[BX + 0x54dc] )
027e format_long_thousands ( 0, &[SI + 0xe0] )
028c crt_fstrcpy ( &[BP + -0x56], 0, &[SI + 0xe0] )
02a3 ~FUN_1310_1fe7 ( &[BP + -0x56], b?:[BX + 0x2e] )
02be crt_fstrcat ( &[BP + -0x56], LBL[49]" PLATZ IN DER " )
02ca ~league_of_rank ( b[BP + 0x6] )
02ea ~text_to_upper_de ( &[BP + -0x80], ?:[BX + 0x2388], ?:[BX + 0x238a] )
02fc crt_fstrcat ( &[BP + -0x56], &[BP + -0x80], SS )
0317 crt_fstrcat ( &[BP + -0x56], LBL[50]", ST[RKE: " )
0372 format_long_thousands ( [BP + 0xff72], 0 )
0380 crt_fstrcat ( &[BP + -0x56], [BP + 0xff72], 0 )
03ab ui_draw_text_centered_shadow ( &[BP + -0x56], AX, &[SI + 0xe0], &[DI + 0x12], 0x1, 0 )
03db ui_draw_text_at ( LBL[51]"HEIMBILANZ:", &[SI + 0x3], 0x37, 0x1 )
03f0 format_long_thousands ( 0, 0 )
03fe crt_fstrcpy ( &[BP + -0x56], 0, 0 )
0419 crt_fstrcat ( &[BP + -0x56], LBL[52]" SIEGE, " )
043d format_long_thousands ( b?:[BX + 0x1e], AX )
044b crt_fstrcat ( &[BP + -0x56], b?:[BX + 0x1e], AX )
0466 crt_fstrcat ( &[BP + -0x56], LBL[53]" UNENT., " )
047b format_long_thousands ( 0, 0 )
0489 crt_fstrcat ( &[BP + -0x56], 0, 0 )
04a4 crt_fstrcat ( &[BP + -0x56], LBL[54]" NIED." )
04c8 ui_draw_text_at ( &[BP + -0x56], &[SI + 0x6b], 0x37, 0x1 )
04f8 ui_draw_text_at ( LBL[55]"AUSW[RTSBILANZ:", &[SI + 0x3], &[DI + 0x8], 0x1 )
050d format_long_thousands ( 0, 0 )
051b crt_fstrcpy ( &[BP + -0x56], 0, 0 )
0536 crt_fstrcat ( &[BP + -0x56], LBL[52]" SIEGE, " )
055a format_long_thousands ( b?:[BX + 0x1f], AX )
0568 crt_fstrcat ( &[BP + -0x56], b?:[BX + 0x1f], AX )
0583 crt_fstrcat ( &[BP + -0x56], LBL[53]" UNENT., " )
0598 format_long_thousands ( 0, 0 )
05a6 crt_fstrcat ( &[BP + -0x56], 0, 0 )
05c1 crt_fstrcat ( &[BP + -0x56], LBL[54]" NIED." )
05e5 ui_draw_text_at ( &[BP + -0x56], &[SI + 0x6b], &[DI + 0x8], 0x1 )
0615 ui_draw_text_at ( LBL[56]"DIE LETZTEN SPIELE:", &[SI + 0x3], &[DI + 0x8], 0x1 )
065c ui_draw_text_at ( &[BP + -0x56], 0x7, &[DI + 0x8], 0x1 )
0682 crt_fstrcpy ( &[BP + -0x56], LBL[57]"TORE PRO SPIEL " )
06b0 crt_fstrcat ( &[BP + -0x56], ?:[BX + 0x4af8], ?:[BX + 0x4afa] )
06d7 ui_draw_text_at ( &[BP + -0x56], &[SI + 0x3], &[DI + 0x8], 0x1 )
06f0 ~FUN_362f_21e1 ( &[BP + -0x80], b[BP + 0x6], b[BP + 0x8], 0x2 )
0714 ui_draw_text_at ( &[BP + -0x80], &[SI + 0x6b], &[DI + 0x8], 0x1 )
072f crt_fstrcpy ( &[BP + -0x56], LBL[58]"SERIEN " )
074e crt_fstrcat ( &[BP + -0x56], ?:[BX + 0x4af8], ?:[BX + 0x4afa] )
0775 ui_draw_text_at ( &[BP + -0x56], &[SI + 0x3], &[DI + 0x8], 0x1 )
07c4 crt_fstrcat ( &[BP + -0x56], ?:[BX + 0x2f2c], ?:[BX + 0x2f2e] )
07e9 crt_fstrcat ( &[BP + -0x56], ?:[BX + 0x4adc], ?:[BX + 0x4ade] )
080d ui_draw_text_at ( &[BP + -0x56], &[SI + 0x3], &[DI + 0x8], 0x1 )
0879 format_long_thousands ( 0, 0 )
0887 crt_fstrcpy ( &[BP + -0x56], 0, 0 )
08a2 crt_fstrcat ( &[BP + -0x56], LBL[59]" SPIELE IN FOLGE " )
08e7 crt_fstrcat ( &[BP + -0x56], ?:[BX + 0x4adc], ?:[BX + 0x4ade] )
094b ui_draw_text_at ( LBL[60]"KEINE BEMERKENSWERTE SERIE VORHANDEN.", &[SI + 0xa], &[DI + 0x8], 0x2, ?:[BX + 0x4ad0], ?:[BX + 0x4ad2] )
097d crt_fstrcpy ( &[BP + -0x56], ?:[BX + 0x2474], ?:[BX + 0x2476] )
09a1 ui_draw_text_at ( &[BP + -0x56], &[SI + 0x3], &[DI + 0x8], 0x1 )
09be CALL 3a41:0f0e ( CS, b[BP + 0x6], b[BP + 0xff6e], &[BP + -0x56], SS )
09d5 CALL 3a41:0f0e ( CS, b[BP + 0x6], b[BP + 0xff6e], &[BP + -0x56], SS )
09ee CALL 3a41:0f0e ( CS, b[BP + 0x6], b[BP + 0xff6e], &[BP + -0x80], SS )
0a52 crt_fstrcpy ( &[BP + -0x56], &[BP + -0x80], LBL[61]" (H)" )
0a6d crt_fstrcat ( &[BP + -0x56], LBL[62]" (A)" )
0a91 ui_draw_text_at ( &[BP + -0x56], &[SI + 0x6b], &[DI + 0x8], 0x1 )
0b1b crt_fstrcpy ( &[BP + -0x56], LBL[63]"HISTORISCHE ERGEBNISSE GEGEN ", ?:[BX + 0x4af4], ?:[BX + 0x4af6] )
0b4b crt_fstrcat ( &[BP + -0x56], 0x22, 0x5238 )
0b6f ui_draw_text_at ( &[BP + -0x56], &[SI + 0x3], &[DI + 0x8], 0x1 )
0b8d crt_fstrcpy ( &[BP + -0x56], LBL[64]"H: " )
0be9 crt_fstrcat ( &[BP + -0x56], 0x54d4=",", DS )
0c39 format_long_thousands ( 0, 0 )
0c47 crt_fstrcat ( &[BP + -0x56], 0, 0 )
0c59 crt_fstrcat ( &[BP + -0x56], 0x54d6=":", DS )
0c93 format_long_thousands ( b?:[BX + 0x9337], 0 )
0ca1 crt_fstrcat ( &[BP + -0x56], b?:[BX + 0x9337], 0 )
0cc3 crt_fstrlen ( &[BP + -0x56], SS )
0ce2 crt_fstrcat ( &[BP + -0x56], LBL[65]" NOCH KEINE" )
0d04 crt_fstrcat ( &[BP + -0x56], LBL[66]" -- A: " )
0d1f crt_fstrlen ( &[BP + -0x56], SS )
0d45 crt_fstrcat ( &[BP + -0x56], LBL[65]" NOCH KEINE" )
0d69 ui_draw_text_at ( &[BP + -0x56], &[SI + 0x3], &[DI + 0x8], 0x1 )
0d87 crt_fstrcpy ( &[BP + -0x56], LBL[67]"PUNKTE IN DER EWIGEN TABELLE: " )
0da8 format_long_thousands ( ?:[BX + 0x32], ?:[BX + 0x34] )
0db6 crt_fstrcat ( &[BP + -0x56], ?:[BX + 0x34], 0 )
0dd3 crt_fstrcat ( &[BP + -0x56], LBL[68]"NOCH KEINEN EINZIGEN..." )
0dfc ui_draw_text_at ( &[BP + -0x56], &[SI + 0x3], &[DI + 0x8], 0 )
0e27 input_poll_mouse ( FAR:[0x6dc0], FAR:[0x6dc2] )
0e2e ~ui_hit_icon_slot (  )
0ea0 ~ui_register_icon_slot ( &[SI + 0xa4], &[DI + 0x9f], LBL[48]"ANZEIGEN", b[BX + 0x54dc] )
0ed3 ~screen_fixture_results ( b[BP + 0x6], b[BP + 0xa], b[BP + 0xc] )
0f01 ui_redraw_buttons ( b[BP + -0x58], 0 )
```

### 3a41:120a screen_fixtures

```
122f ~manager_ai_weekly ( SI, 0, SI, DI )
125a copy_protection_penalty ( LBL[112]"Spielplan", 0, 0, 0 )
128b ui_draw_button ( 0x1, 0x2, 0x13a, 0xb9, &[BP + -0x76], 0, 0x8 )
12c8 ui_draw_button ( 0x10c="font", 0xc1, 0, 0, &[BP + -0x6e], 0, 0x8 )
12fe ui_draw_button ( 0x1, 0xc1, 0, 0x28, &[BP + -0x66], 0, 0x8 )
132e ui_draw_button ( 0x3b, 0xc1, 0, 0x27, &[BP + -0x5e], 0, 0x8 )
1357 ui_draw_line ( 0x3, 0xe, 0x139="\", 0xe, 0xb )
13af ui_fill_rect ( 0x6, 0x3, 0x13a, 0xd, 0x8 )
13d3 crt_fstrcpy ( &[BP + -0x42], LBL[70]"SPIELE  " )
13f7 crt_fstrcat ( &[BP + -0x42], ?:[BX + 0x2388], ?:[BX + 0x238a] )
1409 crt_fstrcat ( &[BP + -0x42], 0x54e5="   ", DS )
1418 format_long_thousands ( b[BP + -0x46], 0 )
1426 crt_fstrcat ( &[BP + -0x42], b[BP + -0x46], 0 )
1437 ~FUN_1310_1fe7 ( &[BP + -0x42], b[BP + -0x46] )
1452 crt_fstrcat ( &[BP + -0x42], LBL[71]"SPIELTAG  " )
146f ~fixture_info_helper ( 0x1, b[BP + -0x46] )
148e ~matchday_refresh ( [BP + -0x4c] )
14a2 format_long_thousands ( FAR:[0x2eba], FAR:[0x2ebc] )
14b0 crt_fstrcat ( &[BP + -0x42], 0x1, 0 )
14cb crt_fstrcat ( &[BP + -0x42], DS:[0x299c], DS:[0x299e] )
14e8 format_long_thousands ( FAR:[0x2e8e], FAR:[0x2e90] )
14f6 crt_fstrcat ( &[BP + -0x42], FAR:[0x2e8e], FAR:[0x2e90] )
1511 crt_fstrcat ( &[BP + -0x42], DS:[0x29a0], DS:[0x29a2] )
152b crt_fstrcpy ( &[BP + -0x42], LBL[72]"NACHHOLSPIELE  " )
154d crt_fstrcat ( &[BP + -0x42], 0x54e9="  ", DS )
155e format_long_thousands ( 0, 0 )
156c crt_fstrcat ( &[BP + -0x42], 0, 0 )
157d ~FUN_1310_1fe7 ( &[BP + -0x42], b[BP + 0xa] )
1598 crt_fstrcat ( &[BP + -0x42], LBL[73]"MINUTE" )
15b1 ui_set_font ( 0x1 )
15de ui_draw_text_centered_shadow ( &[BP + -0x42], 0x7, 0x139="\", 0xb, 0x1, 0x7 )
15f5 CALL 3a41:10e3 ( CS, b[BP + 0x6], b[BP + -0x46] )
161b ui_fill_rect ( 0x3, 0xf, 0x13a, 0xb9, 0x8 )
169a ui_draw_text_at ( 0x22, 0x5238, 0x3, [BP + -0x7e], [BP + -0x80] )
16b8 rastport_move_to ( 0x88, [BP + 0xff7e] )
16d7 rastport_draw_text ( 0x54ec="-", DS, 0x1 )
1705 ui_draw_text_at ( 0x22, 0x5238, 0x92, [BP + 0xff7c], [BP + -0x80] )
171e ui_set_font ( 0x2 )
1737 CALL 3a41:1d18 ( CS, &[BP + -0x42], b[BP + -0x7c], b[BP + 0x6], 0x1 )
1763 ui_draw_text_at ( &[BP + -0x42], 0x5, [BP + -0x7e], 0x4 )
177c CALL 3a41:1d18 ( CS, &[BP + -0x42], b[BP + -0x4e], b[BP + 0x6], 0x1 )
17a1 ui_draw_text_at ( &[BP + -0x42], 0x94, [BP + 0xff7a], 0x4 )
17ba ui_set_font ( 0x1 )
1831 ~matchday_refresh ( 0 )
184f format_long_thousands ( FAR:[0x2eba], FAR:[0x2ebc] )
185d crt_fstrcpy ( &[BP + -0x42], 0, 0 )
1878 crt_fstrcat ( &[BP + -0x42], DS:[0x299c], DS:[0x299e] )
1895 format_long_thousands ( FAR:[0x2e8e], FAR:[0x2e90] )
18a3 crt_fstrcat ( &[BP + -0x42], FAR:[0x2e8e], FAR:[0x2e90] )
18be crt_fstrcat ( &[BP + -0x42], DS:[0x29a0], DS:[0x29a2] )
18d9 crt_fstrcat ( &[BP + -0x42], W[2]" (N)" )
18f2 ui_set_font ( 0x2 )
1920 ui_draw_text_centered_shadow ( &[BP + -0x42], 0x128="t", 0x128="t", [BP + -0x7e], b[BP + -0x80], 0xff )
1939 ui_set_font ( 0x1 )
1993 rastport_set_color ( b[BP + -0x80] )
19b0 rastport_move_to ( 0x120="bold.font", [BP + -0x7e] )
19e1 format_long_thousands ( b?:[BX + 0x6dc8], FAR:[0x2e90], 0x1 )
19f8 rastport_draw_text ( b?:[BX + 0x6dc8], FAR:[0x2e90] )
1a15 rastport_move_to ( 0x129, [BP + -0x7e] )
1a34 rastport_draw_text ( 0x54ee=":", DS, 0x1 )
1a51 rastport_move_to ( 0x12e, [BP + -0x7e] )
1a6e format_long_thousands ( b?:[BX + 0x6dc9], FAR:[0x2e90], 0x1 )
1a85 rastport_draw_text ( b?:[BX + 0x6dc9], FAR:[0x2e90] )
1aac ui_draw_text_at ( 0x54f0="-:-", DS, 0x120="bold.font", [BP + -0x7e], [BP + -0x80] )
1b1b screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0x1 )
1b30 ui_hit_test_buttons ( &[BP + -0x76], b[BP + -0x44], 0x1 )
1ba1 ~ui_wait_with_highlight ( 0x1, 0xc1, 0xc1 )
1c2a mem_alloc ( 0x3c, 0, 0x3c )
1c41 far_memcpy_bytes ( 0x4b5e, 0x5238, 0x3c, FAR:[0x2e90] )
1c79 CALL 3a41:000e ( CS, b?:[BX + 0x4b5e], 0, 0x1, b[BP + -0x6] )
1c91 far_memcpy_bytes ( [BP + -0x7a], [BP + -0x78], 0x4b5e, 0x5238, 0x3c )
1ca3 mem_free ( [BP + -0x7a], [BP + -0x78], 0x3c )
1cc7 ~ui_wait_with_highlight ( 0x10c="font", 0xc1 )
1ce7 CALL 3a41:10e3 ( CS, BL, b?:[BX + 0x225a] )
1cfc ~manager_ai_weekly ( SI, 0x1 )
```

### 3a41:214c screen_league_table

```
2174 crt_fstrcpy ( &[BP + -0x6a], LBL[79]"Tabelle ", SI, DI )
2198 crt_fstrcat ( &[BP + -0x6a], ?:[BX + 0x2388], ?:[BX + 0x238a] )
21ac copy_protection_penalty ( &[BP + -0x6a], 0, 0, 0 )
21c5 ui_set_font ( 0x1 )
21f1 ui_draw_text_centered_shadow ( &[BP + -0x6a], 0, 0x10c="font", 0xdc, 0xb, 0 )
2223 ui_draw_button ( 0x5, 0x8, 0x134, 0xb4, &[BP + 0xff78], 0, 0x8 )
225b ui_draw_button ( 0x10c="font", 0xc1, 0, &[BP + -0x80], &[BP + -0x80], 0, 0x8 )
2280 ui_draw_line ( 0x7, 0x19, 0x137, 0x19, 0x1 )
22a5 ui_draw_line ( 0x7, 0xa9, 0x137, 0xa9, 0x1 )
22be ui_set_font ( 0x2 )
22ee ui_draw_text_at ( LBL[78]"PL.     VEREIN                           BILANZ         SP       S-U-N      PUNKTE     TORE      TORDIFF.", 0x9, 0x17, 0x1 )
2310 screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0 )
2352 ~ui_register_icon_slot ( SI, 0xae, ?:[BX + 0x4b94], ?:[BX + 0x4b96], 0 )
2387 ui_set_font ( 0x1 )
23b0 ui_fill_rect ( 0x6, 0x9, 0x137, 0x11, 0x8 )
23cb crt_fstrcpy ( &[BP + -0x6a], LBL[79]"Tabelle " )
23f0 crt_fstrcat ( &[BP + -0x6a], ?:[BX + 0x4b94], ?:[BX + 0x4b96] )
2402 crt_fstrcat ( &[BP + -0x6a], 0x5501="   ", DS )
2449 format_long_thousands ( b?:[BX + 0x225a], 0x137 )
2457 crt_fstrcat ( &[BP + -0x6a], b?:[BX + 0x225a], 0x137 )
2475 ~FUN_1310_1fe7 ( &[BP + -0x6a], b?:[BX + 0x225a] )
2490 crt_fstrcat ( &[BP + -0x6a], LBL[80]"SPIELTAG" )
24bd ui_draw_text_centered_shadow ( &[BP + -0x6a], 0x5, 0x139="\", 0x10, 0x1, 0x7 )
24d6 ui_set_font ( 0x2 )
251b CALL 3a41:2d33 ( CS, b[BP + 0x6], 0x1, b[BP + -0x6e] )
255b ui_fill_rect ( 0x9, 0x1a, 0x137, 0xa8, 0x8 )
258a ui_fill_rect ( 0x9, 0x1c, 0x13, 0x22, 0xf )
25be ui_fill_rect ( 0x9, 0x23, 0x13, 0x7, 0xc )
2615 ui_fill_rect ( 0x9, b?:[BX + 0x2266], 0x13, b?:[BX + 0x225e], 0x11 )
2653 ui_fill_rect ( 0x9, 0x1c, 0x13, 0x7, 0xc )
269f format_long_thousands ( &[SI + 0x1], 0x137 )
26ad crt_fstrcpy ( &[BP + -0x6a], &[SI + 0x1], 0x137 )
26bf crt_fstrcat ( &[BP + -0x6a], 0x5505=".", DS )
26e6 ui_draw_text_at ( &[BP + -0x6a], 0x9, [BP + 0xff70], 0x1d )
2702 crt_fstrcpy ( &[BP + -0x6a], 0x22, 0x5238 )
2729 ui_draw_text_at ( &[BP + -0x6a], 0x15, [BP + 0xff70], [BP + 0xff6e] )
274b crt_fstrcpy ( &[BP + -0x6a], 0x36, 0x5238 )
275d crt_fstrcat ( &[BP + -0x6a], 0x5507="   ", DS )
27af format_long_thousands ( [BP + -0x70], 0x5238 )
27bd crt_fstrcat ( &[BP + -0x6a], [BP + -0x70], 0x5238 )
27cf crt_fstrcat ( &[BP + -0x6a], 0x550b="     ", DS )
2819 format_long_thousands ( DI, 0 )
2827 crt_fstrcat ( &[BP + -0x6a], DI, 0 )
2839 crt_fstrcat ( &[BP + -0x6a], 0x5511=" ", DS )
2884 format_long_thousands ( [BP + -0x70], 0 )
2892 crt_fstrcat ( &[BP + -0x6a], [BP + -0x70], 0 )
28a4 crt_fstrcat ( &[BP + -0x6a], 0x5513=" ", DS )
28b1 format_long_thousands ( DI, 0 )
28bf crt_fstrcat ( &[BP + -0x6a], DI, 0 )
28d1 crt_fstrcat ( &[BP + -0x6a], 0x5515="    ", DS )
292e crt_fstrcat ( &[BP + -0x6a], 0x551a="     ", DS )
293c format_long_thousands ( [BP + -0x70], 0 )
294a crt_fstrcat ( &[BP + -0x6a], [BP + -0x70], 0 )
2968 crt_fstrcat ( &[BP + -0x6a], 0x5520=":", DS )
29c3 format_long_thousands ( DI, 0 )
29d7 crt_fstrcat ( &[BP + -0x6a], 0x5522=" ", DS, 0 )
29e9 crt_fstrcat ( &[BP + -0x6a], 0x5524="    ", DS )
2a40 format_long_thousands ( [BP + -0x70], 0 )
2a4e crt_fstrcat ( &[BP + -0x6a], [BP + -0x70], 0 )
2a60 crt_fstrcat ( &[BP + -0x6a], 0x5529=":", DS )
2aa7 format_long_thousands ( DI, 0 )
2ab5 crt_fstrcat ( &[BP + -0x6a], DI, 0 )
2ac7 crt_fstrcat ( &[BP + -0x6a], 0x552b="      ", DS )
2afe crt_fstrcat ( &[BP + -0x6a], 0x5536="-", DS )
2b10 ~long_abs_diff ( 0, 0, [BP + -0x70], 0 )
2b1a format_long_thousands ( 0, 0 )
2b28 crt_fstrcat ( &[BP + -0x6a], 0, 0 )
2b4f ui_draw_text_at ( &[BP + -0x6a], 0x67, [BP + 0xff70], [BP + 0xff6e] )
2bb3 screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0x1 )
2bc8 ui_hit_test_buttons ( &[BP + 0xff78], 0x2, 0x1 )
2caa CALL 3a41:000e ( CS, b?:[BX + 0x535a], b[BX + 0x5538], 0, b[BP + 0xff74] )
2ccc ~ui_wait_with_highlight ( 0x10c="font", 0xc1 )
2d0e CALL 3a41:2d33 ( CS, b[BP + 0x6], 0x1, 0x1 )
```

### 3a41:3891 screen_strength_table

```
38b3 copy_protection_penalty ( LBL[113]"St{rketabelle", 0, 0, 0, SI, DI )
38e8 ui_draw_button ( 0x3d, 0x11, 0xbb, 0xb2, &[BP + 0xff50], 0, 0x8 )
3919 ui_draw_button ( 0x10c="font", 0xc1, 0, 0x22, &[BP + 0xff58], 0, 0x8 )
393e ui_draw_line ( 0x3f, 0x23, 0xf6, 0x23, 0x1 )
3963 ui_draw_line ( 0x3f, 0xb2, 0xf6, 0xb2, 0x1 )
397c ui_set_font ( 0x1 )
3998 crt_fstrcpy ( &[BP + 0xff6e], LBL[86]"ST[RKEN " )
39c1 crt_fstrcat ( &[BP + 0xff6e], ?:[BX + 0x2388], ?:[BX + 0x238a] )
39f3 ui_draw_text_centered_shadow ( &[BP + 0xff6e], 0x3d, 0xf8, 0x19, 0x1, 0x7 )
3a0c ui_set_font ( 0x2 )
3a3c ui_draw_text_at ( LBL[81]"PL.     VEREIN               KONDITION   TECHNIK    FORM", 0x41, 0x21, 0x1 )
3a5c crt_fstrcat ( &[BP + 0xff6e], ?:[BX + 0x2388], ?:[BX + 0x238a] )
3a9b ~ui_register_icon_slot ( 0x85, 0xb5, ?:[BX + 0x4bb0], ?:[BX + 0x4bb2], 0x12 )
3ac4 ui_fill_rect ( 0x3e, 0x24, 0xf6, 0xb0, 0x8 )
3c33 format_long_thousands ( &[SI + 0x1], [BX] )
3c42 crt_fstrcpy ( &[BP + 0xff6e], &[SI + 0x1], [BX] )
3c55 crt_fstrcat ( &[BP + 0xff6e], 0x553b=". ", DS )
3c72 crt_fstrcat ( &[BP + 0xff6e], 0x22, 0x5238 )
3c9f ui_draw_text_at ( &[BP + 0xff6e], 0x41, &[SI + 0x6], [BP + 0xff32] )
3d0b ~clamp_int ( &[BP + 0xff6a], 0, 0x63 )
3d1a format_long_thousands ( [BP + 0xff6a], [BX] )
3d29 crt_fstrcpy ( &[BP + 0xff6e], [BP + 0xff6a], [BX] )
3d3c crt_fstrcat ( &[BP + 0xff6e], 0x553e="            ", DS )
3d96 ~clamp_int ( &[BP + 0xff6a], 0, 0x63 )
3da5 format_long_thousands ( [BP + 0xff6a], [BX] )
3db4 crt_fstrcat ( &[BP + 0xff6e], [BP + 0xff6a], [BX] )
3dc7 crt_fstrcat ( &[BP + 0xff6e], 0x554b="            ", DS )
3e21 ~clamp_int ( &[BP + 0xff6a], 0, 0x63 )
3e30 format_long_thousands ( [BP + 0xff6a], [BX] )
3e3f crt_fstrcat ( &[BP + 0xff6e], [BP + 0xff6a], [BX] )
3e6c ui_draw_text_at ( &[BP + 0xff6e], 0xa2, &[SI + 0x6], [BP + 0xff32] )
3ec0 screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0x1 )
3ed5 ui_hit_test_buttons ( &[BP + 0xff50], 0x2, 0x1 )
3f7a CALL 3a41:000e ( CS, b?:[BX + 0x226e], 0x2, 0, 0 )
3f90 ~ui_wait_with_highlight ( 0x10c="font", 0xc1 )
```

### 24a4:0906 screen_player_info

(In these traces `4a01:0dd2` is `strcat` and `4a01:0e18` is `strcpy`; the labels below have them
swapped.)

```
095f ui_draw_button ( 0x59, 0x37, 0xe0, 0xae, &[BP + 0xff7a], 0x1, 0x8, SI, DI )
097b ui_set_font ( 0x1 )
0996 crt_fstrcpy ( &[BP + -0x50], LBL[43]"Info ]ber " )
09b4 crt_fstrcat ( &[BP + -0x50], 0x25, 0x5238 )
09e1 ui_draw_text_centered_shadow ( &[BP + -0x50], 0x59, 0x139="\", 0x3f, 0xb, 0 )
0a06 ui_draw_line ( 0x5a, 0x41, 0x138, 0x41, 0xb )
0a29 ui_set_font ( 0x2 )
0a4a ~ui_register_icon_slot ( 0xac, 0xd6, UI[209]"AHA !", 0xa )
0a68 crt_fstrcpy ( &[BP + -0x50], 0x25, 0x5238 )
0a83 crt_fstrcat ( &[BP + -0x50], UI[210]" IST " )
0aa5 format_long_thousands ( 0, 0 )
0ab3 crt_fstrcat ( &[BP + -0x50], 0x25, 0 )
0ace crt_fstrcat ( &[BP + -0x50], UI[327]" JAHRE ALT, " )
0b1f crt_fstrcat ( &[BP + -0x50], ?:[BX + 0x2608], ?:[BX + 0x260a] )
0b46 ui_draw_text_at ( &[BP + -0x50], &[SI + 0x3], &[DI + 0x12], 0x1 )
0b64 crt_fstrcpy ( &[BP + -0x50], UI[212]"STATUS: " )
0b9a crt_fstrcat ( &[BP + -0x50], UI[213]"IST F]R'S N[CHSTE SPIEL EINGEPLANT (NR." )
0baf format_long_thousands ( 0, 0 )
0bbd crt_fstrcat ( &[BP + -0x50], 0, 0 )
0c30 crt_fstrcat ( &[BP + -0x50], UI[216]"NOCH ", UI[215]"KANN SPIELEN, DARF ABER SCHEINBAR NICHT...", UI[214]"H}TET DIE ERSATZBANK" )
0c45 format_long_thousands ( 0, 0 )
0c53 crt_fstrcat ( &[BP + -0x50], 0, 0 )
0c6e crt_fstrcat ( &[BP + -0x50], UI[217]" SPIELE " )
0c95 crt_fstrcat ( &[BP + -0x50], ES:[BX], ?:[BX + 0x2] )
0cae crt_fstrcat ( &[BP + -0x50], 0x5318="(", DS )
0cd6 ~text_to_upper_de ( &[BP + -0x7e], ?:[BX + 0x23b4], ?:[BX + 0x23b6] )
0ce8 crt_fstrcat ( &[BP + -0x50], &[BP + -0x7e], SS )
0cfa crt_fstrcat ( &[BP + -0x50], 0x531a=")", DS )
0d1e ui_draw_text_at ( &[BP + -0x50], &[SI + 0x3], 0x37, 0x1 )
0d6c ui_draw_text_centered_shadow ( ?:[BX + 0x25d8], ?:[BX + 0x25da], DX, [BX + 0x532c], 0x37, 0x1, 0 )
0db1 ui_draw_text_at ( UI[218]"TORE:", &[SI + 0x3], 0x37, 0x1 )
0dda ~FUN_362f_0a9f ( 0, 0 )
0de8 crt_fstrcpy ( &[BP + -0x50], BX, 0 )
0dfa crt_fstrcat ( &[BP + -0x50], 0x531c="(", DS )
0e1c ~FUN_362f_0a9f ( ?:[BX + 0x22], 0 )
0e2a crt_fstrcat ( &[BP + -0x50], BX, 0 )
0e3c crt_fstrcat ( &[BP + -0x50], 0x531e=")", DS )
0e68 ui_draw_text_at ( &[BP + -0x50], [BX + 0x532c], 0x37, 0x1 )
0ea6 ui_draw_text_at ( UI[219]"SPIELE:", &[SI + 0x3], 0x37, 0x1 )
0ecf ~FUN_362f_0a9f ( 0, 0 )
0edd crt_fstrcpy ( &[BP + -0x50], BX, 0 )
0eef crt_fstrcat ( &[BP + -0x50], 0x5320="(", DS )
0f11 ~FUN_362f_0a9f ( ?:[BX + 0x1c], 0 )
0f1f crt_fstrcat ( &[BP + -0x50], BX, 0 )
0f31 crt_fstrcat ( &[BP + -0x50], 0x5322=")", DS )
0f5d ui_draw_text_at ( &[BP + -0x50], [BX + 0x532c], 0x37, 0x1 )
0f9b ui_draw_text_at ( UI[220]"DATEN:", &[SI + 0x3], 0x37, 0x1 )
1072 ~FUN_362f_0a9f ( [BP + 0xff74], [BP + 0xff76] )
1080 crt_fstrcat ( &[BP + -0x50], &[SI + 0x3], 0 )
10de crt_fstrcpy ( &[BP + -0x50], ?:[BX + 0x2618], ?:[BX + 0x261a] )
1116 crt_fstrcat ( &[BP + -0x50], UI[221]" JAHR", LBL[319]" DM" )
1141 crt_fstrcat ( &[BP + -0x50], UI[222]"E" )
1176 ui_draw_text_centered_shadow ( &[BP + -0x50], [BP + 0xff72], [BP + 0xff72], [BP + 0xff70], 0x1, 0xff )
11f1 ui_draw_text_centered_shadow ( ?:[BX + 0x25e4], ?:[BX + 0x25e6], [BP + 0xff72], [BX + 0x532c], 0, 0x1, 0 )
1223 crt_fstrcat ( &[BP + -0x50], LBL[324]"" )
125c ui_draw_text_at ( UI[223]"TENDENZ", 0x7f, 0x37, 0x1 )
1285 ui_fill_rect ( 0x5f, 0xb4, 0xc1, 0xbe, 0x1 )
12ae ui_fill_rect ( 0x60, 0xb5, 0xc0, 0xbd, 0 )
12f2 ui_fill_rect ( CX, 0xb7, 0, 0xbb, b[BX] )
131e crt_afldiv ( 0, 0x1, 0x26, 0x1 )
1361 ui_draw_line ( b[BP + 0xff6e], 0xb5, b[BP + 0xff6e], 0xbd, b[BX] )
138e ui_draw_text_at ( UI[224]"ERSCH|PFUNG", 0xea, 0x37, 0x1 )
13b7 ui_fill_rect ( 0xd0, 0xb4, 0x132, 0xbe, 0x1 )
13e0 ui_fill_rect ( 0xd1, 0xb5, 0x131, 0xbd, 0 )
141d ui_fill_rect ( AX, 0xb7, 0xc, 0xbb, b[BX + 0x5324] )
1449 crt_afldiv ( 0, 0x1, 0x64, 0x1 )
147f ui_draw_line ( 0, 0xb5, 0, 0xbd, b[BX + 0x5324] )
1495 input_poll_mouse ( FAR:[0x6dc0], FAR:[0x6dc2] )
149c ~ui_hit_icon_slot (  )
14bd ui_redraw_buttons ( b[BP + -0x56], 0 )
```

### 24a4:1ad5 screen_top_scorers

```
1b56 ~squad_count_by_flag ( SI, 0, SI, DI )
1c22 copy_protection_penalty ( LBL[110]"Torsch}tzenliste", 0, 0, 0 )
1c54 ui_draw_button ( 0x23, 0x8, 0xfa, 0xb5, &[BP + 0xff72], 0, 0x8 )
1c85 ui_draw_button ( 0x10c="font", 0xc1, 0, 0x22, &[BP + 0xff7a], 0, 0x8 )
1ccb ui_draw_line ( 0x24, 0x1a, 0x11c, 0x1a, 0x1 )
1ce4 ui_set_font ( 0x1 )
1cff crt_fstrcpy ( &[BP + -0x74], UI[227]"Die Besten der " )
1d24 crt_fstrcat ( &[BP + -0x74], ?:[BX + 0x2664], ?:[BX + 0x2666] )
1d51 ui_draw_text_centered_shadow ( &[BP + -0x74], 0x23, 0x11d="newbold.font", 0x10, 0x1, 0x7 )
1d6a ui_set_font ( 0x2 )
1d9a ui_draw_text_at ( UI[228]"PL. SPIELER                 VEREIN", 0x26, 0x18, 0x1 )
1dca ui_draw_text_at ( UI[229]"TORE       TORE/SPIEL", 0xd2, 0x18, 0x1 )
1e4e ui_draw_text_centered_shadow ( UI[230]"NOCH KEINE EINTR[GE VORHANDEN...", 0x23, 0x11d="newbold.font", 0x28, 0x1, 0 )
1e85 ~ui_register_icon_slot ( 0x36, 0xaf, UI[16]"LIGA", b[BX + 0x5340] )
1ead ~ui_register_icon_slot ( 0xd4, 0xaf, UI[17]"SPIELER", b[BX] )
1ec6 screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0x1 )
1edb ui_hit_test_buttons ( &[BP + 0xff72], 0x2, 0 )
1ef3 input_poll_mouse ( FAR:[0x6dc0], FAR:[0x6dc2] )
1fbd ~manager_index_of_team ( b[BP + 0xff6e], 0xff )
2047 format_long_thousands ( 0, 0 )
2055 crt_fstrcpy ( &[BP + -0x74], 0, 0 )
2067 crt_fstrcat ( &[BP + -0x74], 0x533b=". ", DS )
2088 crt_fstrcat ( &[BP + -0x74], 0x25, 0x5238 )
20b4 ui_draw_text_at ( &[BP + -0x74], 0x26, [BP + 0xfece], b[BP + -0xe] )
20e4 ui_draw_text_at ( 0x22, 0x5238, 0x72, [BP + 0xfece], [BP + 0xfec8] )
210b format_long_thousands ( 0, 0, 0xd4, [BP + 0xfece], [BP + 0xfec8] )
2122 ui_draw_text_at ( 0, 0 )
218a format_long_thousands ( 0xa, 0 )
2198 crt_fstrcpy ( &[BP + -0x74], 0xa, 0 )
21aa crt_fstrcat ( &[BP + -0x74], 0x533e=".", DS )
21bf format_long_thousands ( DX, 0 )
21cd crt_fstrcat ( &[BP + -0x74], DX, 0 )
21ff ui_draw_text_at ( &[BP + -0x74], 0xfc, [BP + 0xfece], b[BP + -0xe] )
2237 rastport_set_color ( 0x1 )
225b rastport_put_pixel ( 0x2d, [BP + 0xfece] )
22d9 ~squad_count_by_flag ( b[BP + 0xfecc], 0 )
231b CALL 24a4:0906 ( CS, b[BP + 0xfece] )
2368 ~ui_hit_icon_slot (  )
23a4 ui_fill_rect ( 0x24, 0x9, 0x11b="t", 0xbb, 0x8 )
23ca ~ui_wait_with_highlight ( 0x10c="font", 0xc1 )
```

### 362f:01b8 screen_settings

```
01e0 ~vga_load_picture ( 0x21, 0, 0, 0, 0x1, SI, DI )
0214 copy_protection_penalty ( LBL[116]"Einstellungen", 0, 0, 0 )
0242 ui_draw_button ( 0xd, 0xd, 0x125="font", 0xa8, &[BP + -0x7c], 0, 0x8 )
0272 ui_draw_button ( 0x10c="font", 0xc1, 0, 0x22, &[BP + -0x74], 0, 0x8 )
028b ui_set_font ( 0x2 )
02a6 crt_fstrcpy ( &[BP + -0x56], W[3]"V2.0 - ENDE: " )
02c3 format_long_thousands ( FAR:[0x513c], 0 )
02e1 crt_fstrcat ( &[BP + -0x56], UI[416]" NIE.", FAR:[0x513c], 0 )
02fc crt_fstrcat ( &[BP + -0x56], W[4]" (LEVEL " )
0315 format_long_thousands ( DS:[0x4a28], 0 )
0323 crt_fstrcat ( &[BP + -0x56], DS:[0x4a28], 0 )
0335 crt_fstrcat ( &[BP + -0x56], 0x5452=")", DS )
035c ui_draw_text_at ( &[BP + -0x56], 0x10, 0x17, 0x3 )
0375 ui_set_font ( 0x1 )
03ab ui_draw_text_centered_shadow ( UI[417]"Halbzeitst{nde", 0x16, 0x79, 0x2e, 0x1d, 0 )
03e1 ui_draw_text_centered_shadow ( UI[418]"Ergebnisse", 0x16, 0x79, 0x40, 0x1d, 0 )
0417 ui_draw_text_centered_shadow ( UI[419]"Tabelle", 0x16, 0x79, 0x53, 0x1d, 0 )
044d ui_draw_text_centered_shadow ( UI[420]"Torszenen", 0x16, 0x79, 0x6f, 0x1d, 0 )
0480 ui_draw_text_centered_shadow ( UI[127]"DfB-Pokal", 0x16, 0x79, 0x82, 0x1d, 0 )
04b3 ui_draw_text_centered_shadow ( UI[421]"Europapokale", 0x16, 0x79, 0x94, 0x1d, 0 )
04e6 ui_draw_text_centered_shadow ( UI[422]"Nachholspiele", 0x16, 0x79, 0xa8, 0x1d, 0 )
051c ui_draw_text_centered_shadow ( UI[423]"Zeitung", 0xac, 0xed, 0x6f, 0x1d, 0 )
0552 ui_draw_text_centered_shadow ( UI[424]"Blenden", 0xac, 0xed, 0x82, 0x1d, 0 )
0582 ui_draw_text_at ( UI[425]"1.Liga", 0x7d, 0x1d, 0x1 )
05b2 ui_draw_text_at ( UI[426]"2.Liga", 0xb8, 0x1d, 0x1 )
05e2 ui_draw_text_at ( UI[427]"3.Liga", 0xf2, 0x1d, 0x1 )
0600 rastport_move_to ( 0x10, 0x11 )
061e rastport_line_to ( 0x130, 0x11 )
0654 ui_draw_text_centered_shadow ( UI[428]"Spielgeschwindigkeit", 0xb6, 0x12c, 0x96, 0x1d, 0 )
0672 rastport_move_to ( 0x10, 0x62 )
0690 rastport_line_to ( 0x130, 0x62 )
06ae rastport_move_to ( 0x10, 0xb4 )
06cc rastport_line_to ( 0x130, 0xb4 )
0701 CALL 362f:0004 ( CS, [BP + -0x5c], [BP + -0x5a], 0, [BX + 0x5464], b?:[BX + 0x5fe] )
073b blit_bitmap ( [BP + -0x5c], [BP + -0x5a], 0, 0, 0xbe, 0x9b, 0x64, 0x13, 0xc0 )
074d CALL 362f:0086 ( CS, DS:[0x63a] )
0762 screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0x1 )
0778 input_poll_mouse ( FAR:[0x6dc0], FAR:[0x6dc2] )
0795 dos_direct_console_io (  )
07b1 rastport_get_pixel ( [BP + -0x62], [BP + -0x6c] )
0847 ui_draw_frame ( 0, 0, 0x13f, 0xff, 0x1 )
0858 dos_direct_console_io (  )
097c ui_draw_frame ( 0, 0, 0x13f, 0xff, 0 )
099f ui_hit_test_buttons ( &[BP + -0x7c], 0x2, 0 )
09cf CALL 362f:0004 ( CS, [BP + -0x5c], [BP + -0x5a], [BP + 0xff74], [BP + 0xff76], 0 )
0a6f CALL 362f:0086 ( CS, [BP + -0x62] )
0a85 ~ui_wait_with_highlight ( 0x10c="font", 0xc1 )
0a92 bitmap_free ( [BP + -0x80], [BP + -0x7e] )
```

### 362f:0af7 screen_statistics

```
0b27 copy_protection_penalty ( LBL[117]"Statistik", 0, 0, 0, SI, DI )
0b5d ui_draw_button ( 0x5, 0x8, 0xc3, 0xb4, &[BP + 0xff60], 0, 0x8 )
0b8f ui_draw_button ( 0xcd, 0x8, 0x6c, 0xb4, &[BP + 0xff60], 0, 0x8 )
0bbe ui_draw_button ( 0x5, 0xc0, 0xc3, 0x27, &[BP + 0xff60], 0, 0x8 )
0bef ui_draw_button ( 0x10c="font", 0xc1, 0, 0x22, &[BP + 0xff60], 0, 0x8 )
0c1a ~finance_sum_entries ( 0, 0x63 )
0c7c palette_set_entry ( FAR:[0x2e70], FAR:[0x2e72], 0x1f, DH, [BX + 0x5498], b[BX + 0x5498] )
0cc8 ui_draw_text_at ( ?:[BX + 0x2448], ?:[BX + 0x244a], [BP + 0xff6c], [BP + 0xff5e], 0x1 )
0d0d crt_fstrcpy ( &[BP + -0x48], 0x5488="  ", DS )
0d30 CALL 362f:21e1 ( CS, &[BP + 0xff70], b[BP + 0xff6e], b[BP + 0xff54], 0 )
0d52 CALL 362f:21e1 ( CS, &[BP + 0xff70], b[BP + 0xff6e], b[BP + 0xff54], 0x1 )
0d63 crt_fstrcat ( &[BP + -0x48], &[BP + 0xff70], SS )
0d7c crt_fstrcat ( &[BP + -0x48], 0x548b="(", DS )
0db9 CALL 362f:0a9f ( CS, b?:[BX + 0xa273], 0 )
0dc5 crt_fstrcat ( &[BP + -0x48], b?:[BX + 0xa273], 0 )
0dd7 crt_fstrcat ( &[BP + -0x48], 0x548d=")", DS )
0e04 ui_draw_text_at ( &[BP + -0x48], 0x22, [BP + 0xff5e], 0x2 )
0e5c CALL 362f:0a9f ( CS, 0, 0 )
0e68 crt_fstrcpy ( &[BP + -0x48], 0, 0 )
0e81 ~list_no_entries ( b[BP + 0xff6e], b[BP + 0xff5c], &[BP + -0x48], SS )
0ec8 ui_draw_text_at ( &[BP + -0x48], 0, [BP + 0xff5e], 0x2 )
11fc format_long_thousands ( [BP + 0xff4e], [BP + 0xff50] )
120b crt_fstrcpy ( &[BP + 0xff70], [BP + 0xff50], 0 )
1233 crt_fstrlen ( ?:[BX + 0x2448], ?:[BX + 0x244a], [BP + 0xff5e], 0x2 )
1257 rastport_text_width ( ?:[SI + 0x2448], ?:[SI + 0x244a], 0x2 )
1278 ui_draw_text_at ( &[BP + 0xff70], 0x2 )
12c8 crt_fstrcat ( &[BP + -0x48], LBL[324]"", [BP + 0xff6c], [BP + 0xff5e], 0x2 )
12ef ~FUN_24a4_2636 ( DS:[0x224e], DS:[0x2250] )
1304 crt_aflmul ( DS:[0x2252], DS:[0x2254], [BP + 0xff5c], 0 )
130b crt_afldiv ( [BP + 0xff5c], 0 )
1312 format_long_thousands ( [BP + 0xff5c], 0 )
1320 crt_fstrcat ( &[BP + -0x48], [BP + 0xff5c], 0 )
133b crt_fstrcat ( &[BP + -0x48], LBL[319]" DM" )
1362 ui_draw_text_at ( &[BP + -0x48], [BP + 0xff6c], [BP + 0xff5e], 0x2 )
13a4 ~FUN_24a4_2822 ( DS:[0x2252], DS:[0x2254], DS:[0x224e], DS:[0x2250] )
1435 ui_set_font ( 0x1, &[BP + -0x48], 0 )
1470 ui_draw_text_at ( ?:[SI + 0x2448], ?:[SI + 0x244a], [BP + 0xff6c], [BP + 0xff5e], 0xb )
1492 crt_fstrlen ( ?:[SI + 0x2448], ?:[SI + 0x244a], b[BP + 0xff5e], 0xb )
14ba rastport_text_width ( ?:[SI + 0x2448], ?:[SI + 0x244a], b[BP + 0xff5e] )
14de ui_draw_line ( [BP + 0xff6c], b[BP + 0xff4c], ?:[SI + 0x2448] )
14f7 ui_set_font ( 0x2 )
151e ui_set_font ( 0x1 )
154e ui_draw_text_at ( LBL[0]"G       H       A", 0x5c, 0x14, 0xb )
1567 ui_set_font ( 0x2 )
1590 ui_fill_rect ( 0xd1, 0x69, 0x130, 0x76, 0 )
15b9 ui_draw_frame ( 0xd0, 0x68, 0x131, 0x77, 0x6 )
15e2 ui_fill_rect ( 0xd1, 0xab, 0x130, 0xb8, 0 )
160b ui_draw_frame ( 0xd0, 0xaa, 0x131, 0xb9, 0x6 )
170a ui_draw_frame ( AX, 0, 0x5, 0x76, 0x15 )
173d ui_fill_rect ( [BP + 0xff5c], 0x77, BX, 0x76, 0x11 )
17e9 ui_draw_line ( [BP + 0xff6c], 0xae, [BP + 0xff6c], 0xb5, b[BP + 0xff5c], [BP + 0xff6c], 0xb8, b[BP + 0xff5c] )
1854 ui_draw_line ( 0xd1, 0xab, 0xd1, 0xb8, 0x13 )
1889 ui_draw_line ( 0x130, 0xab, 0x130, 0xb8, 0x1f )
18a2 screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0x1 )
18b7 ui_hit_test_buttons ( &[BP + 0xff60], 0x1, 0x1 )
18d0 ~ui_wait_with_highlight ( 0x10c="font", 0xc1 )
```

### 362f:18e7 screen_eternal_table

```
1988 ~vga_load_picture ( 0x24, 0, 0, 0, 0x1, SI, DI )
19ad copy_protection_penalty ( LBL[118]"Ewige Tabelle/Bilanz", 0, 0, 0 )
19df ui_draw_button ( 0x5, 0x8, 0x87, 0xde, &[BP + 0xff10], 0, 0x8 )
1a11 ui_draw_button ( 0x91, 0x8, 0xa8, 0xb4, &[BP + 0xff10], 0, 0x8 )
1a42 ui_draw_button ( 0x10c="font", 0xc1, 0, 0x22, &[BP + 0xff10], 0, 0x8 )
1a5b ui_set_font ( 0x1 )
1a91 ui_draw_text_centered_shadow ( LBL[11]"Ewige Tabelle", 0x5, 0x8c, 0x11, 0xb, 0xff )
1ab6 ui_draw_line ( 0x6, 0x13, 0x8b, 0x13, 0xb )
1aec ui_draw_text_centered_shadow ( LBL[12]"Ewige Bilanz", 0x91, 0x139="\", 0x11, 0xb, 0xff )
1b11 ui_draw_line ( 0x92, 0x13, 0x138, 0x13, 0xb )
1b49 blit_bitmap ( ES:[BX], ?:[BX + 0x2], 0, 0, 0x9b, 0x16, 0x15, 0x71, 0xc0 )
1b81 blit_bitmap ( ES:[BX], ?:[BX + 0x2], 0, 0, 0x119="ont", 0x16, 0x15, 0x71, 0xc0 )
1b9a ui_set_font ( 0x2 )
1bbb crt_fstrcpy ( &[BP + 0xff6a], LBL[13]" KOMMT VIELLEICHT NOCH..." )
1bf5 ui_draw_text_centered_shadow ( &[BP + 0xff6a], 0xb0, 0x119="ont", SI, b[BP + -0x46], 0 )
1c32 ui_draw_text_at ( ?:[BX + 0x4a80], ?:[BX + 0x4a82], 0x94, SI, 0x1 )
1c7e ui_draw_text_centered_shadow ( ?:[BX + 0x4a6c], ?:[BX + 0x4a6e], 0xb0, 0x11e="ewbold.font", SI, 0x1, 0 )
1cb6 format_long_thousands ( 0, 0 )
1cc5 crt_fstrcpy ( &[BP + 0xff6a], 0, 0 )
1cf8 ui_draw_text_at ( LBL[14]"GESAMT           HEIM         AUSW[RTS", 0xb7, 0x93, 0x1 )
1da1 format_long_thousands ( ?:[BX + 0x2], 0 )
1db0 crt_fstrcpy ( &[BP + 0xff6a], 0, 0 )
1dd8 crt_fstrcat ( &[BP + 0xff6a], 0x54b5=":", DS )
1de8 format_long_thousands ( [BP + 0xff0a], 0 )
1df7 crt_fstrcat ( &[BP + 0xff6a], [BP + 0xff0a], 0 )
1e84 ui_draw_text_at ( &[BP + 0xff6a], 0x30, SI, 0x1, SI, 0x1 )
1f6d format_long_thousands ( &[SI + 0x1], 0 )
1f7c crt_fstrcpy ( &[BP + 0xff6a], &[SI + 0x1], 0 )
1f8f crt_fstrcat ( &[BP + 0xff6a], 0x54b7=". ", DS )
1fb5 crt_fstrcat ( &[BP + 0xff6a], 0, 0x5238 )
1fdf ui_draw_text_at ( &[BP + 0xff6a], 0x9, 0x1b, b[BP + -0x46] )
2019 crt_afulrem ( ?:[BX + 0xefe], ?:[BX + 0xf00], 0x86a0=" SEIN, DER MANNSCHAFT GELANG ES, AUS EINEM %2:%3 NOCH EIN %0:%1 ZU MACHEN.", 0x1, 0x6f, 0x1b, [BP + 0xfefc] )
2020 format_long_thousands ( 0x36, 0x1 )
2037 ui_draw_text_at ( 0x36, 0x1 )
206e rastport_set_color ( 0x1 )
2095 rastport_put_pixel ( 0xd, &[DI + -0x13] )
20e0 bitmap_free ( [BP + 0xff04], [BP + 0xff06] )
215d rastport_set_color ( b[BP + -0x46] )
2186 draw_circle ( 0xa5, SI, 0xb )
21a8 screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0x1 )
21bd ui_hit_test_buttons ( &[BP + 0xff10], 0x1, 0x1 )
21d4 ~ui_wait_with_highlight ( 0x10c="font", 0xc1 )
```

### 362f:390e screen_manager_history

```
394e copy_protection_penalty ( LBL[119]"Damals... Ihre Erfolge", 0, 0, 0, SI, DI )
397e ui_draw_button ( 0x5, 0x9d, 0x102=" : ", 0x49, &[BP + 0xff52], 0, 0 )
39b0 ui_draw_button ( 0x5, 0x8, 0x134, 0x8f, &[BP + 0xff52], 0, 0x8 )
39e1 ui_draw_button ( 0x10c="font", 0xc1, 0, 0x22, &[BP + 0xff5a], 0, 0x8 )
39fa ui_set_font ( 0x1 )
3a4d ui_draw_text_at ( LBL[19]"Sais. Pl.   Liga       DfB-Pokal       Europapokal", 0, 0x11, 0xb )
3a72 ui_draw_line ( 0x5, 0x13, 0x138, 0x13, 0xb )
3aa8 ui_draw_text_centered_shadow ( LBL[20]"Gesamtentwicklung", 0x6, 0x106, 0xa5, 0xb, 0xff )
3acd ui_draw_line ( 0x6, 0xa7, 0x106, 0xa7, 0xb )
3b5c ui_draw_line ( AX, AX, 0x5, b[BP + 0xff50], b[BP + -0x6] )
3bac ui_set_font ( 0x2 )
3c16 ui_draw_text_centered_shadow ( LBL[21]"NOCH HABEN SIE KEINE HISTORISCH SONDERLICH WICHTIGEN ERFOLGE ERZIELT...", 0, 0x140, 0x1d, 0x1, 0x7 )
3c4c format_long_thousands ( 0x1c, FAR:[0xa7a2], 0xc, [BP + 0xff50], 0x1 )
3c63 ui_draw_text_at ( 0x1c, FAR:[0xa7a2] )
3cac ui_draw_text_at ( LBL[22]"EXISTIERTEN SIE NOCH GAR NICHT ALS MANAGER...", 0x2a, [BP + 0xff50], 0x1 )
3cf9 format_long_thousands ( 0, ES )
3d07 crt_fstrcpy ( &[BP + -0x56], 0, ES )
3d19 crt_fstrcat ( &[BP + -0x56], 0x54d2=".", DS )
3d5d ui_draw_text_at ( &[BP + -0x56], 0x2a, [BP + 0xff50], 0 )
3d9c crt_fstrcpy ( &[BP + 0xff62], ?:[BX + 0x2388], ?:[BX + 0x238a] )
3daf ~text_to_upper_de ( &[BP + -0x56], &[BP + 0xff62], SS )
3dd9 ui_draw_text_centered_shadow ( &[BP + -0x56], 0x55, 0x55, [BP + 0xff50], 0x1, 0xff )
3e02 crt_fstrcpy ( &[BP + -0x56], LBL[23]"SIEGER" )
3e33 crt_fstrcpy ( &[BP + 0xff62], ?:[BX + 0x24c0], ?:[BX + 0x24c2] )
3e46 ~text_to_upper_de ( &[BP + -0x56], &[BP + 0xff62], SS )
3e75 ui_draw_text_centered_shadow ( &[BP + -0x56], 0x90, 0x90, [BP + 0xff50], b[BP + -0x6], 0xff )
3eb0 crt_fstrcpy ( &[BP + 0xff62], LBL[24]"SIEGER " )
3ee6 crt_fstrcpy ( &[BP + 0xff62], ?:[BX + 0x24c4], ?:[BX + 0x24c6] )
3f0d crt_fstrcpy ( &[BP + 0xff62], LBL[25]"nicht im wettbewerb" )
3f3d crt_fstrcpy ( &[BP + -0x56], ?:[BX + 0x24dc], ?:[BX + 0x24de] )
3f93 crt_fstrcat ( &[BP + 0xff62], &[BP + -0x56], SS )
3fa6 ~text_to_upper_de ( &[BP + -0x56], &[BP + 0xff62], SS )
3fd4 ui_draw_text_centered_shadow ( &[BP + -0x56], 0xb4, 0x139="\", [BP + 0xff50], b[BP + -0x6], 0xff )
4006 ui_draw_line ( 0x6, b[BP + 0xff50], 0x138, b[BP + 0xff50], 0x6 )
4038 screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0x1 )
404d ui_hit_test_buttons ( &[BP + 0xff52], 0x2, 0x1 )
408d screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0 )
40b6 ui_fill_rect ( 0x6, 0x18, 0x138, 0x96, 0x8 )
40e9 screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0x1 )
40fe ui_hit_test_buttons ( &[BP + 0xff52], 0x2, 0x1 )
4117 ~ui_wait_with_highlight ( 0x10c="font", 0xc1 )
```

### 362f:2be8 screen_manager_contract

```
2c12 ~vga_load_picture ( 0x1f, 0, 0, 0, 0x1, SI, DI )
2c4d ~vga_load_picture ( 0x20, 0, 0, 0, 0x1 )
2c8c screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0 )
2c9a mouse_pointer_hide ( 0 )
2cb0 ~vga_load_picture ( 0x2e, 0x7530=" ENTGEGEN ZU SETZEN.", 0, 0, 0x1 )
2cb8 mouse_pointer_show (  )
2cce ui_set_font ( 0x2 )
2ce8 CALL 362f:29a8 ( CS, FAR:[0x4b5a], FAR:[0x4b5c], 0 )
2d12 palette_load_rgb4 ( FAR:[0x2e70], FAR:[0x2e72], DS:[0x798], DS:[0x79a], 0x100 )
2d2b screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0x1 )
2e12 blit_bitmap ( FAR:[0x2e76], FAR:[0x2e78], 0x30, 0, 0, 0x5b, 0x30, 0x10, 0xc0 )
2e2a input_poll_mouse ( FAR:[0x6dc0], FAR:[0x6dc2] )
2e86 blit_bitmap ( FAR:[0x2e76], FAR:[0x2e78], 0, 0x2f, 0, 0x5b, 0x30, 0x10, 0xc0 )
2ed2 ui_dialog_box ( LBL[137]"Sie stehen noch", LBL[138]"unter Vertrag" )
3021 CALL 362f:29a8 ( CS, FAR:[0x4b5a], FAR:[0x4b5c], [BP + -0x2] )
3074 blit_bitmap ( FAR:[0x2e76], FAR:[0x2e78], 0x3c, DX, 0xb5, 0x3c, 0x1e, 0x17, 0xc0 )
30ad blit_bitmap ( FAR:[0x2e76], FAR:[0x2e78], 0x3c, 0x17, 0xb5, 0x54, 0x1e, 0x17, 0xc0 )
30ec blit_bitmap ( FAR:[0x2e76], FAR:[0x2e78], 0x60, 0x2f, 0xd9, 0x5b, 0x30, 0x10, 0xc0 )
3128 blit_bitmap ( FAR:[0x2e76], FAR:[0x2e78], 0x60, 0x3f, 0x109="ow.font", 0x5b, 0x30, 0x10, 0xc0 )
31e8 CALL 362f:29a8 ( CS, FAR:[0x4b5a], FAR:[0x4b5c], [BP + -0x2] )
327d ui_fill_rect ( 0xd9, 0x3c, 0x139="\", 0x5a, 0 )
32c4 blit_bitmap ( FAR:[0x2e76], FAR:[0x2e78], 0x60, 0x2f, 0xd9, 0x5b, 0x30, 0x10, 0xc0 )
3300 blit_bitmap ( FAR:[0x2e76], FAR:[0x2e78], 0x60, 0x3f, 0x109="ow.font", 0x5b, 0x30, 0x10, 0xc0 )
3331 ui_fill_rect ( 0x7f, 0x43, 0xa4, SI, 0x9 )
3341 CALL 362f:23e4 ( CS, [BP + -0x2], 0x1d )
3356 CALL 362f:23e4 ( CS, b[BP + -0x26], 0x1c )
33ce ui_fill_rect ( 0x7f, 0x43, 0xa4, SI, 0x9, ?:[SI + 0x670], ?:[SI + 0x672], 0xd9, 0x3e, 0, 0 )
33df CALL 362f:23e4 ( CS, 0xa, 0x1d )
33f4 CALL 362f:23e4 ( CS, b[BP + -0x26], 0x1c )
3438 CALL 362f:2551 ( CS, b?:[BX + 0x0], ?:[SI + 0x66c], ?:[SI + 0x66e], 0xd9, 0x3e, 0, 0 )
34e4 blit_bitmap ( FAR:[0x2e76], FAR:[0x2e78], AX, AX, 0xb5, 0x3c, 0x1e, 0x17, 0xc0 )
351d blit_bitmap ( FAR:[0x2e76], FAR:[0x2e78], AX, 0x17, 0xb5, 0x54, 0x1e, 0x17, 0xc0 )
3541 CALL 362f:248a ( CS, FAR:[0x4b5a], FAR:[0x4b5c], b[BP + -0x6], 0x7f, 0x43 )
3550 CALL 362f:281f ( CS, b[BP + -0x4], b[BP + -0x6] )
35c9 blit_bitmap ( FAR:[0x2e76], FAR:[0x2e78], 0x1e, BX, 0xb5, 0, 0x1e, 0x17, 0xc0 )
35e1 input_poll_mouse ( FAR:[0x6dc0], FAR:[0x6dc2] )
3642 blit_bitmap ( FAR:[0x2e76], FAR:[0x2e78], 0, BX, 0xb5, 0, 0x1e, 0x17, 0xc0 )
3684 CALL 362f:248a ( CS, FAR:[0x4b5a], FAR:[0x4b5c], b[BP + -0x6], 0x7f, 0x43 )
3693 CALL 362f:281f ( CS, b[BP + -0x4], b[BP + -0x6] )
36a6 input_poll_mouse ( FAR:[0x6dc0], FAR:[0x6dc2] )
36ca bitmap_free ( FAR:[0x6dd8], FAR:[0x6dda] )
36df bitmap_free ( FAR:[0x2f14], FAR:[0x2f16] )
```

Notes (port): the two `ui_fill_rect` lines at 3331/33ce fill (0x7f,0x43)-(0xa4,0x60) (`SI`
= 0x60), colour 9; the line at 33ce is a mis-split of the fill and the following
`contract_info_line` arguments. `[BP-2]` = selected tab, `[BP-4]` = kind (100 none, 0 shirt,
1 board), `[BP-6]` = offer shown, `[BP-0x26]` = highlighted tab, `[BP-0x22]` = offer mode. The
`blit_bitmap` at 35c9/3642 draws to `(0xb5, BX*0x18 + 0x3c)`, not `(0xb5, 0)`. Exit at 36b1:
`FAR:2ea2 == 3` (right button).

### 362f:29a8 contract_draw_board

```
29ea CALL 362f:24ed ( CS, [BP + 0x6], [BP + 0x8], 0, SI )
2a1e ui_fill_rect ( AX, 0x13, 0x34, 0x31, 0x9 )
2a6c CALL 362f:248a ( CS, [BP + 0x6], [BP + 0x8], 0, b[BP + -0x56], 0x13 )
2aaf CALL 362f:2551 ( CS, b?:[BX + 0x0], ?:[SI + 0x66c], ?:[SI + 0x66e], 0xf0, 0x80, 0x1, 0 )
2b24 CALL 362f:2551 ( CS, 0, [BP + -0x5a], [BP + -0x58], 0xf0, 0x8e, 0x1, 0 )
2b57 CALL 362f:2551 ( CS, 0, ?:[BX + 0x688], ?:[BX + 0x68a], 0xf0, 0x9c, 0x1, 0 )
2b9e CALL 362f:2551 ( CS, 0, ?:[BX + 0x68c], ?:[BX + 0x68e], 0xf0, 0xb9, 0x1, 0 )
2bdd CALL 362f:2551 ( CS, 0, [BP + -0x54], [BP + -0x52], 0xf0, 0xd2, 0x1, 0 )
```

### 422b:07fe screen_save_game

```
0830 copy_protection_penalty ( LBL[106]"Spielstand Speichern", 0, 0, 0, SI )
0849 ui_set_font ( 0x3 )
0864 ~text_to_upper_de ( &[BP + -0x64], LBL[106]"Spielstand Speichern" )
0890 ui_draw_text_centered_shadow ( &[BP + -0x64], 0, 0x140, 0x14, 0xb, 0 )
08a5 CALL 422b:2bbe ( CS, 0, 0, &[BP + -0x64], 0 )
08c2 crt_fstrcpy ( &[BP + 0xff24], 0x4af6, 0x5238 )
08dc crt_fstrcat ( &[BP + 0xff24], 0x9444="\", DS )
08ef crt_fstrcat ( &[BP + 0xff24], &[BP + -0x64], SS )
0900 file_open ( &[BP + 0xff24], 0 )
0911 file_close ( 0 )
0956 ~message_broadcast ( FAR:[0x304a], LBL[208]"FILE EXISTIERT", LBL[209]"BEREITS.", LBL[210]"]BERSCHREIBEN ?", LBL[218]"ACH BITTE !", LBL[219]"BLO~ NICHT !" )
097b crt_fstrcpy ( &[BP + 0xff24], BX, ES, 0x5238, &[BP + -0x64], SS )
098d file_open ( &[BP + 0xff24], 0x1 )
099e CALL 422b:0006 ( CS )
09b3 crt_fstrlen ( [0x93e0], [0x93e2] )
09c7 file_write ( [BP + 0xff1e], [0x93e0], [0x93e2], 0 )
09dc file_write ( [BP + 0xff1e], 0x93e4=">V2", DS, 0x4 )
09eb random_range ( 0, 0x8fff )
09fb random_range ( 0, 0x8fff )
0a61 far_memcpy_bytes ( 0xa6dc, 0x5238, 0xa6d8, 0x5238, 0xc4 )
0a8c file_write ( [BP + 0xff1e], &[BP + 0xff02], 0x4 )
0a9b random_range ( 0, 0xff )
0abb random_range ( 0, 0xff )
0ad4 file_write ( [BP + 0xff1e], &[BP + 0xff06], 0x1 )
0aec file_write ( [BP + 0xff1e], 0x57cf="DERLAGE#% F]R %t'S MANNSCHAFT.", 0x5238, 0x1 )
0b05 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x60e, 0x5cb3, 0x6 )
0b1c CALL 422b:0047 ( CS, [BP + 0xff1e], 0x618, 0x5cb3, 0x4 )
0b33 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x5350=" (", 0x5238, 0x4 )
0b4a CALL 422b:0047 ( CS, [BP + 0xff1e], 0x304a="h Vorlage von ", 0x5238, 0x1 )
0b61 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x5358, 0x5238, 0x1 )
0b78 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x79e, 0x5cb3, 0x8 )
0b8f CALL 422b:0047 ( CS, [BP + 0xff1e], 0x6ddc="% MANNSCHAFT %x03ERZIELT#ERRUNGEN#ERREICHT#% WURDE.", 0x5238, 0x8e8="s" )
0ba6 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x7ab, 0x5cb3, 0x1 )
0bbd CALL 422b:0047 ( CS, [BP + 0xff1e], 0x7ac, 0x5cb3, 0x1 )
0bd4 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x7ad, 0x5cb3, 0x1 )
0beb CALL 422b:0047 ( CS, [BP + 0xff1e], 0x7ae, 0x5cb3, 0x1 )
0c02 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x7af, 0x5cb3, 0x1 )
0c19 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x7b0, 0x5cb3, 0x1 )
0c30 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x2242="gkeit", 0x5238, 0xc28="unggelenkprellung" )
0c47 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x5714="LAGE#% KAM %a IM SPIEL %x10GEGEN#IN#% %b.", 0x5238, 0x64 )
0c5e CALL 422b:0047 ( CS, [BP + 0xff1e], 0x3066="1 EINLEGEN", 0x5238, 0x1a90=" sind f}r den Europapokal qualifiziert." )
0c75 CALL 422b:0047 ( CS, [BP + 0xff1e], 0xecc="ER SCHNITT", 0x5238, 0xd80="amstag" )
0c8c CALL 422b:0047 ( CS, [BP + 0xff1e], 0x57dd="%t'S MANNSCHAFT.", 0x5238, 0x15d3="en Vertrag nicht^mehr verl{ngern wird.^" )
0ca3 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x774a, 0x5238, 0x1964="onende" )
0cba CALL 422b:0047 ( CS, [BP + 0xff1e], 0x4b5e, 0x5238, 0x3c )
0cd1 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x2eba="MINUTE", 0x5238, 0x4 )
0ce8 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x2e8e="RAGUNG.", 0x5238, 0x4 )
0cff CALL 422b:0047 ( CS, [BP + 0xff1e], 0xa7a0, 0x5238, 0x4 )
0d16 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x7dc, 0x5cb3, 0x4 )
0d2d CALL 422b:0047 ( CS, [BP + 0xff1e], 0x7e0, 0x5cb3, 0x2 )
0d44 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x56fe="INEM SIEG#EINER NIEDERLAGE#% KAM %a IM SPIEL %x10GEGEN#IN#% %b.", 0x5238, 0xa )
0d5b CALL 422b:0047 ( CS, [BP + 0xff1e], 0x57aa="EM UNENTSCHIEDEN#EINEM SIEG#EINER NIEDERLAGE#% F]R %t'S MANNSCHAFT.", 0x5238, 0x12 )
0d71 file_write ( [BP + 0xff1e], 0x56dc="% %b.", 0x5238, 0x1 )
0d8a CALL 422b:0047 ( CS, [BP + 0xff1e], 0x2e6e="INEN EINZIGEN...", 0x5238, 0x2 )
0da1 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x76ca, 0x5238, 0x80 )
0db8 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x21e2="abelle", 0x5238, 0x60 )
0dcf CALL 422b:0047 ( CS, [BP + 0xff1e], 0x8, 0x5238, 0x8 )
0de6 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x1d18="GEHALT PRO MONAT (IN DM)", 0x5238, 0x3 )
0dfd CALL 422b:0047 ( CS, [BP + 0xff1e], 0x535a, 0x5238, 0x3c )
0e14 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x4c7e, 0x5238, 0x80 )
0e2b CALL 422b:0047 ( CS, [BP + 0xff1e], 0x225a, 0x5cb3, 0x3 )
0e42 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x9336, 0x5238, 0x1394="DAUER" )
0e59 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x5fe, 0x5cb3, 0xf )
0e70 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x0, 0x5238, 0x8 )
0e87 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x4b9a, 0x5238, 0x30 )
0e9e CALL 422b:0047 ( CS, [BP + 0xff1e], 0x2ec2="LATZ, ST[RKE ", 0x5238, 0x50 )
0eb5 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x2e, 0x5238, 0x140 )
0ecc CALL 422b:0047 ( CS, [BP + 0xff1e], 0x66c, 0x5cb3, 0x90 )
0ee3 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x4a28, 0x5cb3, 0x1 )
0efa CALL 422b:0047 ( CS, [BP + 0xff1e], 0x513c, 0x5238, 0x2 )
0f11 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x1d34="ER VERTRAG", 0x5238, 0x1e )
0f28 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x5664="SPIEL %a %x10GEGEN#IN#% %b.", 0x5238, 0x78 )
0f3f CALL 422b:0047 ( CS, [BP + 0xff1e], 0x5780, 0x5238, 0x7 )
0f56 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x63a="(", 0x5cb3, 0x2 )
0f6d CALL 422b:0047 ( CS, [BP + 0xff1e], 0x7e2, 0x5cb3, 0x2 )
0f84 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x16e, 0x5238, 0x1 )
0f9b CALL 422b:0047 ( CS, [BP + 0xff1e], 0x2280, 0x5cb3, 0x5f )
0fb2 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x620, 0x5cb3, 0x10 )
0fc9 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x4bce, 0x5238, 0x1 )
0fe0 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x56e2, 0x5238, 0x4 )
1004 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x5fa, ES, 0x4 )
101b CALL 422b:0047 ( CS, [BP + 0xff1e], 0x76c6, 0x5238, 0x4 )
1032 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x90c2, 0x5238, 0x4 )
1049 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x5d6, 0x5cb3, 0x4 )
1060 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x61c, 0x5cb3, 0x4 )
1077 CALL 422b:0047 ( CS, [BP + 0xff1e], 0x5b8, 0x5cb3, 0x4 )
108e CALL 422b:0047 ( CS, [BP + 0xff1e], 0x3060="-DISK 1 EINLEGEN", 0x5238, 0x1 )
10e9 crt_fstrlen ( ?:[BX + 0x5524], ?:[BX + 0x5526] )
1115 CALL 422b:0047 ( CS, [BP + 0xff1e], &[BP + 0xff02], 0x4 )
112a CALL 422b:0047 ( CS, [BP + 0xff1e], &[BP + 0xff06], 0x1 )
1143 CALL 422b:0047 ( CS, [BP + 0xff1e], [BP + 0xfefe], [BP + 0xff00], b[BP + 0xff06] )
117b file_write ( [BP + 0xff1e], 0x56dc="% %b.", 0x5238, 0x1 )
1193 file_write ( [BP + 0xff1e], 0x5f6, 0x5cb3, 0x4 )
11ab file_write ( [BP + 0xff1e], 0x2e7a="N...", 0x5238, 0x4 )
11c3 file_write ( [BP + 0xff1e], 0x2e94=".", 0x5238, 0x4 )
11cf file_close ( [BP + 0xff1e] )
```

### 422b:120c screen_load_game

```
122d copy_protection_penalty ( LBL[107]"Spielstand Laden", 0, 0, 0, SI )
1246 ui_set_font ( 0x3 )
1261 ~text_to_upper_de ( &[BP + -0x64], LBL[107]"Spielstand Laden" )
1295 ui_draw_text_centered_shadow ( &[BP + -0x64], 0, 0x140, 0x14, 0xb, 0 )
12b0 CALL 422b:2bbe ( CS, 0, 0, &[BP + -0x64], b[BP + 0x6] )
12cf crt_fstrcpy ( &[BP + 0xff4a], 0x4af6, 0x5238 )
12e9 crt_fstrcat ( &[BP + 0xff4a], 0x9446="\", DS )
12fc crt_fstrcat ( &[BP + 0xff4a], &[BP + -0x64], SS )
130d file_open ( &[BP + 0xff4a], 0 )
131e CALL 422b:0006 ( CS )
1330 crt_fstrlen ( [0x93e0], [0x93e2] )
1342 file_read ( [BP + 0xff06], &[BP + 0xff0c], 0 )
1361 crt_fstrncmp ( &[BP + 0xff0c], [0x93e0], [0x93e2], 0x18 )
1399 ~dialog_okay ( LBL[285]"Kein", LBL[286]"Bundesliga Manager Prof.", LBL[287]"Spielstand.", LBL[288]"!!!" )
13a5 file_close ( [BP + 0xff06] )
13ae ~messages_free_all (  )
13c1 file_read ( [BP + 0xff06], &[BP + 0xff0c], 0x4 )
13d7 file_read ( [BP + 0xff06], &[BP + 0xfeba], 0x4 )
13e3 file_close ( [BP + 0xff06] )
13f8 crt_fstrncmp ( &[BP + 0xff0c], 0x93e4=">V2", DS, 0x2 )
1420 ui_dialog_box ( UI[18]"Spielstand aus", UI[19]"falscher Version !" )
14c3 far_memcpy_bytes ( 0xa6dc, 0x5238, 0xa6d8, 0x5238, 0xc4, LBL[192]"Keinen heutigen", LBL[193]"Spielstand laden !" )
14ed crt_fstrcpy ( 0x54a2, 0x5238, &[BP + -0x64], SS )
1506 ui_dialog_box ( LBL[143]"Spielstanddisk", 0, 0 )
1517 file_open ( &[BP + 0xff4a], 0 )
152f crt_fstrlen ( [0x93e0], [0x93e2] )
1546 file_read ( [BP + 0xff06], 0x2242="gkeit", 0x5238, 0 )
155c file_read ( [BP + 0xff06], &[BP + 0xfebe], 0x1 )
1574 file_read ( [BP + 0xff06], 0x57cf="DERLAGE#% F]R %t'S MANNSCHAFT.", 0x5238, 0x1 )
159d CALL 422b:013f ( CS, [BP + 0xff06], 0x60e, 0x5cb3, 0x6 )
15b4 CALL 422b:013f ( CS, [BP + 0xff06], 0x618, 0x5cb3, 0x4 )
15cb CALL 422b:013f ( CS, [BP + 0xff06], 0x5350=" (", 0x5238, 0x4 )
15e2 CALL 422b:013f ( CS, [BP + 0xff06], 0x304a="h Vorlage von ", 0x5238, 0x1 )
15f9 CALL 422b:013f ( CS, [BP + 0xff06], 0x5358, 0x5238, 0x1 )
1610 CALL 422b:013f ( CS, [BP + 0xff06], 0x79e, 0x5cb3, 0x8 )
1627 CALL 422b:013f ( CS, [BP + 0xff06], 0x6ddc="% MANNSCHAFT %x03ERZIELT#ERRUNGEN#ERREICHT#% WURDE.", 0x5238, 0x8e8="s" )
163e CALL 422b:013f ( CS, [BP + 0xff06], 0x7ab, 0x5cb3, 0x1 )
1655 CALL 422b:013f ( CS, [BP + 0xff06], 0x7ac, 0x5cb3, 0x1 )
166c CALL 422b:013f ( CS, [BP + 0xff06], 0x7ad, 0x5cb3, 0x1 )
1683 CALL 422b:013f ( CS, [BP + 0xff06], 0x7ae, 0x5cb3, 0x1 )
169a CALL 422b:013f ( CS, [BP + 0xff06], 0x7af, 0x5cb3, 0x1 )
16b1 CALL 422b:013f ( CS, [BP + 0xff06], 0x7b0, 0x5cb3, 0x1 )
16c8 CALL 422b:013f ( CS, [BP + 0xff06], 0x2242="gkeit", 0x5238, 0xc28="unggelenkprellung" )
16df CALL 422b:013f ( CS, [BP + 0xff06], 0x5714="LAGE#% KAM %a IM SPIEL %x10GEGEN#IN#% %b.", 0x5238, 0x64 )
16f6 CALL 422b:013f ( CS, [BP + 0xff06], 0x3066="1 EINLEGEN", 0x5238, 0x1a90=" sind f}r den Europapokal qualifiziert." )
170d CALL 422b:013f ( CS, [BP + 0xff06], 0xecc="ER SCHNITT", 0x5238, 0xd80="amstag" )
1724 CALL 422b:013f ( CS, [BP + 0xff06], 0x57dd="%t'S MANNSCHAFT.", 0x5238, 0x15d3="en Vertrag nicht^mehr verl{ngern wird.^" )
173b CALL 422b:013f ( CS, [BP + 0xff06], 0x774a, 0x5238, 0x1964="onende" )
1752 CALL 422b:013f ( CS, [BP + 0xff06], 0x4b5e, 0x5238, 0x3c )
1769 CALL 422b:013f ( CS, [BP + 0xff06], 0x2eba="MINUTE", 0x5238, 0x4 )
1780 CALL 422b:013f ( CS, [BP + 0xff06], 0x2e8e="RAGUNG.", 0x5238, 0x4 )
1797 CALL 422b:013f ( CS, [BP + 0xff06], 0xa7a0, 0x5238, 0x4 )
17ae CALL 422b:013f ( CS, [BP + 0xff06], 0x7dc, 0x5cb3, 0x4 )
17c5 CALL 422b:013f ( CS, [BP + 0xff06], 0x7e0, 0x5cb3, 0x2 )
17dc CALL 422b:013f ( CS, [BP + 0xff06], 0x56fe="INEM SIEG#EINER NIEDERLAGE#% KAM %a IM SPIEL %x10GEGEN#IN#% %b.", 0x5238, 0xa )
17f3 CALL 422b:013f ( CS, [BP + 0xff06], 0x57aa="EM UNENTSCHIEDEN#EINEM SIEG#EINER NIEDERLAGE#% F]R %t'S MANNSCHAFT.", 0x5238, 0x12 )
1809 file_read ( [BP + 0xff06], 0x4b54, 0x5238, 0x1 )
183a CALL 422b:013f ( CS, [BP + 0xff06], 0x2e6e="INEN EINZIGEN...", 0x5238, 0x2 )
1851 CALL 422b:013f ( CS, [BP + 0xff06], 0x76ca, 0x5238, 0x80 )
1868 CALL 422b:013f ( CS, [BP + 0xff06], 0x21e2="abelle", 0x5238, 0x60 )
187f CALL 422b:013f ( CS, [BP + 0xff06], 0x8, 0x5238, 0x8 )
1896 CALL 422b:013f ( CS, [BP + 0xff06], 0x1d18="GEHALT PRO MONAT (IN DM)", 0x5238, 0x3 )
18ad CALL 422b:013f ( CS, [BP + 0xff06], 0x535a, 0x5238, 0x3c )
18c4 CALL 422b:013f ( CS, [BP + 0xff06], 0x4c7e, 0x5238, 0x80 )
18db CALL 422b:013f ( CS, [BP + 0xff06], 0x225a, 0x5cb3, 0x3 )
18f2 CALL 422b:013f ( CS, [BP + 0xff06], 0x9336, 0x5238, 0x1394="DAUER" )
1909 CALL 422b:013f ( CS, [BP + 0xff06], 0x5fe, 0x5cb3, 0xf )
1920 CALL 422b:013f ( CS, [BP + 0xff06], 0x0, 0x5238, 0x8 )
1937 CALL 422b:013f ( CS, [BP + 0xff06], 0x4b9a, 0x5238, 0x30 )
194e CALL 422b:013f ( CS, [BP + 0xff06], 0x2ec2="LATZ, ST[RKE ", 0x5238, 0x50 )
1965 CALL 422b:013f ( CS, [BP + 0xff06], 0x2e, 0x5238, 0x140 )
197c CALL 422b:013f ( CS, [BP + 0xff06], 0x66c, 0x5cb3, 0x90 )
1993 CALL 422b:013f ( CS, [BP + 0xff06], 0x4a28, 0x5cb3, 0x1 )
19aa CALL 422b:013f ( CS, [BP + 0xff06], 0x513c, 0x5238, 0x2 )
19c1 CALL 422b:013f ( CS, [BP + 0xff06], 0x1d34="ER VERTRAG", 0x5238, 0x1e )
19d8 CALL 422b:013f ( CS, [BP + 0xff06], 0x5664="SPIEL %a %x10GEGEN#IN#% %b.", 0x5238, 0x78 )
19ef CALL 422b:013f ( CS, [BP + 0xff06], 0x5780, 0x5238, 0x7 )
1a06 CALL 422b:013f ( CS, [BP + 0xff06], 0x63a="(", 0x5cb3, 0x2 )
1a1d CALL 422b:013f ( CS, [BP + 0xff06], 0x7e2, 0x5cb3, 0x2 )
1a34 CALL 422b:013f ( CS, [BP + 0xff06], 0x16e, 0x5238, 0x1 )
1a4b CALL 422b:013f ( CS, [BP + 0xff06], 0x2280, 0x5cb3, 0x5f )
1a62 CALL 422b:013f ( CS, [BP + 0xff06], 0x620, 0x5cb3, 0x10 )
1a79 CALL 422b:013f ( CS, [BP + 0xff06], 0x4bce, 0x5238, 0x1 )
1a90 CALL 422b:013f ( CS, [BP + 0xff06], 0x56e2, 0x5238, 0x4 )
1aa7 CALL 422b:013f ( CS, [BP + 0xff06], 0x5fa, 0x5cb3, 0x4 )
1ace CALL 422b:013f ( CS, [BP + 0xff06], 0x76c6, 0x5238, 0x4 )
1ae5 CALL 422b:013f ( CS, [BP + 0xff06], 0x90c2, 0x5238, 0x4 )
1afc CALL 422b:013f ( CS, [BP + 0xff06], 0x5d6, 0x5cb3, 0x4 )
1b13 CALL 422b:013f ( CS, [BP + 0xff06], 0x61c, 0x5cb3, 0x4 )
1b2a CALL 422b:013f ( CS, [BP + 0xff06], 0x5b8, 0x5cb3, 0x4 )
1b41 CALL 422b:013f ( CS, [BP + 0xff06], 0x3060="-DISK 1 EINLEGEN", 0x5238, 0x1 )
1b5c ~squad_count_by_flag ( b[BP + 0xfebe], 0 )
1c4b CALL 422b:013f ( CS, [BP + 0xff06], &[BP + 0xff02], 0x4 )
1c60 CALL 422b:013f ( CS, [BP + 0xff06], &[BP + 0xfeb8], 0x1 )
1c6f mem_alloc ( b[BP + 0xfeb8], 0 )
1c8e CALL 422b:013f ( CS, [BP + 0xff06], [BP + 0xfeb2], [BP + 0xfeb4], b[BP + 0xfeb8] )
1cf0 file_read ( [BP + 0xff06], 0x4b54, 0x5238, 0x1 )
1d20 file_read ( [BP + 0xff06], 0x5f6, 0x5cb3, 0x4 )
1d2d ~FUN_1cb5_0000 (  )
1d44 file_read ( [BP + 0xff06], 0x2e7a="N...", 0x5238, 0x4 )
1d5c file_read ( [BP + 0xff06], 0x2e94=".", 0x5238, 0x4 )
1d68 file_close ( [BP + 0xff06] )
1daf ~vga_load_picture ( b?:[BX + 0x3087], 0, 0, 0, 0x1 )
1df2 blit_bitmap ( ES:[BX], ?:[BX + 0x2], 0, 0, DS:[0x5b0], DS:[0x5b2], 0x29, 0, 0x28, 0x26, 0xc0 )
1e02 bitmap_free ( [BP + 0xfec2], [BP + 0xfec4] )
1e31 screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0 )
1e44 ~vga_load_picture ( 0, 0, 0, 0, 0x1 )
1ece blit_bitmap ( ES:[BX], ?:[BX + 0x2], 0x29, 0, FAR:[0x4d38], FAR:[0x4d3a], 0x29, 0, 0x27, 0x28, 0xc0 )
1eec bitmap_free ( [BP + 0xfec2], [BP + 0xfec4] )
1f0e screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0 )
1f33 ui_fill_rect ( 0, 0, 0x13f, 0xef, 0 )
1f4c ui_set_font ( 0x3 )
1f81 ui_draw_text_centered_shadow ( UI[138]"DER GROSSE MOGELGURU", 0, 0x13f, 0x19, 0x12, 0xff )
1fb6 ui_draw_text_centered_shadow ( UI[139]"VERABSCHEUT IHRE TATEN", 0, 0x13f, 0x2d, 0x12, 0xff )
1fcf ui_set_font ( 0x2 )
201b ui_draw_text_centered_shadow ( ?:[BX + 0x2500], ?:[BX + 0x2502], 0, 0x13f, [BP + 0xfec6], 0x1, 0xff )
203f screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0x1 )
207e ui_draw_frame ( [BP + 0xfec6], [BP + 0xfec6], [BP + 0xfec6], 0x35, b[BP + 0xfeb6] )
2097 delay_ticks ( 0x14, 0x13f )
20d0 screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0 )
20e9 rastport_clear ( 0x8 )
2102 ui_set_font ( 0x1 )
212e ui_draw_text_centered_shadow ( 0x9448="Sie haben einen V1.x-Spielstand geladen.", DS, 0, 0x13f, 0x14, 0x1, 0 )
215a ui_draw_text_centered_shadow ( 0x9471="Bitte geben Sie den Spiel-Level (0-5) ein:", DS, 0, 0x13f, 0x1e, 0x1, 0 )
2173 screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0x1 )
2199 ~money_round_display ( 0x3, 0x13f, 0x7d, 0x32, 0, 0, 0x5, 0x13f, 0x1, 0x13f )
```

### 422b:2366 highscore_screen

```
2393 screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0, SI, DI )
23ac mem_alloc ( 0x488="aqdotirgnjesfklcbm", 0x1, 0x1d52=" wurde angenommen !", 0x5238, 0x488="aqdotirgnjesfklcbm" )
23bb far_memcpy_bytes ( 0x488="aqdotirgnjesfklcbm", 0x5238 )
23cd mem_free ( [BP + -0x7a], [BP + -0x78], 0x488="aqdotirgnjesfklcbm" )
23de CALL 422b:21c4 ( CS, 0 )
23f6 CALL 422b:2a2a ( CS, FAR:[0x304a], 0 )
2404 mouse_pointer_hide ( 0 )
241a ~vga_load_picture ( 0x2d, 0x7530=" ENTGEGEN ZU SETZEN.", 0, 0, 0x1 )
2422 mouse_pointer_show (  )
2438 ui_set_font ( 0x2 )
248c far_memcpy_bytes ( AX, ES, 0x4cfe, 0x5238, [BP + -0x6a] )
24a8 far_memcpy_bytes ( AX, 0x5238, [BP + -0x7e], [BP + -0x7c], [BP + -0x6a] )
24bd far_memcpy_bytes ( 0x4cfe, 0x5238, AX, AX, [BP + -0x6a] )
24e4 CALL 422b:21c4 ( CS, 0x1 )
251b crt_fstrcmp ( 0x3a, 0x5238, 0x4cfe, 0x5238 )
2538 crt_fstrcmp ( DX, 0x5238, 0x4d18, 0x5238 )
258a far_memcpy_bytes ( 0x4cfe, 0x5238, 0x3a, 0x5238, [BP + -0x6a] )
25df far_memcpy_bytes ( AX, 0x5238, 0x3a, 0x5238, [BP + -0x6a] )
260b far_memcpy_bytes ( 0x4cfe, 0x5238, 0x3a, 0x5238, [BP + -0x6a] )
26d7 ui_draw_text_at ( 0x3a, 0x5238, 0x38, [BP + -0x6c], 0 )
2704 ui_draw_text_at ( CX, 0x5238, 0x68, [BP + -0x6c], [BP + -0x4] )
271f crt_fstrcpy ( &[BP + -0x68], UI[140]"PUNKTE:" )
273d format_long_thousands ( ?:[SI + 0x1d86], 0x5238 )
274b crt_fstrcat ( &[BP + -0x68], ?:[SI + 0x1d86], 0x5238 )
277a ui_draw_text_at ( &[BP + -0x68], 0xbb, [BP + -0x6c], [BP + -0x4] )
278c crt_fstrcpy ( &[BP + -0x68], 0x94a4="(X/X/X)", DS )
27dd ui_draw_text_at ( &[BP + -0x68], 0xf3, [BP + -0x6c], [BP + -0x4] )
2833 screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0x1 )
2845 CALL 422b:2366 ( CS, 0x2 )
2857 input_wait_click ( FAR:[0x6dc0], FAR:[0x6dc2] )
```

### 422b:2bbe file_selector

```
2be7 ui_set_font ( 0x2, SI, DI )
2bf6 mem_alloc ( 0x2c, 0 )
2c2f ui_draw_button ( 0x6, 0x21, 0xf1, 0xb5, &[BP + 0xfe76], 0, 0x8 )
2c67 ui_draw_button ( 0x10c="font", 0xc1, 0, b[BP + 0xe], &[BP + 0xfe76], 0, 0x8 )
2c8f ui_draw_line ( 0x7, SI, 0xf6, SI, 0xb )
2ca0 crt_getdrive (  )
2cb2 crt_getcwd ( &[BP + -0x5a], 0x5a )
2cc5 crt_chdrive ( [0x942c] )
2cd3 crt_chdir ( 0x4af6, 0x5238 )
2ce0 crt_getdrive ( &[BP + 0xfe82], SS )
2ce6 crt_dos_setdrive ( &[BP + 0xfe82] )
2d06 crt_fstrcpy ( SI, 0x5238, 0x94ac, DS )
2d35 crt_fstrcpy ( 0x1cfb="E", 0x5238, 0x2994, 0x5cb3 )
2d85 ~ui_register_icon_slot ( DI, 0xd, [BP + 0xfe14], 0x5238, 0x1a )
2db5 crt_fstrcpy ( [BP + 0xa], [BP + 0xc], 0x54a2, ES )
2def rastport_set_draw_mode ( 0 )
2e18 ui_fill_rect ( 0x7, 0x22, 0xf6, 0xa1, 0x8 )
2e2a crt_getcwd ( [BP + 0x6], [BP + 0x8], 0x5a )
2e3d crt_fstrcpy ( &[BP + 0xff44], 0x94ad="*.*", DS )
2e57 crt_dos_findfirst ( &[BP + 0xff44], 0x10, [BP + 0xfe86], [BP + 0xfe88] )
2e8d ~text_to_upper_de ( &[BP + 0xff44], &[BX + 0x1e], ES )
2eaa crt_fstrcpy ( 0xd, 0x5238, &[BP + 0xff44], SS )
2ec7 ~text_to_upper_de ( &[BP + 0xff44], BX, ES )
2ee4 crt_fstrcpy ( 0xd, 0x5238, &[BP + 0xff44], SS )
2ef8 crt_dos_findnext ( [BP + 0xfe86], [BP + 0xfe88] )
2f1e crt_fstrcpy ( &[BP + 0xff44], UI[144]"VERZEICHNIS: " )
2f32 ~text_to_upper_de ( &[BP + 0xfe8c], [BP + 0x6], [BP + 0x8] )
2f46 crt_fstrcat ( &[BP + 0xff44], &[BP + 0xfe8c], SS )
2f74 ui_fill_rect ( 0xa, 0xa5, 0xf0, 0xab, 0x8 )
2f9c ui_draw_text_at ( &[BP + 0xff44], 0xa, 0xaa, 0x1 )
2fc5 ui_fill_rect ( 0xa, 0xaf, 0xf2, 0xb5, 0x8 )
2ff5 ui_draw_text_at ( UI[145]"FILENAME      :", 0xa, 0xb4, 0x1 )
3022 ui_draw_text_at ( BX, ES, 0x3f, 0xb4, 0x1 )
3051 ui_fill_rect ( 0xd2, 0xaf, 0xf2, 0xb5, 0x8 )
306d crt_fstrcpy ( &[BP + 0xff44], UI[146]"SEITE: " )
3083 format_long_thousands ( [BP + 0xfe1a], 0xa )
3092 crt_fstrcat ( &[BP + 0xff44], [BP + 0xfe1a], 0xa )
30ba ui_draw_text_at ( &[BP + 0xff44], 0xd2, 0xb4, 0x2 )
30f1 crt_fstrcpy ( &[BP + 0xff44], SI, 0x5238 )
3116 ui_draw_text_at ( &[BP + 0xff44], 0xa, [BP + 0xfe14], 0x3 )
318f crt_fstrcpy ( &[BP + 0xff44], SI, 0x5238 )
31a3 crt_fstrlen ( [BP + 0xfe10], [BP + 0xfe12] )
31fb ui_draw_text_at ( &[BP + 0xff44], 0x82, [BP + 0xfe14], b[BP + -0x62] )
3234 screen_fade ( FAR:[0x2e70], FAR:[0x2e72], 0x1 )
324a input_poll_mouse ( FAR:[0x6dc0], FAR:[0x6dc2] )
3276 rastport_set_draw_mode ( 0x2 )
3306 ui_fill_rect ( AX, AX, 0x78, CX, 0x1 )
334b ui_fill_rect ( AX, AX, DI, [BP + 0xfe14], 0x1 )
338f ui_fill_rect ( AX, AX, 0x78, 0x7, 0x1 )
33ae rastport_set_draw_mode ( 0 )
33c3 ui_hit_test_buttons ( &[BP + 0xfe76], 0x1, 0 )
3410 rastport_set_draw_mode ( 0 )
3439 ui_fill_rect ( 0x7, 0x22, 0xf6, 0xa1, 0x8 )
34ce ui_fill_rect ( 0x3c, AX, 0xa, 0xa, 0x8 )
3508 ui_scroll_text_box ( FAR:[0x6dc0], FAR:[0x6dc2], &[BP + 0xfe1c], 0x3f, 0xaa, 0xaf, 0x1, 0x8, 0x1 )
3516 crt_chdir ( &[BP + 0xfe1c], SS )
3557 ui_scroll_text_box ( FAR:[0x6dc0], FAR:[0x6dc2], [BP + 0xa], [BP + 0xc], 0x3f, 0xb4, 0x8c, 0x1, 0x8, 0x1 )
358e crt_fstrlen ( [BP + 0xa], [BP + 0xc] )
35a1 crt_fstrlen ( [BP + 0xa], [BP + 0xc] )
35b8 crt_fstrlen ( [BP + 0xa], [BP + 0xc], 0x94b1=".MAN", DS )
35ca crt_fstrcmp ( 0x94b1=".MAN", [BP + 0xc] )
35e4 crt_fstrcat ( [BP + 0xa], [BP + 0xc], 0x94b6=".MAN", DS )
3607 ~ui_wait_with_highlight ( 0x10c="font", 0xc1 )
3650 crt_fstrcpy ( &[BP + 0xfe1c], [BP + 0x6], [BP + 0x8] )
368b crt_fstrcmp ( [BP + 0xa], [BP + 0xc], SI, 0x5238, 0x5238 )
36af crt_fstrcpy ( [BP + 0xa], [BP + 0xc], SI, 0x5238 )
36c3 crt_fstrcpy ( &[BP + 0xff44], [BP + 0xa], [BP + 0xc] )
36ec ui_fill_rect ( 0x3f, 0xaf, 0xf2, 0xb5, 0x8 )
3714 ui_draw_text_at ( [BP + 0xa], [BP + 0xc], 0x3f, 0xb4, 0x1 )
371c ~ui_hit_icon_slot (  )
3778 ~message_broadcast ( FAR:[0x304a], 0x94bb="FILE", DS, 0x23, [BP + 0xc], LBL[211]"WIRKLICH L|SCHEN ?", LBL[212]"MACH SCHON!", LBL[213]"OH NEIN!" )
3790 crt_fstrcpy ( &[BP + 0xff44], [BP + 0xa], [BP + 0xc] )
379e dos_delete_file ( &[BP + 0xff44], SS )
37b9 crt_chdrive ( AX )
37cd mem_free ( [BP + 0xfe86], [BP + 0xfe88], 0x2c )
37d5 crt_getdrive (  )
37e9 crt_getcwd ( 0x4af6, 0x5238, 0x5a )
37f5 crt_chdrive ( [BP + 0xfe7e] )
3800 crt_chdir ( &[BP + -0x5a], SS )
```
