//! The manager's office: header, desk panel with portrait, crest, the
//! competition of the day and the league-position curve, the date panel and the
//! six main icons with their 3x3 submenus. Spec: `docs/re/logic/flow.md` §6.

use bmp_game::league::{
    LEAGUE_ROUND_COUNT, RoundMode, fixtures_build_round_pairings, league_apply_round_and_sort,
};
use bmp_game::matchsim::has_rescheduled_match_today;
use bmp_game::rng::{NumberFormat, format_long_thousands};
use bmp_game::season::calendar_compute_date;
use bmp_game::squad::{LineupRules, auto_lineup};
use bmp_game::state::{GameState, Manager, RESCHEDULED_GAMES_DAY};

use super::contract::screen_manager_contract;
use super::credits::screen_credits;
use super::cup_screens::screen_cup_matches;
use super::disk::{
    HIGHSCORES_DISABLED, HighscoreMode, highscore_screen, quit_game_confirm, screen_save_game,
    screen_settings,
};
use super::load_game::screen_load_game;
use super::scorers::screen_top_scorers;
use super::squad_screen::screen_squad_lineup;
use super::stadium::screen_stadium;
use super::start_menu::Pictures;
use super::stats_screens::{screen_eternal_table, screen_manager_history, screen_statistics};
use super::tables::{screen_fixtures, screen_league_table, screen_strength_table};
use super::training::{screen_training, screen_training_camp};
use super::transfer_screen::screen_transfer_market;
use super::widgets::ui_wait_right_click_button;
use super::{
    GAME_OPTION_A, GAME_OPTION_B, SEASON_START_DAY, Session, append_ordinal_suffix,
    office_show_cached_background,
};
use crate::gfx::{BitmapId, Gfx, Ui, ui_draw_line, ui_draw_text_centered_shadow};
use crate::host::{Host, LEFT_DOWN, RIGHT_DOWN, Run};

/// Pictures of the office (`office_load_bitmaps`).
pub struct OfficeBitmaps {
    /// `PIC\4.VGA`, header bar (`FAR:0024`).
    pub header: BitmapId,
    /// `PIC\23.VGA`, desk / top panel (`FAR:2e9a`).
    pub desk: BitmapId,
}

impl OfficeBitmaps {
    /// Loads the header and the desk panel; modes other than 1 also reload the
    /// icon sheet `PIC\3.VGA`.
    ///
    /// Origin: BMMAIN.EXE 18bc:034d (office_load_bitmaps)
    pub fn load(host: &mut Host, mode: u8) -> Result<Self, String> {
        let header = host.load_picture(4)?;
        let desk = host.load_picture(0x17)?;
        if mode != 1 {
            let icons = host.load_picture(3)?;
            host.ui.icons = Some(icons);
        }
        Ok(Self { header, desk })
    }
}

/// `"SAISONENDE"`, `" Spieltag"`, ... (`DS:25b8..25d0`).
const SEASON_END: &[u8] = b"SAISONENDE";
const MATCHDAY: &[u8] = b" Spieltag";
const CUP: &[u8] = b"DFB-Pokal";
const RELEGATION: &[u8] = b"Relegation";
const EURO_CUP: &[u8] = b"Europapokal";
const RESCHEDULED: &[u8] = b"Nachholspiel";
const NO_MATCH_TODAY: &[u8] = b"Spielfrei";

/// Weekday and month names (`DS:242c`, `DS:23fc`).
const WEEKDAYS: [&[u8]; 7] = [
    b"Montag",
    b"Dienstag",
    b"Mittwoch",
    b"Donnerstag",
    b"Freitag",
    b"Samstag",
    b"Sonntag",
];
const MONTHS: [&[u8]; 12] = [
    b"Januar",
    b"Februar",
    b"M{rz",
    b"April",
    b"Mai",
    b"Juni",
    b"Juli",
    b"August",
    b"September",
    b"Oktober",
    b"November",
    b"Dezember",
];

/// Submenu base icon (`DS:0279 + si`) and icon count (`DS:027f + si`) per main
/// icon 1..6.
const SUB_BASE: [i16; 7] = [0, 0x00, 0x06, 0x0f, 0x15, 0x19, 0x00];
const SUB_COUNT: [i16; 7] = [0, 0x06, 0x09, 0x06, 0x04, 0x05, 0x00];

