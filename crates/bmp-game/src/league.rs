//! League logic of `BMMAIN.EXE` segment 3a41: the fixtures of a round and the
//! table update after a match day (team statistics, form strings, streaks,
//! records, head-to-head logs, morale and the table sort).
//!
//! The league screens of the segment (table, fixtures, strength table,
//! opponent info) are not part of this module.
//!
//! Full documentation with addresses: `docs/re/logic/stats.md` §2.

use crate::matchsim::league_result_cell;
use crate::rng::{Rng, clamp_int, long_abs_diff};
use crate::state::{Club, GERMAN_CLUBS, GameState, MatchHistory, TeamTable};

/// DS:226e first team id of each league tier.
pub const LEAGUE_FIRST_TEAM: [u8; 3] = [0, 18, 38];
/// DS:225e teams per league tier.
pub const LEAGUE_TEAM_COUNT: [u8; 3] = [18, 20, 20];

/// The team ids of league tier `league` (the original also keeps them as a
/// first/last table at DS:2272).
pub fn league_team_ids(league: usize) -> std::ops::Range<u8> {
    let first = LEAGUE_FIRST_TEAM[league];
    first..first + LEAGUE_TEAM_COUNT[league]
}

/// DS:2262 rounds per league tier.
pub const LEAGUE_ROUND_COUNT: [u8; 3] = [34, 38, 38];
/// DS:2271 points for a win (2-point rule).
pub const POINTS_PER_WIN: u8 = 2;
/// Home-goals value of a result that has not been played yet.
pub const NOT_PLAYED: u8 = 0x1e;
/// Home team id (`'F'`) that ends a pairing list.
pub const NO_MATCH: u8 = 0x46;

/// DS:227a form characters.
const FORM_HOME_WIN: u8 = b's';
const FORM_AWAY_WIN: u8 = b'S';
const FORM_HOME_LOSS: u8 = b'n';
const FORM_AWAY_LOSS: u8 = b'N';
const FORM_HOME_DRAW: u8 = b'u';
const FORM_AWAY_DRAW: u8 = b'U';

/// Streak categories `k` of [`MatchHistory::team_series`].
const STREAK_WON: usize = 0;
const STREAK_LOST: usize = 1;
const STREAK_DRAWN: usize = 2;
const STREAK_NOT_WON: usize = 3;
const STREAK_NOT_LOST: usize = 4;
const STREAK_CLEAN_SHEET: usize = 5;
const STREAK_NO_GOAL: usize = 6;
/// Streak column of the total; the home column is 1, the away column 2.
const STREAK_TOTAL: usize = 0;

/// DS:06fc round permutations: 1..17 for 18-team tiers, 19..37 (the pointers
/// from DS:0744) for 20-team tiers; letters `a..` are team slots. Strings 0
/// and 18 are empty and never used, because the round is 1-based.
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

/// Builds the pairings of `round` (1-based) of `league` into
/// [`GameState::matchday_pairings`]; the second half of the season repeats the
/// first with home and away swapped. Round 0 has no permutation (string 0 is
/// empty; the original reads the bytes past it): the list is ended with
/// [`NO_MATCH`] at once, so no fixture is played.
///
/// Origin: BMMAIN.EXE 3a41:10e3 (fixtures_build_round_pairings)
pub fn fixtures_build_round_pairings(state: &mut GameState, league: usize, round: u8) {
    let offset = LEAGUE_FIRST_TEAM[league];
    let half = LEAGUE_ROUND_COUNT[league] >> 1;
    let mut round = round;
    let mut second_half = false;
    if half < round {
        round = round.wrapping_sub(half);
        second_half = true;
    }
    if half < round {
        round = half;
    }
    let count = LEAGUE_TEAM_COUNT[league];
    let perm = if count == 18 {
        PAIRINGS[round as usize]
    } else {
        PAIRINGS[18 + round as usize]
    };
    // TODO(re): the pairings the original builds from the bytes past string 0.
    if perm.is_empty() {
        state.matchday_pairings[league][0][0] = NO_MATCH;
        return;
    }
    for i in 0..(count >> 1) as usize {
        let (mut a, mut b) = (perm[2 * i] - b'a', perm[2 * i + 1] - b'a');
        if second_half {
            std::mem::swap(&mut a, &mut b);
        }
        state.matchday_pairings[league][i] = [a + offset, b + offset];
    }
}

