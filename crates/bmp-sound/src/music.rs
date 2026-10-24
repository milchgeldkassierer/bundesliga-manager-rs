//! The title tune as `main` starts it: SBFMDRV on an emulated OPL2, clocked
//! by the PIT ticks of the driver clock.

use bmp_formats::cmf::Cmf;

use crate::fmdrv::{Fmdrv, PIT_CLOCK_MAIN};
use crate::opl::Opl2;

/// PIT input clocks per OPL2 output sample: both derive from the 14.318 MHz
/// crystal (PIT = /12, OPL2 sample = /(4 * 72)).
const PIT_CLOCKS_PER_SAMPLE: u32 = 24;

/// A playing CMF tune.
#[derive(Clone)]
pub struct Music {
    pub opl: Opl2,
    pub drv: Fmdrv,
    /// PIT clocks since the last driver tick.
    pit: u32,
}

impl Music {
    /// The driver calls of `main` (BMMAIN.EXE 18bc:0418, flow.md §2): init,
    /// reset, driver clock `0x1234DC / ticks_per_second`, instruments, play.
    pub fn start(cmf: &Cmf) -> Self {
        Self::start_on(cmf, Opl2::new())
    }

    /// [`Music::start`] on a given chip (e.g. one with its write log enabled).
    pub fn start_on(cmf: &Cmf, mut opl: Opl2) -> Self {
        let mut drv = Fmdrv::init(&mut opl);
        drv.reset(&mut opl);
        // `_aFldiv` of 0x1234DC by the sign-extended word, truncated to 16 bits.
        let tps = i32::from(cmf.ticks_per_second as i16).max(1);
        drv.set_clock((PIT_CLOCK_MAIN as i32 / tps) as u16);
        if cmf.instrument_offset != 0 {
            let table: Vec<_> = cmf.instruments.iter().map(|i| i.to_bytes()).collect();
            drv.set_instruments(&mut opl, &table);
        }
        drv.play(&mut opl, &cmf.music);
        Self { opl, drv, pit: 0 }
    }

    fn clock(&mut self) {
        self.pit += PIT_CLOCKS_PER_SAMPLE;
        let div = u32::from(self.drv.divisor).max(1);
        while self.pit >= div {
            self.pit -= div;
            self.drv.tick(&mut self.opl);
        }
    }

    /// Renders `out.len()` samples at [`crate::opl::NATIVE_RATE`].
    pub fn render(&mut self, out: &mut [i16]) {
        for o in out {
            self.clock();
            *o = self.opl.sample();
        }
    }

    /// Advances by `samples` native samples running the driver only (the
    /// chip registers are written, no sound is synthesised).
    pub fn advance(&mut self, samples: u64) {
        for _ in 0..samples {
            if !self.drv.playing() {
                return;
            }
            self.clock();
        }
    }

    /// `fmdrv_poll_status`: non-zero while the tune plays.
    pub fn poll_status(&mut self) -> u8 {
        self.drv.poll_status()
    }

    /// `fmdrv_stop_music`.
    pub fn stop(&mut self) {
        self.drv.stop(&mut self.opl);
    }

    /// Renders the whole tune from the start until the driver stops, plus
    /// `tail` samples of release; returns native-rate samples and, with `log`,
    /// the chip's register writes (sample number, register, value).
    pub fn render_all(cmf: &Cmf, tail: usize, log: bool) -> (Vec<i16>, Vec<(u64, u8, u8)>) {
        let mut opl = Opl2::new();
        if log {
            opl.write_log = Some(Vec::new());
        }
        let mut m = Self::start_on(cmf, opl);
        let mut out = Vec::new();
        let mut buf = [0i16; 1024];
        while m.drv.playing() {
            m.render(&mut buf);
            out.extend_from_slice(&buf);
        }
        let mut rest = vec![0i16; tail];
        m.render(&mut rest);
        out.extend(rest);
        (out, m.opl.write_log.take().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_title_music_duration() {
        let Some(dir) = bmp_formats::data_dir() else {
            return;
        };
        let bytes = std::fs::read(dir.join("SOUND/BM2TITLE.CMF")).unwrap();
        let cmf = Cmf::parse(&bytes).unwrap();
        let mut m = Music::start(&cmf);
        assert_eq!(m.drv.divisor, 12428);
        let mut n = 0u64;
        while m.drv.playing() {
            m.advance(1);
            n += 1;
        }
        // 18762 ticks of 12428 PIT clocks = 195.42 s.
        let expected = 18762 * 12428 / 24;
        assert!(n.abs_diff(expected) <= 1, "{n} vs {expected}");
    }
}
