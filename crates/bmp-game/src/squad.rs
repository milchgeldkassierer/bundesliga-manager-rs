//! Squad logic of `BMMAIN.EXE` segment 2ecd: automatic lineup, tactics-board
//! placement and inserting/removing squad records.
//!
//! Every function works on the squad group of the current manager
//! (`g = FAR:304a`, [`GameState::current_manager`]). Groups 0..=3 are the
//! human squads, group 4 is the transfer list. Records stay sorted by player
//! id and packed: the first record with `player_id == 0` ends the list.
//!
//! Segment 2ecd does not change form, fitness, age, injuries or contracts;
//! `2ecd:3360` is the automatic lineup, not a weekly update. The squad screen
//! (`screen_squad_lineup` and its drawing helpers) is not part of this module.
//!
//! Full documentation with addresses: `docs/re/logic/squad.md`.

use crate::news::message_delete;
use crate::rng::{Rng, long_abs_diff};
use crate::state::{
    BOARD_UNPLACED, GameState, LOAN_PRICE_MODE, Layout, SQUAD_COUNT, SQUAD_SCAN, SquadPlayer,
    TRANSFER_LIST, TRANSFER_LIST_SCAN,
};
use crate::transfer::player_value_calc;

/// Highest lineup position of the starting eleven.
const LAST_STARTER: u8 = 11;
/// First lineup position of the bench.
const FIRST_SUBSTITUTE: u8 = 12;

/// DS:541c players per position (GK, DEF, MID, ATT) for the formations
/// 4-4-2, 3-5-2 and 3-4-3.
const FORMATION_COUNTS: [[u8; 4]; 3] = [[1, 4, 4, 2], [1, 3, 5, 2], [1, 3, 4, 3]];
/// DS:5428 first board column of a line with `n` players.
const LINE_FIRST_COL: [i8; 6] = [0, 3, 2, 1, 0, 0];
/// DS:542e board row per position (GK 7, DEF 6, MID 3, ATT 0).
const POSITION_ROW: [i8; 4] = [7, 6, 3, 0];

/// Lineup globals that are not part of a save game (set by
/// `office_menu_dispatcher` 18bc:0b5f).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineupRules {
    /// FAR:513e: suspended players may be selected.
    pub suspended_allowed: bool,
    /// FAR:56ee: highest lineup position, 11 + allowed substitutes (13 or 15).
    pub max_lineup_slot: i8,
}

/// Why [`squad_insert_player`] refused a player. The original shows a dialog
/// and returns 0x7f; showing the dialog is the caller's job here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertRejected {
    /// "Schon 12 Mann auf / dem Transfermarkt": `limit` is 12 and record 11
    /// is taken.
    TransferListFull,
    /// "Schon 24 Mann / im Team": the manager already has `limit` players.
    SquadFull,
}

/// The squad group of the current manager.
///
/// # Panics
/// When [`GameState::current_manager`] is not a group index (0..=4).
pub(crate) fn group(state: &GameState) -> usize {
    usize::try_from(state.current_manager)
        .ok()
        .filter(|&g| g < SQUAD_COUNT)
        .expect("FAR:304a is not a squad group")
}

fn is_starter(rec: &SquadPlayer) -> bool {
    (1..=LAST_STARTER).contains(&rec.lineup_position)
}

/// Availability bits 0-1: 0 available, 1 suspended (selectable only when
/// suspensions do not apply), 2 injured, 3 unavailable.
fn is_available(rec: &SquadPlayer, suspended_allowed: bool) -> bool {
    match rec.status & 3 {
        0 => true,
        1 => suspended_allowed,
        _ => false,
    }
}

/// The records of `group` that `squad_count_by_flag` scans: 12 of the
/// transfer list, 24 of a human squad.
fn counted_records(state: &GameState, group: usize) -> &[SquadPlayer] {
    let n = if group == TRANSFER_LIST {
        TRANSFER_LIST_SCAN
    } else {
        SQUAD_SCAN
    };
    &state.squads[group][..n]
}

/// Counts the records of `group` that hold a player, scanning 12 records of
/// the transfer list and 24 of a human squad. This is mode 0 of the original;
/// mode 1 is [`squad_count_starters`], mode 2 (substitutes) has no caller yet.
///
/// # Panics
/// When `group` is not below [`SQUAD_COUNT`].
///
/// Origin: BMMAIN.EXE 4091:1109 (squad_count_by_flag)
pub fn squad_count_listed(state: &GameState, group: usize) -> u8 {
    counted_records(state, group)
        .iter()
        .filter(|r| r.player_id != 0)
        .count() as u8
}

/// Counts the records of `group` on the pitch (lineup position 1..=11),
/// scanning the same records as [`squad_count_listed`].
///
/// # Panics
/// When `group` is not below [`SQUAD_COUNT`].
///
/// Origin: BMMAIN.EXE 4091:1109 (squad_count_by_flag, mode 1)
pub fn squad_count_starters(state: &GameState, group: usize) -> u8 {
    counted_records(state, group)
        .iter()
        .filter(|r| is_starter(r))
        .count() as u8
}

/// The squad group and record of player `id`: the listed records of the
/// active managers' squads, then the transfer list.
///
/// Origin: BMMAIN.EXE 4091:17b7 (find_player_by_id)
pub fn find_player_by_id(state: &GameState, id: u8) -> Option<(usize, usize)> {
    (0..SQUAD_COUNT)
        .filter(|&g| g < state.manager_count as usize || g == TRANSFER_LIST)
        .find_map(|g| {
            let n = usize::from(squad_count_listed(state, g));
            let i = state.squads[g][..n]
                .iter()
                .position(|r| r.player_id == id)?;
            Some((g, i))
        })
}