/// Copies column `src` of a table record into column `dst` (0 = current, 2 =
/// snapshot): points, goals, the home byte of wins and losses, and the form
/// string. Games played, away wins and away losses are not copied.
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

/// Shifts the 8-character form string left and appends `c` as the newest result.
///
/// Origin: BMMAIN.EXE 3a41:1fb0 (team_form_push)
pub fn team_form_push(form: &mut [u8; 9], c: u8) {
    form.copy_within(1..8, 0);
    form[7] = c;
}

/// Moves the club's morale up (`goal_diff > 0`) or down by a random amount of
/// up to twice the goal difference, clamped to 47..=53. Always draws one
/// random number, also for a draw (`random_range(0, 0)`).
///
/// Origin: BMMAIN.EXE 3a41:1fec (team_morale_adjust)
pub fn team_morale_adjust(club: &mut Club, rng: &mut Rng, goal_diff: i16) {
    let mut v = club.morale as i16;
    let spread = (long_abs_diff(0, goal_diff as i32) as i16).wrapping_mul(2);
    let d = rng.random_range(0, spread);
    if goal_diff > 0 {
        v = v.wrapping_add(d);
    } else {
        v = v.wrapping_sub(d);
    }
    clamp_int(&mut v, 47, 53);
    club.morale = v as u8;
}

/// Appends `their * 16 + own` to manager `m`'s last five results against club
/// `opp` on `side` (0 = at home, 1 = away), dropping the oldest when full.
/// Indices outside the tables (the original writes past them) change nothing.
///
/// Origin: BMMAIN.EXE 3a41:2073 (head_to_head_log_result)
pub fn head_to_head_log_result(
    history: &mut MatchHistory,
    m: usize,
    opp: u8,
    side: usize,
    own: u8,
    their: u8,
) {
    let Some(log) = history
        .recent_results
        .get_mut(m)
        .and_then(|r| r.get_mut(opp as usize))
    else {
        return;
    };
    let mut k = 0;
    while k < 5 && log[k][side] != 0xff {
        k += 1;
    }
    if k == 5 {
        for j in 0..4 {
            log[j][side] = log[j + 1][side];
        }
        k = 4;
    }
    log[k][side] = their.wrapping_mul(16).wrapping_add(own);
}

/// Replaces record slot `s` of club `t` when the weighted score
/// `a * wa - b * wb` beats the stored one (`hi * wa - lo * wb` of the nibbles),
/// smaller wins when `minimize`. The new value is `(a * wa) << 4 | wb * b`
/// with a negative `wb` replaced by 1, all in 8 bits.
///
/// Origin: BMMAIN.EXE 3a41:37cf (team_record_update)
#[allow(clippy::too_many_arguments)]
pub fn team_record_update(
    history: &mut MatchHistory,
    t: u8,
    s: usize,
    a: u8,
    b: u8,
    minimize: bool,
    wa: i16,
    mut wb: i16,
    opp: u8,
) {
    let v = history.record_scores[t as usize][s];
    let (hi, lo) = ((v >> 4) as i16, (v & 15) as i16);
    let (a, b) = (a as i16, b as i16);
    let old_score = hi.wrapping_mul(wa).wrapping_sub(lo.wrapping_mul(wb));
    let new_score = a.wrapping_mul(wa).wrapping_sub(b.wrapping_mul(wb));
    let take = if minimize {
        new_score < old_score
    } else {
        old_score < new_score
    };
    if take {
        if wb < 0 {
            wb = 1;
        }
        let high = (a.wrapping_mul(wa) as u8).wrapping_shl(4);
        history.record_scores[t as usize][s] = high.wrapping_add(wb.wrapping_mul(b) as u8);
        history.record_opponents[t as usize][s] = opp;
    }
}

/// What [`league_apply_round_and_sort`] does with the results of the round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundMode {
    /// Mode 0: add the results to the snapshot columns of the clubs and update
    /// streaks, records, head-to-head logs and morale. Must run exactly once
    /// per round: only points, goals, home wins/losses and the form string
    /// are rebuilt from the snapshot, everything else accumulates.
    Apply,
    /// Mode 1: copy the current columns of every club with a result into the
    /// snapshot columns.
    Snapshot,
}