/// The competition-of-the-day banner and what it implies for the lineup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Banner {
    pub text: Vec<u8>,
    /// `FAR:513e`: a cup match is played today.
    pub cup_day: bool,
    /// `FAR:56ee`: highest lineup slot (bench size).
    pub max_lineup_slot: i8,
}

/// Selects the banner text of the office (`arg == 0xff` forces the season end).
/// `rescheduled` is `has_rescheduled_match_today`, evaluated only where the
/// original calls it.
///
/// Origin: BMMAIN.EXE 18bc:0b5f (office_menu_dispatcher, banner block)
pub fn office_banner(
    state: &mut GameState,
    arg: u8,
    fmt: NumberFormat,
    rescheduled: impl FnOnce(&mut GameState) -> bool,
) -> Banner {
    let cur = state.current_manager as usize;
    let m = state.managers[cur.min(3)].clone();
    let tier = m.league as usize;
    let wk = state.week_competitions[state.season_week as usize % 95];
    let mut cup_day = false;
    let mut max_lineup_slot = 15 - 2 * i8::from(GAME_OPTION_A == 4);
    let text = if arg == 0xff
        || (i32::from(SEASON_START_DAY) + 0x14 == state.day_of_month && state.month == 5)
    {
        SEASON_END.to_vec()
    } else if tier < 3 && (wk >> tier) & 1 != 0 {
        let round = state.competition_rounds[tier];
        let mut buf = format_long_thousands(i32::from(round as i8), fmt);
        append_ordinal_suffix(&mut buf, round, GAME_OPTION_A, GAME_OPTION_B);
        buf.extend_from_slice(MATCHDAY);
        buf
    } else if state.event_state[0] == m.values_132[0] && wk == 8 {
        cup_day = true;
        CUP.to_vec()
    } else if (m.team == state.league_teams[0][15] || m.team == state.league_teams[1][2])
        && wk == 0x10
    {
        RELEGATION.to_vec()
    } else {
        let euro = (1..4).any(|i| state.event_state[i] == m.values_132[i]);
        if euro && wk & 0x70 != 0 && wk != 0x10 {
            max_lineup_slot = 15;
            cup_day = true;
            EURO_CUP.to_vec()
        } else if rescheduled(state) && wk & RESCHEDULED_GAMES_DAY != 0 {
            RESCHEDULED.to_vec()
        } else {
            NO_MATCH_TODAY.to_vec()
        }
    };
    Banner {
        text,
        cup_day,
        max_lineup_slot,
    }
}

/// `M[cur].b[0x10b + i]` for the bytes the curve reads (the position history and
/// the byte after it).
fn manager_byte_10b(m: &Manager, i: usize) -> u8 {
    match i {
        0..=37 => m.position_history[i],
        38 => m.value_131,
        _ => 0,
    }
}

/// One `ui_draw_line(rp, x0, y0, x1, y1, color)` call.
pub type Line = (i16, u8, i16, u8, u8);

/// The league-position curve: x 0x3f..0xb1, y = 0x40 + table position (0-based),
/// one point per `116 / (games - 1)` pixels, computed in single precision like
/// the 8087-emulator code. `history(i)` is `M[cur].b[0x10b + i]`.
///
/// Origin: BMMAIN.EXE 18bc:0b5f (office_menu_dispatcher, curve at 18bc:1297)
pub fn office_curve(games: i8, day_counter: i32, history: impl Fn(usize) -> u8) -> Vec<Line> {
    let step: f32 = if games > 1 {
        (116.0f64 / f64::from(games as i16 - 1)) as f32
    } else {
        116.0
    };
    let mut acc: f32 = 0.0;
    let mut px: i16 = 0x3f;
    let mut py: i16 = if games > 1 {
        i16::from(history(0)) + 0x40
    } else {
        0x4a
    };
    let mut k = 0usize;
    let mut lines = Vec::new();
    let next = |k: usize| i16::from(history(1 + k)) + 0x41;
    for i in 0..0x73i16 {
        if acc > step {
            acc = (f64::from(acc) - f64::from(step)) as f32;
            let y = next(k);
            lines.push((px, py as u8, i + 0x3e, y as u8, 0x0b));
            px = i + 0x3e;
            py = y;
            k += 1;
        }
        acc = (f64::from(acc) + 1.0) as f32;
    }
    if day_counter > 1 {
        lines.push((px, py as u8, 0x3e + 0x73, next(k) as u8, 0x0b));
    } else {
        lines.push((0x3f, 0x4a, 0xb1, 0x4a, 7));
    }
    lines
}

