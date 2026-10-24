//! League helpers of the match-day code the office needs.
//! Spec: `docs/re/logic/flow.md` §6.8.

use bmp_game::league::fixtures_build_round_pairings;
use bmp_game::state::{
    GameState, RESCHEDULED_DAY, RESCHEDULED_PAIR, RESCHEDULED_ROUND, RESCHEDULED_TIER,
};

/// True when the current manager has a rescheduled match in `week` (always for
/// the pseudo manager 0x1e).
///
/// Origin: BMMAIN.EXE 1310:000a (has_rescheduled_match_today)
pub fn has_rescheduled_match_today(state: &mut GameState, week: u8) -> bool {
    let cur = state.current_manager;
    let Some(m) = usize::try_from(cur).ok().filter(|&m| m < 4) else {
        return cur == 0x1e;
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
