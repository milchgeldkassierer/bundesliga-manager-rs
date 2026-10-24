//! `TORE\<n>.<kind>` goal-scene ("Torszenen") animations and the `TORE\ANZAHL`
//! scene count file.
//!
//! Full specification: `docs/re/tore.md`.
//!
//! A scene file is not a picture format: it is a script of object positions. Every
//! frame stores a camera position and 27 objects (players, keepers, referee, ball and
//! the two goal frames), each with a pixel position and a sprite id into
//! `PIC\27.VGA`. The game composes each frame on top of the pitch picture
//! `PIC\26.VGA` (goal frames from `PIC\29.VGA`) and shows a 182x96 window of it.
//!
//! Address conventions:
//! - Ghidra addresses (`2000:502c`) refer to the Ghidra project, which holds the
//!   still EXEPACK-packed image of `BMMAIN.EXE` loaded at segment 0x1000. Code below
//!   image offset 0x35ad2 is stored verbatim there (Ghidra address = image offset +
//!   0x10000).
//! - "raw" addresses (`35ac:20c8`) are segment:offset pairs relative to the
//!   *unpacked* load image (as they appear in far calls); those routines are not
//!   present at that address in the Ghidra project.
//! - DGROUP is segment 0x4cb3 (image offset 0x4cb30); `DS:52be` etc. are offsets
//!   into it.

use std::fmt;

/// Signature at the start of every scene file (`DS:52be`, compared with `strcmp`
/// after reading 12 bytes).
pub const SIGNATURE: &[u8; 12] = b"BM-Ed1.3-WK\0";

/// Size of the fixed header: signature, four sound trigger frames, last frame index.
pub const HEADER_LEN: usize = 17;

/// Bytes per frame record (`mov cx, 0xa4` at Ghidra `2000:522d`).
pub const FRAME_LEN: usize = 164;

/// Objects per frame (`cmp byte [bp-0x16], 0x1b` at Ghidra `1000:c8fb`).
pub const OBJECTS_PER_FRAME: usize = 27;

/// Bytes per object record inside a frame.
pub const OBJECT_LEN: usize = 6;

/// Width of the window shown on screen (`mov ax, 0xb6` at Ghidra `1000:c971`).
pub const VIEW_WIDTH: u16 = 182;

/// Height of the window shown on screen (`mov ax, 0x60` at Ghidra `1000:c96d`).
pub const VIEW_HEIGHT: u16 = 96;

/// Screen x of the window (`mov ax, 0x45` at Ghidra `1000:c978`).
pub const VIEW_SCREEN_X: i16 = 0x45;

/// Row in the pitch picture where object row 0 lies, and where the restored
/// background strip starts (`add cx, 0x23` at Ghidra `1000:c8a2`).
pub const PITCH_TOP: i16 = 0x23;

/// Height of the background strip that is restored every frame (`0x4d`).
pub const STRIP_HEIGHT: u16 = 0x4d;

/// Width of a sprite cell in `PIC\27.VGA`.
pub const CELL_WIDTH: u16 = 12;

/// Height of a sprite cell in `PIC\27.VGA`.
pub const CELL_HEIGHT: u16 = 11;

/// Sprite cells per row in `PIC\27.VGA`.
pub const CELLS_PER_ROW: u16 = 24;

/// First sprite id drawn as the 4 pixel wide ball (`cmp si, 0x8e` at `1000:c86c`).
pub const BALL_FIRST_ID: u16 = 0x8e;

/// Sprite id of the left goal frame.
pub const LEFT_GOAL_ID: u16 = 1000;

/// PIC ids used by the scene player (loaded at Ghidra `2000:4c39`).
pub const PIC_PITCH: u8 = 26;
/// Sprite sheet with all player / referee / ball cells.
pub const PIC_SPRITES: u8 = 27;
/// The two goal frames.
pub const PIC_GOALS: u8 = 29;

/// Frame period in BIOS timer ticks (18.2 Hz) for the first showing.
pub const TICKS_NORMAL: u16 = 2;
/// Frame period in BIOS timer ticks for replays (slow motion).
pub const TICKS_REPLAY: u16 = 5;
/// Number of frames before the end at which the first replay starts.
pub const REPLAY_TAIL_FRAMES: i16 = 0x12;
/// Extra BIOS ticks waited after frame 0 (`push 0xa; call timer_start` at 1000:c9e1).
pub const FIRST_FRAME_HOLD_TICKS: u16 = 10;

/// Errors produced while parsing scene or `ANZAHL` files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToreError {
    /// The file ended before the named part could be read.
    Truncated {
        what: &'static str,
        needed: usize,
        available: usize,
    },
    /// The first 12 bytes do not compare equal to [`SIGNATURE`] (`strcmp`).
    BadSignature,
    /// `ANZAHL` yields a maximum scene number or "E" count below 1 (the game aborts
    /// with fatal error 2).
    BadCounts,
    /// A signed length byte makes the game's `read()` result check fail even
    /// though enough bytes may be present: a last-frame index in `0x80..=0xfe`
    /// (negative byte count after `cbw`) or an author length `>= 0x80`.
    NegativeLength { what: &'static str, value: u8 },
}

impl fmt::Display for ToreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToreError::Truncated {
                what,
                needed,
                available,
            } => write!(f, "truncated {what}: need {needed} bytes, have {available}"),
            ToreError::BadSignature => write!(f, "missing BM-Ed1.3-WK signature"),
            ToreError::BadCounts => write!(f, "ANZAHL counts below 1"),
            ToreError::NegativeLength { what, value } => {
                write!(f, "{what} byte {value:#04x} is negative as a signed char")
            }
        }
    }
}

