//! The VGA path of BMINTRO.EXE's animation, one main-loop iteration at a time.
//!
//! BMINTRO is hand-written assembly without a C runtime. All of its state lives in
//! its single data segment (`DS = 0x00be`, see docs/re/logic/intro.md): counters,
//! the 214 cell records of the logo reveal, the edge masks and colours of the
//! gradient box and the two halves of the "SOFTWARE 2000" bitmap that the marquee
//! consumes bit by bit. The port keeps a mutable copy of that segment and runs the
//! original routines on it, so the frames are bit-identical however far the
//! animation has advanced.

use std::fmt;

/// Paragraph of the data segment relative to the load image (`mov ax, 0x00be`).
const DATA_SEGMENT: usize = 0x00be;
/// Bytes of the data segment present in the file.
const DATA_BYTES: usize = 0x18f8;

/// Mode 13h: 320x200, one byte per pixel.
pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 200;
/// The VGA window at `A000:0000` is 64 KiB; offsets wrap inside it.
const VRAM_BYTES: usize = 0x10000;

// Data-segment variables (word unless noted).
/// Non-zero once the logo reveal is complete (phase 2: gradient box).
const PHASE: usize = 0x0e;
/// Reveal steps taken so far (only feeds the EGA/Hercules cell data).
const STEP_COUNT: usize = 0x10;
/// Iterations since the last reveal step (a step every third iteration).
const DIVIDER: usize = 0x12;
/// Phase 1: the current reveal step; phase 2: the next gradient line.
const STEP: usize = 0x14;
/// Pointer to the next gradient colour byte.
const COLOR_PTR: usize = 0x16;
/// Pointer to the next gradient edge-mask word.
const EDGE_PTR: usize = 0x18;
/// Byte: the stage of the cell being drawn.
const STAGE: usize = 0x1a;
/// Byte: non-zero while the marquee runs.
const MARQUEE: usize = 0x1c;
/// Word table: VRAM offset / 4 of each gradient line.
const LINE_TABLE: usize = 0x305;
/// 214 cell records of 8 bytes: start step, cell index (row * 40 + column),
/// type (0 = solid, else a glyph) and stage.
const CELLS: usize = 0xac2;
const CELL_COUNT: usize = 0xd6;
/// 8x8 one-bit glyphs of the masked cell types (VGA path).
const GLYPHS: usize = 0x1292;
/// "SOFTWARE 2000": four 192-bit rows that enter from the left (odd screen rows)
/// interleaved with three that enter from the right (even rows), 0x30 bytes apart.
const MARQUEE_LEFT: usize = 0x12da;
const MARQUEE_RIGHT: usize = 0x12f2;

/// A cell reaches its final colour after this many stages (colours 2..18).
const CELL_STAGES: u16 = 0x11;
/// Solid cells drawn from this step on start the marquee.
const MARQUEE_START_STEP: u16 = 0x1e;
/// Lines of the gradient box.
const GRADIENT_LINES: u16 = 0x84;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntroError {
    NotAnExecutable,
    Truncated,
}

impl fmt::Display for IntroError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntroError::NotAnExecutable => write!(f, "BMINTRO.EXE: not an MZ executable"),
            IntroError::Truncated => write!(f, "BMINTRO.EXE: data segment truncated"),
        }
    }
}

impl std::error::Error for IntroError {}

/// Returns BMINTRO's initial data segment (`DS:0000..18f8`) from the executable.
pub fn data_segment(exe: &[u8]) -> Result<Vec<u8>, IntroError> {
    if exe.len() < 0x1c || &exe[..2] != b"MZ" {
        return Err(IntroError::NotAnExecutable);
    }
    let header = usize::from(u16::from_le_bytes([exe[8], exe[9]])) * 16;
    let start = header + DATA_SEGMENT * 16;
    exe.get(start..start + DATA_BYTES)
        .map(<[u8]>::to_vec)
        .ok_or(IntroError::Truncated)
}