/// Weekday, month, year and the big day of month on the calendar sheet.
///
/// Origin: BMMAIN.EXE 1310:0fb6 (office_draw_date_panel)
pub fn office_draw_date_panel(host: &mut Host, state: &GameState) {
    host.number_no_separators = true;
    host.set_font(1);
    let weekday = WEEKDAYS[(state.day_counter.wrapping_add(5)).rem_euclid(7) as usize];
    let month = MONTHS[state.month.rem_euclid(12) as usize];
    let (g, rp) = (&mut host.g, &mut host.rp);
    ui_draw_text_centered_shadow(g, rp, weekday, 0xed, 0x138, 0x3c, 4, 7);
    ui_draw_text_centered_shadow(g, rp, month, 0xed, 0x138, 0x5a, 4, 7);
    host.number_pad = 1;
    let year = format_long_thousands(state.year, host.number_format());
    ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, &year, 0xed, 0x138, 0x64, 4, 7);
    host.set_font(3);
    let mut day = format_long_thousands(state.day_of_month, host.number_format());
    if GAME_OPTION_B != 3 {
        append_ordinal_suffix(
            &mut day,
            state.day_of_month as u8,
            GAME_OPTION_A,
            GAME_OPTION_B,
        );
    }
    ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, &day, 0xf3, 0x130, 0x4f, 6, 8);
    host.set_font(1);
}

/// Keeps the lineup slots of the own squad within the bench size `limit`.
fn normalise_bench(state: &mut GameState, cur: usize, limit: i8) {
    let squad = &mut state.squads[cur];
    let n = squad[..24].iter().filter(|p| p.player_id != 0).count();
    let limit16 = i16::from(limit);
    let mut has_limit = false;
    let x = 0;
    for p in &squad[..n] {
        if i16::from(p.lineup_position) == limit16 {
            has_limit = true;
        }
    }
    for p in squad[..n].iter_mut() {
        if i16::from(p.lineup_position) - limit16 == 1 && !(x != 0 || has_limit) {
            p.lineup_position = limit as u8;
        }
    }
    for p in squad[..n].iter_mut() {
        if i16::from(p.lineup_position) > limit16 {
            p.lineup_position = 0;
        }
    }
}

/// Position of main icon `i` (0..5).
fn main_icon_xy(i: i16) -> (i16, i16) {
    (5 + 0x105 * i16::from(i > 2), 0x6e + 0x2b * (i % 3))
}

/// Draws main icon `i` (0..5) with the flags in `flags`.
fn draw_main_icon(host: &mut Host, i: i16, flags: u16) -> [i16; 4] {
    let (x, y) = main_icon_xy(i);
    host.ui
        .draw_button(
            &mut host.g,
            &mut host.rp,
            x,
            y,
            0,
            (i as u16 | flags) as i16,
            false,
            8,
        )
        .1
}

/// True when sub icon `si` (1-based) of main icon `sel` is disabled: the
/// training camp while its cooldown runs, the manager contract in the
/// scenarios 0x7ac/0x7c9, the new game with four managers
/// (18bc:1605..1662, 18bc:17a4..17e9).
fn sub_icon_disabled(state: &GameState, cur: usize, sel: i16, si: i16) -> bool {
    (sel == 3 && si == 5 && state.managers[cur].camp_cooldown != 0)
        || (sel == 1 && si == 6 && (state.scenario == 0x7ac || state.scenario == 0x7c9))
        || (sel == 5 && si == 5 && state.manager_count == 4)
}

