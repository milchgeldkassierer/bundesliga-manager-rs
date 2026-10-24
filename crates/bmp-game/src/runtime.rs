//! Globals of `BMMAIN.EXE` that a `.MAN` save game does not store.
//!
//! [`crate::state::GameState`] holds exactly the 61 saved blocks. The game also keeps state in
//! globals outside those blocks: tables initialised at start-up, per-match scratch data and
//! statistics counters. They live in [`Runtime`], which starts from the values the original has
//! after program start (`docs/re/logic/state.md` §5, `docs/re/logic/ai.md` §5). Fields are added
//! when a ported function needs them.

use crate::state::MANAGER_SLOTS;

/// DS:226e: first club index of each league tier.
pub const TIER_FIRST: [u8; 3] = [0, 18, 38];
/// DS:225e: number of clubs per league tier.
pub const TIER_COUNT: [u8; 3] = [18, 20, 20];
/// DS:2262: number of rounds per league tier.
pub const TIER_ROUNDS: [u8; 3] = [34, 38, 38];

/// A match shown in the live view: the fixture row and the even pairing slot of the home team.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveMatch {
    /// `FAR:305c[k]`: league tier, or cup row.
    pub row: u8,
    /// `FAR:0028[k]`: even slot of the home team in the pairing row.
    pub slot: u8,
}

/// The non-persistent globals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Runtime {
    /// DS:226f, DS:2270 (`char`): first club index of the 2. Bundesliga and of the lower tier.
    pub tier_bounds: [i8; 2],
    /// DS:225e (`char`): number of clubs per league tier.
    pub tier_team_count: [i8; 3],
    /// DS:2262 (`char`): number of rounds per league tier.
    pub tier_round_count: [i8; 3],
    /// DS:2272: inclusive club index range `[lo, hi]` of each tier, plus foreign clubs.
    pub tier_club_ranges: [[u8; 2]; 4],
    /// FAR:90c6: lineup changes per manager since the last match day.
    pub lineup_change_count: [u8; MANAGER_SLOTS],
    /// FAR:2e99: option "show goals of other matches" (flash strip in the live view).
    pub option_show_other_goals: u8,
    /// FAR:6dbc/6dbd: home and away goals of the last unstored quick result.
    pub quick_sim_result: [u8; 2],
    /// FAR:21da + 2 * m: home and away goals of manager `m`'s last quick-simulated match.
    pub manager_last_score: [[u8; 2]; MANAGER_SLOTS],
    /// FAR:515c `[tier][slot][goal]`: goal minutes of the league fixtures, 0 = none.
    pub league_goal_minutes: [[[u8; 8]; 20]; 3],
    /// FAR:4d3c (= FAR:433c + 10 * 256) `[cup row][slot][goal]`: goal minutes of the cup
    /// fixtures, 0 = none.
    pub cup_goal_minutes: [[[u8; 8]; 32]; 4],
    /// FAR:4bf0 (count), FAR:305c, FAR:0028: the matches shown in the live view.
    pub live_matches: Vec<LiveMatch>,
}

impl Default for Runtime {
    fn default() -> Self {
        let last = |t: usize| TIER_FIRST[t] + TIER_COUNT[t] - 1;
        Runtime {
            tier_bounds: [TIER_FIRST[1] as i8, TIER_FIRST[2] as i8],
            tier_team_count: TIER_COUNT.map(|n| n as i8),
            tier_round_count: TIER_ROUNDS.map(|n| n as i8),
            tier_club_ranges: [
                [TIER_FIRST[0], last(0)],
                [TIER_FIRST[1], last(1)],
                [TIER_FIRST[2], last(2)],
                [last(2) + 1, 255],
            ],
            lineup_change_count: [0; MANAGER_SLOTS],
            option_show_other_goals: 0,
            quick_sim_result: [0; 2],
            manager_last_score: [[0; 2]; MANAGER_SLOTS],
            league_goal_minutes: [[[0; 8]; 20]; 3],
            cup_goal_minutes: [[[0; 8]; 32]; 4],
            live_matches: Vec::new(),
        }
    }
}

impl Runtime {
    /// League tier code of a club: 1 = Bundesliga, 2 = 2. Bundesliga, 4 = any other club
    /// (signed compares against [`Runtime::tier_bounds`]).
    ///
    /// Origin: BMMAIN.EXE 27c2:1bb5 (league_of_club)
    pub fn league_of_club(&self, club: u8) -> i16 {
        let club = i16::from(club);
        if club < i16::from(self.tier_bounds[0]) {
            1
        } else if club < i16::from(self.tier_bounds[1]) {
            2
        } else {
            4
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_tables_start_as_in_the_original() {
        let rt = Runtime::default();
        assert_eq!(rt.tier_bounds, [18, 38]);
        assert_eq!(rt.tier_team_count, [18, 20, 20]);
        assert_eq!(rt.tier_round_count, [34, 38, 38]);
        assert_eq!(
            rt.tier_club_ranges,
            [[0, 17], [18, 37], [38, 57], [58, 255]]
        );
    }

    #[test]
    fn league_of_club_uses_the_tier_bounds() {
        let rt = Runtime::default();
        assert_eq!(rt.league_of_club(0), 1);
        assert_eq!(rt.league_of_club(17), 1);
        assert_eq!(rt.league_of_club(18), 2);
        assert_eq!(rt.league_of_club(37), 2);
        assert_eq!(rt.league_of_club(38), 4);
        assert_eq!(rt.league_of_club(255), 4);
    }
}
