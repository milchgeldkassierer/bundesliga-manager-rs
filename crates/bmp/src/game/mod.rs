//! Screen flow of BMMAIN.EXE, ported as blocking code on top of [`Host`].
//! Spec: `docs/re/logic/flow.md`.

pub mod contract;
pub mod credits;
pub mod cup_screens;
pub mod disk;
pub mod live_match;
pub mod load_game;
pub mod negotiation;
pub mod newspaper;
pub mod office;
pub mod scorers;
pub mod season;
pub mod squad_screen;
pub mod stadium;
pub mod start_menu;
pub mod stats_screens;
pub mod tables;
pub mod training;
pub mod transfer_screen;
pub mod widgets;

use bmp_formats::mana::ManaDat;
use bmp_game::newgame::{self, CupDraws};
use bmp_game::rng::Rng;
use bmp_game::runtime::Runtime;
use bmp_game::state::GERMAN_CLUBS;

pub use bmp_game::runtime::SEASON_START_DAY;

use crate::host::{Host, LEFT_DOWN, Run};

/// `DS:224e/2250`: game option long A (1 in the shipped data; 4 selects the
/// English variant: ordinal suffixes, bench of 13).
pub const GAME_OPTION_A: i32 = 1;
/// `DS:2252/2254`: game option long B (1 in the shipped data; 3 selects the
/// French ordinal suffixes).
pub const GAME_OPTION_B: i32 = 1;

/// Game globals that live outside the save game.
pub struct Session {
    pub rng: Rng,
    /// The non-saved logic globals.
    pub rt: Runtime,
    /// `DS:05ac`: the copy protection counts as passed (initial data value 1).
    pub copy_protection_ok: bool,
    /// `DS:05d4`: automatic play ("Spiele automatisch") ends every turn at once.
    pub auto_play: bool,
    pub saves: load_game::SaveSession,
    /// `DS:0044/0046`: position and width of the status-line text.
    pub status_line: stadium::StatusLine,
    /// `DS:05ad`: set after a save from the menu; blocks saving again and
    /// ending the turn with a click (cleared by the settings cheat `d`).
    pub save_disabled: bool,
    /// `DS:05f5`: set by the settings cheat `p` (meaning not traced).
    pub cheat_05f5: bool,
    /// `DS:0668`: the tag written into new high-score entries (+0x36); 0 in
    /// the data image.
    /// TODO(re): its writer is not traced.
    pub highscore_tag: i32,
    /// `DS:5258`: weekly training-camp prices; scaled once per program run
    /// by the first visit of the camp screen.
    pub camp_prices: [i32; 8],
}

impl Session {
    pub fn new(rng: Rng) -> Self {
        Self {
            rng,
            rt: Runtime::default(),
            copy_protection_ok: true,
            auto_play: false,
            saves: load_game::SaveSession::default(),
            status_line: stadium::StatusLine::default(),
            save_disabled: false,
            cheat_05f5: false,
            highscore_tag: 0,
            camp_prices: training::CAMP_BASE_PRICES,
        }
    }
}

/// How [`run`] ended.
#[derive(Debug)]
pub enum Exit {
    /// The last season of the game is over.
    GameOver,
}

/// `time(NULL)`: the seconds since the epoch, the source of both RNG seeds.
pub fn time_now() -> i32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i32)
}

/// The MANA.DAT of the game data.
fn load_mana(host: &Host) -> Result<ManaDat, String> {
    let path = host.data.join("MANA.DAT");
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    ManaDat::parse(&bytes).map_err(|e| e.to_string())
}

/// Shows the blue marble background: decoded once from `PIC\22.CP` and cached in
/// VRAM page 2, later restored from there. The screen stays faded out.
///
/// Origin: BMMAIN.EXE 18bc:3b92 (office_show_cached_background)
pub fn office_show_cached_background(host: &mut Host) -> Run<()> {
    host.screen_fade(0)?;
    host.g.mouse_pointer_hide(0);
    if !host.office_bg_cached {
        // The file is part of the game data; a failure here is fatal in the original.
        host.load_screen_picture(22)
            .expect("PIC\\22.CP (fatal_error_exit 2 in the original)");
        host.g.vga_copy_page(0, 0x9600);
        host.office_bg_cached = true;
    } else {
        host.g.vga_copy_page(0x9600, 0);
    }
    host.g.mouse_pointer_show();
    Ok(())
}

/// Upper-cases ASCII letters and the umlauts ä, ö, ü (CP437 0x84, 0x94, 0x81).
///
/// Origin: BMMAIN.EXE 4091:105d (text_to_upper_de)
pub fn text_to_upper_de(text: &[u8]) -> Vec<u8> {
    text.iter()
        .map(|&c| match c {
            b'a'..=b'z' => c - 0x20,
            0x81 => 0x9a,
            0x84 => 0x8e,
            0x94 => 0x99,
            _ => c,
        })
        .collect()
}

