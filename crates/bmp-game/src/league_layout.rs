//! The league structure: which club ids form each league tier, how many rounds each tier plays,
//! how many clubs go down and the season calendar that schedules the rounds.
//!
//! The original keeps these values as DGROUP tables (DS:225e..DS:2278, DS:2280) that its whole
//! program reads. The port reads them from the one [`LeagueLayout`] selected for the program run
//! ([`set_layout`], called once at start-up before any game state exists): code that has a
//! [`crate::runtime::Runtime`] reads its `tier_*` fields, the run-time mirror of the tables that
//! [`Runtime::default`](crate::runtime::Runtime) fills from [`layout`]; every other reader calls
//! [`layout`]. A `Runtime` or a
//! [`GameState::program_start`](crate::state::GameState::program_start) built before
//! [`set_layout`] carries the layout selected at that time.
//!
//! The selection is per thread, so tests that run a layout through `with_layout` stay isolated
//! from each other; every thread starts with [`ORIGINAL`].

use std::cell::Cell;
use std::ops::Range;

/// The values of the league structure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeagueLayout {
    /// DS:226e: first club id of each league tier.
    pub first_team: [u8; 3],
    /// DS:225e: number of clubs per league tier.
    pub team_count: [u8; 3],
    /// DS:2262: number of rounds per league tier.
    pub round_count: [u8; 3],
    /// Clubs per league tier that go down without a play-off (DS:2266 without the play-off
    /// place).
    pub relegated_directly: [u8; 3],
    /// DS:226a: the league tier's last relegation place plays a play-off.
    pub relegation_playoff: [bool; 3],
    /// DS:2280 (save block 50) in the data segment image (`season.md` §1.4): the competitions
    /// of each half-week slot; bit `t` is a round of league tier `t`.
    pub week_competitions: [u8; 95],
}

impl LeagueLayout {
    /// The club ids of league tier `tier` (the original also keeps them as a first/last table
    /// at DS:2272).
    pub fn team_ids(&self, tier: usize) -> Range<u8> {
        let first = self.first_team[tier];
        first..first + self.team_count[tier]
    }

    /// DS:2272 + 2 * `tier` + 1: the last club id of league tier `tier`.
    pub fn last_team(&self, tier: usize) -> u8 {
        self.first_team[tier] + self.team_count[tier] - 1
    }

    /// DS:2266: the relegation places of league tier `tier`, the play-off place included.
    pub fn relegation_places(&self, tier: usize) -> u8 {
        self.relegated_directly[tier] + u8::from(self.relegation_playoff[tier])
    }

    /// DS:2265 + `tier`: the promotion places of league tier `tier`, which are the relegation
    /// places of the tier above (0 for the top tier).
    pub fn promotion_places(&self, tier: usize) -> u8 {
        match tier {
            0 => 0,
            _ => self.relegation_places(tier - 1),
        }
    }

    /// DS:2278: the number of clubs that play in a league tier.
    pub fn league_club_count(&self) -> u8 {
        self.team_count.iter().sum()
    }
}

/// The league structure of `BMMAIN.EXE`.
pub const ORIGINAL: LeagueLayout = LeagueLayout {
    first_team: [0, 18, 38],
    team_count: [18, 20, 20],
    round_count: [34, 38, 38],
    relegated_directly: [2, 4, 4],
    relegation_playoff: [true, false, false],
    week_competitions: [
        0x07, 0x70, 0x07, 0x00, 0x07, 0x70, 0x07, 0x06, 0x08, 0x00, 0x07, 0x00, 0x07, 0x06, 0x07,
        0x70, 0x07, 0x07, 0x07, 0x70, 0x07, 0x00, 0x08, 0x01, 0x07, 0x06, 0x07, 0x00, 0x07, 0x06,
        0x08, 0x70, 0x07, 0x00, 0x07, 0x70, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07,
        0x07, 0x00, 0x07, 0x70, 0x08, 0x07, 0x07, 0x70, 0x07, 0x06, 0x07, 0x00, 0x07, 0x00, 0x07,
        0x00, 0x07, 0x70, 0x07, 0x00, 0x07, 0x70, 0x07, 0x00, 0x07, 0x00, 0x08, 0x00, 0x07, 0x00,
        0x07, 0x10, 0x10, 0x00, 0x00,
    ],
};

/// MOD2026: the club slots table of `docs/mod2026/PRD.md` R1: 18 / 18 / 20 clubs with the ids
/// 0..=17, 18..=35 and 36..=55, the pool 56..=63, 34 / 34 / 38 rounds, 2 / 2 / 4 clubs relegated
/// directly and a play-off place in the Bundesliga and the 2. Bundesliga.
///
/// The play-off of the 2. Bundesliga is not played yet (only the Bundesliga play-off is ported),
/// so its play-off place goes down directly, and three clubs of the 3. Liga go up.
pub const MOD2026: LeagueLayout = LeagueLayout {
    first_team: [0, 18, 36],
    team_count: [18, 18, 20],
    round_count: [34, 34, 38],
    relegated_directly: [2, 2, 4],
    relegation_playoff: [true, true, false],
    week_competitions: mod_calendar(ORIGINAL.week_competitions),
};

