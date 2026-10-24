//! The part of the season loop (segment 2a58) that leads into the office after a
//! game was loaded. Spec: `docs/re/logic/season.md` §1.2 and §3.1.

use bmp_game::state::GameState;

use super::office::{self, OfficeBitmaps};
use super::start_menu::Pictures;
use super::{Exit, SEASON_START_DAY, Session};
use crate::host::{Host, Run};

/// Month lengths `DS:07b8` (no leap years).
const MONTH_DAYS: [i8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// Converts a day number into day of month, month and year: `n == 0` takes the
/// season day counter, otherwise `n` is a fixture index (two per week).
///
/// Origin: BMMAIN.EXE 1310:1190 (calendar_compute_date)
pub fn calendar_compute_date(state: &mut GameState, n: u16) {
    let mut day: i32;
    state.month = 0;
    if n == 0 {
        day = state.day_counter;
    } else {
        let n = if n == 0x78 { 0 } else { n };
        day = i32::from((n >> 1).wrapping_mul(7));
        if n & 1 != 0 {
            day += 4;
        } else {
            day += i32::from(SEASON_START_DAY);
        }
    }
    state.year = i32::from(state.start_year);
    day += 0xd2;
    while day > 0x16d {
        day -= 0x16d;
        state.year += 1;
    }
    loop {
        let len = i32::from(MONTH_DAYS[state.month as usize]);
        if len >= day {
            break;
        }
        day -= len;
        state.month += 1;
        if state.month > 11 {
            state.month = 0;
        }
    }
    state.day_of_month = day;
}

/// `season_main_loop(resume = 1)` up to the first office turn of the stored
/// current manager.
///
/// TODO(#31): the rest of the season loop (matches, day advance) is not ported;
/// the turn ends the run.
///
/// Origin: BMMAIN.EXE 2a58:3176 (season_main_loop)
pub fn season_main_loop_resume(
    host: &mut Host,
    session: &mut Session,
    pics: &Pictures,
    bitmaps: &OfficeBitmaps,
    state: &mut GameState,
) -> Run<Exit> {
    let saved_mgr = state.current_manager;
    let n = state.manager_count.min(4) as i8;
    calendar_compute_date(state, 0);
    state.loop_month = state.month as u8;
    let mut wf = state.week_competitions[state.season_week as usize % 95];
    if wf == 9 {
        wf = 0;
    }
    for m in 0..n {
        state.current_manager = m;
        // TODO(#23): finance_weekly_processing 212a:0a6d.
    }
    // TODO(#24): league_init_or_ai(1) 1f9d:0697.
    if wf == 0 {
        // TODO(#31): DAY_ADVANCE before the first office turn (no shipped save
        // stops on such a week).
        return Ok(Exit::EndOfTurn);
    }
    for m in 0..n {
        state.current_manager = m;
        let mu = m as usize;
        state.lineup_state[mu][0] = state.lineup_state[mu][1];
        // TODO(#28): squad_weekly_update 2ecd:3360; TODO(#24): manager_ai_weekly 1f9d:0002.
    }
    // OFFICE_B_TEST with resume: the stored manager gets the first turn.
    state.current_manager = saved_mgr;
    state.saved_manager = saved_mgr;
    host.input_wait_release()?;
    if session
        .rng
        .random_range(0, i16::from(state.manager_count) + 3)
        == 0
    {
        // TODO(#29): transfer_random_offer 3277:1e38.
    }
    let week = state.season_week;
    office::office_menu_dispatcher(host, session, pics, bitmaps, state, week)?;
    Ok(Exit::EndOfTurn)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(day: i32, start_year: u16) -> (i32, i32, i32) {
        let mut s = GameState::zeroed();
        s.day_counter = day;
        s.start_year = start_year;
        calendar_compute_date(&mut s, 0);
        (s.day_of_month, s.month, s.year)
    }

    #[test]
    fn calendar_matches_the_saves() {
        // 07072019.MAN: day 88 of the 1965 season is Wednesday, 25 October 1965.
        assert_eq!(date(88, 1965), (25, 9, 1965));
        // 2021-AUG.MAN: day 207 of 1967 is 21 February 1968.
        assert_eq!(date(207, 1967), (21, 1, 1968));
        // Day 0 is 29 July (0xd2 = 210 days into the year).
        assert_eq!(date(0, 1990), (29, 6, 1990));
    }

    #[test]
    fn fixture_index_dates() {
        let mut s = GameState::zeroed();
        s.start_year = 1965;
        calendar_compute_date(&mut s, 3);
        // Week 1 + 4 days: 7 + 4 = 11 days after day 0.
        assert_eq!((s.day_of_month, s.month), (9, 7));
    }
}
