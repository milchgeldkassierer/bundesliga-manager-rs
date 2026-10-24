//! A register-exact reimplementation of the CMF player of Creative's SBFMDRV
//! (version 0x0114), which BMMAIN.EXE carries embedded at segment 4e67. The
//! driver state, voice allocation, frequency table and the driver's quirks
//! follow `docs/re/cmf.md` §2; every OPL register write the original makes is
//! made in the same order.

use bmp_formats::cmf::INSTRUMENT_BYTES;

/// Something that takes OPL register writes.
pub trait OplWrite {
    fn write(&mut self, reg: u8, val: u8);
}

impl OplWrite for crate::opl::Opl2 {
    fn write(&mut self, reg: u8, val: u8) {
        crate::opl::Opl2::write(self, reg, val);
    }
}

impl OplWrite for Vec<(u8, u8)> {
    fn write(&mut self, reg: u8, val: u8) {
        self.push((reg, val));
    }
}

/// PIT input clock as used by the driver's caller (`0x1234DC`).
pub const PIT_CLOCK_MAIN: u32 = 0x1234dc;
/// Hardware PIT clock in Hz.
pub const PIT_HZ: f64 = 1_193_182.0;

/// Driver divisor after init/reset (64 Hz).
const DEFAULT_DIVISOR: u16 = 0x48d3;

/// Operator offset of the modulator of voice v (`CS:01bd`).
const MOD_OP: [u8; 9] = [0x00, 0x01, 0x02, 0x08, 0x09, 0x0a, 0x10, 0x11, 0x12];
/// Operator of rhythm slots 6..10 (`CS:01cb + slot`).
const DRUM_OP: [u8; 11] = [0, 0, 0, 0, 0, 0, 0x10, 0x14, 0x12, 0x15, 0x11];
/// OPL channel of rhythm slots 6..10 (`CS:01d5 + slot`).
const DRUM_CH: [u8; 11] = [0, 0, 0, 0, 0, 0, 6, 7, 8, 8, 7];
/// Register 0xBD bit of MIDI channels 11..15.
const BD_BIT: [u8; 16] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10, 0x08, 0x04, 0x02, 0x01,
];
/// Patch `voices_init` writes to every voice (`CS:01c6`), as (register base, value).
const INIT_PATCH: [(u8, u8); 10] = [
    (0x20, 0x01),
    (0x23, 0x11),
    (0x40, 0x4f),
    (0x43, 0x00),
    (0x60, 0xf1),
    (0x63, 0xf2),
    (0x80, 0x53),
    (0x83, 0x74),
    (0xe0, 0x00),
    (0xe3, 0x00),
];

/// One entry of the F-number table `CS:0305`: 64 steps per semitone over one
/// octave, starting at 0x157 (C). Every entry of the original table equals this
/// formula (checked against the executable in the tests).
pub fn fnum(idx: usize) -> u16 {
    (342.9215 * 2f64.powf(idx as f64 / 768.0) + 0.5).floor() as u16
}

/// Octave/semitone byte of a MIDI note (`CS:0285`): high nibble octave,
/// low nibble semitone.
pub fn oct_semi(n: u8) -> u8 {
    match n {
        0..=23 => n % 12,
        24..=119 => ((n / 12 - 1) << 4) | (n % 12),
        _ => 0x7b,
    }
}

/// The driver.
#[derive(Clone)]
pub struct Fmdrv {
    fnum: Vec<u16>,
    instruments: Vec<[u8; INSTRUMENT_BYTES]>,
    /// `CS:01e7`, set by controller 0x67 only.
    rhythm_flag: bool,
    /// `CS:003a`: number of melodic voices (9 or 6).
    voices: usize,
    bd: u8,
    owner: [u8; 11],
    note_raw: [u8; 11],
    octsemi: [u8; 11],
    freq: [u16; 11],
    age: [u16; 11],
    ksl: [u8; 11],
    level: [u8; 11],
    program: [u8; 16],
    pitch: [i8; 16],
    channel_enable: [bool; 16],
    transpose: i8,
    volume: u8,
    tick_counter: u16,
    playing: bool,
    delay: u32,
    /// The status byte (`DS:94c4`).
    status: u8,
    /// Divisor of PIT channel 0 while playing.
    pub divisor: u16,
    music: Vec<u8>,
    pos: usize,
    running: u8,
}

