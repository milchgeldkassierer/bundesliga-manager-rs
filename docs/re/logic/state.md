# Game state model (`crates/bmp-game/src/state.rs`)

`BMMAIN.EXE` keeps its whole game state in global variables in two data segments:

* `DS:xxxx` is DGROUP, Ghidra segment `5cb3` (load paragraph `4cb3`).
* `FAR:xxxx` is the far data segment, Ghidra segment `5238` (load paragraph `4238`). Code reaches it
  through the selector table `DS:9900..a146` (`MOV ES,[DS:99xx]`).

The two segments use overlapping offsets, so an offset is meaningless without its segment.
`savegame.md` writes load paragraphs (`4cb3`/`4238`), while Ghidra and this document use `5cb3`/`5238`.

A `.MAN` save stores 61 of these globals ("blocks"), the per-manager message string table and a 12-byte
raw tail. `GameState` has one typed field for each block, in save order, so the save **is** the persistent
state. Everything else is rebuilt at run time; the important non-persistent globals are listed in §5.

All addresses are Ghidra addresses in `/unpacked/BMMAIN_unpacked.EXE`. Function names are the
FUNCTION_MAP names. Some of them are only proposed (italic in FUNCTION_MAP) and do not exist as Ghidra
symbols, where the function is still called `FUN_xxxx_xxxx`.

## 1. Rust API

| Item | Origin | Semantics |
|---|---|---|
| `trait Layout` (`LEN`, `read`, `write`, `zeroed`, `from_bytes`, `to_bytes`) | – | Fixed little-endian layout. It is implemented for `u8 i8 u16 i16 u32 i32`, for arrays of any `Layout`, and for every record. MSC tables are packed, so there is no padding |
| `record!` macro | – | Declares a struct whose fields lie back to back. `LEN` is the sum of the field sizes |
| `GameState::from_savegame(&SaveGame)` | 422b:120c `screen_load_game` | Decodes every block into its typed field. Header, string table and tail are kept |
| `GameState::to_savegame()` | 422b:07fe `screen_save_game` / `savegame_write_man` | Inverse of `from_savegame`. `SaveGame::to_bytes` then recomputes the cipher and checksums |
| `GameState::zeroed()` | – | All blocks zero, empty message table, V2 header. Not the program-start state |
| `GameState::program_start()` | DGROUP image (segment 5cb3) | `zeroed()` plus the initialised DGROUP blocks: #0, #5, #8, #24, #34, #36, #41, #42, #47, #50, #51, #54, #57..#59 (every far block starts zero) |
| `GameState::far_data_image()` (crate-private) | – | The 64 KiB far segment with every saved far block at its address and every unsaved far global zero; for ports that read past the end of a far table (`player_pool_season_shuffle`) |
| `GameState::manager_index_of_team(a, b)` | 1310:32b1 `manager_index_of_team` | Scans managers `0..DS:07ab` in order. It returns `m+1` if `team == a`, `0x81+m` if `team == b` (team `a` is checked first for each manager), otherwise `0`. The original loop counter is a byte and is bounded by the unclamped `DS:07ab`. The port clamps it to the 4 records |
| `current()`, `active_managers()` | – | `managers[FAR:304a]`, `managers[..DS:07ab]` |
| `GameState::sort_league_table(tier, team_count, view, ignore_played)` | 3a41:2d33 (sort loop 3a41:33d0..353f, position loop 3a41:3750..3784) | Exchange sort of `league_teams[tier][..DS:225e[tier]]` (for `i < j`, swap when club `j` ranks strictly higher), then `team_tables[league_teams[tier][i]].table_position = i`. See §3.4 for the keys and `view` |
| `GameState::record_position_history(tier)` | 3a41:2d33 (loop 3a41:3786..37c7) | For each manager `m < DS:07ab`: `managers[m].+0x10b[(i8)(DS:225a[tier] - 1)] = team_tables[managers[m].team].table_position`. Index -1 lands on `+0x10a` (`value_10a`), as in the original |
| `BLOCK_FIELD_LENS` | – | Typed size of each block. The test checks it against `savegame::BLOCKS` |

The tests check that:

* the record sizes are 0x12, 0x30a, 0x22, 0x25, 0x34, 0x36 and 0x1394;
* every block's typed size equals `BLOCKS[i].len`;
* **every `*.MAN` in `BMP/` and `BMP/BMP/` round-trips byte-exactly** (`SaveGame -> GameState ->
  SaveGame -> bytes`);
* `zeroed()` round-trips;
* `manager_index_of_team` behaves as described;
* `sort_league_table` applies the four keys in order (synthetic test), and re-sorting the full
  (view 1) table of every real save is a no-op;
