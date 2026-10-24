//! The part of the season loop (segment 2a58) that leads into the first office turn
//! of a new or a loaded game. The loop's logic is [`bmp_game::season`].
//! Spec: `docs/re/logic/season.md` §3.1.

use bmp_game::season::{office_turn_start, season_slot_start};
use bmp_game::state::GameState;

use super::office::{self, OfficeBitmaps};
use super::start_menu::Pictures;
use super::{Exit, Session};
use crate::host::{Host, Run};

/// `season_main_loop(resume)` up to the first office turn: of the stored current
/// manager after a game was loaded (`resume`), of manager 0 in a new game.
///
/// TODO(#31): the rest of the loop runs through [`bmp_game::season::season_main_loop`]
/// once the screens implement its `SeasonUi`; until then the turn ends the run.
///
/// Origin: BMMAIN.EXE 2a58:3176 (season_main_loop)
pub fn season_main_loop(
    host: &mut Host,
    session: &mut Session,
    pics: &Pictures,
    bitmaps: &OfficeBitmaps,
    state: &mut GameState,
    resume: bool,
) -> Run<Exit> {
    let saved_mgr = state.current_manager;
    let mut fmt = host.number_format();
    // TODO(#31): the finance dialogs of the day (`SeasonUi::finance_events`) are not shown.
    let wf = season_slot_start(
        state,
        &session.rt,
        &mut session.rng,
        &mut fmt,
        &mut |_, _, _| {},
    );
    host.number_pad = fmt.pad_width;
    host.number_no_separators = fmt.no_separators;
    if wf == 0 {
        // TODO(#31): DAY_ADVANCE before the first office turn (no shipped save
        // stops on such a week, and week 0 of a new game has a matchday).
        return Ok(Exit::EndOfTurn);
    }
    // OFFICE_B_TEST: manager 0 gets the first turn, after a load the stored manager.
    state.current_manager = if resume { saved_mgr } else { 0 };
    host.input_wait_release()?;
    office_turn_start(state, &mut session.rng);
    let week = state.season_week;
    office::office_menu_dispatcher(host, session, pics, bitmaps, state, week)?;
    Ok(Exit::EndOfTurn)
}