/// Number of players manager `owner` holds: every listed record of group
/// `owner` plus every listed record of the other active squads and the
/// transfer list whose player is owned by `owner`.
///
/// Origin: BMMAIN.EXE 212a:00b4 (count_players_of_owner)
pub fn count_players_of_owner(state: &GameState, owner: u8) -> u8 {
    let mut n = 0u8;
    for s in 0..SQUAD_COUNT {
        if s >= state.manager_count as usize && s != TRANSFER_LIST {
            continue;
        }
        let listed = squad_count_listed(state, s) as usize;
        for rec in &state.squads[s][..listed] {
            if s == owner as usize || state.players[rec.player_id as usize].owner_manager == owner {
                n = n.wrapping_add(1);
            }
        }
    }
    n
}

/// Renumbers the starting eleven 1..k in record order; bench positions stay.
/// The original can also redraw the number column; that half belongs to the
/// squad screen.
///
/// Origin: BMMAIN.EXE 2ecd:24cd (lineup_renumber_starters)
pub fn lineup_renumber_starters(state: &mut GameState) {
    let g = group(state);
    let mut n = 1u8;
    for rec in &mut state.squads[g][..SQUAD_SCAN] {
        if is_starter(rec) {
            rec.lineup_position = n;
            n += 1;
        }
    }
}

/// Index of the first starter on board cell (`col`, `row`).
///
/// Origin: BMMAIN.EXE 2ecd:11f9 (board_find_starter_at)
pub fn board_find_starter_at(state: &GameState, col: u8, row: u8) -> Option<u8> {
    state.squads[group(state)][..SQUAD_SCAN]
        .iter()
        .position(|r| {
            is_starter(r) && r.board_col as i16 == col as i16 && r.board_row as i16 == row as i16
        })
        .map(|i| i as u8)
}

/// Puts an unplaced player on the first free board cell, starting in the
/// row of his position code (GK and DEF 6..5, MID 4..3, ATT 2..1) and moving
/// down the board, wrapping from row 7 to row 0.
///
/// Origin: BMMAIN.EXE 2ecd:1266 (board_auto_place_player)
pub fn board_auto_place_player(state: &mut GameState, idx: u8) {
    let g = group(state);
    let rec = &state.squads[g][idx as usize];
    if rec.board_col != BOARD_UNPLACED {
        return;
    }
    let poscode = state.players[rec.player_id as usize].position_value;
    let mut row = (((7 * poscode as u16) as i16 / -100) as u8).wrapping_add(7);
    if row == 7 {
        row = 6;
    }
    loop {
        if let Some(col) = (0..8).find(|&c| board_find_starter_at(state, c, row).is_none()) {
            let rec = &mut state.squads[g][idx as usize];
            rec.board_col = col as i8;
            rec.board_row = row as i8;
            return;
        }
        row = row.wrapping_add(1);
        if row == 8 {
            row = 0;
        }
    }
}

/// Picks the strongest unselected, available player of position `pos`
/// (0 GK, 1 DEF, 2 MID, 3 ATT) among records 0..24, cycling through the
/// other positions while none is found. Ties go to the later record. A
/// goalkeeper found while seeking an outfield player rates -1 (never picked)
/// the first time and 1 after that. `thr`, unless it is -1, is raised to
/// the value of every new best candidate.
///
/// The original hangs if an available player has a position code of 100 or
/// more; the game never generates one.
///
/// Origin: BMMAIN.EXE 2ecd:3635 (auto_lineup_pick_best)
fn auto_lineup_pick_best(
    state: &GameState,
    mut pos: i8,
    thr: &mut i16,
    suspended_allowed: bool,
) -> Option<u8> {
    let g = group(state);
    let start = pos;
    // 3-4-3 searches forward from the midfield, the other formations backward.
    let step: i8 = if pos == 3 || (pos == 2 && (state.lineup_state[g][0] as i8) < 4) {
        -1
    } else {
        1
    };
    let mut best = None;
    let mut best_val: i16 = 0;
    let mut gk_seen = false;
    let mut any = false;
    loop {
        for (i, rec) in state.squads[g][..SQUAD_SCAN].iter().enumerate() {
            if rec.player_id == 0
                || rec.lineup_position != 0
                || !is_available(rec, suspended_allowed)
            {
                continue;
            }
            any = true;
            let player_pos = state.players[rec.player_id as usize].position_value / 25;
            if player_pos as i16 != pos as i16 {
                continue;
            }
            let mut val = rec.average_skill();
            if player_pos == 0 && start != 0 {
                val = if gk_seen { 1 } else { -1 };
                gk_seen = true;
            }
            if best_val > val {
                continue;
            }
            if *thr != -1 && *thr < val {
                *thr = val;
            }
            best_val = val;
            best = Some(i as u8);
        }
        pos += step;
        if pos == 4 {
            pos = 0;
        }
        if pos == -1 {
            pos = 3;
        }
        if (pos == start && gk_seen) || !any || best.is_some() {
            return best;
        }
    }
}

