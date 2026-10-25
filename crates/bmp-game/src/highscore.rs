//! The high-score entry of a human manager (`BMMAIN.EXE` segment 422b) and the insertion of
//! the managers' entries at the end of the game (segment 2a58). The `HIGH.XY` file and the
//! table screen belong to the screens.
//!
//! Full documentation with addresses: `docs/re/logic/stats.md` §5.3 and
//! `docs/re/logic/season.md` §3.1.

use bmp_formats::savegame::{HIGHSCORE_ENTRIES, HighScoreEntry, HighScoreTable};

use crate::finance::{CreditField, credit_sum};
use crate::rng::{Rng, ldiv, lmul};
use crate::state::GameState;
use crate::stats::stadium_attraction;
use crate::transfer::player_value_calc;

/// The score of manager `m`: the table position term, the cup stages and
/// the logged rounds, averaged over the rounds, times 10.
///
/// Origin: BMMAIN.EXE 422b:2864 (highscore_compute_score)
pub fn highscore_compute_score(state: &GameState, m: usize) -> i32 {
    let layout = crate::league_layout::layout();
    // `DS:226e`: first club id of each tier, as signed bytes; index 3 reads DS:2271.
    // MOD2026: DS:2271 is the layout's points per win.
    let first = layout.first_team;
    let tier_base = [first[0], first[1], first[2], layout.points_per_win].map(|b| b as i8);
    let man = &state.managers[m];
    let pos = state.team_tables[usize::from(man.team) % state.team_tables.len()].table_position;
    // MOD2026: the original's immediates 0x39 and 0x3a are its last league club id and its
    // number of league clubs, so the last club of the 3. Liga scores 0 in both layouts.
    let mut s = i32::from(layout.last_team(2))
        - i32::from(pos)
        - i32::from(tier_base[usize::from(man.league) & 3]);
    if s < 0 {
        s = 0;
    }
    s = ldiv(s, 2);
    for &c in &man.values_132[..4] {
        if c != 0 && c < 7 {
            s = s.wrapping_add(2 * i32::from(c));
        }
    }
    let n = (state.logged_rounds as u8).min(50);
    for e in &man.round_log[..usize::from(n)] {
        s = s.wrapping_add(i32::from(
            (i16::from(layout.league_club_count()) - i16::from(e[0])) / 2,
        ));
        let b = e[2];
        if b & 0x80 != 0 {
            s = s.wrapping_add(20);
            let b3 = e[3];
            s = s.wrapping_add(if b3 & 0x80 != 0 {
                25
            } else {
                3 * i32::from(b3 & 7)
            });
        } else {
            s = s.wrapping_add(2 * i32::from(b & 7));
        }
    }
    if n != 0 {
        s = ldiv(s, i32::from(n) + 1);
    }
    lmul(s, 10)
}

/// `strcpy` into a fixed field: the bytes after the NUL keep their old values.
fn strcpy_field(dst: &mut [u8], src: &[u8]) {
    let n = src.len().min(dst.len() - 1);
    dst[..n].copy_from_slice(&src[..n]);
    dst[n] = 0;
}

/// Writes the high-score entry of manager `m` into the candidate buffer `e` (`FAR:4cfe`):
/// name and club (by `strcpy`, so the rest of each field keeps what was there), the score
/// with the squad value, stadium and balance terms and the three title counters. `variant` 0
/// (the normal game) adds 300. The field at +0x36 is left alone.
///
/// Origin: BMMAIN.EXE 422b:2a2a (highscore_build_entry)
pub fn highscore_build_entry(
    state: &GameState,
    rng: &mut Rng,
    m: usize,
    variant: u8,
    e: &mut HighScoreEntry,
) {
    let man = &state.managers[m];
    strcpy_field(&mut e.manager, man.name());
    strcpy_field(&mut e.club, state.clubs[usize::from(man.team)].name());
    let mut score = highscore_compute_score(state, m) as i16;
    let mut v: i32 = 0;
    for i in 0..state.squad_player_count(m) {
        v = v.wrapping_add(player_value_calc(state, rng, i, 0));
    }
    v = v.wrapping_add(lmul(stadium_attraction(state), 135_000));
    let debts = credit_sum(man, CreditField::Principal, None);
    v = v.wrapping_add(man.balance.wrapping_sub(debts));
    score = score.wrapping_add(ldiv(v, 80_000) as i16);
    // M(m)[0x39..0x3d]: the title counters.
    let (a, b) = (man.titles, man.cup_wins[0]);
    let c = man.cup_wins[1]
        .wrapping_add(man.cup_wins[2])
        .wrapping_add(man.cup_wins[3]);
    e.club[23] = a;
    e.club[24] = b;
    e.club[25] = c;
    // 8-bit MULs for the first two, a 16-bit IMUL for the third.
    score = score.wrapping_add((u16::from(a) * 40) as i16);
    score = score.wrapping_add((u16::from(b) * 20) as i16);
    score = score.wrapping_sub((i16::from(c)).wrapping_mul(-60).wrapping_add(500));
    if variant == 0 {
        score = score.wrapping_add(300);
    }
    if score < 1 {
        score = 1;
    }
    e.score = score;
}

