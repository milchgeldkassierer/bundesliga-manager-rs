#!/usr/bin/env bash
# Tests tools/prepare-public.sh on a throwaway repository with made-up identities, screenshots
# and research data planted in its history. Needs git filter-repo.
set -euo pipefail
# Git exports GIT_DIR and the like to hooks, and the pre-push hook runs this test; left set, the
# git calls below would rewrite that repository instead of the throwaway one.
mapfile -t git_env < <(git rev-parse --local-env-vars)
unset "${git_env[@]}"
export LC_ALL=C.UTF-8
tools_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# A pip --user install of git filter-repo lives under the real home.
PYTHONUSERBASE=$(python3 -m site --user-base)
export PYTHONUSERBASE
export HOME="$work/home"
export GIT_CONFIG_NOSYSTEM=1
mkdir -p "$HOME/.config"
failures=0
fail() { echo "FAIL: $*" >&2; failures=$((failures + 1)); }

old_name='Jörg Exämple'
old_email='joerg@example.org'
other_old_email='jexample@old.example.org'
public_name='public-maintainer'
public_email='12345+public-maintainer@users.noreply.github.com'
cat > "$HOME/.config/bmp-public-mailmap" <<EOF
$public_name <$public_email> <$other_old_email>
$public_name <$public_email> $old_name <$old_email>
$public_name <$public_email> Paul Huebner-Schäfer <paul@old.example.org>
EOF

commit_as() { # <name> <email> <message> <files...>
  local name=$1 email=$2 message=$3; shift 3
  git -C "$src" add "$@"
  GIT_AUTHOR_NAME=$name GIT_AUTHOR_EMAIL=$email GIT_COMMITTER_NAME=$name GIT_COMMITTER_EMAIL=$email \
    git -C "$src" commit --quiet -m "$message"
}

src="$work/src"
git init --quiet --initial-branch=main "$src"
git -C "$src" remote add origin https://example.invalid/never-used.git
mkdir -p "$src/tools" "$src/docs/screenshots" "$src/docs/mod2026/research" "$src/mods"
cp "$tools_dir/prepare-public.sh" "$tools_dir/public-copy.py" "$tools_dir/public-exclude.txt" "$src/tools/"
echo 'mods/OWN.DAT' > "$src/tools/public-allow.txt"
printf '# Test project\n\nSome text.\n' > "$src/README.md"
echo 'png' > "$src/docs/screenshots/title.png"
# The second name is spelled out in the mailmap, with umlauts here, and the other way round.
printf 'Maintained by JÖRG EXAMPLE <Joerg@Example.org>\nCredits: Paul Hübner-Schafer\n' > "$src/NOTES.txt"
commit_as "$old_name" "$old_email" 'Start the project' tools README.md docs NOTES.txt
echo '{"value": 1}' > "$src/docs/mod2026/research/values.json"
echo 'mod data' > "$src/mods/OWN.DAT"
commit_as 'Joerg Example' "$other_old_email" "Add research and mod data

Written with joerg example.

Signed-off-by: $old_name <$old_email>
Co-Authored-By: Helper <noreply@anthropic.com>" docs mods
GIT_COMMITTER_NAME=$old_name GIT_COMMITTER_EMAIL=$old_email \
  git -C "$src" tag -a v1.0.0 -m "Release by $old_name"
git -C "$src" checkout --quiet -b mod_2026
echo 'more' >> "$src/docs/screenshots/title.png"
commit_as "$old_name" "$old_email" "Mod work for $old_email" docs
git -C "$src" checkout --quiet main

state() { git -C "$src" status --porcelain; git -C "$src" for-each-ref; git -C "$src" remote -v; }
before=$(state)

out="$work/out"
if ! report=$("$src/tools/prepare-public.sh" "$out" 2>&1); then
  echo "$report" >&2
  echo "FAIL: prepare-public.sh failed on the planted repository" >&2
  exit 1
fi
[ "$(state)" = "$before" ] || fail "the source repository changed"

git -C "$out" rev-list --all --objects | cut -d' ' -f1 | git -C "$out" cat-file --batch > "$work/objects"
if grep -aqiE 'j(o|ö|oe)rg|ex(a|ä|ae)mple|h(u|ü|ue)bner|old\.example' "$work/objects"; then
  fail "an old identity string survived: $(grep -aoiE '.{0,20}(j(o|ö|oe)rg|ex(a|ä|ae)mple|h(u|ü|ue)bner).{0,20}' "$work/objects" | head -n3)"
fi
identities=$(git -C "$out" log --all --format='%an <%ae>%n%cn <%ce>'; \
  git -C "$out" for-each-ref --format='%(taggername) %(taggeremail)' refs/tags)
[ "$(sort -u <<<"$identities")" = "$public_name <$public_email>" ] \
  || fail "unexpected identities: $(sort -u <<<"$identities" | tr '\n' ';')"
grep -aq "Signed-off-by: $public_name <$public_email>" "$work/objects" \
  || fail "the Signed-off-by trailer was not mapped"
