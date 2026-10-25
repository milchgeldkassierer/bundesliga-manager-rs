# How the original replaces the clubs relegated out of the amateur tier

PRD R1 ("Promotion and relegation as in 2026", 3. Liga bullet) asks how the original replaces
the clubs that go down out of its third group, the Amateur-Oberliga (team ids 38..57). This
document answers it for v1.0.0 with the original data. The rule is ported in
`season_promotion_relegation` (`crates/bmp-game/src/cups.rs`) and pinned by the test
`season_end_third_group_replacement_is_pinned`
(`crates/bmp-game/tests/season_relegation_replacement.rs`). The pseudo code is in
`docs/re/logic/cups.md` §9.1 and `docs/re/logic/season.md` (season end).

Short answer: nothing is generated. The four last-placed amateur clubs swap their whole slot
with four of the six non-league German clubs in slots 58..63: the last-placed club with slot 61
or 62 (one random draw), then 60, 59 and 58. The relegated clubs keep everything they had and
wait in those slots; they can come back in a later season the same way.

## Addresses

| Address | What |
|---|---|
| BMMAIN.EXE 2a58:3d99..40e2 | promotion and relegation block of `season_main_loop` 2a58:3176 (licence withdrawals first) |
| 2a58:3ee0 | start of the per-tier loop `for L = 0..2` (`MOV byte ptr [BP-0x34],0`) |
| 2a58:3f07..3f42 | the replacement slot of the amateur tier: `n = DS:225f + DS:2260 + DS:225e`, `random_range(n+3, n+4)` (call at 2a58:3f31) or `n + 3 - k` |
| 2a58:3f45..3f83 | the Bundesliga play-off slot test (`DS:2266`, `FAR:3060`, `DS:226a`) |
| 1310:0b24 | `team_swap_slots(a, b, 1)`, the record swap |
| 1310:0f44 | `team_swap_in_table_a4ca`, renames the record opponents during the swap |
| 1310:32b1 | `manager_index_of_team`, finds a human manager of a club |
| 176b:0cc7 | `random_range(lo, hi)` = `lo + rand() % (hi - lo + 1)` |
| 1f9d:0697 | `league_strength_drift(10)`, the strength reset right after the block |
| DS:225e | `tier_team_count[3]` = 18, 20, 20 |
| DS:2266 | `tier_relegated_count[3]` = 3, 4, 4 |
| DS:226a | `tier_has_relegation_playoff[3]` = 1, 0, 0 |
| FAR:535a | final table order, 20 bytes per tier (`league_teams`) |
| FAR:3060 | `relegation_result`, 0 = the Bundesliga club won the play-off |

## Who goes down

The block runs once at the season end, on the final tables `FAR:535a` (already sorted), after
the eternal points and the reset of the season's table counters and before
`league_strength_drift(10)`.

1. Licence withdrawals (balance below -2 000 000 DM) only exist in tiers 0 and 1
   (`rec[0x11a] < 2`), so the amateur tier's extra count `extra[2]` is always 0.
2. The tiers are handled in the order 0, 1, 2. Tier 2 relegates `extra[2] + DS:2266[2]` =
   0 + 4 clubs: for `k = 0..3`, `down = FAR:535a[2*20 + DS:225e[2] - 1 - k]`, i.e. table places
   20, 19, 18 and 17, in that order. `DS:226a[2]` is 0, so there is no play-off place and the
   play-off skip (2a58:3f45) never applies to tier 2.
3. Before that, tier 1 has swapped its last `extra[1] + 4` clubs with the amateur tier's table
   places 1, 2, 3, 4 (more when licence withdrawals in the 2. Bundesliga exceed its four
   relegation places). These slots are the amateur tier's promoted clubs and are disjoint from
   the four bottom places, so the two swaps never touch the same slot.

So eight clubs leave the amateur tier per season: four up, four out of the league system.

## Where replacements come from