/// Swaps the board columns of starters in one row so that players stand
/// closer to the column given by their `attr_20`. Both distances use the
/// outer record's column, as the original does.
///
/// Origin: BMMAIN.EXE 1cb5:25d5 (event_helper_25d5, squad_fix_position_mismatch)
fn squad_fix_position_mismatch(state: &mut GameState) {
    let g = group(state);
    let players = &state.players;
    let squad = &mut state.squads[g];
    let distance =
        |col: i8, pid: u8| long_abs_diff(col as i32, players[pid as usize].attr_20 as i32) as u8;
    let placed_starter = |r: &SquadPlayer| r.player_id != 0 && is_starter(r);
    for i in 0..SQUAD_SCAN {
        let (col, row) = (squad[i].board_col, squad[i].board_row);
        let d = distance(col, squad[i].player_id);
        if !placed_starter(&squad[i]) || d <= 1 {
            continue;
        }
        let mut best = 99u8;
        let mut partner = None;
        for (j, t) in squad[..SQUAD_SCAN].iter().enumerate() {
            if j == i || !placed_starter(t) || t.board_row != row {
                continue;
            }
            let dj = distance(col, t.player_id);
            if dj < best {
                best = dj;
                partner = Some(j);
            }
        }
        if let Some(j) = partner
            && d > best
        {
            squad[i].board_col = squad[j].board_col;
            squad[j].board_col = col;
        }
    }
}

/// Selects the starting eleven and the bench of the current manager for the
/// formation in `lineup_state[g][0]` (2 = 4-4-2, 3 = 3-5-2, 4 = 3-4-3) and
/// places the starters on the tactics board. Value 1 means the automatic
/// lineup is off; the game stores no other values, and the original would
/// read outside its formation table for them, so they leave the squad
/// unchanged here.
///
/// # Panics
/// When [`GameState::current_manager`] is not a manager (0..=3); the
/// transfer list has no lineup.
///
/// Origin: BMMAIN.EXE 2ecd:3360 (auto_lineup)
pub fn auto_lineup(state: &mut GameState, rules: LineupRules) {
    let g = group(state);
    let formation = match (state.lineup_state[g][0] as i8).wrapping_sub(2) {
        f @ 0..=2 => f as usize,
        _ => return,
    };
    for rec in &mut state.squads[g][..SQUAD_SCAN] {
        rec.lineup_position = 0;
    }

    let mut next_col = [0i8; 4];
    let mut thr: i16 = -1;
    let mut slot: u8 = 1;
    for pos in 0..4 {
        let line = FORMATION_COUNTS[formation][pos];
        for _ in 0..line {
            let Some(idx) =
                auto_lineup_pick_best(state, pos as i8, &mut thr, rules.suspended_allowed)
            else {
                break;
            };
            let rec = &mut state.squads[g][idx as usize];
            rec.lineup_position = slot;
            slot = slot.wrapping_add(1);
            rec.board_row = POSITION_ROW[pos];
            rec.board_col = LINE_FIRST_COL[line as usize].wrapping_add(next_col[pos]);
            // 4-4-2 pushes starters #2 and #5 (the full-backs) up one row.
            if formation == 0 {
                if slot == 3 {
                    rec.board_row -= 1;
                    rec.board_col += 1;
                }
                if slot == 6 {
                    rec.board_row -= 1;
                    rec.board_col -= 1;
                }
            }
            next_col[pos] += 2;
            if line == 5 && next_col[pos] != 2 && next_col[pos] != 6 {
                next_col[pos] -= 1;
            }
        }
    }

    if rules.max_lineup_slot == 15 {
        // One substitute per position: 12 GK, 13 DEF, 14 MID, 15 ATT.
        let mut thr: i16 = -1;
        for (pos, slot) in (0..4).zip(FIRST_SUBSTITUTE..) {
            let Some(idx) = auto_lineup_pick_best(state, pos, &mut thr, rules.suspended_allowed)
            else {
                break;
            };
            state.squads[g][idx as usize].lineup_position = slot;
        }
    } else {
        // A goalkeeper gets 12, the strongest outfield pick gets 13.
        let mut thr: i16 = 0;
        let mut slot = FIRST_SUBSTITUTE;
        let mut bench_best = None;
        for pos in 0..4 {
            let prev = thr;
            let picked = auto_lineup_pick_best(state, pos, &mut thr, rules.suspended_allowed);
            if slot > FIRST_SUBSTITUTE && prev != thr {
                bench_best = picked;
            }
            let Some(idx) = picked else {
                break;
            };
            if slot == FIRST_SUBSTITUTE {
                thr = 0;
                state.squads[g][idx as usize].lineup_position = slot;
                slot += 1;
            }
        }
        // TODO(re): bench_best is an uninitialised stack byte in the original
        // when no outfield pick raised thr (squad.md §3.1, §6.2), so slot 13 goes
        // to an arbitrary record; the write is skipped here.
        if let Some(idx) = bench_best {
            state.squads[g][idx as usize].lineup_position = slot;
        }
    }

    squad_fix_position_mismatch(state);
    lineup_renumber_starters(state);
}

/// Removes record `idx` from the current group, deleting its pending
/// message, by moving records `idx + 1..=count` down one place; record
/// `count` keeps its stale bytes except for the cleared player id. Returns
/// `false` (and changes nothing) when the player is owned by another manager
/// and `any_owner` is not set.
///
/// # Panics
/// When `count` is above 24 or `idx` above 24.
///
/// Origin: BMMAIN.EXE 2ecd:10ee (squad_remove_player)
pub fn squad_remove_player(state: &mut GameState, idx: u8, any_owner: bool, count: u8) -> bool {
    let g = group(state);
    let pid = state.squads[g][idx as usize].player_id;
    if pid == 0 {
        return true;
    }
    if state.players[pid as usize].owner_manager as i16 != state.current_manager as i16
        && !any_owner
    {
        return false;
    }
    message_delete(state, state.squads[g][idx as usize].message_ptr);
    let squad = &mut state.squads[g];
    for k in idx as usize..count as usize {
        squad[k] = squad[k + 1].clone();
    }
    squad[count as usize].player_id = 0;
    true
}

