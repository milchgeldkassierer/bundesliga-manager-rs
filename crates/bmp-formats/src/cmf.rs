//! Creative Music Format (`.CMF`): the title music `SOUND\BM2TITLE.CMF`.
//!
//! A CMF file is a header, a table of 16-byte OPL2 instruments and a single
//! MIDI-like event track. `main` (BMMAIN.EXE 18bc:0418) reads the header fields
//! at 0x06 (instruments), 0x08 (music), 0x0c (ticks per second) and 0x24
//! (instrument count) and hands the rest to the embedded SBFMDRV. The full
//! specification is in `docs/re/cmf.md`.

use std::fmt;

/// File signature at offset 0.
pub const SIGNATURE: &[u8; 4] = b"CTMF";
/// Size of one instrument record.
pub const INSTRUMENT_BYTES: usize = 16;
/// Size of the fixed header (up to and including the basic tempo at 0x26).
pub const HEADER_BYTES: usize = 0x28;

/// Errors produced by the CMF parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmfError {
    /// The file is shorter than the header or does not start with `CTMF`.
    BadHeader,
    /// The instrument table or the music offset lies outside the file.
    BadOffset,
    /// The event stream ends in the middle of an event.
    Truncated { offset: usize },
    /// A data byte where a status byte is required and no running status is set.
    NoRunningStatus { offset: usize },
}

impl fmt::Display for CmfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CmfError::BadHeader => write!(f, "not a CMF file"),
            CmfError::BadOffset => write!(f, "CMF offset outside the file"),
            CmfError::Truncated { offset } => write!(f, "CMF event truncated at {offset:#x}"),
            CmfError::NoRunningStatus { offset } => {
                write!(f, "CMF data byte without running status at {offset:#x}")
            }
        }
    }
}

impl std::error::Error for CmfError {}

/// One OPL2 instrument: the register values of the modulator and carrier
/// operators and the feedback/connection byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Instrument {
    /// Registers 0x20 (AM/VIB/EG/KSR/MULT), 0x40 (KSL/TL), 0x60 (AR/DR),
    /// 0x80 (SL/RR), 0xE0 (waveform): `[modulator, carrier]`.
    pub char_mult: [u8; 2],
    pub scale_level: [u8; 2],
    pub attack_decay: [u8; 2],
    pub sustain_release: [u8; 2],
    pub wave_select: [u8; 2],
    /// Register 0xC0: feedback (bits 1..3) and connection (bit 0).
    pub feedback_conn: u8,
}

impl Instrument {
    /// Decodes a 16-byte record (bytes 11..15 are padding).
    pub fn from_bytes(b: &[u8]) -> Self {
        Self {
            char_mult: [b[0], b[1]],
            scale_level: [b[2], b[3]],
            attack_decay: [b[4], b[5]],
            sustain_release: [b[6], b[7]],
            wave_select: [b[8], b[9]],
            feedback_conn: b[10],
        }
    }

    /// The 16-byte record.
    pub fn to_bytes(&self) -> [u8; INSTRUMENT_BYTES] {
        let mut b = [0u8; INSTRUMENT_BYTES];
        b[0..2].copy_from_slice(&self.char_mult);
        b[2..4].copy_from_slice(&self.scale_level);
        b[4..6].copy_from_slice(&self.attack_decay);
        b[6..8].copy_from_slice(&self.sustain_release);
        b[8..10].copy_from_slice(&self.wave_select);
        b[10] = self.feedback_conn;
        b
    }
}

/// One decoded event of the music stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    NoteOff {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    /// Note on; velocity 0 is a note off by MIDI convention.
    NoteOn {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    PolyPressure {
        channel: u8,
        note: u8,
        value: u8,
    },
    Controller {
        channel: u8,
        controller: u8,
        value: u8,
    },
    Program {
        channel: u8,
        program: u8,
    },
    ChannelPressure {
        channel: u8,
        value: u8,
    },
    /// Pitch bend, 14 bits (0x2000 = centre).
    PitchBend {
        channel: u8,
        value: u16,
    },
    /// System exclusive (`F0`/`F7`), payload without the length.
    SysEx {
        data: Vec<u8>,
    },
    /// Meta event (`FF type len data`); type 0x2F ends the track.
    Meta {
        kind: u8,
        data: Vec<u8>,
    },
}

