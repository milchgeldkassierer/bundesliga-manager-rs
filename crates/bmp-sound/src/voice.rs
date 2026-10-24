//! The digitised voices of `SOUND\DIGI.VOC` with the semantics of the game's
//! CT-VOICE wrappers (`docs/re/voc.md` §4): one voice at a time, voice 1 is
//! the looping crowd noise that comes back after a one-shot.

use bmp_formats::voc::{self, Pcm};

/// Voice numbers of `sound_play_voice`.
pub const CROWD: u8 = 1;
pub const GOAL: u8 = 2;
pub const WHISTLE: u8 = 3;
pub const INJURY: u8 = 4;
pub const MISS: u8 = 5;

/// The five samples and the playback state.
#[derive(Clone)]
pub struct Voices {
    /// By DIGI.VOC slot.
    slots: Vec<Pcm>,
    /// `DS:018e`: voice last started (0 = none).
    pub current: u8,
    /// `DS:018f`: the crowd loop is wanted.
    pub loop_wanted: bool,
    /// Slot and position (in samples of the slot's rate, 32.32 fixed point)
    /// of the running output; `None` = CT-VOICE status word 0.
    playing: Option<(usize, u64)>,
}

impl Voices {
    /// Loads the samples from the bytes of `SOUND\DIGI.VOC`.
    ///
    /// Origin: BMMAIN.EXE 176b:005f (sound_load_digi_voc)
    pub fn from_digi(bytes: &[u8]) -> Result<Self, voc::VocError> {
        let slots = voc::split_digi(bytes)?
            .iter()
            .map(|v| v.render_from(v.data_offset).ok_or(voc::VocError::BadHeader))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            slots,
            current: 0,
            loop_wanted: false,
            playing: None,
        })
    }

    /// Starts voice `n` (1..5), cutting off whatever plays.
    ///
    /// Origin: BMMAIN.EXE 176b:0182 (sound_play_voice)
    pub fn play(&mut self, n: u8) {
        let Some(&slot) = voc::DIGI_VOICE_SLOT.get(usize::from(n)) else {
            return;
        };
        self.current = n;
        if n == CROWD {
            self.loop_wanted = true;
        }
        self.playing = (slot < self.slots.len()).then_some((slot, 0));
    }

    /// Origin: BMMAIN.EXE 176b:020f (sound_stop_voice)
    pub fn stop(&mut self) {
        if self.current == CROWD {
            self.loop_wanted = false;
        }
        self.current = 0;
        self.playing = None;
    }

    /// The CT-VOICE status word: output running.
    pub fn busy(&self) -> bool {
        self.playing.is_some()
    }

    /// The bookkeeping of `sound_wait_voice_end` once the output has ended.
    ///
    /// Origin: BMMAIN.EXE 176b:024b (sound_wait_voice_end)
    pub fn ended(&mut self) {
        self.current = 0;
    }

    /// Restarts the crowd when nothing plays and the loop is wanted.
    ///
    /// Origin: BMMAIN.EXE 176b:027d (sound_restart_loop_voice)
    pub fn restart_loop(&mut self) {
        if !self.busy() && self.loop_wanted {
            self.play(CROWD);
        }
    }

    /// Mixes the running voice into `out` (mono, `rate` Hz), advancing it;
    /// samples are held (no interpolation), as the DSP outputs them.
    pub fn mix(&mut self, out: &mut [i16], rate: u32) {
        let Some((slot, mut pos)) = self.playing else {
            return;
        };
        let pcm = &self.slots[slot];
        let step = (u64::from(pcm.rate) << 32) / u64::from(rate.max(1));
        for o in out.iter_mut() {
            let Some(s) = pcm.at(pos >> 32) else {
                self.playing = None;
                return;
            };
            *o = o.saturating_add((i16::from(s) - 128) << 7);
            pos += step;
        }
        self.playing = Some((slot, pos));
    }

    /// Advances the running voice by `samples` output samples at `rate` Hz
    /// without producing sound (headless runs).
    pub fn advance(&mut self, samples: u64, rate: u32) {
        let Some((slot, pos)) = self.playing else {
            return;
        };
        let pcm = &self.slots[slot];
        let step = (u64::from(pcm.rate) << 32) / u64::from(rate.max(1));
        let pos = pos + step * samples;
        self.playing = match pcm.total_len() {
            Some(len) if pos >> 32 >= len => None,
            _ => Some((slot, pos)),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voices() -> Voices {
        let mk = |rate, n: usize, repeat| Pcm {
            rate,
            samples: vec![200; n],
            repeat,
        };
        Voices {
            slots: vec![
                mk(10_000, 10, None),
                mk(10_000, 20, None),
                mk(10_000, 5, Some((0..5, 0xffff))),
                mk(20_000, 10, None),
                mk(20_000, 10, None),
            ],
            current: 0,
            loop_wanted: false,
            playing: None,
        }
    }

    #[test]
    fn one_shot_cuts_off_the_crowd_which_restarts_after_it() {
        let mut v = voices();
        v.play(CROWD);
        assert!(v.busy() && v.loop_wanted);
        v.play(WHISTLE);
        assert_eq!(v.current, WHISTLE);
        v.restart_loop();
        assert_eq!(v.current, WHISTLE, "no restart while the whistle plays");
        // Whistle = slot 0, 10 samples at 10 kHz = 20 samples at 20 kHz.
        let mut out = [0i16; 25];
        v.mix(&mut out, 20_000);
        assert!(!v.busy());
        assert_eq!(out[19], (200 - 128) << 7);
        assert_eq!(out[20], 0);
        v.ended();
        v.restart_loop();
        assert_eq!(v.current, CROWD);
        // The crowd loops endlessly.
        v.advance(1_000_000, 20_000);
        assert!(v.busy());
    }

    #[test]
    fn stopping_a_one_shot_keeps_the_loop_wish() {
        let mut v = voices();
        v.play(CROWD);
        v.play(GOAL);
        v.stop();
        assert!(v.loop_wanted);
        v.restart_loop();
        v.stop();
        assert!(!v.loop_wanted && !v.busy());
        v.restart_loop();
        assert!(!v.busy());
    }

    #[test]
    fn advance_ends_one_shots() {
        let mut v = voices();
        v.play(MISS);
        v.advance(9, 20_000);
        assert!(v.busy());
        v.advance(1, 20_000);
        assert!(!v.busy());
    }

    #[test]
    fn original_samples() {
        let Some(dir) = bmp_formats::data_dir() else {
            return;
        };
        let bytes = std::fs::read(dir.join("SOUND/DIGI.VOC")).unwrap();
        let mut v = Voices::from_digi(&bytes).unwrap();
        v.play(WHISTLE);
        // 6985 samples at 10 kHz = 0.6985 s.
        v.advance(6984, 10_000);
        assert!(v.busy());
        v.advance(1, 10_000);
        assert!(!v.busy());
    }
}