/// The animation state: BMINTRO's data segment and the VGA memory it draws to.
pub struct Intro {
    ds: Vec<u8>,
    vram: Vec<u8>,
}

impl Intro {
    /// The state right after the mode switch (mode 13h clears the screen).
    pub fn new(ds: Vec<u8>) -> Self {
        Self {
            ds,
            vram: vec![0; VRAM_BYTES],
        }
    }

    /// The visible 320x200 pixels.
    pub fn screen(&self) -> &[u8] {
        &self.vram[..WIDTH * HEIGHT]
    }

    fn w(&self, at: usize) -> u16 {
        u16::from_le_bytes([self.ds[at], self.ds[at + 1]])
    }

    fn set_w(&mut self, at: usize, v: u16) {
        self.ds[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }

    fn put(&mut self, at: u16, color: u8) {
        self.vram[usize::from(at)] = color;
    }

    /// Whether the whole animation has run: the box is drawn and the marquee stopped.
    #[cfg(test)]
    pub fn finished(&self) -> bool {
        self.w(PHASE) != 0 && self.w(STEP) >= GRADIENT_LINES && self.ds[MARQUEE] == 0
    }

    /// One pass of the main loop after the retrace wait: the marquee (while it
    /// runs), then a reveal step or a gradient line. The key check that follows
    /// in the original is the caller's.
    ///
    /// Origin: BMINTRO.EXE 0000:000b..0028 (main loop of entry)
    pub fn iterate(&mut self) {
        if self.ds[MARQUEE] != 0 {
            self.marquee();
        }
        if self.w(PHASE) == 0 {
            self.reveal_step();
        } else {
            self.gradient_line();
        }
    }

    /// Every third call advances the reveal by one step: each cell whose start
    /// step has come and whose stage is below 17 is drawn in colour `stage + 2`
    /// and moves to the next stage. A step that draws nothing ends phase 1.
    ///
    /// Origin: BMINTRO.EXE 0000:0087 (VGA path 0000:0122)
    fn reveal_step(&mut self) {
        let divider = self.w(DIVIDER).wrapping_add(1);
        self.set_w(DIVIDER, divider);
        if divider <= 2 {
            return;
        }
        self.set_w(STEP_COUNT, self.w(STEP_COUNT).wrapping_add(1));
        self.set_w(DIVIDER, 0);
        let step = self.w(STEP).wrapping_add(1);
        self.set_w(STEP, step);
        let mut drawn = false;
        for i in 0..CELL_COUNT {
            let rec = CELLS + 8 * i;
            if self.w(rec) > step || self.w(rec + 6) >= CELL_STAGES {
                continue;
            }
            // `div cl` by 40: AL = row, AH = column.
            let pos = self.w(rec + 2);
            let (row, col) = ((pos / 40) as u8, (pos % 40) as u8);
            let stage = self.w(rec + 6);
            self.ds[STAGE] = stage as u8;
            match self.w(rec + 4) {
                0 => self.draw_solid_cell(row, col),
                kind => self.draw_glyph_cell(row, col, kind),
            }
            drawn = true;
            self.set_w(rec + 6, stage.wrapping_add(1));
        }
        if !drawn {
            self.set_w(PHASE, 0xffff);
            self.set_w(STEP, 0);
        }
    }

    fn cell_origin(row: u8, col: u8) -> u16 {
        u16::from(col)
            .wrapping_mul(8)
            .wrapping_add(u16::from(row).wrapping_mul(0xa00))
    }

    /// Fills an 8x8 cell with colour `stage + 2`; from step 30 on this also starts
    /// the marquee.
    ///
    /// Origin: BMINTRO.EXE 0000:0345 (VGA path 0000:0410)
    fn draw_solid_cell(&mut self, row: u8, col: u8) {
        if self.w(STEP) >= MARQUEE_START_STEP {
            self.ds[MARQUEE] = 0xff;
        }
        let color = self.ds[STAGE].wrapping_add(2);
        let origin = Self::cell_origin(row, col);
        for y in 0..8u16 {
            for x in 0..8u16 {
                self.put(origin.wrapping_add(y * 320 + x), color);
            }
        }
    }

    /// Draws the set bits of glyph `kind` (MSB = left pixel) in colour `stage + 2`.
    ///
    /// Origin: BMINTRO.EXE 0000:01a7 (VGA path 0000:0305)
    fn draw_glyph_cell(&mut self, row: u8, col: u8, kind: u16) {
        let color = self.ds[STAGE].wrapping_add(2);
        let origin = Self::cell_origin(row, col);
        let glyph = usize::from(GLYPHS as u16 + kind.wrapping_mul(8));
        for y in 0..8u16 {
            let bits = self.ds[glyph + usize::from(y)];
            for x in 0..8u16 {
                if bits & (0x80 >> x) != 0 {
                    self.put(origin.wrapping_add(y * 320 + x), color);
                }
            }
        }
    }

    /// Phase 2: draws the next line of the gradient box (lines 18..149, x 82..237)
    /// in the next colour; the first 8 and the last 9 lines take their two 8-pixel
    /// ends from an edge-mask word (rounded corners). After the last line the
    /// marquee stops.
    ///
    /// Origin: BMINTRO.EXE 0000:0484 (VGA path 0000:054d)
    fn gradient_line(&mut self) {
        let line = self.w(STEP);
        if line >= GRADIENT_LINES {
            self.ds[MARQUEE] = 0;
            return;
        }
        self.set_w(STEP, line + 1);
        let mut di = self.w(LINE_TABLE + usize::from(line) * 2).wrapping_shl(2);
        let mut mask = 0xffff_u16;
        if !(8..=0x7a).contains(&line) {
            let at = self.w(EDGE_PTR);
            mask = self.w(usize::from(at));
            self.set_w(EDGE_PTR, at.wrapping_add(2));
        }
        di = di.wrapping_add(0x16ca);
        let at = self.w(COLOR_PTR);
        let color = self.ds[usize::from(at)];
        self.set_w(COLOR_PTR, at.wrapping_add(1));
        // High byte: the right end (156 pixels further), low byte: the left end.
        for _ in 0..8 {
            if mask & 0x8000 != 0 {
                self.put(di.wrapping_add(0x9c), color);
            }
            mask = mask.rotate_left(1);
            di = di.wrapping_add(1);
        }
        for _ in 0..8 {
            if mask & 0x8000 != 0 {
                self.put(di, color);
            }
            mask = mask.rotate_left(1);
            di = di.wrapping_add(1);
        }
        for _ in 0..0x8c {
            self.put(di, color);
            di = di.wrapping_add(1);
        }
    }

    /// `rep movsw` of `count` words (the direction flag set = `backward`).
    fn movsw(&mut self, mut di: u16, mut si: u16, count: u16, backward: bool) {
        for _ in 0..count {
            let (lo, hi) = (
                self.vram[usize::from(si)],
                self.vram[usize::from(si.wrapping_add(1))],
            );
            self.put(di, lo);
            self.put(di.wrapping_add(1), hi);
            let step = if backward { 0xfffe } else { 2 };
            di = di.wrapping_add(step);
            si = si.wrapping_add(step);
        }
    }

    /// Moves screen rows 193, 195, 197 and 199 one pixel right and rows 194, 196
    /// and 198 one pixel left, then feeds the next bit of each "SOFTWARE 2000" row
    /// in at x = 3 (odd rows) or x = 316 (even rows) in colour 1.
    ///
    /// Origin: BMINTRO.EXE 0000:05c2 (VGA path 0000:09dc)
    fn marquee(&mut self) {
        for row in [193u16, 195, 197, 199] {
            let end = row * 320 + 318;
            self.movsw(end, end - 1, 0x9f, true);
        }
        for row in [194u16, 196, 198] {
            let start = row * 320;
            self.movsw(start, start + 1, 0x9f, false);
        }
        let mut si = MARQUEE_LEFT;
        let mut di = 193 * 320 + 2_u16;
        for _ in 0..4 {
            self.put(di + 1, 0);
            // `rcr` through twelve words, most significant word first.
            let mut carry = false;
            for k in 0..12 {
                let v = self.w(si + 2 * k);
                self.set_w(si + 2 * k, (v >> 1) | (u16::from(carry) << 15));
                carry = v & 1 != 0;
            }
            if carry {
                self.put(di + 1, 1);
            }
            di += 0x280;
            si += 0x30;
        }
        let mut si = MARQUEE_RIGHT;
        let mut di = 194 * 320 + 316_u16;
        for _ in 0..3 {
            self.put(di, 0);
            // `rcl` through twelve words, least significant word first.
            let mut carry = false;
            for k in (0..12).rev() {
                let v = self.w(si + 2 * k);
                self.set_w(si + 2 * k, (v << 1) | u16::from(carry));
                carry = v & 0x8000 != 0;
            }
            if carry {
                self.put(di, 1);
            }
            di += 0x280;
            si += 0x30;
        }
    }
}

/// Main-loop iterations until the animation is complete: 70 reveal steps of three
/// iterations, 132 gradient lines and the iteration that stops the marquee.
#[cfg(test)]
pub const ITERATIONS_TO_FINISH: u32 = 70 * 3 + 132 + 1;

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic data segment: one solid cell at row 1, column 2 from step 1 and
    /// one glyph-1 cell at row 0, column 0 from step 2; no gradient data.
    fn tiny() -> Vec<u8> {
        let mut ds = vec![0u8; DATA_BYTES];
        let rec = |ds: &mut Vec<u8>, i: usize, start: u16, pos: u16, kind: u16| {
            let r = CELLS + 8 * i;
            ds[r..r + 2].copy_from_slice(&start.to_le_bytes());
            ds[r + 2..r + 4].copy_from_slice(&pos.to_le_bytes());
            ds[r + 4..r + 6].copy_from_slice(&kind.to_le_bytes());
        };
        // Every other record never starts.
        for i in 0..CELL_COUNT {
            rec(&mut ds, i, 0xffff, 0, 0);
        }
        rec(&mut ds, 0, 1, 42, 0);
        rec(&mut ds, 1, 2, 0, 1);
        ds[GLYPHS + 8..GLYPHS + 16].copy_from_slice(&[0x80, 0, 0, 0, 0, 0, 0, 0x01]);
        ds
    }

