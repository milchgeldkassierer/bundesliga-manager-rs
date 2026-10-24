//! The live match view of a match day (segments 1310 and 2a58): the title, the score panels
//! of the human clubs' matches, the time bar with the minute ticker, the event panel of
//! chances, goals, cards and injuries, and the result screens after each period. The match
//! itself is played by [`bmp_game::matchsim`]; these functions are its display hooks.
//! Spec: `docs/re/logic/match.md` §2 and `docs/re/logic/season.md` §3.7-3.10.

use bmp_game::matchsim::TickerEvent;
use bmp_game::news::{EVENT_CHANCE, EVENT_GOAL, EVENT_RED_CARD, EVENT_YELLOW_CARD, OPPONENT};
use bmp_game::rng::{NumberFormat, Rng, format_long_thousands};
use bmp_game::runtime::Runtime;
use bmp_game::squad::squad_count_starters;
use bmp_game::state::{GameState, Layout, NewsRecord};
use bmp_game::stats::INJURY_NAMES;

use super::cup_screens::screen_cup_matches;
use super::season::ScreenSeason;
use super::squad_screen::{MatchSubs, screen_squad_lineup};
use super::stats_screens::CUP_NAMES;
use super::tables::{screen_fixtures, screen_league_table};
use super::{GAME_OPTION_A, GAME_OPTION_B, append_ordinal_suffix, office_show_cached_background};
use crate::gfx::{
    BitmapId, Gfx, ui_draw_frame, ui_draw_line, ui_draw_text_at, ui_draw_text_centered_shadow,
    ui_fill_rect,
};
use crate::host::{Host, LEFT_DOWN, Run};

/// `DS:0632` / `DS:063c`: the origins of the four score panels.
const PANEL_X: [i16; 4] = [0, 0xa1, 0, 0xa1];
const PANEL_Y: [i16; 4] = [0, 0, 0x73, 0x73];
/// `DS:008a`: where the ball marker of the time bar starts in each half.
const BALL_START: [i16; 2] = [0x24, 0x11d];
/// `DS:00a2`: the frame lines of the time bar (`x0, y0, x1, y1`); the first four in colour 1,
/// the rest in colour 7.
const TIME_BAR_LINES: [(i16, u8, i16, u8); 8] = [
    (0x23, 0xea, 0x7d, 0xea),
    (0x23, 0xeb, 0x23, 0xed),
    (0xc2, 0xea, 0x11e, 0xea),
    (0xc2, 0xeb, 0xc2, 0xed),
    (0x7e, 0xea, 0x7e, 0xed),
    (0x24, 0xed, 0x7d, 0xed),
    (0x11f, 0xea, 0x11f, 0xed),
    (0xc3, 0xed, 0x11e, 0xed),
];
/// `DS:25d4`, `DS:26c8`.
const MINUTE: &[u8] = b"Minute";
const PENALTY: &[u8] = b"ELFMETER";
/// `DS:26dc`: the attendance of an away match.
const AWAY: &[u8] = b"(AUSW.)";
/// `DS:4bdc`, `DS:4be0`, `DS:4be4`, `DS:4be8`, `DS:4bec`, `DS:4bf0`.
const PENALTY_FOR: &[u8] = b"Elfmeter f]r ";
const CHANCE_FOR: &[u8] = b"Chance f]r ";
const SCORER: &[u8] = b"Torsch}tze: ";
const MISSED: &[u8] = b"Chance vergeben: ";
const OWN_GOAL: &[u8] = b"Eigentor !";
const ASSIST: &[u8] = b"nach Vorlage von ";
/// `DS:26e0`, `DS:26e4`, `DS:26e8`, `DS:26ec`.
const RED_CARD: &[u8] = b"Rote Karte: ";
const YELLOW_RED_CARD: &[u8] = b"Gelb-Rote Karte: ";
const YELLOW_CARD: &[u8] = b"Gelbe Karte: ";
const INJURED: &[u8] = b"Verletzt: ";
/// `DS:063a` values from which the minute waits are skipped.
const FAST_SPEED: u16 = 0x47;
/// [`TickerEvent::kind`] of an injury.
const EVENT_INJURY: u8 = 4;
/// `DS:05fe[3 * L + k]`: the option rows of league tier `L` (half-time, full-time fixtures,
/// table), `DS:0608..060a` the cup and rescheduled-games rows.
const OPTION_HALF_TIME: usize = 0;
const OPTION_FULL_TIME: usize = 1;
const OPTION_TABLE: usize = 2;
const OPTION_DFB_CUP: usize = 10;
const OPTION_EURO_CUPS: usize = 11;
const OPTION_RESCHEDULED: usize = 12;

/// The live view's globals of one match day.
#[derive(Default)]
pub struct LiveView {
    /// `FAR:57c4`: `PIC\38.VGA`, the score panel with the digits.
    panel: Option<BitmapId>,
    /// `FAR:2e9e`: `PIC\8.VGA`, the pitch and the event sprites.
    sprites: Option<BitmapId>,
    /// `DS:05b4`: the 152x38 cache of the computer clubs' crests.
    badges: Option<BitmapId>,
    /// `FAR:4bf1`: the crest cache is filled.
    badges_valid: bool,
    /// `FAR:5140 + 6 m`: yellow, red, injured, players, chances (home side), chances (away side)
    /// of manager `m`'s panel.
    counters: [[u8; 6]; 4],
    /// `FAR:5350`: the home club of each score panel.
    slots: [u8; 4],
    /// `FAR:4c6c + 2 k`: the home and away club of score panel `k` (0xff: no panel), written
    /// by `match_draw_live_screen` and read by the panel click.
    panel_teams: [[u8; 2]; 4],
    /// The cup mask of the running period (`match_live_period`'s `cmask`).
    cup_mask: u8,
    /// `FAR:2f22`: the half of the time bar (0 first, 1 second).
    half: u8,
    /// `FAR:2f18`: the direction of the ball marker.
    dir: i16,
    /// `FAR:2f12`: the x of the ball marker (0: none).
    ball_x: i16,
    /// `FAR:3057`: the current minute.
    minute: u8,
    /// `FAR:2e99` of the running period: it is shown live.
    shown: bool,
    /// The running period's end screen was started (crowd stopped, screen cleared).
    closed: bool,
    /// The y of the event panel to clear (`match_event_redraw`), 0 = none.
    event_y: i16,
    /// The injury whose panel waits for its text ([`injury_and_substitution`]).
    pending_injury: Option<TickerEvent>,
}

impl LiveView {
    /// Frees the sprites (`PIC\\8.VGA`) and the score panel (`PIC\\38.VGA`) for a goal scene.
    pub(crate) fn free_pictures(&mut self, g: &mut Gfx) {
        for id in [self.sprites.take(), self.panel.take()]
            .into_iter()
            .flatten()
        {
            g.free(id);
        }
    }

    /// Loads the sprites and the score panel again after a goal scene.
    pub(crate) fn load_pictures(&mut self, host: &mut Host) {
        self.sprites = host.load_picture(8).ok();
        self.panel = host.load_picture(0x26).ok();
    }

    /// The y of the running event's panel (`ybase`), 0 without one.
    pub(crate) fn event_base(&self) -> i16 {
        (self.event_y - 1).max(0)
    }
}

fn fill(host: &mut Host, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    ui_fill_rect(&mut host.g, &mut host.rp, x0, y0, x1, y1, color);
}