impl std::error::Error for ToreError {}

/// One object of a frame (6 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SceneObject {
    /// Pixel x in the pitch picture (u16 LE).
    pub x: u16,
    /// Pixel y relative to [`PITCH_TOP`] (only the low byte is read).
    pub y: u8,
    /// High byte of the y word, never read by the player.
    pub y_high: u8,
    /// Sprite id: `< 1000` is a cell of `PIC\27.VGA`, `1000` the left goal frame,
    /// anything above 1000 the right goal frame.
    pub sprite: u16,
}

/// One frame (164 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Left edge of the visible window in the pitch picture.
    pub camera_x: u8,
    /// Top edge of the visible window in the pitch picture.
    pub camera_y: u8,
    /// The objects in drawing order (the editor sorts them by y).
    pub objects: [SceneObject; OBJECTS_PER_FRAME],
}

impl Frame {
    fn parse(b: &[u8]) -> Frame {
        let mut objects = [SceneObject::default(); OBJECTS_PER_FRAME];
        for (j, o) in objects.iter_mut().enumerate() {
            let r = &b[2 + j * OBJECT_LEN..2 + (j + 1) * OBJECT_LEN];
            *o = SceneObject {
                x: u16::from_le_bytes([r[0], r[1]]),
                y: r[2],
                y_high: r[3],
                sprite: u16::from_le_bytes([r[4], r[5]]),
            };
        }
        Frame {
            camera_x: b[0],
            camera_y: b[1],
            objects,
        }
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.push(self.camera_x);
        out.push(self.camera_y);
        for o in &self.objects {
            out.extend_from_slice(&o.x.to_le_bytes());
            out.push(o.y);
            out.push(o.y_high);
            out.extend_from_slice(&o.sprite.to_le_bytes());
        }
    }
}

/// A parsed scene file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scene {
    /// Frame indices at which sound effects 2, 3, 4 and 5 start (`DS:52cc..52cf`);
    /// compared as signed bytes, so 0xff never triggers.
    pub sound_frames: [u8; 4],
    /// Index of the last frame (`DS:52ca`); the file holds `last_frame + 1` frames.
    pub last_frame: u8,
    /// All frames, `last_frame + 1` of them.
    pub frames: Vec<Frame>,
    /// Length-prefixed text read into `4238:4bd8`: the scene author's name,
    /// NUL-terminated, in the game's character set (`]` = U-umlaut).
    pub author: Vec<u8>,
    /// Bytes after the author block; ignored by the game (only `3.T` / `3.V` have
    /// two of them).
    pub trailing: Vec<u8>,
}

fn take<'a>(
    data: &'a [u8],
    pos: &mut usize,
    n: usize,
    what: &'static str,
) -> Result<&'a [u8], ToreError> {
    let available = data.len().saturating_sub(*pos);
    if available < n {
        return Err(ToreError::Truncated {
            what,
            needed: n,
            available,
        });
    }
    let s = &data[*pos..*pos + n];
    *pos += n;
    Ok(s)
}

/// Parses a scene file with the same sequence of `read()` calls as the game:
/// 12 bytes signature, 4 bytes sound frames, 1 byte last frame index,
/// `(last_frame + 1) * 164` bytes of frames, 1 byte author length, author bytes.
/// Every short read makes the game reject the file.
///
/// Origin: BMMAIN.EXE 2000:502c (tore_load_and_play_scene)
pub fn parse_scene(data: &[u8]) -> Result<Scene, ToreError> {
    let mut pos = 0;
    let sig = take(data, &mut pos, 12, "signature")?;
    // strcmp: equal up to and including the first NUL of the reference string.
    if sig != SIGNATURE {
        return Err(ToreError::BadSignature);
    }
    let s = take(data, &mut pos, 4, "sound frames")?;
    let sound_frames = [s[0], s[1], s[2], s[3]];
    let last_frame = take(data, &mut pos, 1, "frame count")?[0];
    // 2000:5259: AX = (cbw last + 1) * 0xa4 (16-bit imul), DX = 0, then
    // read(fd, buf, AX) and the `int` result is sign-extended (cwd) and compared
    // with DX:AX. For last in 0x80..=0xfe the requested count is negative as an
    // i16 (-127*164..=-164), so cwd never reproduces DX = 0 and the game takes
    // the read-error path (2000:528c -> 2000:51eb). last = 0xff requests 0 bytes
    // and is accepted with zero frames.
    let signed_count = i16::from(last_frame as i8) + 1;
    if signed_count < 0 {
        return Err(ToreError::NegativeLength {
            what: "last frame index",
            value: last_frame,
        });
    }
    let n_frames = signed_count as usize;
    let raw = take(data, &mut pos, n_frames * FRAME_LEN, "frames")?;
    let frames = raw
        .as_chunks::<FRAME_LEN>()
        .0
        .iter()
        .map(|c| Frame::parse(c))
        .collect();
    let author_len = take(data, &mut pos, 1, "author length")?[0];
    // 2000:52aa: the length is sign-extended (cbw) both for the read() count and
    // for the comparison at 2000:52ce. For lengths >= 0x80 the count is a huge
    // unsigned value; read() returns the (small, non-negative) number of bytes
    // left, which never equals the negative cbw value, so the game fails.
    if author_len >= 0x80 {
        return Err(ToreError::NegativeLength {
            what: "author length",
            value: author_len,
        });
    }
    let author = take(data, &mut pos, usize::from(author_len), "author")?.to_vec();
    let trailing = data[pos..].to_vec();
    Ok(Scene {
        sound_frames,
        last_frame,
        frames,
        author,
        trailing,
    })
}

