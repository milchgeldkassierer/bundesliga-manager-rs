//! Mod 2026 (`docs/mod2026/PRD.md`): the generator of the mod data directory and the switch
//! that tells mod data from the original data.
//!
//! Not part of the original program: everything here is mod-only (`docs/mod2026/CHANGES.md`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use bmp_formats::font::{EURO, encode_cp437};
use bmp_formats::mana::{GERMAN_CLUB_COUNT, ManaDat, PLAYER_NAME_LEN, PLAYERS_PER_CLUB};
use bmp_formats::pal;
use bmp_formats::vga::{self, ICON_SHEET, VgaImage};
use serde_json::Value;

use crate::rng::Rng;
use crate::strength;

/// The marker file whose presence in the data directory switches mod behaviour on.
pub const MARKER: &str = "MOD2026.DAT";

/// Fixed content of [`MARKER`], so that two builds of the same input are byte-identical.
const MARKER_CONTENT: &[u8] = b"Mod 2026 data directory, written by `bmp mod2026 build`.\n";

/// The clubs of the 64 German slots (`docs/mod2026/data/README.md`).
const CLUBS_JSON: &str = include_str!("../../../docs/mod2026/data/clubs.json");
/// The squads of the 64 German slots.
const SQUADS_JSON: &str = include_str!("../../../docs/mod2026/data/squads.json");

/// The crest pictures `docs/mod2026/crests/<n>.png` as (picture number, PNG bytes), sorted by
/// number (embedded by `build.rs`).
pub const CRESTS: &[(u8, &[u8])] = include!(concat!(env!("OUT_DIR"), "/mod2026_crests.rs"));

/// MOD2026: the first palette entry of the mod's own crest colours. No original picture uses
/// an entry above 31 and TEAMED keeps entry 32 black, so entries 64..127 are free; the
/// generator writes the colours into `PIC/1.PAL` and the fade covers them
/// (`docs/mod2026/CHANGES.md`).
pub const CREST_COLOUR_FIRST: usize = 64;
/// The number of crest colours.
pub const CREST_COLOUR_COUNT: usize = 64;

/// The crest colours as 6-bit DAC values: entries 64..127 of the palette every crest PNG
/// carries (`tools/crest-convert.py` writes them as `(v << 2) | (v >> 4)`).
pub fn crest_colours() -> Result<[[u8; 3]; CREST_COLOUR_COUNT], String> {
    let &(n, png) = CRESTS.first().ok_or("no crest PNG")?;
    let reader = png::Decoder::new(png)
        .read_info()
        .map_err(|e| format!("crest {n}.png: {e}"))?;
    let palette = reader.info().palette.as_deref().unwrap_or_default();
    let bytes = palette
        .get(3 * CREST_COLOUR_FIRST..3 * (CREST_COLOUR_FIRST + CREST_COLOUR_COUNT))
        .ok_or_else(|| format!("crest {n}.png: no crest colours in its palette"))?;
    let mut colours = [[0; 3]; CREST_COLOUR_COUNT];
    for (colour, [r, g, b]) in colours.iter_mut().zip(bytes.as_chunks::<3>().0) {
        *colour = [r >> 2, g >> 2, b >> 2];
    }
    Ok(colours)
}

/// `PIC/1.PAL` of `source` with the crest colours in entries 64..127 of all three palettes,
/// or `None` when `source` has no palette file.
fn mod_palette(source: &Path) -> Result<Option<Vec<u8>>, String> {
    let path = source.join(pal::PAL_FILE_PATH);
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut file = pal::PalFile::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let colours = crest_colours()?;
    let mut out = Vec::with_capacity(bytes.len());
    for palette in &mut file.palettes {
        palette.entries[CREST_COLOUR_FIRST..CREST_COLOUR_FIRST + CREST_COLOUR_COUNT]
            .copy_from_slice(&colours);
        out.extend_from_slice(&palette.to_dac_bytes());
    }
    Ok(Some(out))
}

/// The currency after an amount, v1.0.0's `" DM"` or the mod's `" €"` (one glyph where `DM`
/// has two, so a text never gets wider; `docs/mod2026/CHANGES.md`).
pub fn currency(mod2026: bool) -> &'static [u8] {
    if mod2026 { &[b' ', EURO] } else { b" DM" }
}

/// The 2x versions `docs/mod2026/crests/hires/<n>.png` of [`CRESTS`] for the 2x display layer,
/// in the same shape (embedded by `build.rs`).
pub const HIRES_CRESTS: &[(u8, &[u8])] =
    include!(concat!(env!("OUT_DIR"), "/mod2026_hires_crests.rs"));

/// Whether `data_dir` holds mod data, i.e. contains [`MARKER`].
pub fn is_active(data_dir: &Path) -> bool {
    data_dir.join(MARKER).is_file()
}

/// Writes the mod data directory `out`: a copy of every file below `from` with the mod clubs
/// written into `MANA.DAT` and every crest of [`CRESTS`] written as `PIC/<n>.VGA` (replacing
/// the copied original picture; numbers without a PNG keep it), the icon sheet `PIC/3.VGA`
/// with [`third_tier_captions`] and [`cup_captions`] (when `from` has one), `PIC/1.PAL` with
/// the [`crest_colours`] (when `from` has one), then [`MARKER`].
///
/// Refuses, before anything is created or written, an `out` that is `from` or lies inside
/// it, one that is or lies inside the owner's play directory `<repo>/BMP`, one that
/// already holds files, a `from` without a valid `MANA.DAT`, a dataset that does not fit it,
/// a crest PNG that is not an 8-bit indexed picture and an icon sheet whose captions are not
/// laid out as in the original. The
/// copies are written as new files, so they do not inherit the read-only permissions of the
/// pristine copy.
pub fn build(from: &Path, out: &Path) -> Result<(), String> {
    build_guarding(from, out, &repo_play_dir())
}

