//! A YM3812 (OPL2) FM synthesizer, written from the chip's documented
//! behaviour (Yamaha YM3812 application manual, plus the published die-shot
//! analyses of its log-sine/exponent ROMs, envelope rate counter and rhythm
//! phase logic). It runs at the chip's native sample rate
//! ([`NATIVE_RATE`] = 3.579545 MHz / 72) and produces one mono 16-bit sample
//! per call of [`Opl2::sample`].
//!
//! Not modelled: the two timers, the status register and CSM speech mode
//! (the game's music driver does not use them).

/// Output sample rate of a YM3812 clocked at 3.579545 MHz.
pub const NATIVE_RATE: f64 = 3_579_545.0 / 72.0;

/// Frequency multiplier register value -> multiplier x 2.
const MULT_X2: [u32; 16] = [1, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 20, 24, 24, 30, 30];

/// Key scale level attenuation in 0.75 dB-ish steps (x4 = envelope units)
/// for the top four F-number bits, at block 7.
const KSL_ROM: [i32; 16] = [
    0, 32, 40, 45, 48, 51, 53, 55, 56, 58, 59, 60, 61, 62, 63, 64,
];

/// Envelope increment patterns for the four sub-rates (rate & 3); one of the
/// eight entries is used per envelope step.
const EG_PATTERN: [[u8; 8]; 4] = [
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 1, 1, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 1],
];

/// Extra increments of the sub-rates at rates 52..59 (added to the base step).
const EG_EXTRA: [[u8; 8]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 1, 0, 0, 0, 1],
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
];

/// Operator ("slot") number for register offsets 0x00..0x15 of the operator
/// register banks (0x20, 0x40, 0x60, 0x80, 0xE0); `None` for the gaps.
fn op_of_offset(off: u8) -> Option<usize> {
    let (row, col) = (off / 8, off % 8);
    (row < 3 && col < 6).then(|| usize::from(row) * 6 + usize::from(col))
}

/// The modulator operator of channel `ch`; the carrier is this + 3.
pub fn modulator_of(ch: usize) -> usize {
    (ch / 3) * 6 + ch % 3
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EgState {
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone, Copy)]
struct Operator {
    am: bool,
    vib: bool,
    sustained: bool,
    ksr: bool,
    mult: u8,
    ksl: u8,
    tl: u8,
    ar: u8,
    dr: u8,
    sl: u8,
    rr: u8,
    wave: u8,
    /// 19-bit phase accumulator.
    phase: u32,
    /// 10-bit phase presented to the waveform stage this sample.
    phase_out: u32,
    /// Attenuation, 9 bits, 0.1875 dB units (0 = loudest).
    level: u16,
    state: EgState,
    /// Key state from the channel key-on bit and the rhythm bits.
    key_channel: bool,
    key_rhythm: bool,
    out: i32,
    prev_out: i32,
}

impl Default for Operator {
    fn default() -> Self {
        Self {
            am: false,
            vib: false,
            sustained: false,
            ksr: false,
            mult: 0,
            ksl: 0,
            tl: 0,
            ar: 0,
            dr: 0,
            sl: 0,
            rr: 0,
            wave: 0,
            phase: 0,
            phase_out: 0,
            level: 0x1ff,
            state: EgState::Release,
            key_channel: false,
            key_rhythm: false,
            out: 0,
            prev_out: 0,
        }
    }
}

