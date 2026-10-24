//! Match engine of segment 1310 of `BMMAIN.EXE` (`docs/re/logic/match.md`).
//!
//! [`matchday_play`] plays a match day: [`match_live_period`] plans the chances of every
//! pairing with [`match_simulate_quick_result`] and [`goal_minutes_generate`] and plays them
//! minute by minute with [`match_goal_chance`] (both in [`crate::ai`], segment 1f9d), together
//! with the card and injury events of live human matches. The rescheduled league games
//! (`Nachholspiele`) are moved by [`league_postpone_matches_weather`] and played by
//! [`matchday_play_rescheduled`].
//!
//! The functions of other areas that the engine calls mid-match go through [`MatchHooks`]:
//! the gate money and live-match registry ([`crate::season`]) and the match ticker (season
//! area, #31), the cup tie checks and the cup round ([`crate::cups`]) and the newspaper
//! ([`crate::news`]).
//!
//! Not here: the relegation match 1310:02bf ([`crate::cups::matchday_relegation_match`]),
//! the league shuffle 1310:09c5 ([`crate::cups::league_shuffle_teams`]), the club swaps
//! 1310:0ae8/0b24/0f44 ([`GameState::team_swap_slots`]) and every screen of the segment.

use std::iter::StepBy;
use std::ops::Range;

use bmp_formats::savegame::{BLOCK_COMPETITION_SLOTS, BLOCK_PLAYERS, BLOCKS, BlockDesc};

use crate::ai::{GoalFollowUp, match_goal_chance, match_simulate_quick_result};
use crate::cups::round_pairings;
use crate::league::{
    NO_MATCH, NOT_PLAYED, RoundMode, fixtures_build_round_pairings, league_apply_round_and_sort,
};
use crate::rng::{Rng, clamp_int, ldiv, long_abs_diff};
use crate::runtime::{Forfeit, LiveMatch, NO_BOOKING, Runtime};
use crate::squad::{squad_count_listed, squad_count_starters};
use crate::state::{
    GameState, Layout, MANAGER_SLOTS, Player, RESCHEDULED_DAY, RESCHEDULED_GAMES_DAY,
    RESCHEDULED_PAIR, RESCHEDULED_ROUND, RESCHEDULED_TIER,
};
use crate::stats::matchday_assign_ai_scorers;

/// DS:008e: weight in percent of each line (defence, midfield, attack) for the strength kinds
/// 0 = defence, 1 = overall, 2 = attack.
pub const LINE_WEIGHTS: [[u8; 3]; 3] = [[80, 20, 0], [25, 50, 25], [0, 20, 80]];
/// The pseudo manager the match days run as (FAR:304a = 0x1e).
pub const MATCH_DAY_MANAGER: i8 = 0x1e;

/// Far address of the league goal-minute table ([`Runtime::league_goal_minutes`]).
const LEAGUE_MINUTES_ADDR: u16 = 0x515c;
/// Save block 26 ([`GameState::competition_slots`]).
const COMPETITION_SLOTS: BlockDesc = BLOCKS[BLOCK_COMPETITION_SLOTS];
/// Save block 17 ([`GameState::players`]).
const PLAYERS: BlockDesc = BLOCKS[BLOCK_PLAYERS];

/// Far addresses a save block occupies.
fn far_range(block: BlockDesc) -> Range<u16> {
    block.offset..block.offset + block.len
}

/// Weighted strength of a club for strength `kind` (index into [`LINE_WEIGHTS`]) at match
/// `minute`. The base strength of each line decays by `minute * 10 / (base + 1)` (16-bit
/// signed division, clamped at 0) before the current strength and the third strength value
/// are added; the weighted line sums are truncated per line. The club's morale minus 44 is
/// added at the end.
///
/// Origin: BMMAIN.EXE 1310:1468 (team_strength_value)
pub fn team_strength_value(state: &GameState, club: u8, kind: i16, minute: i16) -> i16 {
    let c = &state.clubs[usize::from(club)];
    let weights = LINE_WEIGHTS[kind as usize];
    let mut total: i16 = 0;
    for (j, &weight) in weights.iter().enumerate() {
        let base = i16::from(c.base_strength[j]);
        // The divisor is `0xffff - base`, i.e. -(base + 1); it is never 0.
        let divisor = 0xffff_u16.wrapping_sub(base as u16) as i16;
        let mut v = base.wrapping_add(minute.wrapping_mul(10) / divisor);
        if v < 0 {
            v = 0;
        }
        v = v.wrapping_add(i16::from(c.strength[j]) + i16::from(c.strength_1e[j]));
        let weighted = ldiv(i32::from(v) * i32::from(weight), 100);
        total = total.wrapping_add(weighted as i16);
    }
    total.wrapping_add(i16::from(c.morale) - 0x2c)
}

/// Picks one of two goal counts: `b` when `random_range(0, 1)` is 0, else the larger of the
/// two as signed bytes.
///
/// Origin: BMMAIN.EXE 1310:12d3 (goal_pick)
pub fn goal_pick(rng: &mut Rng, a: u8, b: u8) -> i8 {
    if rng.random_range(0, 1) == 0 || a as i8 <= b as i8 {
        b as i8
    } else {
        a as i8
    }
}

/// Distance between the preferred foot of player `idx` of `squad` (player table +0x20) and
/// the tactics-board column the player stands in.
///
/// Origin: BMMAIN.EXE 1310:1bd7 (player_role_mismatch)
pub fn player_role_mismatch(state: &GameState, squad: u8, idx: u8) -> i16 {
    let p = &state.squads[usize::from(squad)][usize::from(idx)];
    let foot = state.players[usize::from(p.player_id)].attr_20;
    long_abs_diff(i32::from(foot), i32::from(p.board_col)) as i16
}

/// Distance between the position code of player `idx` of `squad` (player table +0x1f, 0..80)
/// and the code of the tactics-board row the player stands in,
/// `(7 - row) * 75 / 7 + 5` (16-bit signed arithmetic).
///
/// Origin: BMMAIN.EXE 1310:1cd3 (player_position_mismatch)
pub fn player_position_mismatch(state: &GameState, squad: u8, idx: u8) -> i16 {
    let p = &state.squads[usize::from(squad)][usize::from(idx)];
    let natural = state.players[usize::from(p.player_id)].position_value;
    let row_code = (7 - i16::from(p.board_row)).wrapping_mul(75) / 7 + 5;
    long_abs_diff(i32::from(natural), i32::from(row_code)) as i16
}

/// The goal-minute slots of one fixture side: `comp < 10` is a league tier, `comp >= 10` a cup
/// row.
fn goal_minutes_mut(rt: &mut Runtime, comp: u8, slot: u8) -> &mut [u8; 8] {
    if comp < 10 {
        &mut rt.league_goal_minutes[usize::from(comp)][usize::from(slot)]
    } else {
        &mut rt.cup_goal_minutes[usize::from(comp - 10)][usize::from(slot)]
    }
}

/// The byte at far address `FAR:515c + offset`, which the collision check of
/// [`goal_minutes_generate`] reads for every competition. For leagues it is the fixture's own
/// minute table; for cups it lies past that table, mostly in the player table.
fn minute_probe(state: &GameState, rt: &Runtime, offset: u16) -> u8 {
    let addr = LEAGUE_MINUTES_ADDR.wrapping_add(offset);
    let league_len = (3 * 20 * 8) as u16;
    if (LEAGUE_MINUTES_ADDR..LEAGUE_MINUTES_ADDR + league_len).contains(&addr) {
        let i = usize::from(addr - LEAGUE_MINUTES_ADDR);
        rt.league_goal_minutes.as_flattened().as_flattened()[i]
    } else if far_range(COMPETITION_SLOTS).contains(&addr) {
        state.competition_slots[usize::from(addr - COMPETITION_SLOTS.offset)]
    } else if far_range(PLAYERS).contains(&addr) {
        let i = usize::from(addr - PLAYERS.offset);
        state.players[i / Player::LEN].to_bytes()[i % Player::LEN]
    } else {
        // TODO(re): far data that no ported table models yet (e.g. FAR:579c..57a9 and
        // FAR:57bc..57dc); assumed 0. FAR:57a6..57a9 is the round list of the match day's
        // forfeits, which a DFB-cup away slot 1 probes; after a forfeit it holds values that
        // `Runtime::forfeits` does not keep.
        0
    }
}

/// Draws `n` (at most 8) distinct goal minutes in `lo..=hi` for one side of a fixture and
/// stores them in its goal-minute slots; unused slots are 0.
///
/// A drawn minute collides with an earlier one when the byte at
/// `FAR:515c + (comp * 20 + slot) * 8 + j` equals it (`j` = earlier goal) or, for leagues, when
/// the slot being filled (still 0) equals it. Collisions are redrawn; after 100 draws the goal
/// keeps minute 0. For cups the check reads bytes past the league table instead of the cup
/// slots, exactly as the original does.
///
/// Origin: BMMAIN.EXE 1310:12fe (goal_minutes_generate)
#[allow(clippy::too_many_arguments)]
pub fn goal_minutes_generate(
    state: &GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    comp: u8,
    slot: u8,
    n: i16,
    lo: i16,
    hi: i16,
) {
    *goal_minutes_mut(rt, comp, slot) = [0; 8];
    let n = n.min(8);
    let probe_base = (u16::from(comp) * 20 + u16::from(slot)) * 8;
    let mut k: i16 = 0;
    while k < n {
        let mut tries: i16 = 0;
        loop {
            let minute = rng.random_range(lo, hi);
            // The league check compares against slot `k`, which is still 0 here.
            let collided = (0..k).any(|j| {
                i16::from(minute_probe(state, rt, probe_base + j as u16)) == minute
                    || (comp < 10 && minute == 0)
            });
            tries += 1;
            if tries >= 100 {
                break;
            }
            if !collided {
                goal_minutes_mut(rt, comp, slot)[k as usize] = minute as u8;
                break;
            }
        }
        k += 1;
    }
}

/// Byte index into the flattened [`GameState::league_results`] of cell `slot` of 1-based
/// `round` of league tier `tier`: row `tier * 38 + round - 1` (the original's table starts one
/// row before save block 6).
pub(crate) fn league_result_cell(tier: u8, round: i16, slot: u8) -> usize {
    let row = i16::from(tier) * 38 + round - 1;
    assert!(
        (0..3 * 38).contains(&row),
        "league result row {row} outside save block 6"
    );
    row as usize * 20 + usize::from(slot)
}

/// Byte index into the flattened [`GameState::cup_pairings`] and [`GameState::cup_results`]
/// of cell `slot` of cup row `cup`.
pub(crate) fn cup_cell(cup: u8, slot: u8) -> usize {
    usize::from(cup) * 32 + usize::from(slot)
}

/// Writes the results of the match day's forfeits: with `v == 0x1e` both cells of each
/// forfeited fixture are marked not played (start of a period); otherwise the side that did
/// not forfeit gets `v` goals and the forfeiting side 0 (end of a period, `v = 2`).
///
/// Origin: BMMAIN.EXE 1310:152e (match_write_forfeit_results)
pub fn match_write_forfeit_results(state: &mut GameState, rt: &Runtime, v: u8) {
    let results = state.league_results.as_flattened_mut().as_flattened_mut();
    for f in &rt.forfeits {
        let round = i16::from(f.round_index) + 1;
        for j in 0..2u8 {
            results[league_result_cell(f.tier, round, f.slot + j)] = if v == NOT_PLAYED {
                NOT_PLAYED
            } else if f.side + j == 2 {
                v
            } else {
                0
            };
        }
    }
}

/// Picks the player of manager `m`'s squad who gets a card or an injury: a random listed
/// record that stands on the pitch and is available, accepted with a chance that grows with
/// its second skill minus its first (`d > random_range(0, 3)`), or 1 in 11 when that
/// difference is not positive. Every rejected record draws again.
///
/// Origin: BMMAIN.EXE 1310:1d45 (match_pick_event_player)
pub fn match_pick_event_player(state: &GameState, rng: &mut Rng, m: u8) -> u8 {
    let n = i16::from(squad_count_listed(state, usize::from(m))) - 1;
    loop {
        let si = rng.random_range(0, n);
        let rec = &state.squads[usize::from(m)][si as usize];
        if rec.lineup_position == 0 || rec.lineup_position >= 12 || rec.status & 3 != 0 {
            continue;
        }
        let d = i16::from(rec.skills[1]) - i16::from(rec.skills[0]);
        let accepted = if d > 0 {
            d > rng.random_range(0, 3)
        } else {
            rng.random_range(0, 10) == 0
        };
        if accepted {
            return si as u8;
        }
    }
}

