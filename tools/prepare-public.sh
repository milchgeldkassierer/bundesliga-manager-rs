#!/usr/bin/env bash
# Writes a cleaned, publishable copy of this repository into <out-dir> (docs/PUBLIC.md).
#
# Usage: tools/prepare-public.sh <out-dir>
#
# The copy holds the branches main and mod_2026 and the tags, with the whole history rewritten
# by git filter-repo: the paths of tools/public-exclude.txt are dropped, and every identity is
# mapped through $HOME/.config/bmp-public-mailmap (authors, committers, taggers, and the same
# names and e-mails in commit and tag messages and file contents). Both branches get a commit
# with the README disclaimer. The copy is then verified; any finding exits non-zero.
# This repository is only read: nothing here is written, fetched into or pushed, and the copy
# has no remote.
set -euo pipefail
# Git exports GIT_DIR and the like to hooks (the pre-push hook runs tools/check.sh, which runs
# this script through its test); left set, they would aim every git call below at that repository.
mapfile -t git_env < <(git rev-parse --local-env-vars)
unset "${git_env[@]}"

usage() { echo "usage: $0 <out-dir>" >&2; exit 2; }
die() { echo "prepare-public: $*" >&2; exit 1; }

[ $# -eq 1 ] || usage
out=$1
tools_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
source_repo=$(git -C "$tools_dir" rev-parse --show-toplevel)
exclude_list="$tools_dir/public-exclude.txt"
allow_list="$tools_dir/public-allow.txt"
mailmap="$HOME/.config/bmp-public-mailmap"
branches=(main mod_2026)

git filter-repo --version >/dev/null 2>&1 \
  || die "git filter-repo is not installed; install it with 'pip install --user git-filter-repo' or your distribution's git-filter-repo package"
[ -f "$mailmap" ] \
  || die "no mailmap at $mailmap; it maps every old identity to its public one (git mailmap format, docs/PUBLIC.md)"
[ -f "$exclude_list" ] || die "missing $exclude_list"
[ -f "$allow_list" ] || die "missing $allow_list"
[ ! -e "$out" ] || die "$out already exists; give a new directory"
for branch in "${branches[@]}"; do
  git -C "$source_repo" rev-parse --verify --quiet "refs/heads/$branch" >/dev/null \
    || die "branch $branch is missing in $source_repo"
done

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
public_identity=$(python3 "$tools_dir/public-copy.py" mapping "$mailmap" "$tmp/replacements")
public_name=${public_identity%%$'\n'*}
public_email=${public_identity#*$'\n'}

echo "prepare-public: copying ${branches[*]} and the tags of $source_repo into $out"
git init --quiet --initial-branch=main "$out"
refspecs=()
for branch in "${branches[@]}"; do refspecs+=("refs/heads/$branch:refs/heads/$branch"); done
git -C "$out" fetch --quiet --no-tags --update-head-ok "$source_repo" "${refspecs[@]}" 'refs/tags/*:refs/tags/*'

echo "prepare-public: rewriting the history"
# --force: the copy is not a clone, so filter-repo cannot tell that it is fresh; it was made above.
git -C "$out" filter-repo --force --quiet \
  --invert-paths --paths-from-file "$exclude_list" \
  --mailmap "$mailmap" \
  --replace-message "$tmp/replacements" \
  --replace-text "$tmp/replacements" \
  --replace-refs delete-no-add

disclaimer='> **Disclaimer.** This project is not affiliated with Software 2000, the DFL, the DFB or
> any football club. No original game data is included; you need your own copy of the game.'
for branch in "${branches[@]}"; do
  git -C "$out" checkout --quiet --force "$branch"
  readme="$out/README.md"
  if [ -f "$readme" ]; then
    awk -v text="$disclaimer" '{ print } !done && /^# / { print ""; print text; done = 1 }
      END { if (!done) { print ""; print text } }' "$readme" > "$tmp/README.md"
    cat "$tmp/README.md" > "$readme"
  else
    printf '%s\n' "$disclaimer" > "$readme"
  fi
  git -C "$out" add README.md
  GIT_AUTHOR_NAME=$public_name GIT_AUTHOR_EMAIL=$public_email \
    GIT_COMMITTER_NAME=$public_name GIT_COMMITTER_EMAIL=$public_email \
    git -C "$out" -c commit.gpgsign=false commit --quiet -m "Add the public-release disclaimer to the README"
done
git -C "$out" checkout --quiet main

echo "prepare-public: verifying $out"
python3 "$tools_dir/public-copy.py" verify "$out" "$mailmap" "$exclude_list" "$allow_list"
echo "prepare-public: $out is ready (docs/PUBLIC.md describes the next steps)"
