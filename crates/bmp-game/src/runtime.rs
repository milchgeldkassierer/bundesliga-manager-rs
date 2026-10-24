//! Globals of `BMMAIN.EXE` that a `.MAN` save game does not store.
//!
//! [`crate::state::GameState`] holds exactly the 61 saved blocks. The game also keeps state in
//! globals outside those blocks: tables initialised at start-up, per-match scratch data and
//! statistics counters. They live in [`Runtime`], which starts from the values the original has
//! after program start (`docs/re/logic/state.md` §5, `docs/re/logic/ai.md` §5). Fields are added
//! when a ported function needs them.

use crate::state::{Layout, MANAGER_SLOTS, NewsRecord};

/// DS:226e: first club index of each league tier.
pub const TIER_FIRST: [u8; 3] = [0, 18, 38];
/// DS:225e: number of clubs per league tier.
pub const TIER_COUNT: [u8; 3] = [18, 20, 20];
/// DS:2262: number of rounds per league tier.
pub const TIER_ROUNDS: [u8; 3] = [34, 38, 38];
/// Booking slot value of [`Runtime::booked_players`] that holds nobody.
pub const NO_BOOKING: i8 = 0x7f;
/// DS:224c: season start-day offset (0 in the shipped data).
pub const SEASON_START_DAY: i8 = 0;

/// A match shown in the live view: the fixture row and the even pairing slot of the home team.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveMatch {
    /// `FAR:305c[k]`: league tier, or cup row.
    pub row: u8,
    /// `FAR:0028[k]`: even slot of the home team in the pairing row.
    pub slot: u8,
}

/// A league fixture lost by forfeit because a human manager could not field a team.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Forfeit {
    /// `FAR:549c[k]`: 1 = the home club's manager forfeited, 2 = the away club's.
    pub side: u8,
    /// `FAR:4bd0[k]`: league tier.
    pub tier: u8,
    /// `FAR:57a6[k]`: 0-based round (`competition_rounds[tier] - 1`).
    pub round_index: u8,
    /// `FAR:0170[k]`: even pairing slot.
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
    /// FAR:2e99: a human manager's match is shown live. Cleared by `matchday_play`
    /// (1310:15db), set by its gate-money hook (2a58:20b2) and by the cup extra-time check
    /// (27c2:1226); card and injury events (1310:2ee5) only happen while it is set.
    pub live_match_shown: u8,
    /// FAR:6dbc/6dbd: home and away goals of the last unstored quick result.
    pub quick_sim_result: [u8; 2],
    /// FAR:21da + 2 * m: home and away goals of manager `m`'s last quick-simulated match.
    pub manager_last_score: [[u8; 2]; MANAGER_SLOTS],
    /// FAR:515c `[tier][slot][goal]`: goal minutes of the league fixtures, 0 = none.
    pub league_goal_minutes: [[[u8; 8]; 20]; 3],
    /// FAR:4d3c (= FAR:433c + 10 * 256) `[cup row][slot][goal]`: goal minutes of the cup
    /// fixtures, 0 = none. After a match, byte 0 of `[m][i]` also holds the newspaper grade
    /// 1 (best)..6 of squad slot `i` of human manager `m` (`docs/re/logic/news.md` §3).
    pub cup_goal_minutes: [[[u8; 8]; 32]; 4],
    /// FAR:4bf0 (count), FAR:305c, FAR:0028: the matches shown in the live view.
    pub live_matches: Vec<LiveMatch>,
    /// FAR:90ca: newspaper match record of each human manager.
    pub news_records: [NewsRecord; MANAGER_SLOTS],
    /// FAR:2e98 (count), FAR:549c, FAR:4bd0, FAR:57a6, FAR:0170: the forfeits of the match day,
    /// built by `matchday_play` (1310:15db) and written by `match_write_forfeit_results`
    /// (1310:152e).
    pub forfeits: Vec<Forfeit>,
    /// FAR:0178 + 2 * m: yellow cards manager `m`'s team can still get in this match. Reset by
    /// `matchday_play`, set by 2a58:20b2, used by 1310:2ee5.
    pub yellow_cards_left: [u8; MANAGER_SLOTS],
    /// FAR:0179 + 2 * m: a red card is still possible for manager `m`'s team. Reset by
    /// `matchday_play`, set by 2a58:20b2, used and cleared by 1310:2ee5.
    pub red_card_possible: [u8; MANAGER_SLOTS],
    /// FAR:1d1c + 6 * m: squad indices of manager `m`'s booked players, [`NO_BOOKING`] =
    /// free. Filled with [`NO_BOOKING`] by 2a58:20b2, used by 1310:2ee5.
    pub booked_players: [[i8; 6]; MANAGER_SLOTS],
    /// FAR:1d14 + m: manager `m`'s team plays now. Set by 2a58:20b2, cleared by 27c2:1226
    /// (which also writes FAR:1d13 + r, i.e. index `r - 1`), read by 1310:2ee5.
    pub manager_plays_now: [u8; MANAGER_SLOTS],
    /// FAR:5710 + m / FAR:5778 + m: home / away chances of manager `m`'s match already played
    /// in this period. Reset by 1310:15db and 1310:2303, counted by 2a58:0ca3, read by
    /// 1310:347f.
    pub period_chances_used: [[u8; 2]; MANAGER_SLOTS],
    /// FAR:57dc: a relegation play-off leg is being played on cup row 1. Set and cleared by
    /// `matchday_relegation_match` (1310:02bf), read by `cup_round_complete` (27c2:16dc).
    pub relegation_mode: u8,
    /// FAR:2e74: used length of [`crate::state::GameState::euro_qualified_list`], set by
    /// `euro_cup_qualifiers_build` (27c2:0ef2).
    // TODO(re): not saved, so after loading a game no club counts as qualified until the next
    // season end rebuilds the list (cups.md §11.2).
    pub euro_qualified_count: u8,
    /// FAR:56f4: the game started in 1992 rather than 1963; set by the new-game options
    /// dialog, not saved.
    pub modern_start: bool,
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
            live_match_shown: 0,
            quick_sim_result: [0; 2],
            manager_last_score: [[0; 2]; MANAGER_SLOTS],
            league_goal_minutes: [[[0; 8]; 20]; 3],
            cup_goal_minutes: [[[0; 8]; 32]; 4],
            live_matches: Vec::new(),
            news_records: Layout::zeroed(),
            forfeits: Vec::new(),
            yellow_cards_left: [0; MANAGER_SLOTS],
            red_card_possible: [0; MANAGER_SLOTS],
            booked_players: [[NO_BOOKING; 6]; MANAGER_SLOTS],
            manager_plays_now: [0; MANAGER_SLOTS],
            period_chances_used: [[0; 2]; MANAGER_SLOTS],
            relegation_mode: 0,
            euro_qualified_count: 0,
            modern_start: false,
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
