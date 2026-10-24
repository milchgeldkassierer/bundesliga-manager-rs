//! Season logic of `BMMAIN.EXE` segment 1cb5 that the new game needs: the tier ranking by
//! strength and the random club pick for free pool players. The rest of the segment comes with
//! the season loop (#31).
//!
//! Full documentation with addresses: `docs/re/logic/season.md` §4.5 and §4.6.

use crate::rng::Rng;
use crate::runtime::{Runtime, TIER_FIRST};
use crate::state::GameState;

/// Ranks the clubs of tier `k` by strength, strongest first. Entry `i` of the result is the
/// position of a club inside the tier (not its club index); only the first
/// `tier_team_count[k]` entries are written.
///
/// A club's strength is the sum of its three `strength`, `base_strength` and `strength_1e`
/// values, divided by 9 (unsigned). The exchange sort swaps on a strictly smaller strength; it
/// moves the strength through a byte, which loses nothing for strengths up to 99.
///
/// Origin: BMMAIN.EXE 1cb5:2a3b (rank_tier_teams_by_strength)
pub fn rank_tier_teams_by_strength(state: &GameState, rt: &Runtime, k: usize) -> [u8; 20] {
    let cnt = rt.tier_team_count[k] as u8;
    let mut out = [0u8; 20];
    let mut s = [0u16; 20];
    for i in 0..cnt {
        out[i as usize] = i;
        let club = &state.clubs[TIER_FIRST[k].wrapping_add(i) as usize];
        let sum = (0..3).fold(0u16, |acc, j| {
            acc.wrapping_add(u16::from(club.strength[j]))
                .wrapping_add(u16::from(club.base_strength[j]))
                .wrapping_add(u16::from(club.strength_1e[j]))
        });
        s[i as usize] = sum / 9;
    }
    for i in 0..usize::from(cnt).saturating_sub(1) {
        for j in i + 1..usize::from(cnt) {
            if s[i] < s[j] {
                let held = s[i] as u8;
                s[i] = s[j];
                s[j] = u16::from(held);
                out.swap(i, j);
            }
        }
    }
    out
}

/// A random club of tier `k` that no human manager runs. Inside the three league tiers the
/// draw favours the first places of the tier: positions 0..3, with chance 1/2 0..6 instead,
/// and with chance 1/6 0..12 instead of either. Beyond the leagues (`k > 2`) any club from the
/// tier's first index up to 255 is drawn once, without the manager check.
///
/// The original also takes a ranked club list ([`rank_tier_teams_by_strength`]) but never
/// reads it.
///
/// Origin: BMMAIN.EXE 1cb5:2987 (pick_random_free_team_in_tier)
pub fn pick_random_free_team_in_tier(
    state: &GameState,
    rt: &Runtime,
    rng: &mut Rng,
    k: usize,
) -> u8 {
    if k > 2 {
        return rng.random_range(i16::from(rt.tier_club_ranges[k][0]), 0xff) as u8;
    }
    loop {
        let mut t = rng.random_range(0, 3) as u8;
        if rng.random_range(0, 1) != 0 {
            t = rng.random_range(0, 6) as u8;
        }
        if rng.random_range(0, 5) == 0 {
            t = rng.random_range(0, 12) as u8;
        }
        let t = t.wrapping_add(TIER_FIRST[k]);
        if state.manager_index_of_team(t, 0xff) == 0 {
            return t;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranking_orders_by_strength_sum() {
        let mut s = GameState::zeroed();
        let rt = Runtime::default();
        // Tier 1 starts at club 18; give positions 2 and 5 the highest sums.
        s.clubs[18 + 2].strength = [30, 30, 30]; // 270 / 9 = 30
        s.clubs[18 + 5].base_strength = [40, 40, 40]; // 40
        s.clubs[18 + 7].strength_1e = [8, 0, 0]; // 8 / 9 = 0: ties with the rest
        let out = rank_tier_teams_by_strength(&s, &rt, 1);
        assert_eq!(&out[..2], &[5, 2]);
        let mut rest: Vec<u8> = out[2..20].to_vec();
        rest.sort_unstable();
        assert_eq!(
            rest,
            (0..20).filter(|&i| i != 2 && i != 5).collect::<Vec<u8>>()
        );
        // Tier 0 has 18 clubs: the last two entries stay unwritten.
        assert_eq!(&rank_tier_teams_by_strength(&s, &rt, 0)[18..], &[0, 0]);
    }

    #[test]
    fn pick_never_returns_a_human_club() {
        let mut s = GameState::zeroed();
        let rt = Runtime::default();
        s.manager_count = 4;
        for m in 0..4 {
            s.managers[m].team = 18 + m as u8;
        }
        let mut rng = Rng::new(3);
        for _ in 0..500 {
            let t = pick_random_free_team_in_tier(&s, &rt, &mut rng, 1);
            assert!((22..=30).contains(&t), "{t}");
        }
    }

    #[test]
    fn pick_draws_three_to_five_numbers_per_try() {
        let s = GameState::zeroed();
        let rt = Runtime::default();
        for seed in 0..50 {
            let mut rng = Rng::new(seed);
            let mut probe = rng;
            let t = pick_random_free_team_in_tier(&s, &rt, &mut rng, 2);
            let mut draws = 0;
            while probe != rng {
                probe.rand();
                draws += 1;
            }
            assert!((3..=5).contains(&draws), "seed {seed}: {draws}");
            assert!((38..=50).contains(&t));
        }
    }
}
