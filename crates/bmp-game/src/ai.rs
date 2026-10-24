//! Computer clubs, team strength and match AI: segment 1f9d of `BMMAIN.EXE`.
//!
//! Only human managers' clubs have player squads. Every club carries three strength arrays
//! with one byte per line (0 = defence, 1 = midfield, 2 = attack) plus a morale byte; the match
//! engine reads them through [`team_strength_value`]. A computer club's strength only changes
//! through the random walk [`league_strength_drift`]; a human club's strength is recomputed from
//! its starting eleven by [`squad_recalc_team_strength`]. Matches that are not shown live are
//! decided by [`match_simulate_quick_result`], goal chances in the live view by
//! [`match_goal_chance`]. [`squad_lineup_totals`] feeds the training screen and
//! [`match_attendance_estimate`] the match-day income.
//!
//! Names in `docs/re/logic/ai.md` and the fields they map to:
//!
//! | ai.md | Field |
//! |---|---|
//! | `str_a` (club +0x18) | [`Club::strength`](crate::state::Club::strength) |
//! | `str_b` (club +0x1b) | [`Club::base_strength`](crate::state::Club::base_strength) |
//! | `str_c` (club +0x1e) | [`Club::strength_1e`](crate::state::Club::strength_1e) |
//! | `str_bonus` (club +0x17) | [`Club::morale`](crate::state::Club::morale) |
//! | `attr_a/b/c` (squad +0x10) | [`SquadPlayer::skills`](crate::state::SquadPlayer::skills) |
//! | `exp` (squad +0x03) | `SquadPlayer::goals[0]` |
//! | `stamina6` (squad +0x06) | `SquadPlayer::appearances[0]` |
//! | `fatigue` (squad +0x13) | [`SquadPlayer::exhaustion`](crate::state::SquadPlayer::exhaustion) |
//! | `role` (squad +0x19) | [`SquadPlayer::board_col`](crate::state::SquadPlayer::board_col) |
//! | `pos_code` (squad +0x1a) | [`SquadPlayer::board_row`](crate::state::SquadPlayer::board_row) |
//! | popularity (manager +0x10a) | [`Manager::value_10a`](crate::state::Manager::value_10a) |
//! | slider (manager +0x131) | [`Manager::value_131`](crate::state::Manager::value_131) |
//! | `f1dc` (manager +0x1dc) | [`Manager::fans`](crate::state::Manager::fans) |
//!
//! The squad names follow `docs/re/logic/squad.md`, which the squad screen's strings back;
//! ai.md named the same bytes from their use here. The arithmetic is the same either way.
// TODO(re): ai.md §6.1/§6.2 — the A/B/C to KONDITION/TECHNIK/FORM mapping and the ai.md
// meanings of squad +0x03/+0x06/+0x19/+0x1a are not confirmed; the bytes are used as read.

use crate::matchsim::{
    cup_cell, goal_minutes_generate, goal_pick, league_result_cell, player_position_mismatch,
    player_role_mismatch, team_strength_value,
};
use crate::rng::{Rng, clamp_int, ldiv, lmul};
use crate::runtime::Runtime;
use crate::squad::squad_count_listed;
use crate::state::{GERMAN_CLUBS, GameState};
use crate::x87::F80;

/// DS:02a2: line of each tactics-board position `7 - board_row` (0 = goal .. 7 = attack).
const POSITION_LINE: [i8; 8] = [0, 0, 0, 1, 1, 1, 2, 2];
/// DS:02aa: most goals a quick result draws for a strength difference of 0..=19.
const QUICK_GOAL_MAX: [u8; 20] = [1, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4];
/// DS:05ce: strength bounds `[lo, hi]` per league tier for the new-season drift.
const LEAGUE_STRENGTH_BOUNDS: [[u8; 2]; 3] = [[70, 93], [40, 74], [15, 45]];
/// DS:02c8: match capacity base per tier of the home club. Entry 4 is what the original reads
/// past the table (the bytes `00 61 62 63` at DS:02d8) when no tier range matches.
const ATTENDANCE_TIER_BASE: [i32; 5] = [4500, 2200, 1200, 600, 0x6362_6100];
/// League result cell value of a fixture that is not to be simulated.
const NOT_PLAYED: u8 = 0x1e;

/// Line (0 = defence, 1 = midfield, 2 = attack) of tactics-board position `pos`.
///
/// DS:02a2 is directly followed by DS:02aa, so positions 8..=13 (board rows -1..=-6, where -1
/// means "not placed") read [`QUICK_GOAL_MAX`]; all of them land on lines 1 and 2.
fn position_line(pos: i16) -> usize {
    match pos {
        0..=7 => POSITION_LINE[pos as usize] as usize,
        8..=13 => usize::from(QUICK_GOAL_MAX[(pos - 8) as usize]),
        _ => panic!("starter at board position {pos}, outside the tactics board"),
    }
}

/// Recomputes the strength arrays of the club of human manager `squad` from the starting
/// eleven, and the manager's match aggression. `full == false` only averages the skills (no
/// RNG); `full == true` also applies the line balance, goalkeeper, role and position rules,
/// the difficulty level and the lineup slider.
///
/// RNG order with `full`: per starter on board positions >= 3, in squad order,
/// `random_range(2, 6)` when the role mismatch is > 2, then `random_range(2, 6)` when the
/// position mismatch is > 25; then three draws when no proper goalkeeper stands in goal; then
/// `random_range(7, 10)` when the defence is thin and `random_range(3, 5)` when that check
/// passes.
///
/// Origin: BMMAIN.EXE 1f9d:0002 (squad_recalc_team_strength)
pub fn squad_recalc_team_strength(
    state: &mut GameState,
    rt: &Runtime,
    rng: &mut Rng,
    squad: u8,
    full: bool,
) {
    let sq = usize::from(squad);
    let level = i16::from(state.level as i8);
    let slider = i16::from(state.managers[sq].value_131);
    let mut balance: [i16; 3] = [-9, -8, -5];
    let mut bonus = [0i16; 3];
    let mut count = [0i16; 3];
    let mut sum_a = [0i16; 3];
    let mut sum_b = [8i16; 3];
    let mut sum_c = [0i16; 3];
    // 1 = no goalkeeper in goal, 2 = a field player in goal, 0 = a goalkeeper in goal.
    let mut keeper: i16 = 1;
    let mut role_points: i16 = 0;
    let mut skill_diff: i16 = 0;
    let mut starters: i16 = 0;

    for i in 0..squad_count_listed(state, usize::from(squad)) {
        let p = &state.squads[sq][usize::from(i)];
        if p.lineup_position == 0 || p.lineup_position >= 12 {
            continue;
        }
        starters += 1;
        let pos = 7 - i16::from(p.board_row);
        let line = position_line(pos);
        let [a, b, c] = p.skills.map(i16::from);
        if full {
            let role = i16::from(p.board_col);
            if role == 1 || role == 5 {
                role_points += 1;
            }
            if role == 0 || role == 6 {
                role_points += 2;
            }
            if pos < 3 {
                balance[0] += 2;
                if pos == 1 && role == 3 && player_role_mismatch(state, squad, i) < 2 && b - 10 > a
                {
                    sum_b[line] += 15;
                }
                if pos == 0 && role == 3 {
                    let natural = state.players[usize::from(p.player_id)].position_value;
                    keeper = if natural / 25 != 0 { 2 } else { 0 };
                }
            } else {
                match pos {
                    3 => {
                        balance[0] += 1;
                        balance[1] += 1;
                        if role > 1
                            && role < 5
                            && player_role_mismatch(state, squad, i) < 2
                            && b - 10 > a
                        {
                            sum_b[line] += 15;
                        }
                    }
                    4 | 5 => balance[1] += 2,
                    6 => {
                        balance[1] += 1;
                        balance[2] += 1;
                    }
                    _ => balance[2] += 2,
                }
                if player_role_mismatch(state, squad, i) > 2 {
                    bonus[line] += rng.random_range(2, 6);
                }
                if player_position_mismatch(state, squad, i) > 25 {
                    bonus[line] += rng.random_range(2, 6);
                }
            }
            skill_diff += a - b;
            sum_a[line] += i16::from(p.appearances[0] / 6) - i16::from(p.exhaustion / 20) + 3;
            sum_b[line] += i16::from(p.goals[0]) - 1;
        }
        sum_a[line] += a;
        sum_b[line] += b;
        sum_c[line] += c;
        count[line] += 1;
    }

    for j in 0..3 {
        sum_b[j] += (level - 5).wrapping_mul(bonus[j]).wrapping_mul(20) / 100;
    }
    if starters == 0 {
        sum_b = [0; 3];
    }
    state.managers[sq].aggression = if starters < 8 { 100 } else { 0 };

    // Pad the line counts to eleven players, round robin from the defence.
    if starters != 0 {
        for k in 0..11 - starters {
            count[k as usize % 3] += 1;
        }
    }

    if full {
        if level == 0 && role_points > 8 {
            sum_b[2] = sum_b[2].wrapping_add((-100i16).wrapping_mul(count[2]));
        }
        if keeper != 0 {
            keeper -= 1;
            let scaled = |r: i16, sum: i16| ldiv(i32::from(r) * i32::from(sum), 100) as i16;
            sum_b[0] = scaled(rng.random_range(10 * keeper + 5, 20 * keeper + 5), sum_b[0]);
            sum_b[1] = scaled(rng.random_range(10, 40), sum_b[1]);
            sum_a[0] = scaled(
                rng.random_range(5 * (keeper + 2), 30 * keeper + 20),
                sum_a[0],
            );
        }
        clamp_int(&mut sum_b[0], 0, 32000);
        clamp_int(&mut sum_b[1], 0, 32000);
        clamp_int(&mut sum_a[0], 0, 32000);
        if balance[1] < 1 {
            if balance[0] > 0 {
                balance[2] += balance[1] - 1;
            }
            if balance[2] > 0 {
                balance[0] += balance[1] - 1;
            }
        }
        if balance[0] < -1 && rng.random_range(7, 10) > balance[0] + 10 {
            balance[1] -= rng.random_range(3, 5);
        }
        let changes = i16::from(rt.lineup_change_count[sq]);
        for j in 0..3 {
            sum_b[j] = sum_b[j].wrapping_add(level);
            if sum_a[j] != 0 {
                sum_a[j] = sum_a[j].wrapping_add(slider - 16);
            }
            if sum_b[j] != 0 && level < 4 {
                let t = balance[j].wrapping_mul(count[j]);
                sum_b[j] = sum_b[j].wrapping_add((changes * 3 + t).wrapping_mul(10));
                sum_a[j] = sum_a[j].wrapping_add(t.wrapping_mul(5));
            }
            sum_b[j] = sum_b[j].max(0);
            sum_a[j] = sum_a[j].max(0);
        }
        skill_diff /= 9;
        clamp_int(&mut skill_diff, -4, 4);
        skill_diff = skill_diff.max(0) + slider;
        clamp_int(&mut skill_diff, 0, 40);
        if state.managers[sq].aggression != 100 {
            state.managers[sq].aggression = skill_diff as u8;
        }
    }

    let team = usize::from(state.managers[sq].team);
    let club = &mut state.clubs[team];
    for j in 0..3 {
        if count[j] <= 1 {
            club.strength[j] = 0;
            club.base_strength[j] = 0;
            club.strength_1e[j] = 0;
        } else {
            club.strength[j] = (sum_a[j] / count[j]) as u8;
            club.base_strength[j] = (sum_b[j] / count[j]) as u8;
            club.strength_1e[j] = (sum_c[j] / count[j]) as u8;
        }
    }
}