fn centered(host: &mut Host, text: &[u8], x0: i16, x1: i16, y: i16, color: u8, shadow: u8) {
    ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, text, x0, x1, y, color, shadow);
}

fn frame(host: &mut Host, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    ui_draw_frame(&mut host.g, &mut host.rp, x0, y0, x1, y1, color);
}

#[allow(clippy::too_many_arguments)]
fn blit(
    host: &mut Host,
    src: Option<BitmapId>,
    sx: i16,
    sy: i16,
    dx: i16,
    dy: i16,
    w: i16,
    h: i16,
) {
    if let Some(src) = src {
        let screen = host.screen();
        host.g.blit_bitmap(src, sx, sy, screen, dx, dy, w, h);
    }
}

fn delay(host: &mut Host, auto: bool, ticks: i32) -> Run<()> {
    if !auto {
        host.delay_ticks(ticks)?;
    }
    Ok(())
}

/// The whistle or the injury sound, then the crowd again (2a58:1198).
fn voice(host: &mut Host, n: u8) -> Run<()> {
    host.voice_play(n);
    host.voice_wait_end()?;
    host.voice_restart_loop();
    Ok(())
}

/// Clears the screen for a match-day title: the German flag bands when the game variant
/// options agree (the DM version), else black; then font 3.
///
/// Origin: BMMAIN.EXE 1310:00fa (match_screen_clear_background)
pub fn match_screen_clear_background(host: &mut Host) {
    let eq = GAME_OPTION_A == GAME_OPTION_B;
    host.g.rastport_clear(&host.rp, if eq { 0x0b } else { 0 });
    if eq {
        fill(host, 1, 1, 0x13e, 0x4f, 0);
        fill(host, 1, 0x50, 0x13e, 0xa0, 0x12);
        fill(host, 1, 0xa1, 0x13e, 0xee, 0x13);
    }
    host.set_font(3);
}

/// A match-day title in the centre of the flag, shown for 50 ticks.
///
/// Origin: BMMAIN.EXE 1310:01f2 (matchday_show_title)
pub fn matchday_show_title(host: &mut Host, title: &[u8], auto: bool) -> Run<()> {
    match_screen_clear_background(host);
    centered(host, title, 0, 0x13f, 0x80, 0x1d, 0);
    host.screen_fade(1)?;
    delay(host, auto, 50)
}

/// The minute ticker under the time bar (`y` 0xef) or in an event panel: "8.Minute", or
/// "ELFMETER" for a penalty; under the time bar the ball marker is plotted.
///
/// Origin: BMMAIN.EXE 1310:2086 (match_ticker_minute)
fn match_ticker_minute(host: &mut Host, live: &LiveView, minute: u8, y: i16, penalty: bool) {
    host.set_font(1);
    fill(host, 0x7f, y - 6, 0xc1, y, 0);
    if minute != 0 {
        let text = if penalty {
            PENALTY.to_vec()
        } else {
            let mut t = minute.to_string().into_bytes();
            append_ordinal_suffix(&mut t, minute, GAME_OPTION_A, GAME_OPTION_B);
            t.extend_from_slice(MINUTE);
            t
        };
        centered(host, &text, 4, 0x140, y, 0x1d, 4);
    }
    if live.ball_x != 0 && y == 0xef && !penalty {
        let (g, rp) = (&mut host.g, &mut host.rp);
        for (yy, c) in [(0xeb, 0x17), (0xec, 0x1d)] {
            Gfx::rastport_set_color(rp, c);
            g.rastport_put_pixel(rp, live.ball_x, yy);
            g.rastport_put_pixel(rp, live.ball_x + 1, yy);
        }
    }
}

/// The time bar at the bottom: the two halves' frames, the played part and the ball marker,
/// then the minute ticker.
///
/// Origin: BMMAIN.EXE 1310:1def (match_draw_time_bar)
fn match_draw_time_bar(host: &mut Host, live: &LiveView, minute: u8) {
    fill(host, 0x24, 0xeb, 0x7d, 0xec, 4);
    fill(host, 0xc2, 0xeb, 0x11e, 0xec, 4);
    for (i, &(x0, y0, x1, y1)) in TIME_BAR_LINES.iter().enumerate() {
        let c = if i > 3 { 7 } else { 1 };
        ui_draw_line(&mut host.g, &mut host.rp, x0, y0, x1, y1, c);
    }
    let progress = |host: &mut Host, x0: i16, x1: i16| {
        ui_draw_line(&mut host.g, &mut host.rp, x0, 0xeb, x1, 0xeb, 0x17);
        ui_draw_line(&mut host.g, &mut host.rp, x0, 0xec, x1, 0xec, 0x1d);
    };
    if minute > 0x2d && minute < 0x5b {
        progress(host, 0x24, 0x7d);
    }
    if minute > 0x69 {
        progress(host, 0x24, 0x40);
    }
    if live.ball_x != 0 && (minute != 0x2e || live.half != 0) {
        progress(host, BALL_START[usize::from(live.half & 1)], live.ball_x);
    }
    match_ticker_minute(host, live, minute, 0xef, false);
}

/// A club name in the score panel, split at the first blank onto two lines when it is wider
/// than 60 pixels.
///
/// Origin: BMMAIN.EXE 2a58:0350 (draw_text_split_two_lines)
fn draw_text_split_two_lines(host: &mut Host, text: &[u8], x0: i16, x1: i16, y: i16) {
    let w = host.g.rastport_text_width(&host.rp, text) as u8;
    if w > 0x3c
        && let Some(i) = text.iter().position(|&c| c == b' ')
    {
        centered(host, &text[..i], x0, x1, y - 5, 1, 0);
        centered(host, &text[i + 1..], x0, x1, y + 1, 1, 0);
        return;
    }
    centered(host, text, x0, x1, y, 1, 0);
}

/// The event counters of manager `m` under the away (`side` 1) or home crest of the panel
/// `row`; the number of players on the pitch is refreshed first.
///
/// Origin: BMMAIN.EXE 2a58:024e (match_draw_manager_bar_stats)
fn match_draw_manager_bar_stats(
    host: &mut Host,
    live: &mut LiveView,
    state: &GameState,
    row: usize,
    m: Option<usize>,
    side: u8,
) {
    let Some(m) = m else { return };
    let side = i16::from(side.min(1));
    let (px, py) = (PANEL_X[row & 3], PANEL_Y[row & 3]);
    let x = side * 0x10 + px + 0x7e;
    fill(host, x, py + 0x4d, x + 10, py + 0x70, 0x0c);
    live.counters[m & 3][3] = squad_count_starters(state, m);
    let fmt = NumberFormat {
        pad_width: 1,
        ..host.number_format()
    };
    let mut y = py + 0x52;
    for i in 0..5usize {
        let k = if i == 4 && side != 0 { 5 } else { i };
        let text = format_long_thousands(i32::from(live.counters[m & 3][k]), fmt);
        ui_draw_text_at(&mut host.g, &mut host.rp, &text, x, y, 1);
        y += 7;
    }
}

