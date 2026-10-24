//! Adds messages to the inbox of manager 0 of a `.MAN` save, for visual scenarios that need
//! more mail than a played game holds (`tools/visual-check.sh`, `# fixture-patch:`).
//!
//! Usage: `save_add_messages <file.MAN> <messages.txt> [--unread]`
//!
//! Every non-empty line of `messages.txt` that does not start with `#` is one message in the
//! game's format (`^` ends a line, the first line is the date); they are inserted in file
//! order at the front of the inbox, so the last line becomes the newest message. `--unread`
//! sets the new-message flag `FAR:56e2[0]`, which opens the inbox when the office is drawn.
//! The file is rewritten in place with the original's checksums.

use std::process::ExitCode;

use bmp_formats::savegame::{SaveGame, Segment, StringEntry};

/// Segment of the made-up far pointers of the added strings: far above every real heap
/// pointer, so the loader's pointer fix-up (`FAR:774a`, `FAR:5664`) never matches them.
const POINTER_SEGMENT: u32 = 0x7f00_0000;

fn run(args: &[String]) -> Result<(), String> {
    let [path, list, rest @ ..] = args else {
        return Err("usage: save_add_messages <file.MAN> <messages.txt> [--unread]".into());
    };
    let unread = match rest {
        [] => false,
        [flag] if flag == "--unread" => true,
        _ => return Err(format!("unexpected arguments {rest:?}")),
    };
    let data = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let mut save = SaveGame::parse(&data).map_err(|e| format!("{path}: {e:?}"))?;
    let text = std::fs::read(list).map_err(|e| format!("{list}: {e}"))?;
    let group = save
        .strings
        .first_mut()
        .ok_or("the save has no message group")?;
    let mut added = 0u8;
    for line in text.split(|&c| c == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() || line.starts_with(b"#") {
            continue;
        }
        let mut bytes = line.to_vec();
        bytes.push(0);
        if bytes.len() > 0x7f {
            return Err(format!(
                "message too long: {}",
                String::from_utf8_lossy(line)
            ));
        }
        added += 1;
        group.insert(
            0,
            StringEntry {
                old_pointer: POINTER_SEGMENT | (u32::from(added) * 0x80),
                bytes,
            },
        );
    }
    let counts = save
        .block_mut(Segment::Dgroup, 0x0618)
        .ok_or("no DS:0618 block")?;
    counts[0] = counts[0]
        .checked_add(added)
        .filter(|&n| n <= 20)
        .ok_or("more than 20 messages")?;
    if unread {
        save.block_mut(Segment::FarData, 0x56e2)
            .ok_or("no FAR:56e2 block")?[0] = 1;
    }
    let out = save.to_bytes().map_err(|e| format!("{e:?}"))?;
    std::fs::write(path, out).map_err(|e| format!("{path}: {e}"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("save_add_messages: {e}");
            ExitCode::FAILURE
        }
    }
}
