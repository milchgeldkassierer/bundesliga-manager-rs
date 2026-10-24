//! Player statistics of `BMMAIN.EXE` segment 24a4: the goal scorers of the
//! clubs no human manages and the top-scorer list.
//!
//! Full documentation with addresses: `docs/re/logic/stats.md` §3.2 and §3.8.

use crate::league::{
    LEAGUE_ROUND_COUNT, LEAGUE_TEAM_COUNT, NO_MATCH, NOT_PLAYED, league_team_ids, round_results,
};
use crate::rng::Rng;
use crate::state::{GameState, PLAYER_COUNT, Player, SquadPlayer};

/// DS:5331 scorer selection weights, indexed by the candidate count.
const SCORER_WEIGHTS: [u8; 11] = [0, 2, 3, 4, 5, 6, 7, 8, 9, 10, 46];
/// Rows of the top-scorer list.
const TOP_SCORER_ROWS: usize = 20;

/// League goals this season (+0x22).
fn goals(p: &Player) -> u8 {
    p.counters[0]
}

/// League games this season (+0x23).
fn games(p: &Player) -> u8 {
    p.counters[1]
}

/// Distributes the goals of the last round of `league` over the pool players
/// of the clubs that no human manages, home club before away club, pair by
/// pair. A round counter beyond the season reads round 1.
///
/// Origin: BMMAIN.EXE 24a4:1662 (matchday_assign_ai_scorers)
pub fn matchday_assign_ai_scorers(state: &mut GameState, rng: &mut Rng, league: usize) {
    let mut r = state.competition_rounds[league];
    if (LEAGUE_ROUND_COUNT[league] as i8 as i16) < r as i16 {
        r = 1;
    }
    // TODO(re): for league 0 and round counter 0 the original reads the 20
    // bytes before FAR:6ddc as results; no goals are assigned here.
    let Some(results) = round_results(state, league, r.wrapping_sub(1) as i8) else {
        return;
    };
    for i in (0..LEAGUE_TEAM_COUNT[league] as usize).step_by(2) {
        let (gh, ga) = (results[i], results[i + 1]);
        // The end marker is tested on the result byte here, not the team id.
        if gh == NO_MATCH {
            break;
        }
        if gh == NOT_PLAYED {
            continue;
        }
        let [h, a] = state.matchday_pairings[league][i / 2];
        match_assign_ai_scorers(state, rng, h, gh);
        match_assign_ai_scorers(state, rng, a, ga);
    }
}

/// Distributes `goals` of `team` over its pool players, unless a human
/// manages the team. Each player of the team plays with chance 94/101 (one
/// league game more) and becomes a candidate when a talent-weighted draw
/// succeeds. With fewer than two candidates no goal is credited; otherwise
/// each goal goes to a random candidate or, with some chance, to nobody.
///
/// Origin: BMMAIN.EXE 24a4:14d4 (match_assign_ai_scorers)
pub fn match_assign_ai_scorers(state: &mut GameState, rng: &mut Rng, team: u8, goals: u8) {
    if goals == 0 || state.manager_index_of_team(team, 0xff) != 0 {
        return;
    }
    let mut cand = [0u8; PLAYER_COUNT];
    let mut n: u8 = 0;
    for p in 1..PLAYER_COUNT {
        let player = &mut state.players[p];
        if player.team != team {
            continue;
        }
        if rng.random_range(0, 100) < 94 {
            player.counters[1] = player.counters[1].wrapping_add(1);
            let x = rng.random_range(10, 99);
            let y = rng.random_range(5, 25) + player.position_value as i16;
            if y > x {
                cand[n as usize] = p as u8;
                n += 1;
            }
        }
    }
    if n < 2 {
        return;
    }
    for _ in 0..goals {
        let mut k = if n < 10 {
            SCORER_WEIGHTS[n as usize] - 1
        } else {
            10
        };
        if k > 2 {
            k -= 1;
        }
        if rng.random_range(0, 4) == 0 {
            continue;
        }
        loop {
            if rng.random_range(0, k as i16) != 0 {
                let idx = rng.random_range(0, n as i16 - 1) as u8;
                let scorer = &mut state.players[cand[idx as usize] as usize];
                scorer.counters[0] = scorer.counters[0].wrapping_add(1);
                break;
            }
            if rng.random_range(0, 1) + 2 >= n as i16 {
                break;
            }
        }
    }
}

