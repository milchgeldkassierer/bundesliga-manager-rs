//! `fatal_error_exit`: the error screen, the crash save `ERROR.MAN` and the end of the
//! program. Spec: `docs/re/logic/flow.md` §3.5.
//!
//! The original calls it at the failure itself (25 call sites: code 0 when the screen or the
//! mouse cannot be opened, 2 for a missing file, 3 for the fonts, 4 for a failed allocation,
//! 99 for the normal end). The port records the code in [`Host::fatal`] and unwinds with
//! [`Quit`](crate::host::Quit) to `game::run`, which still holds the session and the game
//! state, and calls [`fatal_error_exit`] there.

use bmp_game::state::GameState;

use super::Session;
use super::disk::screen_save_game;
use super::widgets::input_wait_click;
use crate::gfx::ui_draw_text_centered_shadow;
use crate::host::{Host, Run};

/// `fatal_error_exit(0)`: `screen_open` or `mouse_open` failed (startup 18bc:005b/00a0).
pub const GRAPHICS_ERROR: u8 = 0;
/// `fatal_error_exit(2)`: a data file is missing (pictures, sounds, goal scenes, palettes).
pub const DISK_ERROR: u8 = 2;
/// `fatal_error_exit(3)`: `font_load_all` failed (startup 18bc:010a).
pub const FONT_ERROR: u8 = 3;
/// `fatal_error_exit(99)`: the normal end of the program, no message and no crash save.
pub const NORMAL_EXIT: u8 = 99;

/// The message per code from the jump table at 176b:11a6 (strings `DS:01b3..01ed`); note
/// that the table maps code 0 to `"GRAFIK-Error"` and code 1 to `"LIB-Error"`. No caller
/// passes 1 or 5.
const ERROR_TEXTS: [&[u8]; 6] = [
    b"GRAFIK-Error",
    b"LIB-Error",
    b"DISK-Error",
    b"FONT-Error",
    b"MEMORY-Error",
    b"BildLade-Error",
];

/// `DS:01fc`: the name of the crash save.
pub const CRASH_SAVE: &[u8] = b"ERROR.MAN";

/// `LBL[348]` (`FAR:4f84` -> `DS:3e08`): the second line, drawn after the crash save.
pub const SAVED_NOTICE: &[u8] = b"SPIELSTAND WURDE GESICHERT (ERROR.MAN) - <TASTE>";

/// The first line of the error screen for `code`; codes above 5 leave the buffer empty.
pub fn error_text(code: u8) -> &'static [u8] {
    ERROR_TEXTS.get(usize::from(code)).copied().unwrap_or(b"")
}