/// The ten result pairs of round index `r` (0-based) of `league`, read like
/// the original's `FAR:6ddc + (league * 38 + r) * 20` from the flat table, so
/// `r = -1` reaches round 37 of the previous league. `None` outside save block 6,
/// by the same rule as [`league_result_cell`], which it is read through.
pub(crate) fn round_results(state: &GameState, league: usize, r: i8) -> Option<[u8; 20]> {
    let cell = league_result_cell(u8::try_from(league).ok()?, i16::from(r) + 1, 0)?;
    let flat = state.league_results.as_flattened().as_flattened();
    flat[cell..cell + 20].try_into().ok()
}

/// Processes the results of round `competition_rounds[league] - 1` (0-based)
/// as `mode` says, then sorts the table of `league` and stores every human
/// manager's position for the round.
///
/// `sort_cols` selects the columns of the sort: 0 = home, 1 = home + away,
/// 2 = away (see [`GameState::sort_league_table`]); `ignore_played` is
/// DS:224d != 0. In [`RoundMode::Apply`] the morale of the home club draws a
/// random number before that of the away club, pair by pair.
///
/// Origin: BMMAIN.EXE 3a41:2d33 (league_apply_round_and_sort)
pub fn league_apply_round_and_sort(
    state: &mut GameState,
    rng: &mut Rng,
    league: usize,
    mode: RoundMode,
    sort_cols: u8,
    ignore_played: bool,
) {
    if league > 2 {
        return;
    }
    let r = state.competition_rounds[league].wrapping_sub(1) as i8;
    if r > LEAGUE_ROUND_COUNT[league] as i8 - 1 {
        return;
    }
    let count = LEAGUE_TEAM_COUNT[league] as usize;
    // TODO(re): for league 0 and round counter 0 the original reads the 20
    // bytes before FAR:6ddc as results; they are treated as unplayed here.
    if let Some(results) = round_results(state, league, r) {
        for i in (0..count).step_by(2) {
            let (gh, ga) = (results[i], results[i + 1]);
            if gh == NOT_PLAYED {
                continue;
            }
            let [h, a] = state.matchday_pairings[league][i / 2];
            if mode == RoundMode::Apply {
                let mh = state.manager_index_of_team(h, 0xff);
                // Arguments swapped in the binary: a match on the second
                // argument returns m + 0x81, masked to m + 1.
                let ma = state.manager_index_of_team(0xff, a) & 0x7f;
                if mh != 0 {
                    let m = mh as usize - 1;
                    head_to_head_log_result(&mut state.match_history, m, a, 0, gh, ga);
                }
                if ma != 0 {
                    let m = ma as usize - 1;
                    head_to_head_log_result(&mut state.match_history, m, h, 1, ga, gh);
                }
            }
            if h == NO_MATCH {
                break;
            }
            match mode {
                RoundMode::Snapshot => {
                    team_stats_copy_column(&mut state.team_tables[h as usize], 0, 2);
                    team_stats_copy_column(&mut state.team_tables[a as usize], 0, 2);
                }
                RoundMode::Apply => apply_result(state, rng, h, a, gh, ga),
            }
        }
    }
    if mode == RoundMode::Apply {
        update_best_streaks(state);
    }
    state.sort_league_table(league, count, sort_cols, ignore_played);
    state.record_position_history(league);
}