/// Appends the ordinal suffix of `n` for the language selected by the game
/// options: `"st"/"nd"/"rd"/"th"` (option A = 4), `"re"/"e"` (option B = 3) or `"."`.
///
/// Origin: BMMAIN.EXE 1310:1fe7 (append_ordinal_suffix)
pub fn append_ordinal_suffix(buf: &mut Vec<u8>, n: u8, option_a: i32, option_b: i32) {
    let n = n as i8;
    let rem = n % 10;
    let suffix: &[u8] = if option_a == 4 {
        if rem < 4 && rem != 0 && !(10..=20).contains(&n) {
            [b"".as_slice(), b"st", b"nd", b"rd"][rem as usize]
        } else {
            b"th"
        }
    } else if option_b == 3 {
        if rem == 1 { b"re" } else { b"e" }
    } else {
        b"."
    };
    buf.extend_from_slice(suffix);
}

/// Pictures `main` keeps loaded for the whole game.
fn load_main_pictures(host: &mut Host) -> Result<(), String> {
    for i in 0..2u16 {
        let frame = host.load_picture(0x18 + i)?;
        host.ui.frames[i as usize] = Some(frame);
    }
    Ok(())
}

/// `main`: title picture, club selection, then the season loop of a new or a loaded
/// game. `time` is the first seed's `time()`; `time2` the second one's, read
/// once the start menu has set up a new game (`None`: the clock at that moment).
///
/// Origin: BMMAIN.EXE 18bc:0418 (main)
pub fn run(host: &mut Host, time: i32, time2: Option<i32>) -> Result<Run<Exit>, String> {
    host.load_palette_slots()?;
    let _ = host.screen_fade(0);
    // Sound Blaster present and sound not disabled: start the title tune.
    let music = host.music_start()?;
    host.load_screen_picture(47)?;
    host.g.palette_load_slot(2, 0x100);
    host.g.mouse_pointer_show();
    if host.screen_fade(1).is_err() {
        return Ok(Err(crate::host::Quit));
    }
    // The title stays until the tune ends or the player clicks. Without music the
    // only pause is the `delay_ticks(150)` below.
    if music {
        while host.music_poll_status() != 0 {
            match host.input_poll_mouse() {
                Err(q) => return Ok(Err(q)),
                Ok(LEFT_DOWN) => host.music_stop(),
                Ok(_) => {}
            }
        }
    }
    host.voice_init()?;
    let mana = load_mana(host)?;
    let mut session = Session::new(Rng::default());
    // TODO(#15): the "An Alle" draw questions and the draw screens are not shown; every
    // question is answered "no".
    let mut cups = CupDraws(|_| false);
    let mut world = newgame::startup(&mana, &mut session.rt, &mut session.rng, time, &mut cups);
    load_main_pictures(host)?;
    let pics = start_menu::Pictures::load(host)?;
    // The 32x23 picture of icon code 0x23, cut from the icon sheet.
    let icon_alt = host.blank_bitmap(0x20, 0x17, 0);
    let icons = host.load_picture(3)?;
    host.g
        .blit_bitmap(icons, 0xa5, 0x78, icon_alt, 0, 0, 0x20, 0x17);
    host.ui.icons = Some(icons);
    host.ui.icon_alt = Some(icon_alt);
    if let Err(q) = host.delay_ticks(150) {
        return Ok(Err(q));
    }
    let outcome = match start_menu::start_menu_new_or_load(
        host,
        &pics,
        &world.clubs[..GERMAN_CLUBS],
        &mut session,
    )? {
        Ok(o) => o,
        Err(q) => return Ok(Err(q)),
    };
    let (mut state, resume) = match outcome {
        start_menu::Outcome::NewGame(choices) => {
            // `bitmap_free(FAR:57d4)`: the icon sheet, reloaded by `office_load_bitmaps(2)`.
            if let Some(icons) = host.ui.icons.take() {
                host.g.free(icons);
            }
            let time2 = time2.unwrap_or_else(time_now);
            newgame::new_game(
                &mut world,
                &mut session.rt,
                &mut session.rng,
                &mana,
                &choices,
                time2,
                &mut cups,
            );
            (world, false)
        }
        start_menu::Outcome::LoadGame(state) => (*state, true),
    };
    let bitmaps = office::OfficeBitmaps::load(host, if resume { 1 } else { 2 })?;
    // TODO(#7): tore_scene_open 24a4:0419 opens TORE\ANZAHL for the goal scenes.
    Ok(season::season_main_loop(
        host,
        &mut session,
        &pics,
        &bitmaps,
        &mut state,
        resume,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upper_case_handles_umlauts() {
        assert_eq!(text_to_upper_de(b"Spielstand Laden"), b"SPIELSTAND LADEN");
        assert_eq!(
            text_to_upper_de(&[0x84, 0x94, 0x81, b'1']),
            [0x8e, 0x99, 0x9a, b'1']
        );
    }

    #[test]
    fn ordinal_suffixes_follow_the_options() {
        let s = |n: u8, a: i32, b: i32| {
            let mut v = Vec::new();
            append_ordinal_suffix(&mut v, n, a, b);
            v
        };
        assert_eq!(s(15, 1, 1), b".");
        assert_eq!(s(1, 4, 1), b"st");
        assert_eq!(s(22, 4, 1), b"nd");
        assert_eq!(s(13, 4, 1), b"th");
        assert_eq!(s(20, 4, 1), b"th");
        assert_eq!(s(1, 1, 3), b"re");
        assert_eq!(s(2, 1, 3), b"e");
    }
}