/// [`build`] with `play_dir` as the play directory that is never written.
fn build_guarding(from: &Path, out: &Path, play_dir: &Path) -> Result<(), String> {
    let source = fs::canonicalize(from).map_err(|e| format!("{}: {e}", from.display()))?;
    if !source.is_dir() {
        return Err(format!("{}: not a directory", from.display()));
    }
    let target = resolve(out).map_err(|e| format!("{}: {e}", out.display()))?;
    if target.starts_with(&source) {
        return Err(format!(
            "--out {} is the --from directory or lies inside it",
            out.display()
        ));
    }
    let play_dir = resolve(play_dir).map_err(|e| e.to_string())?;
    if target.starts_with(&play_dir) {
        return Err(format!(
            "--out {} is the play directory {}, which is never written",
            out.display(),
            play_dir.display()
        ));
    }
    if target.exists() {
        let mut entries = fs::read_dir(&target).map_err(|e| format!("{}: {e}", out.display()))?;
        if entries.next().is_some() {
            return Err(format!("--out {} is not empty", out.display()));
        }
    }
    let mana = mod_mana(&source)?;
    let crests = CRESTS
        .iter()
        .map(|&(n, png)| {
            Ok((
                n,
                vga::encode(&decode_crest(png).map_err(|e| format!("crest {n}.png: {e}"))?),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    // MOD2026: the office's third-tier buttons read "3. LIGA" instead of "AM-OBERL.".
    let icon_sheet = mod_icon_sheet(&source)?;
    // MOD2026: the crest colours in palette entries 64..127.
    let palette = mod_palette(&source)?;
    copy_tree(&source, &target).map_err(|e| format!("{}: {e}", out.display()))?;
    fs::write(target.join("MANA.DAT"), mana).map_err(|e| format!("{}: {e}", out.display()))?;
    for (n, bytes) in crests {
        let path = target.join(vga::file_name(n));
        fs::create_dir_all(path.parent().expect("PIC/<n>.VGA has a parent"))
            .and_then(|()| fs::write(&path, bytes))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    if let Some(bytes) = icon_sheet {
        let path = target.join(vga::file_name(ICON_SHEET));
        fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    if let Some(bytes) = palette {
        let path = target.join(pal::PAL_FILE_PATH);
        fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    fs::write(target.join(MARKER), MARKER_CONTENT).map_err(|e| format!("{}: {e}", out.display()))
}

/// The palette indices of an 8-bit indexed crest PNG, in its own size.
pub fn decode_crest(png: &[u8]) -> Result<VgaImage, String> {
    let mut reader = png::Decoder::new(png)
        .read_info()
        .map_err(|e| e.to_string())?;
    let info = reader.info();
    if (info.color_type, info.bit_depth) != (png::ColorType::Indexed, png::BitDepth::Eight) {
        return Err("not an 8-bit indexed picture".into());
    }
    let size = |v: u32| u16::try_from(v).map_err(|_| format!("size {v} above 65535"));
    let (width, height) = (size(info.width)?, size(info.height)?);
    let mut pixels = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut pixels).map_err(|e| e.to_string())?;
    pixels.truncate(frame.buffer_size());
    Ok(VgaImage {
        width,
        height,
        pixels,
    })
}

/// The 2x version of picture `n` from [`HIRES_CRESTS`], when `low` is exactly the mod crest
/// `n` of [`CRESTS`]. A picture with the same number that is not the mod crest (the original
/// crest, or one changed in the team editor) has no 2x version: it would show another crest.
pub fn hires_crest(n: u16, low: &VgaImage) -> Option<VgaImage> {
    let n = u8::try_from(n).ok()?;
    let png = |table: &[(u8, &'static [u8])]| {
        let i = table.binary_search_by_key(&n, |&(m, _)| m).ok()?;
        decode_crest(table[i].1).ok()
    };
    if png(CRESTS)? != *low {
        return None;
    }
    png(HIRES_CRESTS)
}

/// The columns of a cell that a button shows.
const ICON_WIDTH: usize = vga::ICON_SIZE.0 as usize;
/// The (`2. LIGA`, `AM-OBERL.`) icon codes of the tables submenu's table, fixtures and
/// strength buttons (pinned to the office's submenu by a test in `bmp`).
pub const CAPTION_CELLS: [(i16, i16); 3] = [(13, 14), (16, 17), (19, 20)];
/// The rows of a cell that hold its caption: five glyph rows and the shadow row.
const CAPTION_ROWS: std::ops::Range<usize> = 16..22;
/// The first column of the box of the caption's digit, three glyph columns and the shadow.
const DIGIT_X: usize = 6;
/// The caption's `2`: `W` white, `A` anti-alias, `.` background.
const DIGIT_TWO: [&[u8; 3]; 5] = [b"WWA", b"..W", b"AWA", b"W..", b"WWW"];
/// The `3` drawn in its place, in the same style.
const DIGIT_THREE: [&[u8; 3]; 5] = [b"WWA", b"..W", b".WA", b"..W", b"WWA"];

/// The digit box of `mask` in the colours `[background, white, anti-alias, shadow]`: every
/// glyph pixel casts a shadow one pixel right and down, below the glyph pixels.
fn render_digit(mask: &[&[u8; 3]; 5], [background, white, anti, shadow]: [u8; 4]) -> [[u8; 4]; 6] {
    let mut digit = [[background; 4]; 6];
    let glyph = || {
        mask.iter()
            .enumerate()
            .flat_map(|(y, row)| row.iter().enumerate().map(move |(x, &c)| (x, y, c)))
            .filter(|&(_, _, c)| c != b'.')
    };
    for (x, y, _) in glyph() {
        digit[y + 1][x + 1] = shadow;
    }
    for (x, y, c) in glyph() {
        digit[y][x] = if c == b'W' { white } else { anti };
    }
    digit
}

/// The index in `pixels` of pixel (`x`, `y`) of icon `code` in a sheet `width` pixels wide.
fn icon_index(width: usize, code: i16, x: usize, y: usize) -> usize {
    let (left, top) = vga::icon_origin(code);
    (index(top) + y) * width + index(left) + x
}

/// A non-negative coordinate of the icon sheet as an index.
fn index(v: i16) -> usize {
    usize::try_from(v).expect("icon codes and their origins are non-negative")
}

/// Makes the tables submenu's third-tier buttons read `3. LIGA`: copies the `2. LIGA` caption
/// of each [`CAPTION_CELLS`] pair over the `AM-OBERL.` caption and draws a `3` over its `2`, in
/// the caption's own colours. Refuses, leaving `sheet` unchanged, a sheet whose captions are
/// not laid out as in the original.
fn third_tier_captions(sheet: &mut VgaImage) -> Result<(), String> {
    let (width, height) = (usize::from(sheet.width), usize::from(sheet.height));
    let codes = CAPTION_CELLS.iter().flat_map(|&(from, to)| [from, to]);
    let (left, top) = codes
        .map(vga::icon_origin)
        .fold((0, 0), |(l, t), (x, y)| (l.max(x), t.max(y)));
    if width < index(left) + ICON_WIDTH || height < index(top) + CAPTION_ROWS.end {
        return Err(format!("{width}x{height} is too small for the captions"));
    }
    let at = |code, x, y| icon_index(width, code, x, y);
    let band = |sheet: &VgaImage, code: i16| -> Vec<u8> {
        CAPTION_ROWS
            .flat_map(|y| (0..ICON_WIDTH).map(move |x| (x, y)))
            .map(|(x, y)| sheet.pixels[at(code, x, y)])
            .collect()
    };
    let digit = |sheet: &VgaImage, code: i16| -> Vec<u8> {
        CAPTION_ROWS
            .flat_map(|y| (DIGIT_X..DIGIT_X + 4).map(move |x| sheet.pixels[at(code, x, y)]))
            .collect()
    };
    let (source, target) = (CAPTION_CELLS[0].0, CAPTION_CELLS[0].1);
    for (from, to) in CAPTION_CELLS {
        if band(sheet, from) != band(sheet, source) || band(sheet, to) != band(sheet, target) {
            return Err(format!(
                "the captions of icons {from} and {to} differ from their row's"
            ));
        }
    }
    let top = CAPTION_ROWS.start;
    let roles = [
        (0, top),
        (DIGIT_X, top),
        (DIGIT_X + 2, top),
        (DIGIT_X + 1, top + 1),
    ]
    .map(|(x, y)| sheet.pixels[at(source, x, y)]);
    if (1..4).any(|i| roles[..i].contains(&roles[i])) {
        return Err("the caption's colours are not four distinct ones".into());
    }
    if render_digit(&DIGIT_TWO, roles).concat() != digit(sheet, source) {
        return Err(format!("icon {source} does not read 2. LIGA"));
    }
    let three = render_digit(&DIGIT_THREE, roles);
    for (from, to) in CAPTION_CELLS {
        for y in CAPTION_ROWS {
            for x in 0..ICON_WIDTH {
                sheet.pixels[at(to, x, y)] = match x.checked_sub(DIGIT_X) {
                    Some(dx) if dx < 4 => three[y - top][dx],
                    _ => sheet.pixels[at(from, x, y)],
                };
            }
        }
    }
    Ok(())
}

/// The (`LANDES MEISTER`, `POKAL SIEGER`, `UEFA-CUP`) icon codes of the cup submenu's European
/// cup buttons (pinned to the office's submenu by a test in `bmp`).
pub const CUP_CELLS: [i16; 3] = [28, 29, 30];
/// The two-line captions written over [`CUP_CELLS`]: `CHAMPIONS` does not fit the 32 columns
/// in this font (33 without its shadow), so it is cut like the original's `BUNDESL.`.
const CUP_CAPTIONS: [[&[u8]; 2]; 3] = [
    [b"CHAMP.", b"LEAGUE"],
    [b"CONF.", b"LEAGUE"],
    [b"EUROPA", b"LEAGUE"],
];
/// The rows of a cell that hold a two-line caption: four glyph rows and the shadow row each.
const CUP_CAPTION_ROWS: std::ops::Range<usize> = 12..22;
/// The rows of one line of a two-line caption.
const LINE_ROWS: usize = 5;
/// The second line of `POKAL SIEGER` and its first column, redrawn to check the font.
const SIEGER: (&[u8], usize) = (b"SIEGER", 6);
/// The 4-row font of the two-line captions: `W` white, `a` dark and `b` light anti-alias, `.`
/// background. Letters of the original's two-line captions are drawn as there; `C`, `F`, `H`,
/// `U` and `.` are new in the same style.
const SMALL_FONT: [(u8, [&[u8]; 4]); 16] = [
    (b'.', [b".", b".", b".", b"W"]),
    (b'A', [b"aWa", b"W.W", b"WWW", b"W.W"]),
    (b'C', [b"aWW", b"W..", b"W..", b"aWW"]),
    (b'E', [b"WWW", b"WWa", b"W..", b"WWW"]),
    (b'F', [b"WWW", b"WWa", b"W..", b"W.."]),
    (b'G', [b"aWW", b"W..", b"W.W", b"aWa"]),
    (b'H', [b"W.W", b"WWW", b"W.W", b"W.W"]),
    (b'I', [b"W", b"W", b"W", b"W"]),
    (b'L', [b"W..", b"W..", b"W..", b"WWW"]),
    (b'M', [b"WaW", b"WWW", b"W.W", b"W.W"]),
    (b'N', [b"WaW", b"WWW", b"WWW", b"WaW"]),
    (b'O', [b"aWa", b"W.W", b"W.W", b"aWa"]),
    (b'P', [b"WWa", b"W.W", b"WWa", b"W.."]),
    (b'R', [b"WWa", b"W.W", b"WWa", b"W.W"]),
    (b'S', [b"bWW", b"Wa.", b".bW", b"WWb"]),
    (b'U', [b"W.W", b"W.W", b"W.W", b"aWa"]),
];

/// The glyph of `c` in [`SMALL_FONT`].
fn small_glyph(c: u8) -> &'static [&'static [u8]; 4] {
    let i = SMALL_FONT
        .binary_search_by_key(&c, |&(g, _)| g)
        .unwrap_or_else(|_| panic!("no glyph for {}", char::from(c)));
    &SMALL_FONT[i].1
}

/// The width of `text` in [`SMALL_FONT`], one column between glyphs, without the shadow.
fn small_text_width(text: &[u8]) -> usize {
    let glyphs: usize = text.iter().map(|&c| small_glyph(c)[0].len()).sum();
    glyphs + text.len().saturating_sub(1)
}

/// One line of a two-line caption, `text` from column `x`, in the colours `[background, white,
/// dark anti-alias, light anti-alias, shadow]`: every glyph pixel casts a shadow one pixel
/// right and down, below the glyph pixels.
fn caption_line(text: &[u8], x: usize, roles: [u8; 5]) -> [[u8; ICON_WIDTH]; LINE_ROWS] {
    let [background, white, dark, light, shadow] = roles;
    let mut line = [[background; ICON_WIDTH]; LINE_ROWS];
    let mut pixels = Vec::new();
    let mut left = x;
    for &c in text {
        let glyph = small_glyph(c);
        for (y, row) in glyph.iter().enumerate() {
            for (dx, &p) in row.iter().enumerate() {
                if p != b'.' {
                    pixels.push((left + dx, y, p));
                }
            }
        }
        left += glyph[0].len() + 1;
    }
    for &(x, y, _) in &pixels {
        line[y + 1][x + 1] = shadow;
    }
    for &(x, y, p) in &pixels {
        line[y][x] = match p {
            b'W' => white,
            b'a' => dark,
            _ => light,
        };
    }
    line
}

/// The caption band ([`CUP_CAPTION_ROWS`]) of two lines, each centred in the button.
fn cup_caption(lines: [&[u8]; 2], roles: [u8; 5]) -> Vec<[u8; ICON_WIDTH]> {
    lines
        .iter()
        .flat_map(|text| caption_line(text, (ICON_WIDTH - small_text_width(text)) / 2, roles))
        .collect()
}

/// Makes the cup submenu's European cup buttons read `CHAMP. LEAGUE`, `CONF. LEAGUE` and
/// `EUROPA LEAGUE` ([`CUP_CAPTIONS`]): replaces the caption band of each [`CUP_CELLS`] icon by
/// two lines in the font of the original's two-line captions, in their own colours. Refuses,
/// leaving `sheet` unchanged, a sheet whose cup buttons are not laid out as in the original
/// (checked by redrawing `SIEGER` from [`SMALL_FONT`]).
fn cup_captions(sheet: &mut VgaImage) -> Result<(), String> {
    let (width, height) = (usize::from(sheet.width), usize::from(sheet.height));
    let (left, top) = CUP_CELLS
        .map(vga::icon_origin)
        .iter()
        .fold((0, 0), |(l, t), &(x, y)| (l.max(x), t.max(y)));
    if width < index(left) + ICON_WIDTH || height < index(top) + CUP_CAPTION_ROWS.end {
        return Err(format!(
            "{width}x{height} is too small for the cup captions"
        ));
    }
    let at = |code, x, y| icon_index(width, code, x, y);
    let rows = |sheet: &VgaImage, code: i16, ys: std::ops::Range<usize>| -> Vec<u8> {
        ys.flat_map(|y| (0..ICON_WIDTH).map(move |x| (x, y)))
            .map(|(x, y)| sheet.pixels[at(code, x, y)])
            .collect()
    };
    let art = 0..CUP_CAPTION_ROWS.start;
    for code in CUP_CELLS {
        if rows(sheet, code, art.clone()) != rows(sheet, CUP_CELLS[0], art.clone()) {
            return Err(format!(
                "the picture of icon {code} differs from icon {}",
                CUP_CELLS[0]
            ));
        }
    }
    let (text, x) = SIEGER;
    let line = CUP_CAPTION_ROWS.start + LINE_ROWS;
    let roles = [(0, 0), (x + 1, 0), (x + 1, 1), (x, 0), (x + 3, 1)]
        .map(|(dx, dy)| sheet.pixels[at(CUP_CELLS[1], dx, line + dy)]);
    if (1..5).any(|i| roles[..i].contains(&roles[i])) {
        return Err("the cup caption's colours are not five distinct ones".into());
    }
    if caption_line(text, x, roles).concat() != rows(sheet, CUP_CELLS[1], line..line + LINE_ROWS) {
        return Err(format!("icon {} does not read POKAL SIEGER", CUP_CELLS[1]));
    }
    for (code, lines) in CUP_CELLS.into_iter().zip(CUP_CAPTIONS) {
        for (dy, row) in cup_caption(lines, roles).iter().enumerate() {
            for (x, &p) in row.iter().enumerate() {
                sheet.pixels[at(code, x, CUP_CAPTION_ROWS.start + dy)] = p;
            }
        }
    }
    Ok(())
}

/// [`ICON_SHEET`] of `source` with [`third_tier_captions`] and [`cup_captions`] applied, as file
/// bytes; `None` when `source` has none.
fn mod_icon_sheet(source: &Path) -> Result<Option<Vec<u8>>, String> {
    let path = source.join(vga::file_name(ICON_SHEET));
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut sheet = vga::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    third_tier_captions(&mut sheet)
        .and_then(|()| cup_captions(&mut sheet))
        .map_err(|e| format!("{}: not the original icon sheet: {e}", path.display()))?;
    Ok(Some(vga::encode(&sheet)))
}

/// The `MANA.DAT` of `source` with [`write_mod_clubs`] applied, as file bytes.
fn mod_mana(source: &Path) -> Result<Vec<u8>, String> {
    let path = source.join("MANA.DAT");
    let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut mana = ManaDat::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    write_mod_clubs(&mut mana)?;
    Ok(mana.to_bytes())
}

/// Writes the German slots 0..63 from the dataset (name, crest, the 2025/26 table as the
/// 2-point pair, goals and rank, players) and recomputes their strengths with
/// [`strength::MOD2026`]. The foreign clubs keep their bytes.
fn write_mod_clubs(mana: &mut ManaDat) -> Result<(), String> {
    let clubs = dataset(CLUBS_JSON, "clubs.json")?;
    let squads = dataset(SQUADS_JSON, "squads.json")?;
    for (slot, (club, squad)) in clubs.iter().zip(&squads).enumerate() {
        write_club(mana, slot, club, squad).map_err(|e| format!("dataset slot {slot}: {e}"))?;
    }
    // Fixed seed: the MSC start state (srand(1)) the team editor starts with, so two builds
    // write the same bytes.
    strength::recompute_strengths_grouped(mana, &mut Rng::default(), &strength::MOD2026);
    Ok(())
}

/// The array of 64 slots in `json`.
fn dataset(json: &str, file: &str) -> Result<Vec<Value>, String> {
    match serde_json::from_str(json).map_err(|e| format!("{file}: {e}"))? {
        Value::Array(slots) if slots.len() == GERMAN_CLUB_COUNT => Ok(slots),
        _ => Err(format!("{file}: not an array of {GERMAN_CLUB_COUNT} slots")),
    }
}

fn write_club(mana: &mut ManaDat, slot: usize, club: &Value, squad: &Value) -> Result<(), String> {
    let players = squad["players"]
        .as_array()
        .ok_or("`players` is not an array")?;
    if players.len() > PLAYERS_PER_CLUB {
        return Err(format!(
            "{} players, at most {PLAYERS_PER_CLUB}",
            players.len()
        ));
    }
    let table = &club["table_2025_26"];
    let [wins, draws, losses] = [
        int(table, "wins")?,
        int(table, "draws")?,
        int(table, "losses")?,
    ];
    mana.crest[slot] = byte(int(club, "crest")?, "crest")?;
    // Block 3 stays a 2-point pair: the strength recomputation reads it with the original rule.
    let per_win = i64::from(crate::league_layout::ORIGINAL.points_per_win);
    mana.points[slot] = [
        byte(per_win * wins + draws, "plus points")?,
        byte(per_win * losses + draws, "minus points")?,
    ];
    mana.goals[slot] = [
        byte(int(table, "goals_for")?, "goals_for")?,
        byte(int(table, "goals_against")?, "goals_against")?,
    ];
    mana.rank[slot] = byte(int(table, "rank")?, "rank")?;
    let record = &mut mana.clubs[slot];
    record.name = field(text(club, "name")?)?;
    for (k, player) in record.players.iter_mut().enumerate() {
        *player = match players.get(k) {
            Some(p) => field(text(p, "name")?)?,
            None => [0; PLAYER_NAME_LEN],
        };
    }
    Ok(())
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("`{key}` is not a string"))
}

fn int(value: &Value, key: &str) -> Result<i64, String> {
    value[key]
        .as_i64()
        .ok_or_else(|| format!("`{key}` is not an integer"))
}

fn byte(n: i64, what: &str) -> Result<u8, String> {
    u8::try_from(n).map_err(|_| format!("{what} {n} does not fit a byte"))
}

/// `name` in CP437, NUL-padded to a name field of `N` bytes that keeps its NUL terminator.
fn field<const N: usize>(name: &str) -> Result<[u8; N], String> {
    let bytes = encode_cp437(name);
    if bytes.len() >= N {
        return Err(format!("`{name}` is longer than {} bytes", N - 1));
    }
    let mut field = [0; N];
    field[..bytes.len()].copy_from_slice(&bytes);
    Ok(field)
}

/// `<main checkout>/BMP`, the owner's play directory, for the checkout this crate was built
/// from (see [`play_dir_of`]).
fn repo_play_dir() -> PathBuf {
    let git_common_dir = env!("BMP_GIT_COMMON_DIR");
    play_dir_of(
        Path::new(env!("CARGO_MANIFEST_DIR")),
        (!git_common_dir.is_empty()).then(|| Path::new(git_common_dir)),
    )
}

/// The play directory `BMP` of the main checkout, derived like `tools/check.sh` does: next to
/// `git_common_dir` (`git rev-parse --git-common-dir`, the main checkout's `.git` also for a
/// worktree), or `<manifest_dir>/../../BMP` when there is no git metadata (e.g. a source
/// tarball).
fn play_dir_of(manifest_dir: &Path, git_common_dir: Option<&Path>) -> PathBuf {
    match git_common_dir.and_then(Path::parent) {
        Some(main_checkout) => main_checkout.join("BMP"),
        None => manifest_dir.join("../../BMP"),
    }
}

/// `path` made absolute with every symbolic link resolved, also when it does not exist
/// (yet): the longest existing ancestor is canonicalised and the rest appended.
fn resolve(path: &Path) -> io::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut existing = absolute.as_path();
    let mut rest = Vec::new();
    loop {
        match fs::canonicalize(existing) {
            Ok(base) => return Ok(rest.iter().rev().fold(base, |p, c| p.join(c))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                rest.push(existing.file_name().ok_or(e)?.to_owned());
                existing = existing
                    .parent()
                    .expect("a path with a file name has a parent");
            }
            Err(e) => return Err(e),
        }
    }
}

/// Copies every file below `from` to the same relative path below `to`, in sorted order.
pub fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    let mut entries = fs::read_dir(from)?.collect::<io::Result<Vec<_>>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let target = to.join(entry.file_name());
        if entry.metadata()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::write(target, fs::read(entry.path())?)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmp_formats::mana;

    #[test]
    fn currency_is_dm_or_euro() {
        assert_eq!(currency(false), b" DM");
        assert_eq!(currency(true), [b' ', EURO]);
    }

    /// A fresh, empty directory below the system temp directory.
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bmp-mod2026-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Every file below `dir` as (relative path, bytes), sorted by path.
    fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        fn walk(root: &Path, dir: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(root, &path, files);
                } else {
                    let bytes = fs::read(&path).unwrap();
                    files.push((path.strip_prefix(root).unwrap().to_path_buf(), bytes));
                }
            }
        }
        let mut files = Vec::new();
        walk(dir, dir, &mut files);
        files.sort();
        files
    }

    /// A `MANA.DAT` that parses: every byte zero.
    fn zeroed_mana() -> Vec<u8> {
        vec![0; mana::FILE_SIZE]
    }

    /// Pixel `(x, y)` of icon `code` in `sheet`.
    fn icon_pixel(sheet: &mut VgaImage, code: i16, x: usize, y: usize) -> &mut u8 {
        &mut sheet.pixels[icon_index(usize::from(sheet.width), code, x, y)]
    }

    /// A sheet of the original's size in background 1 whose `2. LIGA` captions hold the `2` in
    /// white 15, anti-alias 7 and shadow 0 and a period, and whose `AM-OBERL.` captions hold
    /// marks in 9.
    fn synthetic_icon_sheet() -> VgaImage {
        let mut sheet = VgaImage {
            width: 197,
            height: 167,
            pixels: vec![1; 197 * 167],
        };
        let two = render_digit(&DIGIT_TWO, [1, 15, 7, 0]);
        for (from, to) in CAPTION_CELLS {
            for (dy, row) in two.iter().enumerate() {
                for (dx, &c) in row.iter().enumerate() {
                    *icon_pixel(&mut sheet, from, DIGIT_X + dx, CAPTION_ROWS.start + dy) = c;
                }
            }
            *icon_pixel(&mut sheet, from, 10, 20) = 15;
            for (x, y) in [(2, 16), (20, 18), (31, 21)] {
                *icon_pixel(&mut sheet, to, x, y) = 9;
            }
        }
        *icon_pixel(&mut sheet, 14, 0, 0) = 5;
        let (text, x) = SIEGER;
        let line = caption_line(text, x, SYNTHETIC_CUP_ROLES);
        for (dy, row) in line.iter().enumerate() {
            for (dx, &c) in row.iter().enumerate() {
                let y = CUP_CAPTION_ROWS.start + LINE_ROWS + dy;
                *icon_pixel(&mut sheet, CUP_CELLS[1], dx, y) = c;
            }
        }
        for code in CUP_CELLS {
            *icon_pixel(&mut sheet, code, 4, 3) = 8;
        }
        *icon_pixel(&mut sheet, CUP_CELLS[2], 20, 18) = 9;
        sheet
    }

    /// The cup caption colours of [`synthetic_icon_sheet`]: background, white, dark and light
    /// anti-alias, shadow.
    const SYNTHETIC_CUP_ROLES: [u8; 5] = [1, 15, 7, 6, 0];

    /// The indices of the caption bands of the [`CUP_CELLS`] in a sheet `width` pixels wide.
    fn cup_bands(width: u16) -> Vec<usize> {
        CUP_CELLS
            .iter()
            .flat_map(|&code| {
                CUP_CAPTION_ROWS.flat_map(move |y| {
                    (0..ICON_WIDTH).map(move |x| icon_index(usize::from(width), code, x, y))
                })
            })
            .collect()
    }

    /// `patched` with the pixels at `indices` taken from `original`.
    fn restored(patched: &VgaImage, original: &VgaImage, indices: &[usize]) -> VgaImage {
        let mut sheet = patched.clone();
        for &i in indices {
            sheet.pixels[i] = original.pixels[i];
        }
        sheet
    }

    /// The text of one caption line (`rows`, four glyph rows), read glyph by glyph: ink is any
    /// colour of `ink` (white, dark and light anti-alias), glyphs are the column runs with ink, each matched against
    /// [`SMALL_FONT`].
    fn read_caption_line(rows: &[&[u8]], ink: [u8; 3]) -> String {
        let [white, dark, _] = ink;
        let inked = |x: usize| rows.iter().any(|row| ink.contains(&row[x]));
        let mut text = String::new();
        let mut x = 0;
        while x < ICON_WIDTH {
            if !inked(x) {
                x += 1;
                continue;
            }
            let end = (x..ICON_WIDTH).find(|&e| !inked(e)).unwrap_or(ICON_WIDTH);
            let mask: Vec<Vec<u8>> = rows
                .iter()
                .map(|row| {
                    row[x..end]
                        .iter()
                        .map(|&p| match p {
                            p if p == white => b'W',
                            p if p == dark => b'a',
                            p if ink.contains(&p) => b'b',
                            _ => b'.',
                        })
                        .collect()
                })
                .collect();
            let glyph = SMALL_FONT
                .iter()
                .find(|(_, g)| g.iter().zip(&mask).all(|(a, b)| *a == b.as_slice()))
                .map_or('?', |&(c, _)| char::from(c));
            text.push(glyph);
            x = end;
        }
        text
    }

    /// Asserts that `patched` is `original` with only the caption bands of the [`CUP_CELLS`]
    /// changed, each to two lines in the caption colours `roles` (background, white, dark and
    /// light anti-alias, shadow) that read [`CUP_CAPTIONS`] and stay inside the button with
    /// their shadows; returns the lines read.
    fn assert_cup_captions(original: &VgaImage, patched: &VgaImage, roles: [u8; 5]) -> Vec<String> {
        assert_eq!(
            (patched.width, patched.height),
            (original.width, original.height)
        );
        let bands = cup_bands(original.width);
        for (i, (a, b)) in original.pixels.iter().zip(&patched.pixels).enumerate() {
            assert!(
                a == b || bands.contains(&i),
                "pixel {i} outside the cup captions changed"
            );
        }
        let [background, white, dark, light, _] = roles;
        let mut read = Vec::new();
        for code in CUP_CELLS {
            let at = |x, y| icon_index(usize::from(original.width), code, x, y);
            let band: Vec<Vec<u8>> = CUP_CAPTION_ROWS
                .map(|y| (0..ICON_WIDTH).map(|x| patched.pixels[at(x, y)]).collect())
                .collect();
            assert!(
                CUP_CAPTION_ROWS.clone().any(|y| (0..ICON_WIDTH)
                    .any(|x| patched.pixels[at(x, y)] != original.pixels[at(x, y)])),
                "icon {code} unchanged"
            );
            assert!(
                band.iter().flatten().all(|p| roles.contains(p)),
                "icon {code} uses a colour outside the caption's"
            );
            for row in &band {
                assert_eq!(row[0], background, "icon {code} clipped on the left");
                assert_eq!(
                    row[ICON_WIDTH - 1],
                    background,
                    "icon {code} clipped on the right"
                );
            }
            for line in band.chunks(LINE_ROWS) {
                let glyph_rows: Vec<&[u8]> = line[..4].iter().map(Vec::as_slice).collect();
                assert!(
                    !line[4].iter().any(|p| [white, dark, light].contains(p)),
                    "icon {code} draws glyphs into a shadow row"
                );
                read.push(read_caption_line(&glyph_rows, [white, dark, light]));
            }
        }
        let expected: Vec<String> = CUP_CAPTIONS
            .iter()
            .flatten()
            .map(|t| String::from_utf8(t.to_vec()).unwrap())
            .collect();
        assert_eq!(read, expected);
        read
    }

    #[test]
    fn small_font_is_sorted_and_four_rows_of_one_width() {
        assert!(SMALL_FONT.windows(2).all(|w| w[0].0 < w[1].0));
        for (c, glyph) in SMALL_FONT {
            assert!(
                glyph.iter().all(|row| row.len() == glyph[0].len()),
                "{}",
                char::from(c)
            );
        }
    }

    /// Every cup caption fits the button with its shadow and one background column on each
    /// side; `CHAMPIONS` would not.
    #[test]
    fn cup_captions_fit_the_button() {
        for text in CUP_CAPTIONS.iter().flatten() {
            assert!(small_text_width(text) < ICON_WIDTH - 2, "{text:?}");
        }
        assert!(small_text_width(b"CHAMPIONS") >= ICON_WIDTH - 2);
    }

    #[test]
    fn cup_captions_read_champions_conference_and_europa_league() {
        let original = synthetic_icon_sheet();
        let mut sheet = original.clone();
        cup_captions(&mut sheet).unwrap();
        assert_cup_captions(&original, &sheet, SYNTHETIC_CUP_ROLES);
    }

    #[test]
    fn cup_captions_refuse_an_unexpected_sheet() {
        let resized = |width: u16, height: u16| {
            let mut sheet = synthetic_icon_sheet();
            let mut pixels = Vec::new();
            for y in 0..usize::from(height) {
                let start = y * usize::from(sheet.width);
                pixels.extend_from_slice(&sheet.pixels[start..start + usize::from(width)]);
            }
            (sheet.width, sheet.height, sheet.pixels) = (width, height, pixels);
            sheet
        };
        let changed = |code: i16, x: usize, y: usize, c: u8| {
            let mut sheet = synthetic_icon_sheet();
            *icon_pixel(&mut sheet, code, x, y) = c;
            sheet
        };
        let (_, x) = SIEGER;
        for sheet in [
            resized(197, 141),
            changed(30, 4, 3, 2),
            changed(29, x + 1, 17, 3),
            changed(29, 20, 21, 3),
            changed(29, x + 1, 18, 15),
        ] {
            let mut patched = sheet.clone();
            assert!(cup_captions(&mut patched).is_err());
            assert_eq!(patched, sheet);
        }
        // Icon 30's caption band ends at row 141.
        cup_captions(&mut resized(197, 142)).unwrap();
        assert!(
            cup_captions(&mut resized(197, 141))
                .unwrap_err()
                .contains("too small")
        );
    }

    /// Asserts that `patched` is `original` with only the captions of the three `AM-OBERL.`
    /// buttons changed, each to its `2. LIGA` caption outside the digit box; returns the
    /// digit boxes of those captions.
    fn assert_captions_copied_from_2_liga(original: &VgaImage, patched: &VgaImage) -> Vec<Vec<u8>> {
        assert_eq!(
            (patched.width, patched.height),
            (original.width, original.height)
        );
        let at = |code, x, y| icon_index(usize::from(original.width), code, x, y);
        let captions: Vec<usize> = CAPTION_CELLS
            .iter()
            .flat_map(|&(_, to)| {
                CAPTION_ROWS.flat_map(move |y| (0..ICON_WIDTH).map(move |x| at(to, x, y)))
            })
            .collect();
        for (i, (a, b)) in original.pixels.iter().zip(&patched.pixels).enumerate() {
            assert!(
                a == b || captions.contains(&i),
                "pixel {i} outside the captions changed"
            );
        }
        let mut digits = Vec::new();
        for (from, to) in CAPTION_CELLS {
            let mut differs = false;
            let mut digit = Vec::new();
            for y in CAPTION_ROWS {
                for x in 0..ICON_WIDTH {
                    let new = patched.pixels[at(to, x, y)];
                    differs |= new != original.pixels[at(to, x, y)];
                    if (DIGIT_X..DIGIT_X + 4).contains(&x) {
                        digit.push(new);
                    } else {
                        assert_eq!(new, original.pixels[at(from, x, y)], "icon {to}");
                    }
                }
            }
            assert!(differs, "icon {to} unchanged");
            digits.push(digit);
        }
        digits
    }

    #[test]
    fn third_tier_captions_draw_3_liga() {
        let original = synthetic_icon_sheet();
        let mut sheet = original.clone();
        third_tier_captions(&mut sheet).unwrap();
        let three = render_digit(&DIGIT_THREE, [1, 15, 7, 0]).concat();
        assert_eq!(
            assert_captions_copied_from_2_liga(&original, &sheet),
            vec![three; 3]
        );
    }

    #[test]
    fn third_tier_captions_refuse_an_unexpected_sheet() {
        let resized = |width: u16, height: u16| VgaImage {
            width,
            height,
            pixels: vec![1; usize::from(width) * usize::from(height)],
        };
        let changed = |cells: &[i16], x: usize, y: usize| {
            let mut sheet = synthetic_icon_sheet();
            for &code in cells {
                *icon_pixel(&mut sheet, code, x, y) = 3;
            }
            sheet
        };
        let mut one_colour = synthetic_icon_sheet();
        for (from, _) in CAPTION_CELLS {
            for y in CAPTION_ROWS {
                for x in DIGIT_X..DIGIT_X + 4 {
                    *icon_pixel(&mut one_colour, from, x, y) = 1;
                }
            }
        }
        for sheet in [
            resized(196, 167),
            resized(197, 93),
            changed(&[16], 20, 18),
            changed(&[17], 20, 19),
            changed(&[13, 16, 19], 8, 18),
            one_colour,
        ] {
            let mut patched = sheet.clone();
            assert!(third_tier_captions(&mut patched).is_err());
            assert_eq!(patched, sheet);
        }
    }

    /// The size check covers every cell read: icon 17 reaches column 196, icon 20 the caption
    /// row 93.
    #[test]
    fn third_tier_captions_need_every_caption_inside_the_sheet() {
        let cropped = |height: u16| {
            let mut sheet = synthetic_icon_sheet();
            sheet.height = height;
            sheet
                .pixels
                .truncate(usize::from(sheet.width) * usize::from(height));
            sheet
        };
        third_tier_captions(&mut cropped(94)).unwrap();
        let mut narrow = synthetic_icon_sheet();
        narrow.width = 150;
        narrow.pixels.truncate(150 * usize::from(narrow.height));
        for mut sheet in [narrow, cropped(93)] {
            let error = third_tier_captions(&mut sheet).unwrap_err();
            assert!(error.contains("too small"), "{error}");
        }
    }

    #[test]
    fn build_refuses_an_unexpected_icon_sheet_and_writes_nothing() {
        let dir = temp("sheet");
        let from = dir.join("from");
        fs::create_dir_all(from.join("PIC")).unwrap();
        fs::write(from.join("MANA.DAT"), zeroed_mana()).unwrap();
        let small = VgaImage {
            width: 10,
            height: 10,
            pixels: vec![0; 100],
        };
        fs::write(from.join(vga::file_name(ICON_SHEET)), vga::encode(&small)).unwrap();
        let out = dir.join("out");

        let error = build(&from, &out).unwrap_err();

        assert!(error.contains("3.VGA"), "{error}");
        assert!(!out.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// With the original data, only the captions of the three `AM-OBERL.` buttons change, each
    /// to the `2. LIGA` caption with another digit. A `.VGA` file holds no palette (that is
    /// `PIC/1.PAL`, which the copy test keeps byte-identical); its header keeps the original
    /// width and height bytes, and only the packed length follows the new pixels.
    #[test]
    fn build_writes_mod_captions_into_the_icon_sheet() {
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        let file = vga::file_name(ICON_SHEET);
        let before = fs::read(data.join(&file)).unwrap();
        let dir = temp("captions");
        let out = dir.join("out");
        build(&data, &out).unwrap();
        assert_eq!(fs::read(data.join(&file)).unwrap(), before);
        let after = fs::read(out.join(&file)).unwrap();
        assert_eq!(after[..4], before[..4], "width and height bytes");
        for bytes in [&before, &after] {
            let header = vga::parse_header(bytes).unwrap();
            assert_eq!(
                bytes.len(),
                vga::HEADER_LEN + usize::from(header.packed_len)
            );
        }
        let original = vga::parse(&before).unwrap();
        let patched = vga::parse(&after).unwrap();
        let without_cups = restored(&patched, &original, &cup_bands(original.width));
        let digits = assert_captions_copied_from_2_liga(&original, &without_cups);
        let third_tier: Vec<usize> = (0..original.pixels.len())
            .filter(|&i| without_cups.pixels[i] != original.pixels[i])
            .collect();
        assert_eq!(digits.len(), 3);
        // The cup captions in the colours of POKAL SIEGER's second line: background, white,
        // dark and light anti-alias, shadow.
        let (_, x) = SIEGER;
        let at = |dx, dy| original.pixels[icon_index(usize::from(original.width), 29, dx, dy)];
        let roles = [
            at(0, 17),
            at(x + 1, 17),
            at(x + 1, 18),
            at(x, 17),
            at(x + 3, 18),
        ];
        let without_tables = restored(&patched, &original, &third_tier);
        assert_cup_captions(&original, &without_tables, roles);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hires_crest_needs_the_mod_crest() {
        for &(n, png) in CRESTS {
            let low = decode_crest(png).unwrap();
            let hires = hires_crest(n.into(), &low).unwrap();
            assert_eq!((hires.width, hires.height), (2 * low.width, 2 * low.height));
            let mut edited = low.clone();
            edited.pixels[0] ^= 1;
            assert_eq!(hires_crest(n.into(), &edited), None, "crest {n} edited");
        }
        let (n, png) = CRESTS[0];
        let low = decode_crest(png).unwrap();
        for other in [0, 22, 144, 256 + u16::from(n)] {
            assert_eq!(hires_crest(other, &low), None, "picture {other}");
        }
    }

    #[test]
    fn marker_decides_whether_mod_data_is_active() {
        let dir = temp("active");
        assert!(!is_active(&dir));
        fs::write(dir.join(MARKER), MARKER_CONTENT).unwrap();
        assert!(is_active(&dir));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn build_copies_the_data_and_writes_the_marker() {
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        let dir = temp("copy");
        let out = dir.join("out");
        build(&data, &out).unwrap();
        let copied = tree(&out);
        let original = tree(&data);
        let crests: Vec<PathBuf> = CRESTS
            .iter()
            .map(|&(n, _)| PathBuf::from(vga::file_name(n)))
            .collect();
        for file in &original {
            if file.0 == Path::new("MANA.DAT")
                || file.0 == Path::new(&vga::file_name(ICON_SHEET))
                || file.0 == Path::new(pal::PAL_FILE_PATH)
                || crests.contains(&file.0)
            {
                continue;
            }
            assert!(copied.contains(file), "{} not copied", file.0.display());
        }
        assert!(ManaDat::parse(&fs::read(out.join("MANA.DAT")).unwrap()).is_ok());
        assert!(is_active(&out));
        assert_eq!(
            copied.len(),
            original.len() + usize::from(!is_active(&data))
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    /// The mod's `PIC/1.PAL` differs from the original only in entries 64..127, which hold the
    /// crest colours in all three palettes; no original picture uses an entry above 31.
    #[test]
    fn build_writes_the_crest_colours_into_the_palette() {
        let Some(data) = bmp_formats::data_dir() else {
            return;
        };
        let dir = temp("palette");
        let out = dir.join("out");
        build(&data, &out).unwrap();
        let original = pal::PalFile::load(&data).unwrap();
        let modded = pal::PalFile::load(&out).unwrap();
        let colours = crest_colours().unwrap();
        let crest_entries = CREST_COLOUR_FIRST..CREST_COLOUR_FIRST + CREST_COLOUR_COUNT;
        for (o, m) in original.palettes.iter().zip(&modded.palettes) {
            for i in 0..pal::PALETTE_ENTRIES {
                if crest_entries.contains(&i) {
                    assert_eq!(m.entries[i], colours[i - CREST_COLOUR_FIRST], "entry {i}");
                } else {
                    assert_eq!(m.entries[i], o.entries[i], "entry {i}");
                }
            }
        }
        for n in 0..=255u8 {
            let Ok(bytes) = fs::read(data.join(vga::file_name(n))) else {
                continue;
            };
            let Ok(picture) = vga::parse(&bytes) else {
                continue;
            };
            assert!(
                picture.pixels.iter().all(|&p| p < 32),
                "{n}.VGA uses an entry above 31"
            );
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn build_is_deterministic() {
        let dir = temp("twice");
        let from = dir.join("from");
        fs::create_dir_all(from.join("PIC")).unwrap();
        fs::write(from.join("MANA.DAT"), zeroed_mana()).unwrap();
        fs::write(from.join("PIC").join("80.VGA"), [4, 5]).unwrap();
        let sheet = vga::encode(&synthetic_icon_sheet());
        fs::write(from.join(vga::file_name(ICON_SHEET)), &sheet).unwrap();
        build(&from, &dir.join("a")).unwrap();
        build(&from, &dir.join("b")).unwrap();
        let a = tree(&dir.join("a"));
        // MANA.DAT, the marker, the icon sheet and the crests; PIC/80.VGA is one of them.
        assert_eq!(a.len(), 3 + CRESTS.len());
        assert_eq!(a, tree(&dir.join("b")));
        assert_ne!(
            fs::read(dir.join("a").join(vga::file_name(ICON_SHEET))).unwrap(),
            sheet
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn build_refuses_unsafe_targets_and_writes_nothing() {
        let dir = temp("refuse");
        let from = dir.join("from");
        fs::create_dir_all(&from).unwrap();
        fs::write(from.join("MANA.DAT"), [1]).unwrap();
        let before = tree(&dir);
        let play_dir_existed = repo_play_dir().exists();

        assert!(build(&from, &from).is_err());
        assert!(build(&from, &from.join("sub")).is_err());
        assert!(build(&from, &repo_play_dir()).is_err());
        assert!(build(&from, &repo_play_dir().join("sub")).is_err());
        let full = dir.join("full");
        fs::create_dir_all(&full).unwrap();
        fs::write(full.join("keep"), [9]).unwrap();
        assert!(build(&from, &full).is_err());

        assert_eq!(tree(&full), [(PathBuf::from("keep"), vec![9])]);
        fs::remove_dir_all(&full).unwrap();
        assert_eq!(tree(&dir), before);
        assert_eq!(repo_play_dir().exists(), play_dir_existed);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn play_dir_follows_the_main_checkout() {
        let manifest = Path::new("/w/tree/crates/bmp-game");
        // A worktree: the common git directory is the main checkout's `.git`.
        assert_eq!(
            play_dir_of(manifest, Some(Path::new("/m/main/.git"))),
            Path::new("/m/main/BMP")
        );
        // The main checkout itself: still `<repo>/BMP`.
        assert_eq!(
            play_dir_of(
                Path::new("/m/main/crates/bmp-game"),
                Some(Path::new("/m/main/.git"))
            ),
            Path::new("/m/main/BMP")
        );
        // No git metadata: next to the crate.
        assert_eq!(
            play_dir_of(manifest, None),
            Path::new("/w/tree/crates/bmp-game/../../BMP")
        );
    }

    #[test]
    fn repo_play_dir_matches_the_check_script() {
        // `tools/check.sh`: dirname of `git rev-parse --path-format=absolute --git-common-dir`.
        let Ok(output) = std::process::Command::new("git")
            .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
        else {
            return;
        };
        if !output.status.success() {
            return;
        }
        let common = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
        assert_eq!(repo_play_dir(), common.parent().unwrap().join("BMP"));
    }

    #[test]
    fn build_from_a_worktree_refuses_the_main_play_directory() {
        let dir = temp("worktree");
        let from = dir.join("from");
        fs::create_dir_all(&from).unwrap();
        fs::write(from.join("MANA.DAT"), zeroed_mana()).unwrap();
        let main_git = dir.join("main").join(".git");
        fs::create_dir_all(&main_git).unwrap();
        let worktree = dir.join("worktree").join("crates").join("bmp-game");
        fs::create_dir_all(&worktree).unwrap();
        let before = tree(&dir);

        let play_dir = play_dir_of(&worktree, Some(&main_git));
        assert_eq!(play_dir, dir.join("main").join("BMP"));
        let error = build_guarding(&from, &play_dir.join("sub"), &play_dir).unwrap_err();
        assert!(
            error.contains("is the play directory") && error.contains("never written"),
            "{error}"
        );
        assert!(build_guarding(&from, &play_dir, &play_dir).is_err());
        assert!(!play_dir.exists());
        assert_eq!(tree(&dir), before);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn build_resolves_symbolic_links_before_refusing() {
        let dir = temp("link");
        let from = dir.join("from");
        fs::create_dir_all(&from).unwrap();
        fs::write(from.join("MANA.DAT"), [1]).unwrap();
        std::os::unix::fs::symlink(&from, dir.join("link")).unwrap();
        let before = tree(&dir);

        assert!(build(&from, &dir.join("link").join("sub")).is_err());

        assert_eq!(tree(&dir), before);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn copies_of_read_only_files_are_writable() {
        let dir = temp("readonly");
        let from = dir.join("from");
        fs::create_dir_all(&from).unwrap();
        let source = from.join("MANA.DAT");
        fs::write(&source, zeroed_mana()).unwrap();
        let mut permissions = fs::metadata(&source).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&source, permissions.clone()).unwrap();

        let out = dir.join("out");
        let result = build(&from, &out);
        #[expect(
            clippy::permissions_set_readonly_false,
            reason = "restores the test file"
        )]
        permissions.set_readonly(false);
        fs::set_permissions(&source, permissions).unwrap();

        result.unwrap();
        fs::write(out.join("MANA.DAT"), [2]).unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn build_refuses_an_unparsable_mana_dat_and_writes_nothing() {
        let dir = temp("unparsable");
        let from = dir.join("from");
        fs::create_dir_all(&from).unwrap();
        fs::write(from.join("MANA.DAT"), [1, 2, 3]).unwrap();
        let out = dir.join("out");

        assert!(build(&from, &out).is_err());

        assert!(!out.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Slot 40 (HANSA ROSTOCK, 3. Liga, rank 5, W/D/L 18/13/7, goals 74:49) gets its strength
    /// from its table with the 3. Liga group's base 20 and size 20, not from random numbers;
    /// one club of each other group pins that group's base and size.
    #[test]
    fn mod_clubs_get_table_based_strengths() {
        let mut m = ManaDat::parse(&zeroed_mana()).unwrap();
        write_mod_clubs(&mut m).unwrap();
        assert_eq!(m.clubs[40].name_bytes(), "FC HANSA ROSTOCK".as_bytes());
        // Group averages: points 39, scored 63, conceded 55; the rank term is 20 - 5.
        assert_eq!(m.strength[40], [37, 37, 37]);
        // Slot 9 (FSV MAINZ 05, Bundesliga): base 70, size 18, rank 10, averages 38/58/50.
        assert_eq!(m.strength[9], [80, 80, 79]);
        // Slot 25 (1.FC NÜRNBERG, 2. Bundesliga): base 34, size 18, rank 8, averages
        // 33/50/53; [46, 46, 46] scaled by 125/100.
        assert_eq!(m.strength[25], [57, 57, 57]);
        // Slot 57 (1.FC LOK LEIPZIG, pool): base 20, size 20, rank 1, averages 42/60/44.
        assert_eq!(m.strength[57], [42, 40, 40]);

        m.rank[40] = 15;
        let mut rng = Rng::default();
        strength::recompute_strengths_grouped(&mut m, &mut rng, &strength::MOD2026);
        assert_eq!(m.strength[40], [27, 27, 27]);
        assert_eq!(rng, Rng::default());
    }

    /// A squad of fewer than 20 players clears the remaining player fields, one of more than
    /// 20 is refused.
    #[test]
    fn short_squads_leave_empty_player_fields() {
        let mut m = ManaDat::parse(&zeroed_mana()).unwrap();
        m.clubs[0].players = vec![[b'X'; PLAYER_NAME_LEN]; PLAYERS_PER_CLUB];
        let clubs = dataset(CLUBS_JSON, "clubs.json").unwrap();
        let squad = |n: usize| {
            let players = (0..n).map(|k| serde_json::json!({ "name": format!("P{k}") }));
            serde_json::json!({ "players": players.collect::<Vec<_>>() })
        };

        write_club(&mut m, 0, &clubs[0], &squad(3)).unwrap();
        assert_eq!(&m.clubs[0].players[2][..3], b"P2\0");
        assert!(
            m.clubs[0].players[3..]
                .iter()
                .all(|p| *p == [0; PLAYER_NAME_LEN])
        );

        assert!(write_club(&mut m, 0, &clubs[0], &squad(PLAYERS_PER_CLUB + 1)).is_err());
    }
}