impl Scene {
    /// Serializes the scene back into the file layout (byte-identical for every
    /// shipped file).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.frames.len() * FRAME_LEN + 16);
        out.extend_from_slice(SIGNATURE);
        out.extend_from_slice(&self.sound_frames);
        out.push(self.last_frame);
        for f in &self.frames {
            f.write(&mut out);
        }
        out.push(self.author.len() as u8);
        out.extend_from_slice(&self.author);
        out.extend_from_slice(&self.trailing);
        out
    }

    /// Author name without the terminating NUL.
    pub fn author_text(&self) -> &[u8] {
        let end = self
            .author
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.author.len());
        &self.author[..end]
    }

    /// Sound effects (numbers 2..=5) the game starts when it reaches `frame`.
    /// Several may match; the game starts them in order 2, 3, 4, 5, each call
    /// replacing the previous one.
    ///
    /// Origin: BMMAIN.EXE 1000:ca2e (tore_play_frames)
    pub fn sounds_at(&self, frame: u8) -> Vec<u8> {
        let mut v = Vec::new();
        for (k, &f) in self.sound_frames.iter().enumerate() {
            // mov al, [52cc+k]; cbw; cmp ax, cx  (cx = frame, zero extended)
            if i16::from(f as i8) == i16::from(frame) {
                v.push(k as u8 + 2);
            }
        }
        v
    }
}

/// Mirrors a scene horizontally and swaps the team colours, exactly as the game
/// does after loading when the other team is the scoring one.
///
/// For every object of every frame (object-major order):
/// `x = 0x140 - x - 2 * (7 - 4 * is_ball)` (16-bit wrapping), then
/// `x > 1000 -> 0`, `x > 0x134 && !ball -> 0x134`, `x > 0x13c -> 0x13c`, then the
/// sprite id is remapped (see `docs/re/tore.md`). Afterwards every frame's camera
/// x becomes `clamp(0x89 - camera_x, 0, 0x89)`.
///
/// When `mirror` is false only the clamps are applied and the ids stay as they are,
/// as in the original.
///
/// Origin: BMMAIN.EXE 2000:4a93 (tore_mirror_scene)
pub fn mirror_scene(scene: &mut Scene, mirror: bool) {
    // Constants at DS:52d6..52ee.
    const A_LAST: i16 = 0x3a; // 52d6
    const B_FIRST: i16 = 0x3b; // 52d8
    const B_LAST: i16 = 0x75; // 52da
    const C_FIRST: i16 = 0x76; // 52dc
    const C_LAST: i16 = 0x8b; // 52de
    const A_KEEP_LO: i16 = 0x19; // 52e8
    const A_KEEP_HI: i16 = 0x21; // 52ea
    const B_KEEP_LO: i16 = 0x54; // 52ec
    const B_KEEP_HI: i16 = 0x5c; // 52ee

    let count = i16::from(scene.last_frame as i8);
    for j in 0..OBJECTS_PER_FRAME {
        let mut si: i16 = 0;
        while count >= si {
            let Some(frame) = scene.frames.get_mut(si as usize) else {
                break;
            };
            let o = &mut frame.objects[j];
            let id = o.sprite as i16;
            let is_ball: u16 = if id > 0x8d { 1 } else { 0 };
            let off = 7u16.wrapping_sub(is_ball << 2);
            if mirror {
                // mul cx (0xfffe): low word of off * 0xfffe, then - x + 0x140.
                o.x = off
                    .wrapping_mul(0xfffe)
                    .wrapping_sub(o.x)
                    .wrapping_add(0x140);
            }
            if o.x > 1000 {
                o.x = 0;
            }
            if o.x > 0x134 && id < 0x8e {
                o.x = 0x134;
            }
            if o.x > 0x13c {
                o.x = 0x13c;
            }
            if id < 0x8e {
                let new = if id < B_FIRST && mirror {
                    if (A_KEEP_LO..=A_KEEP_HI).contains(&id) {
                        Some(id.wrapping_add(B_FIRST))
                    } else {
                        Some(B_LAST.wrapping_sub(id))
                    }
                } else if id > A_LAST && id <= B_LAST && mirror {
                    if (B_KEEP_LO..=B_KEEP_HI).contains(&id) {
                        Some(id.wrapping_sub(B_FIRST))
                    } else {
                        Some(A_LAST.wrapping_sub(id.wrapping_sub(B_FIRST)))
                    }
                } else if mirror {
                    Some(C_FIRST.wrapping_sub(id).wrapping_add(C_LAST))
                } else {
                    None
                };
                if let Some(n) = new {
                    o.sprite = n as u16;
                }
            }
            si += 1;
        }
    }
    let mut si: i16 = 0;
    while count >= si {
        let Some(frame) = scene.frames.get_mut(si as usize) else {
            break;
        };
        let v = (0x89i16 - i16::from(frame.camera_x)).clamp(0, 0x89);
        if mirror {
            frame.camera_x = v as u8;
        }
        si += 1;
    }
}

/// Contents of `TORE\ANZAHL` (`MAXSZENE:43|5|2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anzahl {
    /// Highest plain scene number (`DS:57ce` byte); plain scenes are `2..=max_scene`.
    pub max_scene: u8,
    /// Highest "E" scene number (32-bit at `304c`); E scenes are `2..=e_count`.
    pub e_count: i32,
    /// Highest "J" scene number (32-bit at `4bd4`); J scenes are `2..=j_count`.
    pub j_count: i32,
}

