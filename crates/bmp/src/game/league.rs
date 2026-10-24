//! The parts of the league-table code (segment 3a41) the office needs.
//! Spec: `docs/re/logic/stats.md` §2.1–2.3, `docs/re/logic/flow.md` §6.8.
//!
//! TODO(#26): move into the bmp-game league module once segment 3a41 is ported
//! there; only the snapshot mode (`mode == 1`) of the table update is here.

use bmp_game::state::{GameState, TeamTable};

use super::{TIER_COUNT, TIER_FIRST, TIER_ROUNDS};

/// Pairing permutations `DS:06fc`: index 1..17 for 18-club tiers, 19..37 (the
/// pointers from `DS:0744`) for 20-club tiers; letters `a..` are club slots.
const PAIRINGS: [&[u8]; 38] = [
    b"",
    b"abcdefghijklmnopqr",
    b"ronklijghefcdabqpm",
    b"afchejglinkpmrbdqo",
    b"rknglejchafbdqompi",
    b"ajclengpirkodfbhqm",
    b"rgnclajbhdfqmkoipe",
    b"fhancpergoimdjblqk",
    b"rcnbldjfhqkimgoepa",
    b"arcoemgkhjfldnbpqi",
    b"rbnflhjqigkemcoapd",
    b"amckeijlhnfpdrboqg",
    b"rfnjlqgeickambodph",
    b"aicglnjphrfodmbkqe",
    b"rjnqecgaibkdmfohpl",
    b"aecqlrjohmfkdibgpn",
    b"rncaebgdifkhmjolqp",
    b"aqnolmjkhifgdebcpr",
    b"",
    b"abcdefghijklmnopqrst",
    b"dahcnklibetqropmjgfs",
    b"caglinkpqfsbedhjmrot",
    b"dstmlhbqrkpingjcaefo",
    b"gpceobsairktmfqdjlhn",
    b"phaqdotirgnjesfklcbm",
    b"oahrcsgtifkbqelnjpmd",
    b"bithrjampleofgdksqnc",
    b"mecqlrgbkaosjtnpidhf",
    b"tlaibhqofjdgrnpcsmek",
    b"mqhdcoientjbksgaprlf",
    b"sitpegdjrcomahfnblqk",
    b"hecmnbgsiqkopfldrtja",
    b"mktcejbpoiqgaldnshfr",
    b"nagoimrbjsckpdhqtfle",
    b"slendrkiapfcqjbtmgoh",
    b"hmpetdnsgkralqicjofb",
    b"mjdfqnatcbkholspiger",
    b"lmbdrsnojkfahigctepq",
];

/// `matchday_pairings` value of an empty pairing slot (`'F'`).
pub const NO_MATCH: u8 = 0x46;

/// Builds the pairings of `round` (1-based) of `tier` into
/// [`GameState::matchday_pairings`]; the second half of the season repeats the
/// first with home and away swapped.
///
/// Origin: BMMAIN.EXE 3a41:10e3 (fixtures_build_round_pairings)
pub fn fixtures_build_round_pairings(state: &mut GameState, tier: usize, round: u8) {
    let offset = TIER_FIRST[tier];
    let half = TIER_ROUNDS[tier] >> 1;
    let mut round = round;
    let mut second_half = false;
    if half < round {
        round = round.wrapping_sub(half);
        second_half = true;
    }
    if half < round {
        round = half;
    }
    let count = TIER_COUNT[tier];
    let perm = if count == 18 {
        PAIRINGS[round as usize]
    } else {
        PAIRINGS[18 + round as usize]
    };
    for i in 0..(count >> 1) as usize {
        let (mut a, mut b) = (perm[2 * i] - b'a', perm[2 * i + 1] - b'a');
        if second_half {
            std::mem::swap(&mut a, &mut b);
        }
        state.matchday_pairings[tier][i] = [a + offset, b + offset];
    }
}

