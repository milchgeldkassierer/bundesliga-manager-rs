//! Screen flow of BMMAIN.EXE, ported as blocking code on top of [`Host`].
//! Spec: `docs/re/logic/flow.md`.

pub mod league;
pub mod load_game;
pub mod office;
pub mod season;
pub mod squad_screen;
pub mod start_menu;
pub mod tables;
pub mod widgets;

use bmp_formats::mana::{self, ManaDat};
use bmp_game::league::league_team_ids;
use bmp_game::rng::Rng;

use crate::host::{Host, LEFT_DOWN, Run};

/// The part of a club record (`CLUB[i]`, 0x22 bytes) the start menu needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClubEntry {
    pub name: Vec<u8>,
    /// Crest code; the picture is `PIC\<code + 0x50>.VGA`.
    pub crest: u8,
}

/// `DS:224c`: season start-day offset (0 in the shipped data).
pub const SEASON_START_DAY: i8 = 0;
/// `DS:224e/2250`: game option long A (1 in the shipped data; 4 selects the
/// English variant: ordinal suffixes, bench of 13).
pub const GAME_OPTION_A: i32 = 1;
/// `DS:2252/2254`: game option long B (1 in the shipped data; 3 selects the
/// French ordinal suffixes).
pub const GAME_OPTION_B: i32 = 1;

/// Game globals that live outside the save game.
pub struct Session {
    pub rng: Rng,
    /// `DS:05ac`: the copy protection counts as passed (initial data value 1).
    pub copy_protection_ok: bool,
    /// `DS:05d4`: automatic play ("Spiele automatisch") ends every turn at once.
    pub auto_play: bool,
    pub saves: load_game::SaveSession,
}

impl Session {
    pub fn new(rng: Rng) -> Self {
        Self {
            rng,
            copy_protection_ok: true,
            auto_play: false,
            saves: load_game::SaveSession::default(),
        }
    }
}

/// How [`run`] ended.
#[derive(Debug)]
pub enum Exit {
    /// The player set up a new game (the world initialisation is not ported yet).
    NewGame(start_menu::NewGame),
    /// A loaded game's office turn ended; the season loop is not ported yet.
    EndOfTurn,
}

/// The 64 German club slots in MANA.DAT order.
pub fn load_clubs(host: &Host) -> Result<Vec<ClubEntry>, String> {
    let path = host.data.join("MANA.DAT");
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let dat = ManaDat::parse(&bytes).map_err(|e| e.to_string())?;
    Ok((0..mana::GERMAN_CLUB_COUNT)
        .map(|i| ClubEntry {
            name: dat.clubs[i].name_bytes().to_vec(),
            crest: dat.crest_index(i),
        })
        .collect())
}

/// Shuffles the clubs inside each tier: 55 random slot swaps per tier.
///
/// Only the club records are swapped here; the full world state (pool players,
/// statistics, league order list) moves with them once the new-game
/// initialisation is ported (#30).
///
/// Origin: BMMAIN.EXE 1310:09c5 (league_shuffle_teams)
pub fn league_shuffle_teams(rng: &mut Rng, clubs: &mut [ClubEntry]) {
    for t in 0..3 {
        let ids = league_team_ids(t);
        let (lo, hi) = (ids.start as i16, ids.end as i16 - 1);
        for _ in 0..0x37 {
            let a = rng.random_range(lo, hi) as usize;
            let b = rng.random_range(lo, hi) as usize;
            clubs.swap(a, b);
        }
    }
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

/// `main`: title picture, club selection, then either a new game or a loaded game's
/// first office turn.
///
/// Origin: BMMAIN.EXE 18bc:0418 (main)
pub fn run(host: &mut Host, time: i32) -> Result<Run<Exit>, String> {
    host.load_palette_slots()?;
    // TODO(#17): music. Without a sound card the original shows the title for the
    // `delay_ticks(150)` below only; with music it waits until the tune ends or the
    // player clicks. The port waits for a click, as with music.
    let _ = host.screen_fade(0);
    host.load_screen_picture(47)?;
    host.g.palette_load_slot(2, 0x100);
    host.g.mouse_pointer_show();
    if host.screen_fade(1).is_err() {
        return Ok(Err(crate::host::Quit));
    }
    loop {
        match host.input_poll_mouse() {
            Err(q) => return Ok(Err(q)),
            Ok(LEFT_DOWN) => break,
            Ok(_) => {}
        }
    }
    load_main_pictures(host)?;
    let mut rng = Rng::new(0);
    rng.seed_from_time(time);
    let mut clubs = load_clubs(host)?;
    league_shuffle_teams(&mut rng, &mut clubs);
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
    let mut session = Session::new(rng);
    let outcome = match start_menu::start_menu_new_or_load(host, &pics, &clubs, &mut session)? {
        Ok(o) => o,
        Err(q) => return Ok(Err(q)),
    };
    let mut state = match outcome {
        start_menu::Outcome::NewGame(ng) => {
            // TODO(#30): new-game world initialisation, then the office.
            return Ok(Ok(Exit::NewGame(ng)));
        }
        start_menu::Outcome::LoadGame(state) => state,
    };
    let bitmaps = office::OfficeBitmaps::load(host, 1)?;
    // TODO(#7): tore_scene_open 24a4:0419 opens TORE\ANZAHL for the goal scenes.
    Ok(season::season_main_loop_resume(
        host,
        &mut session,
        &pics,
        &bitmaps,
        &mut state,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seed 35372 was recovered from a DOSBox run of the original: its carousel
    /// showed exactly these seven clubs, and no other 16-bit seed produces them.
    #[test]
    fn league_shuffle_matches_original_carousel() {
        let Some(dir) = bmp_formats::data_dir() else {
            return;
        };
        let bytes = std::fs::read(dir.join("MANA.DAT")).unwrap();
        let dat = ManaDat::parse(&bytes).unwrap();
        let mut clubs: Vec<ClubEntry> = (0..mana::GERMAN_CLUB_COUNT)
            .map(|i| ClubEntry {
                name: dat.clubs[i].name_bytes().to_vec(),
                crest: dat.crest_index(i),
            })
            .collect();
        let mut rng = Rng::new(0);
        rng.seed_from_time(35372);
        league_shuffle_teams(&mut rng, &mut clubs);
        let first: Vec<&[u8]> = clubs[..7].iter().map(|c| c.name.as_slice()).collect();
        assert_eq!(first[0], b"DYNAMO DRESDEN");
        assert_eq!(first[2], b"BORUSSIA DORTMUND");
        assert_eq!(first[3], b"HAMBURGER SV");
        assert_eq!(first[4], b"VFL BOCHUM");
        assert_eq!(first[5], b"WATTENSCHEID 09");
    }

    #[test]
    fn league_shuffle_keeps_clubs_inside_their_tier() {
        let mut clubs: Vec<ClubEntry> = (0..64u8)
            .map(|i| ClubEntry {
                name: vec![i],
                crest: i,
            })
            .collect();
        league_shuffle_teams(&mut Rng::new(7), &mut clubs);
        for t in 0..3 {
            let ids = league_team_ids(t);
            let range = ids.start as usize..ids.end as usize;
            let mut tier: Vec<u8> = clubs[range].iter().map(|c| c.crest).collect();
            tier.sort_unstable();
            assert_eq!(tier, ids.collect::<Vec<_>>());
        }
        assert!(clubs[58..].iter().map(|c| c.crest).eq(58..64));
    }

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