/// Random walk of the strength arrays of the 64 German clubs. `mode` 1 is the weekly drift
/// (steps of -2..=+4, clamped to 1..=99, base strength moves only on a third of the draws);
/// any other mode is the new-game/new-season reset: crest bit 7 is cleared and the values are
/// clamped into the club's tier bounds. In that case only club 0 draws its step size with the
/// given mode; the mode is 3 for every later club and for club 0's own steps.
///
/// RNG calls per club: 1 (step size) + 3 + 3..=6 (base strength: always one draw per line,
/// plus one more when it is 0 or in reset mode) + 3.
///
/// Origin: BMMAIN.EXE 1f9d:0697 (league_strength_drift)
pub fn league_strength_drift(state: &mut GameState, rt: &Runtime, rng: &mut Rng, mode: u8) {
    let mut mode = mode;
    for t in 0..GERMAN_CLUBS as u8 {
        let tier = (rt.league_of_club(t) >> 1) as u8;
        let step = rng.random_range(0, i16::from(mode) * 2) as u8;
        let club = &mut state.clubs[usize::from(t)];
        if mode != 1 {
            club.crest &= 0x7f;
            mode = 3;
        }
        let reset = mode == 3;
        let k: i16 = if reset { 3 } else { 1 };
        let m = i16::from(mode);
        let spread = (k + i16::from(step)) * 2;
        let (lo, hi) = if reset {
            let [lo, hi] = LEAGUE_STRENGTH_BOUNDS[usize::from(tier)];
            (i16::from(lo), i16::from(hi))
        } else {
            (1, 99)
        };
        for v in &mut club.strength {
            let mut x = rng.random_range(0, spread) + i16::from(*v) - k - m;
            clamp_int(&mut x, lo, hi);
            *v = x as u8;
        }
        for v in &mut club.base_strength {
            let mut x = i16::from(*v);
            if rng.random_range(0, 2) == 0 || reset {
                x += rng.random_range(0, spread) - m - k;
            }
            clamp_int(&mut x, lo, hi);
            *v = x as u8;
        }
        for v in &mut club.strength_1e {
            let mut x = i16::from(*v) + rng.random_range(0, 6) - 3;
            clamp_int(&mut x, 0x2d, 0x37);
            *v = x as u8;
        }
    }
}

/// Byte index into the flattened [`GameState::league_results`] of the current round's cell
/// `slot` of league tier `comp`: row `comp * 38 + competition_rounds[comp] - 1` (the
/// original's table starts one row before save block 6, so the round counter is 1-based).
fn league_result_index(state: &GameState, comp: u8, slot: u8) -> usize {
    league_result_cell(
        comp,
        i16::from(state.competition_rounds[usize::from(comp)] as i8),
        slot,
    )
}

/// Upper bound of the goal draws of a quick result for strength difference `d >= 0`.
fn quick_goal_max(d: i16) -> i16 {
    if d > 19 {
        d / 5
    } else {
        i16::from(QUICK_GOAL_MAX[d as usize])
    }
}

/// Decides the goals of fixture slot `slot` (home) / `slot + 1` (away) of competition `comp`
/// (league tier 0..=2, or cup row + 10) between minutes `t0` and `t1` from the clubs' overall
/// strength at `t0`. A league fixture whose result cell holds 0x1e is skipped without any RNG
/// call. With `store` the goal minutes are generated and human managers' last scores are
/// updated; without it the result goes to [`Runtime::quick_sim_result`]. The result cells
/// themselves are left to the caller. The original also adds the goals to a statistics
/// counter (FAR:5788) that nothing in the game reads.
///
/// Origin: BMMAIN.EXE 1f9d:08e9 (match_simulate_quick_result)
#[allow(clippy::too_many_arguments)]
pub fn match_simulate_quick_result(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    comp: u8,
    slot: u8,
    home: u8,
    away: u8,
    t0: i16,
    t1: i16,
    store: bool,
) {
    let home_manager = state.manager_index_of_team(home, 0xff);
    let away_manager = state.manager_index_of_team(away, 0xff);
    // TODO(re): for cups (comp >= 10) the original reads FAR:6dc8 + (comp * 38 +
    // DS:225a[comp]) * 20 + slot here too, far outside every modelled table (inside the squad
    // block); assumed never to hold 0x1e.
    if comp < 10
        && state.league_results.as_flattened().as_flattened()
            [league_result_index(state, comp, slot)]
            == NOT_PLAYED
    {
        return;
    }
    let mut gh = rng.random_range(0, 1);
    let mut ga: i16 = 0;
    let sh = team_strength_value(state, home, 1, t0);
    let sa = team_strength_value(state, away, 1, t0);
    let d = (sa / -3).wrapping_add(sh / 3);
    if d > 0 {
        let max = quick_goal_max(d);
        let r1 = rng.random_range(1, max);
        let r2 = rng.random_range(1, max);
        if rng.random_range(0, 1) != 0 && d > 8 {
            gh += i16::from(goal_pick(rng, r1 as u8, r2 as u8));
        } else {
            gh = r1;
        }
        if d < 25 {
            ga = rng.random_range(0, 2);
        }
    } else {
        let e = -d;
        let max = quick_goal_max(e);
        if e < 25 {
            gh += rng.random_range(0, 2);
        }
        let r1 = rng.random_range(1, max);
        let r2 = rng.random_range(1, max);
        if rng.random_range(0, 2) == 0 && e > 8 {
            ga += i16::from(goal_pick(rng, r1 as u8, r2 as u8));
        } else {
            ga = r1;
        }
    }
    ga += rng.random_range(0, 2);
    gh += rng.random_range(0, 3);
    while gh > 8 || ga > 8 {
        gh -= 1;
        ga -= 1;
    }
    if gh > 4 && rng.random_range(0, 1) != 0 {
        gh -= 1;
    }
    if ga > 4 && rng.random_range(0, 1) != 0 {
        ga -= 1;
    }
    if (gh > 2 || ga > 2) && rng.random_range(0, 1) != 0 {
        gh -= 1;
        ga -= 1;
    }
    gh = gh.max(0);
    ga = ga.max(0);
    if sh == 0 {
        gh = 0;
        if sa > 20 {
            ga = 8;
        }
    }
    if sa == 0 {
        ga = 0;
        if sh > 20 {
            gh = 8;
        }
    }
    let span = t1.wrapping_sub(t0);
    gh = span.wrapping_mul(gh) / 45;
    ga = span.wrapping_mul(ga) / 45;
    if t0 > 90 {
        gh += 1;
        ga += 1;
    }
    let score = [gh as u8, ga as u8];
    if store {
        goal_minutes_generate(state, rt, rng, comp, slot, gh, t0, t1);
        goal_minutes_generate(state, rt, rng, comp, slot + 1, ga, t0, t1);
        // With 0xff as the second team, manager_index_of_team only returns 0 or 1..=4: no
        // manager's club is 0xff.
        for m in [home_manager, away_manager] {
            if m != 0 {
                rt.manager_last_score[usize::from(m) - 1] = score;
            }
        }
    } else {
        rt.quick_sim_result = score;
    }
}

/// What the caller of [`match_goal_chance`] has to do after the chance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalFollowUp {
    /// Nothing further.
    None,
    /// The fixture is shown live as live match `index`. The original continues with
    /// `2a58:0ca3(goal, comp, slot, home, away, side, minute as u8, kind, side, 0)` (goal and
    /// scorer handling), then `2a58:009d(DS:0632[index * 2], DS:063c[index * 2], index,
    /// comp > 9)`, then `1310:322c(result of 2a58:0ca3, ui_arg)`.
    Live {
        /// Index into [`Runtime::live_matches`].
        index: u8,
        /// 0 = league, 1 = cup row 0 (comp 10), 2 = later cup rows.
        kind: u8,
    },
    /// A goal in a watched match that is not shown live: the original flashes
    /// `"<home> - <away>   <home_goals>:<away_goals>"` (club names, DS:02be/02c2/02c6, numbers
    /// via `format_long_thousands`) in font 1 centred in the strip (0,0xe7)-(0x13f,0xef),
    /// holds it for 70 ticks, clears it and redraws the ticker with `1310:1def(minute as u8)`.
    Flash {
        /// Home goals after the goal, reduced below 10 (extra-time scores count from 10).
        home_goals: i16,
        /// Away goals after the goal.
        away_goals: i16,
    },
}

/// Outcome of [`match_goal_chance`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoalChance {
    /// A goal was scored; the result cell has already been incremented.
    pub goal: bool,
    /// The live-view or flash work the original does next.
    pub follow_up: GoalFollowUp,
}

/// Decides one goal chance of the attacking `side` (0 = home, 1 = away) of fixture `slot` of
/// competition `comp` in `minute`, from attack-minus-defence strength differences, and counts
/// a goal into the result cell. The original's UI work after the decision is returned as
/// [`GoalChance::follow_up`]; its last parameter (`ui_arg`) is only passed on to that work and
/// is therefore not taken here. The original's counters of chances and goals (FAR:6db0,
/// FAR:5496) are never read and are not kept.
///
/// RNG: none in extra time (`minute > 90`) while the home score is below 10 or when the
/// attacking side's score is 9 or >= 19; otherwise `random_range(0, 4)` (home) or
/// `random_range(0, 1)` (away) decides whether the chance happens, followed by one or two
/// draws for the goal.
///
/// Origin: BMMAIN.EXE 1f9d:0c3b (match_goal_chance)
#[allow(clippy::too_many_arguments)]
pub fn match_goal_chance(
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    comp: u8,
    slot: u8,
    home: u8,
    away: u8,
    side: u8,
    minute: i16,
) -> GoalChance {
    let home_defence = team_strength_value(state, home, 0, minute);
    let away_advantage = team_strength_value(state, away, 2, minute).wrapping_sub(home_defence) / 3;
    let away_defence = team_strength_value(state, away, 0, minute);
    let home_advantage = team_strength_value(state, home, 2, minute).wrapping_sub(away_defence) / 3;

    let cell = if comp < 10 {
        league_result_index(state, comp, slot)
    } else {
        cup_cell(comp - 10, slot)
    };
    let (mut gh, mut ga) = if comp < 10 {
        let r = state.league_results.as_flattened().as_flattened();
        (i16::from(r[cell] as i8), i16::from(r[cell + 1] as i8))
    } else {
        let r = state.cup_results.as_flattened();
        (i16::from(r[cell]), i16::from(r[cell + 1]))
    };
    if minute > 90 && gh < 10 {
        return GoalChance {
            goal: false,
            follow_up: GoalFollowUp::None,
        };
    }

    // The last live match whose home team is `home` wins.
    let mut live = None;
    for (k, m) in rt.live_matches.iter().enumerate() {
        let team = if comp < 10 {
            state.matchday_pairings.as_flattened().as_flattened()
                [usize::from(m.row) * 20 + usize::from(m.slot)]
        } else {
            state.cup_pairings.as_flattened()[cup_cell(m.row, m.slot)]
        };
        if team == home {
            live = Some(k as u8);
        }
    }

    let score = if side != 0 { ga } else { gh };
    let mut goal = false;
    // TODO(re): ai.md §6.6 — what result scores 9 and 10..=18 encode (extra time or
    // penalties) belongs to the match area.
    if score < 9 || (10..19).contains(&score) {
        let advantage = if side == 0 {
            (rng.random_range(0, 4) > 1).then_some(home_advantage)
        } else {
            (rng.random_range(0, 1) != 0).then_some(away_advantage)
        };
        if let Some(adv) = advantage {
            goal = if adv > 0 {
                rng.random_range(0, (adv >> 2) + 2) != 0
            } else {
                rng.random_range(0, adv.wrapping_neg() >> 2) == 0 || rng.random_range(0, 6) == 0
            };
        }
    }

    if goal {
        let scored = cell + usize::from(side);
        let byte = if comp < 10 {
            &mut state.league_results.as_flattened_mut().as_flattened_mut()[scored]
        } else {
            &mut state.cup_results.as_flattened_mut()[scored]
        };
        *byte = byte.wrapping_add(1);
    }

    let watched =
        (state.clubs[usize::from(home)].crest | state.clubs[usize::from(away)].crest) & 0x80 != 0;
    let follow_up = if let Some(index) = live {
        let kind = match comp {
            10 => 1,
            11.. => 2,
            _ => 0,
        };
        GoalFollowUp::Live { index, kind }
    } else if rt.live_match_shown != 0 && goal && watched {
        if side != 0 {
            ga += 1;
        } else {
            gh += 1;
        }
        while gh > 9 {
            gh -= 10;
        }
        GoalFollowUp::Flash {
            home_goals: gh,
            away_goals: ga,
        }
    } else {
        GoalFollowUp::None
    };
    GoalChance { goal, follow_up }
}