/// Picks the scorer (`kind` 0) or the assistant (`kind` 1) of a chance of manager `m`'s team
/// from the players on the pitch, weighting skills, goalkeeper ability, league goals, board
/// position and how well the player fits it. `alt` is the goal flag for the scorer; without
/// it (a missed chance) the skill part of the weight is random. `None` (the original's 0xfe)
/// when nobody is on the pitch.
///
/// RNG per try: `random_range(0, listed - 1)` for the record; for a starter
/// `random_range(0x78, 0xaa)` when neither `kind` nor `alt` is set, then
/// `random_range(0, 0xdac)` against the weight, then `random_range(0, 0x14)` when the player
/// has no goalkeeper ability, which vetoes the try unless it is 0.
///
/// Origin: BMMAIN.EXE 1310:2c9a (match_pick_scorer)
pub fn match_pick_scorer(state: &GameState, rng: &mut Rng, m: u8, kind: u8, alt: u8) -> Option<u8> {
    let group = usize::from(m);
    if squad_count_starters(state, group) == 0 {
        return None;
    }
    let n = i16::from(squad_count_listed(state, group)) - 1;
    let kind_i = i16::from(kind);
    let weighted = kind != 0 || alt != 0;
    loop {
        let si = rng.random_range(0, n);
        let rec = &state.squads[group][si as usize];
        if rec.lineup_position == 0 || rec.lineup_position >= 12 {
            continue;
        }
        let [s0, s1, s2] = rec.skills;
        let mut w = if weighted {
            (i16::from(s1) * 3) / (5 - 3 * kind_i) + i16::from(s0 / 5) + i16::from(s2 / 5)
        } else {
            rng.random_range(0x78, 0xaa)
        };
        let keeper = state.players[usize::from(rec.player_id)].position_value;
        w = w.wrapping_add(i16::from(keeper / 25) * (6 - kind_i) * 7);
        w = w.wrapping_add(0x2e * i16::from(rec.goals[0]));
        let d = long_abs_diff(0, i32::from(rec.board_row)) as i16;
        let scorer_bonus = if kind == 0 { 300 } else { 0 };
        w = w.wrapping_add(scorer_bonus + (7 - d) * 200);
        if rec.board_row < 2 {
            w = w.wrapping_add(0x4b0);
        }
        let e = long_abs_diff(0, i32::from(rec.board_col) - 3) as i16;
        let scorer_malus = if kind == 0 { 2 } else { 0 };
        w = w.wrapping_add((e - scorer_malus) * 5);
        let side = player_role_mismatch(state, m, si as u8);
        let role = player_position_mismatch(state, m, si as u8);
        if weighted {
            w = w.wrapping_add((5 - side) * 5).wrapping_add(0x23 - role);
        } else {
            w = w.wrapping_add(side * 5).wrapping_add(role);
        }
        let mut ok = rng.random_range(0, 0xdac) < w;
        if keeper == 0 && rng.random_range(0, 0x14) != 0 {
            ok = false;
        }
        if ok {
            return Some(si as u8);
        }
    }
}

/// One call of `match_ticker_events` 2a58:0ca3 (match.md §4.1); the fields are the original's
/// arguments in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickerEvent {
    /// 0 = missed chance, 1 = goal, 2 = red card (yellow-red with `arg10`), 3 = yellow card,
    /// 4 = injury.
    pub kind: u8,
    /// League tier, or cup row + 10 (card and injury events pass the bare cup row).
    pub comp: u8,
    /// Even pairing slot.
    pub slot: u8,
    /// Home club.
    pub home: u8,
    /// Away club.
    pub away: u8,
    /// Chances: attacking side; yellow card: away flag; injury: cup flag; shoot-out: shooter.
    pub arg6: u8,
    /// Chances: minute (0x78 in a shoot-out); cards and injuries: squad index of the player.
    pub player_or_minute: u8,
    /// Chances: 0 = league, 1 = DFB cup, 2 = European cup; cards and injuries: manager.
    pub manager_or_kind: u8,
    /// 0 = home, 1 = away.
    pub side: u8,
    /// Chances: 1 in a shoot-out (penalty scene); red card: 1 for yellow-red.
    pub arg10: u8,
}

/// The functions of other areas that the match engine calls while it plays, and the two
/// inputs of the live view that change the RNG stream. They run at the original's call
/// points because they draw random numbers and change the state in the middle of the
/// engine's own draws.
///
/// What each logic hook has to keep up in [`Runtime`]:
///
/// * [`MatchHooks::match_gate_receipts_and_aftermath`] (season area) sets
///   [`Runtime::manager_plays_now`], [`Runtime::yellow_cards_left`],
///   [`Runtime::red_card_possible`], fills [`Runtime::booked_players`] with [`NO_BOOKING`], sets
///   [`Runtime::live_match_shown`] and pushes the live match onto [`Runtime::live_matches`]
///   (season.md §3.6).
/// * [`MatchHooks::match_ticker_events`] (season area) counts chances into
///   [`Runtime::period_chances_used`] and applies cards, injuries and scorers.
/// * [`MatchHooks::cup_check_ties_extra_time`] and [`MatchHooks::cup_round_complete`] forward
///   to [`crate::cups::cup_check_ties_extra_time`] and [`crate::cups::cup_round_complete`].
#[allow(clippy::too_many_arguments)]
pub trait MatchHooks {
    /// Gate money, forfeits and the live-match registry of one pairing before kick-off.
    /// Returns 0, or 1 / 2 when the home / away human manager forfeits.
    ///
    /// Origin: BMMAIN.EXE 2a58:20b2 (match_gate_receipts_and_aftermath)
    fn match_gate_receipts_and_aftermath(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        home: u8,
        away: u8,
        cup_mask: u8,
        comp: u8,
        slot: u8,
        league_mask: u8,
    ) -> u8;

    /// A chance, card or injury in a match shown live. The original's return value is a
    /// screen position and is not modelled.
    ///
    /// Origin: BMMAIN.EXE 2a58:0ca3 (match_ticker_events)
    fn match_ticker_events(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        event: TickerEvent,
    );

    /// Marks the cup ties still level for extra time (`shootout == false`) or for penalties
    /// (`shootout == true`, playing them); `count` is the number of pairing bytes. Returns
    /// whether a tie was level. Implementers forward to
    /// [`crate::cups::cup_check_ties_extra_time`].
    ///
    /// Origin: BMMAIN.EXE 27c2:1226 (cup_check_ties_extra_time)
    fn cup_check_ties_extra_time(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        cup_mask: u8,
        count: u8,
        shootout: bool,
    ) -> bool;

    /// Completes the cup round after a cup day; `euro` is the cup mask - 1. Implementers
    /// forward to [`crate::cups::cup_round_complete`].
    ///
    /// Origin: BMMAIN.EXE 27c2:16dc (cup_round_complete)
    fn cup_round_complete(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        euro: u8,
    );

    /// Player grades and the newspaper after every match day.
    ///
    /// Origin: BMMAIN.EXE 3e3a:23aa (match_rate_players_and_newspaper)
    fn match_rate_players_and_newspaper(
        &mut self,
        state: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        cup_mask: u8,
    );

    /// The settings key or the pause click in `minute`: the original opens the settings
    /// screen and skips the whole minute.
    fn pause_requested(&mut self, _state: &mut GameState, _rt: &mut Runtime, _minute: u8) -> bool {
        false
    }

    /// Display: a period starts after its chances were planned. A match day shown live
    /// ([`Runtime::live_match_shown`]) draws the score panels and the time bar and starts the
    /// crowd sound; otherwise the screen is only faded in.
    ///
    /// Origin: BMMAIN.EXE 1310:2303 (match_live_period, 1310:2489..24ef)
    fn period_start(
        &mut self,
        _state: &mut GameState,
        _rt: &mut Runtime,
        _league_mask: u8,
        _cup_mask: u8,
        _from: u8,
    ) {
    }

    /// Display: `minute` of a period starts (the wait, the time bar and the minute ticker of
    /// a match shown live). Called before [`MatchHooks::pause_requested`].
    ///
    /// Origin: BMMAIN.EXE 1310:2303 (match_live_period, 1310:24f9..2550)
    fn period_minute(&mut self, _state: &GameState, _rt: &Runtime, _to: u8, _minute: u8) {}

    /// Display: a period ended. On a league day it is called for every played `tier` right
    /// after that tier's results (after minute 90 its table update and computer scorers),
    /// where the option-selected fixture screen of the tier follows; then once with `None`
    /// for the cup result screens. The first call stops the crowd and clears the screen.
    /// `rng`: the match-day fixture screen recomputes the team strengths.
    ///
    /// Origin: BMMAIN.EXE 1310:2303 (match_live_period, 1310:2aab..2c94)
    #[allow(clippy::too_many_arguments)]
    fn period_end(
        &mut self,
        _state: &mut GameState,
        _rt: &mut Runtime,
        _rng: &mut Rng,
        _league_mask: u8,
        _cup_mask: u8,
        _tier: Option<u8>,
        _to: u8,
    ) {
    }

    /// Display: the match day's result screens before the cup round is completed: the
    /// shoot-out results (`penalties`: a cup tie went to penalties) and the league tables of
    /// the masked tiers.
    ///
    /// Origin: BMMAIN.EXE 1310:15db (matchday_play, 1310:1a6a..1b9e)
    fn matchday_results(
        &mut self,
        _state: &mut GameState,
        _rt: &mut Runtime,
        _rng: &mut Rng,
        _league_mask: u8,
        _cup_mask: u8,
        _penalties: bool,
    ) {
    }

    /// A click on the live score panel of pairing `slot` of `comp` (league tier, or cup row
    /// plus 10) in `minute`, which opens the substitution dialog (2a58:1f75). Returns the
    /// manager whose lineup changed; the engine then re-plans that match's chances.
    ///
    /// Origin: BMMAIN.EXE 1310:3314 (match_handle_panel_click)
    fn match_handle_panel_click(
        &mut self,
        _state: &mut GameState,
        _rt: &mut Runtime,
        _rng: &mut Rng,
        _cup_mask: u8,
        _home: u8,
        _away: u8,
        _slot: u8,
        _comp: u8,
        _minute: u8,
    ) -> Option<u8> {
        None
    }
}

/// Draws the red card, yellow card and injury of `minute` for each human manager whose team
/// plays pairing `slot` (`home` vs `away`) of competition `comp` live. Returns `m + 1` when
/// manager `m` lost a player to a red card or an injury (the caller re-plans the chances),
/// else 0.
///
/// Nothing happens while no human match is shown; the whole function returns 0 as soon as
/// the current manager's aggression is 100 or the playing manager has fewer than four
/// starters. Per manager the bounds are `(level + 5) * 50 + 70 * x` (red, only while one is
/// still possible), `5 * level + 2 * x + 37` (yellow, while cards are left) and
/// `55 * x + 60 * level + 200` (injury), with `x = clamp(40 - aggression, 0, 40)`; an event
/// happens when `random_range(0, bound)` is 0. A second yellow for a booked player is a
/// yellow-red card. The original's garbage argument of the red-card ticker call is passed as
/// 0.
///
/// Origin: BMMAIN.EXE 1310:2ee5 (match_minute_card_injury_events)
#[allow(clippy::too_many_arguments)]
pub fn match_minute_card_injury_events<H: MatchHooks + ?Sized>(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    hooks: &mut H,
    comp: u8,
    slot: u8,
    home: u8,
    away: u8,
    is_cup: u8,
) -> u8 {
    if rt.live_match_shown == 0 {
        return 0;
    }
    let mut ret = 0;
    for m in 0..state.manager_count.min(MANAGER_SLOTS as u8) {
        let mi = usize::from(m);
        let team = state.managers[mi].team;
        if rt.manager_plays_now[mi] == 0 || (team != home && team != away) {
            continue;
        }
        let current = state.current().expect("FAR:304a is not a manager");
        if current.aggression == 0x64 || squad_count_starters(state, mi) < 4 {
            return 0;
        }
        let mut x = 0x28 - i16::from(state.managers[mi].aggression);
        clamp_int(&mut x, 0, 0x28);
        let level = i16::from(state.level as i8);
        let away_side = u8::from(team == away);
        let event = |kind, arg6, player, arg10| TickerEvent {
            kind,
            comp,
            slot,
            home,
            away,
            arg6,
            player_or_minute: player,
            manager_or_kind: m,
            side: away_side,
            arg10,
        };

        if rt.red_card_possible[mi] != 0 {
            let bound = (level + 5).wrapping_mul(0x32).wrapping_add(0x46 * x);
            if rng.random_range(0, bound) == 0 {
                rt.red_card_possible[mi] = 0;
                let player = match_pick_event_player(state, rng, m);
                ret = m + 1;
                hooks.match_ticker_events(state, rt, rng, event(2, 0, player, 0));
            }
        }
        if rt.yellow_cards_left[mi] != 0
            && rng.random_range(0, level.wrapping_mul(5) + 2 * x + 0x25) == 0
        {
            rt.yellow_cards_left[mi] -= 1;
            let (player, booked) = loop {
                let player = match_pick_event_player(state, rng, m);
                let booked = rt.booked_players[mi].contains(&(player as i8));
                if rt.red_card_possible[mi] != 0 || !booked {
                    break (player, booked);
                }
            };
            if booked {
                rt.red_card_possible[mi] = 0;
            } else if let Some(free) = rt.booked_players[mi].iter_mut().find(|b| **b == NO_BOOKING)
            {
                *free = player as i8;
            }
            let dup = u8::from(booked);
            hooks.match_ticker_events(state, rt, rng, event(3 - dup, away_side, player, dup));
        }
        let bound = (0x37 * x)
            .wrapping_add(level.wrapping_mul(0x3c))
            .wrapping_add(200);
        if rng.random_range(0, bound) == 0 {
            let player = match_pick_event_player(state, rng, m);
            ret = m + 1;
            hooks.match_ticker_events(state, rt, rng, event(4, is_cup, player, 0));
        }
        // UI: 1310:322c redraws the event panel here.
    }
    ret
}