    #[test]
    fn rejects_non_executables() {
        assert_eq!(data_segment(b"XX"), Err(IntroError::NotAnExecutable));
        let mut exe = vec![0u8; 0x40];
        exe[..2].copy_from_slice(b"MZ");
        exe[8] = 0x20;
        assert_eq!(data_segment(&exe), Err(IntroError::Truncated));
    }

    #[test]
    fn reveal_steps_every_third_iteration() {
        let mut intro = Intro::new(tiny());
        intro.iterate();
        intro.iterate();
        assert!(intro.screen().iter().all(|&p| p == 0));
        intro.iterate();
        // Step 1: the solid cell (row 1 = y 8, column 2 = x 16) in colour 2.
        assert_eq!(intro.screen()[8 * 320 + 16], 2);
        assert_eq!(intro.screen()[15 * 320 + 23], 2);
        assert_eq!(intro.screen()[8 * 320 + 24], 0);
        for _ in 0..3 {
            intro.iterate();
        }
        // Step 2: stage 1 of the solid cell, stage 0 of the glyph cell.
        assert_eq!(intro.screen()[8 * 320 + 16], 3);
        assert_eq!(intro.screen()[0], 2);
        assert_eq!(intro.screen()[7 * 320 + 7], 2);
        assert_eq!(intro.screen()[1], 0);
    }