/// Parses `ANZAHL` the way the game does: read up to 250 bytes, walk backwards from
/// the last byte to the last digit, then read three decimal numbers right to left,
/// each separated by exactly one non-digit byte. The rightmost number is the J
/// count, the middle one the E count, the leftmost (low byte) the maximum scene.
/// The index is an 8-bit variable and wraps; bytes outside the read data count as
/// non-digits here (the original reads uninitialised stack).
///
/// Origin: BMMAIN.EXE 2000:4e59 (tore_read_anzahl)
pub fn parse_anzahl(data: &[u8]) -> Result<Anzahl, ToreError> {
    let data = &data[..data.len().min(250)];
    // read() returns the count in AX; only AL is kept.
    let len = data.len() as u8;
    let at = |i: u8| -> i8 { data.get(usize::from(i)).copied().unwrap_or(0) as i8 };
    let mut pos = len.wrapping_sub(1);
    // Signed char compares (jl / jle).
    let mut guard = 0u32;
    while !(at(pos) >= 0x30 && at(pos) <= 0x39) {
        pos = pos.wrapping_sub(1);
        guard += 1;
        if guard > 256 {
            return Err(ToreError::BadCounts);
        }
    }
    let mut values = [0i32; 3];
    for v in values.iter_mut() {
        let mut acc: i32 = 0;
        let mut mul: i32 = 1;
        while at(pos) >= 0x30 {
            if at(pos) > 0x39 {
                break;
            }
            acc = acc.wrapping_add(i32::from(at(pos) - 0x30).wrapping_mul(mul));
            mul = mul.wrapping_mul(10);
            pos = pos.wrapping_sub(1);
        }
        *v = acc;
        pos = pos.wrapping_sub(1);
    }
    let a = Anzahl {
        j_count: values[0],
        e_count: values[1],
        max_scene: values[2] as u8,
    };
    if (a.max_scene as i8) < 1 || a.e_count < 1 {
        return Err(ToreError::BadCounts);
    }
    Ok(a)
}

/// Which file family a scene comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// `N.T` / `N.V`, `N` in `2..=max_scene`.
    Plain,
    /// `N.TE` / `N.VE`, `N` in `2..=e_count`.
    E,
    /// `N.TJ` / `N.VJ`, `N` in `2..=j_count`.
    J,
}

/// The file chosen by [`select_scene`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneChoice {
    /// Scene number.
    pub number: i16,
    /// File family.
    pub variant: Variant,
    /// `true` for `.T*` (goal), `false` for `.V*` (missed chance).
    pub goal: bool,
    /// The "E" flag after the call; the game returns it to the caller.
    pub force_e: bool,
}

impl SceneChoice {
    /// File name inside the `TORE` directory, upper case as shipped
    /// (the game builds `tore\` + number + `.t`/`.v` + `e`/`j`).
    pub fn file_name(&self) -> String {
        let ext = if self.goal { "T" } else { "V" };
        let suffix = match self.variant {
            Variant::Plain => "",
            Variant::E => "E",
            Variant::J => "J",
        };
        format!("{}.{}{}", self.number, ext, suffix)
    }
}

/// `random(lo, hi)` of the game: `lo + rand() % (hi - lo + 1)` with 16-bit signed
/// `idiv`. `rand` is the next value of the C runtime `rand()` (0..=0x7fff).
///
/// Origin: BMMAIN.EXE 1000:8377 (FUN_1000_8377)
pub fn random_range(rand: i16, lo: i16, hi: i16) -> i16 {
    let span = hi.wrapping_sub(lo).wrapping_add(1);
    if span == 0 {
        return lo;
    }
    rand.wrapping_rem(span).wrapping_add(lo)
}

/// Picks the scene file for one attempt of the game's selection loop.
///
/// `goal` selects `.T` (caller argument non-zero) or `.V`; `force_e` is the caller's
/// "E" argument, `j_flag` the function-local J flag which persists across retries of
/// the same call (start with `false`). `rand` yields successive C runtime `rand()`
/// values. On a failed `open()` the game simply calls this again (same `force_e`,
/// same `j_flag`).
///
/// Order of `rand()` calls: scene number in `2..=max_scene`; then
/// `random(0, goal ? 15 : 25) == 0` sets the E flag; then either an E number in
/// `2..=e_count`, or `random(0, 400) == 0` sets the J flag and a J number in
/// `2..=j_count` is drawn.
///
/// Origin: BMMAIN.EXE 2000:5064 (tore_load_and_play_scene)
pub fn select_scene(
    counts: &Anzahl,
    goal: bool,
    mut force_e: bool,
    j_flag: &mut bool,
    rand: &mut dyn FnMut() -> i16,
) -> SceneChoice {
    let threshold: i16 = if goal { 0xf } else { 0x19 };
    let mut number = random_range(rand(), 2, i16::from(counts.max_scene as i8));
    if random_range(rand(), 0, threshold) == 0 {
        force_e = true;
    }
    if force_e {
        // push word es:[304c] (low word of the 32-bit count)
        number = random_range(rand(), 2, counts.e_count as i16);
    } else if random_range(rand(), 0, 400) == 0 {
        *j_flag = true;
        number = random_range(rand(), 2, counts.j_count as i16);
    }
    let variant = if *j_flag {
        Variant::J
    } else if force_e {
        Variant::E
    } else {
        Variant::Plain
    };
    SceneChoice {
        number,
        variant,
        goal,
        force_e,
    }
}

/// An 8-bit indexed bitmap; the game's sprite object `{u16 w; u16 h; u8 far *data}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u8>,
}

