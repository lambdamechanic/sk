#!/usr/bin/env bash
set -euo pipefail

# Installs a Git pre-commit hook that runs `make precommit` (fmt + clippy)

repo_root=$(git rev-parse --show-toplevel 2>/dev/null || true)
if [[ -z "${repo_root}" ]]; then
  echo "error: not in a git repository" >&2
  exit 1
fi

hook_dir="$repo_root/.git/hooks"
hook_file="$hook_dir/pre-commit"

mkdir -p "$hook_dir"
cat > "$hook_file" <<'HOOK'
#!/usr/bin/env bash
set -euo pipefail

if command -v make >/dev/null 2>&1; then
  echo "[pre-commit] running: make precommit"
  make precommit
else
  echo "[pre-commit] make not found; skipping format/lint gate" >&2
fi
HOOK

chmod +x "$hook_file"
echo "Installed pre-commit hook at $hook_file"

