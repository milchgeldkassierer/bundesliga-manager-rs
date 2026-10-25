//! Save games (`*.MAN`) and high-score tables (`HIGH.XY`).
//!
//! Full specification: `docs/re/savegame.md`.
//!
//! `BMMAIN.EXE` is EXEPACK-compressed. All "real" addresses below are
//! `segment:offset` pairs in the *unpacked* load image (code segment `322b`,
//! DGROUP `4cb3`, far data segment `4238`). The Ghidra project imported the
//! packed file with a load base of `1000:0000`; for code below linear image
//! offset `0x35ad1` the Ghidra address is `linear + 0x10000` written as a
//! 64 KiB chunk, e.g. real `322b:07fe` = linear `0x32aae` = Ghidra `4000:2aae`.
//!
//! A `.MAN` file is not a single memory dump: the save routine writes about
//! seventy separate global variables / tables from two data segments, each
//! through a byte-wise stream cipher with a running plaintext checksum.

use std::fmt;

/// Size of every `.MAN` file produced by the game (string table empty).
pub const MAN_FILE_LEN: usize = 34381;
/// Signature written at the start of every save (DS:93bc).
pub const SIGNATURE: &[u8; 25] = b"WeRnEr_krahe~&~JeNs_onnen";
/// Number of signature bytes the loader actually compares (`strncmp(.., 24)`).
pub const SIGNATURE_CHECK_LEN: usize = 24;
/// Version tag written after the signature (DS:93e4, 4 bytes incl. NUL).
pub const VERSION_V2: [u8; 4] = *b">V2\0";
/// Increment applied to the cipher key after every encrypted byte.
pub const KEY_STEP: u8 = 0xd7;
/// Real-mode paragraph of DGROUP (the default data segment, `DS`).
pub const DGROUP_SEGMENT: u16 = 0x4cb3;
/// Real-mode paragraph of the far data segment holding most game tables.
pub const FAR_DATA_SEGMENT: u16 = 0x4238;
/// File offset of the first encrypted byte.
pub const ENCRYPTED_START: usize = 0x23;

/// Data segment a saved block lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Segment {
    /// DGROUP, paragraph `4cb3` (`DS`).
    Dgroup,
    /// Far data segment, paragraph `4238`.
    FarData,
}

impl Segment {
    /// Real-mode paragraph value of the segment.
    pub fn paragraph(self) -> u16 {
        match self {
            Segment::Dgroup => DGROUP_SEGMENT,
            Segment::FarData => FAR_DATA_SEGMENT,
        }
    }
}

/// One global variable or table written to the save file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockDesc {
    /// Segment of the variable.
    pub segment: Segment,
    /// Offset inside the segment.
    pub offset: u16,
    /// Number of bytes written.
    pub len: u16,
}

const fn blk(segment: Segment, offset: u16, len: u16) -> BlockDesc {
    BlockDesc {
        segment,
        offset,
        len,
    }
}

