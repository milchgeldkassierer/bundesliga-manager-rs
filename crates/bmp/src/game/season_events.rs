//! The screens and dialogs of the season loop between the office turns: the "WINTERPAUSE"
//! title, the special dates (Christmas and the programmers' birthdays), the forfeit of a
//! club that cannot field a team and the loans that fall due. Spec:
//! `docs/re/logic/season.md` §3.1, §3.2, §3.5 and `docs/re/logic/finance.md` §3.

use bmp_game::finance::{BANK, FinanceEvent};
use bmp_game::league_layout::is_mod2026;
use bmp_game::rng::{NumberFormat, format_long_thousands, ldiv};
use bmp_game::season::SpecialDate;
use bmp_game::state::GameState;

use super::messages::screen_messages;
use super::office_show_cached_background;
use super::tables::league_names;
use super::text_to_upper_de;
use super::widgets::{dialog_okay_auto, ui_draw_manager_header};
use crate::gfx::{BitmapId, ui_draw_frame, ui_draw_text_centered_shadow, ui_fill_rect};
use crate::host::{Host, LEFT_DOWN, Run};

/// `DS:4c00`.
const WINTER_BREAK: &[u8] = b"WINTERPAUSE";

/// The colour the original puts on pixel (1, 1) of the screen once "WINTERPAUSE" was shown.
const WINTER_BREAK_MARK: u16 = 8;

/// "WINTERPAUSE" once per winter break: the original marks the screen's pixel (1, 1) with
/// colour 8 after the title and shows it again only when that pixel has another colour.
///
/// Origin: BMMAIN.EXE 2a58:3176 (season_main_loop, 2a58:33c8..3438)
pub fn winter_break(host: &mut Host, auto: bool) -> Run<()> {
    let screen = host.screen();
    if host.g.get_pixel(1, 1, screen) == WINTER_BREAK_MARK {
        return Ok(());
    }
    host.screen_fade(0)?;
    super::live_match::matchday_show_title(host, WINTER_BREAK, auto)?;
    crate::gfx::Gfx::rastport_set_color(&mut host.rp, WINTER_BREAK_MARK as u8);
    host.g.rastport_put_pixel(&host.rp, 1, 1);
    Ok(())
}

/// `DS:26f0`: the Christmas bonus text.
const CHRISTMAS_LINES: [&[u8]; 5] = [
    b"Ihre Spieler sind }berw{ltigt und",
    b"zu Tr{nen ger}hrt, als sie Ihre",
    b"gro~z}gigen Weihnachtspakete unter",
    b"dem lammettabehangenen Christbaum",
    b"ersp{hen. Was f}r eine Freude !",
];
/// `DS:2704`: the birthday text; line 3 gets the programmer's name.
const BIRTHDAY_LINES: [&[u8]; 8] = [
    b"Sehr geehrte Mitspieler !",
    b"Heute ist ein {u~erst denkw}rdiger Tag.",
    b"Es ist der Geburtstag eines der beiden",
    b"glorreichen Programmierer: ",
    b"La~t uns eine Minute in ehrerbietendem",
    b"Schweigen verharren und dem gedenken,",
    b"ohne den dieses affenstarke Spiel nie",
    b"das Licht der Welt erblickt h{tte...",
];
/// `DS:53af`, `DS:53ba`.
const PROGRAMMERS: [&[u8]; 2] = [b"JENS ONNEN", b"WERNER KRAHE"];

/// Ticks a birthday screen stays (`timer_set_deadline(0x438)`, about a minute).
const BIRTHDAY_TICKS: u16 = 0x438;

/// The bonus line `prefix` + the formatted amount + `suffix`, with separators (`DS:07b2 = 0`).
/// The currency factors `DS:2252 / DS:224e` are equal in the shipped data.
fn bonus_line(fmt: NumberFormat, prefix: &[u8], amount: i32, suffix: &[u8]) -> Vec<u8> {
    let mut line = prefix.to_vec();
    line.extend(format_long_thousands(amount, fmt));
    line.extend_from_slice(suffix);
    line
}

/// The text lines of a Christmas bonus of `bonus` DM: what a Bundesliga club, a 2. Bundesliga
/// club (half) and a third-tier club (a third) paid.
pub fn christmas_bonus_lines(fmt: NumberFormat, bonus: i32) -> [Vec<u8>; 2] {
    let mut l1 = bonus_line(fmt, b"BUNDESLIGA: ", bonus, b"");
    l1.extend(bonus_line(fmt, b" DM, 2.LIGA: ", bonus >> 1, b" DM,"));
    // MOD2026: a mod game names its third tier the 3. Liga.
    let third = if is_mod2026() {
        [text_to_upper_de(league_names()[2]), b": ".to_vec()].concat()
    } else {
        b"AMATEUR-OBERLIGA: ".to_vec()
    };
    let l2 = bonus_line(fmt, &third, ldiv(bonus, 3), b" DM (INCL. MWST.)");
    [l1, l2]
}