/// Copies the goal counters of every human squad into the player pool and
/// returns the pool players 1..=150 ordered by goals (most first), ties by
/// games (fewer first). Element 0 is unused.
///
/// Origin: BMMAIN.EXE 24a4:1ad5 (screen_top_scorers, data part before the
/// display)
pub fn top_scorers_order(state: &mut GameState) -> [u8; PLAYER_COUNT] {
    for m in 0..state.active_managers().len() {
        let count = state.squad_player_count(m) as usize;
        for i in 0..count {
            let slot = &state.squads[m][i];
            let (id, goals) = (
                slot.player_id as usize,
                slot.unknown_00[SquadPlayer::LEAGUE_GOALS],
            );
            state.players[id].counters[0] = goals;
        }
    }
    let mut ord: [u8; PLAYER_COUNT] = std::array::from_fn(|p| p as u8);
    let players = &state.players;
    for i in 1..PLAYER_COUNT {
        for j in i + 1..PLAYER_COUNT {
            let (x, y) = (&players[ord[i] as usize], &players[ord[j] as usize]);
            if goals(x) < goals(y) || (goals(x) == goals(y) && games(y) < games(x)) {
                ord.swap(i, j);
            }
        }
    }
    ord
}

/// One row of the top-scorer list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopScorerRow {
    /// 1-based rank among all scorers of the tier, also in the human-teams
    /// list.
    pub rank: u8,
    /// Index into [`GameState::players`].
    pub player: u8,
}

/// The rows of the top-scorer list of league `tier`, built from
/// [`top_scorers_order`]: the players of the tier's clubs with at least two
/// goals, at most 20 rows. With `human_teams_only` ("SPIELER" view) only the
/// players of the human managers' clubs are listed, keeping their rank in the
/// whole tier.
///
/// Origin: BMMAIN.EXE 24a4:1ad5 (screen_top_scorers, list loop)
pub fn top_scorers_rows(
    state: &GameState,
    order: &[u8; PLAYER_COUNT],
    tier: usize,
    human_teams_only: bool,
) -> Vec<TopScorerRow> {
    let teams = league_team_ids(tier);
    let managers = state.active_managers();
    let mut rows = Vec::new();
    let mut rank: u8 = 0;
    for &p in &order[1..] {
        let player = &state.players[p as usize];
        if !teams.contains(&player.team) {
            continue;
        }
        if goals(player) < 2 {
            break;
        }
        rank = rank.wrapping_add(1);
        let human = managers.iter().any(|m| m.team == player.team);
        if rows.len() < TOP_SCORER_ROWS && (!human_teams_only || human) {
            rows.push(TopScorerRow { rank, player: p });
        }
    }
    rows
}