    #[test]
    fn reveal_ends_when_a_step_draws_nothing() {
        let mut intro = Intro::new(tiny());
        // The glyph cell starts at step 2 and needs 17 stages: steps 2..18.
        for _ in 0..18 * 3 {
            intro.iterate();
        }
        assert_eq!(intro.w(PHASE), 0);
        assert_eq!(intro.screen()[0], 18);
        for _ in 0..3 {
            intro.iterate();
        }
        assert_eq!(intro.w(PHASE), 0xffff);
        assert_eq!(intro.w(STEP), 0);
        // The solid cells started before step 30: no marquee.
        assert_eq!(intro.ds[MARQUEE], 0);
    }

    #[test]
    fn gradient_lines_use_edge_masks_and_colours() {
        let mut ds = tiny();
        ds[PHASE] = 0xff;
        for line in 0..GRADIENT_LINES {
            let at = LINE_TABLE + 2 * usize::from(line);
            ds[at..at + 2].copy_from_slice(&(line * 80).to_le_bytes());
        }
        ds[COLOR_PTR..COLOR_PTR + 2].copy_from_slice(&0x0600_u16.to_le_bytes());
        ds[0x600] = 40;
        ds[EDGE_PTR..EDGE_PTR + 2].copy_from_slice(&0x0700_u16.to_le_bytes());
        // Right end: only its first pixel; left end: only its last pixel.
        ds[0x700..0x702].copy_from_slice(&0x8001_u16.to_le_bytes());
        ds[MARQUEE] = 0;
        let mut intro = Intro::new(ds);
        intro.iterate();
        let row = &intro.screen()[18 * 320..19 * 320];
        assert_eq!(row[81], 0);
        assert_eq!(row[82..89], [0; 7]);
        assert_eq!(row[89], 40);
        assert!(row[90..230].iter().all(|&p| p == 40));
        assert_eq!(row[230], 40);
        assert_eq!(row[231..238], [0; 7]);
        assert_eq!(intro.w(EDGE_PTR), 0x702);
        assert_eq!(intro.w(COLOR_PTR), 0x601);
    }

