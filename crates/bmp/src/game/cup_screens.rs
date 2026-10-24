//! The screens behind the office main icon "Pokal/Europa" (segment 27c2): the
//! pairings and results of the current round of the DFB-Pokal and of the three
//! European cups. A click on a club opens its info. Spec:
//! `docs/re/logic/cups.md` §6.
//!
//! The cup data (draws, results, rounds) is [`bmp_game::cups`]; this module
//! ports the list layout, its drawing and the input handling, and the draw
//! screen (§5): the question, the draw board with the labels rising out of the
//! drum and the new pairings ([`DrawBoard`], reached through
//! [`bmp_game::cups::CupDrawUi`] from the new game and the season loop).
//! `screen_title_winner` (§7) is [`super::live_match::screen_title_winner`].

use bmp_game::cups::{CUP_ROUND_PAIRINGS, CupDrawUi};
use bmp_game::matchsim::CUP_ROW_CELLS;
use bmp_game::rng::NumberFormat;
use bmp_game::season::calendar_compute_date;
use bmp_game::state::GameState;

use super::stats_screens::{CUP_NAMES, DFB_STAGES};
use super::tables::{
    LEAGUE_NAMES, cat, centered, club_name, icon, league_tier_of_team, line, num, panel,
    screen_opponent_info, text_at,
};
use super::widgets::{dialog_two_choice, input_wait_click, ui_wait_right_click_button};
use super::{
    GAME_OPTION_A, GAME_OPTION_B, Session, append_ordinal_suffix, office_show_cached_background,
    text_to_upper_de,
};
use crate::gfx::{BitmapId, Gfx, RastPort, Ui, ui_draw_text_centered_shadow};
use crate::host::{Host, LEFT_DOWN, Quit, Run};

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
        .get(usize::from(c) * CUP_ROW_CELLS + s)
        .copied()
        .unwrap_or(0)
}

fn cup_goal(state: &GameState, c: u8, s: usize) -> u8 {
    state
        .cup_results
        .as_flattened()
        .get(usize::from(c) * CUP_ROW_CELLS + s)
        .copied()
        .unwrap_or(0)
}