impl Default for Fmdrv {
    fn default() -> Self {
        Self {
            fnum: (0..768).map(fnum).collect(),
            instruments: Vec::new(),
            rhythm_flag: false,
            voices: 9,
            bd: 0xc0,
            owner: [0xff; 11],
            note_raw: [0; 11],
            octsemi: [0; 11],
            freq: [0; 11],
            age: [0; 11],
            ksl: [0; 11],
            level: [0; 11],
            program: [0; 16],
            pitch: [0; 16],
            channel_enable: [true; 16],
            transpose: 0,
            volume: 0xff,
            tick_counter: 0,
            playing: false,
            delay: 0,
            status: 0,
            divisor: DEFAULT_DIVISOR,
            music: Vec::new(),
            pos: 0,
            running: 0,
        }
    }
}

impl Fmdrv {
    /// fn −1 (init) with its defaults: waveform select on, then reset.
    ///
    /// Origin: BMMAIN.EXE 4e67:1487 / 4e67:140d (init, defaults)
    pub fn init(opl: &mut impl OplWrite) -> Self {
        let mut d = Self::default();
        opl.write(0x01, 0x20);
        opl.write(0x08, 0x00);
        d.reset(opl);
        d
    }

    /// fn 8. The driver's 16 built-in instruments are not modelled: the game
    /// replaces them (fn 2) before playing.
    ///
    /// Origin: BMMAIN.EXE 4e67:0c00 (reset)
    pub fn reset(&mut self, opl: &mut impl OplWrite) {
        self.volume = 0xff;
        self.stop(opl);
        self.rhythm_off(opl);
        self.channel_enable = [true; 16];
        self.instruments.clear();
        self.voices_init(opl);
        self.divisor = DEFAULT_DIVISOR;
        self.transpose = 0;
    }

    /// fn 4: the PIT divisor of the music clock.
    ///
    /// Origin: BMMAIN.EXE 4e67:11ca (set driver clock)
    pub fn set_clock(&mut self, divisor: u16) {
        self.divisor = divisor;
    }

    /// fn 2: the instrument table (16-byte records).
    ///
    /// Origin: BMMAIN.EXE 4e67:1184 (set instruments)
    pub fn set_instruments(&mut self, opl: &mut impl OplWrite, table: &[[u8; INSTRUMENT_BYTES]]) {
        self.instruments = table.to_vec();
        self.rhythm_off(opl);
        self.voices_init(opl);
    }

    /// fn 6: starts the event stream `music` (the bytes from the CMF music offset).
    /// Returns false if music is already playing.
    ///
    /// Origin: BMMAIN.EXE 4e67:0c39 (play)
    pub fn play(&mut self, opl: &mut impl OplWrite, music: &[u8]) -> bool {
        if self.playing {
            return false;
        }
        self.music = music.to_vec();
        self.pos = 0;
        self.pitch = [0; 16];
        self.owner[..9].fill(0xff);
        self.delay = self.varlen();
        self.tick_counter = 0;
        self.rhythm_off(opl);
        self.playing = true;
        self.status = 0xff;
        true
    }