* in every real save, each tier's `table_position`s are the permutation `0..n` (n >= 18) in
  `league_teams` order, `points == 2*wins + (played - wins - losses)` for the home and away slots,
  lineup positions 1..11 occur at most once per human squad, and `position_history[round-1]` (or
  `[round-2]` in AUTOSAVE.MAN, which was saved after the round counter advanced but before that
  round's table update) equals the manager club's `table_position`;
* the AUTOSAVE.MAN content makes sense: OLIVER manages BAYER 05 UERDINGEN, the players he owns carry his
  club, and the form strings contain only `SUNsun@`.

Types follow the original: `char` is `i8`, `int` is `i16` and `long` is `i32`. The managers' `FAR:304a`
and `FAR:5358` are `char`, because the code multiplies them signed (`*(char*)0x304a * 0x30a`).

## 2. The 61 save blocks

The "Users" column comes from `out/funcmap/globals.tsv`, aggregated by `out/state/blocks.py`; the full
list is in `out/state/blocks_xrefs.txt`. Confidence: **H** = verified in decompiled code, **M** =
inferred from code plus observed data, **L** = name or role guessed.

| # | Address | Rust field : type | Meaning | Main users | Conf |
|---|---|---|---|---|---|
| 0 | DS:060e | `interest_rates: [u8;6]` | Loan interest rate per lender in %. Computed in 212a:000a from the base rate DS:0630 (random walk, clamped 2..7) | 212a:000a, `screen_credits` 212a:167f | H |
| 1 | DS:0618 | `message_counts: [u8;4]` | Number of message strings per manager. It is the string-table group size, and the strings are in FAR:5524 | `screen_messages` 4091:05c4, `message_box` 4091:0190 | H |
| 2 | FAR:5350 | `ticker_slots: [u8;4]` | Match-ticker line slots, 0xff = free | `matchday_play` 1310:15db, 2a58:0ca3 | M |
| 3 | FAR:304a | `current_manager: i8` | Index of the manager whose turn it is (474 uses) | `season_main_loop` 2a58:3176, everywhere | H |
| 4 | FAR:5358 | `saved_manager: i8` | Copy of FAR:304a, used by message routing | `message_box`, `mana_load_player_names` | M |
| 5 | DS:079e | `lineup_state: [[u8;2];4]` | Per-manager byte pair. A value of `[m][0] > 1` forces the lineup dialog (2ecd:14c7 `auto_lineup_disable`), which then resets it to 1. `season_main_loop` shifts `[1]` into `[0]` | 2a58:3176, 2ecd:* | M |
| 6 | FAR:6ddc | `league_results: [[[u8;20];38];3]` | League results `[tier][round][slot]`. Even slot = home goals, odd slot = away goals, 0x1e = not played (writer 1310:0ae8..: `((FAR:4bd0[i]*0x26 + FAR:57a6[i])*0x14 + DS:0170[i] + k)`) | 3a41:2d33, 1310:152e | H |
| 7 | DS:07ab | `manager_count: u8` | Number of human managers. It is also the string-table group count | everywhere | H |
| 8 | DS:07ac | `winner_07ac: u8` | Competition winner club + 1. It gives a +20 prestige bonus in `season_end` 2a58:4597 (DFB-Pokal?) | `competition_reset` 27c2:0ef2, `cup_play_round` | M/L |
| 9 | DS:07ad | `winner_07ad: u8` | Same, +50 bonus (European Champions Cup?) | same | M/L |
| 10 | DS:07ae | `winner_07ae: u8` | Same, +40 bonus | same | M/L |
| 11 | DS:07af | `winner_07af: u8` | Same, +30 bonus | same | M/L |
| 12 | DS:07b0 | `competition_07b0: u8` | Competition state (observed 13, 30, 32) | `competition_reset` | L |
| 13 | FAR:2242 | `managers: [Manager;4]` | Human manager records (§3.1) | | H |
| 14 | FAR:5714 | `rescheduled_matches: [[u8;5];20]` | "Nachholspiele" list. Byte +1 = team (0 = free entry), +2..+4 further match data | `matchday_title_rescheduled` 1310:07a2, `screen_fixtures` 3a41:120a | M |
| 15 | FAR:3066 | `clubs: [Club;200]` | Club table (§3.2) | | H |
| 16 | FAR:0ecc | `team_tables: [TeamTable;64]` | League table record per German club (§3.4) | 3a41:2d33, `screen_league_table` 3a41:214c | H |
| 17 | FAR:57dd | `players: [Player;151]` | Player master table (§3.3) | | H |
| 18 | FAR:774a | `squads: [[SquadPlayer;25];5]` | Squads 0..3 belong to the human managers, squad 4 is the transfer pool (§3.5) | `screen_lineup` 2ecd:18bf, `screen_transfer_market` 3277:04a5 | M |
| 19 | FAR:4b5e | `matchday_pairings: [[[u8;2];10];3]` | Current match-day pairings `[tier][match][home, away]`. 0x46 (`'F'`) = no match | `match_live_view` 1310:2303, `league_team_range` 3a41:10e3 | H |
| 20 | FAR:2eba | `day_of_month: i32` | Day of month. Compared with the special-date table DS:53d0 and with loan payment days | `matchday_refresh` 1310:1190, `special_date_events` 2a58:2a06 | H |
| 21 | FAR:2e8e | `month: i32` | Month, 0-based | same | H |
| 22 | FAR:a7a0 | `year: i32` | Current year. Compared with `Credit::due_year`; observed 1965..1968 | `finance_weekly_processing` 212a:0a6d | H |
| 23 | DS:07dc | `day_counter: i32` | Day counter of `season_main_loop`. "WINTERPAUSE"/season end when it passes `DS:224c + 0x142` | 2a58:3176, `random_weekly_events` | H |
| 24 | DS:07e0 | `start_year: u16` | Year word (observed one less than `year`) | `start_menu_new_or_load`, `season_main_loop` | M |
| 25 | FAR:56fe | `cup_team_list: [u8;10]` | Team id list, length FAR:2e74 (`team_is_in_list` 27c2:11e7) | `competition_reset` | M |
| 26 | FAR:57aa | `competition_slots: [u8;18]` | Competition slots, 0x80 = empty | `competition_reset` | M |
| 27 | FAR:2e6e | `relegation_pair: [u8;2]` | Relegation pairing, copied to FAR:21e2 | `matchday_title_relegation` 1310:02bf | M |
| 28 | FAR:76ca | `cup_pairings: [[u8;2];64]` | Cup pairings (home, away) | `cup_play_round` 27c2:16dc, `competition_init` 27c2:09e0 | M |
| 29 | FAR:21e2 | `relegation_table: [u8;96]` | Relegation matches (zero in all samples) | 1310:02bf, 27c2:13a2 | L |
| 30 | FAR:0008 | `event_state: [u8;8]` | Event and state bytes (Christmas text, random events) | `random_weekly_events` 1cb5:13bd, 3e3a:0fdf | L |
| 31 | FAR:1d18 | `far_1d18: [u8;3]` | Relegation/cup state | 1310:02bf, 27c2:16dc | L |
| 32 | FAR:535a | `league_teams: [[u8;20];3]` | Club ids of each league tier (18 or 20 used), reordered by table sorting (3a41:2d33) and swapped in 1310:0f44 | 3a41:2d33, `season_main_loop` | H |
| 33 | FAR:4c7e | `cup_results: [[u8;2];64]` | Cup result goals | `cup_play_round`, `match_penalty_shootout` 1310:356d | M |
| 34 | DS:225a | `competition_rounds: [u8;3]` | Round counter per competition (incremented by bits 0..2 of DS:2280[week]) | `screen_league_table`, `office_menu_dispatcher` | M |
| 35 | FAR:9336 | `match_history: MatchHistory` | Head-to-head and record tables (§3.6) | 3a41:2d33, 3a41:2073 | H |
| 36 | DS:05fe | `options: [u8;15]` | Option flags (settings screen; `+0x0e` read by `screen_fade`) | `screen_settings` 362f:01b8, `match_live_view` | L |
| 37 | FAR:0000 | `far_0000: [u8;8]` | 8 state bytes; `[2*m]` is the main-sponsor contract countdown in months (finance.md §2.1). The xrefs are polluted by struct offsets 0..7 | 362f:2be8, 3277:04a5 | L |
| 38 | FAR:4b9a | `far_4b9a: [u8;48]` | Contract and job-offer state; `[2*(6*m+i)]` is the countdown in months of advertising contract `i` (finance.md §2.1) | `screen_manager_contract` 362f:2be8, 362f:29a8 | L |
| 39 | FAR:2ec2 | `far_2ec2: [u8;80]` | Job-offer state | 362f:2be8, `job_offer_no_interest` 362f:281f | L |
| 40 | FAR:002e | `far_002e: [u8;320]` | Table used by player development and events | `player_aging_or_development` 24a4:2cb4, `random_weekly_events` | L |
| 41 | DS:066c | `manager_contracts: [[i32;9];4]` | Per-manager contracts (`long`s, stride 0x24): `[0]` monthly main-sponsor income, `[1..=6]` monthly advertising-board income, `[7]` an eighth income entry, all added to the monthly income (finance.md §4.1); `[8]` a monthly cost item (§4.2). New game (18bc:2184, only in scenarios FAR:513c == 0x7ac or 0x7c9, only for managers from the first newly added one up to DS:07ab): `[0]` = 180 000 (0x2bf20), `[1..=6]` = 45 000 each, `[8]` = 30 000; `[7]` untouched. Unused rows hold `50000, 6000 x6, 0, 5000` | `screen_manager_contract` 362f:2be8, 362f:29a8 | M |
| 42 | DS:4a28 | `level: u8` | Difficulty level `5 - user level`. Level 4 gives a starting balance of 1.9 M | `start_menu_new_or_load`, `match_simulate_minute` | H |
| 43 | FAR:513c | `scenario: u16` | Scenario/start-year selector. 0x7ac/0x7c9 and 0x7ae/0x7cb are special-cased in the new-game setup and in `highscore_file_name`; `> 2000` changes player generation; 0x56ce in all samples | 18bc:2184, 422b:21c4 | M |
| 44 | FAR:1d34 | `team_slots: [[u8;10];3]` | Three rows of 10 slot bytes (`team_slot_free_index` 4091:0000). MEMORY_MAP lists this as DS:1d34, but the save code uses the far segment | 1cb5:13bd | M |
| 45 | FAR:5664 | `message_pointers: [[u32;10];3]` | Far string pointers. The loader patches them through the string table | `random_weekly_events`, `screen_load_game` | H |
| 46 | FAR:5780 | `construction: [u8;7]` | Per-item construction-firm lock in days, shared by all managers (finance.md §3.5) | `stadium_construction_progress` 1000:20e1 | L |
| 47 | DS:063a | `match_options: u16` | Match-day option ("Spiele automatisch / alleine / weiter / nicht") | `matchday_options_dialog` 18bc:08f2 | M |
| 48 | DS:07e2 | `logged_rounds: u16` | Number of logged rounds in `Manager::round_log`, capped at 0x31 | 2a58:3176, `screen_manager_history` | M |
| 49 | **FAR:016e** | `season_week: u8` | Current week index into DS:2280. **MEMORY_MAP §2.3 says DS:016e; the save code and all xrefs use segment 5238** | 2a58:3176, `office_menu_dispatcher` | H |
| 50 | DS:2280 | `week_competitions: [u8;95]` | Per-week competition bitmask. Bits 0..2 count DS:225a; 9 marks a special week; observed values 0, 1, 6, 7, 8, 0x10, 0x70, 0x80 | 2a58:3176, 1310:05f1 | M |
| 51 | DS:0620 | `ds_0620: [i16;8]` | Training-camp availability counters (> 0 closed, < 0 open), stepped toward 0 daily (finance.md §2.1) | `finance_weekly_processing`, `screen_training_camp` | L |
| 52 | FAR:4bce | `loop_month: u8` | Month copy at loop start | 2a58:3176 | L |
| 53 | FAR:56e2 | `far_56e2: [u8;4]` | 4 bytes | `office_menu_dispatcher`, `message_box` | L |
| 54 | DS:05fa | `ds_05fa: [u8;4]` | `[0]` is copied from the copy-protection flag DS:05ac right before saving | 422b:07fe, 422b:120c | M |
| 55 | FAR:76c6 | `far_76c6: [u8;4]` | Cup state bytes (`[3]`) | `competition_init`, `cup_round_setup` 27c2:13a2 | L |
| 56 | FAR:90c2 | `far_90c2: [u8;4]` | No code reference found in globals.tsv | – | L |
| 57 | DS:05d6 | `ds_05d6: [u8;4]` | No code reference found | – | L |
| 58 | DS:061c | `ds_061c: [u8;4]` | No code reference found | – | L |
| 59 | DS:05b8 | `ds_05b8: [u8;4]` | No code reference found | – | L |
| 60 | FAR:3060 | `far_3060: u8` | Byte read by `season_main_loop` | 2a58:3176 | L |

The fields outside the blocks are:

* the header: `signature`, `version`, `save_id`, `pad`, `initial_key`;
* `messages: Vec<Vec<StringEntry>>`, one group per manager, rebuilt into FAR:5524;
* the raw tail: `ds_05f6`, `far_2e7a`, `far_2e94` (`u32` each, meaning unknown).

## 3. Records

### 3.1 `Manager`: FAR:2242 + m*0x30a, 778 bytes (block 13)

Most fields were traced in the new-game code of `start_menu_new_or_load` 18bc:2184 (`FUN_18bc_2184` in
Ghidra; manager loop around 18bc:2595..2a20) and in `startup_clear_state` 18bc:01fb (`FUN_18bc_01fb` in
Ghidra). MEMORY_MAP gives the offsets relative to FAR:2260 (= +0x1e); this table uses the record start.

| Off | FAR (m=0) | Field : type | Meaning |
|---|---|---|---|
| 0x000 | 2242 | `name: [u8;29]` | Name, NUL terminated |
| 0x01d | 225f | `slot: u8` | 1..4. `startup_clear_state` writes `m+1` and the loader repairs 0. The start menu swaps it when managers are reordered |
| 0x01e | 2260 | `team: u8` | Club index, 100 = none (`NO_TEAM`). A new game with no name typed gets `m*11` |
| 0x01f | | `unknown_01f: [u8;31]` | |
| 0x03e | 2280 | `round_log: [[u8;4];50]` | Per-round log, initialized to 0xff. Shifted when DS:07e2 reaches 0x31 |
| 0x106 | | `unknown_106: [u8;4]` | |
| 0x10a | 234c | `value_10a: u8` | `(s + 8) * 2 - 3*league` (`s` = scenario 0x7ac/0x7c9) |
| 0x10b | 234d | `position_history: [u8;38]` | League-position history, one 0-based table position per round. Initialized to 10. Written for every manager at the end of 3a41:2d33 at index `(i8)(DS:225a[tier] - 1)` of the tier just updated (see `record_position_history`) |
| 0x131 | 2373 | `value_131: u8` | Initialized to 0x10 |
| 0x132 | 2374 | `values_132: [u8;5]` | Initialized to 0x1e. `startup_clear_state` re-sets `[2..5]` |
| 0x137 | | `unknown_137: u8` | |
| 0x138 | 237a | `league: u8` | 0 = Bundesliga, 1 = 2. Bundesliga, 2 = amateur (compared with `league_of_rank()>>1`, 27c2:1bb5) |
| 0x139 | 237b | `camp_cooldown: u8` | Days until the next training camp; `random(7,18)` after a booking, decremented daily (finance.md §2.1) |
| 0x13a | | `values_13a: [u8;5]` | Training/AI bytes |
| 0x13f | 2381 | `training_intensity: u8` | 0..0x22 (`screen_training`); monthly cost and `fatigue_sum` |
| 0x140 | | `value_140: u8` | |
| 0x141 | 2383 | `training_141: [u8;5]` | Copied from DS:0286 on a new game |
| 0x146 | 2388 | `training_146: [u8;4]` | Copied from DS:028c |
| 0x14a | | `unknown_14a: [u8;20]` | |
| 0x15e | 23a0 | `stadium: [[i32;2];7]` | `[item][0]` = current value, `[item][1]` = under construction. New-game values are in the Rust doc comment |
| 0x196 | 23d8 | `build_days: [u16;7]` | Construction countdown per stadium item in days (finance.md §3.2, §3.5) |
| 0x1a4 | 23e6 | `season_stats: [i32;8]` | Season totals (`season_main_loop`). Observed values are DM-sized |
| 0x1c4 | | `unknown_1c4: [u8;24]` | |
| 0x1dc | 241e | `fans: u16` | New game: `50 - 20*league`; grows at month end while below 95 |
| 0x1de | 2420 | `extra_costs: u16` | Monthly expense item |
| 0x1e0 | 2422 | `income_base: u16` | Low word of the monthly base income (24a4:2636) |
| 0x1e2 | 2424 | `fatigue_sum: i16` | `+= 4*training_intensity` at every month end |
| 0x1e4 | 2426 | `records: [i32;3]` | `[2]` (FAR:242e) is initialized to 99999 |
| 0x1f0 | 2432 | `balance: i32` | Account balance in DM (`KONTOSTAND`). New game: 1 500 000 (0x16e360), or 1 900 000 (0x1cfde0) if DS:4a28 == 4 |
| 0x1f4 | 2436 | `values_1f4: [i16;4]` | Written by 2a58:20b2 |
| 0x1fc | 243e | `credits: [[Credit;3];5]` | Loans |

`Credit` is 0x12 bytes long:

| Off | Field | Meaning |
|---|---|---|
| 0x00 | `principal: i32` | Amount owed. The slot is free when it is 0 |
| 0x04 | `interest_payment: i32` | Interest paid on each payment day |
| 0x08 | `day_taken: u8` | Day of month the loan was taken |
| 0x09 | `payment_day: u8` | Compared with FAR:2eba |
| 0x0a | `month_taken: u8` | Month (0-based) the loan was taken |
| 0x0b | `due_month: u8` | Compared with FAR:2e8e |
| 0x0c | `rate: u8` | Interest rate in % |
| 0x0d | `unknown_0d: u8` | Never written by `credit_take` |
| 0x0e | `year_taken: i16` | Year the loan was taken |
| 0x10 | `due_year: i16` | Compared with FAR:a7a0 |

The loan logic is in `finance_weekly_processing` (212a:0a6d, around 212a:0eee):

For lender `l = 0..4` and slot `k = 0..2` of the **current manager** (FAR:304a; only that manager's
record is scanned), with a used slot (`principal != 0`):

* **Payment day** when `payment_day == day` (FAR:2eba as a `long`, i.e. the high word FAR:2ebc is 0),
  **or** (212a:0eb1..0eec) when `payment_day != day`, `days_in_month[month] < days_in_month[due_month]`
  and `day == days_in_month[month]`, i.e. today is the last day of a month that is shorter than the
  loan's due month (table DS:07b8, `char`, indexed by the month FAR:2e8e). This makes a payment day of
  e.g. 31 fall on the 30th / 28th in short months.
* On a payment day `interest_payment` is subtracted from the current manager's `balance` (32-bit with
  borrow). The interest is not credited here; `finance_monthly_income` (24a4:2636) adds every other manager's interest to the monthly income (finance.md §4.1).
