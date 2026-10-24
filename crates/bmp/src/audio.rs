//! The emulated Sound Blaster: a [`Mixer`] fed to an SDL audio device, or run
//! silently on the host clock when there is no device (headless runs, no
//! audio hardware). Also writes WAV files for offline renders.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use bmp_sound::mixer::Mixer;

/// Rate of the silent card and the preferred device rate.
pub const RATE: u32 = 48_000;

struct Feed {
    mixer: Arc<Mutex<Mixer>>,
}

impl sdl2::audio::AudioCallback for Feed {
    type Channel = i16;

    fn callback(&mut self, out: &mut [i16]) {
        match self.mixer.lock() {
            Ok(mut m) => m.fill(out),
            Err(_) => out.fill(0),
        }
    }
}

pub struct SoundCard {
    mixer: Arc<Mutex<Mixer>>,
    device: Option<sdl2::audio::AudioDevice<Feed>>,
    /// Device samples the silent card has advanced so far.
    advanced: u64,
}

impl SoundCard {
    /// A card whose output goes to the default SDL audio device.
    pub fn open(audio: &sdl2::AudioSubsystem) -> Result<Self, String> {
        let desired = sdl2::audio::AudioSpecDesired {
            freq: Some(RATE as i32),
            channels: Some(1),
            samples: Some(1024),
        };
        let mut shared = None;
        let device = audio.open_playback(None, &desired, |spec| {
            let mixer = Arc::new(Mutex::new(Mixer::new(spec.freq.max(1) as u32)));
            shared = Some(mixer.clone());
            Feed { mixer }
        })?;
        device.resume();
        Ok(Self {
            mixer: shared.ok_or("no audio mixer")?,
            device: Some(device),
            advanced: 0,
        })
    }

    /// A card without output, advanced by [`SoundCard::sync`].
    pub fn silent() -> Self {
        Self {
            mixer: Arc::new(Mutex::new(Mixer::new(RATE))),
            device: None,
            advanced: 0,
        }
    }

    pub fn mixer(&self) -> MutexGuard<'_, Mixer> {
        self.mixer.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Advances a silent card to `seconds` since it was created.
    pub fn sync(&mut self, seconds: f64) {
        if self.device.is_some() {
            return;
        }
        let mut m = self.mixer();
        let target = (seconds * f64::from(m.rate())) as u64;
        if target > self.advanced {
            m.advance(target - self.advanced);
            drop(m);
            self.advanced = target;
        }
    }
}

/// Writes 16-bit mono PCM as a WAV file.
pub fn write_wav(path: &Path, rate: u32, samples: &[i16]) -> std::io::Result<()> {
    let data = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + samples.len() * 2);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header() {
        let dir = std::env::temp_dir().join(format!("bmp-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.wav");
        write_wav(&path, 49716, &[1, -2]).unwrap();
        let b = std::fs::read(&path).unwrap();
        assert_eq!(b.len(), 48);
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(b[24..28].try_into().unwrap()), 49716);
        assert_eq!(&b[44..], &[1, 0, 0xfe, 0xff]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn silent_card_advances_on_sync() {
        let mut card = SoundCard::silent();
        card.sync(0.5);
        assert_eq!(card.advanced, 24_000);
        card.sync(0.25);
        assert_eq!(card.advanced, 24_000);
    }
}