/// The digits of the score of panel `row` (`cup_mode`: a cup match).
///
/// Origin: BMMAIN.EXE 2a58:009d (match_draw_score_digit_pair)
fn match_draw_score_digit_pair(
    host: &mut Host,
    live: &LiveView,
    state: &GameState,
    rt: &Runtime,
    row: usize,
    cup_mode: bool,
) {
    let Some(lm) = rt.live_matches.get(row) else {
        return;
    };
    let (x, y) = (PANEL_X[row & 3], PANEL_Y[row & 3]);
    let goals = |j: usize| -> u8 {
        if cup_mode {
            state.cup_results.as_flattened()[usize::from(lm.row) * 32 + usize::from(lm.slot) + j]
        } else {
            let round = state.competition_rounds[usize::from(lm.row)] as i8;
            let r = (i16::from(lm.row) * 38 + i16::from(round) - 1).clamp(0, 3 * 38 - 1);
            state.league_results.as_flattened().as_flattened()
                [r as usize * 20 + usize::from(lm.slot) + j]
        }
    };
    for (j, dx) in [(0, 0x24), (1, 0x72)] {
        let v = goals(j) % 10;
        blit(
            host,
            live.panel,
            i16::from(v) * 0xe,
            0x72,
            x + dx,
            y + 0x10,
            0xb,
            0xf,
        );
    }
}

/// The two clubs of live match `i`.
fn live_pairing(state: &GameState, rt: &Runtime, i: usize, cup_mode: bool) -> (u8, u8) {
    let lm = rt.live_matches[i];
    if cup_mode {
        let c = usize::from(lm.row) * 32 + usize::from(lm.slot);
        let p = state.cup_pairings.as_flattened();
        (p[c], p[c + 1])
    } else {
        let [h, a] = state.matchday_pairings[usize::from(lm.row)][usize::from(lm.slot / 2)];
        (h, a)
    }
}

/// Every manager whose club is `team`.
fn managers_of(state: &GameState, team: u8) -> impl Iterator<Item = usize> + '_ {
    let n = state.active_managers().len();
    (0..n).filter(move |&j| state.managers[j].team == team)
}

/// The whole live screen: per live match the score panel with both crests (a manager's club
/// shows the cached crest, a computer club its crest picture, cached after the first draw),
/// the club names, the attendance, the managers' counters and the score; then the time bar.
///
/// Origin: BMMAIN.EXE 2a58:042f (match_draw_live_screen), `only_slot` 0xff
fn match_draw_live_screen(
    ui: &mut ScreenSeason<'_>,
    state: &GameState,
    rt: &Runtime,
    cup_mode: bool,
) -> Run<()> {
    let crests = Some(ui.crest_cache());
    let host = &mut *ui.host;
    let live = &mut ui.live;
    host.screen_fade(0)?;
    host.g.rastport_clear(&host.rp, 0);
    let mut badge: u8 = 0;
    live.panel_teams = [[0xff; 2]; 4];
    for i in 0..rt.live_matches.len().min(4) {
        let (x, y) = (PANEL_X[i], PANEL_Y[i]);
        blit(host, live.panel, 0, 0, x + 3, y, 0x9b, 0x72);
        let (h, a) = live_pairing(state, rt, i, cup_mode);
        live.slots[i] = h;
        live.panel_teams[i] = [h, a];
        for (team, cx, nx0, nx1) in [
            (h, x + 0x16, x + 5, x + 0x4b),
            (a, x + 0x65, x + 0x55, x + 0x99),
        ] {
            let mut own = false;
            for j in managers_of(state, team) {
                own = true;
                if let Some(c) = crests {
                    let screen = host.screen();
                    host.g
                        .blit_bitmap(c, 0x29 * j as i16, 0, screen, cx, y + 0x23, 0x26, 0x26);
                }
            }
            if !own {
                let bx = i16::from(badge.wrapping_mul(0x26) as i8);
                if !live.badges_valid {
                    let pic = if team < 0x40 {
                        u16::from(state.clubs[usize::from(team)].crest & 0x7f) + 0x50
                    } else {
                        u16::from(rt.live_matches[i].row) + 0x90
                    };
                    if let Ok(bm) = host.load_picture(pic) {
                        let screen = host.screen();
                        host.g
                            .blit_bitmap(bm, 0, 0, screen, cx, y + 0x23, 0x26, 0x26);
                        if let Some(cache) = live.badges {
                            host.g.blit_bitmap(bm, 0, 0, cache, bx, 0, 0x26, 0x26);
                        }
                        host.g.free(bm);
                    }
                } else {
                    blit(host, live.badges, bx, 0, cx, y + 0x23, 0x26, 0x26);
                }
                badge = badge.wrapping_add(1);
            }
            host.set_font(2);
            let name = state.clubs[usize::from(team)].name().to_vec();
            draw_text_split_two_lines(host, &name, nx0, nx1, y + 0x0c);
        }
        let mut mi = state.manager_index_of_team(h, 0xfe);
        if mi == 0 {
            mi = state.manager_index_of_team(a, 0xfe);
        }
        let mut v = if mi != 0 {
            rt.news_records[usize::from(mi - 1) & 3].spectators
        } else {
            0
        };
        if v > 200_000 {
            v -= 200_000;
        }
        let mut col = 1;
        let text = if v != 0 {
            let fmt = NumberFormat {
                no_separators: false,
                ..host.number_format()
            };
            let m = &state.managers[usize::from(mi - 1) & 3];
            if m.stadium[0][0].wrapping_add(m.stadium[1][0]) == v {
                col = 0x11;
            }
            format_long_thousands(v, fmt)
        } else {
            AWAY.to_vec()
        };
        centered(host, &text, x + 9, x + 0x33, y + 0x63, col, 0);
        for j in managers_of(state, h).collect::<Vec<_>>() {
            match_draw_manager_bar_stats(host, live, state, i, Some(j), 0);
        }
        for j in managers_of(state, a).collect::<Vec<_>>() {
            match_draw_manager_bar_stats(host, live, state, i, Some(j), 1);
        }
        match_draw_score_digit_pair(host, live, state, rt, i, cup_mode);
    }
    live.badges_valid = true;
    let minute = live.minute;
    match_draw_time_bar(host, live, minute);
    host.screen_fade(1)
}

/// The start of a period: on the first one of the match day the pictures are loaded and the
/// live state is reset; a period shown live draws the score panels and the time bar, moves the
/// ball marker to its half and starts the crowd.
///
/// Origin: BMMAIN.EXE 1310:15db (matchday_play, the pictures) and 1310:2303
/// (match_live_period, 1310:2489..24ef)
pub fn period_start(
    ui: &mut ScreenSeason<'_>,
    state: &mut GameState,
    rt: &mut Runtime,
    _league_mask: u8,
    cup_mask: u8,
    from: u8,
) -> Run<()> {
    if from == 1 {
        // office_free_bitmaps(1) only frees memory; the port keeps the office bitmaps.
        let host = &mut *ui.host;
        let live = &mut ui.live;
        if live.panel.is_none() {
            live.panel = host.load_picture(0x26).ok();
            live.sprites = host.load_picture(8).ok();
            live.badges = Some(host.blank_bitmap(0x98, 0x26, 0));
        }
        live.badges_valid = false;
        live.counters = [[0; 6]; 4];
        live.slots = [0xff; 4];
        live.dir = -1;
        live.ball_x = 0;
        live.half = 1;
    }
    ui.live.closed = false;
    ui.live.cup_mask = cup_mask;
    ui.live.shown = rt.live_match_shown != 0;
    if !ui.live.shown {
        ui.host.screen_fade(1)?;
    } else {
        match_draw_live_screen(ui, state, rt, cup_mask != 0)?;
        state.ticker_slots = ui.live.slots;
        let live = &mut ui.live;
        live.half = (live.half + 1) % 2;
        live.dir = -live.dir;
        live.ball_x = BALL_START[usize::from(live.half)];
        let minute = from;
        match_draw_time_bar(ui.host, &ui.live, minute);
        ui.host.voice_play(1);
    }
    ui.host.input_wait_release()
}

