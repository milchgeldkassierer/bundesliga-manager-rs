//! Writes a patched copy of a `.MAN` save game for the visual regression scenarios
//! (`tools/visual-check.sh`, "# fixture-edit:" lines), or prints the state the patches touch.
//!
//! Usage: `save_patch <in.MAN> [<out.MAN> <op>...]`; without `<out.MAN>` it prints the state.
//! Operations (applied in order, manager 0 is the first human manager):
//! * `injure <n>`: every listed player of manager 0 after the first `n` available starters
//!   gets injured for ten games ("Bänderriss", like `player_injure` 24a4:30cf, without its
//!   skill loss) and leaves the lineup, so the club cannot field a team.
//! * `cup-final <cup> <home> <away>`: cup row `cup` (0 DFB-Pokal, 1..3 the European cups) is
//!   at its final (round 5) between clubs `home` and `away`, with no goals; a European final
//!   is at its return leg after a first leg the club `home` won 9:0 (stored per slot of the
//!   return leg, as `cup_round_complete` 27c2:16dc leaves it).
//! * `weak <team>`: club `team` gets the lowest form and strengths (1), so it hardly wins.
//! * `slider <v>`: manager 0's tactic slider (0..=34, the base of the match aggression).
//! * `id <n>`: the save id becomes `n`; the game refuses to load a save whose id it loaded or
//!   wrote before in the session (`savegame_read_man` 322b:120c), so fixtures of one source
//!   save that a scenario loads one after the other need distinct ids.
//! * `second-legs`: the European cups play the return legs of their round next; the home club
//!   of every return leg won the first leg 9:0, so no tie needs extra time.
//! * `week <n> <bits>`: the slot `n` slots after the current one (`season_week`) gets the
//!   competition bits `bits`.
//!
//! The save is written through `GameState::to_savegame`, so ciphers and checksums are right.

use std::process::ExitCode;

use bmp_formats::savegame::SaveGame;
use bmp_game::state::GameState;

/// Bänderriss (`INJURY_NAMES[6]`).
const INJURY: u8 = 6;
/// Games an injured player misses.
const INJURY_GAMES: u8 = 10;

fn row(cup: usize) -> std::ops::Range<usize> {
    cup * 32..cup * 32 + 32
}

fn print_state(s: &GameState) {
    println!(
        "date {}.{}.{} day {} season_week {} start_year {} level {}",
        s.day_of_month,
        s.month + 1,
        s.year,
        s.day_counter,
        s.season_week,
        s.start_year,
        s.level
    );
    let w = usize::from(s.season_week);
    println!(
        "week_competitions[{w}..]: {:?}",
        &s.week_competitions[w..(w + 16).min(95)]
    );
    println!("competition_rounds {:?}", s.competition_rounds);
    println!(
        "event_state {:?} euro_second_leg_pending {:?}",
        s.event_state, s.euro_second_leg_pending
    );
    println!("options {:?}", s.options);
    for cup in 0..4 {
        let p = &s.cup_pairings.as_flattened()[row(cup)];
        let g = &s.cup_results.as_flattened()[row(cup)];
        println!(
            "cup {cup}: slots {:?}\n        goals {:?}",
            &p[..16],
            &g[..16]
        );
    }
    for (m, r) in s.managers[..usize::from(s.manager_count)]
        .iter()
        .enumerate()
    {
        println!(
            "manager {m} {} team {} ({}) league {} aggression {} slider {} values_132 {:?}",
            String::from_utf8_lossy(r.name()),
            r.team,
            String::from_utf8_lossy(s.clubs[usize::from(r.team)].name()),
            r.league,
            r.aggression,
            r.value_131,
            r.values_132
        );
        for p in s.squads[m].iter().filter(|p| p.player_id != 0) {
            println!(
                "  player {:3} {:20} status {:#04x} lineup {:2} suspended {}",
                p.player_id,
                String::from_utf8_lossy(s.players[usize::from(p.player_id)].name()),
                p.status,
                p.lineup_position,
                p.suspended
            );
        }
    }
}

fn arg<T: std::str::FromStr>(ops: &mut std::slice::Iter<'_, String>) -> Result<T, String> {
    ops.next()
        .ok_or("missing operand")?
        .parse()
        .map_err(|_| "bad operand".to_string())
}