/// All encrypted fixed-size blocks in file order (call sites in
/// `savegame_write_man`, real `322b:07fe`). A raw checksum byte follows block
/// index [`MID_CHECKSUM_AFTER`]` - 1`.
pub const BLOCKS: [BlockDesc; 61] = [
    blk(Segment::Dgroup, 0x060e, 0x6),     // 322b:0b05
    blk(Segment::Dgroup, 0x0618, 0x4),     // 322b:0b1c  string-table group sizes
    blk(Segment::FarData, 0x5350, 0x4),    // 322b:0b33
    blk(Segment::FarData, 0x304a, 0x1),    // 322b:0b4a
    blk(Segment::FarData, 0x5358, 0x1),    // 322b:0b61
    blk(Segment::Dgroup, 0x079e, 0x8),     // 322b:0b78
    blk(Segment::FarData, 0x6ddc, 0x8e8),  // 322b:0b8f
    blk(Segment::Dgroup, 0x07ab, 0x1),     // 322b:0ba6  string-table group count
    blk(Segment::Dgroup, 0x07ac, 0x1),     // 322b:0bbd
    blk(Segment::Dgroup, 0x07ad, 0x1),     // 322b:0bd4
    blk(Segment::Dgroup, 0x07ae, 0x1),     // 322b:0beb
    blk(Segment::Dgroup, 0x07af, 0x1),     // 322b:0c02
    blk(Segment::Dgroup, 0x07b0, 0x1),     // 322b:0c19
    blk(Segment::FarData, 0x2242, 0xc28),  // 322b:0c30  4 manager records
    blk(Segment::FarData, 0x5714, 0x64),   // 322b:0c47
    blk(Segment::FarData, 0x3066, 0x1a90), // 322b:0c5e  200 club records
    blk(Segment::FarData, 0x0ecc, 0xd80),  // 322b:0c75
    blk(Segment::FarData, 0x57dd, 0x15d3), // 322b:0c8c  151 player records
    blk(Segment::FarData, 0x774a, 0x1964), // 322b:0ca3  5 x 25 records of 52 bytes
    blk(Segment::FarData, 0x4b5e, 0x3c),   // 322b:0cba
    blk(Segment::FarData, 0x2eba, 0x4),    // 322b:0cd1
    blk(Segment::FarData, 0x2e8e, 0x4),    // 322b:0ce8
    blk(Segment::FarData, 0xa7a0, 0x4),    // 322b:0cff
    blk(Segment::Dgroup, 0x07dc, 0x4),     // 322b:0d16
    blk(Segment::Dgroup, 0x07e0, 0x2),     // 322b:0d2d
    blk(Segment::FarData, 0x56fe, 0xa),    // 322b:0d44
    blk(Segment::FarData, 0x57aa, 0x12),   // 322b:0d5b
    // raw checksum byte (FAR:56dc) written here, 322b:0d71
    blk(Segment::FarData, 0x2e6e, 0x2),    // 322b:0d8a
    blk(Segment::FarData, 0x76ca, 0x80),   // 322b:0da1
    blk(Segment::FarData, 0x21e2, 0x60),   // 322b:0db8
    blk(Segment::FarData, 0x0008, 0x8),    // 322b:0dcf
    blk(Segment::FarData, 0x1d18, 0x3),    // 322b:0de6
    blk(Segment::FarData, 0x535a, 0x3c),   // 322b:0dfd
    blk(Segment::FarData, 0x4c7e, 0x80),   // 322b:0e14
    blk(Segment::Dgroup, 0x225a, 0x3),     // 322b:0e2b
    blk(Segment::FarData, 0x9336, 0x1394), // 322b:0e42
    blk(Segment::Dgroup, 0x05fe, 0xf),     // 322b:0e59
    blk(Segment::FarData, 0x0000, 0x8),    // 322b:0e70
    blk(Segment::FarData, 0x4b9a, 0x30),   // 322b:0e87
    blk(Segment::FarData, 0x2ec2, 0x50),   // 322b:0e9e
    blk(Segment::FarData, 0x002e, 0x140),  // 322b:0eb5
    blk(Segment::Dgroup, 0x066c, 0x90),    // 322b:0ecc
    blk(Segment::Dgroup, 0x4a28, 0x1),     // 322b:0ee3  difficulty level
    blk(Segment::FarData, 0x513c, 0x2),    // 322b:0efa
    blk(Segment::FarData, 0x1d34, 0x1e),   // 322b:0f11
    blk(Segment::FarData, 0x5664, 0x78),   // 322b:0f28  3 x 10 far string pointers
    blk(Segment::FarData, 0x5780, 0x7),    // 322b:0f3f
    blk(Segment::Dgroup, 0x063a, 0x2),     // 322b:0f56
    blk(Segment::Dgroup, 0x07e2, 0x2),     // 322b:0f6d
    blk(Segment::FarData, 0x016e, 0x1),    // 322b:0f84
    blk(Segment::Dgroup, 0x2280, 0x5f),    // 322b:0f9b
    blk(Segment::Dgroup, 0x0620, 0x10),    // 322b:0fb2
    blk(Segment::FarData, 0x4bce, 0x1),    // 322b:0fc9
    blk(Segment::FarData, 0x56e2, 0x4),    // 322b:0fe0
    blk(Segment::Dgroup, 0x05fa, 0x4),     // 322b:1004  [0] copied from DS:05ac first
    blk(Segment::FarData, 0x76c6, 0x4),    // 322b:101b
    blk(Segment::FarData, 0x90c2, 0x4),    // 322b:1032
    blk(Segment::Dgroup, 0x05d6, 0x4),     // 322b:1049
    blk(Segment::Dgroup, 0x061c, 0x4),     // 322b:1060
    blk(Segment::Dgroup, 0x05b8, 0x4),     // 322b:1077
    blk(Segment::FarData, 0x3060, 0x1),    // 322b:108e
];

/// Number of blocks before the raw intermediate checksum byte.
pub const MID_CHECKSUM_AFTER: usize = 27;