* If additionally `due_year == year` (FAR:a7a0 as a `long`) and `due_month == month` (with FAR:2e90 ==
  0), the game shows "Ihr Kredit von der Bank wurde heute faellig", subtracts `principal` from the
  current manager's balance, adds it to manager `l`'s balance if `l != 4` (bank = 4), and clears the
  slot (`principal = 0`).

`finance_sum_entries(kind, lender)` (212a:0f8d) sums `principal` (`kind == 0`) or `interest_payment`
over the used slots of `lender`, or of all lenders when `lender == 'c'`. The result is truncated to
`int`. "Schon 3 Kredite" appears when all 3 slots of a lender are used.

### 3.2 `Club`: FAR:3066 + c*0x22, 34 bytes (block 15)

| Off | Field | Meaning |
|---|---|---|
| 0x00 | `name: [u8;23]` | Name (23 bytes as in MANA.DAT) |
| 0x17 | `morale: u8` | `random(0x2d,0x37)` at load. After each match, 3a41:1fec adds `±random(0, 2*f(goal diff))`, clamped by 4091:1009 |
| 0x18 | `strength: [u8;3]` | Current strengths of the three team parts (`team_strength_value` 1310:1468) |
| 0x1b | `base_strength: [u8;3]` | MANA.DAT strength values |
| 0x1e | `strength_1e: [u8;3]` | `random(0x2d,0x37)`, minus 5 for clubs with `i > 64` |
| 0x21 | `crest: u8` | `(crest + 0x30) & 0x7f`. Bit 7 is a run-time flag |

