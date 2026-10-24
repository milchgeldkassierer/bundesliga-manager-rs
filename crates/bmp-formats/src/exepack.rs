//! Microsoft EXEPACK decompression for the original MZ executables.
//!
//! `BMMAIN.EXE`, `BMLOADER.EXE` and `TEAMED.EXE` are EXEPACK-compressed; code and data
//! (fonts, tables, strings) have to be read from the unpacked load image.

use std::fmt;

/// The executable is not an MZ file or not EXEPACK-compressed as expected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExepackError(pub &'static str);

impl fmt::Display for ExepackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bad executable: {}", self.0)
    }
}

impl std::error::Error for ExepackError {}

fn rd16(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(off)?, *b.get(off + 1)?]))
}

/// Unpacks a Microsoft EXEPACK-compressed MZ executable and returns its load image
/// (offset 0 = load segment 0). Relocations are not applied; segment values inside the
/// image stay relative to load segment 0.
///
/// The packed image is decompressed backwards from its end: each command is read
/// backwards as a byte opcode followed by a 16-bit count; `0xB0/0xB1` fill `count`
/// copies of the next (preceding) byte, `0xB2/0xB3` copy `count` bytes; bit 0 marks the
/// last command. Trailing `0xFF` padding is skipped first. The untouched leading part of
/// the packed image is already the start of the unpacked image.
pub fn unpack(exe: &[u8]) -> Result<Vec<u8>, ExepackError> {
    if exe.len() < 0x1c || &exe[0..2] != b"MZ" {
        return Err(ExepackError("missing MZ signature"));
    }
    let last_page = rd16(exe, 2).unwrap_or(0) as usize;
    let pages = rd16(exe, 4).unwrap_or(0) as usize;
    let header = rd16(exe, 8).unwrap_or(0) as usize * 16;
    let cs = rd16(exe, 0x16).unwrap_or(0) as usize;
    let mut file_size = pages * 512;
    if last_page != 0 {
        file_size = file_size.saturating_sub(512 - last_page);
    }
    let file_size = file_size.min(exe.len());
    if header >= file_size {
        return Err(ExepackError("header larger than file"));
    }
    let image = &exe[header..file_size];
    let eh = cs * 16;
    let sig_ok = |o: usize| image.get(eh + o..eh + o + 2) == Some(&b"RB"[..]);
    if !(sig_ok(14) || sig_ok(16)) {
        return Err(ExepackError("no EXEPACK 'RB' signature"));
    }
    let dest_len = rd16(image, eh + 12).ok_or(ExepackError("short EXEPACK header"))? as usize * 16;
    let packed = &image[..eh];
    if dest_len < packed.len() {
        return Err(ExepackError("EXEPACK destination smaller than source"));
    }
    let mut out = vec![0u8; dest_len];
    out[..packed.len()].copy_from_slice(packed);
    let mut src = packed.len();
    while src > 0 && packed[src - 1] == 0xff {
        src -= 1;
    }
    let mut dst = dest_len;
    let err = || ExepackError("corrupt EXEPACK stream");
    loop {
        if src < 3 {
            return Err(err());
        }
        let op = packed[src - 1];
        let count = u16::from_le_bytes([packed[src - 3], packed[src - 2]]) as usize;
        src -= 3;
        match op & 0xfe {
            0xb0 => {
                if src < 1 || dst < count {
                    return Err(err());
                }
                let fill = packed[src - 1];
                src -= 1;
                out[dst - count..dst].fill(fill);
                dst -= count;
            }
            0xb2 => {
                if src < count || dst < count {
                    return Err(err());
                }
                out[dst - count..dst].copy_from_slice(&packed[src - count..src]);
                src -= count;
                dst -= count;
            }
            _ => return Err(err()),
        }
        if op & 1 != 0 {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_mz() {
        assert!(unpack(b"not an exe at all, definitely not").is_err());
    }

    #[test]
    fn unpacks_original_executables() {
        let Some(dir) = crate::data_dir() else {
            return;
        };
        for name in ["BMMAIN.EXE", "BMLOADER.EXE", "TEAMED.EXE"] {
            let exe = std::fs::read(dir.join(name)).unwrap();
            let img = unpack(&exe).unwrap();
            assert!(img.len() > exe.len() / 2, "{name}: image too small");
        }
    }
}