/// Index of the manager table (FAR:2242, 4 x 778 bytes) in [`BLOCKS`].
pub const BLOCK_MANAGERS: usize = 13;
/// Index of the club table (FAR:3066, 200 x 34 bytes) in [`BLOCKS`].
pub const BLOCK_CLUBS: usize = 15;
/// Index of the player table (FAR:57dd, 151 x 37 bytes) in [`BLOCKS`].
pub const BLOCK_PLAYERS: usize = 17;
/// Index of the squad table (FAR:774a, 5 x 25 x 52 bytes) in [`BLOCKS`].
pub const BLOCK_SQUADS: usize = 18;
/// Index of the competition slots (FAR:57aa, 18 bytes) in [`BLOCKS`].
pub const BLOCK_COMPETITION_SLOTS: usize = 26;

/// Number of manager records in FAR:2242.
pub const MANAGER_COUNT: usize = 4;
/// Size of one manager record (stride `0x30a` used at real `322b:1d8b`).
pub const MANAGER_LEN: usize = 0x30a;
/// Number of club records in FAR:3066.
pub const CLUB_COUNT: usize = 200;
/// Size of one club record.
pub const CLUB_LEN: usize = 34;
/// Number of player records in FAR:57dd.
pub const PLAYER_COUNT: usize = 151;
/// Size of one player record.
pub const PLAYER_LEN: usize = 37;

/// Errors produced while decoding a save game or high-score table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Input ended before the structure was complete.
    Truncated,
    /// The first 24 signature bytes do not match.
    BadSignature,
    /// The version tag does not start with `>V`.
    BadVersion([u8; 4]),
    /// A stored checksum byte does not match the computed one.
    Checksum {
        /// Which checksum: 0 = intermediate, 1 = final.
        which: u8,
        /// Byte stored in the file.
        stored: u8,
        /// Checksum computed over the decrypted plaintext.
        computed: u8,
    },
    /// A string-table length byte is not positive when read as `i8`.
    BadStringLength(u8),
    /// The string-table group sizes reach outside the saved DGROUP bytes.
    StringTableShape,
    /// Bytes remain after the structure.
    TrailingData(usize),
    /// A field has the wrong size for writing.
    FieldSize,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// Running state of the save-game stream cipher: key byte FAR:57cf and
/// plaintext checksum FAR:56dc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cipher {
    /// Current key (FAR:57cf).
    pub key: u8,
    /// Running plaintext checksum (FAR:56dc).
    pub sum: u8,
}

impl Cipher {
    /// Both key and checksum start at the initial key byte stored in the file.
    pub fn new(initial_key: u8) -> Self {
        Cipher {
            key: initial_key,
            sum: initial_key,
        }
    }

    /// Encrypts one byte and advances the state.
    ///
    /// Origin: BMMAIN.EXE 322b:0047 (Ghidra 4000:22f7, savegame_write_encrypt, no function object)
    pub fn encrypt(&mut self, plain: u8) -> u8 {
        self.sum = self.sum.wrapping_add(plain);
        let out = self.key.wrapping_add(self.key ^ plain);
        self.key = self.key.wrapping_add(KEY_STEP);
        out
    }

    /// Decrypts one byte and advances the state.
    ///
    /// Origin: BMMAIN.EXE 322b:013f (Ghidra 4000:23ef, savegame_read_decrypt)
    pub fn decrypt(&mut self, cipher: u8) -> u8 {
        let plain = cipher.wrapping_sub(self.key) ^ self.key;
        self.key = self.key.wrapping_add(KEY_STEP);
        self.sum = self.sum.wrapping_add(plain);
        plain
    }
}

/// One entry of the dynamic string table (rebuilt into FAR:5524 on load).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringEntry {
    /// Far pointer (offset in low word, segment in high word) the string had
    /// when it was saved. The loader uses it to patch stale pointers in
    /// FAR:774a and FAR:5664.
    pub old_pointer: u32,
    /// String bytes including the terminating NUL (`strlen + 1` bytes).
    pub bytes: Vec<u8>,
}