/// Copies column `src` of a table record into column `dst` (0 = current, 2 =
/// snapshot): points, goals, the home byte of wins and losses, and the form string.
///
/// Origin: BMMAIN.EXE 3a41:1ef4 (team_stats_copy_column)
pub fn team_stats_copy_column(t: &mut TeamTable, src: usize, dst: usize) {
    for arr in [&mut t.points, &mut t.goals_for, &mut t.goals_against] {
        arr[dst] = arr[src];
        arr[dst + 1] = arr[src + 1];
    }
    t.wins[dst] = t.wins[src];
    t.losses[dst] = t.losses[src];
    let from = t.form[src >> 1];
    let len = from
        .iter()
        .position(|&c| c == 0)
        .map_or(from.len(), |n| n + 1);
    t.form[dst >> 1][..len].copy_from_slice(&from[..len]);
}

/// `league_apply_round_and_sort(tier, 1, 1)`: snapshots the clubs that played in
/// the current round, sorts the full table and stores each manager's position for
/// the round.
///
/// Origin: BMMAIN.EXE 3a41:2d33 (league_apply_round_and_sort, mode 1)
pub fn league_snapshot_and_sort(state: &mut GameState, tier: usize) {
    let r = state.competition_rounds[tier].wrapping_sub(1) as i8;
    if r > TIER_ROUNDS[tier] as i8 - 1 {
        return;
    }
    let count = TIER_COUNT[tier] as usize;
    let mut i = 0;
    while i < count {
        let res = r
            .try_into()
            .ok()
            .and_then(|r: usize| state.league_results[tier].get(r))
            .map_or(0x1e, |row| row[i]);
        if res != 0x1e {
            let [h, a] = state.matchday_pairings[tier][i / 2];
            if h == NO_MATCH {
                break;
            }
            for t in [h, a] {
                if let Some(rec) = state.team_tables.get_mut(t as usize) {
                    team_stats_copy_column(rec, 0, 2);
                }
            }
        }
        i += 2;
    }
    // DS:224d == 0 in the shipped data: the games played are a sort key.
    state.sort_league_table(tier, count, 1, false);
    state.record_position_history(tier);
}

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
        if e[1] != week {
            continue;
        }
        if e[3] as usize != tier || tier > 2 {
            continue;
        }
        fixtures_build_round_pairings(state, tier, e[2]);
        let f = state.matchday_pairings[tier][e[4] as usize % 10];
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairings_cover_every_club_once_per_round() {
        let mut s = GameState::zeroed();
        for tier in 0..3 {
            for round in 1..=TIER_ROUNDS[tier] {
                fixtures_build_round_pairings(&mut s, tier, round);
                let n = TIER_COUNT[tier] as usize;
                let mut seen: Vec<u8> = s.matchday_pairings[tier][..n / 2]
                    .iter()
                    .flatten()
                    .copied()
                    .collect();
                seen.sort_unstable();
                let lo = TIER_FIRST[tier];
                assert_eq!(seen, (lo..lo + n as u8).collect::<Vec<_>>());
            }
        }
    }

    #[test]
    fn second_half_swaps_home_and_away() {
        let mut s = GameState::zeroed();
        fixtures_build_round_pairings(&mut s, 0, 2);
        let first = s.matchday_pairings[0];
        fixtures_build_round_pairings(&mut s, 0, 19);
        for (now, then) in s.matchday_pairings[0][..9].iter().zip(&first) {
            assert_eq!(*now, [then[1], then[0]]);
        }
        // Tier 1 offsets by 18: round 1 pairs slot a with b.
        fixtures_build_round_pairings(&mut s, 1, 1);
        assert_eq!(s.matchday_pairings[1][0], [18, 19]);
    }

    #[test]
    fn copy_column_snapshots_the_current_values() {
        let mut t = GameState::zeroed().team_tables[0].clone();
        t.points = [5, 6, 0, 0];
        t.wins = [2, 3, 0, 0];
        t.form[0] = *b"sUN@@@@@\0";
        team_stats_copy_column(&mut t, 0, 2);
        assert_eq!(t.points, [5, 6, 5, 6]);
        assert_eq!(t.wins, [2, 3, 2, 0]);
        assert_eq!(t.form[1], t.form[0]);
    }
}
