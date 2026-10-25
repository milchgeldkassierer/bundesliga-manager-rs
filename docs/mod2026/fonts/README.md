# 2x fonts (Mod 2026)

The 2x versions of the game's glyph fonts for the 2x display layer (`docs/mod2026/PRD.md`, R5b).
With the display layer on, text drawn through the font routines uses these glyphs in the display
buffer; the logical 320x240 frame, every advance, every text width and every centring stay those
of the original fonts.

| File | Original font | Height |
| --- | --- | --- |
| `micro4.txt` | `micro4.font` | 12 (6) |
| `flow.txt` | `flow.font` | 16 (8) |
| `newbold.txt` | `newbold.font` | 32 (16) |

`normal.font` is a stub without glyph data that the game never draws with, so it has no 2x
version.

## How they are made

Each 2x glyph is the Scale2x (EPX) image of the original glyph. For a pixel `E` with the
neighbours `B` (up), `D` (left), `F` (right) and `H` (down), where pixels outside the glyph cell
count as clear:

```
if B != H and D != F:
    top-left     = D if D == B else E
    top-right    = F if B == F else E
    bottom-left  = D if D == H else E
    bottom-right = F if H == F else E
else:
    all four = E
```

The image of a `w x h` glyph is exactly `2w x 2h`, so every 2x advance and height is twice the
original's. Corners and diagonals get rounded; straight strokes stay as they are.

The files are generated and must not be edited by hand; a test checks that they equal the
generator output. Regenerate them with:

```
cargo run -p bmp-formats --example hires_fonts -- ~/Apps/bmp-pristine
```

The argument is a data directory with the original `BMMAIN.EXE`, which is only read.

## Format

Lines starting with `# ` are comments. `font <name>` names the original font and `height <n>`
gives the 2x cell height. Then one block per character code `0x20..=0x7e` (the glyph slots after
the game's character translation) and the euro block `0xd5` follow: `glyph 0xNN` and `height`
rows of `#` (set) and `.` (clear); the row length is the 2x advance. The program embeds the
files (`crates/bmp-formats/src/hires_font.rs`).

## Euro sign

After the glyph `0x7e` each file has the block `glyph 0xd5`: the 2x euro glyph, the Scale2x
image of the mod's 1x euro glyph (`crates/bmp-formats/data/euro_glyphs.txt`, byte code 0xd5,
see `docs/mod2026/CHANGES.md`). Like every other glyph it is exactly twice the 1x glyph's
advance and height. The 2x display uses it only when the mod's euro glyph is installed;
without it 0xd5 stays v1.0.0's space.