/// A decoded `.MAN` save game. All blocks are kept as decrypted plaintext so
/// that [`SaveGame::to_bytes`] reproduces the file byte for byte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveGame {
    /// Signature as stored (25 bytes; only the first 24 are checked).
    pub signature: [u8; 25],
    /// Version tag (`>V2\0` for current saves, `>V1?` for old ones).
    pub version: [u8; 4],
    /// Random save id; the loader refuses ids already seen this session.
    pub save_id: u32,
    /// Random byte written unencrypted and ignored by the loader.
    pub pad: u8,
    /// Initial cipher key / checksum seed (`random(0,255) + 0x78`).
    pub initial_key: u8,
    /// Decrypted contents of [`BLOCKS`], in the same order.
    pub blocks: Vec<Vec<u8>>,
    /// String table: one inner vector per group (`DS:07ab` groups, sizes in
    /// `DS:0618[i]`).
    pub strings: Vec<Vec<StringEntry>>,
    /// Raw tail: DS:05f6 (4 bytes), FAR:2e7a (4 bytes), FAR:2e94 (4 bytes).
    /// Bytes missing from a short file are 0 here.
    pub tail: [u8; 12],
    /// How many tail bytes the file held (0..=12). The loader reads the three
    /// tail longs with unchecked `file_read` calls, except that a short read of
    /// the first one (fewer than 4 bytes, see [`SaveGame::tail_short`]) skips the
    /// other two. Always 12 for files the game writes.
    pub tail_len: usize,
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.pos.checked_add(n).ok_or(Error::Truncated)?;
        let s = self.data.get(self.pos..end).ok_or(Error::Truncated)?;
        self.pos = end;
        Ok(s)
    }

    fn byte(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    fn decrypt(&mut self, cipher: &mut Cipher, n: usize) -> Result<Vec<u8>, Error> {
        Ok(self.take(n)?.iter().map(|&c| cipher.decrypt(c)).collect())
    }
}

/// Looks up a DGROUP byte inside the saved blocks (used for the string-table
/// group sizes at DS:0618+i, which the game reads past the 4-byte block).
fn dgroup_byte(blocks: &[Vec<u8>], offset: u16) -> Option<u8> {
    BLOCKS.iter().zip(blocks).find_map(|(d, b)| {
        let rel = offset.checked_sub(d.offset)?;
        (d.segment == Segment::Dgroup && rel < d.len).then(|| b[rel as usize])
    })
}

fn string_group_sizes(blocks: &[Vec<u8>]) -> Result<Vec<u8>, Error> {
    let groups = dgroup_byte(blocks, 0x07ab).ok_or(Error::StringTableShape)?;
    (0..groups as u16)
        .map(|i| dgroup_byte(blocks, 0x0618 + i).ok_or(Error::StringTableShape))
        .collect()
}

impl SaveGame {
    /// Decodes a `.MAN` file and verifies both checksums.
    ///
    /// Origin: BMMAIN.EXE 322b:120c (Ghidra 4000:34bc, savegame_read_man)
    pub fn parse(data: &[u8]) -> Result<SaveGame, Error> {
        let mut r = Reader { data, pos: 0 };
        let signature: [u8; 25] = r.take(25)?.try_into().unwrap();
        if signature[..SIGNATURE_CHECK_LEN] != SIGNATURE[..SIGNATURE_CHECK_LEN] {
            return Err(Error::BadSignature);
        }
        let version: [u8; 4] = r.take(4)?.try_into().unwrap();
        if &version[..2] != b">V" {
            return Err(Error::BadVersion(version));
        }
        let save_id = u32::from_le_bytes(r.take(4)?.try_into().unwrap());
        let pad = r.byte()?;
        let initial_key = r.byte()?;
        let mut cipher = Cipher::new(initial_key);

        let mut blocks = Vec::with_capacity(BLOCKS.len());
        for (i, d) in BLOCKS.iter().enumerate() {
            if i == MID_CHECKSUM_AFTER {
                check(0, r.byte()?, cipher.sum)?;
            }
            blocks.push(r.decrypt(&mut cipher, d.len as usize)?);
        }

        let mut strings = Vec::new();
        for count in string_group_sizes(&blocks)? {
            let mut group = Vec::with_capacity(count as usize);
            for _ in 0..count {
                let ptr = r.decrypt(&mut cipher, 4)?;
                let len = r.decrypt(&mut cipher, 1)?[0];
                // The loader sign-extends the length byte (cbw) before use.
                if (len as i8) <= 0 {
                    return Err(Error::BadStringLength(len));
                }
                group.push(StringEntry {
                    old_pointer: u32::from_le_bytes(ptr.try_into().unwrap()),
                    bytes: r.decrypt(&mut cipher, len as usize)?,
                });
            }
            strings.push(group);
        }

        check(1, r.byte()?, cipher.sum)?;
        // The loader reads the tail with three 4-byte `file_read` calls and
        // never checks for the end of the file: a file may be shorter than the
        // full tail, and bytes after it are ignored.
        let rest = &data[r.pos..];
        let tail_len = rest.len().min(12);
        let mut tail = [0u8; 12];
        tail[..tail_len].copy_from_slice(&rest[..tail_len]);
        Ok(SaveGame {
            signature,
            version,
            save_id,
            pad,
            initial_key,
            blocks,
            strings,
            tail,
            tail_len,
        })
    }