    /// fn 7 (also event FC and meta FF 2F).
    ///
    /// Origin: BMMAIN.EXE 4e67:1198 (stop)
    pub fn stop(&mut self, opl: &mut impl OplWrite) {
        if self.playing {
            self.playing = false;
            self.all_notes_off(opl);
            self.status = 0;
        }
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    /// `fmdrv_poll_status`: the status byte, reset to 0xff unless it is 0.
    ///
    /// Origin: BMMAIN.EXE 4d6c:0580 (fmdrv_poll_status)
    pub fn poll_status(&mut self) -> u8 {
        let s = self.status;
        if s != 0 {
            self.status = 0xff;
        }
        s
    }

    /// One INT 8 of the music clock.
    ///
    /// Origin: BMMAIN.EXE 4e67:1052 (INT 8 handler)
    pub fn tick(&mut self, opl: &mut impl OplWrite) {
        if !self.playing {
            return;
        }
        let fire = self.delay == 0;
        self.delay = self.delay.wrapping_sub(1);
        if fire {
            self.process_events(opl);
        }
    }

    /// Origin: BMMAIN.EXE 4e67:0cbf (event pump)
    fn process_events(&mut self, opl: &mut impl OplWrite) {
        self.tick_counter = self.tick_counter.wrapping_add(1);
        loop {
            self.dispatch_one(opl);
            if !self.playing {
                return;
            }
            if self.pos >= self.music.len() {
                // Not in the original (it would run into the following memory).
                self.stop(opl);
                return;
            }
            let d = self.varlen();
            if d != 0 {
                self.delay = d - 1;
                return;
            }
        }
    }

    fn byte(&mut self) -> u8 {
        let b = self.music.get(self.pos).copied().unwrap_or(0);
        self.pos += 1;
        b
    }

    fn varlen(&mut self) -> u32 {
        let mut v = 0u32;
        loop {
            let b = self.byte();
            v = (v << 7) | u32::from(b & 0x7f);
            if b & 0x80 == 0 || self.pos >= self.music.len() {
                return v;
            }
        }
    }

    fn skip(&mut self, n: usize) {
        self.pos += n;
    }

    fn dispatch_one(&mut self, opl: &mut impl OplWrite) {
        if self.pos >= self.music.len() {
            self.stop(opl);
            return;
        }
        if self.music[self.pos] & 0x80 != 0 {
            self.running = self.byte();
        }
        let ch = usize::from(self.running & 0x0f);
        match self.running & 0xf0 {
            0x80 => {
                let note = self.byte();
                self.skip(1);
                self.note_off(opl, ch, note);
            }
            0x90 => {
                let note = self.byte();
                let vel = self.byte();
                if vel == 0 {
                    self.note_off(opl, ch, note);
                } else {
                    self.note_on(opl, ch, note, vel);
                }
            }
            0xa0 | 0xe0 => self.skip(2),
            0xd0 => self.skip(1),
            0xb0 => {
                let ctrl = self.byte();
                let value = self.byte();
                self.controller(opl, ch, ctrl, value);
            }
            0xc0 => {
                let p = self.byte();
                self.program_change(opl, ch, p);
            }
            _ => match self.running {
                0xf0 | 0xf7 => {
                    let n = self.varlen() as usize;
                    self.skip(n);
                }
                0xf2 => self.skip(2),
                0xf3 => self.skip(1),
                0xfc => self.stop(opl),
                0xff => {
                    let kind = self.byte();
                    if kind == 0x2f {
                        self.stop(opl);
                    }
                    let n = self.varlen() as usize;
                    self.skip(n);
                }
                _ => {}
            },
        }
    }

    /// Origin: BMMAIN.EXE 4e67:0efa (controllers)
    fn controller(&mut self, opl: &mut impl OplWrite, ch: usize, ctrl: u8, value: u8) {
        match ctrl {
            0x66 => self.status = value,
            0x67 => {
                self.voices_init(opl);
                self.rhythm_flag = value != 0;
                if self.rhythm_flag {
                    self.voices = 6;
                    self.bd = 0xe0;
                } else {
                    self.voices = 9;
                    self.bd = 0xc0;
                }
                opl.write(0xbd, self.bd);
            }
            0x68 | 0x69 => {
                let v = if ctrl == 0x68 {
                    value as i8
                } else {
                    (value as i8).wrapping_neg()
                };
                self.pitch[ch] = v >> 2;
                for v in 0..self.voices {
                    if usize::from(self.owner[v]) == ch {
                        self.compute_freq(v, ch);
                        self.write_freq(opl, v, v as u8, true);
                    }
                }
            }
            _ => {}
        }
    }

    /// Origin: BMMAIN.EXE 4e67:0f90 (program change)
    fn program_change(&mut self, opl: &mut impl OplWrite, ch: usize, p: u8) {
        let count = self.instruments.len();
        let p = if count == 0 {
            p
        } else {
            (usize::from(p) % count) as u8
        };
        self.program[ch] = p;
        if self.voices <= 6 && ch >= 11 {
            self.load_instrument(opl, p, ch - 5);
            return;
        }
        for v in 0..self.voices {
            if usize::from(self.owner[v]) == ch | 0x80 {
                self.owner[v] = 0xff;
            } else if usize::from(self.owner[v]) == ch {
                self.load_instrument(opl, p, v);
            }
        }
    }

    /// Origin: BMMAIN.EXE 4e67:0a1e (load_instrument)
    fn load_instrument(&mut self, opl: &mut impl OplWrite, p: u8, v: usize) {
        let Some(b) = self.instruments.get(usize::from(p)).copied() else {
            return;
        };
        let lvl = if self.rhythm_flag && v >= 7 {
            b[2]
        } else {
            b[3]
        };
        let l = 0x3f - (lvl & 0x3f);
        self.ksl[v] = lvl & 0xc0;
        self.level[v] = ((u16::from(l) * u16::from(self.volume) + 0x80) >> 8) as u8;
        if v <= 6 {
            let op = MOD_OP[v];
            for (base, i) in [
                (0x20, 0),
                (0x23, 1),
                (0x40, 2),
                (0x60, 4),
                (0x63, 5),
                (0x80, 6),
                (0x83, 7),
                (0xe0, 8),
                (0xe3, 9),
            ] {
                opl.write(base + op, b[i]);
            }
            opl.write(0xc0 + v as u8, b[10]);
        } else {
            let op = DRUM_OP[v];
            opl.write(0x20 + op, b[0]);
            opl.write(0x60 + op, b[4]);
            opl.write(0x80 + op, b[6]);
            opl.write(0xe0 + op, b[8]);
            opl.write(0xc0 + DRUM_CH[v], b[10]);
        }
    }

    /// Origin: BMMAIN.EXE 4e67:0b6e (voice_alloc)
    fn voice_alloc(&mut self, opl: &mut impl OplWrite, ch: u8) -> usize {
        let n = self.voices;
        if let Some(v) = (0..n).find(|&v| self.owner[v] == ch | 0x80) {
            return v;
        }
        if let Some(v) = (0..n).find(|&v| self.owner[v] == 0xff) {
            return v;
        }
        if let Some(v) = (0..n).find(|&v| self.owner[v] > 0x7f) {
            return v;
        }
        let mut best = 0u16;
        let mut sel = 0;
        for v in 0..n {
            let d = self.tick_counter.wrapping_sub(self.age[v]);
            if d >= best {
                best = d;
                sel = v;
            }
        }
        self.write_freq(opl, sel, sel as u8, false);
        sel
    }

    /// Origin: BMMAIN.EXE 4e67:0da0 (note on)
    fn note_on(&mut self, opl: &mut impl OplWrite, ch: usize, note: u8, vel: u8) {
        if !self.channel_enable[ch] {
            return;
        }
        if self.rhythm_flag && ch >= 11 {
            self.rhythm_note_on(opl, ch, note, vel);
            return;
        }
        let v = self.voice_alloc(opl, ch as u8);
        let old = self.owner[v];
        self.owner[v] = ch as u8;
        if usize::from(old & 0x7f) != ch {
            self.load_instrument(opl, self.program[ch], v);
        }
        let att = ((u16::from(self.level[v]) * u16::from(vel | 0x80)) >> 8) as u8;
        opl.write(0x43 + MOD_OP[v], (0x3f - att) | self.ksl[v]);
        self.set_frequency(v, ch, note);
        self.write_freq(opl, v, v as u8, true);
    }

    /// Origin: BMMAIN.EXE 4e67:0e22 (rhythm_note_on)
    fn rhythm_note_on(&mut self, opl: &mut impl OplWrite, ch: usize, note: u8, vel: u8) {
        let s = ch - 5;
        self.bd |= BD_BIT[ch];
        let att = ((u16::from(self.level[s]) * u16::from(vel | 0x80)) >> 8) as u8;
        let reg = 0x40 + DRUM_OP[s] + if s == 6 { 3 } else { 0 };
        opl.write(reg, (0x3f_u8.wrapping_sub(att)) | self.ksl[s]);
        self.set_frequency(s, ch, note);
        self.write_freq(opl, s, DRUM_CH[s], false);
        opl.write(0xbd, self.bd);
    }

    /// Origin: BMMAIN.EXE 4e67:0d29 (note off)
    fn note_off(&mut self, opl: &mut impl OplWrite, ch: usize, note: u8) {
        if self.voices <= 6 && ch >= 11 {
            self.bd &= !BD_BIT[ch];
            opl.write(0xbd, self.bd);
            return;
        }
        if let Some(v) =
            (0..self.voices).find(|&v| self.note_raw[v] == note && usize::from(self.owner[v]) == ch)
        {
            self.owner[v] |= 0x80;
            self.write_freq(opl, v, v as u8, false);
        }
    }

    /// Origin: BMMAIN.EXE 4e67:0e75 (set_frequency)
    fn set_frequency(&mut self, v: usize, ch: usize, note: u8) {
        self.note_raw[v] = note;
        let n = (i16::from(note) + i16::from(self.transpose)).clamp(0, 127) as u8;
        self.octsemi[v] = oct_semi(n);
        self.compute_freq(v, ch);
    }

    /// Frequency word from the stored octave/semitone byte and the channel's
    /// pitch offset; renews the age stamp.
    ///
    /// Origin: BMMAIN.EXE 4e67:0e9e
    fn compute_freq(&mut self, v: usize, ch: usize) {
        let os = self.octsemi[v];
        let mut block4 = i16::from((os & 0x70) >> 2);
        let mut idx = i16::from(os & 0x0f) * 64 + i16::from(self.pitch[ch]);
        if idx < 0 {
            idx += 0x300;
            block4 -= 4;
            if block4 < 0 {
                block4 = 0;
                idx = 0;
            }
        } else if idx >= 0x300 {
            idx -= 0x300;
            block4 += 4;
            if block4 > 0x1c {
                idx = 0x2ff;
                block4 = 0x1c;
            }
        }
        self.freq[v] = self.fnum[idx as usize] | ((block4 as u16) << 8);
        self.age[v] = self.tick_counter;
    }

    fn write_freq(&self, opl: &mut impl OplWrite, v: usize, ch: u8, key: bool) {
        let f = self.freq[v];
        opl.write(0xa0 + ch, f as u8);
        opl.write(0xb0 + ch, (f >> 8) as u8 | if key { 0x20 } else { 0 });
    }

    /// Origin: BMMAIN.EXE 4e67:0bee (rhythm_off)
    fn rhythm_off(&mut self, opl: &mut impl OplWrite) {
        self.voices = 9;
        self.bd = 0xc0;
        opl.write(0xbd, self.bd);
    }

    /// Origin: BMMAIN.EXE 4e67:09ad (voices_init)
    fn voices_init(&mut self, opl: &mut impl OplWrite) {
        self.program = [0; 16];
        self.owner = [0xff; 11];
        for (v, &op) in MOD_OP.iter().enumerate() {
            self.rhythm_off(opl);
            opl.write(0x08, 0x00);
            for (base, val) in INIT_PATCH {
                opl.write(base + op, val);
            }
            // The driver's 11th patch byte goes to `op + v` instead of 0xC0 + v.
            opl.write(op + v as u8, 0x08);
        }
    }

    /// Origin: BMMAIN.EXE 4e67:0964 (all notes off)
    fn all_notes_off(&mut self, opl: &mut impl OplWrite) {
        // The driver's loop adds the voice number to the wrong register, so it
        // writes the same register nine times.
        for _ in 0..9 {
            opl.write(0x83, 0x13);
        }
        for v in 0..9 {
            if self.owner[v] <= 0x7f {
                self.write_freq(opl, v, v as u8, false);
            }
        }
        if self.voices <= 6 {
            self.bd &= 0xe0;
            opl.write(0xbd, self.bd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instrument(fill: u8) -> [u8; 16] {
        let mut b = [fill; 16];
        b[3] = 0x00; // carrier level 0 -> L = 63
        b
    }

    #[test]
    fn frequency_table() {
        let semis: Vec<u16> = (0..12).map(|s| fnum(s * 64)).collect();
        assert_eq!(
            semis,
            [
                0x157, 0x16b, 0x181, 0x198, 0x1b0, 0x1ca, 0x1e5, 0x202, 0x220, 0x241, 0x263, 0x287
            ]
        );
        assert_eq!(fnum(767), 0x2ad);
        assert_eq!(oct_semi(60), 0x40);
        assert_eq!(oct_semi(13), 1);
        assert_eq!(oct_semi(125), 0x7b);
    }

    #[test]
    fn frequency_table_matches_the_executable() {
        let Some(dir) = bmp_formats::data_dir() else {
            return;
        };
        let exe = std::fs::read(dir.join("BMMAIN.EXE")).unwrap();
        let image = bmp_formats::exepack::unpack(&exe).unwrap();
        // Segment 4e67 (Ghidra load segment 0x1000) + 0x305.
        let at = (0x4e67 - 0x1000) * 16 + 0x305;
        for i in 0..768 {
            let w = u16::from_le_bytes([image[at + 2 * i], image[at + 2 * i + 1]]);
            assert_eq!(w, fnum(i), "entry {i}");
        }
    }

    /// Runs `music` from play until it stops; returns the ticks and the writes.
    fn run(music: &[u8], instruments: &[[u8; 16]]) -> (u32, Vec<(u8, u8)>, Fmdrv) {
        let mut w = Vec::new();
        let mut d = Fmdrv::init(&mut w);
        d.set_instruments(&mut w, instruments);
        w.clear();
        assert!(d.play(&mut w, music));
        let mut ticks = 0;
        while d.playing() && ticks < 100_000 {
            d.tick(&mut w);
            ticks += 1;
        }
        (ticks, w, d)
    }

    #[test]
    fn note_on_timing_and_registers() {
        // delta 2: note on ch0 60/127; delta 3: note off; delta 0: end.
        let music = [2, 0x90, 60, 127, 3, 0x80, 60, 0, 0, 0xff, 0x2f, 0];
        let (ticks, w, mut d) = run(&music, &[instrument(0x11)]);
        // First event on INT 8 number delta0 + 1 = 3, the end 3 ticks later.
        assert_eq!(ticks, 6);
        assert_eq!(d.poll_status(), 0);
        // Note on: instrument load (lazy) into voice 0, TL, A0, B0 with key on.
        let i = w.iter().position(|&r| r == (0x20, 0x11)).unwrap();
        assert_eq!(&w[i..i + 3], &[(0x20, 0x11), (0x23, 0x11), (0x40, 0x11)]);
        let tl = w.iter().position(|&r| r.0 == 0x43).unwrap();
        // L = 63, vel 127 | 0x80 = 0xff: att = 62, TL = 1 (+ KSL 0).
        assert_eq!(w[tl], (0x43, 0x01));
        assert_eq!(&w[tl + 1..tl + 3], &[(0xa0, 0x57), (0xb0, 0x31)]);
        // Note off without key on, then stop: nine bogus 0x83 writes.
        assert_eq!(&w[tl + 3..tl + 5], &[(0xa0, 0x57), (0xb0, 0x11)]);
        assert_eq!(w[tl + 5], (0x83, 0x13));
    }

    #[test]
    fn status_byte_and_markers() {
        let music = [0, 0xb0, 0x66, 7, 1, 0xff, 0x2f, 0];
        let mut w = Vec::new();
        let mut d = Fmdrv::init(&mut w);
        d.play(&mut w, &music);
        assert_eq!(d.poll_status(), 0xff);
        d.tick(&mut w);
        assert_eq!(d.poll_status(), 7);
        assert_eq!(d.poll_status(), 0xff);
        d.tick(&mut w);
        assert_eq!(d.poll_status(), 0);
        assert!(!d.playing());
    }

    #[test]
    fn allocation_prefers_the_channels_released_voice() {
        // ch0 note 60 on/off, ch1 note 62 on, ch0 note 64 on -> voice 0 again.
        let music = [
            0, 0x90, 60, 100, 0, 60, 0, 0, 0x91, 62, 100, 0, 0x90, 64, 100, 0, 0xff, 0x2f, 0,
        ];
        let (_, _, d) = run(&music, &[instrument(0), instrument(1)]);
        assert_eq!(d.note_raw[0], 64);
        assert_eq!(d.note_raw[1], 62);
    }

    #[test]
    fn oldest_voice_is_stolen_with_ties_to_the_highest() {
        // Ten held notes on ch0 within one tick: all ages equal, so the
        // tenth note takes the highest voice (8).
        let mut music = Vec::new();
        for n in 0..10u8 {
            music.extend([0, 0x90, 40 + n, 100]);
        }
        music.extend([0, 0xff, 0x2f, 0]);
        let (_, _, d) = run(&music, &[instrument(0)]);
        assert_eq!(d.note_raw[8], 49);
        assert_eq!(d.note_raw[0], 40);
    }

    #[test]
    fn rhythm_mode_drums() {
        // Rhythm on, program 0 on ch11 (bass drum), drum note on/off.
        let music = [
            0, 0xb0, 0x67, 1, 0, 0xcb, 0, 0, 0x9b, 36, 127, 1, 0x9b, 36, 0, 0, 0xff, 0x2f, 0,
        ];
        let (_, w, d) = run(&music, &[instrument(0x22)]);
        assert_eq!(d.voices, 6);
        assert!(w.contains(&(0xbd, 0xe0)));
        assert!(w.contains(&(0x30, 0x22)), "bass drum modulator 0x20+0x10");
        assert!(w.contains(&(0xc6, 0x22)));
        let on = w.iter().position(|&r| r == (0xbd, 0xf0)).unwrap();
        // TL on 0x53, then A0/B0 of channel 6 without key on, then BD.
        assert_eq!(w[on - 3], (0x53, 0x01));
        assert_eq!(&w[on - 2..on], &[(0xa6, 0x57), (0xb6, 0x09)]);
        assert!(w[on..].contains(&(0xbd, 0xe0)));
    }

    #[test]
    fn original_title_music_length() {
        let Some(dir) = bmp_formats::data_dir() else {
            return;
        };
        let bytes = std::fs::read(dir.join("SOUND/BM2TITLE.CMF")).unwrap();
        let cmf = bmp_formats::cmf::Cmf::parse(&bytes).unwrap();
        let table: Vec<[u8; 16]> = cmf.instruments.iter().map(|i| i.to_bytes()).collect();
        let (ticks, _, _) = run(&cmf.music, &table);
        assert_eq!(ticks, 18762);
    }
}