/// First-leg goals of slot `s` of European cup `c` (`FAR:21c2[c][s]`).
fn first_leg_goal(state: &GameState, c: u8, s: usize) -> u8 {
    state
        .cup_first_leg_goals
        .get((usize::from(c) - 1) * CUP_ROW_CELLS + s)
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

/// `DS:269c`, `DS:4da0`, `DS:4d7c[10]`, `DS:4d7c[26]`, `DS:4d7c[0x17]`, `DS:4ddc`, `DS:4de0`:
/// the draw question (`"]"` is Ü, `"|"` Ö in the game font).
const ASK_TITLE: &[u8] = b"An Alle:";
const ASK_LINE1: &[u8] = b"M|CHTEN SIE DEM UN]BERTROFFENEM";
const ASK_LINE2_DFB: &[u8] = b"SCHAUSPIEL EINER DFB-POKAL-";
const ASK_LINE2_EURO: &[u8] = b"SCHAUSPIEL EINER EUROPA-POKAL-";
const ASK_LINE3: &[u8] = b"AUSLOSUNG BEIWOHNEN ?";
const ASK_YES: &[u8] = b"ABER KLAR";
const ASK_NO: &[u8] = b"KEIN GEDANKE";
/// `DS:26a0`.
const PRESS_KEY: &[u8] = b"TASTE DR]CKEN";
/// `DS:5356`: label colour of a DFB-Pokal club by `league_tier_of_team >> 1`.
const TIER_COLORS: [u8; 3] = [0x00, 0x07, 0x1a];
/// Colour of a German club's label in a European draw.
const GERMAN_LABEL_COLOR: u8 = 0x12;
/// `DS:5348`, `DS:534c`: board x of the labels of even and odd slots.
const LABEL_X: [i16; 2] = [0x25, 0xa7];
/// Where a label leaves the drum.
const DRUM_X: i16 = 0x69;
const DRUM_Y: i16 = 0xd1;
/// Top left of the board backup `FAR:3058` on the screen.
const BOARD_X: i16 = 0x16;
const BOARD_Y: i16 = 0x1c;

/// The bitmaps of an open draw board (`FAR:3058`, `FAR:6db4`, `FAR:3062`, `FAR:2ebe`).
struct BoardBitmaps {
    /// `FAR:3058` / `FAR:a6d0`: the board as it was before the current label.
    backup: BitmapId,
    /// `FAR:6db4` / `FAR:5158`: `PIC\48.VGA`, the drum lamps.
    lamps: BitmapId,
    /// `FAR:3062` / `FAR:a6d4`: the moving label over the board.
    compose: BitmapId,
    /// `FAR:2ebe` / `FAR:0174`: the label of the drawn club.
    label: RastPort,
}

/// The draw screen of cup_draw_screen_open .. cup_draw_screen_close: `board` is
/// `FAR:6db4 != NULL` (the player wanted to watch), `phase` the lamp phase `FAR:570e`, `fast`
/// the click that places the remaining labels at once.
#[derive(Default)]
pub struct DrawBoard {
    board: Option<BoardBitmaps>,
    phase: i16,
    fast: bool,
    pointer_hidden: bool,
}

impl DrawBoard {
    /// The question of cup row `cup`'s draw over the office background with the cup name.
    /// On "ABER KLAR" the draw board is shown and `true` returned; otherwise the screen is
    /// cleared (`cup_draw_screen_close` with `FAR:57c8 = 0`).
    ///
    /// Origin: BMMAIN.EXE 27c2:0006 (cup_draw_screen_open)
    pub fn ask(&mut self, host: &mut Host, auto_play: bool, cup: u8) -> Run<bool> {
        self.free(host);
        self.fast = false;
        office_show_cached_background(host)?;
        host.set_font(3);
        centered(
            host,
            CUP_NAMES[usize::from(cup & 3)],
            0,
            0x140,
            0x14,
            0x0b,
            0,
        );
        // copy_protection_penalty: the protection is treated as passed.
        host.screen_fade(1)?;
        // DS:2252 == 3 would take DS:4d7c[0x1a] as the third line of a European draw.
        let line2 = if cup != 0 {
            ASK_LINE2_EURO
        } else {
            ASK_LINE2_DFB
        };
        let yes = !auto_play
            && dialog_two_choice(
                host, auto_play, ASK_TITLE, ASK_LINE1, line2, ASK_LINE3, ASK_YES, ASK_NO,
            )? == 1;
        if !yes {
            host.g.rastport_clear(&host.rp, 0);
            return Ok(false);
        }
        host.screen_fade(0)?;
        let board = host.picture(0x2f)?;
        let screen = host.screen();
        host.g
            .blit_bitmap(board, 0, 0, screen, 0x13, 0x1a, 0x11a, 0xcb);
        host.g.free(board);
        self.phase = 0;
        let backup = host.blank_bitmap(0x113, 0xc6, 0);
        let lamps = host.picture(0x30)?;
        let compose = host.blank_bitmap(0x78, 0x0e, 0);
        let mut label = RastPort::new(host.blank_bitmap(0x73, 9, 0));
        Host::set_rastport_font(&mut label, 2);
        Gfx::rastport_set_draw_mode(&mut label, 0);
        self.board = Some(BoardBitmaps {
            backup,
            lamps,
            compose,
            label,
        });
        host.screen_fade(1)?;
        Ok(true)
    }

    /// Club `team` drawn into slot `s`: its label rises out of the drum and moves to its
    /// place on the board, or (after a click) appears there at once.
    ///
    /// Origin: BMMAIN.EXE 27c2:0378 (cup_draw_animate_slot), with the mouse poll of the
    /// draw loops (27c2:09e0, 27c2:13a2)
    pub fn animate_slot(
        &mut self,
        host: &mut Host,
        state: &GameState,
        cup: u8,
        s: u8,
        team: u8,
    ) -> Run<()> {
        if !self.pointer_hidden {
            host.g.mouse_pointer_hide(0);
            self.pointer_hidden = true;
        }
        if host.input_poll_mouse()? == LEFT_DOWN {
            self.fast = true;
        }
        let r = CUP_ROUND_PAIRINGS
            .get(usize::from(state.event_state[usize::from(cup & 3)]))
            .copied()
            .unwrap_or(0);
        if cup == 0 && r < 4 {
            return Ok(());
        }
        let Some(b) = &self.board else {
            return Ok(());
        };
        let (backup, lamps, label) = (b.backup, b.lamps, b.label.bitmap);
        let screen = host.screen();
        host.g
            .blit_bitmap(screen, BOARD_X, BOARD_Y, backup, 0, 0, 0x113, 0xc6);
        let (text, color) = label_text(state, cup, team, host.number_format());
        let lrp = &mut self.board.as_mut().expect("board").label;
        host.g.rastport_clear(lrp, 0x0b);
        ui_draw_text_centered_shadow(&mut host.g, lrp, &text, 2, 0x70, 5, color, 0xff);
        let (tx, ty) = label_target(s, r);
        let (mut x, mut y) = (DRUM_X, DRUM_Y);
        if !self.fast {
            for i in 0..7i16 {
                self.phase += 2;
                if self.phase == 8 {
                    self.phase = 0;
                }
                crate::intro::wait_vretrace(host)?;
                host.g.blit_bitmap(label, 0, 0, screen, x, y, 0x72, i + 1);
                host.g
                    .blit_bitmap(lamps, self.phase, 0, screen, 0x60, 0xd3, 2, 9);
                host.g
                    .blit_bitmap(lamps, self.phase, 0, screen, 0xe5, 0xd3, 2, 9);
                host.delay_ticks(i32::from(12 - i))?;
                y -= 1;
            }
            let mut toggle = false;
            while !(y <= ty && x == tx) {
                if !toggle {
                    self.draw_label_at(host, x, y)?;
                }
                toggle = !toggle;
                x += (tx - x).signum();
                y += (ty - y).signum();
            }
        }
        self.draw_label_at(host, tx, ty)
    }

    /// The label over the saved board at `(x, y)`, composed in VRAM page 1 and copied to
    /// the visible page after the retrace.
    ///
    /// Origin: BMMAIN.EXE 27c2:0378 (cup_draw_animate_slot, inner part)
    fn draw_label_at(&mut self, host: &mut Host, x: i16, y: i16) -> Run<()> {
        let Some(b) = &self.board else {
            return Ok(());
        };
        let (backup, compose, label) = (b.backup, b.compose, b.label.bitmap);
        host.g.blit_bitmap(
            backup,
            x - (BOARD_X + 2),
            y - BOARD_Y,
            compose,
            0,
            0,
            0x78,
            0x0e,
        );
        host.g.blit_bitmap(label, 0, 0, compose, 2, 0, 0x71, 7);
        host.g.vga_set_draw_page(2);
        let screen = host.screen();
        host.g
            .blit_bitmap(compose, 0, 0, screen, x - 2, y, 0x78, 0x0e);
        host.g.vga_set_draw_page(1);
        crate::intro::wait_vretrace(host)?;
        host.g
            .vga_restore_rect_from_page(0x140, 0xf0, x - 2, y, 0x78, 0x0e, 2);
        Ok(())
    }

    /// The end of a shown draw: "TASTE DRÜCKEN" and a click when the board was shown, then
    /// the pairings of the new round (dated one week later), then a black screen.
    ///
    /// Origin: BMMAIN.EXE 27c2:089a (cup_draw_screen_close)
    pub fn close(
        &mut self,
        host: &mut Host,
        session: &mut Session,
        state: &mut GameState,
        cup: u8,
    ) -> Run<()> {
        if self.pointer_hidden {
            host.g.mouse_pointer_show();
            self.pointer_hidden = false;
        }
        if self.board.is_some() {
            self.free(host);
            host.set_font(2);
            centered(host, PRESS_KEY, 0x5a, 0xed, 0xd9, 0x1d, 0);
            input_wait_click(host, false)?;
        }
        office_show_cached_background(host)?;
        state.season_week = state.season_week.wrapping_add(1);
        let r = screen_cup_matches(host, session, state, cup, 0);
        state.season_week = state.season_week.wrapping_sub(1);
        r?;
        host.g.rastport_clear(&host.rp, 0);
        Ok(())
    }

    /// Frees the board bitmaps (`FAR:6db4 = NULL`).
    fn free(&mut self, host: &mut Host) {
        if let Some(b) = self.board.take() {
            for bm in [b.lamps, b.backup, b.compose, b.label.bitmap] {
                host.g.free(bm);
            }
        }
    }
}

/// Text and colour of the label of club `team` in cup row `cup`'s draw: DFB-Pokal clubs
/// carry their league `" (1)"`..`" (3)"` and its colour; in a European draw German clubs
/// are colour 0x12 and foreign clubs 2..8 by their mean strength.
///
/// Origin: BMMAIN.EXE 27c2:0378 (cup_draw_animate_slot)
pub fn label_text(state: &GameState, cup: u8, team: u8, fmt: NumberFormat) -> (Vec<u8>, u8) {
    let mut buf = club_name(state, team);
    let color = if cup == 0 {
        let tier = league_tier_of_team(team) >> 1;
        cat(&mut buf, b" (");
        cat(&mut buf, &num(fmt, i32::from(tier) + 1));
        cat(&mut buf, b")");
        TIER_COLORS[usize::from(tier)]
    } else if team < FIRST_FOREIGN {
        GERMAN_LABEL_COLOR
    } else {
        let st = state.clubs[usize::from(team)].strength.map(i16::from);
        let avg = (st[0] + st[1] + st[2]) / 3;
        ((avg - 0x4b) * 7 / 20 + 2).clamp(2, 8) as u8
    };
    (buf, color)
}

/// Board position of the label of slot `s` in a draw of `r` clubs: pairs top down, the
/// home club left, centred vertically on the board.
///
/// Origin: BMMAIN.EXE 27c2:0378 (cup_draw_animate_slot)
pub fn label_target(s: u8, r: u8) -> (i16, i16) {
    let x = LABEL_X[usize::from(s & 1)];
    let y = i16::from(s & 0xfe) * 5 - ((i16::from(r & 0xfe) * 5) >> 1) + 0x76;
    (x, y)
}

/// The cup draw screens outside the season loop (a new game): [`DrawBoard`] on the host;
/// a quit is kept in `quit` (the logic cannot stop) and the screens are skipped after it.
pub struct DrawScreens<'a> {
    pub host: &'a mut Host,
    pub session: &'a mut Session,
    pub board: DrawBoard,
    pub quit: Option<Quit>,
}

