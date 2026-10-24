//! Match engine of segment 1310 of `BMMAIN.EXE` (`docs/re/logic/match.md`).
//!
//! Only the helpers that segment 1f9d ([`crate::ai`]) calls are ported so far; their semantics
//! are specified in `docs/re/logic/ai.md` §1.1, §2.7 and §2.8.

use std::ops::Range;

use bmp_formats::savegame::{BLOCK_COMPETITION_SLOTS, BLOCK_PLAYERS, BLOCKS, BlockDesc};

use crate::rng::{Rng, ldiv, long_abs_diff};
use crate::runtime::Runtime;
use crate::state::{GameState, Layout, Player};

/// DS:008e: weight in percent of each line (defence, midfield, attack) for the strength kinds
/// 0 = defence, 1 = overall, 2 = attack.
pub const LINE_WEIGHTS: [[u8; 3]; 3] = [[80, 20, 0], [25, 50, 25], [0, 20, 80]];

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
        // FAR:57bc..57dc); assumed 0.
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
