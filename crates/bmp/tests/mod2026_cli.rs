//! `bmp mod2026 build` as seen from the command line.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use bmp_formats::font::encode_cp437;
use bmp_formats::mana::{self, ManaDat};

fn bmp<const N: usize>(args: [&OsStr; N]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bmp"))
        .args(args)
        .output()
        .unwrap()
}

/// `bmp mod2026 build --from <from> --out <out>`.
fn build(from: &Path, out: &Path) -> Output {
    let [cmd, sub, from_flag, out_flag] = ["mod2026", "build", "--from", "--out"].map(OsStr::new);
    bmp([
        cmd,
        sub,
        from_flag,
        from.as_os_str(),
        out_flag,
        out.as_os_str(),
    ])
}

/// The owner's play directory `<main checkout>/BMP`, derived like `tools/check.sh` does (next
/// to the common git directory, also from a worktree), else `<repo>/BMP` of this checkout.
fn play_dir() -> PathBuf {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .filter(|output| output.status.success());
    match output {
        Some(output) => Path::new(String::from_utf8(output.stdout).unwrap().trim())
            .parent()
            .unwrap()
            .join("BMP"),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../BMP"),
    }
}

/// A fresh directory below the system temp directory holding a single, zeroed `MANA.DAT`.
fn source(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bmp-mod2026-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("MANA.DAT"), vec![0; mana::FILE_SIZE]).unwrap();
    dir
}

/// Asserts that `output` is a refusal with exit code 2 whose message contains `reason`.
fn assert_refused(output: &Output, reason: &str) {
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(reason), "unexpected message: {stderr}");
}

#[test]
fn help_lists_the_build_command() {
    let help = bmp(["--help".as_ref()]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("mod2026 build --from"));
}

#[test]
fn help_lists_lowres() {
    let help = bmp(["--help".as_ref()]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--lowres"));
}

/// `run` and `teamed` take `--lowres`: they reach the missing script instead of refusing the
/// option. Passing the flag on to the window needs a display and is not tested here.
#[test]
fn run_and_teamed_accept_lowres() {
    let script = std::env::temp_dir().join(format!("bmp-lowres-missing-{}", std::process::id()));
    for cmd in ["run", "teamed"] {
        let [cmd, data, script_flag, lowres] =
            [cmd, "--data", "--script", "--lowres"].map(OsStr::new);
        let output = bmp([
            data,
            script.as_os_str(),
            cmd,
            script_flag,
            script.as_os_str(),
            lowres,
        ]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success());
        assert!(!stderr.contains("unexpected argument"), "{stderr}");
        assert!(stderr.contains(&*script.to_string_lossy()), "{stderr}");
    }
}

#[test]
fn build_writes_the_copy_and_the_marker() {
    let from = source("build");
    let out = from.with_extension("out");
    let _ = std::fs::remove_dir_all(&out);

    let output = build(&from, &out);

    assert!(output.status.success(), "{output:?}");
    let mana = ManaDat::parse(&std::fs::read(out.join("MANA.DAT")).unwrap()).unwrap();
    assert_eq!(
        mana.clubs[0].name_bytes(),
        encode_cp437("FC BAYERN MÜNCHEN")
    );
    assert!(out.join("MOD2026.DAT").is_file());
    std::fs::remove_dir_all(&from).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
}

#[test]
fn build_refuses_the_play_directory_and_the_source() {
    let dir = source("refuse");
    let play_dir = play_dir();
    let play_dir_existed = play_dir.exists();

    assert_refused(&build(&dir, &play_dir), "play directory");
    assert_refused(&build(&dir, &play_dir.join("sub")), "play directory");
    assert_refused(&build(&dir, &dir), "lies inside");

    assert_eq!(play_dir.exists(), play_dir_existed);
    let files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into())
        .collect();
    assert_eq!(files, [PathBuf::from("MANA.DAT")]);
    std::fs::remove_dir_all(&dir).unwrap();
}