/// Re-plans the rest of the period of manager `m`'s match (pairing `slot` of `comp`) from
/// `minute` on after a red card, an injury or a substitution: a fresh quick result for
/// `from..to` minus the chances the manager's match already had this period gives the
/// remaining chance counts, whose minutes are drawn in `minute..=to`.
///
/// The fresh result is read from [`Runtime::quick_sim_result`]; for a fixture whose result
/// cell is 0x1e the quick result writes nothing and the previous value is read, as in the
/// original. The original's unused cup-mask parameter is not taken.
///
/// Origin: BMMAIN.EXE 1310:347f (match_reschedule_chances)
#[allow(clippy::too_many_arguments)]
pub fn match_reschedule_chances(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    comp: u8,
    slot: u8,
    home: u8,
    away: u8,
    minute: u8,
    from: u8,
    to: u8,
    m: u8,
) {
    let mi = usize::from(m);
    match_simulate_quick_result(
        state,
        rt,
        rng,
        comp,
        slot,
        home,
        away,
        from.into(),
        to.into(),
        false,
    );
    for s in 0..2 {
        let cur = rt.manager_last_score[mi][s] as i8;
        if i16::from(cur) <= i16::from(rt.quick_sim_result[s]) {
            rt.quick_sim_result[s] = rt.quick_sim_result[s].wrapping_sub(cur as u8);
        }
        let score = &mut rt.manager_last_score[mi][s];
        *score = score
            .wrapping_sub(rt.period_chances_used[mi][s])
            .wrapping_add(rt.quick_sim_result[s]);
        if (*score as i8) < 0 {
            *score = 0;
        }
        let chances = i16::from(*score as i8);
        goal_minutes_generate(
            state,
            rt,
            rng,
            comp,
            slot + s as u8,
            chances,
            minute.into(),
            to.into(),
        );
    }
}

/// Decides the penalty shoot-out of pairing `slot` of cup row `cup`. Without a human club the
/// score is drawn at once (2..=5 each, redrawn while level); otherwise kicks alternate,
/// starting with a random side, each scoring with chance 2/3 and reported through
/// [`MatchHooks::match_ticker_events`], until both sides took five and a full round ends
/// with the scores apart. The home score gets +20 ("after penalties"). The original's unused
/// manager parameter and its screens are not modelled; its cup-mask parameter only reaches
/// the screens.
///
/// Origin: BMMAIN.EXE 1310:356d (match_penalty_shootout)
pub fn match_penalty_shootout<H: MatchHooks + ?Sized>(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    hooks: &mut H,
    cup: u8,
    slot: u8,
) {
    let cell = cup_cell(cup, slot);
    let first = rng.random_range(0, 1) as u8;
    let pairings = state.cup_pairings.as_flattened();
    let (home, away) = (pairings[cell], pairings[cell + 1]);
    let score = if state.manager_index_of_team(home, away) == 0 {
        loop {
            let score = [rng.random_range(2, 5) as u8, rng.random_range(2, 5) as u8];
            if score[0] != score[1] {
                break score;
            }
        }
    } else {
        let mut score = [0u8; 2];
        rt.live_matches.clear();
        state.ticker_slots = [0xff; 4];
        for team in [home, away] {
            if state.manager_index_of_team(team, 0xff) != 0 {
                rt.live_matches.push(LiveMatch { row: cup, slot });
            }
        }
        // UI: FAR:3052 (live managers) and FAR:4bf1 (ticker state) are only read by the screens.
        let kind = if cup == 0 { 1 } else { 2 };
        let mut shooter = first;
        let mut rounds = 0;
        while !(rounds >= 5 && score[0] != score[1] && shooter == first) {
            let goal = rng.random_range(0, 2).min(1) as u8;
            let event = TickerEvent {
                kind: goal,
                comp: cup + 10,
                slot,
                home,
                away,
                arg6: shooter,
                player_or_minute: 0x78,
                manager_or_kind: kind,
                side: shooter,
                arg10: 1,
            };
            hooks.match_ticker_events(state, rt, rng, event);
            if goal != 0 {
                score[usize::from(shooter)] += 1;
            }
            shooter ^= 1;
            if shooter == first {
                rounds += 1;
            }
        }
        score
    };
    let results = state.cup_results.as_flattened_mut();
    results[cell] = score[0].wrapping_add(0x14);
    results[cell + 1] = score[1];
}

/// Pairing bytes of the cup round of a cup day: the European round (FAR:0009) for
/// `cup_mask > 1`, else the DFB cup round (FAR:0008).
fn cup_pairing_bytes(state: &GameState, cup_mask: u8) -> u8 {
    round_pairings(state.event_state[usize::from(cup_mask > 1)]) << 1
}

/// Home and away club of league pairing `slot` (even) of `tier`.
fn league_pairing(state: &GameState, tier: u8, slot: u8) -> (u8, u8) {
    let [h, a] = state.matchday_pairings[usize::from(tier)][usize::from(slot / 2)];
    (h, a)
}

/// Home and away club of cup pairing `slot` (even) of cup row `cup`.
fn cup_pairing(state: &GameState, cup: u8, slot: u8) -> (u8, u8) {
    let i = cup_cell(cup, slot);
    let pairings = state.cup_pairings.as_flattened();
    (pairings[i], pairings[i + 1])
}

/// Even pairing slots of a league tier: `slot < DS:225e[tier]` (signed).
fn league_slots(rt: &Runtime, tier: u8) -> StepBy<Range<u8>> {
    let teams = rt.tier_team_count[usize::from(tier)];
    (0..teams.max(0) as u8).step_by(2)
}

/// One scheduled chance of `side` in `minute`; a chance in a match shown live goes on to the
/// ticker. The "goal elsewhere" flash of [`GoalFollowUp::Flash`] is display only.
#[allow(clippy::too_many_arguments)]
fn play_chance<H: MatchHooks + ?Sized>(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    hooks: &mut H,
    comp: u8,
    slot: u8,
    home: u8,
    away: u8,
    side: u8,
    minute: u8,
) {
    let chance = match_goal_chance(state, rt, rng, comp, slot, home, away, side, minute.into());
    if let GoalFollowUp::Live { kind, .. } = chance.follow_up {
        let event = TickerEvent {
            kind: u8::from(chance.goal),
            comp,
            slot,
            home,
            away,
            arg6: side,
            player_or_minute: minute,
            manager_or_kind: kind,
            side,
            arg10: 0,
        };
        hooks.match_ticker_events(state, rt, rng, event);
        // UI: 2a58:009d and 1310:322c redraw the score panel.
    }
}

/// The per-minute events of one pairing before its chances: cards and injuries (re-planning
/// the chances of a manager who lost a player) and, except in the last minute, a panel click.
#[allow(clippy::too_many_arguments)]
fn pairing_minute_events<H: MatchHooks + ?Sized>(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    hooks: &mut H,
    cup_mask: u8,
    comp: u8,
    slot: u8,
    home: u8,
    away: u8,
    minute: u8,
    from: u8,
    to: u8,
) {
    let (event_comp, is_cup) = if comp >= 10 {
        (comp - 10, 1)
    } else {
        (comp, 0)
    };
    let r = match_minute_card_injury_events(
        state, rt, rng, hooks, event_comp, slot, home, away, is_cup,
    );
    if r != 0 && r - 1 < state.manager_count {
        match_reschedule_chances(
            state,
            rt,
            rng,
            comp,
            slot,
            home,
            away,
            minute,
            from,
            to,
            r - 1,
        );
    }
    if minute != to
        && let Some(m) =
            hooks.match_handle_panel_click(state, rt, rng, cup_mask, home, away, slot, comp, minute)
    {
        match_reschedule_chances(state, rt, rng, comp, slot, home, away, minute, from, to, m);
    }
}

/// Plays minutes `from..=to` of every match of the match day: first the chance counts and
/// minutes of the whole period for every pairing, then minute by minute the card and injury
/// events and the scheduled chances; after minute 90 the league tables are updated and the
/// goals of the computer clubs are given to scorers.
///
/// `league_mask` selects the league tiers (bit per tier; 0 = the tier-0 pairing list only, as
/// for rescheduled games) on a league day (`cup_mask == 0`), `cup_mask` the cups on a cup day
/// (1 = DFB cup, otherwise European cups). Kept as in the original: the chances of a league
/// day are planned for every tier regardless of the mask, a European cup day plays cups 1..=3
/// regardless of the mask, a paused minute is skipped entirely, and league minute tables
/// keep their minutes while cup minutes are cleared once played. The original's unused
/// parameter and its screen flag are not taken.
///
/// Origin: BMMAIN.EXE 1310:2303 (match_live_period)
#[allow(clippy::too_many_arguments)]
pub fn match_live_period<H: MatchHooks + ?Sized>(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    hooks: &mut H,
    league_mask: u8,
    cup_mask: u8,
    from: u8,
    to: u8,
) {
    let active = if league_mask == 0 { 1 } else { league_mask };
    let managers = usize::from(state.manager_count).min(MANAGER_SLOTS);
    rt.period_chances_used[..managers].fill([0; 2]);
    let cup_bytes = if cup_mask != 0 {
        cup_pairing_bytes(state, cup_mask)
    } else {
        0
    };
    let tiers = if league_mask == 0 { 1 } else { 3 };

    if cup_mask == 0 {
        match_write_forfeit_results(state, rt, NOT_PLAYED);
        // TODO(re): match.md §7 open question 3 — every tier is planned, not only the masked
        // ones.
        for tier in 0..tiers {
            for slot in league_slots(rt, tier) {
                let (h, a) = league_pairing(state, tier, slot);
                if h == NO_MATCH {
                    break;
                }
                match_simulate_quick_result(
                    state,
                    rt,
                    rng,
                    tier,
                    slot,
                    h,
                    a,
                    from.into(),
                    to.into(),
                    true,
                );
            }
        }
    } else {
        for cup in 0..4 {
            if (1 << cup) & cup_mask == 0 {
                continue;
            }
            for slot in (0..cup_bytes).step_by(2) {
                let (h, a) = cup_pairing(state, cup, slot);
                match_simulate_quick_result(
                    state,
                    rt,
                    rng,
                    cup + 10,
                    slot,
                    h,
                    a,
                    from.into(),
                    to.into(),
                    true,
                );
            }
        }
    }
    hooks.period_start(state, rt, league_mask, cup_mask, from);

    for minute in from..=to {
        hooks.period_minute(state, rt, to, minute);
        // TODO(re): match.md §7 open question 4 — a pause skips the whole minute.
        if hooks.pause_requested(state, rt, minute) {
            continue;
        }
        // TODO(#32): the pending message dialog (FAR:4bd8) of a right click.
        if cup_mask == 0 {
            for tier in 0..tiers {
                if (active >> tier) & 1 == 0 {
                    continue;
                }
                for slot in league_slots(rt, tier) {
                    let (h, a) = league_pairing(state, tier, slot);
                    if h == NO_MATCH {
                        break;
                    }
                    pairing_minute_events(
                        state, rt, rng, hooks, cup_mask, tier, slot, h, a, minute, from, to,
                    );
                    let fixture = usize::from(slot);
                    for j in 0..8 {
                        let minutes = &rt.league_goal_minutes[usize::from(tier)];
                        if minutes[fixture][j] == minute {
                            play_chance(state, rt, rng, hooks, tier, slot, h, a, 0, minute);
                        }
                        let minutes = &rt.league_goal_minutes[usize::from(tier)];
                        if minutes[fixture + 1][j] == minute {
                            play_chance(state, rt, rng, hooks, tier, slot, h, a, 1, minute);
                        }
                    }
                }
            }
        } else {
            for cup in 0..4u8 {
                // TODO(re): match.md §7 open question 3 — European cups are played regardless of
                // the mask.
                if (cup_mask == 1 && cup != 0) || (cup_mask > 1 && cup == 0) {
                    continue;
                }
                for slot in (0..cup_bytes).step_by(2) {
                    let (h, a) = cup_pairing(state, cup, slot);
                    let comp = cup + 10;
                    pairing_minute_events(
                        state, rt, rng, hooks, cup_mask, comp, slot, h, a, minute, from, to,
                    );
                    for j in 0..8 {
                        for side in 0..2u8 {
                            let fixture = usize::from(slot + side);
                            if rt.cup_goal_minutes[usize::from(cup)][fixture][j] == minute {
                                play_chance(state, rt, rng, hooks, comp, slot, h, a, side, minute);
                                rt.cup_goal_minutes[usize::from(cup)][fixture][j] = 0;
                            }
                        }
                    }
                }
            }
        }
    }

    match_write_forfeit_results(state, rt, 2);
    if cup_mask == 0 {
        for tier in 0..3 {
            if (active >> tier) & 1 != 0 {
                if to == 90 {
                    league_apply_round_and_sort(state, rng, tier, RoundMode::Apply, 1, false);
                    matchday_assign_ai_scorers(state, rng, tier);
                }
                hooks.period_end(state, rt, rng, league_mask, cup_mask, Some(tier as u8), to);
            }
        }
    }
    hooks.period_end(state, rt, rng, league_mask, cup_mask, None, to);
}

