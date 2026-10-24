//! Creative Voice File (`.VOC`): the digitised sounds `SOUND\DIGI.VOC`.
//!
//! A VOC file is a 26-byte header followed by a stream of typed blocks. The
//! game loads the whole block stream into EMS and hands block pointers to the
//! embedded CT-VOICE driver (`sound_load_digi_voc` BMMAIN.EXE 176b:005f). The
//! full specification is in `docs/re/voc.md`.

use std::fmt;

/// Signature at offset 0 (followed by 0x1a).
pub const SIGNATURE: &[u8; 19] = b"Creative Voice File";

/// Lengths of the five VOC files concatenated in `SOUND\DIGI.VOC` (`DS:0164`).
pub const DIGI_SLOT_LENGTHS: [usize; 5] = [7018, 16080, 29084, 22331, 15813];
/// Voice number `n` (1..5) of `sound_play_voice` -> DIGI.VOC slot (`DS:016c`).
pub const DIGI_VOICE_SLOT: [usize; 6] = [0, 2, 1, 0, 3, 4];

/// Splits `SOUND\DIGI.VOC` into its five VOC files, read in order with the
/// fixed lengths of [`DIGI_SLOT_LENGTHS`] like `sound_load_digi_voc`.
///
/// Origin: BMMAIN.EXE 176b:005f (sound_load_digi_voc)
pub fn split_digi(bytes: &[u8]) -> Result<Vec<Voc>, VocError> {
    let mut at = 0;
    DIGI_SLOT_LENGTHS
        .iter()
        .map(|&len| {
            let part = bytes
                .get(at..at + len)
                .ok_or(VocError::Truncated { offset: at })?;
            at += len;
            Voc::parse(part)
        })
        .collect()
}

/// Errors produced by the VOC parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VocError {
    /// Missing signature or a header offset outside the file.
    BadHeader,
    /// A block runs past the end of the file.
    Truncated { offset: usize },
}

impl fmt::Display for VocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VocError::BadHeader => write!(f, "not a VOC file"),
            VocError::Truncated { offset } => write!(f, "VOC block truncated at {offset:#x}"),
        }
    }
}

impl std::error::Error for VocError {}

/// One decoded block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    /// Type 0: end of the stream.
    Terminator,
    /// Type 1: new sample data. `rate_byte` is the SB time constant
    /// (rate = 1_000_000 / (256 - rate_byte)); `pack` 0 = 8-bit unsigned PCM.
    Sound { rate_byte: u8, pack: u8 },
    /// Type 2: more data in the format of the previous type-1 block.
    Continuation,
    /// Type 3: `length + 1` samples of silence at `rate_byte`.
    Silence { length: u16, rate_byte: u8 },
    /// Type 4: a marker number (reported in the driver's status word).
    Marker(u16),
    /// Type 5: ASCIIZ text.
    Text,
    /// Type 6: repeat the following blocks `count + 1` times (0xffff = endless).
    RepeatStart(u16),
    /// Type 7: end of the repeated section.
    RepeatEnd,
    /// Type 8: extended rate/pack/mode for the next type-1 block.
    Extended {
        time_constant: u16,
        pack: u8,
        stereo: bool,
    },
    /// Type 9 and others: kept as raw payload.
    Other(u8),
}

/// A block with its position in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// File offset of the block's type byte.
    pub offset: usize,
    pub kind: BlockKind,
    /// The sample bytes of type 1/2 blocks (after the type-1 parameters),
    /// otherwise the raw payload.
    pub data: std::ops::Range<usize>,
}

/// A parsed VOC file: the raw bytes and the block list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voc {
    pub version: u16,
    pub data_offset: usize,
    pub blocks: Vec<Block>,
    bytes: Vec<u8>,
}

/// Sample rate of an SB time constant.
pub fn rate_of(rate_byte: u8) -> u32 {
    1_000_000 / (256 - u32::from(rate_byte))
}

/// A run of 8-bit unsigned mono PCM at one sample rate, with an optional
/// repeated section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pcm {
    pub rate: u32,
    pub samples: Vec<u8>,
    /// The section of a repeat block (types 6/7): `samples[range]` plays
    /// `count + 1` times in total (`count` 0xffff = endless).
    pub repeat: Option<(std::ops::Range<usize>, u16)>,
}

impl Pcm {
    /// Total length in samples with the repeats expanded (`None` = endless).
    pub fn total_len(&self) -> Option<u64> {
        match &self.repeat {
            None => Some(self.samples.len() as u64),
            Some((_, 0xffff)) => None,
            Some((r, n)) => Some(self.samples.len() as u64 + r.len() as u64 * u64::from(*n)),
        }
    }