### 3.3 `Player`: FAR:57dd + p*0x25, 37 bytes (block 17)

The table is generated in `mana_load_player_names` 422b:035c (`FUN_422b_035c` in Ghidra). Index 0 is
unused and players are 1..150. The `position_value` bands are hard-coded loops (not DS:07a7): 1..0x14
-> 0, 0x15..0x3f -> 25+r, 0x40..0x69 -> 50+r, 0x6a..0x96 -> 75+r (r = `random_range`). DS:07a7[4] holds
the upper index of each of the 4 groups and is used only when dealing players to managers: per manager
and group g, player `DS:07a7[g] - counter[g] + 1` is assigned and `counter[g]` is decremented (players are
dealt downwards from the top of each group). FUNCTION_MAP still calls 422b:035c `savegame_write_all`;
the decompile shows the player-table generator.

| Off | Field | Meaning |
|---|---|---|
| 0x00 | `name: [u8;26]` | Name |
| 0x1a | `age: u8` | Age in years, random at generation (transfer.md §2.1) |
| 0x1b | `average_skill: u8` | `(skills[0] + skills[1] + skills[2]) / 3` (unsigned, truncating) |
| 0x1c | `skills: [u8;3]` | `10 - (random - base)` derived values |
| 0x1f | `position_value: u8` | Band by index group (see above) |
| 0x20 | `attr_20: u8` | Random attribute |
| 0x21 | `owner_manager: u8` | Manager index. 5 = none (`NO_OWNER`) |
| 0x22 | `counters: [u8;2]` | League goals and league games this season (`screen_top_scorers`, `match_assign_ai_scorers`) |
| 0x24 | `team: u8` | Owner's club (`Manager::team`). 0xff = none if FAR:513c > 2000 |