/// Rolls the tendency and the trait bits of a new squad record
/// (4 `random_range` calls).
///
/// Origin: BMMAIN.EXE 3277:000a (player_init_random_attrs)
pub fn player_init_random_attrs(rng: &mut Rng, rec: &mut SquadPlayer) {
    rec.tendency = rng.random_range(0x23, 0x41) as u8;
    rec.traits = rng.random_range(1, 0x0d) as u8;
    rec.traits |= (rng.random_range(1, 3) << 4) as u8;
    rec.traits |= (rng.random_range(0, 1) << 7) as u8;
}

/// Inserts player `pid` into the current group at its sorted place and
/// returns the record index. A player who joins from another club gets the
/// manager's club and his two club counters cleared (not for the transfer
/// list).
///
/// The record's `market_value` is the salary from [`player_value_calc`]
/// (mode 3 for a loan of [`LOAN_PRICE_MODE`], 1 otherwise; no random draws).
/// The original's unused `long` argument is dropped.
///
/// Origin: BMMAIN.EXE 2ecd:37d8 (squad_insert_player)
pub fn squad_insert_player(
    state: &mut GameState,
    rng: &mut Rng,
    pid: u8,
    contract: u8,
    loan: u8,
    limit: u8,
) -> Result<u8, InsertRejected> {
    let g = group(state);
    if usize::from(limit) == TRANSFER_LIST_SCAN
        && state.squads[g][TRANSFER_LIST_SCAN - 1].player_id != 0
    {
        return Err(InsertRejected::TransferListFull);
    }
    let joins_club =
        g != TRANSFER_LIST && state.players[pid as usize].team != state.managers[g].team;
    if joins_club && count_players_of_owner(state, g as u8) as i16 > limit as i16 - 1 {
        return Err(InsertRejected::SquadFull);
    }

    let squad = &mut state.squads[g];
    let mut idx = 0;
    while squad[idx].player_id != 0 && squad[idx].player_id < pid && idx < SQUAD_SCAN {
        idx += 1;
    }
    if squad[idx].player_id != 0 {
        for k in (idx..SQUAD_SCAN).rev() {
            squad[k + 1] = squad[k].clone();
        }
    }
    let mut rec = SquadPlayer::zeroed();
    rec.player_id = pid;
    rec.exhaustion = rng.random_range(0x50, 0x78) as u8;
    player_init_random_attrs(rng, &mut rec);
    rec.contract_years = contract;
    rec.loan = loan;
    rec.skills = state.players[pid as usize].skills;
    state.squads[g][idx] = rec;

    let mode = if loan == LOAN_PRICE_MODE { 3 } else { 1 };
    state.squads[g][idx].market_value = player_value_calc(state, rng, idx as u8, mode);
    if joins_club {
        let team = state.managers[g].team;
        let player = &mut state.players[pid as usize];
        player.counters = [0, 0];
        player.team = team;
    }
    Ok(idx as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::NO_OWNER;
    use crate::test_saves::save_states;

    const GK: u8 = 0;
    const DEF: u8 = 25;
    const MID: u8 = 50;
    const ATT: u8 = 75;

    const FOUR_SUBS: LineupRules = LineupRules {
        suspended_allowed: false,
        max_lineup_slot: 15,
    };
    const TWO_SUBS: LineupRules = LineupRules {
        suspended_allowed: false,
        max_lineup_slot: 13,
    };

    /// One active manager (group 0) with the 4-4-2 automatic lineup.
    fn state() -> GameState {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.lineup_state[0][0] = 2;
        s
    }

    /// Puts player `pid` with position code `poscode` and all three skills
    /// equal to `strength` into record `i` of group 0, unplaced.
    fn put(s: &mut GameState, i: usize, pid: u8, poscode: u8, strength: u8) {
        let p = &mut s.players[pid as usize];
        p.position_value = poscode;
        p.skills = [strength; 3];
        let r = &mut s.squads[0][i];
        r.player_id = pid;
        r.skills = [strength; 3];
        r.board_col = BOARD_UNPLACED;
    }

    /// Fills group 0 with `lines` players per position (GK, DEF, MID, ATT),
    /// stronger with every record inside a position, so each position is
    /// picked from its last record backwards. Returns the next free record.
    fn put_lines(s: &mut GameState, lines: [usize; 4], base: u8) -> usize {
        let mut i = 0;
        for (pos, &n) in lines.iter().enumerate() {
            for k in 0..n {
                let pid = 1 + (pos as u8) * 40 + k as u8 + base;
                put(s, i, pid, pos as u8 * 25, 50 + 5 * k as u8 + base);
                i += 1;
            }
        }
        i
    }

    fn slots(s: &GameState) -> Vec<u8> {
        s.squads[0][..SQUAD_SCAN]
            .iter()
            .map(|r| r.lineup_position)
            .collect()
    }

    fn cell(s: &GameState, i: usize) -> (i8, i8) {
        (s.squads[0][i].board_col, s.squads[0][i].board_row)
    }

    /// Sorted board columns of the starters on `row`.
    fn row_cols(s: &GameState, row: i8) -> Vec<i8> {
        let mut v: Vec<i8> = s.squads[0][..SQUAD_SCAN]
            .iter()
            .filter(|r| is_starter(r) && r.board_row == row)
            .map(|r| r.board_col)
            .collect();
        v.sort();
        v
    }

    #[test]
    fn starter_count_scans_the_listed_records_only() {
        let mut s = state();
        for (i, v) in [0, 1, 11, 12].into_iter().enumerate() {
            s.squads[0][i].lineup_position = v;
        }
        s.squads[0][24].lineup_position = 3;
        assert_eq!(squad_count_starters(&s, 0), 2);
        s.squads[TRANSFER_LIST][11].lineup_position = 1;
        s.squads[TRANSFER_LIST][12].lineup_position = 1;
        assert_eq!(squad_count_starters(&s, TRANSFER_LIST), 1);
    }

    #[test]
    fn renumber_starters_in_record_order() {
        let mut s = state();
        for (i, v) in [5, 0, 12, 2, 11].into_iter().enumerate() {
            s.squads[0][i].lineup_position = v;
        }
        s.squads[0][24].lineup_position = 7;
        lineup_renumber_starters(&mut s);
        assert_eq!(&slots(&s)[..5], &[1, 0, 12, 2, 3]);
        assert_eq!(s.squads[0][24].lineup_position, 7);
    }

    #[test]
    fn board_auto_place_rows() {
        for (poscode, row) in [(0, 6), (25, 6), (41, 5), (50, 4), (66, 3), (75, 2), (99, 1)] {
            let mut s = state();
            put(&mut s, 0, 1, poscode, 50);
            s.squads[0][0].lineup_position = 1;
            board_auto_place_player(&mut s, 0);
            assert_eq!(cell(&s, 0), (0, row), "poscode {poscode}");
        }
    }

    #[test]
    fn board_auto_place_skips_taken_cells() {
        let mut s = state();
        // Row 6 has starters in columns 0 and 1, and a bench player in 2.
        for (i, (col, slot)) in [(0, 1), (1, 2), (2, 12)].into_iter().enumerate() {
            put(&mut s, i, i as u8 + 1, DEF, 50);
            s.squads[0][i].lineup_position = slot;
            s.squads[0][i].board_col = col;
            s.squads[0][i].board_row = 6;
        }
        put(&mut s, 3, 4, GK, 50);
        s.squads[0][3].lineup_position = 3;
        board_auto_place_player(&mut s, 3);
        assert_eq!(cell(&s, 3), (2, 6));
        // An already placed player stays where he is.
        s.squads[0][3].board_col = 5;
        board_auto_place_player(&mut s, 3);
        assert_eq!(cell(&s, 3), (5, 6));
        assert_eq!(board_find_starter_at(&s, 1, 6), Some(1));
        assert_eq!(board_find_starter_at(&s, 2, 6), None);
        assert_eq!(board_find_starter_at(&s, 5, 6), Some(3));
    }

    #[test]
    fn board_auto_place_wraps_after_row_7() {
        let mut s = state();
        for i in 0..16 {
            put(&mut s, i, i as u8 + 1, DEF, 50);
            s.squads[0][i].lineup_position = 1;
            s.squads[0][i].board_col = (i % 8) as i8;
            s.squads[0][i].board_row = 6 + (i / 8) as i8;
        }
        put(&mut s, 16, 17, GK, 50);
        s.squads[0][16].lineup_position = 2;
        board_auto_place_player(&mut s, 16);
        assert_eq!(cell(&s, 16), (0, 0));
    }

    #[test]
    fn pick_best_availability() {
        let mut s = state();
        put(&mut s, 0, 21, DEF, 90);
        s.squads[0][0].status = 2;
        put(&mut s, 1, 22, DEF, 90);
        s.squads[0][1].status = 3;
        put(&mut s, 2, 23, DEF, 80);
        s.squads[0][2].status = 0x41; // suspended, transfer flag set
        put(&mut s, 3, 24, DEF, 50);
        let mut thr = -1;
        assert_eq!(auto_lineup_pick_best(&s, 1, &mut thr, false), Some(3));
        assert_eq!(auto_lineup_pick_best(&s, 1, &mut thr, true), Some(2));
        assert_eq!(thr, -1);
    }

    #[test]
    fn pick_best_ties_threshold_and_record_24() {
        let mut s = state();
        put(&mut s, 0, 21, DEF, 50);
        put(&mut s, 1, 22, DEF, 50);
        s.squads[0][1].skills = [50, 50, 52]; // 152 / 3 = 50
        let mut thr = 0;
        assert_eq!(auto_lineup_pick_best(&s, 1, &mut thr, false), Some(1));
        assert_eq!(thr, 50);
        // Only a player in record 24 is never found.
        let mut s = state();
        put(&mut s, 24, 21, DEF, 50);
        assert_eq!(auto_lineup_pick_best(&s, 1, &mut -1, false), None);
    }

    #[test]
    fn pick_best_cycles_positions() {
        // No defender: the forward search from DEF finds the midfielder.
        let mut s = state();
        put(&mut s, 0, 1, GK, 90);
        put(&mut s, 1, 64, MID, 40);
        put(&mut s, 2, 106, ATT, 60);
        assert_eq!(auto_lineup_pick_best(&s, 1, &mut -1, false), Some(1));
        // MID searches backward in 4-4-2 (DEF, then GK) but forward in 3-4-3.
        let mut s = state();
        put(&mut s, 0, 21, DEF, 40);
        put(&mut s, 1, 106, ATT, 60);
        assert_eq!(auto_lineup_pick_best(&s, 2, &mut -1, false), Some(0));
        s.lineup_state[0][0] = 4;
        assert_eq!(auto_lineup_pick_best(&s, 2, &mut -1, false), Some(1));
    }

    #[test]
    fn pick_best_goalkeeper_fallback() {
        let mut s = state();
        put(&mut s, 0, 1, GK, 90);
        assert_eq!(auto_lineup_pick_best(&s, 1, &mut -1, false), None);
        put(&mut s, 1, 2, GK, 30);
        // The first goalkeeper rates -1, the second 1.
        let mut thr = 0;
        assert_eq!(auto_lineup_pick_best(&s, 1, &mut thr, false), Some(1));
        assert_eq!(thr, 1);
        // Seeking a goalkeeper rates goalkeepers normally.
        assert_eq!(auto_lineup_pick_best(&s, 0, &mut -1, false), Some(0));
    }

    #[test]
    fn auto_lineup_442_board() {
        let mut s = state();
        let next = put_lines(&mut s, [1, 4, 4, 2], 0);
        assert_eq!(next, 11);
        // Pick order is strongest first, i.e. the last record of each line.
        let expected: [(usize, (i8, i8)); 11] = [
            (0, (3, 7)),
            (4, (1, 5)),
            (3, (2, 6)),
            (2, (4, 6)),
            (1, (5, 5)),
            (8, (0, 3)),
            (7, (2, 3)),
            (6, (4, 3)),
            (5, (6, 3)),
            (10, (2, 0)),
            (9, (4, 0)),
        ];
        for &(i, (col, _)) in &expected {
            let pid = s.squads[0][i].player_id;
            s.players[pid as usize].attr_20 = col as u8;
        }
        auto_lineup(&mut s, FOUR_SUBS);
        for &(i, c) in &expected {
            assert_eq!(cell(&s, i), c, "record {i}");
        }
        assert_eq!(&slots(&s)[..12], &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0]);
    }

    #[test]
    fn auto_lineup_352_and_343_columns() {
        let mut s = state();
        s.lineup_state[0][0] = 3;
        put_lines(&mut s, [1, 3, 5, 2], 0);
        auto_lineup(&mut s, FOUR_SUBS);
        assert_eq!(row_cols(&s, 7), [3]);
        assert_eq!(row_cols(&s, 6), [1, 3, 5]);
        assert_eq!(row_cols(&s, 3), [0, 2, 3, 4, 6]);
        assert_eq!(row_cols(&s, 0), [2, 4]);

        let mut s = state();
        s.lineup_state[0][0] = 4;
        put_lines(&mut s, [1, 3, 4, 3], 0);
        auto_lineup(&mut s, FOUR_SUBS);
        assert_eq!(row_cols(&s, 6), [1, 3, 5]);
        assert_eq!(row_cols(&s, 3), [0, 2, 4, 6]);
        assert_eq!(row_cols(&s, 0), [1, 3, 5]);
    }

    /// 4-4-2 starters in records 0..11, then a goalkeeper (40), defender
    /// (30), midfielder (45) and forward (35) for the bench.
    fn squad_with_bench() -> GameState {
        let mut s = state();
        let i = put_lines(&mut s, [1, 4, 4, 2], 0);
        put(&mut s, i, 150, GK, 40);
        put(&mut s, i + 1, 149, DEF, 30);
        put(&mut s, i + 2, 147, MID, 45);
        put(&mut s, i + 3, 146, ATT, 35);
        s
    }

    #[test]
    fn auto_lineup_four_substitutes() {
        let mut s = squad_with_bench();
        auto_lineup(&mut s, FOUR_SUBS);
        assert_eq!(&slots(&s)[11..16], &[12, 13, 14, 15, 0]);
    }

    #[test]
    fn auto_lineup_two_substitutes_take_the_strongest_outfielder() {
        let mut s = squad_with_bench();
        auto_lineup(&mut s, TWO_SUBS);
        assert_eq!(&slots(&s)[11..16], &[12, 0, 13, 0, 0]);
    }

    #[test]
    fn auto_lineup_two_substitutes_skip_unassigned_bench_best() {
        let mut s = state();
        let i = put_lines(&mut s, [1, 4, 4, 2], 0);
        put(&mut s, i, 2, GK, 40);
        put(&mut s, i + 1, 3, GK, 30);
        auto_lineup(&mut s, TWO_SUBS);
        // The 40 goalkeeper is the substitute; the other one gets nothing
        // (the original writes slot 13 through an uninitialised index).
        assert_eq!(&slots(&s)[11..13], &[12, 0]);
        assert!(!slots(&s).contains(&13));
    }

    #[test]
    fn auto_lineup_off_or_unknown_formation_changes_nothing() {
        for value in [0, 1, 5] {
            let mut s = squad_with_bench();
            s.squads[0][3].lineup_position = 9;
            s.lineup_state[0][0] = value;
            let before = s.clone();
            auto_lineup(&mut s, FOUR_SUBS);
            assert_eq!(s, before);
        }
    }

    #[test]
    fn auto_lineup_leaves_record_24() {
        let mut s = squad_with_bench();
        put(&mut s, 24, 120, ATT, 99);
        s.squads[0][24].lineup_position = 4;
        auto_lineup(&mut s, FOUR_SUBS);
        assert_eq!(s.squads[0][24].lineup_position, 4);
        assert_eq!(s.squads[0][24].board_col, BOARD_UNPLACED);
    }

    #[test]
    fn fix_position_mismatch_swaps_columns() {
        let mut s = state();
        // (record, col, attr_20) in row 3, plus a bench player in the row.
        for (i, (col, attr)) in [(0, 6), (6, 3), (2, 1), (4, 4)].into_iter().enumerate() {
            put(&mut s, i, 64 + i as u8, MID, 50);
            s.players[64 + i].attr_20 = attr;
            let r = &mut s.squads[0][i];
            r.lineup_position = i as u8 + 1;
            r.board_col = col;
            r.board_row = 3;
        }
        put(&mut s, 4, 70, MID, 50);
        s.squads[0][4].lineup_position = 12;
        s.squads[0][4].board_col = 0;
        s.squads[0][4].board_row = 3;
        squad_fix_position_mismatch(&mut s);
        // Record 0 (col 0, d 6): the partner is the starter whose attr_20 is
        // nearest to column 0, record 2 (attr 1), so 0 and 2 swap.
        // Record 1 (col 6, d 3): record 0 (attr 6) now fits column 6
        // exactly, so 1 and 0 swap. Records 2 and 3 are within 1.
        assert_eq!(cell(&s, 0), (6, 3));
        assert_eq!(cell(&s, 1), (2, 3));
        assert_eq!(cell(&s, 2), (0, 3));
        assert_eq!(cell(&s, 3), (4, 3));
        assert_eq!(cell(&s, 4), (0, 3));
    }

    #[test]
    fn fix_position_mismatch_keeps_close_players() {
        let mut s = state();
        for (i, (col, attr)) in [(0, 1), (6, 3)].into_iter().enumerate() {
            put(&mut s, i, 64 + i as u8, MID, 50);
            s.players[64 + i].attr_20 = attr;
            let r = &mut s.squads[0][i];
            r.lineup_position = 1;
            r.board_col = col;
            r.board_row = 3;
        }
        // Record 1 is 3 away from its column; record 0 (attr 1) would be 5.
        squad_fix_position_mismatch(&mut s);
        assert_eq!((cell(&s, 0), cell(&s, 1)), ((0, 3), (6, 3)));
    }

    fn rolled(rng: &mut Rng) -> (u8, u8, u8) {
        let exhaustion = rng.random_range(0x50, 0x78) as u8;
        let tendency = rng.random_range(0x23, 0x41) as u8;
        let mut traits = rng.random_range(1, 13) as u8;
        traits |= (rng.random_range(1, 3) as u8) << 4;
        traits |= (rng.random_range(0, 1) as u8) << 7;
        (exhaustion, tendency, traits)
    }

    #[test]
    fn insert_sorted_with_rng_order() {
        let mut s = state();
        s.managers[0].team = 1;
        put(&mut s, 0, 5, DEF, 50);
        put(&mut s, 1, 9, MID, 50);
        s.squads[0][1].exhaustion = 77;
        let p = &mut s.players[7];
        p.skills = [61, 62, 63];
        p.team = 3;
        p.counters = [4, 5];

        let mut rng = Rng::new(1234);
        let mut expected_rng = rng;
        let attrs = rolled(&mut expected_rng);
        assert_ne!(attrs, (0, 0, 0));
        let idx = squad_insert_player(&mut s, &mut rng, 7, 2, LOAN_PRICE_MODE, 24);
        assert_eq!(idx, Ok(1));
        // The salary formula draws no random numbers.
        assert_eq!(rng, expected_rng);
        let value = player_value_calc(&s, &mut rng, 1, 3);
        assert_eq!(rng, expected_rng);

        let r = &s.squads[0][1];
        assert_eq!((r.exhaustion, r.tendency, r.traits), attrs);
        assert_eq!((r.contract_years, r.loan), (2, LOAN_PRICE_MODE));
        assert_eq!(r.skills, [61, 62, 63]);
        assert_eq!(r.market_value, value);
        assert_eq!(r.board_col, 0); // a fresh, zeroed record
        assert_eq!(s.squads[0][2].player_id, 9);
        assert_eq!(s.squads[0][2].exhaustion, 77);
        assert_eq!(s.players[7].team, 1);
        assert_eq!(s.players[7].counters, [0, 0]);

        // Appending after the last player, own club: mode 1, club untouched.
        s.players[20].team = 1;
        s.players[20].counters = [4, 5];
        let idx = squad_insert_player(&mut s, &mut rng, 20, 1, 0, 24);
        assert_eq!(idx, Ok(3));
        assert_eq!(
            s.squads[0][3].market_value,
            player_value_calc(&s, &mut rng, 3, 1)
        );
        assert_eq!(s.players[20].counters, [4, 5]);
    }

    #[test]
    fn insert_into_transfer_list_keeps_club() {
        let mut s = state();
        s.current_manager = 4;
        s.players[7].team = 3;
        s.players[7].counters = [4, 5];
        let idx = squad_insert_player(&mut s, &mut Rng::new(1), 7, 1, 0, 12);
        assert_eq!(idx, Ok(0));
        assert_eq!(s.squads[4][0].player_id, 7);
        assert_eq!(s.players[7].team, 3);
        assert_eq!(s.players[7].counters, [4, 5]);
    }

    #[test]
    fn insert_rejections_consume_no_rng() {
        let mut s = state();
        s.current_manager = 4;
        s.squads[4][11].player_id = 30;
        let mut rng = Rng::new(99);
        let before = s.clone();
        let r = squad_insert_player(&mut s, &mut rng, 7, 1, 0, 12);
        assert_eq!(r, Err(InsertRejected::TransferListFull));
        assert_eq!(s, before);

        let mut s = state();
        s.managers[0].team = 1;
        put(&mut s, 0, 5, DEF, 50);
        put(&mut s, 1, 9, DEF, 50);
        s.players[7].team = 2;
        let before = s.clone();
        let r = squad_insert_player(&mut s, &mut rng, 7, 1, 0, 2);
        assert_eq!(r, Err(InsertRejected::SquadFull));
        assert_eq!(s, before);
        // A player of the own club is not counted against the limit.
        s.players[7].team = 1;
        assert!(squad_insert_player(&mut s, &mut rng.clone(), 7, 1, 0, 2).is_ok());
        assert_eq!(rng, Rng::new(99));
    }

    #[test]
    fn remove_shifts_and_keeps_stale_tail() {
        let mut s = state();
        for (i, pid) in [5, 7, 9].into_iter().enumerate() {
            put(&mut s, i, pid, DEF, 50);
        }
        s.squads[0][24].player_id = 99;
        s.squads[0][24].exhaustion = 77;
        s.players[5].owner_manager = 1;

        let before = s.clone();
        assert!(!squad_remove_player(&mut s, 0, false, 24));
        assert_eq!(s, before);
        assert!(squad_remove_player(&mut s, 3, false, 24)); // empty record

        s.squads[0][0].message_ptr = 0x1234_5678;
        s.messages = vec![vec![bmp_formats::savegame::StringEntry {
            old_pointer: 0x1234_5678,
            bytes: b"X\0".to_vec(),
        }]];
        s.message_counts[0] = 1;
        assert!(squad_remove_player(&mut s, 0, true, 24));
        assert_eq!((s.messages[0].len(), s.message_counts[0]), (0, 0));
        let ids: Vec<u8> = s.squads[0].iter().map(|r| r.player_id).collect();
        assert_eq!(&ids[..3], &[7, 9, 0]);
        assert_eq!((ids[23], ids[24]), (99, 0));
        assert_eq!(s.squads[0][24].exhaustion, 77);

        // Own player, shorter list: only records up to `count` move.
        s.players[7].owner_manager = 0;
        assert!(squad_remove_player(&mut s, 0, false, 2));
        assert_eq!(s.squads[0][0].player_id, 9);
        assert_eq!(s.squads[0][1].player_id, 0);
        assert_eq!(s.squads[0][23].player_id, 99);
    }

    #[test]
    fn counts() {
        let mut s = state();
        for p in &mut s.players {
            p.owner_manager = NO_OWNER;
        }
        put(&mut s, 0, 5, DEF, 50);
        put(&mut s, 1, 6, DEF, 50);
        s.squads[0][24].player_id = 8; // never counted
        assert_eq!(squad_count_listed(&s, 0), 2);

        for i in 0..13 {
            s.squads[4][i].player_id = 20 + i as u8;
        }
        assert_eq!(squad_count_listed(&s, 4), 12);
        // Group 1 is not active, so its player owned by 0 is not counted.
        s.squads[1][0].player_id = 40;
        s.players[40].owner_manager = 0;
        s.players[21].owner_manager = 0;
        s.players[32].owner_manager = 0; // record 12 of the transfer list
        assert_eq!(count_players_of_owner(&s, 0), 3);
        s.manager_count = 2;
        assert_eq!(count_players_of_owner(&s, 0), 4);
        assert_eq!(count_players_of_owner(&s, 1), 1);
    }

    #[test]
    fn init_random_attrs_ranges() {
        let mut rng = Rng::new(7);
        for _ in 0..200 {
            let mut r = SquadPlayer::zeroed();
            player_init_random_attrs(&mut rng, &mut r);
            assert!((35..=65).contains(&r.tendency));
            assert!((1..=13).contains(&(r.traits & 0x0f)));
            assert!((1..=3).contains(&((r.traits >> 4) & 3)));
            assert_eq!(r.traits & 0x40, 0);
        }
    }

    #[test]
    fn real_saves_squads_sorted_and_packed() {
        for (path, s) in save_states() {
            let groups = (0..s.manager_count as usize).chain([TRANSFER_LIST]);
            for g in groups {
                let ids: Vec<u8> = s.squads[g].iter().map(|r| r.player_id).collect();
                let n = ids.iter().take_while(|&&id| id != 0).count();
                assert!(ids[n..].iter().all(|&id| id == 0), "{path:?} {g}");
                assert!(ids[..n].windows(2).all(|w| w[0] < w[1]), "{path:?} {g}");
            }
        }
    }

    #[test]
    fn real_saves_auto_lineup_invariants() {
        for (path, saved) in save_states() {
            for m in 0..saved.manager_count {
                for formation in 2..=4 {
                    for rules in [
                        FOUR_SUBS,
                        LineupRules {
                            suspended_allowed: true,
                            ..TWO_SUBS
                        },
                    ] {
                        let mut s = saved.clone();
                        s.current_manager = m as i8;
                        s.lineup_state[m as usize][0] = formation;
                        auto_lineup(&mut s, rules);
                        let ctx = format!("{path:?} m{m} f{formation} {rules:?}");
                        let squad = &s.squads[m as usize];
                        assert_eq!(squad[24], saved.squads[m as usize][24], "{ctx}");

                        let starters: Vec<&SquadPlayer> = squad[..SQUAD_SCAN]
                            .iter()
                            .filter(|r| is_starter(r))
                            .collect();
                        let numbers: Vec<u8> = starters.iter().map(|r| r.lineup_position).collect();
                        let k = numbers.len() as u8;
                        assert_eq!(numbers, (1..=k).collect::<Vec<_>>(), "{ctx}");

                        let available: Vec<&SquadPlayer> = saved.squads[m as usize][..SQUAD_SCAN]
                            .iter()
                            .filter(|r| {
                                r.player_id != 0 && is_available(r, rules.suspended_allowed)
                            })
                            .collect();
                        let keepers = available
                            .iter()
                            .filter(|r| s.players[r.player_id as usize].position_value < 25)
                            .count();
                        if keepers >= 1 && available.len() - keepers >= 10 {
                            assert_eq!(k, 11, "{ctx}");
                        }
                        for r in &starters {
                            assert!(is_available(r, rules.suspended_allowed), "{ctx}");
                            assert!((0..8).contains(&r.board_col), "{ctx}");
                            assert!((0..8).contains(&r.board_row), "{ctx}");
                        }
                        let mut cells: Vec<(i8, i8)> = starters
                            .iter()
                            .map(|r| (r.board_col, r.board_row))
                            .collect();
                        cells.sort();
                        cells.dedup();
                        assert_eq!(cells.len(), starters.len(), "{ctx}");
                        let bench = squad[..SQUAD_SCAN]
                            .iter()
                            .filter(|r| r.lineup_position > LAST_STARTER)
                            .count();
                        assert!(bench <= (rules.max_lineup_slot - 11) as usize, "{ctx}");
                    }
                }
            }
        }
    }
}