/// Sums of the starting eleven of the current manager (FAR:304a), for the training screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineupTotals {
    /// Sum of `skills[0]`.
    pub skill_0: i16,
    /// Sum of `skills[1]`.
    pub skill_1: i16,
    /// Sum of [`SquadPlayer::exhaustion`](crate::state::SquadPlayer::exhaustion).
    pub exhaustion: i16,
    /// `exhaustion` divided by the number of starters (1 when there are none).
    pub average_exhaustion: i16,
}

/// Totals two skills and the exhaustion of the current manager's starting eleven.
///
/// Origin: BMMAIN.EXE 1f9d:1122 (squad_lineup_totals)
pub fn squad_lineup_totals(state: &GameState) -> LineupTotals {
    let squad = u8::try_from(state.current_manager).expect("no current manager");
    let mut totals = LineupTotals::default();
    let mut starters: i16 = 0;
    for i in 0..squad_count_listed(state, usize::from(squad)) {
        let p = &state.squads[usize::from(squad)][usize::from(i)];
        if p.lineup_position == 0 || p.lineup_position >= 12 {
            continue;
        }
        starters += 1;
        totals.skill_0 = totals.skill_0.wrapping_add(i16::from(p.skills[0]));
        totals.skill_1 = totals.skill_1.wrapping_add(i16::from(p.skills[1]));
        totals.exhaustion = totals.exhaustion.wrapping_add(i16::from(p.exhaustion));
    }
    totals.average_exhaustion = totals.exhaustion / starters.max(1);
    totals
}

