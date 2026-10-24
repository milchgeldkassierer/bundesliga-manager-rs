#!/usr/bin/env bash
# Installs a pre-push hook that runs tools/check.sh on every pushed commit in a clean,
# temporary worktree, so uncommitted files never influence the result. Worktrees share
# the hooks directory, so factory pushes are gated too.
set -euo pipefail
hooks="$(git rev-parse --path-format=absolute --git-common-dir)/hooks"
cat > "$hooks/pre-push" <<'HOOK'
#!/usr/bin/env bash
set -euo pipefail
common="$(git rev-parse --path-format=absolute --git-common-dir)"
main_root="$(dirname "$common")"
zero=0000000000000000000000000000000000000000
while read -r _local_ref local_sha _remote_ref _remote_sha; do
  [ "$local_sha" = "$zero" ] && continue
  tmp="$(mktemp -d)"
  git worktree add --quiet --detach "$tmp/wt" "$local_sha"
  status=0
  CARGO_TARGET_DIR="$main_root/target/pre-push" BMP_DATA="${BMP_DATA:-$main_root/BMP}" \
    bash "$tmp/wt/tools/check.sh" || status=$?
  git worktree remove --force "$tmp/wt"; rm -rf "$tmp"
  if [ "$status" -ne 0 ]; then
    echo "pre-push: checks failed for $local_sha" >&2
    exit "$status"
  fi
done
HOOK
chmod +x "$hooks/pre-push"
echo "pre-push hook installed in $hooks"