    /// The sample at position `i` of the expanded stream (`None` past the end).
    pub fn at(&self, i: u64) -> Option<u8> {
        let idx = match &self.repeat {
            None => i,
            Some((r, n)) => {
                let (start, body) = (r.start as u64, r.len() as u64);
                if i < start || body == 0 {
                    i
                } else if *n == 0xffff || (i - start) / body <= u64::from(*n) {
                    start + (i - start) % body
                } else {
                    i - body * u64::from(*n)
                }
            }
        };
        self.samples.get(usize::try_from(idx).ok()?).copied()
    }
}

impl Voc {
    pub fn parse(bytes: &[u8]) -> Result<Self, VocError> {
        if bytes.len() < 0x1a || &bytes[..19] != SIGNATURE || bytes[19] != 0x1a {
            return Err(VocError::BadHeader);
        }
        let data_offset = usize::from(u16::from_le_bytes([bytes[0x14], bytes[0x15]]));
        let version = u16::from_le_bytes([bytes[0x16], bytes[0x17]]);
        if data_offset > bytes.len() {
            return Err(VocError::BadHeader);
        }
        let blocks = parse_blocks(bytes, data_offset)?;
        Ok(Self {
            version,
            data_offset,
            blocks,
            bytes: bytes.to_vec(),
        })
    }

    /// The bytes of a block range.
    pub fn slice(&self, r: &std::ops::Range<usize>) -> &[u8] {
        &self.bytes[r.clone()]
    }

    /// The whole file.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The sound of the block stream starting at the block at file offset
    /// `from`, played the way CT-VOICE does: sound/continuation data and
    /// silence, up to the terminator. The first repeat section is reported in
    /// [`Pcm::repeat`] instead of being expanded; nested or further repeat
    /// blocks play once. Returns `None` if `from` is not a block offset.
    pub fn render_from(&self, from: usize) -> Option<Pcm> {
        let start = self.blocks.iter().position(|b| b.offset == from)?;
        let mut pcm = Pcm {
            rate: 0,
            samples: Vec::new(),
            repeat: None,
        };
        let mut open: Option<(usize, u16)> = None;
        for b in &self.blocks[start..] {
            match b.kind {
                BlockKind::Terminator => break,
                BlockKind::Sound { rate_byte, .. } => {
                    if pcm.rate == 0 {
                        pcm.rate = rate_of(rate_byte);
                    }
                    pcm.samples.extend_from_slice(self.slice(&b.data));
                }
                BlockKind::Continuation => pcm.samples.extend_from_slice(self.slice(&b.data)),
                BlockKind::Silence { length, rate_byte } => {
                    if pcm.rate == 0 {
                        pcm.rate = rate_of(rate_byte);
                    }
                    pcm.samples
                        .extend(std::iter::repeat_n(0x80, usize::from(length) + 1));
                }
                BlockKind::RepeatStart(n) if pcm.repeat.is_none() && open.is_none() => {
                    open = Some((pcm.samples.len(), n))
                }
                BlockKind::RepeatEnd => {
                    if let Some((at, n)) = open.take() {
                        pcm.repeat = Some((at..pcm.samples.len(), n));
                    }
                }
                _ => {}
            }
        }
        Some(pcm)
    }
}