/// An event with the delta time (in CMF ticks) that precedes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub delta: u32,
    /// Offset of the event's first byte (after the delta) in the file.
    pub offset: usize,
    pub kind: EventKind,
}

/// A parsed CMF file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cmf {
    /// Format version (0x0100 or 0x0101).
    pub version: u16,
    pub instrument_offset: u16,
    pub music_offset: u16,
    pub ticks_per_quarter: u16,
    /// Tempo of the event stream: delta ticks per second.
    pub ticks_per_second: u16,
    pub title_offset: u16,
    pub composer_offset: u16,
    pub remarks_offset: u16,
    /// Channel-in-use flags of the 16 MIDI channels.
    pub channels_used: [bool; 16],
    pub instruments: Vec<Instrument>,
    pub basic_tempo: u16,
    /// The raw event stream from the music offset to the end of the file.
    pub music: Vec<u8>,
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

impl Cmf {
    /// Parses the header, the instrument table and keeps the event stream.
    pub fn parse(b: &[u8]) -> Result<Self, CmfError> {
        if b.len() < HEADER_BYTES || &b[0..4] != SIGNATURE {
            return Err(CmfError::BadHeader);
        }
        let version = u16_at(b, 4);
        let instrument_offset = u16_at(b, 6);
        let music_offset = u16_at(b, 8);
        // Version 1.0 stores the instrument count as one byte; SBFMDRV's caller
        // (`main`) reads only the low byte `cmf[0x24]` in either case.
        let count = if version >= 0x0101 {
            u16_at(b, 0x24)
        } else {
            u16::from(b[0x24])
        };
        let instr_end = usize::from(instrument_offset) + usize::from(count) * INSTRUMENT_BYTES;
        if instr_end > b.len() || usize::from(music_offset) > b.len() {
            return Err(CmfError::BadOffset);
        }
        let instruments = b[usize::from(instrument_offset)..instr_end]
            .as_chunks::<INSTRUMENT_BYTES>()
            .0
            .iter()
            .map(|c| Instrument::from_bytes(c))
            .collect();
        let mut channels_used = [false; 16];
        for (i, used) in channels_used.iter_mut().enumerate() {
            *used = b[0x14 + i] != 0;
        }
        Ok(Self {
            version,
            instrument_offset,
            music_offset,
            ticks_per_quarter: u16_at(b, 0x0a),
            ticks_per_second: u16_at(b, 0x0c),
            title_offset: u16_at(b, 0x0e),
            composer_offset: u16_at(b, 0x10),
            remarks_offset: u16_at(b, 0x12),
            channels_used,
            instruments,
            basic_tempo: if version >= 0x0101 {
                u16_at(b, 0x26)
            } else {
                u16_at(b, 0x25)
            },
            music: b[usize::from(music_offset)..].to_vec(),
        })
    }

    /// Decodes the event stream up to and including the end-of-track meta
    /// event (or the end of the data). Offsets are file offsets.
    pub fn events(&self) -> Result<Vec<Event>, CmfError> {
        let mut reader = EventReader::new(&self.music, usize::from(self.music_offset));
        let mut out = Vec::new();
        while let Some(ev) = reader.next_event()? {
            let end = matches!(ev.kind, EventKind::Meta { kind: 0x2f, .. });
            out.push(ev);
            if end {
                break;
            }
        }
        Ok(out)
    }
}

/// Incremental reader of a CMF/MIDI event stream with running status.
#[derive(Debug, Clone)]
pub struct EventReader<'a> {
    data: &'a [u8],
    pos: usize,
    base: usize,
    running: Option<u8>,
}