/// The start of `minute` of a period shown live: the wait of the game speed, the minute
/// ticker and the ball marker.
///
/// Origin: BMMAIN.EXE 1310:2303 (match_live_period, 1310:24f9..2550)
pub fn period_minute(
    ui: &mut ScreenSeason<'_>,
    state: &GameState,
    _rt: &Runtime,
    to: u8,
    minute: u8,
) -> Run<()> {
    if !ui.live.shown {
        return Ok(());
    }
    let speed = state.match_options;
    if !ui.session.auto_play && speed < FAST_SPEED {
        ui.host.delay_ticks(0x26 - i32::from(speed >> 1))?;
    }
    match_ticker_minute(ui.host, &ui.live, minute, 0xef, false);
    if minute < to {
        ui.live.ball_x += 2 * ui.live.dir;
    }
    ui.live.minute = minute;
    Ok(())
}

/// The input poll of `minute`: the mouse event and a key are read (in every minute, shown
/// live or not). The space key or a left click with the pointer at (0, 0) opens the settings
/// screen over the office background ("Einstellungen", where the copy protection may draw
/// from `rng`); the live screen is drawn again afterwards and the engine skips the minute.
/// Returns whether the minute was paused.
///
/// Origin: BMMAIN.EXE 1310:2303 (match_live_period, 1310:2550..25ae)
pub fn match_pause_poll(
    ui: &mut ScreenSeason<'_>,
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
) -> Run<bool> {
    let button = ui.host.input_poll_mouse()?;
    let key = ui.host.read_key_nowait();
    if key != b' ' && !(ui.host.mouse_xy() == (0, 0) && button == LEFT_DOWN) {
        return Ok(false);
    }
    office_show_cached_background(ui.host)?;
    ui.lend(rng, rt, |ui| {
        super::disk::screen_settings(ui.host, ui.session, state)
    })?;
    let cup_mode = ui.live.cup_mask != 0;
    match_draw_live_screen(ui, state, rt, cup_mode)?;
    ui.host.screen_fade(1)?;
    Ok(true)
}

/// The manager whose lineup a left click at (`mx`, `my`) opens while pairing `home` vs
/// `away` is played, or `None`. Score panel `k` (scanned while its home club is not 0xff;
/// panel 0 is always tested) has its origin at (`DS:0632[k]`, `DS:063c[k]`); the click has to
/// lie strictly inside the crest row `y + 0x22 .. y + 0x49` and in the home half
/// `x + 0x22 .. x + 0x4c` or the away half `x + 0x63 .. x + 0x8f`. The home club's manager is
/// taken unless the click lies right of `x + 0x63` or the home club has none; then the away
/// club's manager. He has to manage `home` or `away`.
///
/// Origin: BMMAIN.EXE 1310:3314 (match_handle_panel_click, the hit test)
pub(crate) fn panel_click_manager(
    state: &GameState,
    panel_teams: &[[u8; 2]; 4],
    mx: i16,
    my: i16,
    home: u8,
    away: u8,
) -> Option<u8> {
    for (k, &[ph, pa]) in panel_teams.iter().enumerate() {
        if k > 0 && ph == 0xff {
            break;
        }
        let (x, y) = (PANEL_X[k], PANEL_Y[k]);
        // mouse_state[0] (y) is tested against DS:063c, mouse_state[1] (x) against DS:0632.
        let row = y + 0x22 < my && my < y + 0x49;
        let half = (x + 0x22 < mx && mx < x + 0x4c) || (x + 0x63 < mx && mx < x + 0x8f);
        if !(row && half) {
            continue;
        }
        let mut mh = state.manager_index_of_team(ph, 0xff);
        let ma = state.manager_index_of_team(0xff, pa) & 0x7f;
        if mh == 0 && ma == 0 {
            continue;
        }
        if mx > x + 0x63 {
            mh = 0;
        }
        if mh == 0 {
            mh = ma;
        }
        let m = mh.wrapping_sub(1);
        if mh != 0
            && state
                .managers
                .get(usize::from(m))
                .is_some_and(|r| r.team == home || r.team == away)
        {
            return Some(m);
        }
    }
    None
}

/// A left click on a live score panel in a minute of a period (the engine calls this for
/// every pairing of a minute before its last): the panel's manager whose club plays `home`
/// vs `away` gets his lineup screen (2a58:1f75, with the match's cup mask as its
/// competition), after the button was released. Returns the manager; the engine re-plans
/// the rest of his match (`match_reschedule_chances`). The click is the minute's mouse event
/// (`FAR:2ea2`) as the events of earlier pairings left it; the lineup screen ends with no
/// event pending, so one click opens at most one screen.
///
/// Origin: BMMAIN.EXE 1310:3314 (match_handle_panel_click)
#[allow(clippy::too_many_arguments)]
pub fn match_handle_panel_click(
    ui: &mut ScreenSeason<'_>,
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    cup_mask: u8,
    home: u8,
    away: u8,
) -> (Option<u8>, Run<()>) {
    if ui.host.button != LEFT_DOWN {
        return (None, Ok(()));
    }
    let (mx, my) = ui.host.mouse_xy();
    let Some(m) = panel_click_manager(state, &ui.live.panel_teams, mx, my, home, away) else {
        return (None, Ok(()));
    };
    let r = ui
        .host
        .input_wait_release()
        .and_then(|()| match_substitution_screen(ui, state, rt, rng, m, cup_mask));
    (Some(m), r)
}

/// The end of a period: the crowd stops and the screen is cleared; then per played tier the
/// fixtures with the period's results when the tier's option asks for them (after minute 45
/// or 90; the rescheduled games by their own option), and on a cup day the cup results.
///
/// Origin: BMMAIN.EXE 1310:2303 (match_live_period, 1310:2aab..2c94)
pub fn period_end(
    ui: &mut ScreenSeason<'_>,
    state: &mut GameState,
    league_mask: u8,
    cup_mask: u8,
    tier: Option<u8>,
    to: u8,
) -> Run<()> {
    if !ui.live.closed {
        ui.live.closed = true;
        if ui.live.shown {
            ui.host.voice_stop();
        }
        fill(ui.host, 0, 0, 0x13f, 0xef, 0);
    }
    match tier {
        Some(t) if league_mask == 0 => {
            if state.options[OPTION_RESCHEDULED] != 0 {
                office_show_cached_background(ui.host)?;
                screen_fixtures(ui.host, ui.session, state, usize::from(t), 2, to, false)?;
            }
        }
        Some(t) => {
            let row = 3 * usize::from(t);
            if (to == 0x2d && state.options[row + OPTION_HALF_TIME] != 0)
                || (to == 0x5a && state.options[row + OPTION_FULL_TIME] != 0)
            {
                office_show_cached_background(ui.host)?;
                screen_fixtures(ui.host, ui.session, state, usize::from(t), 2, to, true)?;
            }
        }
        None if cup_mask != 0 => {
            for c in period_end_cup_rows(state, cup_mask) {
                office_show_cached_background(ui.host)?;
                screen_cup_matches(ui.host, ui.session, state, c, to)?;
            }
        }
        None => {}
    }
    Ok(())
}