### 3.4 `TeamTable`: FAR:0ecc + t*0x36, 54 bytes (block 16), t = German club 0..63

The record is updated per match in 3a41:2d33. Each 4-byte group is `[home, away, saved home, saved away]`;
3a41:1ef4 copies slot a to slot b.

| Off | Field | Meaning |
|---|---|---|
| 0x00 | `points: [u8;4]` | A win adds DS:2271 (2 or 3), a draw adds 1 |
| 0x04 | `form: [[u8;9];2]` | Last 8 results as `S`/`U`/`N`, lower case = home. `@` = none (placeholder from `mana_load_club_table`). Row 1 is a saved copy |
| 0x16 | `goals_for: [u8;4]` | Goals scored |
| 0x1a | `goals_against: [u8;4]` | Goals conceded |
| 0x1e | `played: [u8;4]` | Matches played. 3a41:2d33 adds 1 to `played[0]` of the home club and `played[1]` of the away club in every match. Second sort key (fewer ranks higher), skipped when DS:224d != 0. Draws = `played - wins - losses` |
| 0x22 | `unknown_22: [u8;4]` | |
| 0x26 | `wins: [u8;4]` | Wins |
| 0x2a | `losses: [u8;4]` | Losses. Draws are not stored |
| 0x2e | `table_position: u8` | Current 0-based position inside the club's tier, rewritten after every table sort in 3a41:2d33 (3a41:3769). Read by `season_end` for the rank bonus |
| 0x2f | `unknown_2f: [u8;3]` | |
| 0x32 | `prestige: i32` | Added at `season_end` 2a58:4597: `(#tiers above + ...)*0x19 - rank + 0x13` for clubs `<= DS:2277`, plus 20/30/40/50 for the competition winners DS:07ac/07af/07ae/07ad, plus 12 or 3 for top ranks |