impl Bitmap {
    /// A bitmap filled with colour 0.
    pub fn new(width: u16, height: u16) -> Bitmap {
        Bitmap {
            width,
            height,
            pixels: vec![0; usize::from(width) * usize::from(height)],
        }
    }

    /// Wraps decoded pixels; `pixels.len()` must be `width * height`.
    pub fn from_pixels(width: u16, height: u16, pixels: Vec<u8>) -> Bitmap {
        assert_eq!(pixels.len(), usize::from(width) * usize::from(height));
        Bitmap {
            width,
            height,
            pixels,
        }
    }
}

/// Clips a copy rectangle exactly like the game's blitters (16-bit signed).
/// Returns `None` when nothing is drawn.
fn clip(
    src: (i16, i16),
    s: (i16, i16),
    dst: (i16, i16),
    d: (i16, i16),
    size: (i16, i16),
) -> Option<(i16, i16, i16, i16, i16, i16)> {
    let (src_w, src_h) = src;
    let (dst_w, dst_h) = dst;
    let (mut sx, mut sy) = s;
    let (mut dx, mut dy) = d;
    let (mut w, mut h) = size;
    if sx < 0 {
        w = w.wrapping_add(sx);
        dx = dx.wrapping_sub(sx);
        sx = 0;
        if w <= 0 {
            return None;
        }
    }
    if sy < 0 {
        h = h.wrapping_add(sy);
        dy = dy.wrapping_sub(sy);
        sy = 0;
        if h <= 0 {
            return None;
        }
    }
    if dx < 0 {
        w = w.wrapping_add(dx);
        sx = sx.wrapping_sub(dx);
        dx = 0;
        if w <= 0 {
            return None;
        }
    }
    if dy < 0 {
        h = h.wrapping_add(dy);
        sy = sy.wrapping_sub(dy);
        dy = 0;
        if h <= 0 {
            return None;
        }
    }
    if sx >= src_w || sy >= src_h || dx >= dst_w || dy >= dst_h {
        return None;
    }
    if dst_w < dx.wrapping_add(w) {
        w = w.wrapping_add(dst_w.wrapping_sub(dx.wrapping_add(w)));
    }
    if dst_h < dy.wrapping_add(h) {
        h = h.wrapping_add(dst_h.wrapping_sub(dy.wrapping_add(h)));
    }
    if src_w < sx.wrapping_add(w) {
        w = w.wrapping_add(src_w.wrapping_sub(sx.wrapping_add(w)));
    }
    if src_h < sy.wrapping_add(h) {
        h = h.wrapping_add(src_h.wrapping_sub(sy.wrapping_add(h)));
    }
    if w <= 0 || h <= 0 {
        return None;
    }
    Some((sx, sy, dx, dy, w, h))
}

#[allow(clippy::too_many_arguments)]
fn blit(
    src: &Bitmap,
    sx: i16,
    sy: i16,
    dst: &mut Bitmap,
    dx: i16,
    dy: i16,
    w: i16,
    h: i16,
    transparent: bool,
) {
    let Some((sx, sy, dx, dy, w, h)) = clip(
        (src.width as i16, src.height as i16),
        (sx, sy),
        (dst.width as i16, dst.height as i16),
        (dx, dy),
        (w, h),
    ) else {
        return;
    };
    let (sw, dw) = (usize::from(src.width), usize::from(dst.width));
    for row in 0..h as usize {
        let s0 = (sy as usize + row) * sw + sx as usize;
        let d0 = (dy as usize + row) * dw + dx as usize;
        for col in 0..w as usize {
            let p = src.pixels[s0 + col];
            if !transparent || p != 0 {
                dst.pixels[d0 + col] = p;
            }
        }
    }
}

/// Opaque rectangle copy with the game's clipping.
///
/// Origin: BMMAIN.EXE raw 3930:0991 wrapper -> raw 35ac:1d1c core (no Ghidra
/// function: code lies in the EXEPACK-compressed part; unpacked image 0x39c91 / 0x377dc)
#[allow(clippy::too_many_arguments)]
pub fn blit_opaque(
    src: &Bitmap,
    sx: i16,
    sy: i16,
    dst: &mut Bitmap,
    dx: i16,
    dy: i16,
    w: i16,
    h: i16,
) {
    blit(src, sx, sy, dst, dx, dy, w, h, false);
}

/// Rectangle copy that skips colour 0, with the game's clipping.
///
/// Origin: BMMAIN.EXE raw 3930:0aa5 wrapper -> raw 35ac:20c8 core (no Ghidra
/// function: code lies in the EXEPACK-compressed part; unpacked image 0x39da5 / 0x37b88)
#[allow(clippy::too_many_arguments)]
pub fn blit_transparent(
    src: &Bitmap,
    sx: i16,
    sy: i16,
    dst: &mut Bitmap,
    dx: i16,
    dy: i16,
    w: i16,
    h: i16,
) {
    blit(src, sx, sy, dst, dx, dy, w, h, true);
}

/// Frame composer: owns the working copy of the pitch picture and the saved
/// background strip.
#[derive(Debug, Clone)]
pub struct ScenePlayer {
    /// Working canvas: `PIC\26.VGA` (320x112) that the objects are drawn into.
    pub canvas: Bitmap,
    /// Rows `0x23..0x23+0x4d` of the untouched pitch picture.
    pub strip: Bitmap,
    /// `PIC\27.VGA`, 24x7 cells of 12x11 pixels.
    pub sprites: Bitmap,
    /// `PIC\29.VGA`, the two goal frames.
    pub goals: Bitmap,
}