/// MOD2026: the original calendar with the 2. Bundesliga playing on the Bundesliga's days, so
/// that it plays 34 rounds: the week with only a Bundesliga round (0x01) gets a 2. Bundesliga
/// round, and the weeks with only rounds of the two lower tiers (0x06) keep only the round of
/// the lowest tier.
const fn mod_calendar(mut weeks: [u8; 95]) -> [u8; 95] {
    let mut i = 0;
    while i < weeks.len() {
        weeks[i] = match weeks[i] {
            0x01 => 0x03,
            0x06 => 0x04,
            w => w,
        };
        i += 1;
    }
    weeks
}

thread_local! {
    static ACTIVE: Cell<&'static LeagueLayout> = const { Cell::new(&ORIGINAL) };
}

/// The league structure selected for this thread.
pub fn layout() -> &'static LeagueLayout {
    ACTIVE.get()
}

/// Selects the league structure of this thread.
pub fn set_layout(layout: &'static LeagueLayout) {
    ACTIVE.set(layout);
}

/// Runs `f` with `layout` selected and selects the previous layout again afterwards.
#[cfg(test)]
pub fn with_layout<R>(layout: &'static LeagueLayout, f: impl FnOnce() -> R) -> R {
    struct Restore(&'static LeagueLayout);
    impl Drop for Restore {
        fn drop(&mut self) {
            set_layout(self.0);
        }
    }
    let _restore = Restore(ACTIVE.replace(layout));
    f()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Runtime;
    use crate::state::{GERMAN_CLUBS, GameState};

    #[test]
    fn original_layout_mirrors_the_ds_tables() {
        let l = &ORIGINAL;
        assert_eq!(l.team_count, [18, 20, 20]);
        assert_eq!(l.round_count, [34, 38, 38]);
        assert_eq!(l.team_ids(0), 0..18);
        assert_eq!(l.team_ids(1), 18..38);
        assert_eq!(l.team_ids(2), 38..58);
        assert_eq!([0, 1, 2].map(|t| l.last_team(t)), [17, 37, 57]);
        assert_eq!([0, 1, 2].map(|t| l.relegation_places(t)), [3, 4, 4]);
        assert_eq!([0, 1, 2].map(|t| l.promotion_places(t)), [0, 3, 4]);
        assert_eq!(l.league_club_count(), 58);
        assert_eq!(l.league_club_count()..GERMAN_CLUBS as u8, 58..64);
    }

    #[test]
    fn mod_layout_matches_the_prd() {
        let l = &MOD2026;
        assert_eq!(l.team_count, [18, 18, 20]);
        assert_eq!(l.round_count, [34, 34, 38]);
        assert_eq!(l.relegated_directly, [2, 2, 4]);
        assert_eq!(l.relegation_playoff, [true, true, false]);
        assert_eq!(l.team_ids(0), 0..18);
        assert_eq!(l.team_ids(1), 18..36);
        assert_eq!(l.team_ids(2), 36..56);
        assert_eq!([0, 1, 2].map(|t| l.last_team(t)), [17, 35, 55]);
        assert_eq!([0, 1, 2].map(|t| l.relegation_places(t)), [3, 3, 4]);
        assert_eq!([0, 1, 2].map(|t| l.promotion_places(t)), [0, 3, 3]);
        assert_eq!(l.league_club_count()..GERMAN_CLUBS as u8, 56..64);
    }

    #[test]
    fn calendars_give_each_tier_its_rounds() {
        for l in [&ORIGINAL, &MOD2026] {
            for t in 0..3 {
                let rounds = l
                    .week_competitions
                    .iter()
                    .filter(|&&w| w & 0x08 == 0 && w & (1 << t) != 0)
                    .count();
                assert_eq!(rounds, usize::from(l.round_count[t]), "tier {t}");
            }
        }
        let changed: Vec<usize> = (0..95)
            .filter(|&i| ORIGINAL.week_competitions[i] != MOD2026.week_competitions[i])
            .collect();
        assert_eq!(changed, [7, 13, 23, 25, 29, 69]);
    }

    #[test]
    fn runtime_and_program_start_follow_the_layout() {
        with_layout(&MOD2026, || {
            let rt = Runtime::default();
            assert_eq!(rt.tier_bounds, [18, 36]);
            assert_eq!(rt.tier_team_count, [18, 18, 20]);
            assert_eq!(rt.tier_round_count, [34, 34, 38]);
            assert_eq!(
                rt.tier_club_ranges,
                [[0, 17], [18, 35], [36, 55], [56, 255]]
            );
            let s = GameState::program_start();
            assert_eq!(s.week_competitions, MOD2026.week_competitions);
        });
        assert_eq!(layout(), &ORIGINAL);
        let rt = Runtime::default();
        assert_eq!(
            rt.tier_club_ranges,
            [[0, 17], [18, 37], [38, 57], [58, 255]]
        );
    }
}
