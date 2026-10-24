//! Mixes the FM music and the digitised voice into one mono stream at the
//! output device's rate (the Sound Blaster mixes both on the card).

use crate::music::Music;
use crate::opl::NATIVE_RATE;
use crate::voice::Voices;

/// The sound card.
pub struct Mixer {
    pub music: Option<Music>,
    pub voices: Option<Voices>,
    rate: u32,
    /// Position between the last two native music samples, 0..1.
    frac: f64,
    prev: i16,
    next: i16,
    /// Native samples owed by [`Mixer::advance`] (fractional part).
    owed: f64,
}

impl Mixer {
    pub fn new(rate: u32) -> Self {
        Self {
            music: None,
            voices: None,
            rate: rate.max(1),
            frac: 0.0,
            prev: 0,
            next: 0,
            owed: 0.0,
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Fills `out` (mono samples at the device rate).
    pub fn fill(&mut self, out: &mut [i16]) {
        let step = NATIVE_RATE / f64::from(self.rate);
        match &mut self.music {
            Some(m) => {
                let mut one = [0i16; 1];
                for o in out.iter_mut() {
                    self.frac += step;
                    while self.frac >= 1.0 {
                        self.frac -= 1.0;
                        self.prev = self.next;
                        m.render(&mut one);
                        self.next = one[0];
                    }
                    let a = f64::from(self.prev);
                    let b = f64::from(self.next);
                    *o = (a + (b - a) * self.frac) as i16;
                }
            }
            None => out.fill(0),
        }
        if let Some(v) = &mut self.voices {
            v.mix(out, self.rate);
        }
    }

    /// Advances by `samples` device samples without producing sound.
    pub fn advance(&mut self, samples: u64) {
        if let Some(m) = &mut self.music {
            self.owed += samples as f64 * NATIVE_RATE / f64::from(self.rate);
            let whole = self.owed.floor();
            self.owed -= whole;
            m.advance(whole as u64);
        }
        if let Some(v) = &mut self.voices {
            v.advance(samples, self.rate);
        }
    }
}