/// Handles a click that hit no icon: end of turn (calendar sheet or auto play),
/// header bar (match-day info), desk (mail). Returns 1 at the end of the turn, 2
/// when the office has to be redrawn, else 0.
///
/// Origin: BMMAIN.EXE 18bc:08f2 (office_handle_free_click)
fn office_handle_free_click(host: &mut Host, session: &mut Session, sel: i16) -> Run<u8> {
    let auto = session.auto_play;
    if host.button != LEFT_DOWN && !auto {
        return Ok(0);
    }
    let (mx, my) = host.mouse_xy();
    if my == 0 && mx == 0 && auto {
        // TODO(#15): ui_dialog_box("Spiele nicht", "alleine !").
        session.auto_play = false;
        return Ok(0);
    }
    if !session.save_disabled && (auto || (mx > 0xf2 && my > 0x31 && mx < 0x132 && my < 0x66)) {
        host.screen_fade(0)?;
        host.ui.redraw_buttons(&mut host.g, &mut host.rp, 0x7f, 0);
        return Ok(1);
    }
    if my == 1 && mx == 1 {
        // TODO(#15): ui_dialog_box("Spiele automatisch", "weiter !"); DS:063a = 0x47.
        session.auto_play = true;
        return Ok(0);
    }
    if my < 0x2c {
        host.screen_fade(0)?;
        // TODO(#15): matchday_info_panel 1cb5:2bf9.
        host.ui.redraw_buttons(&mut host.g, &mut host.rp, 0x7f, 0);
        return Ok(2);
    }
    if sel == -1 && mx > 0x39 && my > 0x71 && mx < 0x108 && my < 0xfd {
        // TODO(#32): screen_messages 4091:05c4 (mail).
    }
    Ok(0)
}