fn parse_blocks(b: &[u8], mut at: usize) -> Result<Vec<Block>, VocError> {
    let mut blocks = Vec::new();
    while at < b.len() {
        let kind = b[at];
        if kind == 0 {
            blocks.push(Block {
                offset: at,
                kind: BlockKind::Terminator,
                data: at + 1..at + 1,
            });
            break;
        }
        if at + 4 > b.len() {
            return Err(VocError::Truncated { offset: at });
        }
        let len =
            usize::from(b[at + 1]) | usize::from(b[at + 2]) << 8 | usize::from(b[at + 3]) << 16;
        let start = at + 4;
        let end = start + len;
        if end > b.len() {
            return Err(VocError::Truncated { offset: at });
        }
        let p = &b[start..end];
        let need = |n: usize| {
            if len < n {
                Err(VocError::Truncated { offset: at })
            } else {
                Ok(())
            }
        };
        let (kind, data) = match kind {
            1 => {
                need(2)?;
                (
                    BlockKind::Sound {
                        rate_byte: p[0],
                        pack: p[1],
                    },
                    start + 2..end,
                )
            }
            2 => (BlockKind::Continuation, start..end),
            3 => {
                need(3)?;
                (
                    BlockKind::Silence {
                        length: u16::from_le_bytes([p[0], p[1]]),
                        rate_byte: p[2],
                    },
                    start..end,
                )
            }
            4 => {
                need(2)?;
                (
                    BlockKind::Marker(u16::from_le_bytes([p[0], p[1]])),
                    start..end,
                )
            }
            5 => (BlockKind::Text, start..end),
            6 => {
                need(2)?;
                (
                    BlockKind::RepeatStart(u16::from_le_bytes([p[0], p[1]])),
                    start..end,
                )
            }
            7 => (BlockKind::RepeatEnd, start..end),
            8 => {
                need(4)?;
                (
                    BlockKind::Extended {
                        time_constant: u16::from_le_bytes([p[0], p[1]]),
                        pack: p[2],
                        stereo: p[3] != 0,
                    },
                    start..end,
                )
            }
            k => (BlockKind::Other(k), start..end),
        };
        blocks.push(Block {
            offset: at,
            kind,
            data,
        });
        at = end;
    }
    Ok(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny() -> Vec<u8> {
        let mut b = SIGNATURE.to_vec();
        b.push(0x1a);
        b.extend(0x1au16.to_le_bytes());
        b.extend(0x010au16.to_le_bytes());
        b.extend(0x1129u16.to_le_bytes());
        // Sound: rate byte 0x9c (10 kHz), 3 samples.
        b.extend([1, 5, 0, 0, 0x9c, 0, 1, 2, 3]);
        // Repeat twice more: silence of 2 samples.
        b.extend([6, 2, 0, 0, 2, 0]);
        b.extend([3, 3, 0, 0, 1, 0, 0x9c]);
        b.extend([7, 0, 0, 0]);
        b.extend([2, 1, 0, 0, 9]);
        b.push(0);
        b
    }

    #[test]
    fn parses_blocks() {
        let voc = Voc::parse(&tiny()).unwrap();
        assert_eq!(voc.version, 0x010a);
        let kinds: Vec<_> = voc.blocks.iter().map(|b| b.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                BlockKind::Sound {
                    rate_byte: 0x9c,
                    pack: 0
                },
                BlockKind::RepeatStart(2),
                BlockKind::Silence {
                    length: 1,
                    rate_byte: 0x9c
                },
                BlockKind::RepeatEnd,
                BlockKind::Continuation,
                BlockKind::Terminator,
            ]
        );
        assert_eq!(voc.slice(&voc.blocks[0].data), &[1, 2, 3]);
        assert_eq!(rate_of(0x9c), 10_000);
    }

    #[test]
    fn renders_repeats_and_silence() {
        let voc = Voc::parse(&tiny()).unwrap();
        let pcm = voc.render_from(0x1a).unwrap();
        assert_eq!(pcm.rate, 10_000);
        assert_eq!(pcm.samples, [1, 2, 3, 128, 128, 9]);
        assert_eq!(pcm.repeat, Some((3..5, 2)));
        assert_eq!(pcm.total_len(), Some(10));
        let expanded: Vec<u8> = (0..11).map_while(|i| pcm.at(i)).collect();
        assert_eq!(expanded, [1, 2, 3, 128, 128, 128, 128, 128, 128, 9]);
        assert!(voc.render_from(0x1b).is_none());
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(Voc::parse(b"RIFF"), Err(VocError::BadHeader));
        let mut b = tiny();
        b.truncate(0x1a + 6);
        assert!(matches!(Voc::parse(&b), Err(VocError::Truncated { .. })));
    }

    #[test]
    fn original_digi_voc() {
        let Some(dir) = crate::data_dir() else {
            return;
        };
        let bytes = std::fs::read(dir.join("SOUND/DIGI.VOC")).unwrap();
        assert_eq!(bytes.len(), DIGI_SLOT_LENGTHS.iter().sum::<usize>());
        let slots = split_digi(&bytes).unwrap();
        let rendered: Vec<(u32, usize)> = slots
            .iter()
            .map(|v| {
                let pcm = v.render_from(v.data_offset).unwrap();
                (pcm.rate, pcm.total_len().unwrap() as usize)
            })
            .collect();
        // The crowd (slot 2) plays its repeat section 32001 times.
        assert_eq!(
            rendered,
            [
                (10_000, 6985),
                (10_000, 16047),
                (10_000, 29041 * 32001),
                (20_000, 22298),
                (20_000, 15780),
            ]
        );
        assert_eq!(slots[2].blocks[0].kind, BlockKind::RepeatStart(32000));
    }
}