impl Operator {
    fn key(&self) -> bool {
        self.key_channel || self.key_rhythm
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Channel {
    fnum: u16,
    block: u8,
    feedback: u8,
    additive: bool,
}

impl Channel {
    /// Key scale value for the rate calculation (0..15).
    fn key_scale(&self, nts: bool) -> u8 {
        let bit = if nts {
            (self.fnum >> 8) & 1
        } else {
            (self.fnum >> 9) & 1
        };
        (self.block << 1) | bit as u8
    }

    /// Key scale level attenuation before the KSL shift, in envelope units.
    fn ksl_base(&self) -> i32 {
        let v = (KSL_ROM[usize::from(self.fnum >> 6)] << 2) - ((8 - i32::from(self.block)) << 5);
        v.max(0)
    }
}

/// The emulated chip.
#[derive(Clone)]
pub struct Opl2 {
    regs: [u8; 256],
    ops: [Operator; 18],
    channels: [Channel; 9],
    wave_enable: bool,
    nts: bool,
    deep_am: bool,
    deep_vib: bool,
    rhythm: bool,
    /// Global sample counter: envelope, tremolo and vibrato clocks.
    counter: u32,
    tremolo_pos: u32,
    noise: u32,
    /// Samples produced so far.
    samples: u64,
    /// Register writes with the sample number they happened before, when enabled.
    pub write_log: Option<Vec<(u64, u8, u8)>>,
    log_sin: Box<[u16; 256]>,
    exp: Box<[u16; 256]>,
}

impl Default for Opl2 {
    fn default() -> Self {
        Self::new()
    }
}

impl Opl2 {
    pub fn new() -> Self {
        let mut log_sin = Box::new([0u16; 256]);
        let mut exp = Box::new([0u16; 256]);
        for i in 0..256 {
            let x = ((i as f64 + 0.5) / 256.0 * std::f64::consts::FRAC_PI_2).sin();
            log_sin[i] = (-x.log2() * 256.0).round() as u16;
            exp[i] = ((2f64.powf(i as f64 / 256.0) - 1.0) * 1024.0).round() as u16;
        }
        Self {
            regs: [0; 256],
            ops: [Operator::default(); 18],
            channels: [Channel::default(); 9],
            wave_enable: false,
            nts: false,
            deep_am: false,
            deep_vib: false,
            rhythm: false,
            counter: 0,
            tremolo_pos: 0,
            noise: 1,
            samples: 0,
            write_log: None,
            log_sin,
            exp,
        }
    }

    /// The last value written to register `r`.
    pub fn reg(&self, r: u8) -> u8 {
        self.regs[usize::from(r)]
    }

    /// Current attenuation of operator `op` (0 = full volume, 511 = silent).
    pub fn envelope_level(&self, op: usize) -> u16 {
        self.ops[op].level
    }

    /// Writes register `r`.
    pub fn write(&mut self, r: u8, v: u8) {
        if let Some(log) = &mut self.write_log {
            log.push((self.samples, r, v));
        }
        self.regs[usize::from(r)] = v;
        match r {
            0x01 => {
                self.wave_enable = v & 0x20 != 0;
            }
            0x08 => self.nts = v & 0x40 != 0,
            0x20..=0x35 => {
                if let Some(o) = op_of_offset(r - 0x20) {
                    let op = &mut self.ops[o];
                    op.am = v & 0x80 != 0;
                    op.vib = v & 0x40 != 0;
                    op.sustained = v & 0x20 != 0;
                    op.ksr = v & 0x10 != 0;
                    op.mult = v & 0x0f;
                }
            }
            0x40..=0x55 => {
                if let Some(o) = op_of_offset(r - 0x40) {
                    self.ops[o].ksl = v >> 6;
                    self.ops[o].tl = v & 0x3f;
                }
            }
            0x60..=0x75 => {
                if let Some(o) = op_of_offset(r - 0x60) {
                    self.ops[o].ar = v >> 4;
                    self.ops[o].dr = v & 0x0f;
                }
            }
            0x80..=0x95 => {
                if let Some(o) = op_of_offset(r - 0x80) {
                    let sl = v >> 4;
                    self.ops[o].sl = if sl == 15 { 31 } else { sl };
                    self.ops[o].rr = v & 0x0f;
                }
            }
            0xa0..=0xa8 => {
                let ch = &mut self.channels[usize::from(r - 0xa0)];
                ch.fnum = (ch.fnum & 0x300) | u16::from(v);
            }
            0xb0..=0xb8 => {
                let c = usize::from(r - 0xb0);
                let ch = &mut self.channels[c];
                ch.fnum = (ch.fnum & 0xff) | (u16::from(v & 3) << 8);
                ch.block = (v >> 2) & 7;
                let key = v & 0x20 != 0;
                let m = modulator_of(c);
                self.set_key(m, key, false);
                self.set_key(m + 3, key, false);
            }
            0xbd => {
                self.deep_am = v & 0x80 != 0;
                self.deep_vib = v & 0x40 != 0;
                self.rhythm = v & 0x20 != 0;
                let on = |bit: u8| self.rhythm && v & bit != 0;
                let (bd, sd, tom, tc, hh) = (on(0x10), on(0x08), on(0x04), on(0x02), on(0x01));
                self.set_key(12, bd, true);
                self.set_key(15, bd, true);
                self.set_key(16, sd, true);
                self.set_key(14, tom, true);
                self.set_key(17, tc, true);
                self.set_key(13, hh, true);
            }
            0xc0..=0xc8 => {
                let ch = &mut self.channels[usize::from(r - 0xc0)];
                ch.feedback = (v >> 1) & 7;
                ch.additive = v & 1 != 0;
            }
            0xe0..=0xf5 => {
                if let Some(o) = op_of_offset(r - 0xe0) {
                    self.ops[o].wave = v & 3;
                }
            }
            _ => {}
        }
    }

    /// Updates one of the two key sources of an operator; a rising key
    /// restarts the envelope (attack) and the phase.
    fn set_key(&mut self, o: usize, on: bool, rhythm: bool) {
        let op = &mut self.ops[o];
        let was = op.key();
        if rhythm {
            op.key_rhythm = on;
        } else {
            op.key_channel = on;
        }
        if !was && op.key() {
            op.state = EgState::Attack;
            op.phase = 0;
        } else if was && !op.key() {
            op.state = EgState::Release;
        }
    }

    /// Effective envelope rate 0..63 of operator `o` for register rate `reg`.
    fn eff_rate(&self, o: usize, reg: u8) -> u8 {
        if reg == 0 {
            return 0;
        }
        let ch = &self.channels[channel_of(o)];
        let ks = ch.key_scale(self.nts);
        let ks = if self.ops[o].ksr { ks } else { ks >> 2 };
        (reg * 4 + ks).min(63)
    }

    /// The increment of one envelope step at `rate` this sample (0 = no step).
    fn eg_step(&self, rate: u8) -> u16 {
        if rate == 0 {
            return 0;
        }
        let hi = u32::from(rate >> 2);
        let lo = usize::from(rate & 3);
        if hi < 13 {
            let shift = 12 - hi;
            if self.counter & ((1 << shift) - 1) != 0 {
                return 0;
            }
            u16::from(EG_PATTERN[lo][((self.counter >> shift) & 7) as usize])
        } else {
            // Rates 52..59 step every sample by 1 (or 2), doubled by the extra
            // pattern of the sub-rate; rates 60..63 always by 4.
            if hi == 15 {
                return 4;
            }
            let extra = u16::from(EG_EXTRA[lo][(self.counter & 7) as usize]);
            (1 + extra) << (hi - 13)
        }
    }

    fn envelope(&mut self, o: usize) {
        let op = self.ops[o];
        match op.state {
            EgState::Attack => {
                let rate = self.eff_rate(o, op.ar);
                let mut level = op.level;
                if rate >= 60 {
                    level = 0;
                } else {
                    let inc = self.eg_step(rate);
                    if inc > 0 && level > 0 {
                        // The attack curve: each step removes a fraction of the
                        // remaining attenuation (exponential approach to 0 dB).
                        let shift = match inc {
                            1 => 3,
                            2 => 2,
                            _ => 1,
                        };
                        let dec = (level + 1).div_ceil(1 << shift);
                        level = level.saturating_sub(dec);
                    }
                }
                let op = &mut self.ops[o];
                op.level = level;
                if level == 0 {
                    op.state = EgState::Decay;
                }
            }
            EgState::Decay => {
                if (op.level >> 4) >= u16::from(op.sl) {
                    self.ops[o].state = EgState::Sustain;
                    return;
                }
                let inc = self.eg_step(self.eff_rate(o, op.dr));
                self.ops[o].level = (op.level + inc).min(0x1ff);
            }
            EgState::Sustain | EgState::Release => {
                let reg = if op.state == EgState::Sustain && op.sustained {
                    0
                } else {
                    op.rr
                };
                let inc = self.eg_step(self.eff_rate(o, reg));
                let mut level = (op.level + inc).min(0x1ff);
                if level >= 0x1f8 {
                    level = 0x1ff;
                }
                self.ops[o].level = level;
            }
        }
    }

    /// Advances the phase of operator `o` by one sample.
    fn advance_phase(&mut self, o: usize) {
        let ch = self.channels[channel_of(o)];
        let mut fnum = i32::from(ch.fnum);
        if self.ops[o].vib {
            let pos = (self.counter >> 10) & 7;
            let mut range = (fnum >> 7) & 7;
            if pos & 3 == 0 {
                range = 0;
            } else if pos & 1 != 0 {
                range >>= 1;
            }
            if !self.deep_vib {
                range >>= 1;
            }
            if pos & 4 != 0 {
                range = -range;
            }
            fnum += range;
        }
        let base = ((fnum as u32) << ch.block) >> 1;
        let op = &mut self.ops[o];
        op.phase = (op.phase + ((base * MULT_X2[usize::from(op.mult)]) >> 1)) & 0x7ffff;
        op.phase_out = op.phase >> 9;
    }

    /// Waveform stage: phase (10 bits) plus attenuation -> signed output.
    fn wave_out(&self, wave: u8, phase: u32, atten: u32) -> i32 {
        let phase = phase & 0x3ff;
        let quarter = |p: u32| {
            if p & 0x100 != 0 {
                u32::from(self.log_sin[((p & 0xff) ^ 0xff) as usize])
            } else {
                u32::from(self.log_sin[(p & 0xff) as usize])
            }
        };
        let silent = 0x1000;
        let (log, negative) = match if self.wave_enable { wave } else { 0 } {
            0 => (quarter(phase), phase & 0x200 != 0),
            1 => (
                if phase & 0x200 != 0 {
                    silent
                } else {
                    quarter(phase)
                },
                false,
            ),
            2 => (quarter(phase), false),
            _ => (
                if phase & 0x100 != 0 {
                    silent
                } else {
                    u32::from(self.log_sin[(phase & 0xff) as usize])
                },
                false,
            ),
        };
        let total = (log + (atten << 3)).min(0x1fff);
        let mag = (((u32::from(self.exp[((total & 0xff) ^ 0xff) as usize]) | 0x400) << 1)
            >> (total >> 8)) as i32;
        if negative { -mag - 1 } else { mag }
    }

    /// Total attenuation of operator `o` (envelope + TL + KSL + tremolo).
    fn attenuation(&self, o: usize) -> u32 {
        let op = &self.ops[o];
        let ch = &self.channels[channel_of(o)];
        let ksl = match op.ksl {
            0 => 0,
            1 => ch.ksl_base() >> 1,
            2 => ch.ksl_base() >> 2,
            _ => ch.ksl_base(),
        };
        let trem = if op.am {
            let p = self.tremolo_pos;
            let t = if p < 105 { p } else { 210 - p };
            if self.deep_am { t >> 2 } else { t >> 4 }
        } else {
            0
        };
        (u32::from(op.level) + (u32::from(op.tl) << 2) + ksl as u32 + trem).min(0x1ff)
    }

    /// Computes the output of operator `o` for phase `phase` (incl. modulation).
    fn op_out(&mut self, o: usize, phase: u32) -> i32 {
        let atten = self.attenuation(o);
        let out = self.wave_out(self.ops[o].wave, phase, atten);
        let op = &mut self.ops[o];
        op.prev_out = op.out;
        op.out = out;
        out
    }

    /// A normal two-operator channel; returns its output.
    fn channel_out(&mut self, c: usize) -> i32 {
        let m = modulator_of(c);
        let ch = self.channels[c];
        let fb = if ch.feedback != 0 {
            (self.ops[m].prev_out + self.ops[m].out) >> (9 - ch.feedback)
        } else {
            0
        };
        let phase_m = self.ops[m].phase_out.wrapping_add(fb as u32);
        let mo = self.op_out(m, phase_m);
        if ch.additive {
            let co = self.op_out(m + 3, self.ops[m + 3].phase_out);
            mo + co
        } else {
            let phase_c = self.ops[m + 3].phase_out.wrapping_add(mo as u32);
            self.op_out(m + 3, phase_c)
        }
    }

    /// Produces the next output sample.
    pub fn sample(&mut self) -> i16 {
        for o in 0..18 {
            self.envelope(o);
            self.advance_phase(o);
        }
        let mut sum = 0i32;
        let melodic = if self.rhythm { 6 } else { 9 };
        for c in 0..melodic {
            sum += self.channel_out(c);
        }
        if self.rhythm {
            sum += self.rhythm_out();
        }
        // Clocks: noise every sample, tremolo every 64, envelope counter.
        let bit = ((self.noise >> 14) ^ self.noise) & 1;
        self.noise = (self.noise >> 1) | (bit << 22);
        self.counter = self.counter.wrapping_add(1);
        self.samples += 1;
        if self.counter & 63 == 0 {
            self.tremolo_pos = (self.tremolo_pos + 1) % 210;
        }
        sum.clamp(i16::MIN.into(), i16::MAX.into()) as i16
    }

    /// The five rhythm instruments on channels 6..8 (each output doubled).
    fn rhythm_out(&mut self) -> i32 {
        // Bass drum: channel 6 as a normal channel, except that in additive
        // mode only the carrier sounds.
        let ch6 = self.channels[6];
        let bd = if ch6.additive {
            let fb = if ch6.feedback != 0 {
                (self.ops[12].prev_out + self.ops[12].out) >> (9 - ch6.feedback)
            } else {
                0
            };
            self.op_out(12, self.ops[12].phase_out.wrapping_add(fb as u32));
            self.op_out(15, self.ops[15].phase_out)
        } else {
            self.channel_out(6)
        };
        let hh_phase = self.ops[13].phase_out;
        let tc_phase = self.ops[17].phase_out;
        let b = |p: u32, n: u32| (p >> n) & 1;
        let xor = (b(hh_phase, 2) ^ b(hh_phase, 7))
            | (b(hh_phase, 3) ^ b(tc_phase, 5))
            | (b(tc_phase, 3) ^ b(tc_phase, 5));
        let noise = self.noise & 1;
        let hh = (xor << 9) | if xor ^ noise != 0 { 0xd0 } else { 0x34 };
        let sd = (b(hh_phase, 8) << 9) | ((b(hh_phase, 8) ^ noise) << 8);
        let tc = (xor << 9) | 0x80;
        let hh_out = self.op_out(13, hh);
        let sd_out = self.op_out(16, sd);
        let tom_out = self.op_out(14, self.ops[14].phase_out);
        let tc_out = self.op_out(17, tc);
        2 * (bd + hh_out + sd_out + tom_out + tc_out)
    }
}

fn channel_of(o: usize) -> usize {
    (o / 6) * 3 + (o % 6) % 3
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain sine voice on channel 0: modulator silent, carrier at 0 dB.
    fn sine_voice(opl: &mut Opl2, ar: u8, dr: u8, sl: u8, rr: u8) {
        opl.write(0x20, 0x01);
        opl.write(0x23, 0x21); // sustained, mult 1
        opl.write(0x40, 0x3f);
        opl.write(0x43, 0x00);
        opl.write(0x60, 0xff);
        opl.write(0x63, (ar << 4) | dr);
        opl.write(0x83, (sl << 4) | rr);
        opl.write(0xc0, 0x00);
    }

    #[test]
    fn operator_mapping() {
        assert_eq!(op_of_offset(0x00), Some(0));
        assert_eq!(op_of_offset(0x03), Some(3));
        assert_eq!(op_of_offset(0x08), Some(6));
        assert_eq!(op_of_offset(0x15), Some(17));
        assert_eq!(op_of_offset(0x06), None);
        assert_eq!(modulator_of(7), 13);
        for c in 0..9 {
            assert_eq!(channel_of(modulator_of(c)), c);
            assert_eq!(channel_of(modulator_of(c) + 3), c);
        }
    }

    #[test]
    fn tables_match_rom_endpoints() {
        let opl = Opl2::new();
        assert_eq!(opl.log_sin[0], 2137);
        assert_eq!(opl.log_sin[255], 0);
        assert_eq!(opl.exp[0], 0);
        assert_eq!(opl.exp[255], 1018);
    }

    #[test]
    fn silent_after_reset() {
        let mut opl = Opl2::new();
        assert!((0..1000).all(|_| opl.sample() == 0));
    }

    #[test]
    fn sine_has_the_programmed_frequency() {
        let mut opl = Opl2::new();
        sine_voice(&mut opl, 15, 0, 0, 0);
        // 440 Hz: fnum = 440 * 2^20 / 49716 / 2^4 = 580 at block 4.
        opl.write(0xa0, (580 & 0xff) as u8);
        opl.write(0xb0, 0x20 | (4 << 2) | (580 >> 8) as u8);
        let n = NATIVE_RATE as usize;
        let s: Vec<i16> = (0..n).map(|_| opl.sample()).collect();
        let crossings = s.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count();
        assert!((438..=442).contains(&crossings), "{crossings}");
        let peak = s.iter().map(|v| v.unsigned_abs()).max().unwrap();
        assert!((4000..=4100).contains(&peak), "{peak}");
    }

    #[test]
    fn envelope_attack_decay_release() {
        let mut opl = Opl2::new();
        sine_voice(&mut opl, 12, 12, 4, 10);
        opl.write(0xa0, 0x44);
        opl.write(0xb0, 0x20 | (4 << 2) | 1);
        assert_eq!(opl.envelope_level(3), 0x1ff);
        let mut t = 0;
        while opl.envelope_level(3) != 0 {
            opl.sample();
            t += 1;
        }
        // Attack rate 12 (effective rate 50 with the key scale of block 4):
        // datasheet 1.38 ms at rate 48, about 1 ms at rate 50.
        assert!((30..=80).contains(&t), "attack {t} samples");
        for _ in 0..5000 {
            opl.sample();
        }
        // Sustained at SL 4 = 12 dB = 64 units.
        assert_eq!(opl.envelope_level(3) >> 4, 4);
        opl.write(0xb0, (4 << 2) | 1);
        let mut t = 0;
        while opl.envelope_level(3) != 0x1ff {
            opl.sample();
            t += 1;
        }
        // Release rate 10 at block 4 is effective rate 42: one unit per 16/3
        // samples, 440 units from 12 dB to the cut-off.
        assert!((2200..=2500).contains(&t), "release {t} samples");
    }

    #[test]
    fn decay_time_matches_datasheet() {
        let mut opl = Opl2::new();
        sine_voice(&mut opl, 15, 8, 15, 0);
        opl.write(0x23, 0x01); // not sustained
        // Block 0, F-number 0: no key scaling, so the effective rate is 32.
        opl.write(0xb0, 0x20);
        opl.sample();
        let mut t = 0;
        while opl.envelope_level(3) < 0x1f0 {
            opl.sample();
            t += 1;
        }
        // Decay rate 8: 0..96 dB in 327 ms (datasheet 0-96 dB: 335 ms).
        let ms = t as f64 * 1000.0 / NATIVE_RATE;
        assert!((300.0..=360.0).contains(&ms), "{ms} ms");
    }

    #[test]
    fn decay_times_halve_per_rate_step() {
        // Time from 0 dB to the cut-off for effective rates 48, 52, 56, 60
        // (block 0, no key scaling): datasheet 0..96 dB 19.2, 9.6, 4.8, 2.4 ms.
        let mut prev = f64::MAX;
        for dr in [12u8, 13, 14, 15] {
            let mut opl = Opl2::new();
            sine_voice(&mut opl, 15, dr, 15, 0);
            opl.write(0x23, 0x01);
            opl.write(0xb0, 0x20);
            opl.sample();
            let mut t = 0;
            while opl.envelope_level(3) < 0x1f0 {
                opl.sample();
                t += 1;
            }
            let ms = t as f64 * 1000.0 / NATIVE_RATE;
            let expected = 19.2 / f64::from(1 << (dr - 12));
            assert!((ms / expected - 1.0).abs() < 0.12, "rate {dr}: {ms} ms");
            assert!(ms < prev);
            prev = ms;
        }
    }

    #[test]
    fn rhythm_bass_drum_keys_both_operators() {
        let mut opl = Opl2::new();
        opl.write(0xbd, 0x30);
        assert!(opl.ops[12].key() && opl.ops[15].key() && !opl.ops[13].key());
        opl.write(0xbd, 0x21);
        assert!(!opl.ops[12].key() && opl.ops[13].key());
        opl.write(0xbd, 0x01);
        assert!(!opl.ops[13].key(), "rhythm bits need rhythm mode");
    }

    #[test]
    fn waveform_select_needs_enable_bit() {
        let mut opl = Opl2::new();
        sine_voice(&mut opl, 15, 0, 0, 0);
        opl.write(0xe3, 1); // half sine
        opl.write(0xa0, 0x44);
        opl.write(0xb0, 0x20 | (4 << 2) | 1);
        let neg = |opl: &mut Opl2| (0..5000).filter(|_| opl.sample() < -10).count();
        assert!(neg(&mut opl) > 1000);
        opl.write(0x01, 0x20);
        assert_eq!(neg(&mut opl), 0);
    }
}