Table sort (3a41:2d33, 3a41:33d0..353f). Parameter `view` (3rd argument): 0 = home slot only, 1 = home +
away, 2 = away slot only (slot 0 is summed when `view < 2`, slot 1 when `view != 0`). For each pair
`i < j` of `league_teams[tier][..DS:225e[tier]]` the entries are swapped when club `j` ranks strictly
higher than club `i`. The keys, in order:

1. points, more is better (unsigned 16-bit sums);
2. matches played (`+0x1e`), **fewer** is better; both values are forced to 0 when DS:224d != 0;
3. goal difference `goals_for - goals_against`, more is better (signed 16-bit compare);
4. goals scored, more is better.

Clubs equal in all four keys are not swapped. The sort is a plain exchange sort (not stable), so the
prior order of tied clubs matters. After the sort `table_position` is rewritten for the tier, and then
every manager's `position_history` is updated (§3.1). The whole function only runs when
`(i8)(DS:225a[tier] - 1) <= DS:2262[tier] - 1` and `tier < 3`.

### 3.5 `SquadPlayer`: FAR:774a + (s*25+i)*0x34, 52 bytes (block 18)

| Off | Field | Meaning |
|---|---|---|
| 0x00 | `cards: [u8;3]` | Red cards, yellow cards, unused (squad.md §1.2); a value term of `player_value_calc` |
| 0x03 | `goals: [u8;3]` | Goals in league, DFB-Pokal, Europapokal. In market records `goals[0]` doubles as the "bid rejected" mask (transfer.md §2.1) |
| 0x06 | `appearances: [u8;3]` | Appearances in league, DFB-Pokal, Europapokal |
| 0x09 | `status: u8` | Bits 0-1 availability (0 available, 1 suspended, 2 injured, 3 unavailable); bits 6-7 transfer flags |
| 0x0a | `lineup_position: u8` | Lineup position. 0 = not in the lineup, 1..11 = starting eleven (each used once), 12..15 = bench. `squad_count_by_flag(squad, mode)` 4091:1109 scans 24 slots (12 for squad 4): mode 0 counts `player_id != 0`, mode 1 counts `1 <= v < 12`, mode 2 counts `v >= 12` |
| 0x0b | `contract_years: u8` | Contract length in seasons |
| 0x0c | `loan: u8` | Loan marker; `0x63` (`LOAN_PRICE_MODE`) selects price mode 3 of `player_value_calc` |
| 0x0d | `suspended: u8` | Remaining suspension games |
| 0x0e | `tendency: u8` | Tendency 35..65 (`player_init_random_attrs` 3277:000a) |
| 0x0f | `player_id: u8` | Index into `players`. 0 = empty; the slots are zeroed in 422b:035c |
| 0x10 | `skills: [u8;3]` | Random on a new game, mirrored into `Player::skills` |
| 0x13 | `exhaustion: u8` | Exhaustion, `random_range(80,120)` on insert |
| 0x14 | `traits: u8` | Bit field from `player_init_random_attrs` (bits 0-3, 4-5, 7) |
| 0x19 | `board_col: i8` | Tactics-board column 0..7, -1 (`BOARD_UNPLACED`) = not placed |
| 0x16 | `sale_club: u8` | Club the player moves to when sold |
| 0x18 | `negotiation: u8` | 0 willing, 1..99 cool-down, 100..127 insists on `value - 100` seasons, bit 7 retires (transfer.md §2.1) |
| 0x1a | `board_row: i8` | Tactics-board row 0..7 (0 = attack, 7 = goal) |
| 0x1c | `value_terms: [u16;3]` | Value terms weighted 50/20/30 by `player_value_calc` |
| 0x28 | `market_value: i32` | Monthly salary in squads 0..3, asking fee in the market (`player_value_calc` 3277:25de) |
| 0x30 | `message_ptr: u32` | Far pointer to a message. The loader patches it through the string table |