From the non-league German slots 58..63 (`docs/re/logic/cups.md` §1: "German reserve teams
that can be promoted into the amateur league"). `n = DS:225f + DS:2260 + DS:225e` = 20 + 20 +
18 = 58 is the first of them. Per `k`:

| k | relegated club (table place) | replacement slot |
|---|---|---|
| 0 | 20th | `random_range(n + 3, n + 4)` = 61 or 62 (only while `(i8)n < 60`, always true in the original) |
| 1 | 19th | `n + 3 - 1` = 60 |
| 2 | 18th | `n + 3 - 2` = 59 |
| 3 | 17th | `n + 3 - 3` = 58 |

Slot 63 never comes up, and of 61 and 62 only the drawn one does; the other stays in place for
the season. The relegated club takes over the replacement's slot number, so next season the
last-placed club of this season sits in slot 61 or 62 and the 17th in slot 58. Slots 58..63 are
in no league and are never shuffled, so the relegated clubs come back up the same way in a
later season: the clubs in slots 58, 59 and 60 every season, the one in 61 or 62 on the draw.
The club in slot 63 never enters the amateur tier through this rule.

A replacement takes the table position of the slot it moves into: the `FAR:535a` row of the
amateur tier is not edited, it keeps the same ids 38..57, and those ids now hold the new club
records. The yearly `league_shuffle_teams` (1310:09c5) then permutes the tier's slots as usual.

A human manager of a club in the amateur tier's bottom four is not relegated out of the game:
the manager gets the season-end flag 8 ("Sie beginnen neu mit 500.000 DM.") and a balance of
500 000 DM, and `team_swap_slots(down, up, 1)` runs twice (once in that branch, once for every
club), so the two swaps cancel. The club stays in its slot and in the amateur tier, the
replacement slot of that place stays out of it, and that season only three clubs go out of the
league system.

## What is copied or regenerated

Nothing is regenerated by the replacement itself; `team_swap_slots(down, up, 1)` (1310:0b24)
exchanges everything that belongs to the two slot numbers:

- the club record `FAR:3066` (0x22 bytes): name, morale, strengths (`+0x18`, base `+0x1b`, third
  value `+0x1e`) and the crest picture number `+0x21`, so the crest moves with the club;
- the team season record `FAR:0ecc` (0x36 bytes, save block 16): table counters, position and
  the eternal points;
- the players: every player record `FAR:57dd + 0x25*p + 0x24` (p = 1..150) of club `down`
  gets `up` and vice versa, so each squad moves with its club;
- the human managers' team ids and, with the third argument 1, the cup title holders
  `DS:07ac..07b0`;
- the history tables: series counters `FAR:9d36`, record scores `FAR:a2ca` and record opponents
  `FAR:a4ca` (whose opponent entries are renamed, 1310:0f44), and the managers' last results
  against the two clubs `FAR:9336`.

The league table order `FAR:535a` (save block 32) is not part of the swap (see above). The
`MANA.DAT` table blocks 3-5 (previous-season points, goals and rank, `docs/re/mana.md`) play no
part either: BMMAIN reads them into a buffer and discards them at load (362f:36ec), so there is
nothing of them to copy; only TEAMED uses them, to recompute the strengths when it saves.

What does change right after the block is the strength: `league_strength_drift(10)` (1f9d:0697)
runs for all 64 German slots in reset mode, clears crest bit 7 and clamps every strength value
of a club into the bounds of the tier its slot now belongs to (`LEAGUE_STRENGTH_BOUNDS`, 15..45
for slots 38 and up, which includes 58..63). A club coming up from slot 58..62 therefore keeps
its own strengths, moved by the reset's random walk and clamped to 15..45.

## RNG calls

- The replacement consumes exactly **one** `rand()` per season: `random_range(61, 62)` at
  2a58:3f31 for `k = 0` of tier 2 (61 for an even `rand()`, 62 for an odd one). Tiers 0 and 1
  and the licence withdrawals draw nothing; `team_swap_slots` and `manager_index_of_team` draw
  nothing.
- The strength reset right after consumes 13 draws per German slot (1 step size, 3 strengths,
  2 per base strength, 3 third values), 832 in total, before the highscore insertion, the
  advertising offers and `league_shuffle_teams` (330 draws: 55 pairs per tier).

## Mod 2026

With the mod layout (18 / 18 / 20 clubs, `league_layout::MOD2026`) the same code runs on the
3. Liga, ids 36..55, with `n` = 56: the replacements are slot 59 or 60, then 58, 57 and 56. The
mod applies this rule to its pool 56..63 unchanged (pinned by
`cups::tests::mod_pool_clubs_follow_the_original_replacement_rule`).
