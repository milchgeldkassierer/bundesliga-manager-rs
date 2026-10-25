//! The screens of the season end and the game end: the "Saisonende" report of every manager,
//! the "Trennungen" screen with the retirement messages, the "Vertragsverhandlungen" with the
//! expiring contracts (`season_end_messages` 1cb5:0012, `docs/re/logic/season.md` §4.8), and the
//! "ENDE" screen of a game whose last season is over (`game_end_screen` 2a58:000c, §3.4).
//!
//! The logic is [`bmp_game::season::season_end_messages`]; it reports what to show as
//! [`SeasonEndEvent`]s, which [`season_end_event`] draws.

use bmp_game::rng::format_long_thousands;
use bmp_game::season::SeasonEndEvent;
use bmp_game::state::GameState;

use super::messages::screen_messages;
use super::season::ScreenSeason;
use super::squad_screen::squad_list_draw;
use super::tables::league_names;
use super::widgets::{dialog_okay, input_wait_click};
use super::{
    GAME_OPTION_A, GAME_OPTION_B, append_ordinal_suffix, office_show_cached_background,
    text_to_upper_de,
};
use crate::gfx::{BitmapId, ui_draw_line, ui_draw_text_centered_shadow, ui_fill_rect};
use crate::host::{Host, Run};

/// `DS:2744`, `DS:2748`, `DS:274c`.
const YOU_FINISHED: &[u8] = b"Sie beendeten Ihre Saison";
const EVENTS: &[u8] = b"Ereignisse";
const PLACE_OF: &[u8] = b"Platz der ";
/// `DS:4f44..4f50`: the promoted club's advertising partners may cancel.
const SPONSORS_RELEASED: [&[u8]; 4] = [
    b"Ihre Werbepartner erlauben",
    b"es, die Vertr{ge jederzeit",
    b"zu k}ndigen. Die Einnahmen",
    b"bleiben solange erhalten.",
];
/// `DS:4e08..4e24`: the joke of the DM version.
const JOKE: [[&[u8]; 4]; 2] = [
    [
        b"Ihre Spieler werden",
        b"{lter und schw{cher...",
        b" ",
        b"Pech gehabt.",
    ],
    [
        b"Mit freundlichem Gru~",
        b"vom Bundesliga Manager I:",
        b"War nur ein",
        b"SCHERZ !!!",
    ],
];
/// `DS:4e38..4e40`: the player signed again.
const STAYS: [&[u8]; 3] = [b"bleibt Ihnen auch", b"die n{chste Saison", b"erhalten."];
/// `DS:4e28..4e2c`: the player leaves.
const LEAVES: [&[u8]; 2] = [b"kehrt Ihrem Verein", b"den R}cken..."];
/// `DS:4e30`, `DS:4e34`, `DS:4f64`: the transfer fee.
const FEE: [&[u8]; 2] = [b"Sie erhalten eine", b"Abl|sesumme von"];
const CURRENCY: &[u8] = b" DM";
/// `DS:537e`.
const THE_END: &[u8] = b"ENDE";

/// Column mask and mode of the contracts list (as on the contracts tab).
const CONTRACT_COLUMNS: u32 = 0x0003_d146;
const CONTRACT_LIST_MODE: u8 = 3;

/// `y` of the first event line of the report and the line height.
const FIRST_LINE_Y: i16 = 0x94;
const LINE_STEP: i16 = 13;

fn fill(host: &mut Host, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    ui_fill_rect(&mut host.g, &mut host.rp, x0, y0, x1, y1, color);
}

fn centered(host: &mut Host, text: &[u8], x0: i16, x1: i16, y: i16, color: u8, shadow: u8) {
    ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, text, x0, x1, y, color, shadow);
}

#[allow(clippy::too_many_arguments)]
fn blit(host: &mut Host, src: BitmapId, sx: i16, sy: i16, dx: i16, dy: i16, w: i16, h: i16) {
    let screen = host.screen();
    host.g.blit_bitmap(src, sx, sy, screen, dx, dy, w, h);
}

/// Waits `ticks` unless automatic play skips the delays.
fn delay(host: &mut Host, auto: bool, ticks: i32) -> Run<()> {
    if !auto {
        host.delay_ticks(ticks)?;
    }
    Ok(())
}

/// `dialog_okay`, which automatic play leaves at once.
fn okay(host: &mut Host, auto: bool, lines: [&[u8]; 4]) -> Run<()> {
    if !auto {
        let [a, b, c, d] = lines;
        dialog_okay(host, a, b, c, d)?;
    }
    Ok(())
}