/// Code 99 goes straight to the teardown. Any other code clears the screen, shows
/// `"<kind>-Error"` (font 3, centred at y 0x78, colour 0x11), turns the screen on, waits
/// 10 ticks, writes the crash save `ERROR.MAN` through the normal save path, shows
/// [`SAVED_NOTICE`] (font 2, y 0x8c) and waits for a click. The caller ends the program
/// (`exit(0)` after the teardown: sound, EMS, mouse, screen, bitmaps, fonts, messages).
///
/// DEVIATION: the original saves whatever is in memory, even before a game exists; the port
/// writes `ERROR.MAN` only when it has a session and a game state.
///
/// Origin: BMMAIN.EXE 176b:114c (fatal_error_exit)
pub fn fatal_error_exit(
    host: &mut Host,
    session: Option<&mut Session>,
    state: Option<&mut GameState>,
    code: u8,
) -> Run<()> {
    if code == NORMAL_EXIT {
        return Ok(());
    }
    host.g.rastport_clear(&host.rp, 0);
    host.set_font(3);
    ui_draw_text_centered_shadow(
        &mut host.g,
        &mut host.rp,
        error_text(code),
        0,
        0x140,
        0x78,
        0x11,
        0,
    );
    host.screen_fade(1)?;
    host.delay_ticks(10)?;
    let auto_play = session.as_ref().is_some_and(|s| s.auto_play);
    if let (Some(session), Some(state)) = (session, state) {
        screen_save_game(host, session, state, CRASH_SAVE)?;
    }
    host.set_font(2);
    ui_draw_text_centered_shadow(
        &mut host.g,
        &mut host.rp,
        SAVED_NOTICE,
        0,
        0x140,
        0x8c,
        0x11,
        0,
    );
    input_wait_click(host, auto_play)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_map_to_the_jump_table_texts() {
        assert_eq!(error_text(GRAPHICS_ERROR), b"GRAFIK-Error");
        assert_eq!(error_text(1), b"LIB-Error");
        assert_eq!(error_text(DISK_ERROR), b"DISK-Error");
        assert_eq!(error_text(FONT_ERROR), b"FONT-Error");
        assert_eq!(error_text(4), b"MEMORY-Error");
        assert_eq!(error_text(5), b"BildLade-Error");
        assert_eq!(error_text(6), b"");
        assert_eq!(error_text(NORMAL_EXIT), b"");
    }

    /// A temporary game directory: the data linked, except `PIC\<missing>` and the
    /// crash save (the data copy stays untouched).
    #[cfg(unix)]
    fn data_without(data: &std::path::Path, tag: &str, missing: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bmp-fatal-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("PIC")).unwrap();
        for e in std::fs::read_dir(data).unwrap().flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.eq_ignore_ascii_case("PIC") {
                for p in std::fs::read_dir(e.path()).unwrap().flatten() {
                    let pname = p.file_name().to_string_lossy().to_string();
                    if !pname.eq_ignore_ascii_case(missing) {
                        std::os::unix::fs::symlink(p.path(), dir.join("PIC").join(&pname)).unwrap();
                    }
                }
            } else if !name.eq_ignore_ascii_case("ERROR.MAN") {
                std::os::unix::fs::symlink(e.path(), dir.join(&name)).unwrap();
            }
        }
        dir
    }

    /// The settings screen without `PIC\33.VGA` ends in `fatal_error_exit(2)`: the screen
    /// unwinds with the code recorded, and the error screen writes `ERROR.MAN`, the same bytes
    /// the normal save path writes from the same state and generator, then waits for a click.
    #[cfg(unix)]
    #[test]
    fn a_missing_picture_writes_the_crash_save() {
        use super::super::disk::{savegame_write_man, screen_settings};
        use crate::host::parse_script;
        use bmp_formats::savegame::SaveGame;
        use bmp_game::rng::Rng;
        let (Some(data), Some(save)) = (
            bmp_formats::data_dir(),
            bmp_formats::save_files().into_iter().next(),
        ) else {
            return;
        };
        let dir = data_without(&data, "save", "33.VGA");
        let load = || {
            GameState::from_savegame(&SaveGame::parse(&std::fs::read(&save).unwrap()).unwrap())
                .unwrap()
        };
        let script = parse_script("wait 2000\nclick 160 120\nwait 300\n").unwrap();
        let mut host = Host::with_script(&dir, script, &dir, false).unwrap();
        let mut session = Session::new(Rng::new(7));
        let mut state = load();

        let screen = screen_settings(&mut host, &mut session, &mut state);
        let code = host.fatal.take();
        let exit = fatal_error_exit(&mut host, Some(&mut session), Some(&mut state), DISK_ERROR);
        let written = std::fs::read(dir.join("ERROR.MAN")).ok();
        // The reference: the normal save path from a fresh copy of the same inputs.
        let reference = dir.join("REF.MAN");
        let mut ref_session = Session::new(Rng::new(7));
        savegame_write_man(
            &reference,
            &mut load(),
            &mut ref_session.rng,
            &mut ref_session.saves,
            ref_session.copy_protection_ok,
        )
        .unwrap();
        let expected = std::fs::read(&reference).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(screen.is_err(), "the settings screen ends the run");
        assert_eq!(code, Some(DISK_ERROR));
        assert_eq!(exit, Ok(()), "the click ends the error screen");
        let written = written.expect("ERROR.MAN written");
        assert!(
            written == expected,
            "ERROR.MAN equals a normal save of the state"
        );
        assert_eq!(
            session.rng, ref_session.rng,
            "the save draws the same random numbers"
        );
    }

    /// Code 99 is the normal end: no screen, no crash save.
    #[cfg(unix)]
    #[test]
    fn the_normal_exit_writes_nothing() {
        use bmp_game::rng::Rng;
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        let dir = data_without(&data, "normal", "");
        let mut host = Host::with_script(&dir, Vec::new(), &dir, false).unwrap();
        let mut session = Session::new(Rng::new(7));
        let mut state = GameState::zeroed();
        let r = fatal_error_exit(&mut host, Some(&mut session), Some(&mut state), NORMAL_EXIT);
        let written = dir.join("ERROR.MAN").exists();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(r, Ok(()));
        assert!(!written);
    }
}