The other bytes are kept as `unknown_*`.

### 3.6 `MatchHistory`: FAR:9336, 0x1394 bytes (block 35)

`startup_clear_state` clears the whole block. The parts are:

| FAR | Field | Layout / meaning |
|---|---|---|
| 9336 | `recent_results` | `[manager 4][club 64][5][2]` last five results, `goals_a*16 + goals_b`, 0xff = none. 3a41:2073 appends and shifts. `startup_clear_state` fills 0xff |
| 9d36 | `team_series` | `[club 64][7][3]` series counters (incremented/reset per result in 3a41:2d33) |
| a276 | `manager_series_best` | `[manager 4][7][3]` maximum of `team_series` for the manager's club |
| a2ca | `record_scores` | `[club 64][8]` record results (high nibble/low nibble goals). Updated by 3a41:37cf (`FUN_3a41_37cf`) when a new result beats the stored one by `w1*a - w2*b` |
| a4ca | `record_opponents` | `[club 64][8]` opponent of each record. 1310:0f44 swaps two club ids in it |

## 4. Ghidra labels set

The following labels were added in `/unpacked/BMMAIN_unpacked.EXE`: `interest_rates`, `message_counts`,
`ticker_slots`, `current_manager`, `saved_manager`, `lineup_state`, `league_results`, `manager_count`,
`managers`, `rescheduled_matches`, `clubs`, `team_tables`, `players`, `squads`, `matchday_pairings`,
`day_of_month`, `month`, `year`, `day_counter`, `start_year`, `league_teams`, `match_history_*` (5 parts),
`manager_contracts`, `difficulty_level`, `scenario`, `season_week`, `week_competitions`, `logged_rounds`,
`options`, `cup_pairings` and `cup_results`. Each label is at the segment:offset given in §2.

## 5. Important non-persistent globals

These globals are not saved. They are reinitialized at start-up or by game logic. Sources: the
xrefs/decompilations above and MEMORY_MAP.