/// Plays a match day: the league tiers of `league_mask` on a league day (`cup_mask == 0`;
/// mask 0 plays the tier-0 pairing list as it stands, for rescheduled games), or the cups of
/// `cup_mask` on a cup day. Before kick-off every pairing goes through
/// [`MatchHooks::match_gate_receipts_and_aftermath`]; a league forfeit marks the fixture not
/// played and records it in [`Runtime::forfeits`]. Both halves follow, then on a cup day the
/// extra time and penalties of level ties, the cup round, and finally the newspaper.
///
/// The current manager becomes 0, as in the original; the caller restores it. The original's
/// two unused parameters and all its screens are not modelled.
///
/// Origin: BMMAIN.EXE 1310:15db (matchday_play)
pub fn matchday_play<H: MatchHooks + ?Sized>(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    hooks: &mut H,
    league_mask: u8,
    cup_mask: u8,
) {
    // UI: the match pictures are loaded here.
    state.current_manager = 0;
    rt.forfeits.clear();
    state.ticker_slots = [0xff; 4];
    rt.live_match_shown = 0;
    rt.live_matches.clear();
    rt.yellow_cards_left = [0; MANAGER_SLOTS];
    rt.red_card_possible = [0; MANAGER_SLOTS];
    // not kept: FAR:3052 (live managers), FAR:4bf1 and FAR:5140 (event counters of the panel).
    let managers = usize::from(state.manager_count).min(MANAGER_SLOTS);
    rt.period_chances_used[..managers].fill([0; 2]);
    let cup_bytes = if cup_mask != 0 {
        cup_pairing_bytes(state, cup_mask)
    } else {
        0
    };

    if cup_mask == 0 {
        let tiers = if league_mask == 0 { 1 } else { 3 };
        for tier in 0..tiers {
            let t = usize::from(tier);
            let round = state.competition_rounds[t];
            if league_mask != 0 {
                fixtures_build_round_pairings(state, t, round);
            }
            // DS:224d (ignore games played in the sort) is 0 in the shipped data.
            league_apply_round_and_sort(state, rng, t, RoundMode::Snapshot, 1, false);
            for slot in league_slots(rt, tier) {
                let (h, a) = league_pairing(state, tier, slot);
                if h == NO_MATCH {
                    break;
                }
                let cell = league_result_cell(tier, i16::from(round as i8), slot);
                if state.league_results.as_flattened().as_flattened()[cell] == NOT_PLAYED {
                    continue;
                }
                let side = hooks.match_gate_receipts_and_aftermath(
                    state,
                    rt,
                    rng,
                    h,
                    a,
                    0,
                    tier,
                    slot,
                    league_mask,
                );
                if side != 0 {
                    state.league_results.as_flattened_mut().as_flattened_mut()[cell] = NOT_PLAYED;
                    rt.forfeits.push(Forfeit {
                        side,
                        tier,
                        round_index: round.wrapping_sub(1),
                        slot,
                    });
                }
            }
        }
    } else {
        for cup in 0..4 {
            if (1 << cup) & cup_mask == 0 {
                continue;
            }
            for slot in (0..cup_bytes).step_by(2) {
                let (h, a) = cup_pairing(state, cup, slot);
                // A cup forfeit is not handled here: the original ignores the result.
                hooks.match_gate_receipts_and_aftermath(
                    state, rt, rng, h, a, cup_mask, cup, slot, 0,
                );
            }
        }
    }

    match_live_period(state, rt, rng, hooks, league_mask, cup_mask, 1, 45);
    match_live_period(state, rt, rng, hooks, league_mask, cup_mask, 46, 90);
    rt.live_match_shown = 0;
    let mut penalties = false;
    if cup_mask != 0 && hooks.cup_check_ties_extra_time(state, rt, rng, cup_mask, cup_bytes, false)
    {
        match_live_period(state, rt, rng, hooks, league_mask, cup_mask, 91, 105);
        match_live_period(state, rt, rng, hooks, league_mask, cup_mask, 106, 120);
        // Whether a tie needed penalties only decides which result screens are shown.
        penalties = hooks.cup_check_ties_extra_time(state, rt, rng, cup_mask, cup_bytes, true);
    }
    hooks.matchday_results(state, rt, rng, league_mask, cup_mask, penalties);
    if cup_mask != 0 {
        hooks.cup_round_complete(state, rt, rng, cup_mask.wrapping_sub(1));
    }
    hooks.match_rate_players_and_newspaper(state, rt, rng, cup_mask);
}

/// Postpones random league matches of the current round of `tier` in winter (days 0x19..=0x45;
/// the most around day 0x2c) by moving them to later days with
/// [`league_reschedule_match`]. Every free reschedule slot draws a new random pairing until
/// one can be moved, so the number of draws depends on the occupied slots; it stops when a
/// pass places nothing.
///
/// Origin: BMMAIN.EXE 1310:0463 (league_postpone_matches_weather)
pub fn league_postpone_matches_weather(
    state: &mut GameState,
    rt: &Runtime,
    rng: &mut Rng,
    day: u8,
    tier: u8,
) {
    if !(0x19..=0x45).contains(&day) {
        return;
    }
    let mut used: Vec<i16> = Vec::new();
    let mut n = 9 - (i16::from(day) - 0x2c).abs() / 3;
    if n != 0 && rng.random_range(0, 1) != 0 {
        n -= 1;
    }
    n = rng.random_range(0, n);
    let round = i16::from(state.competition_rounds[usize::from(tier)] as i8);
    let last_pair = (rt.tier_team_count[usize::from(tier)] >> 1) as i16 - 1;
    while n != 0 {
        let before = n;
        n = n.min(8);
        for s in 0..20u8 {
            if state.rescheduled_matches[usize::from(s)][RESCHEDULED_DAY] != 0 {
                continue;
            }
            let p = rng.random_range(0, last_pair);
            if used.contains(&p) {
                continue;
            }
            let cell = league_result_cell(tier, round, (2 * p) as u8);
            if state.league_results.as_flattened().as_flattened()[cell] == NOT_PLAYED {
                continue;
            }
            if !league_reschedule_match(state, day, tier, p as u8, s) {
                continue;
            }
            n -= 1;
            // The original keeps the pairs as `i8`.
            used.push(i16::from(p as i8));
            break;
        }
        if before == n {
            return;
        }
    }
}

/// Moves pairing `pair` of the current round of `tier` to the first day after `day` (skipping
/// the winter break 0x24..=0x3a) that has no other competition, fewer than nine rescheduled
/// games and no club playing twice, and records it in reschedule slot `slot`. Returns whether
/// a day was found.
///
/// Every rescheduled game already on a candidate day regenerates the pairing lists of its
/// own round and then of the current round of `tier` ([`fixtures_build_round_pairings`]),
/// which leaves those lists changed, as in the original.
///
/// Origin: BMMAIN.EXE 1310:05f1 (league_reschedule_match)
pub fn league_reschedule_match(
    state: &mut GameState,
    day: u8,
    tier: u8,
    pair: u8,
    slot: u8,
) -> bool {
    let t = usize::from(tier);
    let mut d = day.wrapping_add(1);
    while d < 0x5a {
        if d > 0x23 && d < 0x3b {
            d = 0x3b;
        }
        let other = state.week_competitions[usize::from(d)];
        if other == 0 || other == RESCHEDULED_GAMES_DAY {
            let mut ok = true;
            let mut count = 0;
            for s in 0..20 {
                let e = state.rescheduled_matches[s];
                if e[RESCHEDULED_DAY] != d {
                    continue;
                }
                count += 1;
                if count == 9 {
                    ok = false;
                }
                let e_tier = usize::from(e[RESCHEDULED_TIER]);
                fixtures_build_round_pairings(state, e_tier, e[RESCHEDULED_ROUND]);
                let [h, a] = state.matchday_pairings[e_tier][usize::from(e[RESCHEDULED_PAIR])];
                fixtures_build_round_pairings(state, t, state.competition_rounds[t]);
                let [h2, a2] = state.matchday_pairings[t][usize::from(pair)];
                if h2 == h || a2 == a || a2 == h || h2 == a {
                    ok = false;
                }
            }
            if ok {
                let round = state.competition_rounds[t];
                let e = &mut state.rescheduled_matches[usize::from(slot)];
                e[RESCHEDULED_DAY] = d;
                e[RESCHEDULED_ROUND] = round;
                e[RESCHEDULED_PAIR] = pair;
                e[RESCHEDULED_TIER] = tier;
                let cell = league_result_cell(tier, i16::from(round as i8), 2 * pair);
                state.league_results.as_flattened_mut().as_flattened_mut()[cell] = NOT_PLAYED;
                state.week_competitions[usize::from(d)] = RESCHEDULED_GAMES_DAY;
                return true;
            }
        }
        d += 1;
    }
    false
}

/// True when the current manager has a rescheduled match in `week` (always for
/// [`MATCH_DAY_MANAGER`]).
///
/// Origin: BMMAIN.EXE 1310:000a (has_rescheduled_match_today)
pub fn has_rescheduled_match_today(state: &mut GameState, week: u8) -> bool {
    let cur = state.current_manager;
    let Some(m) = usize::try_from(cur).ok().filter(|&m| m < 4) else {
        return cur == MATCH_DAY_MANAGER;
    };
    let tier = state.managers[m].league as usize;
    let team = state.managers[m].team;
    let mut found = false;
    for i in 0..20 {
        let e = state.rescheduled_matches[i];
        if e[RESCHEDULED_DAY] != week {
            continue;
        }
        if e[RESCHEDULED_TIER] as usize != tier || tier > 2 {
            continue;
        }
        fixtures_build_round_pairings(state, tier, e[RESCHEDULED_ROUND]);
        let f = state.matchday_pairings[tier][e[RESCHEDULED_PAIR] as usize % 10];
        if f[0] == team || f[1] == team {
            found = true;
        }
    }
    if tier <= 2 {
        let round = state.competition_rounds[tier];
        fixtures_build_round_pairings(state, tier, round);
    }
    found
}