    /// True when the file ended before the first tail long (DS:05f6) was read
    /// in full. The loader then calls 1cb5:0000, which sets the copy-protection
    /// flag DS:05ac to 1 (passed), and skips the other two tail reads.
    ///
    /// Origin: BMMAIN.EXE 422b:1d28 (screen_load_game), 1cb5:0000
    pub fn tail_short(&self) -> bool {
        self.tail_len < 4
    }

    /// Encodes the save game exactly as the game writes it, recomputing both
    /// checksums.
    ///
    /// Origin: BMMAIN.EXE 322b:07fe (Ghidra 4000:2aae, savegame_write_man)
    pub fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        if self.blocks.len() != BLOCKS.len()
            || BLOCKS
                .iter()
                .zip(&self.blocks)
                .any(|(d, b)| b.len() != d.len as usize)
        {
            return Err(Error::FieldSize);
        }
        let sizes = string_group_sizes(&self.blocks)?;
        if sizes.len() != self.strings.len()
            || sizes
                .iter()
                .zip(&self.strings)
                .any(|(&n, g)| g.len() != n as usize)
        {
            return Err(Error::StringTableShape);
        }

        let mut out = Vec::with_capacity(MAN_FILE_LEN);
        out.extend_from_slice(&self.signature);
        out.extend_from_slice(&self.version);
        out.extend_from_slice(&self.save_id.to_le_bytes());
        out.push(self.pad);
        out.push(self.initial_key);
        let mut cipher = Cipher::new(self.initial_key);
        let mut enc = |out: &mut Vec<u8>, bytes: &[u8]| {
            out.extend(bytes.iter().map(|&b| cipher.encrypt(b)));
        };
        for (i, b) in self.blocks.iter().enumerate() {
            if i == MID_CHECKSUM_AFTER {
                // Raw write of FAR:56dc; does not advance the key.
                let sum = cipher_sum(&self.blocks[..i], self.initial_key);
                out.push(sum);
            }
            enc(&mut out, b);
        }
        for e in self.strings.iter().flatten() {
            let len = e.bytes.len();
            if len == 0 || len > 0x7f {
                return Err(Error::FieldSize);
            }
            enc(&mut out, &e.old_pointer.to_le_bytes());
            enc(&mut out, &[len as u8]);
            enc(&mut out, &e.bytes);
        }
        let mut sum = cipher_sum(&self.blocks, self.initial_key);
        for e in self.strings.iter().flatten() {
            for &b in e
                .old_pointer
                .to_le_bytes()
                .iter()
                .chain(&[e.bytes.len() as u8])
                .chain(&e.bytes)
            {
                sum = sum.wrapping_add(b);
            }
        }
        out.push(sum);
        out.extend_from_slice(&self.tail);
        Ok(out)
    }

    /// Plaintext of a saved block by segment and offset.
    pub fn block(&self, segment: Segment, offset: u16) -> Option<&[u8]> {
        BLOCKS
            .iter()
            .position(|d| d.segment == segment && d.offset == offset)
            .map(|i| self.blocks[i].as_slice())
    }

    /// Mutable plaintext of a saved block by segment and offset.
    pub fn block_mut(&mut self, segment: Segment, offset: u16) -> Option<&mut [u8]> {
        BLOCKS
            .iter()
            .position(|d| d.segment == segment && d.offset == offset)
            .map(|i| self.blocks[i].as_mut_slice())
    }

    /// The four human-manager records (FAR:2242, 778 bytes each).
    pub fn managers(&self) -> impl Iterator<Item = ManagerRecord<'_>> {
        self.blocks[BLOCK_MANAGERS]
            .as_chunks::<MANAGER_LEN>()
            .0
            .iter()
            .map(|raw| ManagerRecord { raw })
    }

    /// The 200 club records (FAR:3066, 34 bytes each).
    pub fn clubs(&self) -> impl Iterator<Item = ClubRecord<'_>> {
        self.blocks[BLOCK_CLUBS]
            .as_chunks::<CLUB_LEN>()
            .0
            .iter()
            .map(|raw| ClubRecord { raw })
    }

    /// The 151 player records in FAR:57dd (37 bytes each).
    pub fn players(&self) -> impl Iterator<Item = PlayerRecord<'_>> {
        self.blocks[BLOCK_PLAYERS]
            .as_chunks::<PLAYER_LEN>()
            .0
            .iter()
            .map(|raw| PlayerRecord { raw })
    }

    /// Internal level byte DS:4a28 (`5 - user level`; a V1 save asks the
    /// user for 0-5 and stores `5 - input`).
    pub fn level(&self) -> u8 {
        self.block(Segment::Dgroup, 0x4a28).unwrap()[0]
    }
}

