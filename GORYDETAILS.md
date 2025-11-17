# GORYDETAILS — internals, workflows, and command reference

## Catalog workflows in depth
`sk repo add` clones a remote skills catalog into your per-user cache and records the alias inside the `skills.lock.json` repo registry so everyone in the repo can reuse it. After caching, you can browse or script against the catalog without installing anything yet.

```bash
sk repo add @anthropics/skills --alias anthropic
sk repo catalog anthropic                 # human-readable listing
sk repo search "retro"                    # search across every cached repo
sk repo catalog @anthropics/skills --json  # machine-readable listing for tooling
```
`sk update` refreshes every cached repo. All catalog files live under `~/.cache/sk/repos/<host>/<owner>/<repo>` unless you override `SK_CACHE_DIR`.

## Template behavior
`sk template create <name> "<description>"` copies the canonical template into `./skills/<name>`, rewrites the YAML front matter, and adds stub prompt/test files so any agent can use the new helper. The source template comes from `sk config get template_source` (defaults to `@anthropics/skills template-skill`). Change it with `sk config set template_source <repo>/<skill>`. The install root follows `./skills` unless you override it via `sk config set default_root <dir>`.

## `sk doctor` deep dive
`sk doctor [name...]` recalculates digests, confirms cached commits still exist, and tells you which follow-up command fixes each issue. Add `--apply` to rebuild missing installs from the cached commit, drop orphaned lock entries, and prune caches so the lockfile stays aligned with disk. When editing a new skill, run `sk doctor <name>` frequently so you know if upstream advanced while you were working.

## `sk sync-back` internals
After editing files under `skills/<name>`:
1. The install directory is mirrored into a clean worktree of the cached repo under `~/.cache/sk/repos/...` (prefers `rsync -a --delete`, falls back to a recursive copy if `rsync` is unavailable).
2. `sk` commits and pushes to the repo supplied via `--repo` or, when omitted, `sk config get default_repo`. The destination skill path defaults to the install name so `sk sync-back <name>` works with no extra flags once `default_repo` is set.
3. Branches default to `sk/sync/<name>/<timestamp>`. `gh pr create` (and `gh pr merge` when auto-merge is armed) handles the review path. Missing `rsync` or `gh` triggers warnings but never aborts the publish.
4. Upon success, `skills.lock.json` updates to the new commit and digest so teammates get the latest version immediately.

## Installing or hacking on `sk` itself
```bash
git clone https://github.com/<you>/sk-decisions.git
cd sk-decisions
cargo build --release          # binary at target/release/sk
# optional: cargo install --path .   # installs into ~/.cargo/bin/sk
```
Install the pinned qlty CLI locally so the make targets match CI:
```bash
./scripts/install-qlty.sh      # installs version from .qlty-version into ~/.qlty/bin
export PATH="$HOME/.qlty/bin:$PATH"
```
Qlty now runs with its upgrade check enabled both locally and in CI, so expect it to contact qlty.sh to confirm versions before linting. Keep network access available; if you must skip the check for debugging, do it temporarily and never commit without re-enabling it.

Upgrade dependencies or lint locally before sending PRs:
```bash
make precommit                 # fmt + clippy + qlty + smells (all blocking)
# or run the pieces manually:
cargo fmt --all
cargo clippy --all-targets --all-features
make qlty
make qlty-smells               # blocking; use make qlty-smells-advisory for warn-only runs
```

## Key concepts & layout
- `skills/` — default install root (override via `sk init --root` or `sk config set default_root`).
- `skills.lock.json` — lockfile tracking each installed skill plus the shared repo registry (name, repo URL, commit, digest, timestamps, aliases).
- Cache clones live under `~/.cache/sk/repos/<host>/<owner>/<repo>` (override with `SK_CACHE_DIR`).
- User config lives in `~/.config/sk/config.json` (override with `SK_CONFIG_DIR`). Keys include `default_root`, `default_repo`, `template_source`, `protocol` (`ssh` or `https`), `default_host`, `github_user`.
- Every skill subdirectory needs `SKILL.md` with YAML front matter declaring `name` and `description`.

## Command cheat sheet
| Command | Use it when |
| --- | --- |
| `sk init [--root ./skills]` | Bootstrap a repo-local skills directory and lockfile. |
| `sk install <repo> <skill-name> [--path subdir] [--alias name]` | Copy a skill from a git repo into `skills/<alias>` and lock its commit/digest. |
| `sk list` / `sk where <name>` | Inspect installed skill set or find the on-disk path. |
| `sk doctor [name...] [--summary|--status|--diff] [--json] [--apply]` | Unified install health checks: `--summary` replaces `sk check`, `--status` shows digests plus remote tips, `--diff` compares against the cached default-branch tip, and without flags it runs the deep repair flow (optionally `--apply`). |
| `sk repo add <repo> [--alias foo]` | Cache a remote repo (and record it in `skills.lock.json`’s repo registry) without installing a skill yet. |
| `sk repo list [--json]` | Show cached repos plus total skills vs. installed counts; unreachable repos reuse cached counts and show a `*` next to the SKILLS column (`--json` prints the raw registry). |
| `sk repo remove <alias-or-repo> [--json]` | Remove a cached repo entry by alias or repo spec when it’s no longer needed. |
| `sk repo catalog <alias-or-repo> [--json]` | List every skill exposed by a cached repo before installing. |
| `sk repo search <query> [--repo alias] [--json]` | Search all cached repos (or a single repo via `--repo`) for matching skills. |
| `sk update` | Refresh cached repos (safe to run on CI). |
| `sk upgrade [--all or <name>] [--dry-run]` | Copy newer commits into the repo and update the lockfile. |
| `sk template create <name> "<description>"` | Scaffold a new skill from the configured template into `skills/<name>`. |
| `sk sync-back <name> [-m "..."]` | Push local edits (or brand-new skills) to the configured repo and open a PR with `gh`. |
| `sk precommit [--allow-local]` | Ensure `skills.lock.json` contains only shareable sources before committing. |
| `sk config get <key>` / `sk config set <key> [value]` | View or tweak defaults like install root, protocol, host, or GitHub username. |