/// After the last period: the cup results after a shoot-out, the pictures are freed, and the
/// tables of the played tiers whose option asks for them are shown.
///
/// Origin: BMMAIN.EXE 1310:15db (matchday_play, 1310:1a6a..1b9e)
pub fn matchday_results(
    ui: &mut ScreenSeason<'_>,
    state: &mut GameState,
    league_mask: u8,
    cup_mask: u8,
    penalties: bool,
    relegation: bool,
) -> Run<()> {
    if penalties {
        for c in shootout_cup_rows(state, cup_mask, relegation) {
            office_show_cached_background(ui.host)?;
            screen_cup_matches(ui.host, ui.session, state, c, 0x78)?;
            ui.host.screen_fade(0)?;
        }
    }
    let live = &mut ui.live;
    for bm in [live.badges.take(), live.panel.take(), live.sprites.take()]
        .into_iter()
        .flatten()
    {
        ui.host.g.free(bm);
    }
    if cup_mask == 0 && league_mask != 0 {
        for t in 0..3usize {
            if (league_mask >> t) & 1 != 0 && state.options[3 * t + OPTION_TABLE] != 0 {
                office_show_cached_background(ui.host)?;
                screen_league_table(ui.host, ui.session, state, t, 0)?;
            }
        }
    }
    // The cup round (cup_round_complete) and its draw screens follow in the logic
    // (SeasonUi::ask_draw, draw_slot, draw_close).
    ui.host.screen_fade(0)
}

/// The cup rows whose list closes a period of a cup day: the DFB-Pokal (`cup_mask` 1) or
/// the masked European rows, when their option asks for them (`DS:0608`, `DS:0609`) or
/// always from day 316 on (the relegation play-off, `DS:07dc > 0x13b`).
///
/// Origin: BMMAIN.EXE 1310:2303 (match_live_period, 1310:2c0e..2c94)
fn period_end_cup_rows(state: &GameState, cup_mask: u8) -> Vec<u8> {
    let show = state.day_counter > 0x13b
        || (cup_mask == 1 && state.options[OPTION_DFB_CUP] != 0)
        || (cup_mask > 1 && state.options[OPTION_EURO_CUPS] != 0);
    if !show {
        Vec::new()
    } else if cup_mask == 1 {
        vec![0]
    } else {
        (1..4u8).filter(|c| (1 << c) & cup_mask != 0).collect()
    }
}

/// The cup rows whose results are shown after a penalty shoot-out: the DFB-Pokal when its
/// option `DS:0608` is set, otherwise the masked European rows when their option `DS:0609`
/// is set or a relegation play-off leg is played (`FAR:57dc`, `relegation`).
///
/// Origin: BMMAIN.EXE 1310:15db (matchday_play, 1310:1a51..1aec)
fn shootout_cup_rows(state: &GameState, cup_mask: u8, relegation: bool) -> Vec<u8> {
    if state.options[OPTION_DFB_CUP] != 0 && cup_mask == 1 {
        return vec![0];
    }
    if !relegation && state.options[OPTION_EURO_CUPS] == 0 {
        return Vec::new();
    }
    (1..4u8).filter(|c| (1 << c) & cup_mask != 0).collect()
}

/// The panel row of the match of `home` (`FAR:5350`) and the y of its event panel.
fn event_panel(live: &LiveView, home: u8) -> (usize, i16) {
    let mut k = 0usize;
    while k < 4 && live.slots[k] != home {
        k += 1;
    }
    (k, ((3 - k as i16) / 2) * 0x71)
}

/// The manager whose club plays on the event's side (the last one, as in the original).
fn side_manager(state: &GameState, e: &TickerEvent) -> Option<usize> {
    let team = if e.side == 0 { e.home } else { e.away };
    let n = state.active_managers().len();
    (0..n).rev().find(|&j| state.managers[j].team == team)
}

/// The event panel of a live ticker event before its logic: the panel with both crests, the
/// managers' portraits and the pitch, the whistle of a card or the injury sound, and for a
/// chance without goal scenes the blinking "Chance f]r ..." text and the goal or miss sound.
/// The chance counters of the panel are counted here.
///
/// Origin: BMMAIN.EXE 2a58:0ca3 (match_ticker_events, up to 2a58:14c0)
pub fn ticker_event_start(
    ui: &mut ScreenSeason<'_>,
    state: &GameState,
    _rt: &Runtime,
    e: &TickerEvent,
) -> Run<()> {
    let hm = side_manager(state, e);
    let portraits = ui.portraits();
    let auto = ui.session.auto_play;
    let host = &mut *ui.host;
    let live = &mut ui.live;
    let (k, yb) = event_panel(live, e.home);
    fill(host, 0x81, 0xe9, 0xc0, 0xef, 0);
    if hm.is_some_and(|m| squad_count_starters(state, m) == 0) {
        live.event_y = 0;
        return Ok(());
    }
    host.set_font(2);
    fill(host, 0, yb, 0x13f, yb + 0x7e, 0);
    let penalty = e.arg10 != 0 && e.kind != EVENT_RED_CARD;
    let minute = live.minute;
    match_ticker_minute(host, live, minute, yb + 0x77, penalty);
    for i in 0..2 {
        blit(
            host,
            live.sprites,
            0xa3,
            0x66,
            0x73 + 0x4e * i,
            yb + 0x6c,
            0x0d,
            0x0d,
        );
    }
    frame(host, 0, yb, 0x13f, yb + 0x7e, 1);
    for i in 0..2i16 {
        let x = 0xf1 * i;
        fill(host, x + 0x13, yb + 4, x + 0x3a, yb + 0x2b, 0);
        frame(host, x + 0x12, yb + 3, x + 0x3b, yb + 0x2c, 2);
        fill(host, x + 0x13, yb + 0x3c, x + 0x3b, yb + 0x65, 0);
        frame(host, x + 0x12, yb + 0x3b, x + 0x3c, yb + 0x66, 2);
    }
    let n = state.active_managers().len();
    for j in 0..n {
        let team = state.managers[j].team;
        let dx = if team == e.home {
            0x14
        } else if team == e.away {
            0x105
        } else {
            continue;
        };
        let screen = host.screen();
        host.g.blit_bitmap(
            portraits,
            0x29 * j as i16 + 1,
            0,
            screen,
            dx,
            yb + 0x3d,
            0x28,
            0x28,
        );
    }
    let sy = i16::from(yb == 0) * 0x73 + 0x23;
    let sx = (k as i16 & 1) * 0xa1;
    let screen = host.screen();
    host.g
        .blit_bitmap(screen, sx + 0x16, sy, screen, 0x14, yb + 5, 0x26, 0x26);
    host.g
        .blit_bitmap(screen, sx + 0x65, sy, screen, 0x105, yb + 5, 0x26, 0x26);
    blit(host, live.sprites, 0, 0, 0x42, yb + 2, 0xbc, 0x66);
    host.set_font(1);
    match e.kind {
        EVENT_RED_CARD | EVENT_YELLOW_CARD => voice(host, 3)?,
        EVENT_INJURY => voice(host, 4)?,
        _ => {}
    }
    live.event_y = yb + 1;
    if e.kind == EVENT_INJURY {
        live.pending_injury = Some(*e);
    }
    if e.kind > EVENT_GOAL {
        return Ok(());
    }
    // The chance counters (2a58:11b0).
    let hm8 = hm.map_or(0xff, |m| m as u16);
    for (team, own, other) in [(e.home, 4, 5), (e.away, 5, 4)] {
        let mi = state.manager_index_of_team(team, 0xff);
        if mi != 0 {
            let idx = if u16::from(mi) - 1 == hm8 { own } else { other };
            let c = &mut live.counters[usize::from(mi - 1) & 3][idx];
            *c = c.wrapping_add(1);
        }
    }
    if state.options[9] == 0 {
        fill(host, 0x0a, yb + 0x6c, 0x136, yb + 0x79, 0);
        if !auto {
            let att = if e.side == 0 { e.home } else { e.away };
            let mut buf = if e.arg10 == 0 {
                CHANCE_FOR.to_vec()
            } else {
                PENALTY_FOR.to_vec()
            };
            buf.extend_from_slice(state.clubs[usize::from(att)].name());
            buf.extend_from_slice(b" !");
            for _ in 0..4 {
                host.delay_ticks(6)?;
                centered(host, &buf, 0, 0x13f, yb + 0x75, 0x0b, 0);
                host.delay_ticks(6)?;
                fill(host, 0x0a, yb + 0x6c, 0x136, yb + 0x79, 0);
            }
            voice(host, if e.kind == EVENT_GOAL { 2 } else { 5 })?;
        }
    }
    // With the goal-scene option the logic plays the scene next (SeasonUi::goal_scene).
    Ok(())
}