/// Goals per game in tenths (`goals * 10 / games`, unsigned, `goals * 10`
/// without games), shown as `"<q / 10>.<q % 10>"`.
///
/// Origin: BMMAIN.EXE 24a4:1ad5 (screen_top_scorers, TORE/SPIEL column)
pub fn goals_per_game_tenths(player: &Player) -> u16 {
    let q = goals(player) as u16 * 10;
    match games(player) {
        0 => q,
        g => q / g as u16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::tests::save_files;
    use bmp_formats::savegame::SaveGame;

    /// Players 1..=150 belong to no club; `team` gets the given players.
    fn pool(team: u8, members: &[usize], talent: u8) -> GameState {
        let mut s = GameState::zeroed();
        for p in &mut s.players {
            p.team = 0xff;
        }
        for &p in members {
            s.players[p].team = team;
            s.players[p].position_value = talent;
        }
        s
    }

    #[test]
    fn no_randomness_without_goals_or_for_human_clubs() {
        let mut s = pool(5, &[1, 2, 3], 80);
        let mut rng = Rng::new(3);
        match_assign_ai_scorers(&mut s, &mut rng, 5, 0);
        s.manager_count = 1;
        s.managers[0].team = 5;
        match_assign_ai_scorers(&mut s, &mut rng, 5, 4);
        assert_eq!(rng, Rng::new(3));
        assert!(s.players.iter().all(|p| p.counters == [0, 0]));
    }

    #[test]
    fn scorer_call_order_for_two_certain_candidates() {
        // Talent 255: y >= 5 + 255 > 99 >= x, so every playing player is a
        // candidate.
        let mut s = pool(7, &[10, 20], 255);
        let mut rng = Rng::new(77);
        let mut expect = rng;
        match_assign_ai_scorers(&mut s, &mut rng, 7, 1);

        let mut played = Vec::new();
        for p in [10u8, 20] {
            if expect.random_range(0, 100) < 94 {
                expect.random_range(10, 99);
                expect.random_range(5, 25);
                played.push(p);
            }
        }
        let mut scorer = None;
        // Two candidates: k = DS:5331[2] - 1 = 2; the retry test
        // `random_range(0, 1) + 2 < 2` never holds.
        if played.len() == 2 && expect.random_range(0, 4) != 0 {
            if expect.random_range(0, 2) != 0 {
                scorer = Some(played[expect.random_range(0, 1) as usize]);
            } else {
                expect.random_range(0, 1);
            }
        }
        assert_eq!(rng, expect);
        for p in [10u8, 20] {
            let c = s.players[p as usize].counters;
            assert_eq!(c[1], played.contains(&p) as u8, "games of {p}");
            assert_eq!(c[0], (scorer == Some(p)) as u8, "goals of {p}");
        }
    }

    /// Replays `match_assign_ai_scorers` for `members` of talent 255 (every
    /// playing member is a candidate) and returns the candidate count and the
    /// goals per member.
    fn replay_scorers(rng: &mut Rng, members: &[usize], goals: u8) -> (usize, Vec<u8>) {
        // k per candidate count, hand-derived: DS:5331[n] - 1 below 10
        // candidates, else 10; one less when above 2.
        const K: [i16; 11] = [0, 0, 2, 2, 3, 4, 5, 6, 7, 8, 9];
        let cand: Vec<usize> = (0..members.len())
            .filter(|_| {
                let plays = rng.random_range(0, 100) < 94;
                if plays {
                    rng.random_range(10, 99);
                    rng.random_range(5, 25);
                }
                plays
            })
            .collect();
        let n = cand.len();
        let mut scored = vec![0; members.len()];
        if n < 2 {
            return (n, scored);
        }
        let k = K[n.min(10)];
        for _ in 0..goals {
            if rng.random_range(0, 4) == 0 {
                continue;
            }
            loop {
                if rng.random_range(0, k) != 0 {
                    scored[cand[rng.random_range(0, n as i16 - 1) as usize]] += 1;
                    break;
                }
                if rng.random_range(0, 1) + 2 >= n as i16 {
                    break;
                }
            }
        }
        (n, scored)
    }

    #[test]
    fn scorer_weights_follow_the_candidate_count() {
        let mut counts = std::collections::BTreeSet::new();
        for size in [2, 3, 4, 5, 9, 10, 12, 14] {
            let members: Vec<usize> = (30..30 + size).collect();
            for seed in 0..64 {
                let mut s = pool(7, &members, 255);
                let mut rng = Rng::new(seed);
                let mut expect = rng;
                match_assign_ai_scorers(&mut s, &mut rng, 7, 6);
                let (n, scored) = replay_scorers(&mut expect, &members, 6);
                assert_eq!(rng, expect, "{size} members, seed {seed}");
                let goals: Vec<u8> = members.iter().map(|&p| s.players[p].counters[0]).collect();
                assert_eq!(goals, scored, "{size} members, seed {seed}");
                counts.insert(n);
            }
        }
        for n in [2, 3, 4, 5, 9, 10, 12, 14] {
            assert!(counts.contains(&n), "no seed with {n} candidates");
        }
    }

    #[test]
    fn a_single_candidate_never_scores() {
        let mut s = pool(7, &[10], 255);
        let mut rng = Rng::new(5);
        match_assign_ai_scorers(&mut s, &mut rng, 7, 9);
        assert_eq!(s.players[10].counters[0], 0);
    }

    #[test]
    fn scorers_never_exceed_the_goals() {
        let members: Vec<usize> = (40..56).collect();
        let mut s = pool(3, &members, 60);
        let mut rng = Rng::new(2024);
        for _ in 0..50 {
            match_assign_ai_scorers(&mut s, &mut rng, 3, 3);
        }
        let goals: u32 = s.players.iter().map(|p| p.counters[0] as u32).sum();
        assert!(goals > 0 && goals <= 150, "{goals}");
        assert!(s.players[40..56].iter().all(|p| p.counters[1] <= 50));
    }

    #[test]
    fn matchday_walks_the_last_round() {
        let mut s = pool(2, &[1, 2, 3], 255);
        for p in [4, 5, 6] {
            s.players[p].team = 3;
            s.players[p].position_value = 255;
        }
        s.competition_rounds[0] = 2;
        s.matchday_pairings[0][0] = [2, 3];
        s.league_results[0][1] = [NOT_PLAYED; 20];
        s.league_results[0][1][..2].copy_from_slice(&[2, 0]);
        let mut rng = Rng::new(8);
        let mut expect = rng;
        let mut t = s.clone();
        matchday_assign_ai_scorers(&mut s, &mut rng, 0);
        match_assign_ai_scorers(&mut t, &mut expect, 2, 2);
        match_assign_ai_scorers(&mut t, &mut expect, 3, 0);
        assert_eq!((rng, &s.players), (expect, &t.players));
        // A counter past the season reads round 1 (index 0): unplayed here.
        s.competition_rounds[0] = 35;
        s.league_results[0][0] = [NOT_PLAYED; 20];
        matchday_assign_ai_scorers(&mut s, &mut rng, 0);
        assert_eq!(rng, expect);
    }

    #[test]
    fn top_scorers_sort_by_goals_then_fewer_games() {
        let mut s = GameState::zeroed();
        for (p, goals, games) in [(3, 5, 4), (7, 5, 3), (2, 1, 1), (9, 0, 0)] {
            s.players[p].counters = [goals, games];
        }
        // A human squad's goal counter overrides the pool counter.
        s.manager_count = 1;
        s.squads[0][0].player_id = 9;
        s.squads[0][0].unknown_00[SquadPlayer::LEAGUE_GOALS] = 6;
        let ord = top_scorers_order(&mut s);
        assert_eq!(ord[1..5], [9, 7, 3, 2]);
        assert_eq!(s.players[9].counters[0], 6);
    }

    #[test]
    fn top_scorer_rows_filter_by_tier_and_stop_below_two_goals() {
        let mut s = GameState::zeroed();
        let rows = [
            (1, 18, 9, 4),
            (2, 8, 8, 1),
            (3, 5, 7, 2),
            (4, 12, 2, 3),
            (5, 3, 1, 5),
        ];
        for (p, team, goals, games) in rows {
            let player = &mut s.players[p];
            (player.team, player.counters) = (team, [goals, games]);
        }
        s.players[6].team = 0xff;
        s.manager_count = 1;
        s.managers[0].team = 5;
        let ord = top_scorers_order(&mut s);
        let list = top_scorers_rows(&s, &ord, 0, false);
        // Player 1 (team 18) is in tier 1; player 5 has a single goal.
        let ranked: Vec<(u8, u8)> = list.iter().map(|r| (r.rank, r.player)).collect();
        assert_eq!(ranked, [(1, 2), (2, 3), (3, 4)]);
        let human = top_scorers_rows(&s, &ord, 0, true);
        assert_eq!(human, [TopScorerRow { rank: 2, player: 3 }]);
        assert_eq!(top_scorers_rows(&s, &ord, 1, false)[0].player, 1);
    }

    #[test]
    fn top_scorer_list_has_at_most_twenty_rows() {
        let mut s = GameState::zeroed();
        for p in 1..40 {
            s.players[p].team = 40;
            s.players[p].counters = [5, 5];
        }
        let ord = top_scorers_order(&mut s);
        let rows = top_scorers_rows(&s, &ord, 2, false);
        assert_eq!(rows.len(), 20);
        assert_eq!(rows[19].rank, 20);
    }

    #[test]
    fn goals_per_game_truncates() {
        let mut p = GameState::zeroed().players[1].clone();
        p.counters = [7, 3];
        assert_eq!(goals_per_game_tenths(&p), 23);
        p.counters = [7, 0];
        assert_eq!(goals_per_game_tenths(&p), 70);
        p.counters = [255, 1];
        assert_eq!(goals_per_game_tenths(&p), 2550);
    }

    #[test]
    fn real_saves_top_scorers() {
        for path in save_files() {
            let data = std::fs::read(&path).unwrap();
            let mut s = GameState::from_savegame(&SaveGame::parse(&data).unwrap()).unwrap();
            let ord = top_scorers_order(&mut s);
            let mut ids = ord[1..].to_vec();
            ids.sort_unstable();
            assert!(ids.iter().copied().eq(1..=150), "{path:?}");
            for w in ord[1..].windows(2) {
                let (a, b) = (&s.players[w[0] as usize], &s.players[w[1] as usize]);
                assert!(goals(a) >= goals(b), "{path:?}");
                assert!(goals(a) > goals(b) || games(a) <= games(b), "{path:?}");
            }
            // Pool players play at most one league game per round.
            let rounds = s.competition_rounds.iter().max().copied().unwrap_or(0);
            assert!(
                s.players[1..].iter().all(|p| games(p) <= rounds),
                "{path:?}"
            );
            for tier in 0..LEAGUE_TEAM_COUNT.len() {
                let rows = top_scorers_rows(&s, &ord, tier, false);
                for r in &rows {
                    let p = &s.players[r.player as usize];
                    assert!(
                        league_team_ids(tier).contains(&p.team) && goals(p) >= 2,
                        "{path:?}"
                    );
                }
            }
        }
    }
}
