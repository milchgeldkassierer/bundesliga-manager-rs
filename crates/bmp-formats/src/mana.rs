//! `MANA.DAT`: the club and player-name database.
//!
//! The file is a fixed-size (38864 byte) sequence of plain, unencrypted blocks.
//! It is written by the team editor `TEAMED.EXE` and only read by `BMMAIN.EXE`.
//! See `docs/re/mana.md` for the full specification.
//!
//! All Ghidra addresses refer to the EXEPACK-unpacked images imported into the
//! shared Ghidra project as `/unpacked/BMMAIN_UNPACKED.EXE` and
//! `/unpacked/TEAMED_UNPACKED.EXE` (the packed originals cannot be analyzed).
//!
//! Layout (all offsets in bytes):
//!
//! | offset | size  | content                                                    |
//! |--------|-------|------------------------------------------------------------|
//! | 0x0000 | 64    | crest picture number per German club (`PIC/<n>.VGA`)       |
//! | 0x0040 | 600   | 3 strength bytes for each of the 200 clubs                 |
//! | 0x0298 | 128   | previous season points (plus, minus) per German club       |
//! | 0x0318 | 128   | previous season goals (scored, conceded) per German club   |
//! | 0x0398 | 64    | previous season final rank per German club                 |
//! | 0x03d8 | 37880 | 200 club records: 23 byte name, clubs 0..63 add 20 x 26 byte player names |

use std::fmt;

/// Exact size of `MANA.DAT`.
pub const FILE_SIZE: usize = 38864;
/// Number of club records (64 German clubs followed by 136 foreign clubs).
pub const CLUB_COUNT: usize = 200;
/// Number of German clubs; only these carry player names and table statistics.
pub const GERMAN_CLUB_COUNT: usize = 64;
/// Player name slots per German club.
pub const PLAYERS_PER_CLUB: usize = 20;
/// Size of a club name field (NUL terminated, may contain stale bytes after the NUL).
pub const CLUB_NAME_LEN: usize = 23;
/// Size of a player name field (NUL terminated).
pub const PLAYER_NAME_LEN: usize = 26;

/// Offset of the strength block.
pub const STRENGTH_OFFSET: usize = 0x40;
/// Offset of the points block.
pub const POINTS_OFFSET: usize = 0x298;
/// Offset of the goals block.
pub const GOALS_OFFSET: usize = 0x318;
/// Offset of the rank block.
pub const RANK_OFFSET: usize = 0x398;
/// Offset of the first club record (equals the 0x3d8 header bytes BMMAIN skips).
pub const CLUBS_OFFSET: usize = 0x3d8;
/// Size of a German club record: name plus 20 player names (0x21f).
pub const GERMAN_CLUB_RECORD_LEN: usize = CLUB_NAME_LEN + PLAYERS_PER_CLUB * PLAYER_NAME_LEN;

/// Error returned when `MANA.DAT` cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManaError {
    /// The input does not have the fixed file size.
    WrongSize(usize),
}

impl fmt::Display for ManaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManaError::WrongSize(n) => {
                write!(f, "MANA.DAT must be {FILE_SIZE} bytes, got {n}")
            }
        }
    }
}

impl std::error::Error for ManaError {}

/// One club record of the name block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Club {
    /// Raw 23 byte name field, CP437, NUL terminated. Bytes after the NUL are kept
    /// verbatim (the original file contains stale editor bytes there).
    pub name: [u8; CLUB_NAME_LEN],
    /// Raw 26 byte player name fields; 20 entries for clubs 0..63, empty for
    /// foreign clubs. An empty slot starts with a NUL byte.
    pub players: Vec<[u8; PLAYER_NAME_LEN]>,
}

impl Club {
    /// Club name up to (excluding) the first NUL, as raw CP437 bytes.
    pub fn name_bytes(&self) -> &[u8] {
        c_str(&self.name)
    }

    /// Player name `slot` up to the first NUL, as raw CP437 bytes.
    pub fn player_name_bytes(&self, slot: usize) -> Option<&[u8]> {
        self.players.get(slot).map(|p| c_str(p))
    }
}

/// Parsed contents of `MANA.DAT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaDat {
    /// Crest picture number of each German club as stored in the file
    /// (the game shows `PIC/<n>.VGA`). See [`ManaDat::crest_index`].
    pub crest: [u8; GERMAN_CLUB_COUNT],
    /// Three strength values per club (0..=99 in the shipped data).
    pub strength: [[u8; 3]; CLUB_COUNT],
    /// Previous season points `[plus, minus]` per German club (2-point rule).
    pub points: [[u8; 2]; GERMAN_CLUB_COUNT],
    /// Previous season goals `[scored, conceded]` per German club.
    pub goals: [[u8; 2]; GERMAN_CLUB_COUNT],
    /// Previous season final table position per German club (0 = none).
    pub rank: [u8; GERMAN_CLUB_COUNT],
    /// The 200 club records in file order.
    pub clubs: Vec<Club>,
}