/// The last newspaper entry of `kind` (goal or chance) of a record: the player and the
/// assistant.
fn last_entry(rec: &NewsRecord, kind: u8) -> Option<(u8, u8)> {
    let image = rec.to_bytes();
    let (count, player, assist) = if kind == EVENT_GOAL {
        (0x8b, 0x27, 0x63)
    } else {
        (0x8c, 0x4f, 0x77)
    };
    let n = usize::from(image[count]);
    (n > 0 && n <= 20).then(|| (image[player + n - 1], image[assist + n - 1]))
}

/// The event panel after the logic: the scorer and the assistant of a human club's chance,
/// the card texts, the waits, the counters, and the panel is cleared again with the score and
/// the time bar redrawn.
///
/// Origin: BMMAIN.EXE 2a58:0ca3 (match_ticker_events, from 2a58:14c0), 1310:322c
/// (match_event_redraw) and 2a58:009d after a chance
pub fn ticker_event_end(
    ui: &mut ScreenSeason<'_>,
    state: &GameState,
    rt: &Runtime,
    e: &TickerEvent,
) -> Run<()> {
    if ui.live.event_y == 0 {
        return Ok(());
    }
    let hm = side_manager(state, e);
    let auto = ui.session.auto_play;
    let speed = i32::from(state.match_options);
    let (k, yb) = event_panel(&ui.live, e.home);
    let host = &mut *ui.host;
    let live = &mut ui.live;
    let fmt = host.number_format();
    let (m, pl) = (
        usize::from(e.manager_or_kind) & 3,
        usize::from(e.player_or_minute),
    );
    let player_name = |state: &GameState, m: usize, i: usize| -> Vec<u8> {
        state
            .squads
            .get(m)
            .and_then(|s| s.get(i))
            .map(|s| state.players[usize::from(s.player_id)].name().to_vec())
            .unwrap_or_default()
    };
    let tor_x = 0xf1 * i16::from(e.side.min(1)) + 0x14;
    match e.kind {
        EVENT_CHANCE | EVENT_GOAL => {
            let (att, def) = if e.side == 0 {
                (e.home, e.away)
            } else {
                (e.away, e.home)
            };
            let goal = e.kind == EVENT_GOAL;
            let mut human = 0;
            for am in managers_of(state, att).collect::<Vec<_>>() {
                human = 1;
                let (sc, assist) =
                    last_entry(&rt.news_records[am & 3], e.kind).unwrap_or((0xfe, 0xfe));
                let mut buf = if goal {
                    blit(host, live.sprites, 0x7c, 0x66, tor_x, yb + 0x3d, 0x27, 0x28);
                    SCORER.to_vec()
                } else {
                    MISSED.to_vec()
                };
                if sc != 0xfe {
                    buf.extend(player_name(state, am, usize::from(sc)));
                } else {
                    buf.extend_from_slice(OWN_GOAL);
                }
                if goal && e.arg10 == 0 && sc != 0xfe {
                    let s = &state.squads[am][usize::from(sc)];
                    let total =
                        i32::from(s.goals[0]) + i32::from(s.goals[1]) + i32::from(s.goals[2]);
                    buf.extend_from_slice(b" (");
                    buf.extend(format_long_thousands(total, fmt));
                    buf.push(b')');
                }
                fill(host, 0x0a, yb + 0x6c, 0x136, yb + 0x7a, 0);
                if goal || sc != 0xfe {
                    let y = yb + 0x70 + 5 * i16::from(assist == 0xfe);
                    centered(host, &buf, 0, 0x13f, y, 1, 0);
                    // The penalty-scene flag of a goal scene would suppress the line too.
                    if assist != 0xfe && assist != OPPONENT && e.arg10 == 0 {
                        let mut b = ASSIST.to_vec();
                        b.extend(player_name(state, am, usize::from(assist)));
                        centered(host, &b, 0, 0x13f, yb + 0x7a, 1, 0);
                    }
                }
            }
            if human == 0 && goal && managers_of(state, def).next().is_some() {
                blit(host, live.sprites, 0x7c, 0x66, tor_x, yb + 0x3d, 0x27, 0x28);
            }
            delay(host, auto, 0x78 - speed)?;
        }
        EVENT_RED_CARD => {
            let mut buf = if e.arg10 != 0 {
                YELLOW_RED_CARD.to_vec()
            } else {
                RED_CARD.to_vec()
            };
            buf.extend(player_name(state, m, pl));
            if let Some(h) = hm {
                live.counters[h & 3][1] = live.counters[h & 3][1].wrapping_add(1);
            }
            blit(host, live.sprites, 0x2a, 0x66, tor_x, yb + 0x3d, 0x28, 0x28);
            fill(host, 0x0a, yb + 0x6c, 0x136, yb + 0x79, 0);
            centered(host, &buf, 0, 0x13f, yb + 0x75, 0x12, 0x10);
            delay(host, auto, 0x82 - speed)?;
        }
        EVENT_YELLOW_CARD => {
            let mut buf = YELLOW_CARD.to_vec();
            buf.extend(player_name(state, m, pl));
            if let Some(h) = hm {
                live.counters[h & 3][0] = live.counters[h & 3][0].wrapping_add(1);
            }
            blit(host, live.sprites, 0x01, 0x66, tor_x, yb + 0x3d, 0x28, 0x28);
            let cards = state.squads[m].get(pl).map_or(0, |s| s.cards[1]);
            buf.extend_from_slice(b" (");
            buf.extend(format_long_thousands(i32::from(cards), fmt));
            buf.push(b')');
            fill(host, 0x0a, yb + 0x6c, 0x136, yb + 0x79, 0);
            centered(host, &buf, 0, 0x13f, yb + 0x75, 0x17, 0);
            delay(host, auto, 0x82 - speed)?;
        }
        EVENT_INJURY if ui.live.pending_injury.take().is_some() => {
            draw_injury(ui, state, e)?;
        }
        _ => {}
    }
    let host = &mut *ui.host;
    let live = &mut ui.live;
    host.set_font(2);
    match_draw_manager_bar_stats(host, live, state, k, hm, e.side);
    // match_event_redraw 1310:322c: clear the event panel (or redraw everything with three
    // or more panels), then the time bar; after a chance the score.
    let ev = live.event_y;
    live.event_y = 0;
    let panels = live.slots.iter().take_while(|&&s| s != 0xff).count();
    let cup_mode = e.comp >= 10;
    if panels < 3 {
        fill(host, 0, ev - 1, 0x13f, ev + 0x7d, 0);
    } else {
        match_draw_live_screen(ui, state, rt, cup_mode)?;
    }
    let host = &mut *ui.host;
    let live = &ui.live;
    if e.kind <= EVENT_GOAL && k < 4 {
        match_draw_score_digit_pair(host, live, state, rt, k, cup_mode);
    }
    let minute = live.minute;
    match_draw_time_bar(host, live, minute);
    Ok(())
}

