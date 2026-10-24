//! The screens behind the office main icon "Pokal/Europa" (segment 27c2): the
//! pairings and results of the current round of the DFB-Pokal and of the three
//! European cups. A click on a club opens its info. Spec:
//! `docs/re/logic/cups.md` §6.
//!
//! The cup data (draws, results, rounds) is [`bmp_game::cups`]; this module
//! ports the list layout, its drawing and the input handling.
//!
//! TODO(#31): the draw screen (§5, `cup_draw_screen_open` 27c2:0006 and the
//! label animation) and `screen_title_winner` (§7, 27c2:274d) are reached from
//! the season loop (match days, season start), not from the office.

use bmp_game::cups::CUP_ROUND_PAIRINGS;
use bmp_game::rng::NumberFormat;
use bmp_game::season::calendar_compute_date;
use bmp_game::state::GameState;

use super::stats_screens::{CUP_NAMES, DFB_STAGES};
use super::tables::{
    LEAGUE_NAMES, cat, centered, club_name, icon, league_tier_of_team, line, num, panel,
    screen_opponent_info, text_at,
};
use super::widgets::ui_wait_right_click_button;
use super::{GAME_OPTION_A, GAME_OPTION_B, Session, append_ordinal_suffix, text_to_upper_de};
use crate::gfx::Ui;
use crate::host::{Host, Run};

/// `DS:0654[1..6]`, `DS:065c[1..6]`: tier-pair categories `tierA + 8 * tierB` of
/// the DFB-Pokal groups (both orientations); index 0 is unused.
const PAIRING_CATEGORY: [u8; 7] = [0, 0x09, 0x0a, 0x0c, 0x12, 0x22, 0x24];
const PAIRING_CATEGORY_SWAPPED: [u8; 7] = [0, 0x09, 0x11, 0x21, 0x12, 0x14, 0x24];
/// `DS:537c[c != 0]`: last week index for which the next match date is shown.
const DATE_LAST_WEEK: [u8; 2] = [0x40, 0x40];
/// Day from which a cup list with a live minute is the relegation play-off.
const RELEGATION_DAY: i32 = 315;
/// First foreign club id (`0x40`): lower ids are German.
const FIRST_FOREIGN: u8 = 0x40;

/// `DS:25c4`, `DS:25d4`, `DS:26b0`, `DS:26b4`, `DS:26b8`, `DS:5371`.
const RELEGATION: &[u8] = b"Relegation";
const MINUTE: &[u8] = b"Minute";
const VERSUS: &[u8] = b"GEGEN";
const PENALTIES: &[u8] = b"n.E.";
const EXTRA_TIME: &[u8] = b"n.V.";
const NO_PAIRING: &[u8] = b"@@@@@@@@@";

/// One line of the cup list body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CupLine {
    /// DFB-Pokal group heading `k` (1..6, see [`cup_tier_pairing_label`]).
    Label { k: u8, y: u16 },
    /// Pairing `j` (0-based) of the cup row: home club, away club.
    Pair {
        y: u16,
        j: usize,
        home: u8,
        away: u8,
    },
    /// `"@@@@@@@@@"`: a DFB-Pokal group without a pairing.
    Empty { y: u16 },
}

/// Position and contents of the cup list of competition `c`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CupMatchesLayout {
    /// Top of the layout; the panel starts at `top + 5`.
    pub top: u8,
    /// Panel height + 8.
    pub height: u8,
    pub lines: Vec<CupLine>,
}

fn cup_slot(state: &GameState, c: u8, s: usize) -> u8 {
    // Rounds before the first one read on into the next cup rows, like the
    // original's flat FAR:76ca array.
    state
        .cup_pairings
        .as_flattened()
        .get(usize::from(c) * 32 + s)
        .copied()
        .unwrap_or(0)
}

fn cup_goal(state: &GameState, c: u8, s: usize) -> u8 {
    state
        .cup_results
        .as_flattened()
        .get(usize::from(c) * 32 + s)
        .copied()
        .unwrap_or(0)
}