/// The screen of a special date (the logic decided that it happens and paid the Christmas
/// bonus): the picture (`PIC\42.VGA` Christmas, `PIC\41.VGA` the programmer) in a frame, the
/// date, and the Christmas or birthday text. Christmas waits for a click; a birthday waits a
/// minute, and a click before that earns a reprimand.
///
/// Origin: BMMAIN.EXE 2a58:2a06 (special_date_events), the screen
pub fn special_date_screen(
    host: &mut Host,
    state: &GameState,
    date: SpecialDate,
    auto: bool,
) -> Run<()> {
    let birthday = date.idx != 0;
    host.screen_fade(0)?;
    ui_fill_rect(&mut host.g, &mut host.rp, 0, 0, 0x13f, 0xef, 0x10);
    ui_draw_frame(&mut host.g, &mut host.rp, 0, 0, 0x13f, 0xef, 0x13);
    let pic = if birthday { 0x29 } else { 0x2a };
    {
        let bm = host.picture(pic)?;
        let screen = host.screen();
        host.g.blit_bitmap(bm, 0, 0, screen, 0x88, 0x34, 0x30, 0x39);
        ui_draw_frame(&mut host.g, &mut host.rp, 0x86, 0x32, 0xb7, 0x6c, 0x1d);
        host.g.free(bm);
    }
    host.set_font(1);
    let weekday =
        super::office::WEEKDAYS[(state.day_counter.wrapping_add(4)).rem_euclid(7) as usize];
    let mut line = weekday.to_vec();
    line.extend_from_slice(b", ");
    line.extend(format_long_thousands(
        state.day_of_month,
        host.number_format(),
    ));
    line.extend_from_slice(b". ");
    line.extend_from_slice(super::office::MONTHS[state.month.rem_euclid(12) as usize]);
    centered(host, &line, 8, 0x0b);
    if !birthday {
        centered(host, b"Frohe Weihnachten !", 0x23, 0x13);
        host.set_font(1);
        if let Some(bonus) = date.christmas_bonus {
            host.number_no_separators = false;
            for (i, l) in CHRISTMAS_LINES.iter().enumerate() {
                centered(host, l, 0x8c + 11 * i as i16, 0x0b);
            }
            host.set_font(2);
            let [l1, l2] = christmas_bonus_lines(host.number_format(), bonus);
            centered(host, &l1, 0xde, 0x0b);
            centered(host, &l2, 0xe6, 0x0b);
        }
    } else {
        centered(host, b"Oh happy day !!!", 0x21, 0x13);
        host.set_font(1);
        for (i, l) in BIRTHDAY_LINES.iter().enumerate() {
            let mut text = l.to_vec();
            if i == 3 {
                text.extend_from_slice(PROGRAMMERS[usize::from(date.idx != 1)]);
            }
            centered(host, &text, 0x7d + 11 * i as i16, 0x0b);
        }
    }
    host.screen_fade(1)?;
    host.g.timer_set_deadline(BIRTHDAY_TICKS);
    let mut timed_out = false;
    loop {
        if host.g.timer_deadline_reached() {
            timed_out = true;
        }
        host.input_poll_mouse()?;
        if host.button == LEFT_DOWN || auto {
            break;
        }
        if timed_out && birthday {
            break;
        }
    }
    if !timed_out && birthday {
        centered(host, b"Das war keine Minute !", 0xe4, 0x0b);
        centered(host, b"Sch{men Sie Sich !", 0xee, 0x0b);
        host.delay_ticks(150)?;
    }
    Ok(())
}

fn centered(host: &mut Host, text: &[u8], y: i16, color: u8) {
    ui_draw_text_centered_shadow(&mut host.g, &mut host.rp, text, 0, 0x13f, y, color, 0);
}

/// Manager `m`'s club cannot field a team: the match counts 0:2 and costs a fine.
///
/// Origin: BMMAIN.EXE 2a58:2051 (match_forfeit_penalty), the dialog
pub fn forfeit_dialog(host: &mut Host, state: &GameState, m: u8, auto: bool) -> Run<()> {
    let name = state.managers[usize::from(m) & 3].name().to_vec();
    dialog_okay_auto(
        host,
        auto,
        &name,
        b"Ihr Spiel wird mit 0:2",
        b"gewertet. Sie zahlen",
        b"200.000 DM Strafe",
    )?;
    Ok(())
}

/// The bitmaps [`ui_draw_manager_header`] takes from: the portraits and the club crests.
#[derive(Clone, Copy, Debug)]
pub struct HeaderBitmaps {
    /// `FAR:2f24`.
    pub portraits: BitmapId,
    /// `FAR:5354`.
    pub crests: BitmapId,
}