/// Spectators of a home match of manager `mgr`'s club `home` against `away`.
///
/// `kind` is the match importance from the caller (clamped to 0..=3), `fixed_cap` draws the
/// capacity from the home club's tier instead of the stadium, `pop_override` (when non-zero)
/// replaces the manager's popularity. The original's fifth parameter is never read and is
/// omitted.
///
/// The first estimate is computed with the x87 sequence of the original (see [`F80`]):
/// `((1 - (rank_a - 1) * 2 * 0.01) * str_a + (1 - (rank_h - 1) * 2 * 0.01) * str_h) * 10/43 *
/// 1150`, stored as a double, minus a draw, plus 1000, truncated by `_ftol`. The rest is long
/// arithmetic.
///
/// RNG calls in order: the estimate draw, `random_range(5q, 9q)`, `random_range(4q, 12q)`,
/// `random_range(400, 700)` when stadium item 0 + 2000 exceeds item 1, and the capacity draw
/// with `fixed_cap`. None when both strengths are 0 (the result is then 1).
///
/// Origin: BMMAIN.EXE 1f9d:11e0 (match_attendance_estimate)
#[allow(clippy::too_many_arguments)]
pub fn match_attendance_estimate(
    state: &GameState,
    rt: &Runtime,
    rng: &mut Rng,
    mgr: u8,
    home: u8,
    away: u8,
    kind: u8,
    fixed_cap: bool,
    pop_override: u8,
) -> i32 {
    let manager = &state.managers[usize::from(mgr)];
    let rank = |club: u8| -> i32 {
        if usize::from(club) < GERMAN_CLUBS {
            i32::from(state.team_tables[usize::from(club)].table_position)
        } else {
            5
        }
    };
    let strength = |club: u8| -> i32 {
        let overall = team_strength_value(state, club, 1, 0);
        let defence = team_strength_value(state, club, 0, 0);
        let attack = team_strength_value(state, club, 2, 0);
        let sum = attack.wrapping_add(overall.wrapping_add(defence));
        i32::from(sum.wrapping_mul(2) / 15)
    };
    let mut rank_h = rank(home);
    let rank_a = rank(away);
    let str_h = strength(home);
    let str_a = strength(away);
    let pop = if pop_override != 0 {
        i32::from(pop_override)
    } else {
        i32::from(manager.value_10a)
    };
    let tier = i32::from(manager.league);
    let divisor = 1i32 << (2 - tier);
    if divisor == 4 && rank_h > 14 {
        rank_h = 14;
    }
    if str_h == 0 && str_a == 0 {
        return 1;
    }

    let weight = |rank: i32, strength: i32| {
        let r = F80::from_i32(rank - 1) * F80::from_f64(2.0) * F80::from_f64(0.01);
        (F80::from_f64(1.0) - r) * F80::from_i32(strength)
    };
    let estimate = ((weight(rank_a, str_a) + weight(rank_h, str_h))
        * F80::from_f64(10.0 / 43.0)
        * F80::from_f64(1150.0))
    .to_f64();
    let pop16 = pop as i16;
    let r = rng.random_range(
        pop16.wrapping_sub(10).wrapping_mul(1050),
        pop16.wrapping_mul(1550).wrapping_sub(14400),
    );
    let mut att = (F80::from_f64(estimate) - F80::from_i16(r) + F80::from_f64(1000.0)).ftol();

    // TODO(re): ai.md §6.3 — stadium items 2..=6 and manager +0x1dc are not named yet.
    let item = |i: usize| manager.stadium[i][0];
    let q = ldiv(lmul(str_h + str_a, 100), 180);
    let q16 = q as i16;
    let r = rng.random_range(q16.wrapping_mul(5), q16.wrapping_mul(9));
    att = att.wrapping_add(lmul(i32::from(r), item(6)).wrapping_sub(lmul(q, 20)));
    let r = rng.random_range(q16.wrapping_mul(4), q16.wrapping_mul(12));
    att = att.wrapping_add(lmul(i32::from(r), item(5)).wrapping_sub(lmul(q, 31)));
    att = att.wrapping_add(lmul(q, lmul(item(3), 10).wrapping_sub(8)));
    att = att.wrapping_add(lmul(q, lmul(item(4), 10).wrapping_sub(5)));
    att = att.wrapping_add(lmul(q, ldiv(item(2), 1000).wrapping_sub(8)));
    let mood = manager.fans;
    if mood > 50 {
        att = att.wrapping_add(i32::from(30u16.wrapping_mul(mood).wrapping_sub(1500)));
    }
    att = att.wrapping_add(lmul(tier, -2000));
    if state.level != 0 {
        att = att.wrapping_add(1200);
    }
    if item(0).wrapping_add(2000) > item(1) {
        att = att.wrapping_add(i32::from(rng.random_range(400, 700)));
    }

    let tier_idx = tier as usize;
    let week = i32::from(state.competition_rounds[tier_idx] as i8);
    let weeks = i32::from(rt.tier_round_count[tier_idx]);
    let cap = if fixed_cap {
        let base = ATTENDANCE_TIER_BASE[usize::from(rt.tier_of_club(home))];
        let t = base as i16;
        let t3 = ldiv(base, 3) as i16;
        i32::from(rng.random_range(t.wrapping_sub(t3), t.wrapping_add(t3))) * 10
    } else {
        item(0).wrapping_add(item(1))
    };

    let mut kind = kind.min(3);
    if tier == 2 {
        kind = 0;
    }
    if str_a < 90 && kind > 1 {
        kind -= 1;
    }
    if pop < 22 {
        att = ldiv(lmul(i32::from(kind) + 2, att), 3);
    }
    if week + 5 > weeks && pop < 22 {
        if week == weeks {
            att = cap;
        } else {
            let lower = i32::from(tier != 0);
            if 7 - 2 * lower > rank_h {
                att = att.wrapping_add(ldiv(cap, 4));
                if rank_a < 7 {
                    att = att.wrapping_add(ldiv(cap, 5));
                }
            }
            if rank_h + lower + 5 > i32::from(rt.tier_team_count[tier_idx]) {
                att = att.wrapping_add(ldiv(cap, 4));
            }
        }
    }
    if pop > 10 {
        att = att.wrapping_add(lmul(10 - pop, 900));
    }
    let limit = lmul(divisor * 2 - i32::from(divisor == 4), 100_000);
    while lmul(att, pop) > limit {
        att = att.wrapping_sub(1000);
    }
    while att <= 500 {
        att += 500;
    }
    att.min(cap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::LiveMatch;
    use bmp_formats::savegame::SaveGame;

    const TEAM: u8 = 7;

    /// Advances a copy of `rng` by `n` calls of `rand()`.
    fn advanced(rng: Rng, n: usize) -> Rng {
        let mut r = rng;
        for _ in 0..n {
            r.rand();
        }
        r
    }

    /// Board row of the positions used below.
    const GOAL: i8 = 7;
    const DEFENCE: i8 = 6;
    const MIDFIELD: i8 = 3;
    const ATTACK: i8 = 0;

    /// Row code of `player_position_mismatch` for a board row.
    fn row_code(row: i8) -> u8 {
        ((7 - i16::from(row)) * 75 / 7 + 5) as u8
    }

    /// Puts player `i` of squad 0 into the lineup at `row`/`column` with the given skills. The
    /// player's foot and position code match the board spot, so no mismatch draw happens.
    fn starter(s: &mut GameState, i: usize, row: i8, column: i8, skills: [u8; 3]) {
        let id = i as u8 + 1;
        s.players[usize::from(id)].attr_20 = column as u8;
        s.players[usize::from(id)].position_value = row_code(row);
        let p = &mut s.squads[0][i];
        p.player_id = id;
        p.lineup_position = id;
        p.board_row = row;
        p.board_col = column;
        p.skills = skills;
    }

    fn squad_state() -> GameState {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.managers[0].team = TEAM;
        s.managers[0].value_131 = 16;
        s
    }

    /// 1 goalkeeper, 4 defenders, 4 midfielders, 2 forwards; skills `[50 + i, 60, 40 + i]`.
    fn eleven() -> GameState {
        let mut s = squad_state();
        let rows = [GOAL, DEFENCE, DEFENCE, DEFENCE, DEFENCE];
        let rows = rows.iter().chain(&[MIDFIELD; 4]).chain(&[ATTACK; 2]);
        for (i, &row) in rows.enumerate() {
            starter(&mut s, i, row, 3, [50 + i as u8, 60, 40 + i as u8]);
        }
        s
    }

    #[test]
    fn recalc_without_full_averages_the_lines() {
        let mut s = eleven();
        s.managers[0].aggression = 55;
        let mut rng = Rng::new(1);
        squad_recalc_team_strength(&mut s, &Runtime::default(), &mut rng, 0, false);
        let c = &s.clubs[usize::from(TEAM)];
        // Lines hold 5, 4 and 2 players. str_a: 260/5, 226/4, 119/2; str_b: (8 + 300)/5,
        // (8 + 240)/4, (8 + 120)/2; str_c: 210/5, 186/4, 99/2.
        assert_eq!(c.strength, [52, 56, 59]);
        assert_eq!(c.base_strength, [61, 62, 64]);
        assert_eq!(c.strength_1e, [42, 46, 49]);
        assert_eq!(s.managers[0].aggression, 0);
        assert_eq!(rng, Rng::new(1));
    }

    #[test]
    fn recalc_pads_short_lineups_round_robin() {
        let mut s = eleven();
        for p in &mut s.squads[0][7..11] {
            p.lineup_position = 0;
        }
        squad_recalc_team_strength(&mut s, &Runtime::default(), &mut Rng::new(1), 0, false);
        // 7 starters (5 + 2 + 0) are padded with 4 virtual players: counts 7, 3, 1. A line
        // with one player gets zeros. str_a 260/7, 111/3; str_b 308/7, 128/3; str_c 210/7,
        // 91/3.
        let c = &s.clubs[usize::from(TEAM)];
        assert_eq!(c.strength, [37, 37, 0]);
        assert_eq!(c.base_strength, [44, 42, 0]);
        assert_eq!(c.strength_1e, [30, 30, 0]);
        assert_eq!(s.managers[0].aggression, 100);

        let mut s = squad_state();
        s.clubs[usize::from(TEAM)].strength = [9; 3];
        let mut rng = Rng::new(1);
        squad_recalc_team_strength(&mut s, &Runtime::default(), &mut rng, 0, false);
        assert_eq!(s.clubs[usize::from(TEAM)].strength, [0; 3]);
        assert_eq!(s.managers[0].aggression, 100);
        assert_eq!(rng, Rng::new(1));
        // The full recalculation of an empty lineup still draws: three goalkeeper draws,
        // then the thin-defence pair.
        squad_recalc_team_strength(&mut s, &Runtime::default(), &mut rng, 0, true);
        assert_eq!(rng, advanced(Rng::new(1), 5));
    }

    /// The `full` fixture: level 5 (no balance terms), slider 16 (neutral), a proper
    /// goalkeeper, one defender on column 3 eligible for the +15 bonus, and one forward whose
    /// foot mismatches his column by 3 (one bonus draw).
    fn full_fixture() -> GameState {
        let mut s = squad_state();
        s.level = 5;
        let spots = [
            (GOAL, 3),
            (DEFENCE, 1),
            (DEFENCE, 2),
            (DEFENCE, 3),
            (DEFENCE, 4),
            (MIDFIELD, 1),
            (MIDFIELD, 2),
            (MIDFIELD, 3),
            (MIDFIELD, 4),
            (ATTACK, 2),
            (ATTACK, 5),
        ];
        for (i, &(row, column)) in spots.iter().enumerate() {
            starter(&mut s, i, row, column, [50, 50, 50]);
            let p = &mut s.squads[0][i];
            p.goals[0] = 1;
            p.appearances[0] = 12;
            p.exhaustion = 40;
        }
        s.squads[0][3].skills = [40, 55, 50];
        s.players[10].attr_20 = 5;
        s
    }

    #[test]
    fn recalc_full_with_a_goalkeeper() {
        let mut s = full_fixture();
        let mut rng = Rng::new(1);
        squad_recalc_team_strength(&mut s, &Runtime::default(), &mut rng, 0, true);
        // Per starter `appearances / 6 - exhaustion / 20 + 3 = 3` on str_a and `goals - 1 = 0`
        // on str_b; defender 3 adds 15 to str_b of the defence; level 5 adds 5 to every str_b.
        // str_a: (240 + 15)/5, (200 + 12)/4, (100 + 6)/2;
        // str_b: (8 + 255 + 15 + 5)/5, (8 + 200 + 5)/4, (8 + 100 + 5)/2.
        let c = &s.clubs[usize::from(TEAM)];
        assert_eq!(c.strength, [51, 53, 53]);
        assert_eq!(c.base_strength, [56, 53, 56]);
        assert_eq!(c.strength_1e, [50, 50, 50]);
        // skill difference -15 / 9 = -1 -> 0, plus slider 16.
        assert_eq!(s.managers[0].aggression, 16);
        // Only the forward's role-mismatch bonus draws.
        assert_eq!(rng, advanced(Rng::new(1), 1));
    }

    #[test]
    fn recalc_full_with_a_field_player_in_goal() {
        let mut s = full_fixture();
        s.players[1].position_value = 60;
        let mut rng = Rng::new(1);
        squad_recalc_team_strength(&mut s, &Runtime::default(), &mut rng, 0, true);
        // The bonus draw comes first, then the goalkeeper draws (15,25), (10,40), (15,50).
        let mut r = Rng::new(1);
        r.random_range(2, 6);
        let b0 = r.random_range(15, 25);
        let b1 = r.random_range(10, 40);
        let a0 = r.random_range(15, 50);
        assert_eq!(rng, r);
        let c = &s.clubs[usize::from(TEAM)];
        assert_eq!(c.strength[0], (a0 * 255 / 100 / 5) as u8);
        assert_eq!(c.base_strength[0], ((b0 * 278 / 100 + 5) / 5) as u8);
        assert_eq!(c.base_strength[1], ((b1 * 208 / 100 + 5) / 4) as u8);
    }

    #[test]
    fn recalc_full_at_level_zero_without_goalkeeper() {
        // Two defenders, five midfielders, four forwards; columns 0 and 6 give 2 role points,
        // 1 and 5 give 1: 9 points in total, so the attack loses 100 per player at level 0.
        let mut s = squad_state();
        s.managers[0].value_131 = 20;
        let spots = [
            (DEFENCE, 0),
            (DEFENCE, 6),
            (MIDFIELD, 0),
            (MIDFIELD, 6),
            (MIDFIELD, 1),
            (MIDFIELD, 2),
            (MIDFIELD, 3),
            (ATTACK, 2),
            (ATTACK, 3),
            (ATTACK, 3),
            (ATTACK, 4),
        ];
        for (i, &(row, column)) in spots.iter().enumerate() {
            starter(&mut s, i, row, column, [50, 50, 50]);
        }
        let mut rt = Runtime::default();
        rt.lineup_change_count[0] = 2;
        let mut rng = Rng::new(4);
        squad_recalc_team_strength(&mut s, &rt, &mut rng, 0, true);

        let mut r = Rng::new(4);
        assert_eq!(r.random_range(5, 5), 5);
        let b1 = r.random_range(10, 40);
        r.random_range(10, 20);
        // Balance [-5, 2, 3]: the defence is thin, and 7..=10 always beats -5 + 10.
        assert!(r.random_range(7, 10) > 5);
        let thin = r.random_range(3, 5);
        assert_eq!(rng, r);

        let c = &s.clubs[usize::from(TEAM)];
        // Defence: str_b 5 * 106 / 100 = 5, then (2 * 3 - 5 * 2) * 10 -> negative -> 0;
        // str_a (r * 106 / 100) + 4 - 50 -> negative -> 0.
        assert_eq!((c.strength[0], c.base_strength[0]), (0, 0));
        // Midfield: t = (2 - thin) * 5.
        let t = (2 - thin) * 5;
        assert_eq!(c.strength[1], ((265 + 4 + 5 * t) / 5) as u8);
        assert_eq!(
            c.base_strength[1],
            ((b1 * 253 / 100 + (6 + t) * 10) / 5) as u8
        );
        // Attack: str_b 204 - 400 + (6 + 12) * 10 = -16 -> 0; str_a (212 + 4 + 60) / 4.
        assert_eq!((c.strength[2], c.base_strength[2]), (69, 0));
        assert_eq!(s.managers[0].aggression, 20);
    }

    /// Level 3 and slider 20 with every board row in use: rows 7, 6, 6, 5 (defence), 4, 3, 3,
    /// 2 (midfield), 1, 1, 0 (attack) on columns 3, 2, 4, 1, 3, 2, 4, 5, 1, 5, 3. Starter `i`
    /// has skills `[60 + i, 50, 45]`, 2 goals, 17 appearances and exhaustion 59. Starter 8's
    /// position code is 0 (69 off its row), starter 10's foot is 0 (3 off its column). Record
    /// 11 is a substitute (lineup position 12) with skills 99.
    fn every_row_fixture() -> GameState {
        let mut s = squad_state();
        s.level = 3;
        s.managers[0].value_131 = 20;
        let spots = [
            (7, 3),
            (6, 2),
            (6, 4),
            (5, 1),
            (4, 3),
            (3, 2),
            (3, 4),
            (2, 5),
            (1, 1),
            (1, 5),
            (0, 3),
        ];
        for (i, &(row, column)) in spots.iter().enumerate() {
            starter(&mut s, i, row, column, [60 + i as u8, 50, 45]);
            let p = &mut s.squads[0][i];
            p.goals[0] = 2;
            p.appearances[0] = 17;
            p.exhaustion = 59;
        }
        s.players[9].position_value = 0;
        s.players[11].attr_20 = 0;
        starter(&mut s, 11, ATTACK, 3, [99; 3]);
        s.squads[0][11].lineup_position = 12;
        s
    }

    #[test]
    fn recalc_full_below_level_four_balances_the_lines() {
        let mut s = every_row_fixture();
        let mut rt = Runtime::default();
        rt.lineup_change_count[0] = 2;
        let mut rng = Rng::new(3);
        squad_recalc_team_strength(&mut s, &rt, &mut rng, 0, true);
        // Seed 3 (48, 7196): starter 8's position mismatch draws 5, starter 10's role
        // mismatch 3, both for the attack: (3 - 5) * 8 * 20 / 100 = -3 on its str_b. The
        // proper goalkeeper and a midfield balance of 1 draw nothing more.
        assert_eq!(rng, advanced(Rng::new(3), 2));
        // Per starter 17 / 6 - 59 / 20 + 3 = 3 on str_a and 2 - 1 = 1 on str_b. Lines of 4,
        // 4 and 3: str_a 246 + 12, 262 + 12, 207 + 9; str_b 8 + 200 + 4, 8 + 200 + 4,
        // 8 + 150 + 3 - 3. Balance [-9, -8, -5] + [8 + 1, 1 + 6 + 2, 2 + 2] = [0, 1, -1],
        // t = balance * count = [0, 4, -3]: str_b += 3 + (2 * 3 + t) * 10, str_a += 20 - 16 +
        // 5t. str_a 262, 298, 205; str_b 275, 315, 191.
        let c = &s.clubs[usize::from(TEAM)];
        assert_eq!(c.strength, [65, 74, 68]);
        assert_eq!(c.base_strength, [68, 78, 63]);
        assert_eq!(c.strength_1e, [45, 45, 45]);
        // Skill difference 110 + 55 = 165, / 9 = 18, clamped to 4, plus slider 20.
        assert_eq!(s.managers[0].aggression, 24);
    }

    #[test]
    fn recalc_full_rewards_a_technical_player_on_the_middle_row() {
        // Starter 4 (row 4) moves to column 4 with skills [40, 51]: 51 - 10 > 40 adds 15 to
        // the midfield str_b, 315 + 1 + 15 = 331; with 41 it is 316.
        for (a, base_strength) in [(40, 82), (41, 79)] {
            let mut s = every_row_fixture();
            starter(&mut s, 4, 4, 4, [a, 51, 45]);
            let mut rt = Runtime::default();
            rt.lineup_change_count[0] = 2;
            squad_recalc_team_strength(&mut s, &rt, &mut Rng::new(3), 0, true);
            assert_eq!(
                s.clubs[usize::from(TEAM)].base_strength[1],
                base_strength,
                "{a}"
            );
        }
    }

    #[test]
    fn recalc_full_with_eight_starters() {
        // The first eight starters of every_row_fixture: lines 4, 4, 0, padded to 5, 5, 1;
        // balance [0, -1, -5], no draws. Slider 38.
        let mut s = every_row_fixture();
        s.managers[0].value_131 = 38;
        for p in &mut s.squads[0][8..] {
            p.lineup_position = 0;
        }
        let mut rt = Runtime::default();
        rt.lineup_change_count[0] = 2;
        let mut rng = Rng::new(1);
        squad_recalc_team_strength(&mut s, &rt, &mut rng, 0, true);
        assert_eq!(rng, Rng::new(1));
        // t = [0, -5, -5]: str_a 258 + 22, 274 + 22 - 25; str_b 212 + 3 + 60, 212 + 3 + 10.
        let c = &s.clubs[usize::from(TEAM)];
        assert_eq!(c.strength, [56, 54, 0]);
        assert_eq!(c.base_strength, [55, 45, 0]);
        assert_eq!(c.strength_1e, [36, 36, 0]);
        // Eight starters set the aggression: 108 / 9 = 12 -> 4, + 38 = 42, clamped to 40.
        assert_eq!(s.managers[0].aggression, 40);

        // Seven starters: lines 4, 3, 0, padded to 6, 4, 1; balance [0, -3, -5], t = [0, -12,
        // -5]. str_a 280, 204 + 22 - 60; str_b 275, 161 + 3 - 60. The aggression stays 100.
        s.squads[0][7].lineup_position = 0;
        squad_recalc_team_strength(&mut s, &rt, &mut rng, 0, true);
        assert_eq!(rng, Rng::new(1));
        let c = &s.clubs[usize::from(TEAM)];
        assert_eq!(c.strength, [46, 41, 0]);
        assert_eq!(c.base_strength, [45, 26, 0]);
        assert_eq!(c.strength_1e, [30, 33, 0]);
        assert_eq!(s.managers[0].aggression, 100);
    }

    #[test]
    fn recalc_full_at_level_zero_counts_columns_one_and_five() {
        // Columns 0, 6, 0 give 6 role points, 1, 5, 5 one each: 9, so the attack loses 100 per
        // player. Two defenders, five midfielders, four forwards, no goalkeeper, ten lineup
        // changes.
        let mut s = squad_state();
        let spots = [
            (DEFENCE, 0),
            (DEFENCE, 6),
            (MIDFIELD, 0),
            (MIDFIELD, 1),
            (MIDFIELD, 5),
            (MIDFIELD, 5),
            (MIDFIELD, 3),
            (ATTACK, 3),
            (ATTACK, 3),
            (ATTACK, 3),
            (ATTACK, 3),
        ];
        for (i, &(row, column)) in spots.iter().enumerate() {
            starter(&mut s, i, row, column, [50, 50, 50]);
        }
        let mut rt = Runtime::default();
        rt.lineup_change_count[0] = 10;
        let mut rng = Rng::new(1);
        squad_recalc_team_strength(&mut s, &rt, &mut rng, 0, true);
        // Seed 1: goalkeeper draws 5, 32, 19, then 7 > -5 + 10 and 5 off the midfield
        // balance: five draws.
        assert_eq!(rng, advanced(Rng::new(1), 5));
        // Attack: str_b 8 + 200 - 4 - 400 = -196, t = 3 * 4: + (30 + 12) * 10 = 224; str_a
        // 200 + 12 + 60. Midfield: str_b 253 * 32 / 100 = 80, t = (2 - 5) * 5: + 150; str_a
        // 265 - 75. Defence: str_b 106 * 5 / 100 + (30 - 10) * 10; str_a 106 * 19 / 100 - 50.
        let c = &s.clubs[usize::from(TEAM)];
        assert_eq!(c.strength, [0, 38, 68]);
        assert_eq!(c.base_strength, [102, 46, 56]);
        assert_eq!(s.managers[0].aggression, 16);
    }

    #[test]
    fn drift_weekly_mode() {
        let mut s = GameState::zeroed();
        for c in &mut s.clubs[..GERMAN_CLUBS] {
            c.strength = [50, 1, 99];
            c.base_strength = [50; 3];
            c.strength_1e = [45, 55, 50];
            c.crest = 0x85;
        }
        s.clubs[64].strength = [7; 3];
        let mut rng = Rng::new(1);
        league_strength_drift(&mut s, &Runtime::default(), &mut rng, 1);

        let mut r = Rng::new(1);
        // Club 0: step size 41 % 3 = 2, so each step is random_range(0, 6) - 2; the first is
        // 18467 % 7 - 2 = -1.
        assert_eq!(r.random_range(0, 2), 2);
        let c = &s.clubs[0];
        let a: Vec<i16> = (0..3).map(|_| r.random_range(0, 6) - 2).collect();
        assert_eq!(a[0], -1);
        assert_eq!(c.strength[0] as i16, 50 + a[0]);
        assert_eq!(c.strength[1] as i16, (1 + a[1]).clamp(1, 99));
        assert_eq!(c.strength[2] as i16, (99 + a[2]).clamp(1, 99));
        for j in 0..3 {
            let mut v = 50;
            if r.random_range(0, 2) == 0 {
                v += r.random_range(0, 6) - 2;
            }
            assert_eq!(c.base_strength[j] as i16, v);
        }
        for (j, old) in [45, 55, 50].into_iter().enumerate() {
            let v: i16 = old + r.random_range(0, 6) - 3;
            assert_eq!(c.strength_1e[j] as i16, v.clamp(45, 55));
        }
        for c in &s.clubs[..GERMAN_CLUBS] {
            assert_eq!(c.crest, 0x85);
            assert!(c.strength.iter().all(|v| (1..=99).contains(v)));
        }
        assert_eq!(s.clubs[64].strength, [7; 3]);
        let draws = (640..=64 * 13).find(|&n| advanced(Rng::new(1), n) == rng);
        assert!(draws.is_some());
    }

    #[test]
    fn drift_reset_mode() {
        let mut s = GameState::zeroed();
        for c in &mut s.clubs[..65] {
            c.strength = [60; 3];
            c.base_strength = [60; 3];
            c.strength_1e = [50; 3];
            c.crest = 0x80;
        }
        let mut rng = Rng::new(1);
        league_strength_drift(&mut s, &Runtime::default(), &mut rng, 10);

        // Club 0 draws its step size with mode 10 (41 % 21 = 20) and then steps with mode 3.
        let mut r = Rng::new(1);
        assert_eq!(r.random_range(0, 20), 20);
        let v = r.random_range(0, 46) + 60 - 3 - 3;
        assert_eq!(s.clubs[0].strength[0] as i16, v.clamp(70, 93));
        // Club 0 used 1 + 3 + 6 + 3 draws; club 1 draws its step size with mode 3.
        let mut r = advanced(Rng::new(1), 13);
        let step = r.random_range(0, 6);
        let v = r.random_range(0, (3 + step) * 2) + 60 - 6;
        assert_eq!(s.clubs[1].strength[0] as i16, v.clamp(70, 93));
        assert_eq!(rng, advanced(Rng::new(1), 64 * 13));

        let bounds = |t: usize| match t {
            0..18 => 70..=93,
            18..38 => 40..=74,
            _ => 15..=45,
        };
        for (t, c) in s.clubs[..GERMAN_CLUBS].iter().enumerate() {
            assert_eq!(c.crest, 0);
            assert!(c.strength.iter().all(|v| bounds(t).contains(v)), "club {t}");
            assert!(c.base_strength.iter().all(|v| bounds(t).contains(v)));
            assert!(c.strength_1e.iter().all(|v| (45..=55).contains(v)));
        }
        assert_eq!(s.clubs[64].crest, 0x80);
        assert_eq!(s.clubs[64].strength, [60; 3]);
    }

    const HOME: u8 = 1;
    const AWAY: u8 = 2;

    /// Sets every line of `club` to `[strength, base, third]` with neutral morale.
    fn set_club(s: &mut GameState, club: u8, lines: [u8; 3]) {
        let c = &mut s.clubs[usize::from(club)];
        c.strength = [lines[0]; 3];
        c.base_strength = [lines[1]; 3];
        c.strength_1e = [lines[2]; 3];
        c.morale = 44;
    }

    /// Home overall/attack/defence 160, away 100 at minute 0; round 1 of tier 0.
    fn match_state() -> GameState {
        let mut s = GameState::zeroed();
        set_club(&mut s, HOME, [60, 50, 50]);
        set_club(&mut s, AWAY, [30, 40, 30]);
        s.competition_rounds = [1, 1, 1];
        s
    }

    #[test]
    fn quick_result_hand_traced() {
        // Strength 160 vs 100: d = 100 / -3 + 160 / 3 = 20, so goals are drawn up to 20 / 5.
        // Seed 1 (41, 18467, 6334, 26500, 19169, 15724, 11478, 29358, 26962): gh = 1, r1 = 4,
        // r2 = 3, no pick (26500 even) so gh = 4, ga = 2, ga = 3, gh = 6, no reductions;
        // scaled by 90 / 45.
        let mut s = match_state();
        let mut rt = Runtime::default();
        let mut rng = Rng::new(1);
        match_simulate_quick_result(&mut s, &mut rt, &mut rng, 0, 2, HOME, AWAY, 0, 90, false);
        assert_eq!(rt.quick_sim_result, [12, 6]);
        assert_eq!(rng, advanced(Rng::new(1), 9));

        // Seed 3 (48, 7196, 9294, 9091, 7031, 23577, 17702, 23503, 27217, 12168): gh = 0,
        // r1 = 1, r2 = 3, pick (9091 odd): goal_pick(1, 3) = 3, ga = 0, ga = 2, gh = 6,
        // gh > 4 and 27217 odd: gh = 5.
        let mut rng = Rng::new(3);
        match_simulate_quick_result(&mut s, &mut rt, &mut rng, 0, 2, HOME, AWAY, 0, 90, false);
        assert_eq!(rt.quick_sim_result, [10, 4]);
        assert_eq!(rng, advanced(Rng::new(3), 10));
    }

    /// Gives `club` the overall strength `overall` at every minute: lines of `overall / 4`
    /// (weighted 25/50/25) with no base strength, the rest through the morale.
    fn set_overall(s: &mut GameState, club: u8, overall: u8) {
        let c = &mut s.clubs[usize::from(club)];
        c.strength = [overall / 4 * 4; 3];
        c.base_strength = [0; 3];
        c.strength_1e = [0; 3];
        c.morale = 44 + overall % 4;
    }

    #[test]
    fn quick_result_branches_hand_traced() {
        // (seed, home overall, away overall, result, draws), all over 90 minutes (x 2).
        let cases = [
            // d = 160 / -3 + 100 / 3 = -20, the away branch with goals up to 4. Seed 1 (41,
            // 18467, 6334, 26500, 19169, 15724, 11478, 29358, 26962): gh = 1, gh = 3 (e < 25),
            // r1 = 3, r2 = 1, no pick (19169 % 3 = 2) so ga = r1 = 3, ga = 4, gh = 5, no
            // reductions: 5:4.
            (1, 100, 160, [10, 8], 9),
            // d = 126 / -3 + 99 / 3 = -9, the smallest difference that picks. Seed 5 (54,
            // 28693, 12255, 24449, 27660, 31430, 23927, 17649, 27472, 32640): gh = 0, gh = 1,
            // r1 = 1, r2 = 3, pick (27660 % 3 = 0): goal_pick(1, 3) = 3, ga = 5, gh = 2, ga > 4
            // but 27472 is even, no common reduction: 2:5.
            (5, 99, 126, [4, 10], 10),
            // d = 171 / -3 + 99 / 3 = -24, the largest difference with a home draw. Seed 0
            // (38, 7719, 21238, 2437, 8855, 11797, 8365, 32285): gh = 0, gh = 0, r1 = 3, r2 =
            // 2, no pick (8855 % 3 = 2) so ga = 3, ga = 4, gh = 1, ga > 2 and 32285 odd: 0:3.
            (0, 99, 171, [0, 6], 8),
            // d = 60 / -3 + 255 / 3 = 65, goals up to 13. Seed 0 (38, 7719, 21238, 2437, 8855,
            // 11797, 8365, 32285, 10450): gh = 0, r1 = 11, r2 = 10, pick: goal_pick(11, 10) =
            // 11, no away draw (d >= 25), ga = 1, gh = 12; four common steps down to 8:-3, gh >
            // 4 and 32285 odd: 7, 10450 even: 7:0.
            (0, 255, 60, [14, 0], 9),
        ];
        for (seed, home, away, result, draws) in cases {
            let mut s = match_state();
            set_overall(&mut s, HOME, home);
            set_overall(&mut s, AWAY, away);
            let mut rt = Runtime::default();
            let mut rng = Rng::new(seed);
            match_simulate_quick_result(&mut s, &mut rt, &mut rng, 0, 2, HOME, AWAY, 0, 90, false);
            assert_eq!(rt.quick_sim_result, result, "seed {seed}, {home} vs {away}");
            assert_eq!(rng, advanced(Rng::new(seed), draws), "seed {seed}");
        }
    }

    #[test]
    fn quick_result_skips_unplayable_fixtures() {
        let mut s = match_state();
        s.competition_rounds[1] = 5;
        s.league_results[1][4][6] = NOT_PLAYED;
        let mut rt = Runtime::default();
        let mut rng = Rng::new(1);
        match_simulate_quick_result(&mut s, &mut rt, &mut rng, 1, 6, HOME, AWAY, 0, 90, true);
        assert_eq!(rng, Rng::new(1));
        assert_eq!(rt, Runtime::default());
    }

    #[test]
    fn quick_result_zero_strength_and_time_scaling() {
        let mut s = GameState::zeroed();
        s.competition_rounds = [1, 1, 1];
        // Home strength 0 (all zero, morale 44); away 60 at any minute (base 0 decays to 0).
        set_club(&mut s, HOME, [0, 0, 0]);
        set_club(&mut s, AWAY, [30, 0, 30]);
        let mut rt = Runtime::default();
        let run = |s: &mut GameState, rt: &mut Runtime, t0, t1| {
            match_simulate_quick_result(s, rt, &mut Rng::new(5), 0, 0, HOME, AWAY, t0, t1, false);
            rt.quick_sim_result
        };
        // 0 vs > 20 forces 0:8; 8 * 90 / 45 = 16.
        assert_eq!(run(&mut s, &mut rt, 0, 90), [0, 16]);
        // 8 * 30 / 45 = 5 (truncated), no extra-time goal at t0 = 90.
        assert_eq!(run(&mut s, &mut rt, 90, 120), [0, 5]);
        // t0 > 90 adds one goal each: 8 * 29 / 45 = 5, + 1.
        assert_eq!(run(&mut s, &mut rt, 91, 120), [1, 6]);
        // Both strengths 0: 0:0, plus the extra-time goals.
        set_club(&mut s, AWAY, [0, 0, 0]);
        assert_eq!(run(&mut s, &mut rt, 105, 120), [1, 1]);
    }

    #[test]
    fn quick_result_store_writes_minutes_and_manager_scores() {
        let mut s = GameState::zeroed();
        s.competition_rounds = [1, 1, 1];
        s.manager_count = 2;
        s.managers[0].team = AWAY;
        s.managers[1].team = HOME;
        set_club(&mut s, HOME, [0, 0, 0]);
        set_club(&mut s, AWAY, [30, 0, 30]);
        let mut rt = Runtime::default();
        rt.league_goal_minutes[2][4] = [77; 8];
        let mut rng = Rng::new(5);
        match_simulate_quick_result(&mut s, &mut rt, &mut rng, 2, 4, HOME, AWAY, 0, 90, true);
        // Both clubs' managers get the score; managers 3 and 4 are not playing.
        assert_eq!(rt.manager_last_score, [[0, 16], [0, 16], [0, 0], [0, 0]]);
        assert_eq!(rt.quick_sim_result, [0, 0]);
        assert_eq!(rt.league_goal_minutes[2][4], [0; 8]);
        let away = rt.league_goal_minutes[2][5];
        let mut sorted = away;
        sorted.sort();
        assert!(
            sorted.windows(2).all(|w| w[0] != w[1] || w[0] == 0),
            "{away:?}"
        );
        assert!(away.iter().filter(|&&m| m != 0).count() >= 7);
    }

    #[test]
    fn goal_chance_in_extra_time_needs_an_extra_time_score() {
        let mut s = match_state();
        let mut rt = Runtime::default();
        let mut rng = Rng::new(0);
        let r = match_goal_chance(&mut s, &mut rt, &mut rng, 0, 0, HOME, AWAY, 0, 91);
        assert!(!r.goal);
        assert_eq!(rng, Rng::new(0));
    }

    #[test]
    fn goal_chance_blocked_scores_draw_nothing() {
        let mut s = match_state();
        s.league_results[0][0][0] = 9;
        let mut rt = Runtime::default();
        let mut rng = Rng::new(0);
        let r = match_goal_chance(&mut s, &mut rt, &mut rng, 0, 0, HOME, AWAY, 0, 50);
        assert!(!r.goal);
        assert_eq!(rng, Rng::new(0));
        s.league_results[0][0][0] = 19;
        let r = match_goal_chance(&mut s, &mut rt, &mut rng, 0, 0, HOME, AWAY, 0, 50);
        assert!(!r.goal);
        assert_eq!(rng, Rng::new(0));
    }

    #[test]
    fn goal_chance_draws_only_for_open_scores() {
        // Home advantage 20 and seed 0 (38 % 5 = 3, 7719 % 8 = 7): a goal after two draws
        // whenever the home score lets the chance happen.
        for (score, draws) in [(8, 2), (9, 0), (10, 2), (18, 2), (19, 0)] {
            let mut s = match_state();
            s.league_results[0][0][0] = score;
            let mut rng = Rng::new(0);
            let r = match_goal_chance(
                &mut s,
                &mut Runtime::default(),
                &mut rng,
                0,
                0,
                HOME,
                AWAY,
                0,
                50,
            );
            assert_eq!(r.goal, draws != 0, "score {score}");
            assert_eq!(rng, advanced(Rng::new(0), draws), "score {score}");
            assert_eq!(s.league_results[0][0][0], score + u8::from(r.goal));
        }
    }

    #[test]
    fn goal_chance_home_chance_starts_at_two() {
        // Seed 12: 77 % 5 = 2 is a chance, then 5628 % 8 = 4: goal.
        let mut s = match_state();
        let mut rng = Rng::new(12);
        let r = match_goal_chance(
            &mut s,
            &mut Runtime::default(),
            &mut rng,
            0,
            0,
            HOME,
            AWAY,
            0,
            50,
        );
        assert!(r.goal);
        assert_eq!(rng, advanced(Rng::new(12), 2));
    }

    #[test]
    fn goal_chance_weaker_side_scores_on_one_in_seven() {
        // Away advantage -20. Seed 18: 97 is odd (chance), 4582 % 6 = 4 misses, 15113 % 7 = 0
        // scores.
        let mut s = match_state();
        let mut rng = Rng::new(18);
        let r = match_goal_chance(
            &mut s,
            &mut Runtime::default(),
            &mut rng,
            0,
            0,
            HOME,
            AWAY,
            1,
            50,
        );
        assert!(r.goal);
        assert_eq!(rng, advanced(Rng::new(18), 3));
        assert_eq!(&s.league_results[0][0][0..2], &[0, 1]);
    }

    #[test]
    fn goal_chance_home_side() {
        // Home advantage (160 - 100) / 3 = 20.
        let mut s = match_state();
        let mut rt = Runtime::default();
        // Seed 1: 41 % 5 = 1, no chance after one draw.
        let mut rng = Rng::new(1);
        let r = match_goal_chance(&mut s, &mut rt, &mut rng, 0, 2, HOME, AWAY, 0, 0);
        assert_eq!(
            r,
            GoalChance {
                goal: false,
                follow_up: GoalFollowUp::None
            }
        );
        assert_eq!(rng, advanced(Rng::new(1), 1));
        // Seed 0: 38 % 5 = 3, then 7719 % (20 / 4 + 3) = 7: goal.
        let mut rng = Rng::new(0);
        let r = match_goal_chance(&mut s, &mut rt, &mut rng, 0, 2, HOME, AWAY, 0, 0);
        assert!(r.goal);
        assert_eq!(rng, advanced(Rng::new(0), 2));
        assert_eq!(&s.league_results[0][0][2..4], &[1, 0]);
    }

    #[test]
    fn goal_chance_away_side() {
        // Away advantage (100 - 160) / 3 = -20: seed 1 passes the chance (41 odd), then
        // 18467 % 6 = 5 and 6334 % 7 = 6: no goal after three draws.
        let mut s = match_state();
        let mut rt = Runtime::default();
        let mut rng = Rng::new(1);
        let r = match_goal_chance(&mut s, &mut rt, &mut rng, 0, 2, HOME, AWAY, 1, 30);
        assert!(!r.goal);
        assert_eq!(rng, advanced(Rng::new(1), 3));
        // A strong away team in cup row 1 (comp 11): advantage 20, 18467 % 8 = 3: goal,
        // counted in the cup result cell 32 + 4 + 1.
        let mut rng = Rng::new(1);
        let r = match_goal_chance(&mut s, &mut rt, &mut rng, 11, 4, AWAY, HOME, 1, 30);
        assert!(r.goal);
        assert_eq!(rng, advanced(Rng::new(1), 2));
        assert_eq!(s.cup_results[18], [0, 1]);
    }

    #[test]
    fn goal_chance_live_matches_last_one_wins() {
        let mut s = match_state();
        let mut rt = Runtime::default();
        let flat = s.matchday_pairings.as_flattened_mut().as_flattened_mut();
        flat[0] = HOME;
        flat[22] = AWAY;
        flat[4] = HOME;
        rt.live_matches = vec![
            LiveMatch { row: 0, slot: 0 },
            LiveMatch { row: 1, slot: 2 },
            LiveMatch { row: 0, slot: 4 },
        ];
        let r = match_goal_chance(&mut s, &mut rt, &mut Rng::new(1), 0, 0, HOME, AWAY, 0, 10);
        assert_eq!(r.follow_up, GoalFollowUp::Live { index: 2, kind: 0 });
        let r = match_goal_chance(&mut s, &mut rt, &mut Rng::new(1), 0, 0, AWAY, HOME, 0, 10);
        assert_eq!(r.follow_up, GoalFollowUp::Live { index: 1, kind: 0 });
        // Cups look the home team up in the cup pairings; comp 10 is kind 1, later rows 2.
        s.cup_pairings.as_flattened_mut()[32 + 2] = HOME;
        let r = match_goal_chance(&mut s, &mut rt, &mut Rng::new(1), 10, 0, HOME, AWAY, 0, 10);
        assert_eq!(r.follow_up, GoalFollowUp::Live { index: 1, kind: 1 });
        let r = match_goal_chance(&mut s, &mut rt, &mut Rng::new(1), 12, 0, HOME, AWAY, 0, 10);
        assert_eq!(r.follow_up, GoalFollowUp::Live { index: 1, kind: 2 });
    }

    #[test]
    fn goal_chance_flash_for_watched_clubs() {
        // Extra-time score 12:3, away (strong) scores: the flash shows 2:4.
        let mut s = match_state();
        set_club(&mut s, HOME, [30, 40, 30]);
        set_club(&mut s, AWAY, [60, 50, 50]);
        s.league_results[0][0][0..2].copy_from_slice(&[12, 3]);
        s.clubs[usize::from(HOME)].crest = 0x80;
        let mut rt = Runtime::default();
        let r = match_goal_chance(&mut s, &mut rt, &mut Rng::new(1), 0, 0, HOME, AWAY, 1, 100);
        assert_eq!(
            r,
            GoalChance {
                goal: true,
                follow_up: GoalFollowUp::None
            }
        );
        rt.live_match_shown = 1;
        let r = match_goal_chance(&mut s, &mut rt, &mut Rng::new(1), 0, 0, HOME, AWAY, 1, 100);
        assert_eq!(
            r.follow_up,
            GoalFollowUp::Flash {
                home_goals: 2,
                away_goals: 5
            }
        );
        assert_eq!(&s.league_results[0][0][0..2], &[12, 5]);
    }

    #[test]
    fn goal_chance_flash_wraps_a_home_score_of_ten() {
        let mut s = match_state();
        set_club(&mut s, HOME, [30, 40, 30]);
        set_club(&mut s, AWAY, [60, 50, 50]);
        s.league_results[0][0][0] = 10;
        s.clubs[usize::from(AWAY)].crest = 0x80;
        let mut rt = Runtime {
            live_match_shown: 1,
            ..Runtime::default()
        };
        let r = match_goal_chance(&mut s, &mut rt, &mut Rng::new(1), 0, 0, HOME, AWAY, 1, 50);
        assert_eq!(
            r.follow_up,
            GoalFollowUp::Flash {
                home_goals: 0,
                away_goals: 1
            }
        );
    }

    #[test]
    fn lineup_totals_of_the_current_manager() {
        let mut s = GameState::zeroed();
        s.current_manager = 1;
        let sq = &mut s.squads[1];
        for (i, p) in sq[..4].iter_mut().enumerate() {
            p.player_id = i as u8 + 1;
            p.lineup_position = if i < 3 { i as u8 + 1 } else { 12 };
            p.skills = [10 * (i as u8 + 1), 5, 0];
            p.exhaustion = 100 + i as u8;
        }
        // Record 24 is outside the scanned records; record 6 lies past the first empty
        // record, beyond the counted number of occupied records.
        sq[24].player_id = 9;
        sq[24].lineup_position = 4;
        sq[6].player_id = 8;
        sq[6].lineup_position = 5;
        let t = squad_lineup_totals(&s);
        assert_eq!(
            t,
            LineupTotals {
                skill_0: 60,
                skill_1: 15,
                exhaustion: 303,
                average_exhaustion: 101
            }
        );
        s.current_manager = 2;
        assert_eq!(squad_lineup_totals(&s), LineupTotals::default());
    }

    /// Home club 3 (table position 9, strength 7), away club 4 (table position 1,
    /// strength 1); manager 0 with popularity 10, tier 0, stadium items 0/1 = 0/5000.
    fn attendance_state() -> GameState {
        let mut s = GameState::zeroed();
        // v = 4 per line: strength values 3 + 4 + 3 = 10, 10 * 2 / 15 = 1.
        set_club(&mut s, 4, [2, 1, 1]);
        // v = 20, morale 42: 18 per kind, 54 * 2 / 15 = 7.
        set_club(&mut s, 3, [10, 5, 5]);
        s.clubs[3].morale = 42;
        s.team_tables[3].table_position = 9;
        s.team_tables[4].table_position = 1;
        s.competition_rounds = [10, 10, 10];
        let m = &mut s.managers[0];
        m.value_10a = 10;
        m.stadium[1][0] = 5000;
        s
    }

    #[test]
    fn attendance_hand_computed_with_extended_precision() {
        // (1 - 0 * 0.02) * 1 + (1 - 8 * 0.02) * 7 = 6.88; * 10/43 * 1150 = 1840 exactly in
        // the 80-bit register (an f64 evaluation gives 1839.9999999999998 and ends one lower).
        // q = 800 / 180 = 4; the long terms add -80 - 124 - 32 - 20 - 32 = -288.
        let s = attendance_state();
        let rt = Runtime::default();
        let mut rng = Rng::new(2);
        let att = match_attendance_estimate(&s, &rt, &mut rng, 0, 3, 4, 1, false, 0);
        let mut r = Rng::new(2);
        let first = r.random_range(0, 1100);
        r.random_range(20, 36);
        r.random_range(16, 48);
        assert_eq!(rng, r);
        assert_eq!(att, 2840 - i32::from(first) - 288);
    }

    /// One case of [`attendance_tiers_ranks_and_caps`].
    struct AttendanceCase {
        name: &'static str,
        setup: fn(&mut GameState, &mut Runtime),
        home: u8,
        kind: u8,
        fixed_cap: bool,
        pop_override: u8,
        expected: i32,
        draws: usize,
    }

    /// The fixture of [`attendance_state`] in manager 0's league `tier` at round `week`.
    fn in_league(s: &mut GameState, tier: u8, week: u8) {
        s.managers[0].league = tier;
        s.competition_rounds[usize::from(tier)] = week;
    }

    /// Moves the home club of [`attendance_state`] (club 3) to club index `club`.
    fn home_at(s: &mut GameState, club: u8) {
        s.clubs[usize::from(club)] = s.clubs[3].clone();
        s.team_tables[usize::from(club)].table_position = 9;
    }

    #[test]
    fn attendance_tiers_ranks_and_caps() {
        // Every case starts from attendance_hand_computed_with_extended_precision: seed 2
        // draws 45 first, the estimate is 1840 + 1000 - 45, the long terms add -288, and the
        // capacity is 5000. A rank change moves the estimate by 10/43 * 1150 = 267.44 per
        // weighted strength point.
        let base = |home, setup| AttendanceCase {
            name: "",
            setup,
            home,
            kind: 1,
            fixed_cap: false,
            pop_override: 0,
            expected: 0,
            draws: 3,
        };
        let cases = [
            AttendanceCase {
                // Rank 4: (1 - 0.06) * 7 + 1 = 7.58 -> 2027.2 -> 2694; -2000 for the tier;
                // in the last five rounds a rank below 7 - 2 adds 5000 / 4, an away rank
                // below 7 another 5000 / 5.
                name: "2. Bundesliga top club near the end",
                expected: 2694 - 2000 + 1250 + 1000,
                ..base(3, |s, _| {
                    in_league(s, 1, 34);
                    s.team_tables[3].table_position = 4;
                })
            },
            AttendanceCase {
                // Rank 5: 7.44 -> 1989.8 -> 2656; rank 5 is not below 7 - 2.
                name: "2. Bundesliga rank 5 near the end",
                expected: 2656 - 2000,
                ..base(3, |s, _| {
                    in_league(s, 1, 34);
                    s.team_tables[3].table_position = 5;
                })
            },
            AttendanceCase {
                // Rank 14: 5.18 + 1 -> 1652.8 -> 2319 - 2000, lifted to 819; 14 + 1 + 5 is
                // not above the 20 clubs of the tier.
                name: "2. Bundesliga rank 14 near the end",
                expected: 819,
                ..base(3, |s, _| {
                    in_league(s, 1, 34);
                    s.team_tables[3].table_position = 14;
                })
            },
            AttendanceCase {
                // Rank 15: 5.04 + 1 -> 1615.3 -> 2282 - 2000; the relegation zone adds
                // 5000 / 4.
                name: "2. Bundesliga rank 15 near the end",
                expected: 282 + 1250,
                ..base(3, |s, _| {
                    in_league(s, 1, 34);
                    s.team_tables[3].table_position = 15;
                })
            },
            AttendanceCase {
                // Rank 6 and away rank 7: 0.88 + 6.3 = 7.18 -> 1920.2 -> 2587; round 30 is in
                // the last five of 34, rank 6 is below 7, away rank 7 is not.
                name: "Bundesliga rank 6 against rank 7 near the end",
                expected: 2587 + 1250,
                ..base(3, |s, _| {
                    in_league(s, 0, 30);
                    s.team_tables[3].table_position = 6;
                    s.team_tables[4].table_position = 7;
                })
            },
            AttendanceCase {
                name: "last round fills the stadium",
                expected: 5000,
                ..base(3, |s, _| in_league(s, 0, 34))
            },
            AttendanceCase {
                // Tier 2: -4000, and the match importance drops to 0 (factor 2/3 instead of
                // the 4/3 of kind 3 -> 2): ldiv(-2986, 3) = -995, lifted by 500 to 505.
                name: "lower tier ignores the importance",
                kind: 3,
                expected: 505,
                ..base(3, |s, _| in_league(s, 2, 10))
            },
            AttendanceCase {
                // Bundesliga ranks above 14 count as 14: 5.18 + 1 -> 1652.8 -> 2319.
                name: "Bundesliga rank 16",
                expected: 2319,
                ..base(3, |s, _| s.team_tables[3].table_position = 16)
            },
            AttendanceCase {
                // Level 1: +1200; mood 51: 30 * 51 - 1500 = 30; item 2 = 5000: q * (5 - 8)
                // instead of q * -8, +20.
                name: "difficulty, mood and stadium item 2",
                expected: 2507 + 1200 + 30 + 20,
                ..base(3, |s, _| {
                    s.level = 1;
                    s.managers[0].fans = 51;
                    s.managers[0].stadium[2][0] = 5000;
                })
            },
            AttendanceCase {
                // Popularity 11 (override): first draw random_range(1050, 2650) = 1095, so
                // 1840 + 1000 - 1095 - 288; above 10 it costs 900.
                name: "popularity 11",
                pop_override: 11,
                expected: 1457 - 900,
                ..base(3, |_, _| {})
            },
            AttendanceCase {
                // Popularity 22: first draw random_range(12600, 19700) = 12645, so -10093;
                // no importance scaling, -12 * 900: -20893, lifted 43 times by 500.
                name: "popularity 22",
                kind: 3,
                expected: 607,
                ..base(3, |s, _| s.managers[0].value_10a = 22)
            },
            AttendanceCase {
                // The same popularity in the last round: no full stadium.
                name: "popularity 22 in the last round",
                expected: 607,
                ..base(3, |s, _| {
                    in_league(s, 0, 34);
                    s.managers[0].value_10a = 22;
                })
            },
            AttendanceCase {
                // Item 3 = 2000: +80000 over the base, 82507; the Bundesliga limit is
                // (8 - 1) * 100000, so att * 10 ends at 69507.
                name: "Bundesliga limit",
                expected: 69507,
                ..base(3, |s, _| {
                    s.managers[0].stadium[1][0] = 100_000;
                    s.managers[0].stadium[3][0] = 2000;
                })
            },
            AttendanceCase {
                // Item 3 = 1000: q * (10000 - 8) instead of q * -8, +40000; tier 2: -4000 and
                // factor 2/3: 25671; att * 10 may not exceed 2 * 100000, so 19671.
                name: "lower tier limit",
                expected: 19671,
                ..base(3, |s, _| {
                    in_league(s, 2, 10);
                    s.managers[0].stadium[1][0] = 100_000;
                    s.managers[0].stadium[3][0] = 1000;
                })
            },
            AttendanceCase {
                // Fixed caps in the last round: 4th draw (17795) in base +- base / 3, x 10.
                // Club 20: 17795 % 1467 + 1467 = 1658.
                name: "fixed cap of a 2. Bundesliga club",
                fixed_cap: true,
                expected: 16580,
                draws: 4,
                ..base(20, |s, _| {
                    home_at(s, 20);
                    in_league(s, 0, 34);
                })
            },
            AttendanceCase {
                // Club 40: 17795 % 801 + 800 = 973.
                name: "fixed cap of a lower-tier club",
                fixed_cap: true,
                expected: 9730,
                draws: 4,
                ..base(40, |s, _| {
                    home_at(s, 40);
                    in_league(s, 0, 34);
                })
            },
            AttendanceCase {
                // Club 60: 17795 % 401 + 400 = 551.
                name: "fixed cap of a club outside the leagues",
                fixed_cap: true,
                expected: 5510,
                draws: 4,
                ..base(60, |s, _| {
                    home_at(s, 60);
                    in_league(s, 0, 34);
                })
            },
            AttendanceCase {
                // No range matches: base 0x6362_6100, t = 0x6100 = 24832, t3 = ldiv(base, 3)
                // = 0x2120_cb00 -> -13568 as i16: random_range(-27136, 11264) = -9341.
                name: "fixed cap past the tier table",
                fixed_cap: true,
                expected: -93410,
                draws: 4,
                ..base(3, |s, rt| {
                    rt.tier_club_ranges = [[0, 0]; 4];
                    in_league(s, 0, 34);
                })
            },
        ];
        for case in cases {
            let mut s = attendance_state();
            let mut rt = Runtime::default();
            (case.setup)(&mut s, &mut rt);
            let mut rng = Rng::new(2);
            let att = match_attendance_estimate(
                &s,
                &rt,
                &mut rng,
                0,
                case.home,
                4,
                case.kind,
                case.fixed_cap,
                case.pop_override,
            );
            assert_eq!(att, case.expected, "{}", case.name);
            assert_eq!(rng, advanced(Rng::new(2), case.draws), "{}", case.name);
        }
    }

    #[test]
    fn attendance_rng_calls_and_capacity() {
        let s = attendance_state();
        let rt = Runtime::default();
        let mut base = Rng::new(2);
        let a = match_attendance_estimate(&s, &rt, &mut base, 0, 3, 4, 1, false, 0);
        assert_eq!(base, advanced(Rng::new(2), 3));

        // Stadium item 0 + 2000 above item 1 adds random_range(400, 700) as the 4th draw.
        let mut s4 = attendance_state();
        s4.managers[0].stadium[0][0] = 3001;
        let mut rng = Rng::new(2);
        let b = match_attendance_estimate(&s4, &rt, &mut rng, 0, 3, 4, 1, false, 0);
        let mut r = advanced(Rng::new(2), 3);
        assert_eq!(b, a + i32::from(r.random_range(400, 700)));
        assert_eq!(rng, r);

        // A fixed cap draws the capacity last: club 3 is in tier range 0 (4500 +- 1500).
        let mut rng = Rng::new(2);
        let c = match_attendance_estimate(&s4, &rt, &mut rng, 0, 3, 4, 1, true, 0);
        let cap = i32::from(r.random_range(3000, 6000)) * 10;
        assert_eq!(rng, r);
        assert_eq!(c, b.min(cap));

        // The stadium capacity caps the result.
        let mut small = attendance_state();
        small.managers[0].stadium[1][0] = 1000;
        let d = match_attendance_estimate(&small, &rt, &mut Rng::new(2), 0, 3, 4, 1, false, 0);
        assert_eq!(d, 1000);
    }

    #[test]
    fn attendance_kind_and_minimum() {
        let s = attendance_state();
        let rt = Runtime::default();
        let a = match_attendance_estimate(&s, &rt, &mut Rng::new(2), 0, 3, 4, 1, false, 0);
        // Popularity below 22 scales by (kind + 2) / 3; kind 5 clamps to 3 and drops to 2
        // because the away strength is below 90.
        let b = match_attendance_estimate(&s, &rt, &mut Rng::new(2), 0, 3, 4, 5, false, 0);
        assert_eq!(b, a * 4 / 3);
        // A large negative stadium term is lifted in steps of 500 above 500.
        let mut low = attendance_state();
        low.managers[0].stadium[3][0] = -100;
        let c = match_attendance_estimate(&low, &rt, &mut Rng::new(2), 0, 3, 4, 1, false, 0);
        let raw = a - 4 * 1000;
        assert!(
            c > 500 && c <= 1000 && (c - raw) % 500 == 0,
            "{c} from {raw}"
        );
        // Both strengths 0: 1, without any draw.
        let mut zero = attendance_state();
        set_club(&mut zero, 3, [0, 0, 0]);
        set_club(&mut zero, 4, [0, 0, 0]);
        let mut rng = Rng::new(2);
        assert_eq!(
            match_attendance_estimate(&zero, &rt, &mut rng, 0, 3, 4, 1, true, 0),
            1
        );
        assert_eq!(rng, Rng::new(2));
    }

    fn real_saves() -> Vec<GameState> {
        let Some(dir) = bmp_formats::data_dir() else {
            return Vec::new();
        };
        let mut paths = Vec::new();
        for d in [dir.clone(), dir.join("BMP")] {
            if let Ok(rd) = std::fs::read_dir(&d) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("man")) {
                        paths.push(p);
                    }
                }
            }
        }
        paths.sort();
        paths
            .iter()
            .map(|p| {
                let data = std::fs::read(p).unwrap();
                GameState::from_savegame(&SaveGame::parse(&data).unwrap()).unwrap()
            })
            .collect()
    }

    #[test]
    fn real_saves_starters_stand_on_the_board() {
        for s in real_saves() {
            for sq in 0..s.active_managers().len() as u8 {
                let n = usize::from(squad_count_listed(&s, usize::from(sq)));
                for p in &s.squads[usize::from(sq)][..n] {
                    if (1..12).contains(&p.lineup_position) {
                        assert!((-1..=7).contains(&p.board_row), "{p:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn real_saves_recalc_reproduces_the_saved_strength() {
        // The season loop's per-manager passes (2a58:323a, 2a58:3728) recalculate without
        // `full` after the match day's full pass, and the sample saves were taken after such
        // a pass: the saved strength arrays and aggression are exactly its result.
        for s in real_saves() {
            for m in 0..s.active_managers().len() as u8 {
                let mut t = s.clone();
                let mut rng = Rng::new(1);
                squad_recalc_team_strength(&mut t, &Runtime::default(), &mut rng, m, false);
                assert_eq!(t, s, "manager {m}");
            }
        }
    }

    #[test]
    fn real_saves_drift_stays_in_bounds() {
        for s in real_saves() {
            let rt = Runtime::default();
            let mut weekly = s.clone();
            league_strength_drift(&mut weekly, &rt, &mut Rng::new(7), 1);
            for (c, before) in weekly.clubs[..GERMAN_CLUBS].iter().zip(&s.clubs) {
                assert!(c.strength.iter().all(|v| (1..=99).contains(v)));
                assert!(c.base_strength.iter().all(|v| (1..=99).contains(v)));
                assert!(c.strength_1e.iter().all(|v| (45..=55).contains(v)));
                assert_eq!(c.crest, before.crest);
            }
            let mut reset = s.clone();
            league_strength_drift(&mut reset, &rt, &mut Rng::new(7), 10);
            for (t, c) in reset.clubs[..GERMAN_CLUBS].iter().enumerate() {
                let [lo, hi] =
                    LEAGUE_STRENGTH_BOUNDS[usize::from(rt.league_of_club(t as u8) as u8 >> 1)];
                assert!(c.strength.iter().all(|v| (lo..=hi).contains(v)));
                assert!(c.base_strength.iter().all(|v| (lo..=hi).contains(v)));
                assert_eq!(c.crest & 0x80, 0);
            }
        }
    }

    #[test]
    fn real_saves_attendance_respects_the_capacity() {
        for s in real_saves() {
            let rt = Runtime::default();
            for (m, manager) in s.active_managers().iter().enumerate() {
                let home = manager.team;
                if usize::from(home) >= GERMAN_CLUBS {
                    continue;
                }
                let stadium_cap = manager.stadium[0][0] + manager.stadium[1][0];
                for away in [0u8, 20, 40, 63, 120] {
                    for kind in 0..4 {
                        let mut rng = Rng::new(u16::from(away) + kind as u16);
                        let att = match_attendance_estimate(
                            &s, &rt, &mut rng, m as u8, home, away, kind, false, 0,
                        );
                        assert!(att <= stadium_cap, "{att} > {stadium_cap}");
                        assert!(att > 500 || att == stadium_cap || att == 1, "{att}");
                    }
                }
            }
        }
    }
}
