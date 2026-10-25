# Public copy

This repository stays private and is never cleaned (MISSION.md, "Publication"). A public release
is a **new repository** made from a cleaned copy of the history by `tools/prepare-public.sh`.

## Making the copy

```
tools/prepare-public.sh <out-dir>
```

Needs [git filter-repo](https://github.com/newren/git-filter-repo) (`pip install --user
git-filter-repo` or the distribution's package) and the mailmap
`$HOME/.config/bmp-public-mailmap`. The mailmap is kept outside the repository because its old
side holds the identities that must not be published; it uses the
[git mailmap format](https://git-scm.com/docs/gitmailmap), one line per old identity, e.g.

```
public-name <12345+public-name@users.noreply.github.com> Old Name <old@example.org>
public-name <12345+public-name@users.noreply.github.com> <other-old@example.org>
```

The script reads this repository only: it fetches the branches `main` and `mod_2026` and every
tag into the new directory `<out-dir>` (which must not exist), and the copy gets no remote. In
the copy it then

1. rewrites the whole history with git filter-repo:
   - drops every path listed in `tools/public-exclude.txt` (screenshots of original game graphics,
     raw research data; plain paths only, no `glob:` or `regex:` lines, because the verification
     compares them literally);
   - maps every author, committer and tagger through the mailmap;
   - replaces every old name and e-mail of the mailmap with its public one in commit and tag
     messages (`Co-Authored-By`, `Signed-off-by` and free text) and in file contents, in any
     case, with or without accents and with each umlaut written either way (`ü` / `ue`), whichever
     way the mailmap writes it;
2. commits the disclaimer (not affiliated with Software 2000, the DFL, the DFB or any club; no
   original game data included; you need your own copy of the game) under the README's title on
   both branches, as the mailmap's first public identity;
3. verifies every commit, tag and file of the copy (`tools/public-copy.py verify`) and exits
   non-zero on any **finding**:
   - a file with an extension of the original game data (`.VGA .CP .PAL .MAN .DAT .VOC .CMF
     .EXE .T .V`, any case) that is not listed in `tools/public-allow.txt`;
   - a path of `tools/public-exclude.txt`;
   - an author, committer, tagger or `...-by:` trailer whose e-mail is neither a public e-mail of
     the mailmap nor a noreply address;
   - any old name or e-mail of the mailmap (any case, accents and umlaut spellings folded) in a
     file, commit message or tag message;

   and reports as **warnings**, with branch, file and line, what a human has to judge in the
   branch heads: local paths (`/home/`, `/media/`) and e-mail addresses other than the public and
   noreply ones. A summary closes the report.

A finding means the copy must not be published: fix the cause (extend the mailmap or the exclude
list) and make a new copy. Warnings do not stop the script. Whenever the script exits non-zero,
`<out-dir>` may hold an unfinished or unverified copy: never push it, delete it before the next
run.

Known gaps in the copy: the README of both branches embeds the screenshots of `docs/screenshots/`, and on
`mod_2026` the tests `crates/bmp-game/tests/mod2026_clubs.rs` and `mod2026_squads.rs` include files of
`docs/mod2026/research/`, and `crates/bmp/tests/mod2026_playthrough.rs` compares with the golden frames
of `tests/mod2026/golden/` (port screenshots showing the game's artwork); in the copy those images are
missing, the two research tests do not build and the playthrough test fails when `BMP_DATA` is set.

## Publishing (later)

1. Create a new, empty repository on GitHub (no README, licence or `.gitignore`, so the first
   push does not conflict).
2. In the copy: `git remote add origin <url>`, then
   `git push origin main mod_2026 --tags`.
3. Set the default branch on GitHub (`main`, the 1:1 port).

Only the git history moves over. Issues, pull requests, releases, wiki pages and other GitHub
settings of this repository stay here; recreate on the new repository what the public one needs
(for example a release for the tag `v1.0.0`).
