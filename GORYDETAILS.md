# GORYDETAILS — internals, workflows, and command reference

## Catalog workflows in depth
`sk repo add` clones a remote skills catalog into your per-user cache and records the alias inside `skills.repos.json` so everyone in the repo can reuse it. After caching, you can browse or script against the catalog without installing anything yet.

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
2. `sk` commits and pushes to the repo recorded in `skills.lock.json`. For brand-new skills, it falls back to `sk config get default_repo`.
3. Branches default to `sk/sync/<name>/<timestamp>`. `gh pr create` (and `gh pr merge` when auto-merge is armed) handles the review path. Missing `rsync` or `gh` triggers warnings but never aborts the publish.
4. Upon success, `skills.lock.json` updates to the new commit and digest so teammates get the latest version immediately.

## Installing or hacking on `sk` itself
```bash
git clone https://github.com/<you>/sk-decisions.git
cd sk-decisions
cargo build --release          # binary at target/release/sk
# optional: cargo install --path .   # installs into ~/.cargo/bin/sk
```
Upgrade dependencies or lint locally before sending PRs:
```bash
cargo fmt --all
cargo clippy --all-targets --all-features
```

## Key concepts & layout
- `skills/` — default install root (override via `sk init --root` or `sk config set default_root`).
- `skills.lock.json` — lockfile tracking each installed skill (name, repo URL, commit, digest, timestamp).
- `skills.repos.json` — optional catalog registry populated by `sk repo add` so teammates know which repos you’ve cached.
- Cache clones live under `~/.cache/sk/repos/<host>/<owner>/<repo>` (override with `SK_CACHE_DIR`).
- User config lives in `~/.config/sk/config.json` (override with `SK_CONFIG_DIR`). Keys include `default_root`, `default_repo`, `template_source`, `protocol` (`ssh` or `https`), `default_host`, `github_user`.
- Every skill subdirectory needs `SKILL.md` with YAML front matter declaring `name` and `description`.

## Command cheat sheet
| Command | Use it when |
| --- | --- |
| `sk init [--root ./skills]` | Bootstrap a repo-local skills directory and lockfile. |
| `sk install <repo> <skill-name> [--path subdir] [--alias name]` | Copy a skill from a git repo into `skills/<alias>` and lock its commit/digest. |
| `sk list` / `sk where <name>` | Inspect installed skill set or find the on-disk path. |
| `sk check [name...] [--json]` | Quick OK/modified/missing status for installs. |
| `sk status [name...] [--json]` | Compare digests plus show upstream tip (`old -> new`). |
| `sk repo add <repo> [--alias foo]` | Cache a remote repo (and record it in `skills.repos.json`) without installing a skill yet. |
| `sk repo list [--json]` | Show cached repos plus total skills vs. installed counts (`--json` prints the raw registry). |
| `sk repo catalog <alias-or-repo> [--json]` | List every skill exposed by a cached repo before installing. |
| `sk repo search <query> [--repo alias] [--json]` | Search all cached repos (or a single repo via `--repo`) for matching skills. |
| `sk update` | Refresh cached repos (safe to run on CI). |
| `sk upgrade [--all or <name>] [--dry-run]` | Copy newer commits into the repo and update the lockfile. |
| `sk template create <name> "<description>"` | Scaffold a new skill from the configured template into `skills/<name>`. |
| `sk sync-back <name> [-m "..."]` | Push local edits (or brand-new skills) to the configured repo and open a PR with `gh`. |
| `sk doctor [name...] [--apply]` | Diagnose duplicates, missing caches, digest drift; with `--apply` rebuild installs and prune caches. |
| `sk precommit [--allow-local]` | Ensure `skills.lock.json` contains only shareable sources before committing. |
| `sk config get <key>` / `sk config set <key> [value]` | View or tweak defaults like install root, protocol, host, or GitHub username. |