/// The injury text of a ticker event ("Verletzt: <name> (<injury>)").
///
/// Origin: BMMAIN.EXE 2a58:0ca3 (match_ticker_events, 2a58:1cf9..1e2c)
fn draw_injury(ui: &mut ScreenSeason<'_>, state: &GameState, e: &TickerEvent) -> Run<()> {
    let hm = side_manager(state, e);
    let (_, yb) = event_panel(&ui.live, e.home);
    let host = &mut *ui.host;
    let live = &mut ui.live;
    let (m, pl) = (
        usize::from(e.manager_or_kind) & 3,
        usize::from(e.player_or_minute),
    );
    let mut buf = INJURED.to_vec();
    if let Some(s) = state.squads[m].get(pl) {
        buf.extend_from_slice(state.players[usize::from(s.player_id)].name());
        buf.extend_from_slice(b" (");
        buf.extend_from_slice(INJURY_NAMES[usize::from(s.injury) % INJURY_NAMES.len()]);
        buf.push(b')');
    }
    if let Some(h) = hm {
        live.counters[h & 3][2] = live.counters[h & 3][2].wrapping_add(1);
    }
    let tor_x = 0xf1 * i16::from(e.side.min(1)) + 0x14;
    blit(host, live.sprites, 0x53, 0x66, tor_x, yb + 0x3d, 0x28, 0x28);
    host.set_font(1);
    fill(host, 0x0a, yb + 0x6c, 0x136, yb + 0x79, 0);
    centered(host, &buf, 0, 0x13f, yb + 0x75, 0x12, 0x10);
    Ok(())
}

/// The injury of a live match outside automatic play: its text, the stretcher animation
/// (when the logic's `random_range(0, 15)` drew 0) or a wait, and the substitution screen.
///
/// Origin: BMMAIN.EXE 2a58:0ca3 (match_ticker_events, 2a58:1cf9..1f3d)
pub fn injury_and_substitution(
    ui: &mut ScreenSeason<'_>,
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    m: u8,
    cup_mode: u8,
    stretcher: bool,
) -> Run<()> {
    // The logic has drawn the stretcher's random number already; the event is the last one.
    let Some(e) = ui.live.pending_injury.take() else {
        return Ok(());
    };
    draw_injury(ui, state, &e)?;
    if stretcher {
        let (_, yb) = event_panel(&ui.live, e.home);
        super::goal_scene::stretcher_animation(ui.host, ui.live.sprites, yb)?;
    } else {
        ui.host.delay_ticks(150)?;
    }
    match_substitution_screen(ui, state, rt, rng, m, cup_mode)
}

/// The lineup screen of manager `m` during a live match (as many players on the pitch as
/// [`Runtime::players_on_pitch`] allows, the substitutions left), over the office
/// background; afterwards the live screen is drawn again.
///
/// Origin: BMMAIN.EXE 2a58:1f75 (match_substitution_screen)
pub fn match_substitution_screen(
    ui: &mut ScreenSeason<'_>,
    state: &mut GameState,
    rt: &mut Runtime,
    rng: &mut Rng,
    m: u8,
    cup_mode: u8,
) -> Run<()> {
    let mi = usize::from(m) & 3;
    let saved = state.current_manager;
    state.current_manager = m as i8;
    office_show_cached_background(ui.host)?;
    ui.host.input_wait_release()?;
    let mut subs = MatchSubs {
        remaining: rt.substitutions_left[mi],
        made: rt.lineup_change_count[mi],
    };
    let (allowed, rules) = (rt.players_on_pitch[mi], rt.lineup_rules);
    let r = ui.lend(rng, rt, |ui| {
        screen_squad_lineup(
            ui.host,
            ui.session,
            state,
            rules,
            allowed,
            cup_mode,
            Some(&mut subs),
        )
    });
    rt.substitutions_left[mi] = subs.remaining;
    rt.lineup_change_count[mi] = subs.made;
    r?;
    let host = &mut *ui.host;
    host.screen_fade(0)?;
    host.ui.redraw_buttons(&mut host.g, &mut host.rp, 0x7f, 0);
    fill(host, 0, 0, 0x13f, 0xef, 0);
    match_draw_live_screen(ui, state, rt, cup_mode != 0)?;
    ui.host.set_font(2);
    ui.host.input_wait_release()?;
    ui.host.screen_fade(1)?;
    state.current_manager = saved;
    Ok(())
}

/// `UI` strings of the title screens.
const CHAMPIONSHIP: &[u8] = b"Deutsche Meisterschaft";
const WINS_THE_F: &[u8] = b"gewinnt die";
const WINS_THE_M: &[u8] = b"gewinnt den";