    #[test]
    fn marquee_feeds_bits_from_both_sides() {
        let mut ds = tiny();
        ds[MARQUEE] = 0xff;
        // Left row 0: lowest bit of the last word set; right row 0: top bit of the
        // first word set.
        ds[MARQUEE_LEFT + 22] = 0x01;
        ds[MARQUEE_RIGHT + 1] = 0x80;
        let mut intro = Intro::new(ds);
        intro.marquee();
        assert_eq!(intro.screen()[193 * 320 + 3], 1);
        assert_eq!(intro.screen()[194 * 320 + 316], 1);
        intro.marquee();
        assert_eq!(intro.screen()[193 * 320 + 3], 0);
        assert_eq!(intro.screen()[193 * 320 + 4], 1);
        assert_eq!(intro.screen()[194 * 320 + 316], 0);
        assert_eq!(intro.screen()[194 * 320 + 315], 1);
        // The bitmaps are consumed.
        assert!(
            intro.ds[MARQUEE_LEFT..MARQUEE_LEFT + 24]
                .iter()
                .all(|&b| b == 0)
        );
    }

    #[test]
    fn original_animation_finishes_on_schedule() {
        let Some(dir) = bmp_formats::data_dir() else {
            return;
        };
        let exe = std::fs::read(dir.join("BMINTRO.EXE")).expect("BMINTRO.EXE");
        let mut intro = Intro::new(data_segment(&exe).expect("data segment"));
        for _ in 0..ITERATIONS_TO_FINISH - 1 {
            intro.iterate();
            assert!(!intro.finished());
        }
        intro.iterate();
        assert!(intro.finished());
        let done = intro.screen().to_vec();
        intro.iterate();
        assert_eq!(intro.screen(), &done[..], "the final frame is static");
        // Gradient box corner and centre, the grey frame, the marquee text.
        assert_eq!(done[18 * 320 + 82], 0);
        assert_ne!(done[80 * 320 + 160], 0);
        assert!(done[193 * 320..].contains(&1));
    }
}