/// First-leg goals of slot `s` of European cup `c` (`FAR:21c2[c][s]`).
fn first_leg_goal(state: &GameState, c: u8, s: usize) -> u8 {
    state
        .cup_first_leg_goals
        .get((usize::from(c) - 1) * 32 + s)
        .copied()
        .unwrap_or(0)
}

/// Pairings played in the current round of cup row `c` (the European cups
/// share the round counter of row 1); at least 1.
fn current_pairings(state: &GameState, c: u8) -> u8 {
    let round = usize::from(state.event_state[usize::from(c != 0)]);
    // DS:07d6 + 6 is the low byte of the day counter DS:07dc.
    let r = match round {
        0..=5 => CUP_ROUND_PAIRINGS[round],
        6 => state.day_counter as u8,
        _ => 0,
    };
    r.max(1)
}

/// Computes the panel position and the lines of the cup list: the DFB-Pokal
/// groups its pairings by the leagues of the two clubs (with a heading per
/// group and `"@@@@@@@@@"` for an empty one), the European cups list theirs
/// in draw order.
///
/// Origin: BMMAIN.EXE 27c2:1ccb (screen_cup_matches, layout)
pub fn cup_matches_layout(state: &GameState, c: u8) -> CupMatchesLayout {
    let euro = c != 0;
    let r = i16::from(current_pairings(state, c));
    let mut rows: i16 = i16::from(euro) * r + 1;
    // cat[j] for j = 1..rows-1 (DFB-Pokal only).
    let mut cats = vec![0u8];
    if !euro {
        for s in (0..2 * r as usize).step_by(2) {
            let a = league_tier_of_team(cup_slot(state, c, s));
            let b = league_tier_of_team(cup_slot(state, c, s + 1));
            cats.push((b << 3).wrapping_add(a));
            rows += 1;
        }
    }
    let matches = |k: usize, v: u8| v == PAIRING_CATEGORY[k] || v == PAIRING_CATEGORY_SWAPPED[k];
    let mut cnt = rows as u8;
    if euro {
        cnt = cnt.wrapping_sub(6);
    } else {
        for k in 1..7 {
            if !cats[1..].iter().any(|&v| matches(k, v)) {
                cnt = cnt.wrapping_add(1);
            }
        }
    }
    let height = cnt.wrapping_mul(7).wrapping_add(0x3a);
    let top = ((0xe9 - i16::from(height)) / 2 - 0x14) as u8;
    let mut y = u16::from(top) + (2 + u16::from(!euro)) * 8;
    let mut lines = Vec::new();
    for k in (if euro { 6 } else { 1 })..7usize {
        if !euro {
            lines.push(CupLine::Label { k: k as u8, y });
        }
        y = y.wrapping_add(7);
        let y_head = y as u8;
        for j in 1..rows as usize {
            if !euro && !cats.get(j).is_some_and(|&v| matches(k, v)) {
                continue;
            }
            let s = 2 * (j - 1);
            lines.push(CupLine::Pair {
                y,
                j: j - 1,
                home: cup_slot(state, c, s),
                away: cup_slot(state, c, s + 1),
            });
            y = y.wrapping_add(7);
        }
        if !euro && u16::from(y_head) == y {
            lines.push(CupLine::Empty { y });
            y = y.wrapping_add(7);
        }
    }
    CupMatchesLayout { top, height, lines }
}

/// The heading of a DFB-Pokal group `k` (1..6), e.g. `"BUNDESLIGA GEGEN
/// ZWEITE LIGA:"`.
///
/// Origin: BMMAIN.EXE 27c2:1bf0 (cup_tier_pairing_label)
pub fn cup_tier_pairing_label(k: u8) -> Vec<u8> {
    let a = if k < 4 {
        1
    } else if k < 6 {
        2
    } else {
        3
    };
    let b = match k {
        1 => 1,
        2 | 4 => 2,
        _ => 3,
    };
    let mut buf = text_to_upper_de(LEAGUE_NAMES[a - 1]);
    cat(&mut buf, b" GEGEN ");
    cat(&mut buf, &text_to_upper_de(LEAGUE_NAMES[b - 1]));
    cat(&mut buf, b":");
    buf
}