/// The title screen of manager `m`'s club winning competition `comp` (0 = the German
/// championship, 1 = the DFB-Pokal, 2..4 = the European cups 1..3): the club name on the
/// flag, "gewinnt die"/"gewinnt den", the trophy (`PIC\50..54.VGA`) and the competition name
/// ("Deutsche Meisterschaft" or `DS:24d8[comp]`); it waits for a click.
///
/// Origin: BMMAIN.EXE 27c2:274d (screen_title_winner)
pub fn screen_title_winner(
    host: &mut Host,
    state: &GameState,
    m: u8,
    comp: u8,
    auto: bool,
) -> Run<()> {
    host.screen_fade(0)?;
    match_screen_clear_background(host);
    let team = state.managers[usize::from(m) & 3].team;
    let name = state.clubs[usize::from(team)].name().to_vec();
    centered(host, &name, 0, 0x13f, 0x21, 0x1d, 0);
    let (title, wins) = if comp == 0 {
        (CHAMPIONSHIP, WINS_THE_F)
    } else {
        (CUP_NAMES[usize::from(comp - 1) & 3], WINS_THE_M)
    };
    centered(host, title, 0, 0x13f, 0xdd, 0x1d, 0);
    host.set_font(1);
    centered(host, wins, 0, 0x13f, 0x3c, 0x1d, 0);
    if let Ok(pic) = host.load_picture(0x32 + u16::from(comp)) {
        let screen = host.screen();
        host.g
            .blit_bitmap(pic, 0, 0, screen, 0x74, 0x54, 0x58, 0x58);
        host.g.free(pic);
    }
    host.screen_fade(1)?;
    host.input_wait_release()?;
    super::widgets::input_wait_click(host, auto)?;
    host.g.rastport_clear(&host.rp, 0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn period_end_shows_the_relegation_list_without_the_option() {
        let mut s = GameState::zeroed();
        s.day_counter = 0x13b;
        assert!(period_end_cup_rows(&s, 2).is_empty());
        assert!(period_end_cup_rows(&s, 1).is_empty());
        s.options[OPTION_EURO_CUPS] = 1;
        assert_eq!(period_end_cup_rows(&s, 0x0e), [1, 2, 3]);
        assert!(period_end_cup_rows(&s, 1).is_empty());
        s.options[OPTION_EURO_CUPS] = 0;
        s.day_counter = 0x13c;
        assert_eq!(period_end_cup_rows(&s, 2), [1]);
        assert_eq!(period_end_cup_rows(&s, 1), [0]);
    }

    #[test]
    fn shootout_lists_follow_the_options_or_the_relegation_leg() {
        let mut s = GameState::zeroed();
        assert!(shootout_cup_rows(&s, 1, false).is_empty());
        assert!(shootout_cup_rows(&s, 2, false).is_empty());
        // FAR:57dc shows the relegation leg's row although DS:0609 is off.
        assert_eq!(shootout_cup_rows(&s, 2, true), [1]);
        s.options[OPTION_DFB_CUP] = 1;
        assert_eq!(shootout_cup_rows(&s, 1, false), [0]);
        assert!(shootout_cup_rows(&s, 0x0a, false).is_empty());
        s.options[OPTION_EURO_CUPS] = 1;
        assert_eq!(shootout_cup_rows(&s, 0x0a, false), [1, 3]);
    }
    use bmp_game::news::news_record_match_event;

    /// Two managers: 0 manages club 7 (home of panel 0), 1 manages club 12 (away of panel 1).
    fn panel_state() -> (GameState, [[u8; 2]; 4]) {
        let mut s = GameState::zeroed();
        s.manager_count = 2;
        s.managers[0].team = 7;
        s.managers[1].team = 12;
        (s, [[7, 8], [11, 12], [0xff; 2], [0xff; 2]])
    }

    #[test]
    fn panel_hit_boxes_are_the_crest_halves_of_each_panel() {
        let (s, teams) = panel_state();
        // Panel 0 at (0, 0): home half x 0x23..=0x4b, crest row y 0x23..=0x48.
        assert_eq!(panel_click_manager(&s, &teams, 0x23, 0x23, 7, 8), Some(0));
        assert_eq!(panel_click_manager(&s, &teams, 0x4b, 0x48, 7, 8), Some(0));
        // The bounds are exclusive.
        assert_eq!(panel_click_manager(&s, &teams, 0x22, 0x30, 7, 8), None);
        assert_eq!(panel_click_manager(&s, &teams, 0x4c, 0x30, 7, 8), None);
        assert_eq!(panel_click_manager(&s, &teams, 0x30, 0x22, 7, 8), None);
        assert_eq!(panel_click_manager(&s, &teams, 0x30, 0x49, 7, 8), None);
        // Between the halves and right of the away half nothing happens.
        assert_eq!(panel_click_manager(&s, &teams, 0x50, 0x30, 7, 8), None);
        assert_eq!(panel_click_manager(&s, &teams, 0x8f, 0x30, 7, 8), None);
        // Panel 1 at (0xa1, 0): its away half belongs to manager 1 (club 12).
        assert_eq!(
            panel_click_manager(&s, &teams, 0xa1 + 0x70, 0x30, 11, 12),
            Some(1)
        );
        // Its home half: club 11 has no manager, so the away manager is taken.
        assert_eq!(
            panel_click_manager(&s, &teams, 0xa1 + 0x30, 0x30, 11, 12),
            Some(1)
        );
        // Panel 2 (y 0x73) is not tested: the scan stops at its 0xff.
        assert_eq!(
            panel_click_manager(&s, &teams, 0x30, 0x73 + 0x30, 7, 8),
            None
        );
    }

    #[test]
    fn panel_click_away_half_takes_the_away_manager() {
        let (mut s, mut teams) = panel_state();
        // Both clubs of panel 0 human: manager 0 at home (7), manager 1 away (12).
        teams[0] = [7, 12];
        s.managers[1].team = 12;
        assert_eq!(panel_click_manager(&s, &teams, 0x30, 0x30, 7, 12), Some(0));
        assert_eq!(panel_click_manager(&s, &teams, 0x70, 0x30, 7, 12), Some(1));
        // An away-half click on a panel whose away club has no manager finds nobody.
        teams[0] = [7, 8];
        assert_eq!(panel_click_manager(&s, &teams, 0x70, 0x30, 7, 8), None);
    }

    #[test]
    fn panel_click_needs_the_manager_in_the_pairing_being_played() {
        let (s, teams) = panel_state();
        // The click on panel 0 is only taken while manager 0's pairing (club 7) is played.
        assert_eq!(panel_click_manager(&s, &teams, 0x30, 0x30, 3, 4), None);
        assert_eq!(panel_click_manager(&s, &teams, 0x30, 0x30, 4, 7), Some(0));
        // Panel 0 is tested even when no panel is registered (the original's do-while).
        let none = [[0xff; 2]; 4];
        assert_eq!(panel_click_manager(&s, &none, 0x30, 0x30, 7, 8), None);
    }

    #[test]
    fn event_panels_of_the_first_two_rows_lie_in_the_lower_half() {
        let live = LiveView {
            slots: [7, 9, 11, 0xff],
            ..LiveView::default()
        };
        assert_eq!(event_panel(&live, 7), (0, 0x71));
        assert_eq!(event_panel(&live, 9), (1, 0x71));
        assert_eq!(event_panel(&live, 11), (2, 0));
        // A club without a panel runs off the end: row 4, top half.
        assert_eq!(event_panel(&live, 3), (4, 0));
    }

    #[test]
    fn the_last_goal_and_chance_of_the_record_name_the_players() {
        let mut rt = Runtime::default();
        assert_eq!(last_entry(&rt.news_records[0], EVENT_GOAL), None);
        news_record_match_event(&mut rt, 0, EVENT_GOAL, 4, 6, 12);
        news_record_match_event(&mut rt, 0, EVENT_CHANCE, 2, 0xfe, 20);
        news_record_match_event(&mut rt, 0, EVENT_GOAL, 8, 3, 33);
        assert_eq!(last_entry(&rt.news_records[0], EVENT_GOAL), Some((8, 3)));
        assert_eq!(
            last_entry(&rt.news_records[0], EVENT_CHANCE),
            Some((2, 0xfe))
        );
        assert_eq!(last_entry(&rt.news_records[1], EVENT_CHANCE), None);
    }

    #[test]
    fn the_side_manager_is_the_last_one_of_the_club() {
        let mut s = GameState::zeroed();
        s.manager_count = 3;
        s.managers[0].team = 5;
        s.managers[1].team = 8;
        s.managers[2].team = 5;
        let e = |side| TickerEvent {
            kind: EVENT_GOAL,
            comp: 0,
            slot: 0,
            home: 5,
            away: 8,
            arg6: side,
            player_or_minute: 10,
            manager_or_kind: 0,
            side,
            arg10: 0,
        };
        assert_eq!(side_manager(&s, &e(0)), Some(2));
        assert_eq!(side_manager(&s, &e(1)), Some(1));
        s.managers[1].team = 9;
        assert_eq!(side_manager(&s, &e(1)), None);
    }
}