/// Plays the games rescheduled to `day`: their pairings are put into the tier-0 pairing list,
/// played with [`matchday_play`] (league mask 0, so the results land in tier 0's cells of its
/// current round), copied back to their own rounds, and the entries are cleared.
///
/// Origin: BMMAIN.EXE 1310:07a2 (matchday_play_rescheduled)
pub fn matchday_play_rescheduled<H: MatchHooks + ?Sized>(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    hooks: &mut H,
    day: u8,
) {
    // UI: the title "Nachholspiele".
    let due = |e: &[u8; 5]| e[RESCHEDULED_DAY] == day && e[RESCHEDULED_ROUND] != 0;
    let mut games = Vec::new();
    for s in 0..20 {
        let e = state.rescheduled_matches[s];
        if due(&e) {
            let tier = usize::from(e[RESCHEDULED_TIER]);
            fixtures_build_round_pairings(state, tier, e[RESCHEDULED_ROUND]);
            games.push(state.matchday_pairings[tier][usize::from(e[RESCHEDULED_PAIR])]);
        }
    }
    let pairings = state.matchday_pairings.as_flattened_mut();
    pairings[..games.len()].copy_from_slice(&games);
    if games.len() < 10 {
        pairings[games.len()][0] = NO_MATCH;
    }
    matchday_play(state, rt, rng, hooks, 0, 0);

    // The 8-bit IMUL gives the full 16-bit product: no truncation for rounds >= 7.
    let round = i16::from(state.competition_rounds[0] as i8);
    let mut k = 0;
    for s in 0..20 {
        let e = state.rescheduled_matches[s];
        if !due(&e) {
            continue;
        }
        let tier = e[RESCHEDULED_TIER];
        fixtures_build_round_pairings(state, usize::from(tier), e[RESCHEDULED_ROUND]);
        let dst = league_result_cell(tier, e[RESCHEDULED_ROUND].into(), 2 * e[RESCHEDULED_PAIR]);
        let results = state.league_results.as_flattened_mut().as_flattened_mut();
        for j in 0..2 {
            results[dst + j] = results[league_result_cell(0, round, k)];
            k += 1;
        }
        state.rescheduled_matches[s][RESCHEDULED_DAY] = 0;
    }
    // TODO(re): match.md §7 open question 5 — only 19 of the 20 bytes are cleared.
    let results = state.league_results.as_flattened_mut().as_flattened_mut();
    let base = league_result_cell(0, round, 0);
    results[base..base + 19].fill(0);
    state.week_competitions[usize::from(day)] = 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::GoalChance;

    fn club_state(strength: u8, base: u8, third: u8, morale: u8) -> GameState {
        let mut s = GameState::zeroed();
        let c = &mut s.clubs[5];
        c.strength = [strength; 3];
        c.base_strength = [base; 3];
        c.strength_1e = [third; 3];
        c.morale = morale;
        s
    }

    /// Advances a copy of `rng` by `n` calls of `rand()`.
    fn advanced(rng: Rng, n: usize) -> Rng {
        let mut r = rng;
        for _ in 0..n {
            r.rand();
        }
        r
    }

    #[test]
    fn team_strength_decays_base_strength_with_the_minute() {
        let s = club_state(60, 50, 50, 44);
        // minute 0: every line 160, weights sum to 100.
        assert_eq!(team_strength_value(&s, 5, 1, 0), 160);
        // minute 90: 900 / -51 = -17, line = 33 + 110 = 143; 35 + 71 + 35 = 141.
        assert_eq!(team_strength_value(&s, 5, 1, 90), 141);
        // Attack and defence weights 80/20/0 drop the truncated fractions separately.
        assert_eq!(team_strength_value(&s, 5, 0, 90), 114 + 28);
        assert_eq!(team_strength_value(&s, 5, 2, 90), 28 + 114);
        // Base strength 0: 900 / -1 clamps the line to 0, leaving 15 per line
        // (3 + 7 + 3 after truncation); morale 0 subtracts 44.
        let s = club_state(10, 0, 5, 0);
        assert_eq!(team_strength_value(&s, 5, 1, 90), 13 - 44);
    }

    #[test]
    fn goal_pick_branches() {
        // Seed 1: rand() = 41 (odd) -> random_range(0, 1) = 1.
        let mut rng = Rng::new(1);
        assert_eq!(goal_pick(&mut rng, 3, 2), 3);
        assert_eq!(goal_pick(&mut Rng::new(1), 2, 3), 3);
        assert_eq!(goal_pick(&mut Rng::new(1), 0x80, 2), 2); // 0x80 is -128 as a signed byte
        // Seed 0: rand() = 38 (even) -> always b.
        assert_eq!(goal_pick(&mut Rng::new(0), 3, 2), 2);
    }

    #[test]
    fn mismatch_helpers() {
        let mut s = GameState::zeroed();
        s.players[7].attr_20 = 5;
        s.players[7].position_value = 60;
        let p = &mut s.squads[1][3];
        p.player_id = 7;
        p.board_col = 1;
        p.board_row = 3;
        assert_eq!(player_role_mismatch(&s, 1, 3), 4);
        // (7 - 3) * 75 / 7 + 5 = 42 + 5 = 47
        assert_eq!(player_position_mismatch(&s, 1, 3), 13);
        s.squads[1][3].board_col = -1;
        s.squads[1][3].board_row = -1;
        assert_eq!(player_role_mismatch(&s, 1, 3), 6);
        // (7 + 1) * 75 / 7 + 5 = 85 + 5 = 90
        assert_eq!(player_position_mismatch(&s, 1, 3), 30);
    }

    #[test]
    fn goal_minutes_clamp_to_eight_distinct_minutes() {
        let s = GameState::zeroed();
        let mut rt = Runtime::default();
        let mut rng = Rng::new(9);
        let start = rng;
        goal_minutes_generate(&s, &mut rt, &mut rng, 1, 4, 9, 1, 90);
        let mins = rt.league_goal_minutes[1][4];
        assert!(mins.iter().all(|&m| (1..=90).contains(&m)));
        let mut sorted = mins;
        sorted.sort();
        assert!(sorted.windows(2).all(|w| w[0] != w[1]));
        let draws = (0..200).find(|&n| advanced(start, n) == rng).unwrap();
        assert!(draws >= 8);
    }

    #[test]
    fn league_collisions_redraw_until_the_try_limit() {
        let s = GameState::zeroed();
        let mut rt = Runtime::default();
        rt.league_goal_minutes[0][2] = [9; 8];
        let mut rng = Rng::new(3);
        let start = rng;
        // Every draw is 30: goal 0 gets it, goal 1 collides 100 times and keeps 0.
        goal_minutes_generate(&s, &mut rt, &mut rng, 0, 2, 2, 30, 30);
        assert_eq!(rt.league_goal_minutes[0][2], [30, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(rng, advanced(start, 101));
        // Minute 0 collides with the still empty slot being filled.
        let mut rng = Rng::new(3);
        goal_minutes_generate(&s, &mut rt, &mut rng, 0, 2, 2, 0, 0);
        assert_eq!(rt.league_goal_minutes[0][2], [0; 8]);
        assert_eq!(rng, advanced(start, 101));
    }

    #[test]
    fn cup_collisions_probe_the_player_table() {
        // Cup row 1 (comp 11), slot 0: the probe reads FAR:515c + 220 * 8 = FAR:583c, which is
        // byte 21 of player 2 (FAR:57dd + 2 * 0x25 + 21).
        let mut s = GameState::zeroed();
        let mut rt = Runtime::default();
        let mut rng = Rng::new(3);
        let start = rng;
        goal_minutes_generate(&s, &mut rt, &mut rng, 11, 0, 2, 30, 30);
        assert_eq!(rt.cup_goal_minutes[1][0], [30, 30, 0, 0, 0, 0, 0, 0]);
        assert_eq!(rng, advanced(start, 2));
        s.players[2].name[21] = 30;
        let mut rng = Rng::new(3);
        goal_minutes_generate(&s, &mut rt, &mut rng, 11, 0, 2, 30, 30);
        assert_eq!(rt.cup_goal_minutes[1][0], [30, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(rng, advanced(start, 101));
    }
    /// Number of `rand()` calls that took `start` to `end`.
    fn draws(start: Rng, end: Rng) -> usize {
        (0..10_000).find(|&n| advanced(start, n) == end).unwrap()
    }

    /// The `n`th value of `random_range(lo, hi)` drawn from a fresh copy of `rng`.
    fn nth_range(rng: Rng, n: usize, lo: i16, hi: i16) -> i16 {
        advanced(rng, n).random_range(lo, hi)
    }

    /// Puts player `pid` into record `i` of squad `m` at lineup position `slot`.
    fn put_player(s: &mut GameState, m: usize, i: usize, pid: u8, slot: u8) {
        let r = &mut s.squads[m][i];
        r.player_id = pid;
        r.lineup_position = slot;
    }

    #[test]
    fn forfeit_results_mark_then_award() {
        let mut s = GameState::zeroed();
        let rt = Runtime {
            forfeits: vec![
                Forfeit {
                    side: 1,
                    tier: 0,
                    round_index: 0,
                    slot: 2,
                },
                Forfeit {
                    side: 2,
                    tier: 1,
                    round_index: 5,
                    slot: 4,
                },
            ],
            ..Runtime::default()
        };
        match_write_forfeit_results(&mut s, &rt, NOT_PLAYED);
        assert_eq!(s.league_results[0][0][2..4], [0x1e, 0x1e]);
        // Tier 1, round index 5 is row 43 of the flat table.
        assert_eq!(s.league_results.as_flattened()[43][4..6], [0x1e, 0x1e]);
        match_write_forfeit_results(&mut s, &rt, 2);
        assert_eq!(s.league_results[0][0][2..4], [0, 2]);
        assert_eq!(s.league_results[1][5][4..6], [2, 0]);
    }

    #[test]
    fn event_player_skips_unavailable_records() {
        let mut s = GameState::zeroed();
        put_player(&mut s, 0, 0, 1, 0); // not selected
        put_player(&mut s, 0, 1, 2, 12); // bench
        put_player(&mut s, 0, 2, 3, 2);
        s.squads[0][2].status = 1; // suspended
        put_player(&mut s, 0, 3, 4, 5);
        s.squads[0][3].skills = [10, 14, 0]; // d = 4 beats every random_range(0, 3)
        let start = Rng::new(7);
        let mut rng = start;
        assert_eq!(match_pick_event_player(&s, &mut rng, 0), 3);
        let tries = (0..).find(|&k| nth_range(start, k, 0, 3) == 3).unwrap();
        assert_eq!(draws(start, rng), tries + 2);
    }

    #[test]
    fn event_player_without_skill_gap_needs_one_in_eleven() {
        let mut s = GameState::zeroed();
        put_player(&mut s, 0, 0, 1, 1);
        s.squads[0][0].skills = [20, 20, 0];
        let start = Rng::new(11);
        let mut rng = start;
        assert_eq!(match_pick_event_player(&s, &mut rng, 0), 0);
        // Each try draws the record (always 0) and random_range(0, 10).
        let tries = (0..)
            .find(|&k| nth_range(start, 2 * k + 1, 0, 10) == 0)
            .unwrap();
        assert_eq!(draws(start, rng), 2 * tries + 2);
    }

    /// One starter in record 0 of squad 0 with the plan's worked example: weight 2977 as a
    /// scorer of a goal (`kind` 0, `alt` 1).
    fn scorer_state() -> GameState {
        let mut s = GameState::zeroed();
        put_player(&mut s, 0, 0, 9, 4);
        let r = &mut s.squads[0][0];
        r.skills = [50, 60, 70];
        r.goals[0] = 2;
        r.board_row = 1;
        r.board_col = 3;
        s.players[9].position_value = 60;
        s.players[9].attr_20 = 3;
        s
    }

    #[test]
    fn scorer_without_starters_draws_nothing() {
        let mut s = scorer_state();
        s.squads[0][0].lineup_position = 12;
        let mut rng = Rng::new(1);
        assert_eq!(match_pick_scorer(&s, &mut rng, 0, 0, 1), None);
        assert_eq!(rng, Rng::new(1));
    }

    #[test]
    fn scorer_weight_matches_the_hand_derived_value() {
        // 60*3/5 + 50/5 + 70/5 = 60; + 2*6*7 = 144; + 46*2 = 236; + 300 + 6*200 = 1736;
        // row 1 < 2: + 1200 = 2936; |3 - 3| = 0: - 10 = 2926; foot fits: + 25 = 2951;
        // |60 - 69| = 9: + 26 = 2977.
        let s = scorer_state();
        let seed_drawing = |v: i16| {
            (0..=u16::MAX)
                .map(Rng::new)
                .find(|&r| nth_range(r, 1, 0, 0xdac) == v)
                .unwrap()
        };
        let start = seed_drawing(2976);
        let mut rng = start;
        assert_eq!(match_pick_scorer(&s, &mut rng, 0, 0, 1), Some(0));
        assert_eq!(draws(start, rng), 2);
        let start = seed_drawing(2977);
        let mut rng = start;
        assert_eq!(match_pick_scorer(&s, &mut rng, 0, 0, 1), Some(0));
        assert!(draws(start, rng) >= 4);
    }

    #[test]
    fn scorer_without_keeper_ability_draws_the_veto() {
        let mut s = scorer_state();
        s.players[9].position_value = 0;
        let start = Rng::new(5);
        let mut rng = start;
        assert_eq!(match_pick_scorer(&s, &mut rng, 0, 0, 1), Some(0));
        // Each try: record, weight check, veto; only a veto draw of 0 can end the loop.
        let n = draws(start, rng);
        assert_eq!(n % 3, 0);
        assert_eq!(nth_range(start, n - 1, 0, 0x14), 0);
    }
    /// A call the engine made into [`MatchHooks`].
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        Gate {
            home: u8,
            away: u8,
            cup_mask: u8,
            comp: u8,
            slot: u8,
            league_mask: u8,
        },
        Ticker(TickerEvent),
        Ties {
            cup_mask: u8,
            count: u8,
            shootout: bool,
        },
        RoundComplete(u8),
        Newspaper(u8),
    }

    /// Hooks for computer-only match days: every logic hook only records its call and returns
    /// the next queued answer (0 / `false` when the queue is empty).
    #[derive(Default)]
    struct Recorder {
        calls: Vec<Call>,
        gate_answers: Vec<u8>,
        tie_answers: Vec<bool>,
        pause_every_minute: bool,
        /// Every minute the engine played or skipped, in order.
        minutes: Vec<u8>,
        /// `(minute, manager)`: the panel click of that minute changes that manager's lineup.
        click: Option<(u8, u8)>,
    }

    impl Recorder {
        fn tickers(&self) -> Vec<TickerEvent> {
            self.calls
                .iter()
                .filter_map(|c| match c {
                    Call::Ticker(e) => Some(*e),
                    _ => None,
                })
                .collect()
        }
    }

    impl MatchHooks for Recorder {
        fn match_gate_receipts_and_aftermath(
            &mut self,
            _: &mut GameState,
            _: &mut Runtime,
            _: &mut Rng,
            home: u8,
            away: u8,
            cup_mask: u8,
            comp: u8,
            slot: u8,
            league_mask: u8,
        ) -> u8 {
            self.calls.push(Call::Gate {
                home,
                away,
                cup_mask,
                comp,
                slot,
                league_mask,
            });
            if self.gate_answers.is_empty() {
                0
            } else {
                self.gate_answers.remove(0)
            }
        }

        fn match_ticker_events(
            &mut self,
            _: &mut GameState,
            _: &mut Runtime,
            _: &mut Rng,
            e: TickerEvent,
        ) {
            self.calls.push(Call::Ticker(e));
        }

        fn cup_check_ties_extra_time(
            &mut self,
            _: &mut GameState,
            _: &mut Runtime,
            _: &mut Rng,
            cup_mask: u8,
            count: u8,
            shootout: bool,
        ) -> bool {
            self.calls.push(Call::Ties {
                cup_mask,
                count,
                shootout,
            });
            !self.tie_answers.is_empty() && self.tie_answers.remove(0)
        }

        fn cup_round_complete(
            &mut self,
            _: &mut GameState,
            _: &mut Runtime,
            _: &mut Rng,
            euro: u8,
        ) {
            self.calls.push(Call::RoundComplete(euro));
        }

        fn match_rate_players_and_newspaper(
            &mut self,
            _: &mut GameState,
            _: &mut Runtime,
            _: &mut Rng,
            cup_mask: u8,
        ) {
            self.calls.push(Call::Newspaper(cup_mask));
        }

        fn pause_requested(&mut self, _: &mut GameState, _: &mut Runtime, minute: u8) -> bool {
            self.minutes.push(minute);
            self.pause_every_minute
        }

        fn match_handle_panel_click(
            &mut self,
            _: &mut GameState,
            _: &mut Runtime,
            _: &mut Rng,
            _: u8,
            _: u8,
            _: u8,
            _: u8,
            _: u8,
            minute: u8,
        ) -> Option<u8> {
            self.click.filter(|&(at, _)| at == minute).map(|(_, m)| m)
        }
    }

    const HOME: u8 = 3;
    const AWAY: u8 = 7;

    /// Human manager 0 manages `HOME`, plays live and has four starters whose skill gap of 4
    /// accepts the first starter drawn as the event player.
    fn card_state(level: u8, aggression: u8) -> (GameState, Runtime) {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.level = level;
        s.managers[0].team = HOME;
        s.managers[0].aggression = aggression;
        for i in 0..4 {
            put_player(&mut s, 0, i, i as u8 + 1, i as u8 + 1);
            s.squads[0][i].skills = [10, 14, 0];
        }
        let rt = Runtime {
            live_match_shown: 1,
            manager_plays_now: [1, 0, 0, 0],
            ..Runtime::default()
        };
        (s, rt)
    }

    fn card_events(s: &mut GameState, rt: &mut Runtime, rng: &mut Rng, hooks: &mut Recorder) -> u8 {
        match_minute_card_injury_events(s, rt, rng, hooks, 0, 2, HOME, AWAY, 0)
    }

    #[test]
    fn card_events_need_a_live_playable_match() {
        let (mut s, mut rt) = card_state(0, 40);
        let mut hooks = Recorder::default();
        let mut rng = Rng::new(1);
        rt.live_match_shown = 0;
        assert_eq!(card_events(&mut s, &mut rt, &mut rng, &mut hooks), 0);
        rt.live_match_shown = 1;
        s.managers[0].aggression = 100; // the current manager, 0 on a match day
        assert_eq!(card_events(&mut s, &mut rt, &mut rng, &mut hooks), 0);
        s.managers[0].aggression = 40;
        s.squads[0][3].lineup_position = 12;
        assert_eq!(card_events(&mut s, &mut rt, &mut rng, &mut hooks), 0);
        assert_eq!(rng, Rng::new(1));
        assert!(hooks.calls.is_empty());
    }

    #[test]
    fn injury_bound_uses_the_full_16_bit_level_product() {
        // Level 3, x = 0: 0x3c * 3 = 180, bound 380. Truncated to i8 it would be 124.
        let start = (0..=u16::MAX)
            .map(Rng::new)
            .find(|&r| nth_range(r, 0, 0, 380) == 0 && nth_range(r, 0, 0, 124) != 0)
            .unwrap();
        let (mut s, mut rt) = card_state(3, 40);
        let mut hooks = Recorder::default();
        let mut rng = start;
        assert_eq!(card_events(&mut s, &mut rt, &mut rng, &mut hooks), 1);
        let player = nth_range(start, 1, 0, 3) as u8;
        let expected = TickerEvent {
            kind: 4,
            comp: 0,
            slot: 2,
            home: HOME,
            away: AWAY,
            arg6: 0,
            player_or_minute: player,
            manager_or_kind: 0,
            side: 0,
            arg10: 0,
        };
        assert_eq!(hooks.calls, [Call::Ticker(expected)]);
        assert_eq!(draws(start, rng), 3);
    }

    #[test]
    fn red_card_ends_the_red_card_chance() {
        // Level 0, x = 0: red bound 5 * 50 = 250 (200 with level + 4), injury bound 200.
        let start = (0..=u16::MAX)
            .map(Rng::new)
            .find(|&r| {
                nth_range(r, 0, 0, 250) == 0
                    && nth_range(r, 0, 0, 200) != 0
                    && nth_range(r, 3, 0, 200) != 0
            })
            .unwrap();
        let (mut s, mut rt) = card_state(0, 40);
        rt.red_card_possible[0] = 1;
        let mut hooks = Recorder::default();
        let mut rng = start;
        assert_eq!(card_events(&mut s, &mut rt, &mut rng, &mut hooks), 1);
        let red = TickerEvent {
            kind: 2,
            comp: 0,
            slot: 2,
            home: HOME,
            away: AWAY,
            arg6: 0,
            player_or_minute: nth_range(start, 1, 0, 3) as u8,
            manager_or_kind: 0,
            side: 0,
            arg10: 0,
        };
        assert_eq!(hooks.calls, [Call::Ticker(red)]);
        assert_eq!(rt.red_card_possible[0], 0);
        assert_eq!(draws(start, rng), 4);
    }

    #[test]
    fn an_injury_replans_the_injured_managers_match() {
        // Manager 1 manages HOME and plays live; manager 0 (the current one) does not play.
        let mut s = period_state();
        s.manager_count = 2;
        s.managers[0].team = 99;
        s.managers[1].team = HOME;
        s.managers[1].aggression = 40;
        for i in 0..4 {
            put_player(&mut s, 1, i, i as u8 + 1, i as u8 + 1);
            s.squads[1][i].skills = [10, 14, 0];
        }
        let mut rt = Runtime {
            live_match_shown: 1,
            manager_plays_now: [0, 1, 0, 0],
            ..Runtime::default()
        };
        rt.manager_last_score[1] = [3, 2];
        // Level 0, x = 0: the first draw hits the injury (bound 200).
        let start = (0..=u16::MAX)
            .map(Rng::new)
            .find(|&r| nth_range(r, 0, 0, 200) == 0)
            .unwrap();
        let (mut s2, mut rt2, mut rng2) = (s.clone(), rt.clone(), start);
        let mut rng = start;
        let mut hooks = Recorder::default();
        pairing_minute_events(
            &mut s, &mut rt, &mut rng, &mut hooks, 0, 0, 0, HOME, AWAY, 10, 1, 45,
        );
        let r = match_minute_card_injury_events(
            &mut s2,
            &mut rt2,
            &mut rng2,
            &mut Recorder::default(),
            0,
            0,
            HOME,
            AWAY,
            0,
        );
        assert_eq!(r, 2);
        match_reschedule_chances(&mut s2, &mut rt2, &mut rng2, 0, 0, HOME, AWAY, 10, 1, 45, 1);
        assert_eq!(rng, rng2);
        assert_eq!(rt.manager_last_score, rt2.manager_last_score);
        assert_eq!(rt.league_goal_minutes, rt2.league_goal_minutes);
        assert_eq!(hooks.tickers()[0].kind, 4);
    }

    /// A seed whose first draw misses the red card (bound 250) and whose second hits the
    /// yellow card (bound 37): level 0, x = 0.
    fn yellow_seed() -> Rng {
        (0..=u16::MAX)
            .map(Rng::new)
            .find(|&r| nth_range(r, 0, 0, 250) != 0 && nth_range(r, 1, 0, 37) == 0)
            .unwrap()
    }

    #[test]
    fn yellow_card_books_the_player() {
        let start = yellow_seed();
        let (mut s, mut rt) = card_state(0, 40);
        rt.red_card_possible[0] = 1;
        rt.yellow_cards_left[0] = 6;
        let mut hooks = Recorder::default();
        let mut rng = start;
        card_events(&mut s, &mut rt, &mut rng, &mut hooks);
        let e = hooks.tickers()[0];
        assert_eq!((e.kind, e.arg10, e.arg6), (3, 0, 0));
        assert_eq!(rt.booked_players[0][0], e.player_or_minute as i8);
        assert_eq!(rt.yellow_cards_left[0], 5);
        assert_eq!(rt.red_card_possible[0], 1);
    }

    #[test]
    fn second_yellow_card_is_yellow_red() {
        let start = yellow_seed();
        let (mut s, mut rt) = card_state(0, 40);
        rt.red_card_possible[0] = 1;
        rt.yellow_cards_left[0] = 6;
        rt.booked_players[0] = [0, 1, 2, 3, NO_BOOKING, NO_BOOKING];
        let mut hooks = Recorder::default();
        let mut rng = start;
        card_events(&mut s, &mut rt, &mut rng, &mut hooks);
        let e = hooks.tickers()[0];
        assert_eq!((e.kind, e.arg10), (2, 1));
        assert_eq!(rt.red_card_possible[0], 0);
        assert_eq!(rt.booked_players[0], [0, 1, 2, 3, NO_BOOKING, NO_BOOKING]);
    }

    #[test]
    fn rescheduled_chances_subtract_the_played_ones() {
        let mut s = GameState::zeroed();
        s.competition_rounds[0] = 1;
        s.league_results[0][0][4] = NOT_PLAYED; // the quick result is skipped, no RNG
        let mut rt = Runtime {
            quick_sim_result: [3, 1],
            ..Runtime::default()
        };
        rt.manager_last_score[0] = [2, 2];
        rt.period_chances_used[0] = [1, 0];
        let start = Rng::new(4);
        let mut rng = start;
        match_reschedule_chances(&mut s, &mut rt, &mut rng, 0, 4, HOME, AWAY, 10, 1, 45, 0);
        assert_eq!(rt.quick_sim_result, [1, 1]);
        assert_eq!(rt.manager_last_score[0], [2, 3]);
        for (side, n) in [(4, 2), (5, 3)] {
            let mins = rt.league_goal_minutes[0][side];
            assert_eq!(mins.iter().filter(|&&m| m != 0).count(), n);
            assert!(mins[..n].iter().all(|m| (10..=45).contains(m)));
        }
        assert!(draws(start, rng) >= 5);
    }

    #[test]
    fn rescheduled_chances_clamp_at_zero() {
        let mut s = GameState::zeroed();
        s.competition_rounds[0] = 1;
        s.league_results[0][0][4] = NOT_PLAYED;
        let mut rt = Runtime::default();
        rt.manager_last_score[0] = [5, 0];
        rt.period_chances_used[0] = [7, 0];
        rt.league_goal_minutes[0][4] = [9; 8];
        let mut rng = Rng::new(4);
        match_reschedule_chances(&mut s, &mut rt, &mut rng, 0, 4, HOME, AWAY, 10, 1, 45, 0);
        assert_eq!(rt.manager_last_score[0], [0, 0]);
        assert_eq!(rt.league_goal_minutes[0][4], [0; 8]);
        assert_eq!(rng, Rng::new(4));
    }

    fn shootout_state(cup: usize, slot: usize) -> GameState {
        let mut s = GameState::zeroed();
        s.cup_pairings[cup * 16 + slot / 2] = [HOME, AWAY];
        s.cup_results[cup * 16 + slot / 2] = [12, 12];
        s
    }

    #[test]
    fn computer_shootout_is_drawn_at_once() {
        for seed in 0..50 {
            let mut s = shootout_state(1, 2);
            let mut rt = Runtime::default();
            let mut hooks = Recorder::default();
            let start = Rng::new(seed);
            let mut rng = start;
            match_penalty_shootout(&mut s, &mut rt, &mut rng, &mut hooks, 1, 2);
            let [h, a] = s.cup_results[17];
            assert!((22..=25).contains(&h) && (2..=5).contains(&a) && h - 20 != a);
            assert_eq!(draws(start, rng) % 2, 1);
            assert!(hooks.calls.is_empty());
        }
    }

    #[test]
    fn human_shootout_alternates_kicks_through_the_ticker() {
        let mut s = shootout_state(0, 4);
        s.manager_count = 1;
        s.managers[0].team = HOME;
        s.ticker_slots = [1, 2, 3, 4];
        let mut rt = Runtime {
            live_matches: vec![LiveMatch { row: 9, slot: 9 }],
            ..Runtime::default()
        };
        let mut hooks = Recorder::default();
        let start = Rng::new(21);
        let mut rng = start;
        match_penalty_shootout(&mut s, &mut rt, &mut rng, &mut hooks, 0, 4);
        let kicks = hooks.tickers();
        assert!(kicks.len() >= 10 && kicks.len() % 2 == 0);
        let first = nth_range(start, 0, 0, 1) as u8;
        let mut goals = [0u8; 2];
        for (k, e) in kicks.iter().enumerate() {
            let shooter = first ^ (k as u8 & 1);
            let expected = TickerEvent {
                kind: e.kind,
                comp: 10,
                slot: 4,
                home: HOME,
                away: AWAY,
                arg6: shooter,
                player_or_minute: 0x78,
                manager_or_kind: 1,
                side: shooter,
                arg10: 1,
            };
            assert_eq!(*e, expected);
            assert_eq!(e.kind, nth_range(start, k + 1, 0, 2).min(1) as u8);
            goals[usize::from(shooter)] += e.kind;
        }
        assert_ne!(goals[0], goals[1]);
        assert_eq!(s.cup_results[2], [goals[0] + 20, goals[1]]);
        assert_eq!(rt.live_matches, [LiveMatch { row: 0, slot: 4 }]);
        assert_eq!(s.ticker_slots, [0xff; 4]);
        assert_eq!(draws(start, rng), kicks.len() + 1);
    }
    /// Round 1 of every tier; tier 0 plays `HOME` (strong) against `AWAY` in slot 0 only, and
    /// tiers 1 and 2 play nothing.
    fn period_state() -> GameState {
        let mut s = GameState::zeroed();
        s.competition_rounds = [1, 1, 1];
        for (club, lines) in [(HOME, [60, 50, 50]), (AWAY, [30, 40, 30])] {
            let c = &mut s.clubs[usize::from(club)];
            c.strength = [lines[0]; 3];
            c.base_strength = [lines[1]; 3];
            c.strength_1e = [lines[2]; 3];
            c.morale = 44;
        }
        s.matchday_pairings[0][0] = [HOME, AWAY];
        s.matchday_pairings[0][1] = [NO_MATCH, 0];
        s.matchday_pairings[1][0] = [NO_MATCH, 0];
        s.matchday_pairings[2][0] = [NO_MATCH, 0];
        s
    }

    /// The first half of the slot-0 pairings composed by hand from the segment 1f9d
    /// primitives: the quick result of every `planned` tier, then per minute, `played` tier and
    /// goal slot the home chance before the away chance. Returns every chance as
    /// `(minute, side, chance)`.
    fn reference_league_half(
        s: &mut GameState,
        rt: &mut Runtime,
        rng: &mut Rng,
        planned: &[u8],
        played: &[u8],
    ) -> Vec<(u8, u8, GoalChance)> {
        let pairing = |s: &GameState, tier: u8| league_pairing(s, tier, 0);
        for &tier in planned {
            let (h, a) = pairing(s, tier);
            match_simulate_quick_result(s, rt, rng, tier, 0, h, a, 1, 45, true);
        }
        let mut chances = Vec::new();
        for minute in 1..=45 {
            for &tier in played {
                let (h, a) = pairing(s, tier);
                for j in 0..8 {
                    for side in 0..2 {
                        if rt.league_goal_minutes[usize::from(tier)][usize::from(side)][j] == minute
                        {
                            let c =
                                match_goal_chance(s, rt, rng, tier, 0, h, a, side, minute.into());
                            chances.push((minute, side, c));
                        }
                    }
                }
            }
        }
        chances
    }

    #[test]
    fn live_period_composes_the_primitives_in_the_original_order() {
        for seed in [1, 2, 3, 99] {
            let (mut s, mut rt, mut rng) = (period_state(), Runtime::default(), Rng::new(seed));
            let (mut s2, mut rt2, mut rng2) = (s.clone(), rt.clone(), rng);
            match_live_period(
                &mut s,
                &mut rt,
                &mut rng,
                &mut Recorder::default(),
                1,
                0,
                1,
                45,
            );
            reference_league_half(&mut s2, &mut rt2, &mut rng2, &[0], &[0]);
            assert_eq!(rng, rng2, "seed {seed}");
            assert_eq!(s.league_results, s2.league_results);
            assert_eq!(rt.league_goal_minutes, rt2.league_goal_minutes);
            assert!(
                rt.league_goal_minutes[0][0][0] != 0,
                "seed {seed} planned no chance"
            );
        }
    }

    /// Clubs of the tier-1 pairing that [`two_tier_state`] adds.
    const T1_HOME: u8 = 20;
    const T1_AWAY: u8 = 25;

    /// [`period_state`] with a second pairing, `T1_HOME` against `T1_AWAY`, in tier 1.
    fn two_tier_state() -> GameState {
        let mut s = period_state();
        for club in [T1_HOME, T1_AWAY] {
            s.clubs[usize::from(club)] = s.clubs[usize::from(HOME)].clone();
        }
        s.matchday_pairings[1][0] = [T1_HOME, T1_AWAY];
        s.matchday_pairings[1][1] = [NO_MATCH, 0];
        s
    }

    #[test]
    fn league_mask_selects_the_tiers_that_play() {
        // (league mask, tiers planned, tiers played); mask 0 is the rescheduled-games day.
        let cases: [(u8, &[u8], &[u8]); 3] =
            [(0, &[0], &[0]), (1, &[0, 1], &[0]), (2, &[0, 1], &[1])];
        for (mask, planned, played) in cases {
            let (mut s, mut rt, mut rng) = (two_tier_state(), Runtime::default(), Rng::new(2));
            let (mut s2, mut rt2, mut rng2) = (s.clone(), rt.clone(), rng);
            let mut hooks = Recorder::default();
            match_live_period(&mut s, &mut rt, &mut rng, &mut hooks, mask, 0, 1, 45);
            let chances = reference_league_half(&mut s2, &mut rt2, &mut rng2, planned, played);
            assert!(!chances.is_empty(), "mask {mask} played no chance");
            assert_eq!(rng, rng2, "mask {mask}");
            assert_eq!(s.league_results, s2.league_results, "mask {mask}");
            assert_eq!(
                rt.league_goal_minutes, rt2.league_goal_minutes,
                "mask {mask}"
            );
        }
    }

    #[test]
    fn chances_of_a_live_match_go_to_the_ticker() {
        let (mut goals, mut away_chances) = (0, 0);
        for seed in [1, 2, 3, 99] {
            let mut s = period_state();
            let mut rt = Runtime {
                live_match_shown: 1,
                live_matches: vec![LiveMatch { row: 0, slot: 0 }],
                ..Runtime::default()
            };
            let mut rng = Rng::new(seed);
            let (mut s2, mut rt2, mut rng2) = (s.clone(), rt.clone(), rng);
            let mut hooks = Recorder::default();
            match_live_period(&mut s, &mut rt, &mut rng, &mut hooks, 1, 0, 1, 45);
            let chances = reference_league_half(&mut s2, &mut rt2, &mut rng2, &[0], &[0]);
            let expected: Vec<_> = chances
                .iter()
                .map(|&(minute, side, c)| {
                    assert_eq!(c.follow_up, GoalFollowUp::Live { index: 0, kind: 0 });
                    TickerEvent {
                        kind: u8::from(c.goal),
                        comp: 0,
                        slot: 0,
                        home: HOME,
                        away: AWAY,
                        arg6: side,
                        player_or_minute: minute,
                        manager_or_kind: 0,
                        side,
                        arg10: 0,
                    }
                })
                .collect();
            assert_eq!(hooks.tickers(), expected, "seed {seed}");
            goals += expected.iter().filter(|e| e.kind == 1).count();
            away_chances += expected.iter().filter(|e| e.side == 1).count();
        }
        assert!(goals > 0 && away_chances > 0);
    }

    #[test]
    fn postponed_fixture_still_plays_stale_minutes() {
        let mut s = period_state();
        s.league_results[0][0][0] = NOT_PLAYED;
        let mut rt = Runtime::default();
        rt.league_goal_minutes[0][1][0] = 10;
        let start = Rng::new(5);
        let mut rng = start;
        match_live_period(
            &mut s,
            &mut rt,
            &mut rng,
            &mut Recorder::default(),
            1,
            0,
            1,
            45,
        );
        // No quick result; the away chance of minute 10 is played all the same.
        let (mut s2, mut rt2, mut rng2) = (period_state(), Runtime::default(), start);
        s2.league_results[0][0][0] = NOT_PLAYED;
        match_goal_chance(&mut s2, &mut rt2, &mut rng2, 0, 0, HOME, AWAY, 1, 10);
        assert_eq!(rng, rng2);
        assert_eq!(s.league_results[0][0][..2], s2.league_results[0][0][..2]);
    }

    #[test]
    fn paused_minutes_play_no_chances() {
        let (mut s, mut rt, mut rng) = (period_state(), Runtime::default(), Rng::new(1));
        let mut hooks = Recorder {
            pause_every_minute: true,
            ..Recorder::default()
        };
        let (mut s2, mut rt2, mut rng2) = (s.clone(), rt.clone(), rng);
        match_live_period(&mut s, &mut rt, &mut rng, &mut hooks, 1, 0, 1, 45);
        match_simulate_quick_result(&mut s2, &mut rt2, &mut rng2, 0, 0, HOME, AWAY, 1, 45, true);
        assert_eq!(rng, rng2);
        assert_eq!(s.league_results, s2.league_results);
    }

    #[test]
    fn panel_click_replans_the_rest_of_the_period() {
        let mut s = period_state();
        s.manager_count = 1;
        s.managers[0].team = HOME;
        let (mut rt, mut rng) = (Runtime::default(), Rng::new(1));
        let (mut s2, mut rt2, mut rng2) = (s.clone(), rt.clone(), rng);
        let mut hooks = Recorder {
            click: Some((10, 0)),
            ..Recorder::default()
        };
        match_live_period(&mut s, &mut rt, &mut rng, &mut hooks, 1, 0, 1, 45);
        match_live_period(
            &mut s2,
            &mut rt2,
            &mut rng2,
            &mut Recorder::default(),
            1,
            0,
            1,
            45,
        );
        assert_ne!(rng, rng2);
        for side in 0..2 {
            let mins = rt.league_goal_minutes[0][side];
            assert!(
                mins.iter().all(|&m| m == 0 || (10..=45).contains(&m)),
                "{mins:?}"
            );
        }
    }

    #[test]
    fn league_forfeit_awards_two_nil_and_enters_the_table() {
        let mut s = GameState::zeroed();
        s.competition_rounds = [1, 1, 1];
        let mut rt = Runtime::default();
        let mut hooks = Recorder {
            gate_answers: vec![1],
            ..Recorder::default()
        };
        s.current_manager = 2;
        matchday_play(&mut s, &mut rt, &mut Rng::new(1), &mut hooks, 1, 0);
        // Round 1 of tier 0 opens with club 0 at home to club 1.
        assert_eq!(s.matchday_pairings[0][0], [0, 1]);
        assert_eq!(s.league_results[0][0][..2], [0, 2]);
        assert_eq!(
            rt.forfeits,
            [Forfeit {
                side: 1,
                tier: 0,
                round_index: 0,
                slot: 0
            }]
        );
        assert_eq!(s.team_tables[0].played[0], 1);
        assert_eq!(s.team_tables[1].goals_for[1], 2);
        assert_eq!(s.current_manager, 0);
        let gates = hooks
            .calls
            .iter()
            .filter(|c| matches!(c, Call::Gate { .. }))
            .count();
        assert_eq!(gates, 9 + 10 + 10);
        assert_eq!(hooks.calls.last(), Some(&Call::Newspaper(0)));
    }

    #[test]
    fn league_day_skips_postponed_fixtures_and_credits_computer_scorers() {
        let mut s = GameState::zeroed();
        s.competition_rounds = [1, 1, 1];
        for club in 0..18 {
            s.clubs[club] = period_state().clubs[usize::from(HOME)].clone();
        }
        // Three sure scorer candidates in the pool for every tier-0 club.
        for p in 1..=54 {
            s.players[p].team = ((p - 1) / 3) as u8;
            s.players[p].position_value = 80;
        }
        s.league_results[0][0][2] = NOT_PLAYED;
        let mut hooks = Recorder::default();
        matchday_play(
            &mut s,
            &mut Runtime::default(),
            &mut Rng::new(1),
            &mut hooks,
            1,
            0,
        );
        let tier0_gates: Vec<u8> = hooks
            .calls
            .iter()
            .filter_map(|c| match c {
                Call::Gate { comp: 0, slot, .. } => Some(*slot),
                _ => None,
            })
            .collect();
        assert_eq!(tier0_gates, [0, 4, 6, 8, 10, 12, 14, 16]);
        assert_eq!(s.league_results[0][0][2..4], [NOT_PLAYED, 0]);
        let results = s.league_results[0][0];
        let clubs = s.matchday_pairings[0].as_flattened();
        let mut credited = 0;
        for (i, &club) in clubs[..18].iter().enumerate() {
            let goals = s.players[1..=54]
                .iter()
                .filter(|p| p.team == club)
                .map(|p| p.counters[0])
                .sum::<u8>();
            let scored = if results[i & !1] == NOT_PLAYED {
                0
            } else {
                results[i]
            };
            assert!(goals <= scored, "club {club}: {goals} of {scored}");
            credited += goals;
        }
        assert!(credited > 0);
    }

    /// DFB cup final (`HOME` vs `AWAY`) on a cup day.
    fn cup_final_state() -> GameState {
        let mut s = period_state();
        s.event_state[0] = 5;
        s.cup_pairings[0] = [HOME, AWAY];
        s
    }

    #[test]
    fn cup_day_plays_extra_time_when_a_tie_is_level() {
        let mut s = cup_final_state();
        let mut rt = Runtime::default();
        let mut rng = Rng::new(3);
        let mut hooks = Recorder {
            tie_answers: vec![true, false],
            ..Recorder::default()
        };
        matchday_play(&mut s, &mut rt, &mut rng, &mut hooks, 0, 1);
        assert_eq!(hooks.minutes, (1..=120).collect::<Vec<u8>>());
        let gate = Call::Gate {
            home: HOME,
            away: AWAY,
            cup_mask: 1,
            comp: 0,
            slot: 0,
            league_mask: 0,
        };
        let ties = |shootout| Call::Ties {
            cup_mask: 1,
            count: 2,
            shootout,
        };
        assert_eq!(
            hooks.calls,
            [
                gate,
                ties(false),
                ties(true),
                Call::RoundComplete(0),
                Call::Newspaper(1)
            ]
        );
        // Extra time plans one more chance per side, so the RNG differs from a day without it.
        let mut no_et = Rng::new(3);
        matchday_play(
            &mut cup_final_state(),
            &mut Runtime::default(),
            &mut no_et,
            &mut Recorder::default(),
            0,
            1,
        );
        assert_ne!(rng, no_et);
    }
    /// A European cup day: cup row 1 plays `HOME` v `AWAY` and `T1_HOME` v `T1_AWAY` in round 4
    /// while the DFB cup (row 0) is in its final.
    fn euro_day_state() -> GameState {
        let mut s = cup_final_state();
        for club in [T1_HOME, T1_AWAY] {
            s.clubs[usize::from(club)] = s.clubs[usize::from(HOME)].clone();
        }
        s.event_state[1] = 4;
        s.cup_pairings[16] = [HOME, AWAY];
        s.cup_pairings[17] = [T1_HOME, T1_AWAY];
        s
    }

    #[test]
    fn cup_days_play_their_own_rows_and_clear_played_minutes() {
        // (cup mask, pairing bytes, rows planned, rows played): European cups play rows 1..=3
        // whatever the mask.
        let cases: [(u8, u8, &[u8], &[u8]); 2] = [(1, 2, &[0], &[0]), (2, 4, &[1], &[1, 2, 3])];
        for (mask, bytes, planned, played) in cases {
            let mut s = euro_day_state();
            let mut rt = Runtime::default();
            // A stale minute in every row: only the rows of the day may play it.
            for row in &mut rt.cup_goal_minutes {
                row[0][7] = 30;
            }
            let (mut s2, mut rt2, mut rng2) = (s.clone(), rt.clone(), Rng::new(4));
            let mut rng = rng2;
            match_live_period(
                &mut s,
                &mut rt,
                &mut rng,
                &mut Recorder::default(),
                0,
                mask,
                1,
                45,
            );

            for &cup in planned {
                for slot in (0..bytes).step_by(2) {
                    let (h, a) = cup_pairing(&s2, cup, slot);
                    match_simulate_quick_result(
                        &mut s2,
                        &mut rt2,
                        &mut rng2,
                        cup + 10,
                        slot,
                        h,
                        a,
                        1,
                        45,
                        true,
                    );
                }
            }
            for minute in 1..=45 {
                for &cup in played {
                    for slot in (0..bytes).step_by(2) {
                        let (h, a) = cup_pairing(&s2, cup, slot);
                        for j in 0..8 {
                            for side in 0..2 {
                                let m = &mut rt2.cup_goal_minutes[usize::from(cup)];
                                if m[usize::from(slot + side)][j] == minute {
                                    m[usize::from(slot + side)][j] = 0;
                                    match_goal_chance(
                                        &mut s2,
                                        &mut rt2,
                                        &mut rng2,
                                        cup + 10,
                                        slot,
                                        h,
                                        a,
                                        side,
                                        minute.into(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            assert_eq!(rng, rng2, "mask {mask}");
            assert_eq!(s.cup_results, s2.cup_results, "mask {mask}");
            for cup in 0..4u8 {
                let minutes = rt.cup_goal_minutes[usize::from(cup)];
                if played.contains(&cup) {
                    assert!(
                        minutes[..usize::from(bytes)]
                            .iter()
                            .flatten()
                            .all(|&m| m == 0)
                    );
                } else {
                    assert_eq!(minutes[0][7], 30, "mask {mask} row {cup}");
                }
            }
        }
    }

    #[test]
    fn weather_postpones_only_in_winter() {
        let mut s = GameState::zeroed();
        s.competition_rounds = [1, 1, 1];
        for day in [0x18, 0x46] {
            let mut rng = Rng::new(1);
            league_postpone_matches_weather(&mut s, &Runtime::default(), &mut rng, day, 0);
            assert_eq!(rng, Rng::new(1));
        }
    }

    #[test]
    fn weather_without_a_free_slot_draws_the_count_only() {
        let mut s = GameState::zeroed();
        s.competition_rounds = [1, 1, 1];
        for e in &mut s.rescheduled_matches {
            e[RESCHEDULED_DAY] = 0x50;
        }
        let before = s.clone();
        for day in [0x19, 0x2c, 0x45] {
            for seed in 0..20 {
                let start = Rng::new(seed);
                let mut rng = start;
                // n = 9 - |day - 0x2c| / 3, maybe one less, then random_range(0, n).
                league_postpone_matches_weather(&mut s, &Runtime::default(), &mut rng, day, 0);
                assert_eq!(draws(start, rng), 2);
                assert_eq!(s, before);
            }
        }
    }

    #[test]
    fn weather_moves_drawn_pairings_to_later_days() {
        let mut s = GameState::zeroed();
        s.competition_rounds = [3, 1, 1];
        // Two games to move: n = 9 - random_range(0, 1), then random_range(0, n) == 2.
        let start = (0..=u16::MAX)
            .map(Rng::new)
            .find(|&r| nth_range(r, 1, 0, 9 - nth_range(r, 0, 0, 1)) == 2)
            .unwrap();
        let mut rng = start;
        league_postpone_matches_weather(&mut s, &Runtime::default(), &mut rng, 0x2c, 0);
        let moved: Vec<_> = s
            .rescheduled_matches
            .iter()
            .filter(|e| e[RESCHEDULED_DAY] != 0)
            .collect();
        assert_eq!(moved.len(), 2);
        assert_ne!(moved[0][RESCHEDULED_PAIR], moved[1][RESCHEDULED_PAIR]);
        for e in moved {
            // Day 0x2d lies in the winter break: the first day after it is 0x3b.
            assert_eq!(e[RESCHEDULED_DAY], 0x3b);
            assert_eq!(e[RESCHEDULED_ROUND], 3);
            let p = usize::from(e[RESCHEDULED_PAIR]);
            assert_eq!(s.league_results[0][2][2 * p], NOT_PLAYED);
        }
        assert_eq!(s.week_competitions[0x3b], RESCHEDULED_GAMES_DAY);
    }

    #[test]
    fn weather_count_shrinks_by_one_per_three_days_from_the_centre() {
        let mut s = GameState::zeroed();
        s.competition_rounds = [3, 1, 1];
        // Pair 0 was postponed before: it is never drawn again.
        s.league_results[0][2][0] = NOT_PLAYED;
        // Day 0x20: n = 9 - 12 / 3 = 5; no coin decrement, then random_range(0, 5) == 5. The
        // first pair drawn is pair 0.
        let start = (0..=u16::MAX)
            .map(Rng::new)
            .find(|&r| {
                nth_range(r, 0, 0, 1) == 0
                    && nth_range(r, 1, 0, 5) == 5
                    && nth_range(r, 2, 0, 8) == 0
            })
            .unwrap();
        let mut rng = start;
        league_postpone_matches_weather(&mut s, &Runtime::default(), &mut rng, 0x20, 0);
        let moved: Vec<u8> = s
            .rescheduled_matches
            .iter()
            .filter(|e| e[RESCHEDULED_DAY] != 0)
            .map(|e| e[RESCHEDULED_PAIR])
            .collect();
        assert_eq!(moved.len(), 5, "{moved:?}");
        assert!(!moved.contains(&0), "{moved:?}");
    }

    fn reschedule_state() -> GameState {
        let mut s = GameState::zeroed();
        s.competition_rounds = [1, 1, 1];
        s
    }

    #[test]
    fn reschedule_skips_busy_days_and_the_winter_break() {
        let mut s = reschedule_state();
        s.week_competitions[0x23] = 0x08;
        assert!(league_reschedule_match(&mut s, 0x22, 0, 4, 7));
        assert_eq!(s.rescheduled_matches[7], [0, 0x3b, 1, 0, 4]);
        assert_eq!(s.league_results[0][0][8], NOT_PLAYED);
        assert_eq!(s.week_competitions[0x3b], RESCHEDULED_GAMES_DAY);
        // A day with only rescheduled games is still usable.
        let mut s = reschedule_state();
        s.week_competitions[0x11] = RESCHEDULED_GAMES_DAY;
        assert!(league_reschedule_match(&mut s, 0x10, 0, 4, 0));
        assert_eq!(s.rescheduled_matches[0][RESCHEDULED_DAY], 0x11);
    }

    #[test]
    fn reschedule_avoids_a_club_playing_twice() {
        let mut s = reschedule_state();
        // Pair 0 of round 1 of tier 0 is already on day 0x11; pair 0 again conflicts with it.
        s.rescheduled_matches[0] = [0, 0x11, 1, 0, 0];
        assert!(league_reschedule_match(&mut s, 0x10, 0, 0, 1));
        assert_eq!(s.rescheduled_matches[1][RESCHEDULED_DAY], 0x12);
        // A different pair of the same round shares no club.
        assert!(league_reschedule_match(&mut s, 0x10, 0, 1, 2));
        assert_eq!(s.rescheduled_matches[2][RESCHEDULED_DAY], 0x11);
    }

    #[test]
    fn reschedule_refuses_a_tenth_game_on_a_day() {
        let mut s = reschedule_state();
        // Nine games on day 0x11, all from other rounds of tier 2 sharing no club with tier 0.
        for i in 0..9 {
            s.rescheduled_matches[i] = [0, 0x11, 1, 2, i as u8];
        }
        assert!(league_reschedule_match(&mut s, 0x10, 0, 3, 10));
        assert_eq!(s.rescheduled_matches[10][RESCHEDULED_DAY], 0x12);
        // With eight there is room.
        let mut s = reschedule_state();
        for i in 0..8 {
            s.rescheduled_matches[i] = [0, 0x11, 1, 2, i as u8];
        }
        assert!(league_reschedule_match(&mut s, 0x10, 0, 3, 10));
        assert_eq!(s.rescheduled_matches[10][RESCHEDULED_DAY], 0x11);
    }

    #[test]
    fn rescheduled_games_use_the_untruncated_round_base() {
        let mut s = GameState::zeroed();
        // Round 7: 20 * 7 = 140 does not fit an i8.
        s.competition_rounds = [7, 1, 1];
        s.rescheduled_matches[3] = [0, 0x30, 2, 1, 4];
        s.league_results[1][1][8] = NOT_PLAYED;
        s.league_results[0][6][19] = 0x55;
        s.week_competitions[0x30] = RESCHEDULED_GAMES_DAY;
        let mut reference = s.clone();
        let (mut rt, mut rng) = (Runtime::default(), Rng::new(8));
        let (mut rt2, mut rng2) = (rt.clone(), rng);
        let mut hooks = Recorder::default();
        matchday_play_rescheduled(&mut s, &mut rt, &mut rng, &mut hooks, 0x30);

        // The same game played by hand in tier 0's list.
        fixtures_build_round_pairings(&mut reference, 1, 2);
        let game = reference.matchday_pairings[1][4];
        reference.matchday_pairings[0][0] = game;
        reference.matchday_pairings[0][1][0] = NO_MATCH;
        matchday_play(
            &mut reference,
            &mut rt2,
            &mut rng2,
            &mut Recorder::default(),
            0,
            0,
        );
        assert_eq!(rng, rng2);
        let gate = Call::Gate {
            home: game[0],
            away: game[1],
            cup_mask: 0,
            comp: 0,
            slot: 0,
            league_mask: 0,
        };
        assert_eq!(hooks.calls, [gate, Call::Newspaper(0)]);
        assert_eq!(
            s.league_results[1][1][8..10],
            reference.league_results[0][6][..2]
        );
        assert_eq!(s.league_results[0][6][..19], [0; 19]);
        assert_eq!(s.league_results[0][6][19], 0x55);
        assert_eq!(s.rescheduled_matches[3], [0, 0, 2, 1, 4]);
        assert_eq!(s.week_competitions[0x30], 0);
    }
    #[test]
    fn match_day_on_real_saves_only_adds_goals_and_is_deterministic() {
        for (path, save) in crate::test_saves::save_states() {
            let rounds_ok = (0..3).all(|t| {
                let r = save.competition_rounds[t] as i8;
                (1..=crate::league::LEAGUE_ROUND_COUNT[t] as i8).contains(&r)
            });
            if !rounds_ok {
                continue;
            }
            let current = |s: &GameState| -> Vec<[u8; 20]> {
                (0..3)
                    .map(|t| s.league_results[t][usize::from(s.competition_rounds[t]) - 1])
                    .collect()
            };
            let play = || {
                let (mut s, mut rt, mut rng) = (save.clone(), Runtime::default(), Rng::new(1));
                matchday_play(&mut s, &mut rt, &mut rng, &mut Recorder::default(), 7, 0);
                (s, rt, rng)
            };
            let (s, rt, rng) = play();
            for (t, (before, after)) in current(&save).iter().zip(current(&s)).enumerate() {
                let teams = Runtime::default().tier_team_count[t] as usize;
                for slot in 0..teams {
                    let (b, a) = (before[slot], after[slot]);
                    if before[slot & !1] == NOT_PLAYED {
                        assert_eq!(a, b, "{path:?} tier {t} slot {slot}");
                    } else {
                        assert!(
                            (0..=16).contains(&a.wrapping_sub(b)),
                            "{path:?} tier {t} slot {slot}: {b} -> {a}"
                        );
                    }
                }
            }
            assert_ne!(current(&s), current(&save), "{path:?}: no goal scored");
            assert_eq!(play(), (s, rt, rng), "{path:?}");
        }
    }
}