/// The result text of a pairing: `"(g0:g1)"` plus `"n.V."` after extra time
/// (+10 on the home goals) or `"n.E."` after penalties (+20).
///
/// Origin: BMMAIN.EXE 27c2:1ccb (screen_cup_matches, 27c2:231e..2411)
pub fn cup_result_text(home: u8, away: u8, fmt: NumberFormat) -> Vec<u8> {
    let mut g = home;
    let mut pen = 0;
    if g > 9 {
        g -= 10;
        pen = 1;
        if g > 9 {
            g -= 10;
            pen = 2;
        }
    }
    let mut buf = b"(".to_vec();
    cat(&mut buf, &num(fmt, i32::from(g)));
    cat(&mut buf, b":");
    cat(&mut buf, &num(fmt, i32::from(away)));
    cat(&mut buf, b")");
    match pen {
        2 => cat(&mut buf, PENALTIES),
        1 => cat(&mut buf, EXTRA_TIME),
        _ => {}
    }
    buf
}

/// Week index of the next match of cup `c` from week `from` on: the first week
/// with a cup bit (`>= 8`) of the cup's mask (`c` 0: 1, else `2 * c`; for
/// c = 3 that is 6, the bits of cups 1 and 2, an original quirk without effect
/// since all European legs are 0x70). The original loop is unbounded; the port
/// stops at the end of the calendar.
///
/// Origin: BMMAIN.EXE 27c2:1ccb (screen_cup_matches, 27c2:203b..2082)
pub fn next_cup_week(week_competitions: &[u8; 95], c: u8, from: u8) -> u16 {
    let mask = if c == 0 { 1 } else { u16::from(c) << 1 };
    let mut w = usize::from(from);
    while w < week_competitions.len() {
        let v = week_competitions[w];
        if v >= 8 && u16::from(v >> 3) & mask != 0 {
            break;
        }
        w += 1;
    }
    w as u16
}

fn is_human_team(state: &GameState, t: u8) -> bool {
    let n = usize::from(state.manager_count).min(4);
    state.managers[..n].iter().any(|m| m.team == t)
}