/// Returns the bytes of `field` before the first NUL (or all of it).
pub fn c_str(field: &[u8]) -> &[u8] {
    match field.iter().position(|&b| b == 0) {
        Some(n) => &field[..n],
        None => field,
    }
}

impl ManaDat {
    /// Parses a complete `MANA.DAT` image.
    ///
    /// Block order and sizes follow the TEAMED loader exactly (the BMMAIN
    /// loaders read the same blocks but skip some of them).
    ///
    /// Origin: TEAMED.EXE 1000:1762 (teamed_init_and_load_mana)
    pub fn parse(data: &[u8]) -> Result<ManaDat, ManaError> {
        if data.len() != FILE_SIZE {
            return Err(ManaError::WrongSize(data.len()));
        }
        let mut crest = [0u8; GERMAN_CLUB_COUNT];
        crest.copy_from_slice(&data[..GERMAN_CLUB_COUNT]);

        let mut strength = [[0u8; 3]; CLUB_COUNT];
        for (i, s) in strength.iter_mut().enumerate() {
            let o = STRENGTH_OFFSET + i * 3;
            s.copy_from_slice(&data[o..o + 3]);
        }

        let mut points = [[0u8; 2]; GERMAN_CLUB_COUNT];
        let mut goals = [[0u8; 2]; GERMAN_CLUB_COUNT];
        let mut rank = [0u8; GERMAN_CLUB_COUNT];
        for i in 0..GERMAN_CLUB_COUNT {
            let p = POINTS_OFFSET + i * 2;
            points[i].copy_from_slice(&data[p..p + 2]);
            let g = GOALS_OFFSET + i * 2;
            goals[i].copy_from_slice(&data[g..g + 2]);
            rank[i] = data[RANK_OFFSET + i];
        }

        let mut clubs = Vec::with_capacity(CLUB_COUNT);
        let mut o = CLUBS_OFFSET;
        for i in 0..CLUB_COUNT {
            let mut name = [0u8; CLUB_NAME_LEN];
            name.copy_from_slice(&data[o..o + CLUB_NAME_LEN]);
            o += CLUB_NAME_LEN;
            let mut players = Vec::new();
            if i < GERMAN_CLUB_COUNT {
                players.reserve(PLAYERS_PER_CLUB);
                for _ in 0..PLAYERS_PER_CLUB {
                    let mut p = [0u8; PLAYER_NAME_LEN];
                    p.copy_from_slice(&data[o..o + PLAYER_NAME_LEN]);
                    players.push(p);
                    o += PLAYER_NAME_LEN;
                }
            }
            clubs.push(Club { name, players });
        }
        debug_assert_eq!(o, FILE_SIZE);

        Ok(ManaDat {
            crest,
            strength,
            points,
            goals,
            rank,
            clubs,
        })
    }