/// One office turn of the current manager. Returns when the turn ends.
///
/// TODO(#15): only the "Tabellen" screens and the squad list behind the sub
/// icons (§6.5) are ported; choosing another one redraws the office and
/// reopens the submenu.
///
/// Origin: BMMAIN.EXE 18bc:0b5f (office_menu_dispatcher)
pub fn office_menu_dispatcher(
    host: &mut Host,
    session: &mut Session,
    pics: &Pictures,
    bitmaps: &OfficeBitmaps,
    state: &mut GameState,
    arg: u8,
) -> Run<()> {
    let mut reopen: i16 = 0;
    let mut warned = false;
    // DS:5256 (autosave request) is 0: no autosave here.
    state.saved_manager = state.current_manager;
    let mut day_saved = state.day_counter;
    host.screen_fade(0)?;
    host.number_no_separators = true;
    for (t, &rounds) in LEAGUE_ROUND_COUNT.iter().enumerate() {
        if (state.competition_rounds[t] as i8) < rounds as i8 {
            let round = state.competition_rounds[t];
            fixtures_build_round_pairings(state, t, round);
            // DS:224d == 0 in the shipped data: the games played are a sort key.
            league_apply_round_and_sort(state, &mut session.rng, t, RoundMode::Snapshot, 1, false);
        }
    }
    let mut ret: u8 = 0;
    'redraw: loop {
        let cur = (state.current_manager as usize).min(3);
        let team = state.managers[cur].team as usize;
        let games = state
            .team_tables
            .get(team)
            .map_or(0, |t| t.played[0].wrapping_add(t.played[1])) as i8;
        office_show_cached_background(host)?;
        let mut sel: i16 = -1;
        let screen = host.screen();
        Gfx::rastport_set_draw_mode(&mut host.rp, 0);
        host.g
            .blit_bitmap(bitmaps.header, 0, 0, screen, 0, -3, 0x140, 0x2c);
        host.g
            .blit_bitmap(bitmaps.desk, 0, 0, screen, 0, 0x29, 0x13f, 0x45);
        let c = cur as i16;
        host.g.blit_bitmap(
            pics.portraits,
            c * 0x29 + 1,
            0,
            screen,
            0x10,
            0x3b,
            0x27,
            0x28,
        );
        host.g.blit_bitmap(
            pics.crest_cache,
            c * 0x29,
            0,
            screen,
            0xbb,
            0x3c,
            0x26,
            0x26,
        );
        host.set_font(1);
        host.number_pad = 1;
        let week = state.season_week;
        let fmt = host.number_format();
        let banner = office_banner(state, arg, fmt, |s| has_rescheduled_match_today(s, week));
        normalise_bench(state, cur, banner.max_lineup_slot);
        let rules = LineupRules {
            suspended_allowed: banner.cup_day,
            max_lineup_slot: banner.max_lineup_slot,
        };
        session.rt.lineup_rules = rules;
        auto_lineup(state, rules);
        let name = state.managers[cur].name().to_vec();
        let (g, rp) = (&mut host.g, &mut host.rp);
        ui_draw_text_centered_shadow(g, rp, &banner.text, 0x4a, 0xa5, 0x3a, 0, 4);
        ui_draw_text_centered_shadow(g, rp, &name, 0x3e, 0xb2, 0x62, 0, 4);
        office_draw_date_panel(host, state);
        let mut main_rect = [[0i16; 4]; 6];
        for i in 0..3i16 {
            main_rect[i as usize] = draw_main_icon(host, i, 0);
            main_rect[i as usize + 3] = draw_main_icon(host, i + 3, 0);
        }
        state.day_counter = day_saved;
        let m = &state.managers[cur];
        for (x0, y0, x1, y1, color) in
            office_curve(games, state.day_counter, |i| manager_byte_10b(m, i))
        {
            ui_draw_line(&mut host.g, &mut host.rp, x0, y0, x1, y1, color);
        }
        if state.far_56e2[cur] != 0 {
            state.far_56e2[cur] = 0;
            // TODO(#32): screen_messages 4091:05c4 (unread mail).
        }
        if reopen == 0 {
            host.screen_fade(1)?;
        }
        if !warned {
            // TODO(#31): season_goal_messages 212a:314d.
            warned = true;
        }
        'main: loop {
            let si = if reopen != 0 {
                reopen
            } else {
                let ev = host.input_poll_mouse()?;
                let (mx, my) = host.mouse_xy();
                let si = Ui::hit_test_buttons(&main_rect, 6, mx, my, Some(ev as i16));
                if si == 0 {
                    if host.button == RIGHT_DOWN {
                        if Ui::hit_test_buttons(&main_rect, 6, mx, my, None) == 6 {
                            quit_game_confirm(host, session, state)?;
                        }
                    } else {
                        ret = office_handle_free_click(host, session, sel)?;
                    }
                    match ret {
                        1 => return Ok(()),
                        2 => continue 'redraw,
                        _ => continue 'main,
                    }
                }
                si
            };
            if si >= 6 || si == sel {
                continue;
            }
            let quick = reopen != 0 && sel != -1;
            // §6.4: open the submenu of main icon `si`.
            if sel >= 1 {
                draw_main_icon(host, sel - 1, 0x2000);
            }
            draw_main_icon(host, si - 1, 0xa000);
            let mut rects = [[0i16; 4]; 15];
            let mut cnt: i16 = 0;
            for col in 0..3i16 {
                for row in 0..3i16 {
                    let mut icon = if cnt == 0x63 {
                        0x1023
                    } else {
                        cnt + SUB_BASE[si as usize]
                    };
                    if icon == 0x1c {
                        icon = 0x5e;
                    }
                    if icon == 0x1d {
                        icon = 0x90;
                    }
                    let (x, y) = (col * 0x36 + 0x52, row * 0x2a + 0x70);
                    let code = (i16::from(quick) << 14) + icon + 6;
                    let r = host
                        .ui
                        .draw_button(&mut host.g, &mut host.rp, x, y, 0, code, false, 8)
                        .1;
                    rects[(col * 3 + row) as usize] = r;
                    if cnt != 0x63 && sub_icon_disabled(state, cur, si, cnt + 1) {
                        host.ui
                            .draw_button(&mut host.g, &mut host.rp, x, y, 0, 0x4029, false, 8);
                    }
                    if cnt != 0x63 && SUB_COUNT[si as usize] - 1 > cnt {
                        cnt += 1;
                    } else {
                        cnt = 0x63;
                    }
                }
            }
            sel = si;
            rects[9..].copy_from_slice(&main_rect);
            if reopen != 0 {
                host.screen_fade(1)?;
            }
            loop {
                let ev = host.input_poll_mouse()?;
                let (mx, my) = host.mouse_xy();
                let si = Ui::hit_test_buttons(&rects, 15, mx, my, Some(ev as i16));
                let valid = si != 0
                    && si - sel != 9
                    && !(SUB_COUNT[sel as usize] < si && si < 10)
                    && si != 15
                    && !sub_icon_disabled(state, cur, sel, si);
                if !valid {
                    if host.button == RIGHT_DOWN {
                        if Ui::hit_test_buttons(&rects, 15, mx, my, None) != 15 {
                            if let Some(work) = host.ui.work {
                                host.g.blit_bitmap_or_restore_page2(
                                    work, 0x39, 0x70, screen, 0x39, 0x70, 0xce, 0x89,
                                );
                            }
                            if sel >= 1 {
                                draw_main_icon(host, sel - 1, 0);
                            }
                            reopen = 0;
                            continue 'main;
                        }
                        quit_game_confirm(host, session, state)?;
                    }
                    ret = office_handle_free_click(host, session, sel)?;
                    match ret {
                        1 => return Ok(()),
                        2 => continue 'redraw,
                        _ => continue,
                    }
                }
                reopen = 0;
                if si > 9 {
                    reopen = si - 9;
                    draw_main_icon(host, sel - 1, 0);
                    continue 'main;
                }
                let k = si - 1;
                ui_wait_right_click_button(
                    host,
                    k / 3 * 0x36 + 0x52,
                    k % 3 * 0x2a + 0x70,
                    session.auto_play,
                )?;
                host.input_wait_release()?;
                office_show_cached_background(host)?;
                host.ui.redraw_buttons(&mut host.g, &mut host.rp, 0x7f, 0);
                match (sel, si) {
                    (1, 1) => screen_statistics(host, session, state)?,
                    (1, 2) => screen_eternal_table(host, session, state)?,
                    (1, 3) => screen_manager_history(host, session, state)?,
                    (1, 4) => screen_stadium(host, session, state)?,
                    (1, 5) => screen_credits(host, session, pics, state)?,
                    // office_free_bitmaps(3) / office_load_bitmaps(3) around the
                    // call only free memory; the port keeps the bitmaps.
                    (1, 6) => screen_manager_contract(host, session, state)?,
                    (2, 1..=3) => {
                        screen_league_table(host, session, state, (si - 1) as usize, 1)?;
                    }
                    (2, 4..=6) => {
                        screen_fixtures(host, session, state, (si - 4) as usize, 0, 0, true)?;
                    }
                    (2, 7..=9) => {
                        screen_strength_table(host, session, state, (si - 7) as usize)?;
                    }
                    (3, 1) => {
                        screen_squad_lineup(host, session, state, rules, 0, 0xff, None)?;
                    }
                    (3, 2) => {
                        screen_squad_lineup(host, session, state, rules, 0x63, 0xff, None)?;
                    }
                    (3, 3) => screen_transfer_market(host, session, state, rules)?,
                    (3, 4) => screen_training(host, session, state)?,
                    // office_free_bitmaps(3) / office_load_bitmaps(3) around the
                    // call only free memory.
                    (3, 5) => screen_training_camp(host, session, state)?,
                    (3, 6) => {
                        screen_top_scorers(host, session, state, false)?;
                    }
                    (4, 1..=4) => screen_cup_matches(host, session, state, (si - 1) as u8, 0)?,
                    (5, 1) => {
                        if let Some(loaded) = screen_load_game(host, session, pics, 0)? {
                            *state = *loaded;
                            day_saved = state.day_counter;
                            warned = false;
                        }
                    }
                    (5, 2) if !session.save_disabled => {
                        screen_save_game(host, session, state, b"")?
                    }
                    (5, 3) if !HIGHSCORES_DISABLED => {
                        highscore_screen(host, session, state, HighscoreMode::Preview)?;
                    }
                    (5, 4) => screen_settings(host, session, state)?,
                    _ => {}
                }
                // TODO(#15): (5, 5) is start_menu_new_or_load(0), adding managers.
                // TODO(#15): the other screens of flow.md §6.5.
                host.screen_fade(0)?;
                host.g.palette_load_slot(0, 0x100);
                host.ui.redraw_buttons(&mut host.g, &mut host.rp, 0x7f, 0);
                if !(sel == 5 && si <= 2) {
                    reopen = sel;
                }
                calendar_compute_date(state, 0);
                state.current_manager = state.saved_manager;
                continue 'redraw;
            }
        }
    }
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

    fn state_with(wk: u8, tier: u8) -> GameState {
        let mut s = GameState::zeroed();
        s.manager_count = 1;
        s.managers[0].league = tier;
        s.managers[0].team = 36;
        s.managers[0].values_132 = [1, 30, 30, 30, 0];
        s.event_state = [3, 3, 3, 3, 0, 0, 0, 0];
        s.season_week = 25;
        s.week_competitions[25] = wk;
        s.competition_rounds = [14, 15, 15];
        s.day_of_month = 25;
        s.month = 9;
        s.league_teams[0][15] = 99;
        s.league_teams[1][2] = 98;
        s
    }

    fn banner(s: &GameState, arg: u8, rescheduled: bool) -> Banner {
        office_banner(&mut s.clone(), arg, fmt(), |_| rescheduled)
    }

    #[test]
    fn banner_shows_the_league_round_of_the_own_tier() {
        // 07072019.MAN: week mask 6, second tier, round 15.
        let b = banner(&state_with(6, 1), 25, false);
        assert_eq!(b.text, b"15. Spieltag");
        assert!(!b.cup_day);
        assert_eq!(b.max_lineup_slot, 15);
        // The first tier does not play in this week.
        assert_eq!(banner(&state_with(6, 0), 25, false).text, b"Spielfrei");
    }

    #[test]
    fn banner_season_end_and_cups() {
        assert_eq!(banner(&state_with(6, 1), 0xff, false).text, b"SAISONENDE");
        let mut s = state_with(0, 1);
        s.day_of_month = 20;
        s.month = 5;
        assert_eq!(banner(&s, 0, false).text, b"SAISONENDE");
        let mut s = state_with(8, 1);
        s.event_state[0] = 1;
        let b = banner(&s, 0, false);
        assert_eq!((b.text.as_slice(), b.cup_day), (CUP, true));
        let mut s = state_with(0x10, 1);
        s.league_teams[1][2] = 36;
        assert_eq!(banner(&s, 0, false).text, RELEGATION);
        let mut s = state_with(0x70, 1);
        s.event_state[2] = 30;
        let b = banner(&s, 0, false);
        assert_eq!((b.text.as_slice(), b.cup_day), (EURO_CUP, true));
        assert_eq!(
            banner(&state_with(RESCHEDULED_GAMES_DAY, 1), 0, true).text,
            RESCHEDULED
        );
        assert_eq!(
            banner(&state_with(RESCHEDULED_GAMES_DAY, 1), 0, false).text,
            NO_MATCH_TODAY
        );
    }

    #[test]
    fn curve_spreads_the_games_over_116_pixels() {
        let hist = |i: usize| i as u8;
        let lines = office_curve(14, 88, hist);
        // 12 segments inside the loop plus the closing one.
        assert_eq!(lines.len(), 13);
        assert_eq!(lines[0], (0x3f, 0x40, 71, 0x42, 0x0b));
        for w in lines.windows(2) {
            assert_eq!(w[0].2, w[1].0, "segments are connected");
            assert_eq!(w[0].3, w[1].1);
        }
        assert_eq!(lines.last().unwrap().2, 0xb1);
        // Single precision: 116/13 accumulates exactly like the 8087 code.
        let xs: Vec<i16> = lines.iter().map(|l| l.2).collect();
        assert_eq!(
            xs,
            [71, 80, 89, 98, 107, 116, 125, 134, 143, 152, 161, 170, 177]
        );
    }

    #[test]
    fn curve_without_games() {
        assert_eq!(office_curve(0, 1, |_| 0), vec![(0x3f, 0x4a, 0xb1, 0x4a, 7)]);
        assert_eq!(
            office_curve(1, 5, |_| 2),
            vec![(0x3f, 0x4a, 0xb1, 0x43, 0x0b)]
        );
    }
}