fn cipher_sum(blocks: &[Vec<u8>], initial_key: u8) -> u8 {
    blocks
        .iter()
        .flatten()
        .fold(initial_key, |s, &b| s.wrapping_add(b))
}

fn check(which: u8, stored: u8, computed: u8) -> Result<(), Error> {
    if stored == computed {
        Ok(())
    } else {
        Err(Error::Checksum {
            which,
            stored,
            computed,
        })
    }
}

fn c_string(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    &bytes[..end]
}

/// View of a human-manager record (FAR:2242 + i * 0x30a).
#[derive(Clone, Copy, Debug)]
pub struct ManagerRecord<'a> {
    /// The full 778-byte record.
    pub raw: &'a [u8],
}

impl<'a> ManagerRecord<'a> {
    /// Manager name (CP437, NUL-terminated within the first 0x1d bytes).
    pub fn name(&self) -> &'a [u8] {
        c_string(&self.raw[..0x1d])
    }

    /// Byte at +0x1d (FAR:225f): player slot number 1-4; zero is replaced by
    /// `slot index + 1` after loading (real `322b:1e75`).
    pub fn slot(&self) -> u8 {
        self.raw[0x1d]
    }

    /// Byte at +0x1e (FAR:2260): index into the club table, 100 = no club.
    pub fn club(&self) -> u8 {
        self.raw[0x1e]
    }
}

/// View of a club record (FAR:3066 + i * 34).
#[derive(Clone, Copy, Debug)]
pub struct ClubRecord<'a> {
    /// The full 34-byte record.
    pub raw: &'a [u8],
}

impl<'a> ClubRecord<'a> {
    /// Club name (CP437, NUL-padded, 20 bytes).
    pub fn name(&self) -> &'a [u8] {
        c_string(&self.raw[..20])
    }

    /// The 14 attribute bytes following the name (semantics not yet traced).
    pub fn attributes(&self) -> &'a [u8] {
        &self.raw[20..]
    }
}

/// View of a player record (FAR:57dd + i * 37).
#[derive(Clone, Copy, Debug)]
pub struct PlayerRecord<'a> {
    /// The full 37-byte record.
    pub raw: &'a [u8],
}

impl<'a> PlayerRecord<'a> {
    /// Player name (CP437, NUL-padded, 26 bytes).
    pub fn name(&self) -> &'a [u8] {
        c_string(&self.raw[..26])
    }

    /// The 11 attribute bytes following the name (semantics not yet traced).
    pub fn attributes(&self) -> &'a [u8] {
        &self.raw[26..]
    }
}

/// Size of a `HIGH.XY` file (FAR:1d52, 0x488 bytes).
pub const HIGHSCORE_FILE_LEN: usize = 0x488;
/// Number of entries in a high-score table.
pub const HIGHSCORE_ENTRIES: usize = 20;
/// Size of one high-score entry.
pub const HIGHSCORE_ENTRY_LEN: usize = 0x3a;

/// One high-score entry (FAR:1d52 + i * 0x3a).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HighScoreEntry {
    /// Manager name, NUL-terminated (+0x00, compared with FAR:4cfe).
    pub manager: [u8; 26],
    /// Club name, NUL-terminated (+0x1a, compared with FAR:4d18).
    pub club: [u8; 26],
    /// Score (+0x34), signed; the table is sorted descending on it.
    pub score: i16,
    /// Value copied from DS:0668 when the entry is created (+0x36).
    pub extra: u32,
}

impl HighScoreEntry {
    /// Manager name without the NUL padding.
    pub fn manager_name(&self) -> &[u8] {
        c_string(&self.manager)
    }

    /// Club name without the NUL padding.
    pub fn club_name(&self) -> &[u8] {
        c_string(&self.club)
    }
}

/// A decoded `HIGH.XY` file (stored unencrypted).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HighScoreTable {
    /// The 20 entries in file order.
    pub entries: Vec<HighScoreEntry>,
}