    /// Serializes back to the exact on-disk layout written by the team editor.
    ///
    /// Clubs with fewer than 20 player slots are padded with NUL slots; surplus
    /// slots and any player slots on foreign clubs are ignored, as the format has
    /// no room for them.
    ///
    /// Origin: TEAMED.EXE 1579:04b5 (teamed_save_mana)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(FILE_SIZE);
        out.extend_from_slice(&self.crest);
        for s in &self.strength {
            out.extend_from_slice(s);
        }
        for p in &self.points {
            out.extend_from_slice(p);
        }
        for g in &self.goals {
            out.extend_from_slice(g);
        }
        out.extend_from_slice(&self.rank);
        let empty_club = Club {
            name: [0; CLUB_NAME_LEN],
            players: Vec::new(),
        };
        for i in 0..CLUB_COUNT {
            let club = self.clubs.get(i).unwrap_or(&empty_club);
            out.extend_from_slice(&club.name);
            if i < GERMAN_CLUB_COUNT {
                for slot in 0..PLAYERS_PER_CLUB {
                    match club.players.get(slot) {
                        Some(p) => out.extend_from_slice(p),
                        None => out.extend_from_slice(&[0u8; PLAYER_NAME_LEN]),
                    }
                }
            }
        }
        debug_assert_eq!(out.len(), FILE_SIZE);
        out
    }

    /// In-memory crest value BMMAIN derives from the file byte of German club
    /// `club`: `(b + 0x30) & 0x7f`, i.e. `b - 0x50` for the shipped range
    /// 0x50..=0x8f. The game later renders picture `(value & 0x7f) + 0x50`
    /// and uses bit 7 of the in-memory byte as a runtime flag.
    ///
    /// Origin: BMMAIN.EXE 362f:3759 (mana_load_club_table)
    pub fn crest_index(&self, club: usize) -> u8 {
        self.crest[club].wrapping_add(0x30) & 0x7f
    }

    /// File offset of player name `slot` of German club `club`, as computed by
    /// BMMAIN after it has located the club block by name (`0x21f` bytes per
    /// club, 23 byte name, `0x1a` bytes per player).
    ///
    /// Origin: BMMAIN.EXE 422b:01d0 (mana_find_player_name)
    pub fn player_name_offset(club: usize, slot: usize) -> usize {
        CLUBS_OFFSET + club * GERMAN_CLUB_RECORD_LEN + CLUB_NAME_LEN + slot * PLAYER_NAME_LEN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(rel: &str) -> Option<Vec<u8>> {
        let dir = crate::data_dir()?;
        std::fs::read(dir.join(rel)).ok()
    }

    fn synthetic() -> Vec<u8> {
        (0..FILE_SIZE).map(|i| (i * 7 + 3) as u8).collect()
    }

    #[test]
    fn layout_constants_add_up() {
        assert_eq!(GERMAN_CLUB_RECORD_LEN, 0x21f);
        assert_eq!(CLUBS_OFFSET, 64 + 600 + 128 + 128 + 64);
        assert_eq!(
            CLUBS_OFFSET
                + GERMAN_CLUB_COUNT * GERMAN_CLUB_RECORD_LEN
                + (CLUB_COUNT - GERMAN_CLUB_COUNT) * CLUB_NAME_LEN,
            FILE_SIZE
        );
        // BMMAIN reads 0x40 + 0x258 + 0x140 + 0x93f8 bytes.
        assert_eq!(0x40 + 0x258 + 0x140 + 0x93f8, FILE_SIZE);
        // The player-name loader reads 0x3d8 + 0x87c0 bytes.
        assert_eq!(0x3d8 + 0x87c0, CLUBS_OFFSET + 64 * GERMAN_CLUB_RECORD_LEN);
    }

    #[test]
    fn synthetic_round_trip() {
        let data = synthetic();
        let m = ManaDat::parse(&data).unwrap();
        assert_eq!(m.to_bytes(), data);
    }

    #[test]
    fn rejects_wrong_size() {
        assert_eq!(ManaDat::parse(&[0u8; 10]), Err(ManaError::WrongSize(10)));
    }

    #[test]
    fn crest_index_wraps_like_original() {
        let mut m = ManaDat::parse(&synthetic()).unwrap();
        m.crest[0] = 0x58;
        assert_eq!(m.crest_index(0), 8);
        m.crest[0] = 0x8f;
        assert_eq!(m.crest_index(0), 63);
        m.crest[0] = 0x10;
        assert_eq!(m.crest_index(0), 0x40);
    }

    #[test]
    fn original_files_round_trip() {
        for rel in ["MANA.DAT", "BMP/MANA.DAT"] {
            let Some(data) = load(rel) else {
                return;
            };
            let m = ManaDat::parse(&data).unwrap();
            assert_eq!(m.to_bytes(), data, "{rel}");
        }
    }

    #[test]
    fn original_contents() {
        let Some(data) = load("MANA.DAT") else {
            return;
        };
        let m = ManaDat::parse(&data).unwrap();
        assert_eq!(m.clubs.len(), CLUB_COUNT);
        assert_eq!(m.clubs[0].name_bytes(), b"1.FC KAISERSLAUTERN");
        assert_eq!(m.clubs[1].name_bytes(), b"FC BAYERN M\x9aNCHEN");
        assert_eq!(m.clubs[64].name_bytes(), b"FC LIVERPOOL");
        assert_eq!(m.clubs[199].name_bytes(), b"ANDORRA KICKERS");
        assert_eq!(m.clubs[0].player_name_bytes(0), Some(&b"EHRMANN"[..]));
        assert!(m.clubs[64].players.is_empty());
        // Stale bytes after the NUL are preserved.
        assert_eq!(&m.clubs[26].name[..16], b"SVW MANNHEIM\x0098\x00");
        assert_eq!(m.crest[0], 0x58);
        assert_eq!(m.crest_index(0), 8);
        // Crest picture numbers are a permutation of 80..=143.
        let mut c = m.crest;
        c.sort_unstable();
        assert!(c.iter().enumerate().all(|(i, &b)| b as usize == 0x50 + i));
        assert_eq!(m.strength[0], [94, 94, 95]);
        assert_eq!(m.points[0], [48, 20]);
        assert_eq!(m.goals[0], [72, 45]);
        assert_eq!(m.rank[0], 1);
        let off = ManaDat::player_name_offset(0, 1);
        assert_eq!(&data[off..off + 4], b"SERR");
    }
}