/// Mode 0 body of [`league_apply_round_and_sort`] for one played pair: home
/// club `h` scored `gh`, away club `a` scored `ga`.
fn apply_result(state: &mut GameState, rng: &mut Rng, h: u8, a: u8, gh: u8, ga: u8) {
    let (hi, ai) = (h as usize, a as usize);
    team_stats_copy_column(&mut state.team_tables[hi], 2, 0);
    team_stats_copy_column(&mut state.team_tables[ai], 2, 0);
    {
        let th = &mut state.team_tables[hi];
        th.goals_for[0] = th.goals_for[0].wrapping_add(gh);
        th.goals_against[0] = th.goals_against[0].wrapping_add(ga);
        th.played[0] = th.played[0].wrapping_add(1);
    }
    {
        let ta = &mut state.team_tables[ai];
        ta.goals_for[1] = ta.goals_for[1].wrapping_add(ga);
        ta.goals_against[1] = ta.goals_against[1].wrapping_add(gh);
        ta.played[1] = ta.played[1].wrapping_add(1);
    }
    let hist = &mut state.match_history;
    team_record_update(hist, h, 2, gh, ga, false, 1, 0, a);
    team_record_update(hist, a, 7, gh, ga, false, 1, 0, h);
    team_record_update(hist, h, 3, gh, ga, false, 0, -1, a);
    team_record_update(hist, a, 6, gh, ga, false, 0, -1, h);

    let mut home = Streaks::new(hist, hi, 1);
    home.set(STREAK_CLEAN_SHEET, ga == 0);
    home.set(STREAK_NO_GOAL, gh == 0);
    let mut away = Streaks::new(hist, ai, 2);
    away.set(STREAK_CLEAN_SHEET, gh == 0);
    away.set(STREAK_NO_GOAL, ga == 0);

    team_morale_adjust(
        &mut state.clubs[hi],
        rng,
        (gh as i16).wrapping_sub(ga as i16),
    );
    team_morale_adjust(
        &mut state.clubs[ai],
        rng,
        (ga as i16).wrapping_sub(gh as i16),
    );

    let hist = &mut state.match_history;
    let (home_form, away_form);
    if ga < gh {
        let pts = &mut state.team_tables[hi].points[0];
        *pts = pts.wrapping_add(POINTS_PER_WIN);
        Streaks::new(hist, hi, 1).set_outcome(Outcome::Won);
        team_record_update(hist, h, 0, gh, ga, false, 1, 1, a);
        team_record_update(hist, a, 5, gh, ga, false, 1, 1, h);
        Streaks::new(hist, ai, 2).set_outcome(Outcome::Lost);
        let wins = &mut state.team_tables[hi].wins[0];
        *wins = wins.wrapping_add(1);
        let losses = &mut state.team_tables[ai].losses[1];
        *losses = losses.wrapping_add(1);
        (home_form, away_form) = (FORM_HOME_WIN, FORM_AWAY_LOSS);
    } else if ga == gh {
        let pts = &mut state.team_tables[hi].points[0];
        *pts = pts.wrapping_add(1);
        let pts = &mut state.team_tables[ai].points[1];
        *pts = pts.wrapping_add(1);
        Streaks::new(hist, hi, 1).set_outcome(Outcome::Drawn);
        Streaks::new(hist, ai, 2).set_outcome(Outcome::Drawn);
        (home_form, away_form) = (FORM_HOME_DRAW, FORM_AWAY_DRAW);
    } else {
        let ta = &mut state.team_tables[ai];
        ta.points[1] = ta.points[1].wrapping_add(POINTS_PER_WIN);
        ta.wins[1] = ta.wins[1].wrapping_add(1);
        let losses = &mut state.team_tables[hi].losses[0];
        *losses = losses.wrapping_add(1);
        Streaks::new(hist, ai, 2).set_outcome(Outcome::Won);
        Streaks::new(hist, hi, 1).set_outcome(Outcome::Lost);
        team_record_update(hist, h, 1, gh, ga, true, 1, 1, a);
        team_record_update(hist, a, 4, gh, ga, true, 1, 1, h);
        (home_form, away_form) = (FORM_HOME_LOSS, FORM_AWAY_WIN);
    }
    team_form_push(&mut state.team_tables[hi].form[0], home_form);
    team_form_push(&mut state.team_tables[ai].form[0], away_form);
}

/// Result of a match from one club's view.
#[derive(Clone, Copy)]
enum Outcome {
    Won,
    Lost,
    Drawn,
}

/// The streak counters of one club in the total column and one side column
/// (1 = home, 2 = away); every update touches both.
struct Streaks<'a> {
    cells: &'a mut [[u8; 3]; 7],
    side: usize,
}

impl<'a> Streaks<'a> {
    fn new(history: &'a mut MatchHistory, team: usize, side: usize) -> Self {
        Streaks {
            cells: &mut history.team_series[team],
            side,
        }
    }

    /// Extends streak `k` when `holds` (u8 wrap), else resets it.
    fn set(&mut self, k: usize, holds: bool) {
        for c in [self.side, STREAK_TOTAL] {
            let v = &mut self.cells[k][c];
            *v = if holds { v.wrapping_add(1) } else { 0 };
        }
    }

    fn set_outcome(&mut self, outcome: Outcome) {
        let (won, lost, drawn) = match outcome {
            Outcome::Won => (true, false, false),
            Outcome::Lost => (false, true, false),
            Outcome::Drawn => (false, false, true),
        };
        self.set(STREAK_WON, won);
        self.set(STREAK_LOST, lost);
        self.set(STREAK_DRAWN, drawn);
        self.set(STREAK_NOT_WON, !won);
        self.set(STREAK_NOT_LOST, !lost);
    }
}