fn apply(s: &mut GameState, ops: &[String]) -> Result<(), String> {
    let mut it = ops.iter();
    while let Some(op) = it.next() {
        match op.as_str() {
            "injure" => {
                let keep: usize = arg(&mut it)?;
                let mut kept = 0;
                for p in s.squads[0].iter_mut().filter(|p| p.player_id != 0) {
                    if p.status & 3 == 0 && (1..12).contains(&p.lineup_position) && kept < keep {
                        kept += 1;
                        continue;
                    }
                    p.status = (p.status & !3) | 2;
                    p.suspended = INJURY_GAMES;
                    p.injury = INJURY;
                    p.lineup_position = 0;
                }
            }
            "cup-final" => {
                let cup: usize = arg(&mut it)?;
                let (home, away): (u8, u8) = (arg(&mut it)?, arg(&mut it)?);
                if cup > 3 {
                    return Err("cup must be 0..3".into());
                }
                s.event_state[cup] = 5;
                let slots = &mut s.cup_pairings.as_flattened_mut()[row(cup)];
                slots.fill(0);
                slots[..2].copy_from_slice(&[home, away]);
                s.cup_results.as_flattened_mut()[row(cup)].fill(0);
                if cup > 0 {
                    s.euro_second_leg_pending[cup - 1] = 1;
                    let first = &mut s.cup_first_leg_goals[row(cup - 1)];
                    first.fill(0);
                    // Stored per slot of the return leg: the club of `home` won the first leg.
                    first[..2].copy_from_slice(&[9, 0]);
                }
            }
            "weak" => {
                let team: usize = arg(&mut it)?;
                let c = s.clubs.get_mut(team).ok_or("club out of range")?;
                c.morale = 0;
                c.strength = [1; 3];
                c.base_strength = [1; 3];
                c.strength_1e = [1; 3];
            }
            "slider" => s.managers[0].value_131 = arg(&mut it)?,
            "id" => s.save_id = arg(&mut it)?,
            "second-legs" => {
                s.euro_second_leg_pending = [1; 3];
                for cup in 1..4 {
                    let first = &mut s.cup_first_leg_goals[row(cup - 1)];
                    for pair in first.chunks_mut(2) {
                        pair.copy_from_slice(&[9, 0]);
                    }
                }
            }
            "week" => {
                let ahead: usize = arg(&mut it)?;
                let bits: u8 = arg(&mut it)?;
                let w = usize::from(s.season_week) + ahead;
                s.week_competitions[w] = bits;
            }
            _ => return Err(format!("unknown operation {op}")),
        }
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let input = args
        .first()
        .ok_or("usage: save_patch <in.MAN> [<out.MAN> <op>...]")?;
    let bytes = std::fs::read(input).map_err(|e| format!("{input}: {e}"))?;
    let save = SaveGame::parse(&bytes).map_err(|e| format!("{input}: {e:?}"))?;
    let mut state = GameState::from_savegame(&save).map_err(|e| format!("{input}: {e:?}"))?;
    let Some(out) = args.get(1) else {
        print_state(&state);
        return Ok(());
    };
    apply(&mut state, &args[2..])?;
    let bytes = state
        .to_savegame()
        .to_bytes()
        .map_err(|e| format!("{e:?}"))?;
    std::fs::write(out, bytes).map_err(|e| format!("{out}: {e}"))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("save_patch: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmp_game::cups::cup_tie_winner;
    use bmp_game::squad::squad_count_starters;

    fn ops(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// Manager 0 with eleven available starters (player ids 1..=11) and two reserves.
    fn squad_state() -> GameState {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        for (i, p) in s.squads[0][..13].iter_mut().enumerate() {
            p.player_id = i as u8 + 1;
            p.lineup_position = if i < 11 { i as u8 + 1 } else { 12 };
        }
        s
    }

    #[test]
    fn injure_keeps_the_first_n_available_starters() {
        let mut s = squad_state();
        s.squads[0][0].status = 1; // suspended: not kept
        apply(&mut s, &ops(&["injure", "7"])).unwrap();
        assert_eq!(squad_count_starters(&s, 0), 7);
        let kept: Vec<u8> = s.squads[0][..13]
            .iter()
            .filter(|p| p.lineup_position != 0)
            .map(|p| p.player_id)
            .collect();
        assert_eq!(kept, [2, 3, 4, 5, 6, 7, 8]);
        for p in s.squads[0][..13].iter().filter(|p| p.lineup_position == 0) {
            assert_eq!(
                (p.status & 3, p.suspended, p.injury),
                (2, INJURY_GAMES, INJURY)
            );
        }
    }

    #[test]
    fn a_patched_european_final_goes_to_the_home_club_after_a_lost_return_leg() {
        let mut s = GameState::zeroed();
        apply(&mut s, &ops(&["cup-final", "2", "36", "107"])).unwrap();
        assert_eq!(s.event_state[2], 5);
        assert_eq!(s.euro_second_leg_pending, [0, 1, 0]);
        // A 0:3 return leg still leaves 9:3 on aggregate: slot 0 (36) goes through.
        s.cup_results.as_flattened_mut()[row(2)][..2].copy_from_slice(&[0, 3]);
        assert_eq!(cup_tie_winner(&s, 2, 0), 0);
    }

    #[test]
    fn second_legs_give_every_tie_a_first_leg_lead() {
        let mut s = GameState::zeroed();
        apply(&mut s, &ops(&["second-legs"])).unwrap();
        assert_eq!(s.euro_second_leg_pending, [1; 3]);
        assert!(s.cup_first_leg_goals.chunks(2).all(|p| p == [9, 0]));
    }

    #[test]
    fn week_id_slider_and_weak_set_their_fields() {
        let mut s = GameState::zeroed();
        s.season_week = 25;
        apply(
            &mut s,
            &ops(&[
                "week", "1", "120", "id", "1001", "slider", "34", "weak", "63",
            ]),
        )
        .unwrap();
        assert_eq!(s.week_competitions[26], 120);
        assert_eq!(s.save_id, 1001);
        assert_eq!(s.managers[0].value_131, 34);
        assert_eq!(s.clubs[63].strength, [1; 3]);
        assert!(apply(&mut s, &ops(&["bogus"])).is_err());
        assert!(apply(&mut s, &ops(&["cup-final", "4", "1", "2"])).is_err());
    }
}