impl<'a> DrawScreens<'a> {
    pub fn new(host: &'a mut Host, session: &'a mut Session) -> Self {
        Self {
            host,
            session,
            board: DrawBoard::default(),
            quit: None,
        }
    }

    fn keep<T>(&mut self, r: Run<T>, default: T) -> T {
        r.unwrap_or_else(|q| {
            self.quit = Some(q);
            default
        })
    }
}

impl CupDrawUi for DrawScreens<'_> {
    fn ask(&mut self, cup: u8) -> bool {
        if self.quit.is_some() {
            return false;
        }
        let auto = self.session.auto_play;
        let r = self.board.ask(self.host, auto, cup);
        self.keep(r, false)
    }

    fn animate_slot(&mut self, state: &GameState, cup: u8, slot: u8, team: u8) {
        if self.quit.is_none() {
            let r = self.board.animate_slot(self.host, state, cup, slot, team);
            self.keep(r, ());
        }
    }

    fn close(&mut self, state: &mut GameState, cup: u8) {
        if self.quit.is_none() {
            let r = self.board.close(self.host, self.session, state, cup);
            self.keep(r, ());
        }
    }

    fn title_winner(&mut self, state: &GameState, m: u8, comp: u8) {
        if self.quit.is_none() {
            let auto = self.session.auto_play;
            let r = super::live_match::screen_title_winner(self.host, state, m, comp, auto);
            self.keep(r, ());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draw_label_positions() {
        assert_eq!(label_target(0, 32), (0x25, 0x26));
        assert_eq!(label_target(31, 32), (0xa7, 0xbc));
        assert_eq!(label_target(3, 4), (0xa7, 0x76));
        assert_eq!(label_target(0, 16), (0x25, 0x4e));
    }

    #[test]
    fn draw_label_texts_and_colours() {
        let mut s = GameState::zeroed();
        s.clubs[3].name[..3].copy_from_slice(b"ABC");
        s.clubs[40].name[..3].copy_from_slice(b"XYZ");
        let (t, c) = label_text(&s, 0, 3, fmt());
        assert_eq!((t.as_slice(), c), (&b"ABC (1)"[..], 0));
        let (t, c) = label_text(&s, 0, 40, fmt());
        assert_eq!((t.as_slice(), c), (&b"XYZ (3)"[..], 0x1a));
        assert_eq!(label_text(&s, 1, 3, fmt()), (b"ABC".to_vec(), 0x12));
        for (strength, want) in [([75u8; 3], 2), ([80; 3], 3), ([100; 3], 8), ([10; 3], 2)] {
            s.clubs[70].strength = strength;
            assert_eq!(label_text(&s, 2, 70, fmt()).1, want, "{strength:?}");
        }
    }

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