paths=$(git -C "$out" rev-list --all --objects | cut -s -d' ' -f2-)
if grep -qE '^docs/(screenshots|mod2026/research)' <<<"$paths"; then
  fail "an excluded path survived"
fi
grep -qx 'mods/OWN.DAT' <<<"$paths" || fail "the allow-listed mod file was dropped"
[ "$(git -C "$out" for-each-ref --format='%(refname)' | tr '\n' ' ')" = \
  "refs/heads/main refs/heads/mod_2026 refs/tags/v1.0.0 " ] || fail "unexpected refs in the copy"
[ -z "$(git -C "$out" remote)" ] || fail "the copy has a remote"
for branch in main mod_2026; do
  git -C "$out" show "$branch:README.md" | grep -q 'not affiliated with Software 2000' \
    || fail "no disclaimer in the README of $branch"
done
grep -q '0 findings' <<<"$report" || fail "no clean summary: $report"

if "$src/tools/prepare-public.sh" "$out" >/dev/null 2>&1; then fail "an existing out-dir was accepted"; fi

git -C "$src" checkout --quiet mod_2026
echo 'original' > "$src/mods/GAME.DAT"
commit_as 'Stranger' 'stranger@example.net' 'Add a game file' mods
if report=$("$src/tools/prepare-public.sh" "$work/bad" 2>&1); then
  fail "the verification passed an unmapped identity and an original-data file"
fi
grep -q 'original-data file in the history: mods/GAME.DAT' <<<"$report" \
  || fail "the original-data file was not reported"
grep -q 'unmapped identity in commit' <<<"$report" || fail "the unmapped identity was not reported"

# The verification alone, on the unfiltered source: it must catch what the filter would remove.
cp "$src/mods/OWN.DAT" "$src/mods/COPY.DAT"
echo 'vga data' > "$src/mods/OLD.VGA"
echo 'Credits: Joerg Example' > "$src/MIXED.txt"
echo 'Credits: Paul Hübner-Schafer' > "$src/SPELLED.txt"
printf 'Data under /home/someone/game.\nAsk stranger@example.net.\n' > "$src/PATHS.txt"
commit_as "$public_name" "$public_email" 'Add copies' mods MIXED.txt SPELLED.txt PATHS.txt
git -C "$src" mv mods/OLD.VGA mods/old.bin
commit_as "$public_name" "$public_email" 'Rename the data

Signed-off-by: Other Person <other@example.net>' mods
trailer_commit=$(git -C "$src" rev-parse HEAD)
mixed_blob=$(git -C "$src" rev-parse HEAD:MIXED.txt)
spelled_blob=$(git -C "$src" rev-parse HEAD:SPELLED.txt)
if report=$(python3 "$src/tools/public-copy.py" verify "$src" "$HOME/.config/bmp-public-mailmap" \
    "$src/tools/public-exclude.txt" "$src/tools/public-allow.txt" 2>&1); then
  fail "the verification passed the unfiltered source"
fi
for expected in \
  'FINDING: excluded path in the history: docs/screenshots/title.png' \
  'FINDING: excluded path in the history: docs/mod2026/research/values.json' \
  'FINDING: original-data file in the history: mods/COPY.DAT' \
  'FINDING: original-data file in the history: mods/OLD.VGA' \
  "FINDING: unmapped identity in commit $trailer_commit: Other Person <other@example.net>" \
  'warning: local path in mod_2026:PATHS.txt:1:' \
  'warning: e-mail address in mod_2026:PATHS.txt:2:'; do
  grep -qF "$expected" <<<"$report" || fail "the verification did not report: $expected"
done
grep -qE "^FINDING: old identity string #[0-9]+ of the mailmap in blob $mixed_blob" <<<"$report" \
  || fail "the verification missed a mixed spelling of the old name"
grep -qE "^FINDING: old identity string #[0-9]+ of the mailmap in blob $spelled_blob" <<<"$report" \
  || fail "the verification missed an umlaut spelling of a spelled-out old name"

mkdir "$work/norepo"
if report=$(GIT_CEILING_DIRECTORIES=$work python3 "$src/tools/public-copy.py" verify "$work/norepo" \
    "$HOME/.config/bmp-public-mailmap" "$src/tools/public-exclude.txt" "$src/tools/public-allow.txt" 2>&1)
then
  fail "the verification passed a directory that is no repository"
fi
grep -q '^prepare-public: git .* failed: ' <<<"$report" || fail "no git error reported: $report"

rm "$HOME/.config/bmp-public-mailmap"
if report=$("$src/tools/prepare-public.sh" "$work/nomap" 2>&1); then fail "ran without a mailmap"; fi
grep -q 'no mailmap at' <<<"$report" || fail "no clear message for a missing mailmap: $report"
[ ! -e "$work/nomap" ] || fail "an out-dir was written without a mailmap"

[ "$failures" -eq 0 ] || exit 1
echo "prepare-public test passed"
