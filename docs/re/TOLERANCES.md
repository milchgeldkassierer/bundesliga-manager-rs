# Graphics tolerances

Each rendered 320x200 frame is compared against a DOSBox reference capture of the same screen.

## Current acceptance threshold (phase 1)
- At least **90 %** of pixels must match the reference (a pixel matches when every 8-bit RGB channel differs by at most 4).
- Layout must be recognizably identical; remaining differences are tracked as follow-up issues and fixed later.

## Long-term target
- At most 0.5 % differing pixels, color differences only from VGA DAC rounding, geometry exact.

The reference implementation is `bmp compare <port.png> <reference.png> [--diff out.png] [--threshold 90]` (`crates/bmp/src/compare.rs`).