| Address | Suggested name | Meaning | Evidence |
|---|---|---|---|
| DS:0630 | `base_interest_rate` | `int` random walk (clamped 2..7, initial 5) from which DS:060e is derived. `GameState::base_interest_rate` | 212a:000a |
| DS:0644[4] | `month_balance_sum` | `long` sum of each manager's daily balances in the current month. `GameState::month_balance_sum` | 212a:0a6d, 24a4:2636 |
| DS:5256 | `quarter_flag` | Set at the end of months 0, 4, 8. `GameState::quarter_flag` | 212a:0a6d |
| DS:07a7[4] | `player_group_bounds` | Index bounds of the 4 player groups in FAR:57dd | 422b:035c |
| DS:0286[5], DS:028c[4] | `training_defaults` | Initial values for `Manager::training_141/146` | 18bc:2184 |
| DS:2271 | `points_per_win` | Points for a win (2 or 3). `< 3` means the 2-point display | 3a41:2d33 |
| DS:224c | `season_start_offset` | `runtime::SEASON_START_DAY`. Season end at `DS:07dc > DS:224c + 0x142` | 2a58:3176 |
| FAR:56f4 | `modern_start` | 1 = the game starts in 1992 (options dialog, "Histor. Start" off). `Runtime::modern_start` | 18bc:2184 |
| DS:943c[4] | `free_group_records` | Records each human squad of a new game takes from the top of a player group; counted down. A static in the original, so a second new game continues from there; the port copies it into a local of `newgame_init_player_pool`, so it restarts from the image on every call | 422b:035c |
| DS:224d | `ignore_matches_played` | When != 0 the "matches played" sort key (second key) is skipped | 3a41:2d33 |
| DS:224e/2250, 2252/2254 | `option_long_a/b` | Game-option longs compared together in the start menu | 18bc:2184 |
| DS:225e[3] | `tier_team_count` | Number of clubs per league tier (bound of the table sort) | 3a41:10e3, 3a41:2d33 |
| DS:2262[3] | `tier_round_count` | Number of league rounds per tier: 3a41:2d33 only runs while `DS:225a[tier] - 1 <= DS:2262[tier] - 1` (MEMORY_MAP calls it offsets) | 3a41:2d33 |
| DS:226e/226f/2270 | `tier_bounds` | League tier boundaries (`league_of_rank` 27c2:1bb5) | |
| DS:2277 | `last_ranked_club` | Highest club index that takes part in the leagues | 2a58:4597, 18bc:2184 |
| DS:227a..227f | `form_letters` | Result letters written into `TeamTable::form` | 3a41:2d33 |
| DS:05ac | `copy_protection_ok` | Copied to `ds_05fa[0]` on save | 422b:07fe |
| DS:05ae | `save_id_count` | Number of entries in FAR:a6d8 | savegame.md |
| DS:05d4 | `abort_requested` | Abort flag of the wait loops | MEMORY_MAP |
| DS:079c | `number_format_mode` | Number-format/picture-loading flag set before `format_long_thousands` | 3a41:0f0e |
| DS:9440 | `save_io_error` | Disables the cipher I/O after an error | savegame.md |
| DS:9720/9722 | `rand_state` | MSC `rand()` state. It is **not** saved, so loading a save does not restore the RNG | FUNCTION_MAP §3.4 |
| DS:9438[4]? | – | Group counter used by `mana_load_player_names` (`-0x6bc8`). The segment is uncertain | 422b:035c |
| FAR:5524[4*20] | `message_strings` | Far pointers of the per-manager message strings (rebuilt from the string table) | 4091:* |
| FAR:a6d8[50] | `save_id_history` | Session history of loaded/saved ids | 422b:07fe |
| FAR:57cf / 56dc | `save_key` / `save_checksum` | Cipher state | savegame.md |
| FAR:2e74 | `cup_team_list_len` | Length of FAR:56fe | MEMORY_MAP |
| FAR:2e98 | `match_count` | Number of matches in the current match-day buffer (FAR:4bd0, 57a6, DS:0170, 549c) | 1310:0ae8 |
| FAR:2ea2, 3046/3048 | `mouse_event`, `button_latch` | Input state | MEMORY_MAP |
| FAR:2f28/2f2a | `main_rastport` | Rastport used by all drawing | MEMORY_MAP |
| FAR:4b50 | `current_palette` | | MEMORY_MAP |
| FAR:53a0[20] | `ui_buttons` | Registered buttons | MEMORY_MAP |
| FAR:56f4 | `scenario_flag` | Selects the 0x7ae vs 0x7cb branch of the scenario tests | 18bc:2184 |
| FAR:a306.. | `number_buffer` | `format_long_thousands` output | MEMORY_MAP |

## 6. Discrepancies with other documents

* Block 49 is **FAR:016e**, not DS:016e as MEMORY_MAP §2.3 / FUNCTION_MAP §3.2 say. The save code
  (block table) and every access in `globals.tsv` use segment 5238.
* DS:07ab and DS:0618 are the string-table group count and sizes in savegame.md. In the code they are
  the manager count and the per-manager message count. Both descriptions are correct.
* The `savegame.rs` doc comments still describe FAR:57dd as "player table" and say club and player field
  widths are inferred from data. Club field widths are now confirmed by MANA.DAT loading and 3a41:1fec;
  player fields by 422b:035c.
* `savegame.rs` lacks nothing needed here. `SaveGame` exposes `blocks`, `strings` and `tail`, which is
  sufficient for the typed model.

## 7. Open questions

* `Manager` bytes 0x01f..0x03d, 0x106..0x109, 0x14a..0x15d and 0x1c4..0x1db have not been
  traced, and the meaning of most of 0x10a..0x146.
* Which competition each of DS:07ac..07af is (inferred only from the prestige bonus order).
* Blocks 30, 31, 36..40, 44, 46, 51..60 have only been located (users known); their meaning is not
  derived.
* Player `attr_20` (contract?).
* `TeamTable::unknown_22` (+0x22) is not touched by 3a41:2d33; its meaning is open.