impl ScenePlayer {
    /// Sets up the buffers: the pitch picture becomes the canvas and a 320x77 copy
    /// of its rows 35..112 is saved as the background strip.
    ///
    /// Origin: BMMAIN.EXE 2000:4c39 (tore_load_scene_graphics)
    pub fn new(pitch: Bitmap, sprites: Bitmap, goals: Bitmap) -> ScenePlayer {
        let mut strip = Bitmap::new(0x140, STRIP_HEIGHT);
        blit_opaque(&pitch, 0, PITCH_TOP, &mut strip, 0, 0, 0x140, 0x4d);
        ScenePlayer {
            canvas: pitch,
            strip,
            sprites,
            goals,
        }
    }

    /// Draws one frame into [`ScenePlayer::canvas`]: restore the strip, then draw the
    /// 27 objects in file order.
    ///
    /// Origin: BMMAIN.EXE 1000:cab2..1000:c965 (tore_play_frames)
    pub fn draw_frame(&mut self, frame: &Frame) {
        blit_opaque(
            &self.strip,
            0,
            0,
            &mut self.canvas,
            0,
            PITCH_TOP,
            0x140,
            0x4d,
        );
        for o in &frame.objects {
            let si = o.sprite;
            if si > 999 {
                if si == LEFT_GOAL_ID {
                    blit_transparent(&self.goals, 0, 0, &mut self.canvas, 0, 0x36, 0x18, 0x14);
                } else {
                    blit_transparent(
                        &self.goals,
                        0x18,
                        0,
                        &mut self.canvas,
                        0x128,
                        0x36,
                        0x17,
                        0x14,
                    );
                }
                continue;
            }
            // Ball cells are drawn 4 pixels wide starting 4 pixels into the cell.
            let (x_off, width) = if si >= BALL_FIRST_ID {
                (4u16, 4u16)
            } else {
                (0, CELL_WIDTH)
            };
            let sx = (si % CELLS_PER_ROW)
                .wrapping_mul(CELL_WIDTH)
                .wrapping_add(x_off);
            let sy = (si / CELLS_PER_ROW).wrapping_mul(CELL_HEIGHT);
            blit_transparent(
                &self.sprites,
                sx as i16,
                sy as i16,
                &mut self.canvas,
                o.x as i16,
                (u16::from(o.y) + PITCH_TOP as u16) as i16,
                width as i16,
                CELL_HEIGHT as i16,
            );
        }
    }

    /// Copies the visible 182x96 window at the frame's camera position into
    /// `screen` at (`0x45`, `screen_y`). The game uses `screen_y` = 5 or 118
    /// (`((3 - slot) / 2) * 113 + 5`, caller at Ghidra `2000:b223`).
    ///
    /// Origin: BMMAIN.EXE 1000:c965 (tore_play_frames)
    pub fn present(&self, frame: &Frame, screen: &mut Bitmap, screen_y: i16) {
        blit_opaque(
            &self.canvas,
            i16::from(frame.camera_x),
            i16::from(frame.camera_y),
            screen,
            VIEW_SCREEN_X,
            screen_y,
            VIEW_WIDTH as i16,
            VIEW_HEIGHT as i16,
        );
    }

    /// Convenience: draw `frame` and return just the 182x96 window.
    pub fn render_view(&mut self, frame: &Frame) -> Bitmap {
        self.draw_frame(frame);
        let mut view = Bitmap::new(VIEW_WIDTH, VIEW_HEIGHT);
        blit_opaque(
            &self.canvas,
            i16::from(frame.camera_x),
            i16::from(frame.camera_y),
            &mut view,
            0,
            0,
            VIEW_WIDTH as i16,
            VIEW_HEIGHT as i16,
        );
        view
    }

    /// Convenience: draw `frame` and return a 320x200 frame with the window at
    /// (`0x45`, `screen_y`) on colour 0.
    pub fn render_screen(&mut self, frame: &Frame, screen_y: i16) -> Bitmap {
        self.draw_frame(frame);
        let mut screen = Bitmap::new(320, 200);
        self.present(frame, &mut screen, screen_y);
        screen
    }
}

/// Timing of one showing of a scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Playback {
    /// First frame shown.
    pub first_frame: u8,
    /// Frame period in BIOS ticks (18.2065 Hz).
    pub ticks_per_frame: u16,
    /// Extra pause in ticks after frame 0. The game tests only the frame
    /// index (`[bp-0xa] == 0` at 1000:c9db), so the hold applies to every showing
    /// that starts at frame 0: the first showing, the full slow-motion replay, and
    /// the tail replay when `last_frame < 18`.
    pub first_frame_hold: u16,
}