impl HighScoreTable {
    /// Decodes a high-score file.
    ///
    /// Origin: BMMAIN.EXE 322b:21c4 (Ghidra 4000:4474, highscore_load_or_store)
    pub fn parse(data: &[u8]) -> Result<HighScoreTable, Error> {
        if data.len() < HIGHSCORE_FILE_LEN {
            return Err(Error::Truncated);
        }
        if data.len() > HIGHSCORE_FILE_LEN {
            return Err(Error::TrailingData(data.len() - HIGHSCORE_FILE_LEN));
        }
        let entries = data
            .as_chunks::<HIGHSCORE_ENTRY_LEN>()
            .0
            .iter()
            .map(|e| HighScoreEntry {
                manager: e[0..26].try_into().unwrap(),
                club: e[26..52].try_into().unwrap(),
                score: i16::from_le_bytes([e[52], e[53]]),
                extra: u32::from_le_bytes(e[54..58].try_into().unwrap()),
            })
            .collect();
        Ok(HighScoreTable { entries })
    }

    /// Encodes the table byte for byte.
    ///
    /// Origin: BMMAIN.EXE 322b:21c4 (Ghidra 4000:4474, highscore_load_or_store)
    pub fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        if self.entries.len() != HIGHSCORE_ENTRIES {
            return Err(Error::FieldSize);
        }
        let mut out = Vec::with_capacity(HIGHSCORE_FILE_LEN);
        for e in &self.entries {
            out.extend_from_slice(&e.manager);
            out.extend_from_slice(&e.club);
            out.extend_from_slice(&e.score.to_le_bytes());
            out.extend_from_slice(&e.extra.to_le_bytes());
        }
        Ok(out)
    }

    /// Sorts the table descending by score with the game's exchange sort
    /// (`for i in 0..19 { for j in i+1..20 { if s[j] > s[i] swap } }`).
    ///
    /// Origin: BMMAIN.EXE 322b:2448 (Ghidra 4000:46f8, inside highscore_insert_entry)
    pub fn sort(&mut self) {
        for i in 0..HIGHSCORE_ENTRIES - 1 {
            for j in i + 1..HIGHSCORE_ENTRIES {
                if self.entries[j].score > self.entries[i].score {
                    self.entries.swap(i, j);
                }
            }
        }
    }
}

/// FAR:513c (save block 43) of a historical one-year game: it ends after the 1964 season.
pub const SCENARIO_ONE_YEAR: u16 = 0x7ac;
/// FAR:513c of a historical three-year game: it ends after the 1966 season.
pub const SCENARIO_THREE_YEARS: u16 = 0x7ae;
/// Years a modern (1992) start adds to the FAR:513c end-year codes.
pub const SCENARIO_MODERN_OFFSET: u16 = 0x1d;
/// FAR:513c codes of the one-year games (historical, modern).
pub const ONE_YEAR_SCENARIOS: [u16; 2] = [
    SCENARIO_ONE_YEAR,
    SCENARIO_ONE_YEAR + SCENARIO_MODERN_OFFSET,
];
/// FAR:513c codes of the three-year games (historical, modern).
pub const THREE_YEAR_SCENARIOS: [u16; 2] = [
    SCENARIO_THREE_YEARS,
    SCENARIO_THREE_YEARS + SCENARIO_MODERN_OFFSET,
];

