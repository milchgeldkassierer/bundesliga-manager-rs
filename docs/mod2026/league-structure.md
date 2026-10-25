# League structure inventory

Every place in the Rust port (the game and the team editor) and in `BMMAIN.EXE` and `TEAMED.EXE`
that hard-codes the league structure of the original: 18 / 20 / 20 clubs, 34 / 38 / 38 match
days, the club ids 0..17, 18..37 and 38..57, the 58 league clubs, the 64 German club slots, and
the relegation and play-off rules. R1 of [the PRD](PRD.md) changes these values; this list is what
that change has to touch (issue #176).

**Status.** The list records the port before #186. Since #186 the game crates read the league
structure from one owner, `bmp_game::league_layout` (`ORIGINAL` / `MOD2026`): the constants named
below (`LEAGUE_*`, `league_team_ids`, `TIER_*`, `TIER_BASE`, `TIER_RANGE`, `RELEGATION_PLACES`,
`RELEGATION_PLAYOFF`, `HAS_RELEGATION_PLAYOFF`, `PROMOTION_PLACES`, `DFB_LAST_TEAM`) are gone, and
the `file:line` pointers have moved. The `history_graph_lines` height and the
`highscore_compute_score` immediates read the owner too. Open are the team editor's rows
(`crates/bmp/src/teamed/`).

## How the list was built

- **Grep.** `grep -rnE '\b(17|18|19|20|34|37|38|57|58|64)\b' crates/bmp-game/src crates/bmp/src/game`
  returned 793 matches. Each one was read in its context. The ones that encode a league size are
  the rows marked `grep` below; the rest (save block numbers, RNG ranges, pixel coordinates,
  player counts, match minutes, test values that only happen to equal these numbers) are not
  league structure.
- **Table readers.** Most of the port does not repeat the numbers: it reads them from the
  constants and `Runtime` fields that mirror the DS tables below. Every use of `LEAGUE_*`,
  `TIER_*`, `tier_*`, `RELEGATION_*`, `PROMOTION_PLACES`, `EURO_PLACES`, `UEFA_PLACES`,
  `DFB_LAST_TEAM` and `POINTS_PER_WIN` (now `LeagueLayout::points_per_win`) in `crates/` is listed as a `reader` row by function.
- **Hex and fixed indices.** `0x39`, `0x3f`, `0x40`, `[u8; 60]`, row 114 and the play-off ranks
  `[0][15]` / `[1][2]` are league values the decimal grep cannot see (`hex` and `index` rows).
- **Team editor.** The team editor (`crates/bmp/src/teamed/`, `TEAMED.EXE`) keeps its own copies
  of the sizes and is outside the grep roots. Its sites come from reading
  `grep -rnE '\b(17|18|19|20|34|37|38|57|58|64)\b|0x11|0x25|0x39|GROUP_|_group' crates/bmp/src/teamed`;
  its Ghidra addresses are `TEAMED.EXE` addresses, not `BMMAIN.EXE` ones. The other directories
  of `crates/bmp/src` (`audio`, `compare`, `gfx`, `host`, `intro`, `main`, `video`) have no
  league-size site.
- **Tests.** Grep, reader and index sites inside the `#[cfg(test)]` modules are not repeated in the
  category tables; `Tests that assert the original sizes` lists them once per test module.
- **Original.** All addresses except the team editor's are Ghidra addresses in
  `/unpacked/BMMAIN_unpacked.EXE`. The data values were read from DGROUP `5cb3`; the instructions
  that read them come from the access list `out/funcmap/globals.tsv` (`docs/re/MEMORY_MAP.md`),
  checked in Ghidra where a row depends on it.
  A Ghidra column of the form `seg:off (DS:xxxx at seg:off)` names the original function and the
  instructions in it that read the table.

`FAR:2260` is not `DS:2260`. `DS:2260` (DGROUP) is the team count of tier 2. `FAR:2260` (far data
segment `5238`) is the team-id byte of the human manager records (`FAR:2260 + 0x30a * m`); every
`2260` access in `manager_index_of_team` 1310:32b1, `team_swap_slots` 1310:0b24 and
`match_minute_card_injury_events` 1310:2ee5 is that manager field.

## Original data tables

The game keeps its league structure in one block of DGROUP. Tier 0 is the Bundesliga, tier 1 the
2. Bundesliga, tier 2 the Amateur-Oberliga. The constants that mirror a table are listed here once;
the category sections below list only the sites that use them.

| Category | Rust location | Function | Ghidra address | Constant(s) | Meaning |
|---|---|---|---|---|---|
| League order lists | `crates/bmp-game/src/league.rs:17`, `crates/bmp-game/src/runtime.rs:18` | `LEAGUE_TEAM_COUNT`, `TIER_COUNT` | DS:225e, DS:225f, DS:2260 | 18, 20, 20 | clubs per tier (`DS:225e[tier]`); DS:2260 is the tier-2 count, read directly only in the 58 sum at 2a58:3f0f |
| Fixture generator | `crates/bmp-game/src/league.rs:27`, `crates/bmp-game/src/runtime.rs:20` | `LEAGUE_ROUND_COUNT`, `TIER_ROUNDS` | DS:2262, DS:2263, DS:2264 | 34, 38, 38 | match days per tier; DS:2264 is the tier-2 entry, reached as `DS:2262[2]` |
| Season end | `crates/bmp-game/src/cups.rs:66`, `crates/bmp/src/game/tables.rs:43` | `RELEGATION_PLACES` | DS:2266, DS:2267, DS:2268 | 3, 4, 4 | clubs relegated per tier; Bundesliga 2 direct plus the play-off place |
| Tables | `crates/bmp/src/game/tables.rs:41` | `PROMOTION_PLACES` | DS:2265 + L | 0, 3, 4 | promotion places of tier L, the relegation places of the tier above |
| Season end | `crates/bmp-game/src/cups.rs:68`, `crates/bmp-game/src/finance.rs:638` | `HAS_RELEGATION_PLAYOFF`, `RELEGATION_PLAYOFF` | DS:226a, DS:226b, DS:226c | 1, 0, 0 | the tier has a relegation play-off |
| Cup qualification | `crates/bmp-game/src/cups.rs:64`, `crates/bmp/src/game/tables.rs:39` | `UEFA_PLACES`, `EURO_PLACES` | DS:226d | 4 | UEFA-Cup places of the Bundesliga |
| League order lists | `crates/bmp-game/src/league.rs:15`, `crates/bmp-game/src/runtime.rs:16` | `LEAGUE_FIRST_TEAM`, `TIER_FIRST` | DS:226e, DS:226f, DS:2270 | 0, 18, 38 | first club id per tier; DS:226f/DS:2270 are the tier bounds of `league_of_rank` 27c2:1bb5 |
| Tables | `crates/bmp-game/src/league_layout.rs:35` | `LeagueLayout::points_per_win` | DS:2271 | 2 | points for a win (3 in the mod, R1b) |
| League order lists | `crates/bmp/src/game/scorers.rs:54`, `crates/bmp-game/src/runtime.rs:58` | `TIER_RANGE`, `Runtime::tier_club_ranges` | DS:2272..DS:2277 | 0..17, 18..37, 38..57 | first and last club id per tier |
| Cup qualification | `crates/bmp-game/src/cups.rs:60` | `DFB_LAST_TEAM` | DS:2277 | 57 | last league club id, also the top of the DFB-Pokal draw |
| Season end | `crates/bmp-game/src/cups.rs:832`, 835 | `season_promotion_relegation` | DS:2278 | 58 | first non-league club id (58..63 are the non-league German clubs); the port computes it as the sum of the tier counts |
| Fixture generator | `crates/bmp-game/src/league.rs:57` | `PAIRINGS` | DS:06fc, DS:0744 | 17, 19, 37 | round permutations: strings 1..17 for 18-club tiers, 19..37 for 20-club tiers |
| League order lists | `crates/bmp/src/teamed/club.rs:20`, 23 | `GROUP_FIRST`, `GROUP_RANKS` | TEAMED.EXE DS:0338, DS:033c | 0, 18, 38, 58 / 18, 20, 20, 20 | team editor: first club and highest rank per league group (group 3 = clubs 58..63) |
| Tables | `crates/bmp-game/src/strength.rs:36` | `ORIGINAL` | TEAMED.EXE DS:0554, DS:033c | 70, 34, 20 / 18, 20, 20 | team editor: base strength and clubs per league per group of the strength recomputation (rank term `size - rank`) |
| League order lists | `crates/bmp-game/src/state.rs:785` | `GameState::league_teams` | FAR:535a | 20 | order list `[tier][20]`; the stride 20 is the largest tier |
| Fixture generator | `crates/bmp-game/src/state.rs:719` | `GameState::league_results` | FAR:6ddc | 38, 20 | results `[tier][38][20]`; stride of 38 rounds and 20 slots |

Before #186 the same values were defined more than once in the port: `league.rs` (`LEAGUE_*`),
`runtime.rs` (`TIER_*` and the `Runtime` copies), `cups.rs` and `tables.rs`
(`RELEGATION_PLACES`), `finance.rs` (`RELEGATION_PLAYOFF`), `highscore.rs` (`TIER_BASE`) and
`scorers.rs` (`TIER_RANGE`). The game crates now read `league_layout` (the `Runtime` fields are
its run-time copy). Duplicates remain only in the team editor, `teamed/club.rs` (`GROUP_FIRST`,
`GROUP_RANKS`) and `teamed/mod.rs` (`league_group`), and in its strength recomputation in
`bmp-game` `strength.rs` (`ORIGINAL`, and the generator's `MOD2026`, which repeats the slot table
of `clubs.json`).

## League order lists

| Category | Rust location | Function | Ghidra address | Constant(s) | Meaning | Found by |
|---|---|---|---|---|---|---|
| League order lists | `crates/bmp-game/src/ai.rs:248` | `league_strength_drift` | 1f9d:0697 | 64 | 64 German club slots iterated by the strength drift | grep |
| League order lists | `crates/bmp-game/src/cups.rs:757`, 758 | `league_shuffle_teams` | 1310:09c5 (DS:225e at 1310:0a29, 0a44, 0ad3; DS:226e at 1310:0ac7) | 18, 20, 20 | new-game shuffle of each tier's order list FAR:535a inside the tier's id range | reader |
| League order lists | `crates/bmp-game/src/finance.rs:554` | `season_goal_messages` | 212a:314d | 19 | rank index clamped to last slot 19 of league_teams row | grep |
| League order lists | `crates/bmp-game/src/league.rs:21` | `league_team_ids` | DS:226e / DS:225e (DS:2272) | 0..18, 18..38, 38..58 | id range of a tier = first id + team count | reader |
| League order lists | `crates/bmp-game/src/newgame.rs:322` | `mana_team_player_name_ptr` | 422b:01d0 | 64 | 64 German club records in MANA.DAT (GERMAN_CLUBS) | grep |
| League order lists | `crates/bmp-game/src/newgame.rs:347` | `mana_pick_unused_player_name` | 422b:022e (DS:225e at 422b:0260) | 18 | club of a named player drawn from the Bundesliga ids 0..DS:225e - 1 | reader |
| League order lists | `crates/bmp-game/src/newgame.rs:613`, 683, 684 | `init_manager` / `draw_tier_slot` | 18bc:2184 (DS:225e at 18bc:343e, 3b48; DS:226e at 18bc:344a, 3b54; DS:2277 at 18bc:3b1d) | 0..17, 18..37, 38..57 | start club of a new manager drawn inside the chosen tier | reader |
| League order lists | `crates/bmp-game/src/runtime.rs:52`, 54, 56, 140, 142, 143, 144, 145 | `Runtime::tier_bounds` / `tier_team_count` / `tier_round_count` / `tier_club_ranges` | DS:226f / DS:2270 / DS:225e / DS:2262 / DS:2272 | 18, 38; 18, 20, 20; 34, 38, 38; 0..17, 18..37, 38..57, 58..255 | run-time copies of the DS tables, built from TIER_FIRST/TIER_COUNT/TIER_ROUNDS; most game logic reads these | reader |
| League order lists | `crates/bmp-game/src/runtime.rs:190`, 203 | `league_of_club` / `tier_of_club` | 27c2:1bb5 (DS:226f at 27c2:1bc3, DS:2270 at 27c2:1bd6) | 18, 38 | tier code 1/2/4 by signed compare against the first ids of tiers 1 and 2; tier index from DS:2272 ranges | reader |
| League order lists | `crates/bmp-game/src/season.rs:878` | `match_gate_receipts_and_aftermath` | 2a58:20b2 (DS:2272/2273 at 2a58:20ff, 2106, 212a, 2131) | 0..17, 18..37, 38..57 | home and away tier of a match from the DS:2272 ranges | reader |
| League order lists | `crates/bmp-game/src/season.rs:1110` | `news_record_start` | 2a58:20b2 | 64 | team_tables bound of 64 German club slots | grep |
| League order lists | `crates/bmp-game/src/season.rs:2643` | `season_rebalance_player_clubs` | 1cb5:2756 (DS:2272 at 1cb5:27e6, DS:2273 at 1cb5:2804) | 0..17, 18..37, 38..57 | pool players counted per tier by the DS:2272 id ranges (plus the non-league range) | reader |
| League order lists | `crates/bmp-game/src/season.rs:2712`, 2722 | `pick_random_free_team_in_tier` | 1cb5:2987 (DS:2272 at 1cb5:29a8, DS:226e at 1cb5:2a1a) | 0..17, 18..37, 38..57 | random free club of a tier for a pool player (new game, season rebalancing) | reader |
| League order lists | `crates/bmp-game/src/state.rs:159` | `GERMAN_CLUBS` | FAR:0ecc (64 TeamTable records) | 64 | German club slots (bmp-formats mana::GERMAN_CLUB_COUNT); ids 58..63 are the non-league clubs | reader |
| League order lists | `crates/bmp-game/src/state.rs:396` | `Club::strength_1e` | FAR:3084 | 64 | clubs >= 64 (non-German) get minus 5; German club bound | grep |
| League order lists | `crates/bmp-game/src/state.rs:784` | `GameState::league_teams` | FAR:535a | 20 | doc: league order list [tier][20] slots per tier; array dimension = max clubs per tier | grep |
| League order lists | `crates/bmp-game/src/state.rs:1407`, 1429 | `manager_index_of_team` / `team_swap_slots` | 1310:32b1 / 1310:0b24 (FAR:2260 at 1310:32d1, 3301, 0b94, 0b9e, 0bc4) | 64 | FAR:2260 here is the manager record (team id), not the DS:2260 team count; team_swap_slots bounds clubs by GERMAN_CLUBS | reader |
| League order lists | `crates/bmp-game/src/state.rs:1598` | `team_swap_in_table_a4ca` | 1310:0f44 | 64 | record opponent table per German club slot (64) | grep |
| League order lists | `crates/bmp-game/src/stats.rs:229`, 230, 231 | `mana_load_club_table` | 362f:36ec | 18, 38, 58, 64 | 64 German club slots get crests and form; clubs above id 64 (foreign) get lower strength; initial league order lists per tier | grep |
| League order lists | `crates/bmp-game/src/stats.rs:249`, 259 | `mana_load_club_table` | 362f:36ec (DS:225e at 362f:38c6; CMP 0x40 at 362f:3791, 3810, 38a0) | 64 (0x40); 18, 20, 20 | strength penalty for clubs above 0x40; initial order lists from tier_team_count | reader |
| League order lists | `crates/bmp-game/src/stats.rs:285` | `pick_team_for_player` | 24a4:24bf | 0x3f, 0x40 | German club ids 0..0x3f vs foreign 0x40..0xc7 | hex |
| League order lists | `crates/bmp-game/src/transfer.rs:84` | `player_value_calc` | 3277:25de (CMP 0x3f at 3277:295d) | 0x3f | sale club above 0x3f (foreign) fixes the offer at 130 % | hex |
| League order lists | `crates/bmp-game/src/transfer.rs:246`, 250 | `refill_market_slot` | 3277:1e38 (DS:226e at 3277:1eda, DS:225e at 3277:1eeb, DS:2272/2273 at 3277:1f0c, 1f14) | 0..17, 18..37, 38..57 | a market player whose club a human manager runs gets a new club drawn from the same tier's id range | reader |
| League order lists | `crates/bmp/src/game/cup_screens.rs:41`, 325 | `FIRST_FOREIGN` / `screen_cup_matches` | 27c2:1ccb (CMP 0x40 at 27c2:21b9, 22d1) | 0x40 | German vs foreign club in the cup lists | hex |
| League order lists | `crates/bmp/src/game/live_match.rs:430` | `match_draw_live_screen_slots` | 2a58:042f (CMP 0x40 at 2a58:05a0, 083a) | 0x40 | crest picture only for German clubs | hex |
| League order lists | `crates/bmp/src/game/tables.rs:208`, 210 | `league_tier_of_team` | 27c2:1bb5 | 18, 38 | screen-side tier of a club from LEAGUE_FIRST_TEAM | reader |
| League order lists | `crates/bmp/src/game/tables.rs:973`, 984 | `screen_opponent_info` | 3a41:000e (CMP 0x3f at 3a41:015b, 0268) | 0x3f | German vs foreign opponent | hex |
| League order lists | `crates/bmp/src/teamed/mod.rs:81`, 82, 394 | `league_group` / `draw_panel` | TEAMED.EXE 1000:24be | 0x11, 0x25, 0x39 | league group of a German club `(i > 17) + (i > 37) + (i > 57)`, picks the league name in the club panel | hex |
| League order lists | `crates/bmp/src/teamed/club.rs:102` | `league_group` / `edit_panel` | TEAMED.EXE 12a5:056b | 0x11, 0x25, 0x39 | league group of the selected club for the panel edits | reader |
| League order lists | `crates/bmp/src/teamed/club.rs:187`, 192, 193 | `set_rank` | TEAMED.EXE 12a5:0e7a (DS:0338, DS:033c) | 18, 20, 20, 20 | rank input accepts 1..size of the group; in groups 0 and 1 the club holding the rank is found among the group's clubs and swaps | reader |

## Fixture generator

| Category | Rust location | Function | Ghidra address | Constant(s) | Meaning | Found by |
|---|---|---|---|---|---|---|
| Fixture generator | `crates/bmp-game/src/ai.rs:300` | `league_result_index` | FAR:6ddc | 38 | league_results row = tier*38 rounds + round - 1 | grep |
| Fixture generator | `crates/bmp-game/src/ai.rs:346`, 347 | `match_simulate_quick_result` | 1f9d:08e9 | 20, 38 | results row stride: comp*38 rounds per tier; results cell stride: 20 slots per round row | grep |
| Fixture generator | `crates/bmp-game/src/ai.rs:539` | `match_goal_chance` | 1f9d:0c3b | 20 | matchday_pairings row stride of 20 slots per tier | grep |
| Fixture generator | `crates/bmp-game/src/league.rs:54`, 55, 56 | `PAIRINGS` | DS:06fc | 17, 18, 19, 20, 37, 38 | round permutation ranges for 18- and 20-team tiers; 20-team tier permutations; empty string 18 separates the two permutation sets; pairing table size 2 x (half season + 1) | grep |
| Fixture generator | `crates/bmp-game/src/league.rs:106`, 107, 117 | `fixtures_build_round_pairings` | 3a41:10e3 (DS:225e at 3a41:1100, 11d5; DS:225f at 3a41:110d; DS:2262 at 3a41:111e, 113d; CMP 0x12 at 3a41:11e8) | 18, 20, 34, 38 | base id, half season and pair count of a tier; 18 selects the DS:06fc or DS:0744 permutations | reader |
| Fixture generator | `crates/bmp-game/src/league.rs:118`, 121 | `fixtures_build_round_pairings` | 3a41:10e3 | 18 | 18-team tier selects first permutation set; offset to 20-team permutation set | grep |
| Fixture generator | `crates/bmp-game/src/league.rs:268`, 269, 271, 274 | `round_results` | FAR:6ddc | 20, 37, 38 | league_results stride: 38 rounds x 20 slots; last round index of previous league; results per round (10 pairs); results per round slice length | grep |
| Fixture generator | `crates/bmp-game/src/league.rs:303` | `league_apply_round_and_sort` | 3a41:2d33 | 20 | 20 result bytes per round before FAR:6ddc | grep |
| Fixture generator | `crates/bmp-game/src/matchsim.rs:131` | `minute_probe` | 1310:12fe | 20 | league goal-minute table FAR:515c is 3 tiers x 20 slots x 8 | grep |
| Fixture generator | `crates/bmp-game/src/matchsim.rs:153`, 172 | `goal_minutes_generate` | 1310:12fe | 20 | goal-minute probe stride 20 slots per competition | grep |
| Fixture generator | `crates/bmp-game/src/matchsim.rs:197`, 199, 200, 203, 206, 207 | `league_result_cell` | FAR:6ddc | 20, 38 | flat results row = tier*38+round-1, 38 rounds per tier; round 0 reaches round 38 of previous tier; last valid round 38 of tier 2; row stride 38 rounds per tier; bound 3 tiers x 38 rounds of league_results; cell stride 20 slots per round row | grep |
| Fixture generator | `crates/bmp-game/src/matchsim.rs:887` | `league_slots` | 1310:2303 (DS:225e at 1310:23d7, 273a); 1310:15db (DS:225e at 1310:1893) | 18, 20, 20 | even pairing slots of a tier, slot < DS:225e[tier], used by the live and quick match days | reader |
| Fixture generator | `crates/bmp-game/src/matchsim.rs:1260` | `league_postpone_matches_weather` | 1310:0463 (DS:225e at 1310:05c3) | 9, 10 | last pair index = DS:225e[tier] / 2 - 1 | reader |
| Fixture generator | `crates/bmp-game/src/matchsim.rs:1448`, 1451 | `matchday_play_rescheduled` | 1310:07a2 | 19, 20 | results row of 20 slots; original clears only 19 cells; clears 19 of 20 result cells of a round row | grep |
| Fixture generator | `crates/bmp-game/src/runtime.rs:74` | `Runtime::league_goal_minutes` | FAR:515c | 20 | [tier][20 slots][goal] = fixture slots per tier | grep |
| Fixture generator | `crates/bmp-game/src/runtime.rs:155` | `Runtime::default` | FAR:515c | 20 | init of [tier][20 slots] goal minutes | grep |
| Fixture generator | `crates/bmp-game/src/season.rs:2137` | `season_final` | FAR:6ddc | 20, 38 | clear league_results[3][38][20] (rounds x slots per tier) | grep |
| Fixture generator | `crates/bmp-game/src/transfer.rs:319` | `season_left_percent` | 3277:2171 (DS:2262 also read by 3277:2270 at 3277:24c3 and 3277:2a8f at 3277:34f1) | 34, 38 | share of the season left, from the tier's round count | reader |
| Fixture generator | `crates/bmp/src/game/live_match.rs:336`, 338 | `match_draw_score_digit_pair` | 2a58:009d / FAR:6ddc | 20, 38 | league_results stride 38 rounds per tier, 3*38 bound; league_results stride 20 slots per round | grep |
| Fixture generator | `crates/bmp/src/game/tables.rs:660`, 665 | `screen_fixtures` | 3a41:120a (DS:2262 at 3a41:1368, 1b6f, 1b91; DS:225e at 3a41:1abf, 1bff) | 18, 20, 34, 38 | round range and pair count of the fixtures screen | reader |
| Fixture generator | `crates/bmp/src/game/tables.rs:848`, 851, 857, 858 | `round_result` | 3a41:120a / FAR:6ddc | 20, 38 | flat result table address: 20 slots, 38 rounds per tier; rounds per tier stride of league_results | grep |
| Fixture generator | `crates/bmp/src/game/tables.rs:1068`, 1115 | `screen_team_fixtures` | 1000:28c4 (DS:2262 at 1000:2917, 2c1e; DS:225e at 1000:2f8c) | 17, 19, 34, 38 | one half of a club's fixtures ("RESTPROGRAMM") | reader |

## Tables

| Category | Rust location | Function | Ghidra address | Constant(s) | Meaning | Found by |
|---|---|---|---|---|---|---|
| Tables | `crates/bmp-game/src/ai.rs:741`, 772 | `match_attendance_estimate` | 1f9d:11e0 | 34, 38; 18, 20 | attendance from rounds per tier and rank against team count | reader |
| Tables | `crates/bmp-game/src/league.rs:299`, 302 | `league_apply_round_and_sort` | 3a41:2d33 (DS:2262 at 3a41:2d49; DS:225e at 3a41:32fe, 3562, 3736, 3778) | 18, 20, 34, 38 | round gate and table sort bound | reader |
| Tables | `crates/bmp-game/src/league.rs:375` | `LeagueLayout::points_per_win` / `apply_result` | DS:2271 (3a41:2d33 at 3a41:2fcb, 31c4) | 2 | points for a win (3 in the mod, R1b done) | reader |
| Tables | `crates/bmp-game/src/newgame.rs:499` | `add_manager_squads` | FAR:234d | 38 | position history init, one entry per round (38) | grep |
| Tables | `crates/bmp-game/src/state.rs:251`, 257 | `Manager::position_history` | FAR:234d | 38 | position history holds one byte per round, 38 rounds max; array dimension = max rounds per tier (DS:2262) | grep |
| Tables | `crates/bmp-game/src/state.rs:1494` | `GameState::sort_league_table` | 3a41:2d33 | 18, 20 | exchange sort of league_teams[tier][..DS:225e[tier]] | reader |
| Tables | `crates/bmp-game/src/state.rs:1501` | `GameState::sort_league_table` | 3a41:2d33 | 20 | clamps sorted team count to 20 slots per tier | grep |
| Tables | `crates/bmp-game/src/state.rs:1563`, 1576 | `GameState::record_position_history` | 3a41:2d33 | 37 | last round index of position history; valid round indices 0..=37 of position history | grep |
| Tables | `crates/bmp/src/game/office.rs:171`, 172 | `manager_byte_10b` | 18bc:0b5f (curve 18bc:1297) | 37, 38 | position_history 38 bytes = max rounds per season; byte after the 38-round position history | grep |
| Tables | `crates/bmp/src/game/office.rs:466` | `office_menu_dispatcher` | 18bc:0b5f (DS:2262 at 18bc:0be1) | 34, 38 | builds the next round's pairings for every tier still inside its round count | reader |
| Tables | `crates/bmp/src/game/tables.rs:374`, 378 | `league_table_row_text` | 3a41:214c (DS:2271 at 3a41:291c, 2956, 29b8) | 2 | points column of the table row | reader |
| Tables | `crates/bmp/src/game/tables.rs:413`, 414, 884, 885 | `strength_order` / `screen_strength_table` | 3a41:3891 (DS:226e at 3a41:3ae8, 3e98, 3f67; DS:225e at 3a41:3b29, 3b7d, 3bdb, 3e83, 3f50) | 0, 18, 38; 18, 20 | strength table of a tier | reader |
| Tables | `crates/bmp/src/game/tables.rs:493` | `sort_table_clamped` | 3a41:000e | 34, 38 | opponent info sorts the table up to the last round | reader |
| Tables | `crates/bmp/src/game/tables.rs:538` | `screen_league_table` | 3a41:214c (DS:2262 at 3a41:24f4, 24ff, 2cea, 2cf5; DS:225e at 3a41:25d2, 2b61, 2c71) | 18, 20, 34, 38 | rows and round range of the table screen | reader |
| Tables | `crates/bmp/src/game/tables.rs:565` | `EURO_PLACES` / `screen_league_table` | DS:226d (3a41:214c at 3a41:259b) | 4 | European places marked in the Bundesliga table | reader |
| Tables | `crates/bmp/src/game/tables.rs:567` | `RELEGATION_PLACES` / `screen_league_table` | DS:2266 (3a41:214c at 3a41:25ed) | 3, 4, 4 | relegation places marked at the bottom of the table | reader |
| Tables | `crates/bmp/src/game/tables.rs:575` | `PROMOTION_PLACES` / `screen_league_table` | DS:2265 + L (3a41:214c at 3a41:2630) | 0, 3, 4 | promotion places of league L read as DS:2266[L - 1] | reader |
| Tables | `crates/bmp-game/src/strength.rs:36` | `ORIGINAL` / `recompute_strengths` | TEAMED.EXE 1579:006d (DS:0554, DS:033c) | 18, 38, 64 / 70, 34, 20 / 18, 20, 20 | group ends `(i > 37) + (i > 17)` (the amateurs and clubs 58..63 share group 2), per-group averages and the rank term `size - rank` of the strength recomputation when the editor saves | reader |

## Newspaper

None found.

```
grep -rnE 'LEAGUE_|TIER_|tier_|league_teams|table_position|RELEGATION|PROMOTION' crates/bmp-game/src/news.rs crates/bmp-game/src/news
```

The command returns nothing. The 70 matches of the decimal grep in the newspaper code are template, group and field indices, none of them a league size. Ranks and tiers reach the player as messages only through `season_goal_messages` and `season_end_messages` (see `Season end`).

## Cup qualification

| Category | Rust location | Function | Ghidra address | Constant(s) | Meaning | Found by |
|---|---|---|---|---|---|---|
| Cup qualification | `crates/bmp-game/src/cups.rs:261`, 263 | `cup_draw_first_round` | 27c2:09e0 | 57, 64 | DFB draw picks league clubs 0..57 (DS:2277); foreign club ids start after 64 German club slots | grep |
| Cup qualification | `crates/bmp-game/src/cups.rs:288` | `cup_draw_first_round` | 27c2:09e0 (DS:2277 at 27c2:0a44, 0a4b) | 57 | DFB-Pokal draws the league clubs 0..DS:2277 | reader |
| Cup qualification | `crates/bmp-game/src/cups.rs:399` | `euro_cup_qualifiers_build` | 27c2:0ef2 | 60 | league_teams flattened: 3 tiers x 20 slots | index |
| Cup qualification | `crates/bmp-game/src/cups.rs:400` | `UEFA_PLACES` / `euro_cup_qualifiers_build` | DS:226d (27c2:0ef2 at 27c2:0f02) | 4 | UEFA-Cup places taken from the Bundesliga table | reader |

## Statistics

| Category | Rust location | Function | Ghidra address | Constant(s) | Meaning | Found by |
|---|---|---|---|---|---|---|
| Statistics | `crates/bmp-game/src/highscore.rs:22` | `highscore_compute_score` | 422b:2864 / DS:226e | 0, 18, 38 | first club id per tier (DS:226e) for highscore position term | grep |
| Statistics | `crates/bmp-game/src/highscore.rs:25` | `highscore_compute_score` | 422b:2864 (DS:226e at 422b:2898) | 0x39 (57) | position term 57 - table position - first id of the tier | hex |
| Statistics | `crates/bmp-game/src/season.rs:720`, 738 | `season_end_eternal_points` | 2a58:4597 | 19, 64 | all-time table loop over 64 German club slots (0x40); eternal points base: last rank (19) of a 20-team tier scores 0 | grep |
| Statistics | `crates/bmp-game/src/season.rs:727`, 728 | `season_end_eternal_points` | 2a58:4597 (DS:226f at 2a58:45c2, 46c3; DS:2277 at 2a58:45fb, 4711; DS:2270 at 2a58:4745) | 18, 38, 57 | eternal-table points per tier at the season end | reader |
| Statistics | `crates/bmp-game/src/season.rs:807`, 814 | `season_record_manager` | 2a58:3176 (DS:2262 at 2a58:3909, DS:226e at 2a58:3975) | 34, 38; 0, 18, 38 | manager history: final rank and rounds of the season | reader |
| Statistics | `crates/bmp-game/src/season.rs:927` | `match_gate_receipts_and_aftermath` | 2a58:20b2 | 19 | max home games per season (38/2), home_attendance index | grep |
| Statistics | `crates/bmp-game/src/stats.rs:50`, 58 | `matchday_assign_ai_scorers` | 24a4:1662 (DS:2262 at 24a4:1677, DS:225e at 24a4:170e) | 18, 20, 34, 38 | round gate and pair count of the last round played | reader |
| Statistics | `crates/bmp-game/src/stats.rs:53` | `matchday_assign_ai_scorers` | FAR:6ddc | 20 | league_results row stride 20 slots per tier | grep |
| Statistics | `crates/bmp-game/src/stats.rs:195` | `top_scorers_rows` | 24a4:1ad5 (DS:2272 at 24a4:228e, DS:2273 at 24a4:22b9) | 0..17, 18..37, 38..57 | scorer list filtered by tier id range | reader |
| Statistics | `crates/bmp/src/game/scorers.rs:353` | `screen_top_scorers` | 24a4:1ad5 | 0..17, 18..37, 38..57 | tier filter of the top-scorers screen (TIER_RANGE) | reader |
| Statistics | `crates/bmp/src/game/stats_screens.rs:235` | `finance_needed_attendance` | 24a4:2b15 / DS:225e | 34 | fallback for teams-per-tier lookup (games = teams - 1) | grep |
| Statistics | `crates/bmp/src/game/stats_screens.rs:464` | `screen_statistics` | 362f:0af7 | 20 | home-attendance bytes, one per home game (max 19 with 20 teams) | grep |
| Statistics | `crates/bmp/src/game/stats_screens.rs:489`, 493, 494, 497 | `eternal_table_order` | 362f:18e7 | 64 | 64 German club slots in the eternal table; order array over 64 club slots; exchange sort bound: 64 club slots | grep |
| Statistics | `crates/bmp/src/game/stats_screens.rs:520` | `eternal_table_rows` | 362f:18e7 | 64 | order array over 64 club slots | grep |
| Statistics | `crates/bmp/src/game/stats_screens.rs:715` | `history_graph_lines` | 362f:390e | 58 | graph height = 58 league clubs (overall rank) | grep |
| Statistics | `crates/bmp/src/game/stats_screens.rs:727`, 730, 800 | `history_graph_lines` / `screen_manager_history` | 362f:390e (DS:2270 at 362f:3b03, DS:226f at 362f:3b13, DS:226e at 362f:3cd8) | 0, 18, 38 | overall rank = first id of tier + position | reader |

## Save games

| Category | Rust location | Function | Ghidra address | Constant(s) | Meaning | Found by |
|---|---|---|---|---|---|---|
| Save games | `crates/bmp-game/src/state.rs:298` | `Manager::home_attendance` | FAR:2242 | 20 | one slot per league home game; max home games = rounds/2 (19) | grep |
| Save games | `crates/bmp-game/src/state.rs:672` | `MatchHistory::recent_results` | FAR:9336 | 64 | per-club dimension = 64 German club slots | grep |
| Save games | `crates/bmp-game/src/state.rs:674` | `MatchHistory::team_series` | FAR:9d36 | 64 | per-club dimension = 64 German club slots | grep |
| Save games | `crates/bmp-game/src/state.rs:678` | `MatchHistory::record_scores` | FAR:a2ca | 64 | per-club dimension = 64 German club slots | grep |
| Save games | `crates/bmp-game/src/state.rs:680` | `MatchHistory::record_opponents` | FAR:a4ca | 64 | per-club dimension = 64 German club slots | grep |
| Save games | `crates/bmp-game/src/state.rs:717` | `GameState::league_results` | FAR:6ddc | 19, 20, 37, 38 | doc: round index 0..37, slot 0..19 per tier; array dims [tier][38 rounds][20 slots] | grep |
| Save games | `crates/bmp-game/src/state.rs:742`, 743 | `GameState::team_tables` | FAR:0ecc | 64 | table records of the 64 German club slots; array dimension = 64 German club slots | grep |
| Save games | `crates/bmp-game/src/state.rs:748` | `GameState::matchday_pairings` | FAR:4b5e | 10, 19 | block number 19; [tier][10] = max pairs per match day | grep |
| Save games | `crates/bmp-game/src/state.rs:978`, 988, 991, 1004 | `for_each_block` | FAR:6ddc / FAR:0ecc / FAR:4b5e / FAR:535a | 10, 19, 20, 38, 64 | block 6 league_results dims [3][38][20]; block 16 team_tables, 64 German club slots; block 19 matchday_pairings dim 10 = max pairs per match day; block 32 league_teams dim 20 = max clubs per tier | grep |

## Season end

| Category | Rust location | Function | Ghidra address | Constant(s) | Meaning | Found by |
|---|---|---|---|---|---|---|
| Season end | `crates/bmp-game/src/cups.rs:730` | `matchday_relegation_match` | 1310:02bf | 15, 2 | play-off pairing league_teams[0][15] (16th of the Bundesliga) vs league_teams[1][2] (3rd of the 2. Bundesliga) | index |
| Season end | `crates/bmp-game/src/cups.rs:781`, 827, 829 | `season_promotion_relegation` | 2a58:3d99 / FAR:535a | 20, 58 | reserve clubs 58..62 promoted into amateur tier (DS:2278); league_teams stride 20 for relegated club lookup; league_teams stride 20 for promoted club lookup | grep |
| Season end | `crates/bmp-game/src/cups.rs:800`, 822 | `season_promotion_relegation` | DS:225e (2a58:3d99 at 2a58:3dd5, 3e92, 4069) | 18, 20, 20 | licence withdrawal moves a club to the bottom of its tier; bottom slot of each tier | reader |
| Season end | `crates/bmp-game/src/cups.rs:812` | `RELEGATION_PLACES` / `season_promotion_relegation` | DS:2266 (2a58:3d99 at 2a58:3f5c, 404f, 40cd, 40d7) | 3, 4, 4 | clubs swapped between a tier and the one below (Bundesliga: 2 direct + play-off) | reader |
| Season end | `crates/bmp-game/src/cups.rs:832`, 835 | `season_promotion_relegation` | DS:225f + DS:2260 + DS:225e (2a58:3d99 at 2a58:3f0b, 3f0f, 3f14) | 58, 60, 61, 62 | n = 20 + 20 + 18 = 58 league clubs; the amateur tier swaps with reserve clubs n+3/n+4 (61, 62), then 60, 59, 58 | reader |
| Season end | `crates/bmp-game/src/cups.rs:845` | `HAS_RELEGATION_PLAYOFF` / `season_promotion_relegation` | DS:226a (2a58:3d99 at 2a58:3f7c) | 1, 0, 0 | a play-off the Bundesliga club won keeps its slot | reader |
| Season end | `crates/bmp-game/src/finance.rs:543`, 551, 552, 557, 560 | `season_goal_messages` | 212a:314d (DS:225e at 212a:317d, DS:226a at 212a:318a, DS:2262 at 212a:31bd, 3418, 34f8, DS:2271 at 212a:340a, 34ea) | 18, 20; 34, 38; 2; 1, 0, 0 | title, promotion, UEFA and staying-up goals from team count, rounds left, points per win and the play-off flag | reader |
| Season end | `crates/bmp-game/src/season.rs:2266` | `season_end_messages` | 1cb5:0012 (DS:226e at 1cb5:0ce2) | 0, 18, 38 | season-end messages: rank inside the tier = overall rank - first id of the tier | reader |
| Season end | `crates/bmp/src/game/office.rs:144` | `office_banner` | 18bc:0b5f | 15, 2 | office banner for the two play-off clubs (16th BL, 3rd 2. BL) | index |

## Tests that assert the original sizes

The `#[cfg(test)]` modules below build their fixtures from, or assert, the original sizes (for
example "club 20 is a 2. Bundesliga club", "18 Bundesliga clubs"). They are not sites the game
reads, so they are kept out of the category tables: one row per test module, the categories it
touches and the DS tables or original functions it mirrors. A change of a league size that keeps
the original data as the default leaves them valid; tests of the mod data need their own fixtures.

| Category | Rust location | Function | Ghidra address | Constant(s) | Meaning |
|---|---|---|---|---|---|
| League order lists, Tables, Season end | `crates/bmp-game/src/ai.rs:1175`, 1204, 1205, 1231, 1234, 1235, 1244, 1245, 1735, 1744, 1750, 1754, 1764, 1770, 1782, 1830, 1857, 1862, 1863, 1864, 1875, 1886, 1898, 2054 | `drift_weekly_mode`, `drift_reset_mode`, `attendance_tiers_ranks_and_caps`, `real_saves_attendance_respects_the_capacity` | 1f9d:0697, DS:2272, DS:2263, DS:2262, DS:225f | 18, 20, 34, 38, 64 | test fixture or assertion of the original value |
| League order lists, Cup qualification, Season end | `crates/bmp-game/src/cups.rs:1015`, 1018, 1025, 1028, 1031, 1032, 1155, 1172, 1180, 1183, 1224, 1568, 1581, 1700, 1704, 1725, 1726, 1727, 1730, 1731, 1734, 1749, 1759, 1761, 1762, 1764, 1765, 1767, 1768, 1774, 1775, 1777, 1778, 1779, 1919 | `amateur_home_swap_rule`, `table_state`, `dfb_first_round_draw`, `euro_draw_state`, `euro_first_round_draw`, `shown_semi_final_draw_gives_amateurs_home_advantage`, `identity_state`, `promotion_relegation_swaps_the_right_slots`, `real_saves_league_shuffle_keeps_managers_with_their_clubs` | DS:2270, DS:2272, DS:225e, DS:225f, DS:226f, DS:2277, 27c2:09e0, FAR:535a, 2a58:3d99, DS:2266, DS:2278, 1310:09c5 | 17, 18, 19, 20, 34, 37, 38, 57, 58, 64 | test fixture or assertion of the original value |
| League order lists, Season end | `crates/bmp-game/src/finance.rs:647`, 651, 653, 654, 655, 677, 679, 698, 699, 702 | `goal_state`, `a_lost_promotion_is_announced_once`, `secured_goals_are_announced_but_the_title_only_in_the_bundesliga` | DS:2272, DS:2263 | 18, 19, 20, 37, 38 | test fixture or assertion of the original value |
| League order lists, Fixture generator, Tables | `crates/bmp-game/src/league.rs:525`, 529, 531, 534, 598, 632, 645, 711, 712, 750, 751, 752 | `second_half_swaps_home_and_away`, `head_to_head_keeps_the_last_five`, `round_one`, `apply_round_updates_every_table`, `apply_round_logs_away_results_draw_streaks_and_positions` | 3a41:10e3, 3a41:2073, FAR:6ddc, 3a41:2d33 | 17, 18, 19, 20, 34, 64 | test fixture or assertion of the original value |
| League order lists, Fixture generator | `crates/bmp-game/src/matchsim.rs:2313`, 2460, 2462, 2463, 2464, 2466, 2493, 2494, 2495, 2496, 2499, 2526, 2529, 2531, 2534, 2586, 2617, 2972, 2976, 3011, 3012, 3026 | `T1_HOME`, `cells_of_flat_row`, `result_cell_and_round_results_share_the_flat_row_rule`, `round_zero_of_a_tier_applies_the_row_both_helpers_read`, `league_day_skips_postponed_fixtures_and_credits_computer_scorers`, `rescheduled_games_use_the_untruncated_round_base`, `match_day_on_real_saves_only_adds_goals_and_is_deterministic` | DS:226f, FAR:6ddc, DS:225e | 18, 19, 20, 37, 38, 114 | test fixture or assertion of the original value |
| League order lists, Tables | `crates/bmp-game/src/newgame.rs:929`, 930, 995, 1019, 1048, 1065, 1213, 1268, 1269 | `league_shuffle_stays_inside_the_tiers`, `player_pool_gives_each_manager_twenty_named_players`, `endless_games_leave_free_players_without_a_club`, `pool_assignment_in_year_zero_only_places_clubless_players`, `three_year_manager_writes_the_carousel_record`, `added_manager_gets_twenty_free_players` | DS:2278, DS:226f, 24a4:19fb, FAR:234d | 18, 38, 58, 64 | test fixture or assertion of the original value |
| League order lists, Fixture generator | `crates/bmp-game/src/runtime.rs:218`, 219, 220, 223, 231, 232, 233, 234 | `tier_tables_start_as_in_the_original`, `league_of_club_uses_the_tier_bounds` | DS:226f, DS:225e, DS:2272, DS:2262, 27c2:1bb5 | 17, 18, 20, 34, 37, 38, 57, 58 | test fixture or assertion of the original value |
| League order lists, Fixture generator, Statistics, Season end | `crates/bmp-game/src/season.rs:2740`, 2763, 3042, 3048, 4087, 4088, 4089, 4090, 4101, 4103, 4105, 4109, 4110, 4111, 4112, 4113, 4115, 4125, 4182, 4192, 4458, 4660, 4744, 4967 | `pick_never_returns_a_human_club`, `pick_draws_three_to_five_numbers_per_try`, `two_managers`, `eternal_points_by_tier_and_cup`, `eternal_points_of_a_low_rank_go_negative`, `season_record_manager_shifts_a_full_log`, `league_season`, `season_final_state`, `season_final_runs_its_callees_in_order`, `rebalancing_moves_nothing_without_free_places` | 1cb5:2987, DS:226e, 2a58:4597, 2a58:3176, FAR:535a, DS:2262, FAR:6ddc, 1cb5:2756 | 18, 19, 20, 34, 37, 38, 58 | test fixture or assertion of the original value |
| League order lists, Tables, Save games | `crates/bmp-game/src/state.rs:1782`, 1975, 2002, 2143, 2174 | `team_swap_slots_moves_every_reference`, `saved_index_past_its_table_is_rejected`, `real_saves_table_semantics` | 1310:0f44, FAR:535a, FAR:2242, DS:225e, DS:2262 | 18, 19, 38, 63, 64 | test fixture or assertion of the original value |
| League order lists, Statistics | `crates/bmp-game/src/stats.rs:671`, 676, 678, 886, 897, 921, 936, 1015, 1016, 1020, 1029, 1030, 1031, 1032 | `the_top_scorer_flag_needs_the_leagues_first_scorer`, `matchday_walks_the_last_round`, `top_scorer_rows_filter_by_tier_and_stop_below_two_goals`, `club_table_comes_from_mana` | DS:2272, FAR:6ddc, DS:226f, 362f:36ec, DS:225e, FAR:535a | 18, 20, 38, 58, 64 | test fixture or assertion of the original value |
| League order lists, Fixture generator | `crates/bmp-game/src/transfer.rs:914`, 920, 1626 | `state`, `refresh_lists_a_player_in_draw_order` | DS:2262, DS:2272 | 17, 34, 38, 57 | test fixture or assertion of the original value |
| Cup qualification | `crates/bmp/src/game/cup_screens.rs:764`, 801 | `quarter_final`, `dfb_list_groups_the_pairings_by_league` | 27c2:1ccb | 17 | test fixture or assertion of the original value |
| Statistics | `crates/bmp/src/game/stats_screens.rs:906`, 981, 982, 984, 986, 987, 988 | `eternal_order_and_rows`, `graph_bars_hang_from_the_rank` | 362f:18e7, 362f:390e | 19, 20, 57, 58, 64 | test fixture or assertion of the original value |
| Fixture generator, Tables | `crates/bmp/src/game/tables.rs:1348`, 1349, 1350, 1351, 1352, 1460, 1466, 1468 | `the_remaining_fixtures_show_one_half_of_the_rounds`, `strength_order_sorts_by_the_selected_parts` | 1000:28c4, DS:2262, 3a41:3891, DS:225e, DS:2272 | 17, 18, 19, 20, 34, 38 | test fixture or assertion of the original value |
| League order lists, Tables | `crates/bmp-game/src/strength.rs:194`, 195, 196, 197, 198, 227, 237 | `groups`, `shipped_strengths_reproduce`, `random_clubs_use_seven_calls_from_the_msc_start_state` | TEAMED.EXE 1579:006d | 17, 18, 37, 38, 63, 64 | test fixture or assertion of the original value |
| League order lists | `crates/bmp/src/teamed/club.rs:477`, 485 | `rank_swaps_within_the_bundesligas` | TEAMED.EXE 12a5:0e7a (DS:033c) | 19, 21 | test fixture or assertion of the original value |
| League order lists | `crates/bmp/src/teamed/mod.rs:555`, 556, 557, 558, 559, 560, 561, 562 | `league_groups` | TEAMED.EXE 1000:24be | 17, 18, 37, 38, 57, 58, 63 | test fixture or assertion of the original value |