/// The screens of manager `m`'s day of finances, in the order of `events`:
///
/// * a construction that finished in the winter break or after the season's last day
///   (`stadium_construction_daily` 1000:20e1, 1000:2295..234c): the message is shown at once
///   in the mail viewer over the office background with the manager's header
///   (`message_post` with `show` set calls `screen_messages`), then the screen goes off and
///   the buttons are removed;
/// * a loan that fell due, shown over the office background with the manager's header
///   ("Ihr Kredit von <lender> wurde heute f{llig.").
///
/// Origin: BMMAIN.EXE 212a:0a6d (finance_daily_processing, 212a:0d1a..0e3e)
pub fn finance_dialogs(
    host: &mut Host,
    state: &GameState,
    m: u8,
    events: &[FinanceEvent],
    bitmaps: HeaderBitmaps,
    auto: bool,
) -> Run<()> {
    let mi = usize::from(m) & 3;
    for e in events {
        let lender = match e {
            FinanceEvent::LoanDue { lender } => *lender,
            FinanceEvent::ConstructionFinished {
                shown: Some((text, count)),
                ..
            } => {
                office_show_cached_background(host)?;
                let name = state.managers[mi].name().to_vec();
                ui_draw_manager_header(host, mi, &name, bitmaps.portraits, bitmaps.crests);
                let inbox = state.messages.get(mi).map_or(&[][..], Vec::as_slice);
                screen_messages(host, inbox, *count, Some(text), auto)?;
                host.screen_fade(0)?;
                host.ui.redraw_buttons(&mut host.g, &mut host.rp, 0x7f, 0);
                continue;
            }
            FinanceEvent::ConstructionFinished { shown: None, .. } => continue,
        };
        let who = if lender == BANK {
            b"der Bank".to_vec()
        } else {
            state.managers[lender].name().to_vec()
        };
        // copy_protection_penalty: the protection is treated as passed (DS:05ac != 0).
        office_show_cached_background(host)?;
        let name = state.managers[mi].name().to_vec();
        ui_draw_manager_header(host, mi, &name, bitmaps.portraits, bitmaps.crests);
        dialog_okay_auto(
            host,
            auto,
            b"Ihr Kredit von",
            &who,
            b"wurde heute f{llig.",
            b"",
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmp_game::league_layout::{MOD2026, ORIGINAL, set_layout};

    /// A construction finished in the winter break opens the mail viewer with its message;
    /// a click on the exit button closes it and the screen goes off (1000:2295..234c).
    /// Needs `BMP_DATA`.
    #[test]
    fn a_construction_shown_at_once_waits_in_the_mail_viewer() {
        use crate::host::parse_script;
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        let script = parse_script("wait 300\nshot unused\nclick 254 170\nwait 300\n").unwrap();
        let shots = std::env::temp_dir().join(format!("bmp-construction-{}", std::process::id()));
        std::fs::create_dir_all(&shots).unwrap();
        let mut host = Host::with_script(&data, script, &shots, false).unwrap();
        let pics = super::super::start_menu::Pictures::load(&mut host).unwrap();
        let bitmaps = HeaderBitmaps {
            portraits: pics.portraits,
            crests: pics.crest_cache,
        };
        let mut state = GameState::program_start();
        state.manager_count = 1;
        let text = b"Mo 01.12.1993^Die Stadionrenovierung^ist abgeschlossen.^ ^".to_vec();
        let events = [FinanceEvent::ConstructionFinished {
            item: 5,
            shown: Some((text, 1)),
        }];
        finance_dialogs(&mut host, &state, 0, &events, bitmaps, false).unwrap();
        assert_eq!(
            host.script_clicks_left(),
            0,
            "the viewer waited for the click"
        );
        assert!(host.g.blanked, "screen_fade(0) after the viewer");
        std::fs::remove_dir_all(&shots).unwrap();
    }

    #[test]
    fn the_christmas_bonus_lines_split_the_bonus_by_tier() {
        let fmt = NumberFormat {
            pad_width: 1,
            ..NumberFormat::default()
        };
        let [l1, l2] = christmas_bonus_lines(fmt, 850_000);
        assert_eq!(l1, b"BUNDESLIGA: 850.000 DM, 2.LIGA: 425.000 DM,");
        assert_eq!(l2, b"AMATEUR-OBERLIGA: 283.333 DM (INCL. MWST.)");
    }

    #[test]
    fn a_mod_game_pays_the_third_bonus_to_the_3_liga() {
        let fmt = NumberFormat {
            pad_width: 1,
            ..NumberFormat::default()
        };
        let original = christmas_bonus_lines(fmt, 850_000);
        set_layout(&MOD2026);
        let modded = christmas_bonus_lines(fmt, 850_000);
        set_layout(&ORIGINAL);
        assert_eq!(modded[0], original[0]);
        assert_eq!(modded[1], b"3. LIGA: 283.333 DM (INCL. MWST.)");
    }
}