/// The portrait, name and crest of manager `m` at the top right.
///
/// Origin: BMMAIN.EXE 4091:185c (ui_draw_manager_header)
fn ui_draw_manager_header(
    host: &mut Host,
    portraits: BitmapId,
    crests: BitmapId,
    state: &GameState,
    m: usize,
) {
    let x = m as i16 * 0x29;
    host.ui
        .draw_button(&mut host.g, &mut host.rp, 0x109, 5, 0x2a, 0x2a, false, 8);
    blit(host, portraits, x, 0, 0x10a, 6, 0x28, 0x28);
    host.set_font(1);
    let name = state.managers[m].name().to_vec();
    centered(host, &name, 0x113, 0x127, 0x39, 0, 1);
    host.ui
        .draw_button(&mut host.g, &mut host.rp, 0x109, 0x3c, 0x2a, 0x29, false, 0);
    blit(host, crests, x, 0, 0x10b, 0x3e, 0x28, 0x26);
}

/// The marble background with the header of the current manager. The title is only passed to
/// the copy-protection trap, which draws nothing; the protection counts as passed here (the
/// trap's random draw happens only when it was not).
///
/// Origin: BMMAIN.EXE 24a4:260a (office_redraw_with_header)
fn office_redraw_with_header(ui: &mut ScreenSeason<'_>, state: &GameState) -> Run<()> {
    office_show_cached_background(ui.host)?;
    let m = (state.current_manager as usize).min(3);
    let (portraits, crests) = (ui.portraits(), ui.crest_cache());
    ui_draw_manager_header(ui.host, portraits, crests, state, m);
    Ok(())
}

/// The rank line of the report: "15.Platz der ZWEITE LIGA".
fn rank_text(host: &Host, rank: i32, tier: u8) -> Vec<u8> {
    let mut buf = format_long_thousands(rank, host.number_format());
    append_ordinal_suffix(&mut buf, rank as u8, GAME_OPTION_A, GAME_OPTION_B);
    buf.extend_from_slice(PLACE_OF);
    let name = league_names()
        .get(usize::from(tier))
        .copied()
        .unwrap_or(b"");
    buf.extend(text_to_upper_de(name));
    buf
}

/// Draws one season-end event.
///
/// Origin: BMMAIN.EXE 1cb5:0012 (season_end_messages, the screens)
pub(super) fn season_end_event(
    ui: &mut ScreenSeason<'_>,
    state: &GameState,
    event: SeasonEndEvent<'_>,
) -> Run<()> {
    let auto = ui.session.auto_play;
    match event {
        SeasonEndEvent::Report {
            rank,
            tier,
            lines,
            sponsors_released,
            joke,
            ..
        } => season_end_report(ui, state, rank, tier, lines, sponsors_released, joke),
        SeasonEndEvent::Separations { .. } => {
            office_redraw_with_header(ui, state)?;
            ui.host.screen_fade(1)
        }
        SeasonEndEvent::Retirement { message } => {
            let m = (state.current_manager as usize).min(3);
            let count = state.message_counts[m];
            let inbox = state.messages.get(m).map_or(&[][..], Vec::as_slice);
            screen_messages(ui.host, inbox, count, Some(message), auto)
        }
        SeasonEndEvent::SeparationsDone => {
            delay(ui.host, auto, 70)?;
            ui.host.screen_fade(0)?;
            ui.host
                .ui
                .redraw_buttons(&mut ui.host.g, &mut ui.host.rp, 0x7f, 0);
            Ok(())
        }
        SeasonEndEvent::Contracts { expiring, .. } => {
            ui.host.screen_fade(0)?;
            office_redraw_with_header(ui, state)?;
            if expiring {
                ui.host
                    .ui
                    .draw_button(&mut ui.host.g, &mut ui.host.rp, 2, 7, 0xee, 0xb5, false, 8);
                contracts_list_draw(ui, state);
            }
            ui.host.set_font(2);
            ui.host.screen_fade(1)?;
            if !expiring {
                delay(ui.host, auto, 70)?;
            }
            Ok(())
        }
        SeasonEndEvent::Stays { pid } => {
            let name = state.players[usize::from(pid)].name().to_vec();
            okay(ui.host, auto, [&name, STAYS[0], STAYS[1], STAYS[2]])
        }
        SeasonEndEvent::Leaves { pid, fee } => {
            let name = state.players[usize::from(pid)].name().to_vec();
            okay(ui.host, auto, [&name, LEAVES[0], LEAVES[1], b""])?;
            // DS:224d (the currency prefix DS:4f78) is 0 in the shipped data.
            let shown = fee.wrapping_mul(GAME_OPTION_B) / GAME_OPTION_A;
            let mut buf = format_long_thousands(shown, ui.host.number_format());
            buf.extend_from_slice(CURRENCY);
            okay(ui.host, auto, [FEE[0], FEE[1], &buf, b""])
        }
        SeasonEndEvent::ContractDone { .. } => {
            fill(ui.host, 3, 8, 0xef, 0xb8, 8);
            contracts_list_draw(ui, state);
            Ok(())
        }
        SeasonEndEvent::Done => {
            ui.host.screen_fade(0)?;
            ui.host
                .ui
                .redraw_buttons(&mut ui.host.g, &mut ui.host.rp, 0x7f, 0);
            Ok(())
        }
    }
}