/// Pairings and results of the current round of cup `c` (0 = DFB-Pokal,
/// 1..3 = European cups). `minute` is 0 before the matches (office, after a
/// draw), 120 after the match day or the live minute. A click on a club opens
/// its info, the OK icon closes the list.
///
/// Origin: BMMAIN.EXE 27c2:1ccb (screen_cup_matches)
pub fn screen_cup_matches(
    host: &mut Host,
    session: &mut Session,
    state: &mut GameState,
    c: u8,
    minute: u8,
) -> Run<()> {
    // The title "Pokalspiele"/"Relegationsspiel" (DS:26a8/26ac) is copied but
    // never drawn. copy_protection_penalty: the protection is treated as passed.
    let euro = c != 0;
    let layout = cup_matches_layout(state, c);
    let top = i16::from(layout.top);
    let mut rects = [[0i16; 4]; 2];
    rects[0] = panel(host, 5, top + 5, 0x134, i16::from(layout.height) - 8, false).1;
    rects[1] = icon(host, 0x10c, 0xc1, 0x22 + i16::from(minute != 0));
    line(host, 7, (top + 0xf) as u8, 0x137, (top + 0xf) as u8, 0xb);
    host.set_font(1);
    let late = state.day_counter >= RELEGATION_DAY;
    let mut head;
    if !late || minute == 0 {
        let round = usize::from(state.event_state[usize::from(c)]);
        head = DFB_STAGES.get(round + 1).copied().unwrap_or(b"").to_vec();
        cat(&mut head, CUP_NAMES[usize::from(c & 3)]);
    } else {
        head = RELEGATION.to_vec();
    }
    if minute != 0 {
        cat(&mut head, b"    ");
        cat(
            &mut head,
            &num(host.number_format(), i32::from(minute as i8)),
        );
        append_ordinal_suffix(&mut head, minute, GAME_OPTION_A, GAME_OPTION_B);
        cat(&mut head, MINUTE);
    } else if !late && DATE_LAST_WEEK[usize::from(euro)] >= state.season_week {
        cat(&mut head, b"    ");
        let w = next_cup_week(&state.week_competitions, c, state.season_week);
        // Overwrites the date globals, like the original.
        calendar_compute_date(state, w);
        let fmt = host.number_format();
        cat(&mut head, &num(fmt, state.day_of_month));
        cat(&mut head, b".");
        cat(&mut head, &num(fmt, state.month + 1));
        cat(&mut head, b".");
    }
    centered(host, &head, 5, 0x139, top + 0xd, 0xb, 7);
    host.set_font(2);
    // row_y[], list0[], list1[] of the click handler (32 entries, unused = 0).
    let mut row_y = [0u8; 32];
    let mut clubs = [[0u8; 32]; 2];
    let mut n = 0usize;
    let fmt = host.number_format();
    for l in &layout.lines {
        match *l {
            CupLine::Label { k, y } => {
                centered(host, &cup_tier_pairing_label(k), 0, 0x12b, y as i16, 2, 0);
            }
            CupLine::Empty { y } => centered(host, NO_PAIRING, 0, 0x118, y as i16, 2, 7),
            CupLine::Pair { y, j, home, away } => {
                let color = |t: u8| {
                    let own = if euro {
                        t < FIRST_FOREIGN
                    } else {
                        is_human_team(state, t)
                    };
                    if own { 0xb } else { 1 }
                };
                let yi = y as i16;
                if n < 32 {
                    row_y[n] = y as u8;
                    clubs[0][n] = home;
                    clubs[1][n] = away;
                }
                n += 1;
                let (ch, ca) = (color(home), color(away));
                centered(host, &club_name(state, home), 0, 0x82, yi, ch, 7);
                centered(host, VERSUS, 0, 0x118, yi, 3, 0);
                centered(host, &club_name(state, away), 0x8c, 0x10e, yi, ca, 7);
                let s = 2 * j;
                let res = cup_result_text(cup_goal(state, c, s), cup_goal(state, c, s + 1), fmt);
                text_at(host, &res, if euro { 0xf9 } else { 0x10c }, yi, 1);
                if euro {
                    let mut buf = b"(".to_vec();
                    cat(&mut buf, &num(fmt, i32::from(first_leg_goal(state, c, s))));
                    cat(&mut buf, b":");
                    cat(
                        &mut buf,
                        &num(fmt, i32::from(first_leg_goal(state, c, s + 1))),
                    );
                    cat(&mut buf, b")");
                    text_at(host, &buf, 0x11d, yi, 1);
                }
            }
        }
    }
    host.screen_fade(1)?;
    loop {
        let ev = host.input_poll_mouse()?;
        let (mx, my) = host.mouse_xy();
        let hit = Ui::hit_test_buttons(&rects, 2, mx, my, Some(ev as i16));
        if hit == 1 && my > top + 5 {
            let side = usize::from(mx > 0x82);
            let row = row_y
                .iter()
                .position(|&ry| i16::from(ry) - 6 < my && my <= i16::from(ry));
            if let Some(i) = row {
                screen_opponent_info(host, state, clubs[side][i], side as u8, false, 0)?;
            }
        }
        if hit == 2 || session.auto_play {
            break;
        }
    }
    ui_wait_right_click_button(host, 0x10c, 0xc1, session.auto_play)?;
    host.screen_fade(0)?;
    host.ui.redraw_buttons(&mut host.g, &mut host.rp, 0x7f, 0);
    host.g.rastport_clear(&host.rp, 0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt() -> NumberFormat {
        NumberFormat {
            no_separators: true,
            comma_separator: false,
            pad_width: 1,
        }
    }

    /// The quarter-final of 07072019.MAN.
    fn quarter_final() -> GameState {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.managers[0].team = 36;
        s.event_state = [3, 3, 3, 3, 0, 0, 0, 0];
        s.cup_pairings[..4].copy_from_slice(&[[8, 0], [4, 11], [16, 5], [17, 31]]);
        s.cup_pairings[16..20].copy_from_slice(&[[148, 152], [139, 151], [150, 132], [119, 81]]);
        s
    }

    #[test]
    fn dfb_list_groups_the_pairings_by_league() {
        let l = cup_matches_layout(&quarter_final(), 0);
        // Five rows plus four empty groups: h = 7 * 9 + 0x3a.
        assert_eq!((l.top, l.height), (36, 121));
        use CupLine::*;
        assert_eq!(
            l.lines,
            vec![
                Label { k: 1, y: 60 },
                Pair {
                    y: 67,
                    j: 0,
                    home: 8,
                    away: 0
                },
                Pair {
                    y: 74,
                    j: 1,
                    home: 4,
                    away: 11
                },
                Pair {
                    y: 81,
                    j: 2,
                    home: 16,
                    away: 5
                },
                Label { k: 2, y: 88 },
                Pair {
                    y: 95,
                    j: 3,
                    home: 17,
                    away: 31
                },
                Label { k: 3, y: 102 },
                Empty { y: 109 },
                Label { k: 4, y: 116 },
                Empty { y: 123 },
                Label { k: 5, y: 130 },
                Empty { y: 137 },
                Label { k: 6, y: 144 },
                Empty { y: 151 },
            ]
        );
    }

    #[test]
    fn euro_list_keeps_the_draw_order() {
        let l = cup_matches_layout(&quarter_final(), 1);
        // cnt = 5 - 6 wraps to 0xff: h = (7 * 0xff + 0x3a) as u8 = 0x33.
        assert_eq!((l.top, l.height), (71, 0x33));
        let ys: Vec<_> = l
            .lines
            .iter()
            .map(|l| match l {
                CupLine::Pair { y, j, .. } => (*y, *j),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(ys, [(94, 0), (101, 1), (108, 2), (115, 3)]);
    }

    #[test]
    fn first_round_lists_sixteen_pairs() {
        let mut s = quarter_final();
        s.event_state = [1, 1, 1, 1, 0, 0, 0, 0];
        let l = cup_matches_layout(&s, 2);
        assert_eq!(l.lines.len(), 16);
        // cnt = 17 - 6 = 11: h = 7 * 11 + 0x3a, top = (0xe9 - 0x87) / 2 - 0x14.
        assert_eq!((l.top, l.height), (29, 0x87));
    }

    #[test]
    fn group_labels() {
        assert_eq!(cup_tier_pairing_label(1), b"BUNDESLIGA GEGEN BUNDESLIGA:");
        assert_eq!(cup_tier_pairing_label(2), b"BUNDESLIGA GEGEN ZWEITE LIGA:");
        assert_eq!(cup_tier_pairing_label(3), b"BUNDESLIGA GEGEN AM.-OBERLIGA:");
        assert_eq!(cup_tier_pairing_label(4), b"ZWEITE LIGA GEGEN ZWEITE LIGA:");
        assert_eq!(
            cup_tier_pairing_label(5),
            b"ZWEITE LIGA GEGEN AM.-OBERLIGA:"
        );
        assert_eq!(
            cup_tier_pairing_label(6),
            b"AM.-OBERLIGA GEGEN AM.-OBERLIGA:"
        );
    }

    #[test]
    fn result_markers() {
        assert_eq!(cup_result_text(2, 1, fmt()), b"(2:1)");
        assert_eq!(cup_result_text(13, 3, fmt()), b"(3:3)n.V.");
        assert_eq!(cup_result_text(24, 3, fmt()), b"(4:3)n.E.");
    }

    #[test]
    fn next_cup_week_by_mask() {
        let mut wk = [0u8; 95];
        wk[30] = 8;
        wk[31] = 0x70;
        wk[0x5b] = 0x10;
        assert_eq!(next_cup_week(&wk, 0, 25), 30);
        assert_eq!(next_cup_week(&wk, 1, 25), 31);
        assert_eq!(next_cup_week(&wk, 3, 25), 31);
        // The relegation legs carry bit 4 only, which is mask 2 (cup 1).
        assert_eq!(next_cup_week(&wk, 1, 32), 0x5b);
        assert_eq!(next_cup_week(&wk, 0, 32), 95);
    }
}