impl<'a> EventReader<'a> {
    /// `base` is the file offset of `data[0]` (used for error offsets only).
    pub fn new(data: &'a [u8], base: usize) -> Self {
        Self {
            data,
            pos: 0,
            base,
            running: None,
        }
    }

    fn byte(&mut self) -> Result<u8, CmfError> {
        let b = *self.data.get(self.pos).ok_or(CmfError::Truncated {
            offset: self.base + self.pos,
        })?;
        self.pos += 1;
        Ok(b)
    }

    /// A MIDI variable-length quantity (7 bits per byte, high bit = more).
    pub fn varlen(&mut self) -> Result<u32, CmfError> {
        let mut v = 0u32;
        for _ in 0..4 {
            let b = self.byte()?;
            v = (v << 7) | u32::from(b & 0x7f);
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Ok(v)
    }

    fn bytes(&mut self, n: usize) -> Result<Vec<u8>, CmfError> {
        let end = self.pos + n;
        if end > self.data.len() {
            return Err(CmfError::Truncated {
                offset: self.base + self.data.len(),
            });
        }
        let v = self.data[self.pos..end].to_vec();
        self.pos = end;
        Ok(v)
    }

    /// The next delta time and event, `None` at the end of the data.
    pub fn next_event(&mut self) -> Result<Option<Event>, CmfError> {
        if self.pos >= self.data.len() {
            return Ok(None);
        }
        let delta = self.varlen()?;
        let offset = self.base + self.pos;
        let first = self.byte()?;
        let status = if first & 0x80 != 0 {
            first
        } else {
            self.pos -= 1;
            self.running.ok_or(CmfError::NoRunningStatus { offset })?
        };
        let channel = status & 0x0f;
        let kind = match status & 0xf0 {
            0x80..=0xe0 => {
                self.running = Some(status);
                let a = self.byte()?;
                match status & 0xf0 {
                    0x80 => EventKind::NoteOff {
                        channel,
                        note: a,
                        velocity: self.byte()?,
                    },
                    0x90 => EventKind::NoteOn {
                        channel,
                        note: a,
                        velocity: self.byte()?,
                    },
                    0xa0 => EventKind::PolyPressure {
                        channel,
                        note: a,
                        value: self.byte()?,
                    },
                    0xb0 => EventKind::Controller {
                        channel,
                        controller: a,
                        value: self.byte()?,
                    },
                    0xc0 => EventKind::Program {
                        channel,
                        program: a,
                    },
                    0xd0 => EventKind::ChannelPressure { channel, value: a },
                    _ => {
                        let hi = self.byte()?;
                        EventKind::PitchBend {
                            channel,
                            value: u16::from(a & 0x7f) | (u16::from(hi & 0x7f) << 7),
                        }
                    }
                }
            }
            _ => match status {
                0xf0 | 0xf7 => {
                    let len = self.varlen()? as usize;
                    EventKind::SysEx {
                        data: self.bytes(len)?,
                    }
                }
                0xff => {
                    let kind = self.byte()?;
                    let len = self.varlen()? as usize;
                    EventKind::Meta {
                        kind,
                        data: self.bytes(len)?,
                    }
                }
                _ => EventKind::Meta {
                    kind: status,
                    data: Vec::new(),
                },
            },
        };
        Ok(Some(Event {
            delta,
            offset,
            kind,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny() -> Vec<u8> {
        let mut b = vec![0u8; HEADER_BYTES];
        b[0..4].copy_from_slice(SIGNATURE);
        b[4..6].copy_from_slice(&0x0101u16.to_le_bytes());
        b[6..8].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
        b[8..10].copy_from_slice(&((HEADER_BYTES + 16) as u16).to_le_bytes());
        b[0x0a] = 48;
        b[0x0c] = 96;
        b[0x14] = 1;
        b[0x24] = 1;
        b.extend((0..16u8).map(|i| i + 1));
        // delta 0: program ch0 = 0; delta 0x81 0x00 (=128): note on 60/127;
        // running status note on 60/0; delta 0: end of track.
        b.extend([0x00, 0xc0, 0x00, 0x81, 0x00, 0x90, 60, 127, 0x10, 60, 0]);
        b.extend([0x00, 0xff, 0x2f, 0x00]);
        b
    }

    #[test]
    fn parses_header_and_instruments() {
        let cmf = Cmf::parse(&tiny()).unwrap();
        assert_eq!(cmf.ticks_per_second, 96);
        assert_eq!(cmf.ticks_per_quarter, 48);
        assert!(cmf.channels_used[0] && !cmf.channels_used[1]);
        assert_eq!(cmf.instruments.len(), 1);
        let i = cmf.instruments[0];
        assert_eq!(i.char_mult, [1, 2]);
        assert_eq!(i.wave_select, [9, 10]);
        assert_eq!(i.feedback_conn, 11);
        assert_eq!(&i.to_bytes()[..11], &(1..=11).collect::<Vec<u8>>()[..]);
    }

    #[test]
    fn decodes_events_with_running_status_and_varlen() {
        let events = Cmf::parse(&tiny()).unwrap().events().unwrap();
        let kinds: Vec<_> = events.iter().map(|e| (e.delta, e.kind.clone())).collect();
        assert_eq!(
            kinds,
            vec![
                (
                    0,
                    EventKind::Program {
                        channel: 0,
                        program: 0
                    }
                ),
                (
                    128,
                    EventKind::NoteOn {
                        channel: 0,
                        note: 60,
                        velocity: 127
                    }
                ),
                (
                    16,
                    EventKind::NoteOn {
                        channel: 0,
                        note: 60,
                        velocity: 0
                    }
                ),
                (
                    0,
                    EventKind::Meta {
                        kind: 0x2f,
                        data: vec![]
                    }
                ),
            ]
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(Cmf::parse(b"MThd"), Err(CmfError::BadHeader));
        let mut b = tiny();
        b[0x24] = 200;
        assert_eq!(Cmf::parse(&b), Err(CmfError::BadOffset));
        let mut b = tiny();
        let n = b.len();
        b.truncate(n - 4);
        b.extend([0x00, 0x90, 60]);
        assert!(matches!(
            Cmf::parse(&b).unwrap().events(),
            Err(CmfError::Truncated { .. })
        ));
        let mut b = tiny();
        b.truncate(HEADER_BYTES + 16);
        b.extend([0x00, 0x3c, 0x40]);
        assert!(matches!(
            Cmf::parse(&b).unwrap().events(),
            Err(CmfError::NoRunningStatus { .. })
        ));
    }

    #[test]
    fn original_title_music() {
        let Some(dir) = crate::data_dir() else {
            return;
        };
        let bytes = std::fs::read(dir.join("SOUND/BM2TITLE.CMF")).unwrap();
        let cmf = Cmf::parse(&bytes).unwrap();
        assert_eq!(cmf.version, 0x0101);
        assert_eq!(cmf.instrument_offset, 0x28);
        assert_eq!(cmf.music_offset, 0xe8);
        assert_eq!(cmf.ticks_per_second, 96);
        assert_eq!(cmf.instruments.len(), 12);
        let events = cmf.events().unwrap();
        assert!(matches!(
            events.last().unwrap().kind,
            EventKind::Meta { kind: 0x2f, .. }
        ));
        // The first event switches on rhythm mode (controller 0x67).
        assert_eq!(
            events[0].kind,
            EventKind::Controller {
                channel: 0,
                controller: 0x67,
                value: 1
            }
        );
        for e in &events {
            if let EventKind::NoteOn { channel, .. } | EventKind::NoteOff { channel, .. } = e.kind {
                assert!(cmf.channels_used[usize::from(channel)], "{e:?}");
            }
        }
    }
}