/// Playback parameters for showing number `pass` (0 = first, 1 = first replay,
/// 2 = second replay; the caller allows replays while its counter is below 3).
///
/// Origin: BMMAIN.EXE 1000:c7c4 (tore_play_frames)
pub fn playback(scene: &Scene, pass: u8) -> Playback {
    let mut first: i16 = 0;
    if pass == 1 {
        first = i16::from(scene.last_frame as i8) - REPLAY_TAIL_FRAMES;
    }
    if first < 0 {
        first = 0;
    }
    Playback {
        first_frame: first as u8,
        ticks_per_frame: if pass != 0 {
            TICKS_REPLAY
        } else {
            TICKS_NORMAL
        },
        first_frame_hold: if first == 0 {
            FIRST_FRAME_HOLD_TICKS
        } else {
            0
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic(n_frames: usize) -> Vec<u8> {
        let mut v = SIGNATURE.to_vec();
        v.extend_from_slice(&[3, 0xff, 0xff, 0xff]);
        v.push((n_frames - 1) as u8);
        for i in 0..n_frames {
            v.push(10 + i as u8);
            v.push(5);
            for j in 0..OBJECTS_PER_FRAME {
                v.extend_from_slice(&(j as u16 * 10).to_le_bytes());
                v.push(j as u8);
                v.push(0);
                v.extend_from_slice(&(j as u16).to_le_bytes());
            }
        }
        v.push(4);
        v.extend_from_slice(b"ABC\0");
        v
    }

    #[test]
    fn parse_synthetic_roundtrip() {
        let data = synthetic(3);
        let s = parse_scene(&data).unwrap();
        assert_eq!(s.frames.len(), 3);
        assert_eq!(s.last_frame, 2);
        assert_eq!(s.frames[1].camera_x, 11);
        assert_eq!(s.frames[2].objects[5].x, 50);
        assert_eq!(s.author_text(), b"ABC");
        assert_eq!(s.sounds_at(3), vec![2]);
        assert!(s.sounds_at(0xff).is_empty());
        assert_eq!(s.to_bytes(), data);
    }

    #[test]
    fn parse_rejects_bad_input() {
        let mut data = synthetic(2);
        data[0] = b'X';
        assert_eq!(parse_scene(&data), Err(ToreError::BadSignature));
        let data = synthetic(2);
        assert!(matches!(
            parse_scene(&data[..100]),
            Err(ToreError::Truncated { what: "frames", .. })
        ));
    }

    #[test]
    fn parse_signed_length_bytes() {
        // last = 0xff: cbw -1, +1 = 0 bytes requested, accepted with no frames.
        let mut v = SIGNATURE.to_vec();
        v.extend_from_slice(&[0xff; 4]);
        v.push(0xff);
        v.push(0);
        let s = parse_scene(&v).unwrap();
        assert!(s.frames.is_empty());
        // last = 0xfe: negative count, the game reports a read error.
        v[16] = 0xfe;
        assert_eq!(
            parse_scene(&v),
            Err(ToreError::NegativeLength {
                what: "last frame index",
                value: 0xfe
            })
        );
        // Author length >= 0x80 fails even with enough bytes present.
        let mut data = synthetic(1);
        let len_pos = HEADER_LEN + FRAME_LEN;
        data[len_pos] = 0x80;
        data.extend_from_slice(&[b'A'; 0x80]);
        assert_eq!(
            parse_scene(&data),
            Err(ToreError::NegativeLength {
                what: "author length",
                value: 0x80
            })
        );
    }

    #[test]
    fn playback_hold_depends_only_on_start_frame() {
        let long = parse_scene(&synthetic(30)).unwrap();
        let p0 = playback(&long, 0);
        assert_eq!(
            (p0.first_frame, p0.ticks_per_frame, p0.first_frame_hold),
            (0, 2, 10)
        );
        let p1 = playback(&long, 1);
        assert_eq!(
            (p1.first_frame, p1.ticks_per_frame, p1.first_frame_hold),
            (11, 5, 0)
        );
        let p2 = playback(&long, 2);
        assert_eq!(
            (p2.first_frame, p2.ticks_per_frame, p2.first_frame_hold),
            (0, 5, 10)
        );
        // Short scene: the tail replay start clamps to 0 and gets the hold too.
        let short = parse_scene(&synthetic(10)).unwrap();
        let p1 = playback(&short, 1);
        assert_eq!((p1.first_frame, p1.first_frame_hold), (0, 10));
    }

    #[test]
    fn anzahl_parsing() {
        let a = parse_anzahl(b"MAXSZENE:43|5|2\0").unwrap();
        assert_eq!(
            a,
            Anzahl {
                max_scene: 43,
                e_count: 5,
                j_count: 2
            }
        );
        // Trailing non-digits are skipped, one separator byte between numbers.
        let a = parse_anzahl(b"X:7/12/3\r\n").unwrap();
        assert_eq!((a.max_scene, a.e_count, a.j_count), (7, 12, 3));
        assert_eq!(parse_anzahl(b"X:0|5|2"), Err(ToreError::BadCounts));
    }

    #[test]
    fn mirror_remaps_ids_and_positions() {
        let data = synthetic(1);
        let mut s = parse_scene(&data).unwrap();
        let o = &mut s.frames[0].objects;
        o[0] = SceneObject {
            x: 100,
            y: 0,
            y_high: 0,
            sprite: 0,
        };
        o[1] = SceneObject {
            x: 100,
            y: 0,
            y_high: 0,
            sprite: 30,
        };
        o[2] = SceneObject {
            x: 100,
            y: 0,
            y_high: 0,
            sprite: 60,
        };
        o[3] = SceneObject {
            x: 100,
            y: 0,
            y_high: 0,
            sprite: 90,
        };
        o[4] = SceneObject {
            x: 100,
            y: 0,
            y_high: 0,
            sprite: 120,
        };
        o[5] = SceneObject {
            x: 100,
            y: 0,
            y_high: 0,
            sprite: 150,
        };
        o[6] = SceneObject {
            x: 330,
            y: 0,
            y_high: 0,
            sprite: 1000,
        };
        mirror_scene(&mut s, true);
        let o = &s.frames[0].objects;
        assert_eq!((o[0].x, o[0].sprite), (320 - 100 - 14, 117));
        assert_eq!(o[1].sprite, 89);
        assert_eq!(o[2].sprite, 58 - 1);
        assert_eq!(o[3].sprite, 31);
        assert_eq!(o[4].sprite, 257 - 120);
        assert_eq!((o[5].x, o[5].sprite), (320 - 100 - 6, 150));
        // 320 - 330 - 14 wraps to a huge value -> 0.
        assert_eq!(o[6].x, 0);
        assert_eq!(s.frames[0].camera_x, 0x89 - 10);
    }

    #[test]
    fn selection_sequence() {
        let counts = Anzahl {
            max_scene: 43,
            e_count: 5,
            j_count: 2,
        };
        let mut vals = [100i16, 7, 400].into_iter();
        let mut j = false;
        let c = select_scene(&counts, true, false, &mut j, &mut || vals.next().unwrap());
        assert_eq!(c.number, 2 + 100 % 42);
        assert_eq!(c.file_name(), "18.T");
        let mut vals = [100i16, 16, 3].into_iter();
        let c = select_scene(&counts, true, false, &mut j, &mut || vals.next().unwrap());
        assert_eq!(c.file_name(), "5.TE");
        assert!(c.force_e);
        let mut vals = [1i16, 1, 401, 1].into_iter();
        let c = select_scene(&counts, false, false, &mut j, &mut || vals.next().unwrap());
        assert_eq!(c.file_name(), "2.VJ");
    }

    #[test]
    fn blit_clipping() {
        let src = Bitmap::from_pixels(2, 2, vec![1, 0, 3, 4]);
        let mut dst = Bitmap::new(3, 3);
        blit_transparent(&src, 0, 0, &mut dst, -1, 2, 2, 2);
        assert_eq!(dst.pixels, vec![0, 0, 0, 0, 0, 0, 0, 0, 0]);
        blit_transparent(&src, 0, 0, &mut dst, 2, 2, 2, 2);
        assert_eq!(dst.pixels, vec![0, 0, 0, 0, 0, 0, 0, 0, 1]);
        blit_opaque(&src, 0, 0, &mut dst, 0, 0, 5, 5);
        assert_eq!(dst.pixels, vec![1, 0, 0, 3, 4, 0, 0, 0, 1]);
    }

    fn load_pic(dir: &std::path::Path, id: u8) -> Option<Bitmap> {
        let data = std::fs::read(dir.join("PIC").join(format!("{id}.VGA"))).ok()?;
        let img = crate::vga::parse(&data).ok()?;
        Some(Bitmap::from_pixels(img.width, img.height, img.pixels))
    }

    #[test]
    fn real_files_parse_and_roundtrip() {
        let Some(dir) = crate::data_dir() else {
            return;
        };
        let tore = dir.join("TORE");
        let counts = parse_anzahl(&std::fs::read(tore.join("ANZAHL")).unwrap()).unwrap();
        assert_eq!(
            counts,
            Anzahl {
                max_scene: 43,
                e_count: 5,
                j_count: 2
            }
        );
        let mut n = 0;
        for entry in std::fs::read_dir(&tore).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().unwrap() == "ANZAHL" {
                continue;
            }
            let data = std::fs::read(&path).unwrap();
            let s = parse_scene(&data).unwrap_or_else(|e| panic!("{path:?}: {e}"));
            assert_eq!(s.frames.len(), usize::from(s.last_frame) + 1);
            assert_eq!(s.to_bytes(), data);
            assert!(s.trailing.is_empty() || s.trailing == [0, 0], "{path:?}");
            n += 1;
        }
        assert_eq!(n, 94);
        // Every file the selection logic can produce exists.
        for i in 2..=i16::from(counts.max_scene) {
            assert!(tore.join(format!("{i}.T")).exists());
            assert!(tore.join(format!("{i}.V")).exists());
        }
        for i in 2..=counts.e_count {
            assert!(tore.join(format!("{i}.TE")).exists());
            assert!(tore.join(format!("{i}.VE")).exists());
        }
        for i in 2..=counts.j_count {
            assert!(tore.join(format!("{i}.TJ")).exists());
            assert!(tore.join(format!("{i}.VJ")).exists());
        }
    }

    #[test]
    fn real_scene_renders() {
        let Some(dir) = crate::data_dir() else {
            return;
        };
        let (Some(pitch), Some(sprites), Some(goals)) = (
            load_pic(&dir, PIC_PITCH),
            load_pic(&dir, PIC_SPRITES),
            load_pic(&dir, PIC_GOALS),
        ) else {
            return;
        };
        assert_eq!((pitch.width, pitch.height), (320, 112));
        assert_eq!((sprites.width, sprites.height), (288, 77));
        let export = std::env::var_os("BMP_TORE_EXPORT").map(std::path::PathBuf::from);
        for (name, mirror) in [
            ("2.T", false),
            ("2.V", false),
            ("2.T", true),
            ("5.TE", false),
            ("2.TJ", false),
            ("31.V", false),
        ] {
            let data = std::fs::read(dir.join("TORE").join(name)).unwrap();
            let mut scene = parse_scene(&data).unwrap();
            mirror_scene(&mut scene, mirror);
            let mut player = ScenePlayer::new(pitch.clone(), sprites.clone(), goals.clone());
            let last = scene.frames.len() - 1;
            for (k, idx) in [0, last / 2, last].into_iter().enumerate() {
                let frame = scene.frames[idx].clone();
                let screen = player.render_screen(&frame, 5);
                // The window must differ from plain background somewhere.
                assert!(screen.pixels.iter().any(|&p| p != 0));
                if let Some(out) = &export {
                    let _ = std::fs::create_dir_all(out);
                    let file = out.join(format!(
                        "{}{}_{k}_f{idx}.idx",
                        name.replace('.', "_"),
                        if mirror { "_m" } else { "" }
                    ));
                    std::fs::write(file, &screen.pixels).unwrap();
                }
            }
        }
    }
}