/// Builds the high-score file name `HIGH.XY` (template at DS:949c).
///
/// `X` is `'1'` when FAR:513c is one of [`ONE_YEAR_SCENARIOS`], `'3'` when it
/// is one of [`THREE_YEAR_SCENARIOS`], else `'0'`; `Y` is `'5' - level`
/// (DS:4a28), wrapping like the original byte arithmetic.
///
/// Origin: BMMAIN.EXE 322b:21c4 (Ghidra 4000:4474, highscore_load_or_store)
pub fn highscore_file_name(far_513c: u16, level: u8) -> String {
    let mut variant = 0u8;
    if ONE_YEAR_SCENARIOS.contains(&far_513c) {
        variant = 1;
    }
    if THREE_YEAR_SCENARIOS.contains(&far_513c) {
        variant = 3;
    }
    let x = (variant + b'0') as char;
    let y = b'5'.wrapping_sub(level) as char;
    format!("HIGH.{x}{y}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save_files;

    #[test]
    fn block_table_matches_file_size() {
        let fixed: usize = BLOCKS.iter().map(|d| d.len as usize).sum();
        // header 35 + blocks + 2 checksum bytes + 12 tail bytes
        assert_eq!(ENCRYPTED_START + fixed + 2 + 12, MAN_FILE_LEN);
    }

    #[test]
    fn cipher_round_trip() {
        let mut e = Cipher::new(0xcb);
        let mut d = Cipher::new(0xcb);
        for b in 0..=255u8 {
            assert_eq!(d.decrypt(e.encrypt(b)), b);
        }
        assert_eq!(e, d);
    }

    #[test]
    fn synthetic_round_trip_with_strings() {
        let mut blocks: Vec<Vec<u8>> = BLOCKS.iter().map(|d| vec![0; d.len as usize]).collect();
        blocks[7][0] = 2; // DS:07ab groups
        blocks[1] = vec![1, 2, 0, 0]; // DS:0618 sizes
        let entry = |s: &[u8], p| StringEntry {
            old_pointer: p,
            bytes: s.iter().copied().chain([0]).collect(),
        };
        let save = SaveGame {
            signature: *SIGNATURE,
            version: VERSION_V2,
            save_id: 0x1234_5678,
            pad: 0x42,
            initial_key: 0x99,
            blocks,
            strings: vec![
                vec![entry(b"MEIER", 0x4238_0010)],
                vec![entry(b"A", 1), entry(b"BC", 2)],
            ],
            tail: [1; 12],
            tail_len: 12,
        };
        let bytes = save.to_bytes().unwrap();
        assert_eq!(SaveGame::parse(&bytes).unwrap(), save);
    }

    #[test]
    fn real_saves_round_trip() {
        for path in save_files() {
            let data = std::fs::read(&path).unwrap();
            assert_eq!(data.len(), MAN_FILE_LEN, "{path:?}");
            let save = SaveGame::parse(&data).unwrap_or_else(|e| panic!("{path:?}: {e}"));
            assert_eq!(save.version, VERSION_V2);
            assert_eq!(save.to_bytes().unwrap(), data, "{path:?}");
            assert_eq!(save.clubs().count(), CLUB_COUNT);
            assert_eq!(save.players().count(), PLAYER_COUNT);
            // Every manager either has a club index into the club table or 100.
            for m in save.managers() {
                assert!(m.club() < CLUB_COUNT as u8 || m.club() == 100);
            }
        }
    }

    #[test]
    fn autosave_content() {
        let Some(dir) = crate::data_dir() else {
            return;
        };
        let Ok(data) = std::fs::read(dir.join("AUTOSAVE.MAN")) else {
            return;
        };
        let save = SaveGame::parse(&data).unwrap();
        let clubs: Vec<_> = save.clubs().map(|c| c.name().to_vec()).collect();
        assert_eq!(clubs[0], b"BORUSSIA DORTMUND");
        assert_eq!(clubs[199], b"ANDORRA KICKERS");
        let m = save.managers().next().unwrap();
        assert_eq!(m.name(), b"OLIVER");
        assert_eq!(clubs[m.club() as usize], b"BAYER 05 UERDINGEN");
        assert_eq!(save.players().nth(1).unwrap().name(), b"STEIN");
        // Corrupting one encrypted byte must break a checksum.
        let mut bad = data.clone();
        bad[ENCRYPTED_START + 100] ^= 1;
        assert!(matches!(
            SaveGame::parse(&bad),
            Err(Error::Checksum { which: 0, .. })
        ));
    }

    #[test]
    fn highscore_round_trip() {
        let Some(dir) = crate::data_dir() else {
            return;
        };
        for name in ["HIGH.00", "HIGH.02"] {
            let Ok(data) = std::fs::read(dir.join(name)) else {
                continue;
            };
            let t = HighScoreTable::parse(&data).unwrap();
            assert_eq!(t.entries.len(), HIGHSCORE_ENTRIES);
            assert_eq!(t.to_bytes().unwrap(), data);
            let mut sorted = t.clone();
            sorted.sort();
            assert_eq!(sorted, t, "{name} is stored sorted");
        }
        let data = std::fs::read(dir.join("HIGH.00")).unwrap();
        let t = HighScoreTable::parse(&data).unwrap();
        assert_eq!(t.entries[0].manager_name(), b"OLIVER");
        assert_eq!(t.entries[0].club_name(), b"KARLSRUHER SC");
        assert_eq!(t.entries[0].score, 0xcf);
    }

    #[test]
    fn highscore_names() {
        assert_eq!(highscore_file_name(0x56ce, 5), "HIGH.00");
        assert_eq!(highscore_file_name(0x56ce, 3), "HIGH.02");
        assert_eq!(highscore_file_name(0x7c9, 0), "HIGH.15");
        assert_eq!(highscore_file_name(0x7cb, 5), "HIGH.30");
    }
}