/// Raises each human manager's best streaks to the current streaks of the
/// manager's club. Managers without a German club are skipped (the original
/// reads past the streak table for them).
///
/// Origin: BMMAIN.EXE 3a41:2d33 (league_apply_round_and_sort, best-streak loop)
fn update_best_streaks(state: &mut GameState) {
    for m in 0..state.active_managers().len() {
        let t = state.managers[m].team as usize;
        if t >= GERMAN_CLUBS {
            continue;
        }
        let hist = &mut state.match_history;
        for k in 0..7 {
            for c in 0..3 {
                let cur = hist.team_series[t][k][c];
                let best = &mut hist.manager_series_best[m][k][c];
                if *best < cur {
                    *best = cur;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmp_formats::save_files;
    use bmp_formats::savegame::SaveGame;

    const EMPTY_FORM: [u8; 9] = *b"@@@@@@@@\0";

    #[test]
    fn pairings_cover_every_club_once_per_round() {
        let mut s = GameState::zeroed();
        for tier in 0..3 {
            for round in 1..=LEAGUE_ROUND_COUNT[tier] {
                fixtures_build_round_pairings(&mut s, tier, round);
                let n = LEAGUE_TEAM_COUNT[tier] as usize;
                let mut seen: Vec<u8> = s.matchday_pairings[tier][..n / 2]
                    .iter()
                    .flatten()
                    .copied()
                    .collect();
                seen.sort_unstable();
                assert_eq!(seen, league_team_ids(tier).collect::<Vec<_>>());
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
        // Past the season the round is clamped to the last one of the half.
        let mut t = s.clone();
        fixtures_build_round_pairings(&mut s, 0, 34);
        fixtures_build_round_pairings(&mut t, 0, 60);
        assert_eq!(s.matchday_pairings[0], t.matchday_pairings[0]);
    }

    #[test]
    fn copy_column_snapshots_the_current_values() {
        let mut t = GameState::zeroed().team_tables[0].clone();
        t.points = [5, 6, 0, 0];
        t.wins = [2, 3, 0, 0];
        t.played = [4, 5, 0, 0];
        t.form[0] = *b"sUN@@@@@\0";
        team_stats_copy_column(&mut t, 0, 2);
        assert_eq!(t.points, [5, 6, 5, 6]);
        assert_eq!(t.wins, [2, 3, 2, 0]);
        assert_eq!(t.played, [4, 5, 0, 0]);
        assert_eq!(t.form[1], t.form[0]);
    }

    #[test]
    fn form_push_drops_the_oldest_result() {
        let mut form = *b"abcdefgh\0";
        team_form_push(&mut form, b'S');
        assert_eq!(&form, b"bcdefghS\0");
    }

    #[test]
    fn morale_draws_once_and_clamps() {
        let mut club = GameState::zeroed().clubs[0].clone();
        // A draw still calls random_range(0, 0) and clamps 40 up to 47.
        club.morale = 40;
        let mut rng = Rng::new(9);
        let mut expect = rng;
        team_morale_adjust(&mut club, &mut rng, 0);
        expect.rand();
        assert_eq!((club.morale, rng), (47, expect));
        // A 0:5 loss can only push the morale further down: stays at 47.
        team_morale_adjust(&mut club, &mut rng, -5);
        assert_eq!(club.morale, 47);
        // A 3:0 win adds random_range(0, 6), capped at 53.
        club.morale = 50;
        let mut expect = rng;
        let d = expect.random_range(0, 6) as u8;
        team_morale_adjust(&mut club, &mut rng, 3);
        assert_eq!(club.morale, (50 + d).min(53));
        assert_eq!(rng, expect);
    }

    #[test]
    fn head_to_head_keeps_the_last_five() {
        let mut h = GameState::zeroed().match_history;
        h.recent_results[1][7] = [[0xff; 2]; 5];
        for g in 0..6 {
            head_to_head_log_result(&mut h, 1, 7, 1, g, 1);
        }
        let away: Vec<u8> = h.recent_results[1][7].iter().map(|e| e[1]).collect();
        assert_eq!(away, [0x11, 0x12, 0x13, 0x14, 0x15]);
        assert!(h.recent_results[1][7].iter().all(|e| e[0] == 0xff));
        // `their * 16` wraps in 8 bits: 17 * 16 + 2 = 0x112 -> 0x12.
        head_to_head_log_result(&mut h, 1, 7, 0, 2, 17);
        assert_eq!(h.recent_results[1][7][0][0], 0x12);
        // Manager or opponent beyond the tables: nothing is written.
        let before = h.clone();
        head_to_head_log_result(&mut h, 4, 7, 0, 1, 1);
        head_to_head_log_result(&mut h, 0, 64, 0, 1, 1);
        assert_eq!(h, before);
    }

    #[test]
    fn record_update_weights_and_truncation() {
        let mut h = GameState::zeroed().match_history;
        // Slot 0 (biggest home win): 3:1 beats the empty record; 4:2 has the
        // same difference and is not taken; 5:1 is.
        team_record_update(&mut h, 0, 0, 3, 1, false, 1, 1, 9);
        assert_eq!((h.record_scores[0][0], h.record_opponents[0][0]), (0x31, 9));
        team_record_update(&mut h, 0, 0, 4, 2, false, 1, 1, 8);
        assert_eq!((h.record_scores[0][0], h.record_opponents[0][0]), (0x31, 9));
        team_record_update(&mut h, 0, 0, 5, 1, false, 1, 1, 8);
        assert_eq!((h.record_scores[0][0], h.record_opponents[0][0]), (0x51, 8));
        // Slot 3 (goals conceded at home): wb = -1 scores `b`, stored as 1 * b.
        team_record_update(&mut h, 0, 3, 0, 3, false, 0, -1, 4);
        assert_eq!(h.record_scores[0][3], 0x03);
        team_record_update(&mut h, 0, 3, 5, 2, false, 0, -1, 4);
        assert_eq!(h.record_scores[0][3], 0x03);
        // Slot 4 (biggest away win, minimized home - away): 0:2 -> 0x02.
        team_record_update(&mut h, 1, 4, 0, 2, true, 1, 1, 0);
        assert_eq!(h.record_scores[1][4], 0x02);
        // 20 goals do not fit the nibble: (20 << 4) & 0xff = 0x40.
        team_record_update(&mut h, 2, 2, 20, 0, false, 1, 0, 3);
        assert_eq!(h.record_scores[2][2], 0x40);
    }

    /// League 0 in round 1 with three played pairs (0-1 3:1, 2-3 2:2, 4-5 0:1),
    /// manager 0 managing club 0.
    fn round_one() -> GameState {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.managers[0].team = 0;
        s.match_history.recent_results = [[[[0xff; 2]; 5]; 64]; 4];
        for t in &mut s.team_tables {
            t.form = [EMPTY_FORM; 2];
        }
        for c in &mut s.clubs {
            c.morale = 50;
        }
        for (i, t) in s.league_teams[0].iter_mut().enumerate() {
            *t = i as u8;
        }
        s.competition_rounds[0] = 1;
        fixtures_build_round_pairings(&mut s, 0, 1);
        let row = &mut s.league_results[0][0];
        *row = [NOT_PLAYED; 20];
        row[..6].copy_from_slice(&[3, 1, 2, 2, 0, 1]);
        s
    }

    #[test]
    fn apply_round_updates_every_table() {
        let mut s = round_one();
        let mut rng = Rng::new(1234);
        let mut expect = rng;
        league_apply_round_and_sort(&mut s, &mut rng, 0, RoundMode::Apply, 1, false);

        // Morale: home before away, pair by pair; a draw draws random_range(0, 0).
        let d: Vec<u8> = [4, 4, 0, 0, 2, 2]
            .iter()
            .map(|&hi| expect.random_range(0, hi) as u8)
            .collect();
        assert_eq!(rng, expect);
        let morale: Vec<u8> = s.clubs[..6].iter().map(|c| c.morale).collect();
        assert_eq!(morale, [50 + d[0], 50 - d[1], 50, 50, 50 - d[4], 50 + d[5]]);

        let t = &s.team_tables;
        assert_eq!(
            (t[0].points, t[0].goals_for, t[0].goals_against),
            ([2, 0, 0, 0], [3, 0, 0, 0], [1, 0, 0, 0])
        );
        assert_eq!((t[0].played, t[0].wins), ([1, 0, 0, 0], [1, 0, 0, 0]));
        assert_eq!(&t[0].form[0], b"@@@@@@@s\0");
        assert_eq!((t[1].played, t[1].losses), ([0, 1, 0, 0], [0, 1, 0, 0]));
        assert_eq!(
            (t[1].goals_for, t[1].goals_against),
            ([0, 1, 0, 0], [0, 3, 0, 0])
        );
        assert_eq!(&t[1].form[0], b"@@@@@@@N\0");
        assert_eq!((t[2].points, t[3].points), ([1, 0, 0, 0], [0, 1, 0, 0]));
        assert_eq!(
            (&t[2].form[0], &t[3].form[0]),
            (b"@@@@@@@u\0", b"@@@@@@@U\0")
        );
        assert_eq!((t[4].losses, &t[4].form[0]), ([1, 0, 0, 0], b"@@@@@@@n\0"));
        assert_eq!((t[5].points, t[5].wins), ([0, 2, 0, 0], [0, 1, 0, 0]));
        assert_eq!(&t[5].form[0], b"@@@@@@@S\0");
        assert_eq!(t[6].played, [0; 4]);

        let h = &s.match_history;
        // Streaks: [won, lost, drawn, not won, not lost, clean sheet, no goal]
        // x [total, home, away].
        let won_home = [[1, 1, 0], [0; 3], [0; 3], [0; 3], [1, 1, 0], [0; 3], [0; 3]];
        assert_eq!(h.team_series[0], won_home);
        let lost_away = [[0; 3], [1, 0, 1], [0; 3], [1, 0, 1], [0; 3], [0; 3], [0; 3]];
        assert_eq!(h.team_series[1], lost_away);
        assert_eq!(h.team_series[4][6], [1, 1, 0]);
        assert_eq!(h.team_series[5][5], [1, 0, 1]);
        assert_eq!(h.manager_series_best[0], won_home);
        // Records: home winner 0 and away loser 1.
        assert_eq!(h.record_scores[0], [0x31, 0, 0x30, 0x01, 0, 0, 0, 0]);
        assert_eq!(h.record_opponents[0], [1, 0, 1, 1, 0, 0, 0, 0]);
        assert_eq!(h.record_scores[1], [0, 0, 0, 0, 0, 0x31, 0x01, 0x30]);
        assert_eq!((h.record_scores[5][4], h.record_scores[4][1]), (0x01, 0x01));
        // Manager 0 logged 3:1 at home against club 1, nothing else.
        assert_eq!(h.recent_results[0][1][0], [0x13, 0xff]);
        let logged = h.recent_results.iter().flatten().flatten().flatten();
        assert_eq!(logged.filter(|&&b| b != 0xff).count(), 1);

        // Table: 0 (+2) before 5 (+1); unplayed clubs before the losers 4 and 1.
        assert_eq!(s.league_teams[0][..2], [0, 5]);
        assert_eq!(s.league_teams[0][16..18], [4, 1]);
        assert_eq!(s.team_tables[1].table_position, 17);
        assert_eq!(s.managers[0].position_history[0], 0);
    }

    #[test]
    fn apply_round_logs_away_results_draw_streaks_and_positions() {
        let mut s = round_one();
        s.managers[0].team = 1;
        let mut rng = Rng::new(1234);
        league_apply_round_and_sort(&mut s, &mut rng, 0, RoundMode::Apply, 1, false);

        let h = &s.match_history;
        // Manager 0 lost 1:3 away at club 0: their 3 * 16 + own 1.
        assert_eq!(h.recent_results[0][0][0], [0xff, 0x31]);
        let logged = h.recent_results.iter().flatten().flatten().flatten();
        assert_eq!(logged.filter(|&&b| b != 0xff).count(), 1);
        // The 2:2 of clubs 2 (home) and 3 (away).
        let drawn_home = [
            [0; 3],
            [0; 3],
            [1, 1, 0],
            [1, 1, 0],
            [1, 1, 0],
            [0; 3],
            [0; 3],
        ];
        let drawn_away = [
            [0; 3],
            [0; 3],
            [1, 0, 1],
            [1, 0, 1],
            [1, 0, 1],
            [0; 3],
            [0; 3],
        ];
        assert_eq!(h.team_series[2], drawn_home);
        assert_eq!(h.team_series[3], drawn_away);
        assert_eq!(h.manager_series_best[0], h.team_series[1]);
        // Club 1 lost the highest defeat: 17th of 18 after round 1.
        assert_eq!(s.team_tables[1].table_position, 17);
        assert_eq!(s.managers[0].position_history[0], 17);
    }

    #[test]
    fn apply_twice_rebuilds_points_but_counts_games_again() {
        let mut s = round_one();
        let mut rng = Rng::new(1);
        league_apply_round_and_sort(&mut s, &mut rng, 0, RoundMode::Apply, 1, false);
        league_apply_round_and_sort(&mut s, &mut rng, 0, RoundMode::Apply, 1, false);
        let t = &s.team_tables[0];
        assert_eq!((t.points, t.goals_for), ([2, 0, 0, 0], [3, 0, 0, 0]));
        assert_eq!((t.played, t.wins), ([2, 0, 0, 0], [1, 0, 0, 0]));
        assert_eq!(s.match_history.team_series[0][0], [2, 2, 0]);
    }

    #[test]
    fn snapshot_copies_played_clubs_without_randomness() {
        let mut s = round_one();
        let mut rng = Rng::new(1);
        league_apply_round_and_sort(&mut s, &mut rng, 0, RoundMode::Apply, 1, false);
        s.team_tables[6].points[0] = 7;
        let before = rng;
        league_apply_round_and_sort(&mut s, &mut rng, 0, RoundMode::Snapshot, 1, false);
        assert_eq!(rng, before);
        assert_eq!(s.team_tables[0].points, [2, 0, 2, 0]);
        assert_eq!(s.team_tables[0].form[1], s.team_tables[0].form[0]);
        assert_eq!(s.team_tables[6].points, [7, 0, 0, 0]);
    }

    #[test]
    fn end_marker_stops_after_logging() {
        let mut s = round_one();
        s.matchday_pairings[0][1] = [NO_MATCH, 3];
        s.managers[0].team = 3;
        let mut rng = Rng::new(1);
        league_apply_round_and_sort(&mut s, &mut rng, 0, RoundMode::Apply, 1, false);
        assert_eq!(s.team_tables[0].played, [1, 0, 0, 0]);
        assert_eq!(s.team_tables[4].played, [0; 4]);
        // Club 3's manager is looked up before the marker test, but the
        // opponent id 0x46 lies outside the log, so nothing is written.
        let logged = s.match_history.recent_results.iter().flatten().flatten();
        assert!(logged.flatten().all(|&b| b == 0xff));
        assert_eq!(s.team_tables[3].played, [0; 4]);
    }

    #[test]
    fn round_counter_past_the_season_changes_nothing() {
        let mut s = round_one();
        s.competition_rounds[0] = 35;
        s.league_teams[0].reverse();
        let before = s.clone();
        let mut rng = Rng::new(1);
        league_apply_round_and_sort(&mut s, &mut rng, 0, RoundMode::Apply, 1, false);
        assert_eq!(s, before);
        assert_eq!(rng, Rng::new(1));
    }

    /// Replays the season so far from the saved results (snapshot, then
    /// apply, round by round) and compares the tables with the save. Clubs
    /// whose rescheduled matches were played out of order end with the same
    /// totals but a different form string, so the form is not compared.
    #[test]
    fn real_saves_replay_the_league_tables() {
        for path in save_files() {
            let data = std::fs::read(&path).unwrap();
            let s = GameState::from_savegame(&SaveGame::parse(&data).unwrap()).unwrap();
            let mut t = s.clone();
            let mut rng = Rng::new(1);
            for l in 0..3 {
                let clubs = league_team_ids(l).map(usize::from);
                for c in clubs.clone() {
                    let r = &mut t.team_tables[c];
                    for col in [
                        &mut r.points,
                        &mut r.goals_for,
                        &mut r.goals_against,
                        &mut r.played,
                        &mut r.wins,
                        &mut r.losses,
                    ] {
                        *col = [0; 4];
                    }
                    r.form = [EMPTY_FORM; 2];
                }
                for round in 1..s.competition_rounds[l] {
                    t.competition_rounds[l] = round;
                    fixtures_build_round_pairings(&mut t, l, round);
                    league_apply_round_and_sort(&mut t, &mut rng, l, RoundMode::Snapshot, 1, false);
                    league_apply_round_and_sort(&mut t, &mut rng, l, RoundMode::Apply, 1, false);
                }
                for c in clubs {
                    let (a, b) = (&s.team_tables[c], &t.team_tables[c]);
                    let current = |r: &TeamTable| {
                        let cols = [
                            r.points,
                            r.goals_for,
                            r.goals_against,
                            r.played,
                            r.wins,
                            r.losses,
                        ];
                        cols.map(|col| [col[0], col[1]])
                    };
                    assert_eq!(current(a), current(b), "{path:?} club {c}");
                }
                assert_eq!(t.league_teams[l], s.league_teams[l], "{path:?} league {l}");
            }
        }
    }
}