/// Puts the candidate buffer `cand` into `table` at the game end: over the row of the same
/// manager and club, or over the last row when it scores more, then sorts the table by score
/// (the exchange sort swaps through the candidate buffer). The scan goes on after the row
/// with the buffer as the candidate, which then holds the row the last swap moved out.
///
/// Origin: BMMAIN.EXE 2a58:3176 (season_main_loop, 2a58:425a..42ee)
pub fn highscore_game_end_insert(table: &mut HighScoreTable, cand: &mut HighScoreEntry) {
    let rows = &mut table.entries;
    for i in 0..HIGHSCORE_ENTRIES {
        let row = &rows[i];
        let same = row.manager_name() == cand.manager_name() && row.club_name() == cand.club_name();
        if !same && !(i == HIGHSCORE_ENTRIES - 1 && row.score < cand.score) {
            continue;
        }
        rows[i] = *cand;
        for a in 0..HIGHSCORE_ENTRIES - 1 {
            for b in a + 1..HIGHSCORE_ENTRIES {
                if rows[a].score < rows[b].score {
                    *cand = rows[a];
                    rows[a] = rows[b];
                    rows[b] = *cand;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmp_formats::savegame::HIGHSCORE_FILE_LEN;

    /// Rows "A".."T" of club "FC" with scores 200, 190, .., 10.
    fn table() -> HighScoreTable {
        let mut t = HighScoreTable::parse(&[0; HIGHSCORE_FILE_LEN]).unwrap();
        for (i, row) in t.entries.iter_mut().enumerate() {
            *row = entry(b'A' + i as u8, 200 - 10 * i as i16);
        }
        t
    }

    fn entry(name: u8, score: i16) -> HighScoreEntry {
        let mut e = HighScoreEntry {
            manager: [0; 26],
            club: [0; 26],
            score,
            extra: 0,
        };
        e.manager[0] = name;
        e.club[..2].copy_from_slice(b"FC");
        e
    }

    fn names(t: &HighScoreTable) -> Vec<u8> {
        t.entries.iter().map(|e| e.manager[0]).collect()
    }

    #[test]
    fn a_better_entry_replaces_the_last_row_and_is_sorted_in() {
        let mut t = table();
        highscore_game_end_insert(&mut t, &mut entry(b'X', 95));
        assert_eq!(names(&t), b"ABCDEFGHIJKXLMNOPQRS");
        let mut t = table();
        highscore_game_end_insert(&mut t, &mut entry(b'X', 10));
        assert_eq!(t, table(), "not better than the last row");
    }

    #[test]
    fn a_managers_row_is_replaced_and_the_swap_buffer_goes_on_scanning() {
        let mut t = table();
        highscore_game_end_insert(&mut t, &mut entry(b'C', 250));
        // C moves up through the buffer, which ends up holding B; B then beats the last row.
        assert_eq!(names(&t), b"CABBDEFGHIJKLMNOPQRS");
        assert_eq!(t.entries[0].score, 250);
        assert_eq!((t.entries[2].score, t.entries[3].score), (190, 190));
    }

    /// The score of a manager of the 3. Liga in table position `pos` with `logged` seasons, each
    /// in the last place of all league clubs.
    fn third_tier_score(pos: u8, logged: u16) -> i32 {
        let layout = crate::league_layout::layout();
        let mut state = GameState::program_start();
        let team = layout.first_team[2] + pos;
        state.managers[0].team = team;
        state.managers[0].league = 2;
        state.team_tables[usize::from(team)].table_position = pos;
        state.logged_rounds = logged;
        for log in &mut state.managers[0].round_log[..usize::from(logged)] {
            log[0] = layout.league_club_count();
        }
        highscore_compute_score(&state, 0)
    }

    #[test]
    fn the_last_league_club_scores_nothing_in_both_layouts() {
        use crate::league_layout::{MOD2026, ORIGINAL, with_layout};
        for layout in [&ORIGINAL, &MOD2026] {
            with_layout(layout, || {
                assert_eq!(third_tier_score(layout.team_count[2] - 1, 0), 0);
                // 19 / 2 for the first place, a last-place season adds nothing: (9 + 0) / 2.
                assert_eq!(third_tier_score(0, 1), 40);
            });
        }
    }

    #[test]
    fn strcpy_keeps_the_bytes_after_the_terminator() {
        // HIGH.12 of the original after "OLIVER" / "BAYER 05 UERDINGEN" were in the buffer.
        let mut name = [0u8; 26];
        strcpy_field(&mut name, b"OLIVER");
        strcpy_field(&mut name, b"ADI");
        assert_eq!(&name[..7], b"ADI\0ER\0");
        let mut club = [0u8; 26];
        strcpy_field(&mut club, b"BAYER 05 UERDINGEN");
        strcpy_field(&mut club, b"1.FC N\x9aRNBERG");
        assert_eq!(&club[13..19], b"\0NGEN\0");
        strcpy_field(&mut club, &[b'X'; 40]);
        assert_eq!(club[25], 0, "a long name is cut before the last byte");
    }
}
