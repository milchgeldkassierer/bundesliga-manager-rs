//! Characterization test of how the season end replaces the clubs relegated out of the
//! amateur tier (team ids 38..57) with the original league layout. The rule is written down in
//! `docs/mod2026/relegation-replacement.md`; this test pins the port's behaviour of
//! BMMAIN.EXE 2a58:3ee0 (promotion and relegation) and 1f9d:0697 (the strength reset that
//! follows) on a fixed state.

use bmp_game::ai::league_strength_drift;
use bmp_game::cups::season_promotion_relegation;
use bmp_game::rng::Rng;
use bmp_game::runtime::Runtime;
use bmp_game::state::{GERMAN_CLUBS, GameState};

/// `srand` seed of the run. Its first `rand()` is even, so `random_range(61, 62)` picks 61.
const SEED: u16 = 1996;

/// Final table order of the amateur tier, best club first.
const AMATEUR_TABLE: [u8; 20] = [
    45, 38, 52, 41, 57, 39, 50, 44, 47, 40, 55, 42, 49, 56, 43, 53, 46, 51, 48, 54,
];

/// Every club record and one player per club tagged with the club's original slot id, the
/// two Bundesligas in identity table order, the amateur tier in [`AMATEUR_TABLE`] order, and
/// the 2. Bundesliga club winning the relegation play-off.
fn final_tables() -> GameState {
    let mut s = GameState::zeroed();
    for i in 0..18 {
        s.league_teams[0][i] = i as u8;
    }
    for i in 0..20 {
        s.league_teams[1][i] = 18 + i as u8;
    }
    s.league_teams[2] = AMATEUR_TABLE;
    for id in 0..GERMAN_CLUBS {
        s.clubs[id].name[0] = id as u8;
        s.clubs[id].strength = [99; 3];
        s.clubs[id].base_strength = [1; 3];
        s.players[1 + id].team = id as u8;
    }
    s.relegation_result = 1;
    s
}

/// Original slot id of the club record now in slot `id`.
fn origin(s: &GameState, id: u8) -> u8 {
    s.clubs[usize::from(id)].name[0]
}

#[test]
fn season_end_third_group_replacement_is_pinned() {
    let rt = Runtime::default();
    let mut s = final_tables();
    let start = Rng::new(SEED);
    let mut rng = start;
    season_promotion_relegation(&mut s, &rt, &mut rng);

    // The only random number: one `random_range(61, 62)` for the replacement of the last club.
    let mut once = start;
    assert_eq!(once.random_range(61, 62), 61);
    assert_eq!(rng, once, "exactly one rand() call");

    // Slots 38..57 afterwards, by the original slot of the club now in them: the four top
    // clubs' slots (45, 38, 52, 41) took the 2. Bundesliga's last four (37, 36, 35, 34), the
    // four bottom clubs' slots (54, 48, 51, 46) took the non-league clubs 61, 60, 59, 58.
    let occupants: Vec<u8> = (38..=57).map(|id| origin(&s, id)).collect();
    assert_eq!(
        occupants,
        [
            36, 39, 40, 34, 42, 43, 44, 37, 58, 47, 60, 49, 50, 59, 35, 53, 61, 55, 56, 57
        ]
    );

    // The clubs that left the amateur tier: up into the 2. Bundesliga's last four slots, and
    // down into the non-league slots, the last-placed club into the drawn slot 61.
    let promoted: Vec<u8> = (34..=37).map(|id| origin(&s, id)).collect();
    assert_eq!(promoted, [41, 52, 38, 45]);
    let non_league: Vec<u8> = (58..=63).map(|id| origin(&s, id)).collect();
    assert_eq!(non_league, [46, 51, 48, 54, 62, 63]);

    // Players move with their club record.
    for id in 0..GERMAN_CLUBS as u8 {
        let owner = s.players[1 + usize::from(origin(&s, id))].team;
        assert_eq!(owner, id, "player of club {}", origin(&s, id));
    }
    assert_eq!(s.competition_rounds, [1, 1, 1]);

    // Nothing is regenerated: the strengths stay as they were until the new-season reset
    // (mode 10), which clamps every club from slot 38 up into the amateur bounds 15..=45.
    assert!((0..GERMAN_CLUBS).all(|id| s.clubs[id].strength == [99; 3]));
    league_strength_drift(&mut s, &rt, &mut rng, 10);
    for id in 38..GERMAN_CLUBS {
        let club = &s.clubs[id];
        for v in club.strength.iter().chain(&club.base_strength) {
            assert!((15..=45).contains(v), "slot {id}: {v}");
        }
    }
}
