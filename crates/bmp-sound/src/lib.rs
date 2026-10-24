//! Sound for the port: a YM3812 (OPL2) emulator, the SBFMDRV-compatible CMF
//! player for the title music and the CT-VOICE-compatible digitised voices.
//! Everything here is independent of the audio backend; `crates/bmp` feeds the
//! rendered samples to SDL.

pub mod fmdrv;
pub mod mixer;
pub mod music;
pub mod opl;
pub mod voice;
