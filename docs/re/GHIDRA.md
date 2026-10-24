# Ghidra project

Project `ghidra_proj/BMP` (not in git), served by `tools/ghidra-headless.sh` (GhidraMCP 7.0.0 on Ghidra 12.2, JDK 25).

`BMMAIN.EXE`, `BMLOADER.EXE` and `TEAMED.EXE` are compressed with Microsoft EXEPACK. The programs in the
project root are the packed files and only useful for the small uncompressed prefix. **Use the unpacked
programs** (load segment 0x1000, Ghidra segment = image segment + 0x1000):

| Program | Canonical Ghidra path | Notes |
|---|---|---|
| BMMAIN | `/unpacked/BMMAIN_unpacked.EXE` | Two relocation bytes patched by hand (see FUNCTION_MAP.md); most renames live here |
| TEAMED | `/unpacked/TEAMED_unpacked.EXE` | Same relocation patch |
| BMLOADER | `/unpacked/BMLOADER_UNPACKED.EXE` | |
| BMINTRO | `/BMINTRO.EXE` | Not packed, no C runtime |

`/unpacked/funcmap_duplicates/` holds older duplicate imports; some early renames (palette routines) were made
there and may still need to be copied to the canonical programs.

The Rust decompressor is `bmp_formats::exepack::unpack`.
