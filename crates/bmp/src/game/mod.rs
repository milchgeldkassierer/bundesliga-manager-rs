//! Screen flow of BMMAIN.EXE, ported as blocking code on top of [`Host`].
//! Spec: `docs/re/logic/flow.md`.

pub mod start_menu;

use bmp_formats::mana::{self, ManaDat};
use bmp_game::rng::Rng;

use crate::host::{Host, LEFT_DOWN, Run};

/// The part of a club record (`CLUB[i]`, 0x22 bytes) the start menu needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClubEntry {
    pub name: Vec<u8>,
    /// Crest code; the picture is `PIC\<code + 0x50>.VGA`.
    pub crest: u8,
}

/// League tiers: first slot (`DS:226e`) and club count (`DS:225e`).
pub const TIER_FIRST: [u8; 3] = [0, 18, 38];
pub const TIER_COUNT: [u8; 3] = [18, 20, 20];

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
        let lo = TIER_FIRST[t] as i16;
        let hi = lo + TIER_COUNT[t] as i16 - 1;
        for _ in 0..0x37 {
            let a = rng.random_range(lo, hi) as usize;
            let b = rng.random_range(lo, hi) as usize;
            clubs.swap(a, b);
        }
    }
}

/// `main` up to the start menu: title picture, then the club selection.
///
/// Origin: BMMAIN.EXE 18bc:0418 (main)
pub fn run(host: &mut Host, time: i32) -> Result<Run<start_menu::Outcome>, String> {
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
    let mut rng = Rng::new(0);
    rng.seed_from_time(time);
    let mut clubs = load_clubs(host)?;
    league_shuffle_teams(&mut rng, &mut clubs);
    let pics = start_menu::Pictures::load(host)?;
    if let Err(q) = host.delay_ticks(150) {
        return Ok(Err(q));
    }
    start_menu::start_menu_new_or_load(host, &pics, &clubs)
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
            let lo = TIER_FIRST[t] as usize;
            let hi = lo + TIER_COUNT[t] as usize;
            let mut tier: Vec<u8> = clubs[lo..hi].iter().map(|c| c.crest).collect();
            tier.sort_unstable();
            assert_eq!(tier, (lo as u8..hi as u8).collect::<Vec<_>>());
        }
        assert!(clubs[58..].iter().map(|c| c.crest).eq(58..64));
    }
}