/// The contracts list of the current manager (`squad_list_draw(6, 0x12, 0, 0x18, 0xec, 0xb0,
/// 0x3d146, 3)`). Mode 3 draws no random numbers.
fn contracts_list_draw(ui: &mut ScreenSeason<'_>, state: &GameState) {
    squad_list_draw(
        ui.host,
        state,
        &mut ui.session.rng,
        6,
        0x12,
        0,
        0x18,
        0xec,
        0xb0,
        CONTRACT_COLUMNS,
        CONTRACT_LIST_MODE,
        false,
    );
}

/// Manager `m`'s "Saisonende" report: the final rank in the tier, the events (promotion with
/// the advertising dialog, relegation, licence, restart, top scorer, European cup, youth
/// player), the joke of the DM version, then a click.
///
/// Origin: BMMAIN.EXE 1cb5:0012 (season_end_messages, 1cb5:0040..0726)
fn season_end_report(
    ui: &mut ScreenSeason<'_>,
    state: &GameState,
    rank: i32,
    tier: u8,
    lines: &[&[u8]],
    sponsors_released: bool,
    joke: bool,
) -> Run<()> {
    let auto = ui.session.auto_play;
    office_redraw_with_header(ui, state)?;
    let host = &mut *ui.host;
    host.ui
        .draw_button(&mut host.g, &mut host.rp, 0x17, 0x19, 0xde, 0x3c, false, 8);
    host.ui
        .draw_button(&mut host.g, &mut host.rp, 0x0a, 0x7e, 0x127, 0x52, false, 8);
    host.set_font(1);
    centered(host, YOU_FINISHED, 0x17, 0xf5, 0x21, 0x0b, 0xff);
    ui_draw_line(&mut host.g, &mut host.rp, 0x18, 0x24, 0xf4, 0x24, 0x0b);
    centered(host, EVENTS, 0x0a, 0x131, 0x86, 0x0b, 0xff);
    ui_draw_line(&mut host.g, &mut host.rp, 0x0b, 0x89, 0x130, 0x89, 0x0b);
    let buf = rank_text(host, rank, tier);
    centered(host, &buf, 0x17, 0xf5, 0x3c, 1, 0xff);
    let mut y = FIRST_LINE_Y;
    for (i, line) in lines.iter().enumerate() {
        centered(host, line, 0x0a, 0x131, y, 0x0b, 0xff);
        y += LINE_STEP;
        // The promotion line comes first; its dialog shows before the other lines.
        if i == 0 && sponsors_released {
            okay(host, auto, SPONSORS_RELEASED)?;
            host.set_font(1);
        }
    }
    host.screen_fade(1)?;
    if joke {
        okay(host, auto, JOKE[0])?;
        okay(host, auto, JOKE[1])?;
    }
    input_wait_click(host, auto)?;
    host.screen_fade(0)?;
    host.ui.redraw_buttons(&mut host.g, &mut host.rp, 0x7f, 0);
    Ok(())
}

/// The last screen of a game whose last season is over: "ENDE" on black, then a click.
///
/// Origin: BMMAIN.EXE 2a58:000c (game_end_screen)
pub(super) fn game_end_screen(host: &mut Host, auto: bool) -> Run<()> {
    fill(host, 0, 0, 0x13f, 0xef, 0);
    host.set_font(3);
    centered(host, THE_END, 8, 0x13f, 0x73, 1, 0xff);
    input_wait_click(host, auto)
}
