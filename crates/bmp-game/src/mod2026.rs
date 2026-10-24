//! Mod 2026 (`docs/mod2026/PRD.md`): the generator of the mod data directory and the switch
//! that tells mod data from the original data.
//!
//! Not part of the original program: everything here is mod-only (`docs/mod2026/CHANGES.md`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The marker file whose presence in the data directory switches mod behaviour on.
pub const MARKER: &str = "MOD2026.DAT";

/// Fixed content of [`MARKER`], so that two builds of the same input are byte-identical.
const MARKER_CONTENT: &[u8] = b"Mod 2026 data directory, written by `bmp mod2026 build`.\n";

/// Whether `data_dir` holds mod data, i.e. contains [`MARKER`].
pub fn is_active(data_dir: &Path) -> bool {
    data_dir.join(MARKER).is_file()
}

/// Writes the mod data directory `out`: a copy of every file below `from`, then [`MARKER`].
///
/// Refuses, before anything is created or written, an `out` that is `from` or lies inside
/// it, one that is or lies inside the owner's play directory `<repo>/BMP`, and one that
/// already holds files. The copies are written as new files, so they do not inherit the
/// read-only permissions of the pristine copy.
pub fn build(from: &Path, out: &Path) -> Result<(), String> {
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
    let play_dir = resolve(&repo_play_dir()).map_err(|e| e.to_string())?;
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
    copy_tree(&source, &target).map_err(|e| format!("{}: {e}", out.display()))?;
    fs::write(target.join(MARKER), MARKER_CONTENT).map_err(|e| format!("{}: {e}", out.display()))
}

/// `<repo>/BMP` of the checkout this crate was built from.
fn repo_play_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../BMP")
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
fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
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
        for file in &original {
            assert!(copied.contains(file), "{} not copied", file.0.display());
        }
        assert!(is_active(&out));
        assert_eq!(
            copied.len(),
            original.len() + usize::from(!is_active(&data))
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn build_is_deterministic() {
        let dir = temp("twice");
        let from = dir.join("from");
        fs::create_dir_all(from.join("PIC")).unwrap();
        fs::write(from.join("MANA.DAT"), [1, 2, 3]).unwrap();
        fs::write(from.join("PIC").join("80.VGA"), [4, 5]).unwrap();
        build(&from, &dir.join("a")).unwrap();
        build(&from, &dir.join("b")).unwrap();
        let a = tree(&dir.join("a"));
        assert_eq!(a.len(), 3);
        assert_eq!(a, tree(&dir.join("b")));
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
        fs::write(&source, [1]).unwrap();
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
}
