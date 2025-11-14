# sk — repo-scoped Claude Skills manager

`sk` keeps Claude Skills vendored *inside* your Git repository so teammates, CI, and downstream consumers all get the exact same set of helper skills. It clones remote skill repos into a per-user cache, copies selected skills into `./skills/<name>`, pins them in `skills.lock.json`, and gives you tooling to inspect, upgrade, and publish edits without leaving your repo.

## Why you might want it
- Install skills from the default Anthropic catalog (`@anthropics/skills`) or from any git remote/path (SSH, HTTPS, `file://`, GitHub shorthand).
- Keep a project-local lockfile so skills travel with the repo—no hidden global state.
- Detect drift with `sk status`, `sk check`, and `sk doctor`.
- Use `sk sync-back` to push local edits (or entirely new skills) back to the source repo, automatically opening a PR via `gh` when possible.
- Cache fetches with `sk update` and apply upgrades with `sk upgrade --dry-run|--all`.
- Guard CI with `sk precommit` to block unreproducible `file://` sources.

## Quickstart: install → fetch Anthropic skills → publish your own
`sk` is published on crates.io—install it once and then keep everything repo-scoped.

### 0. Install `sk` (one-time)
```bash
cargo install sk
# later upgrades
cargo install sk --force
```

### 1. Initialize inside your repo
```bash
cd /path/to/your/git/repo
sk init                      # creates ./skills and skills.lock.json if missing
sk config set default_root ./skills   # optional: persist the root
```
Commit both `skills/` contents and `skills.lock.json`.

### 2. Pull a few canonical Anthropic skills
`@owner/repo` shorthand targets the default host (`github.com`) over SSH. Grab multiple helpers from the official Anthropic catalog at `github.com/anthropics/skills`:
```bash
sk install @anthropics/skills template-skill --alias template
sk install @anthropics/skills frontend-design
sk install @anthropics/skills artifacts-builder
sk list
sk status template frontend-design artifacts-builder
```

### 3. Create your own upstream repo with `gh`
Use the GitHub CLI (already required for `sk sync-back`) to host skills you author:
```bash
gh repo create your-gh-username/claude-skills --private --clone
# or, inside the clone:
gh repo create your-gh-username/claude-skills --private --add-readme
```
Point `sk` at that repo when installing or publishing new skills. For example, scaffold a local skill directory, then use `sync-back` to push it to your brand-new repo:
```bash
mkdir -p skills/retro-template
cat > skills/retro-template/SKILL.md <<'EOF'
---
name: retro-template
description: My retro template skill
---
EOF

sk sync-back retro-template \
  --repo @your-gh-username/claude-skills \
  --skill-path retro-template
```
`sk` mirrors your local edits into the cached clone, pushes a branch to `github.com:your-gh-username/claude-skills.git`, and opens/auto-merges a PR via `gh`.

### 4. Inspect edits with `sk status` and `sk doctor`
`sk status` recomputes digests and shows pending upstream updates:
```bash
sk status
# frontend-design    modified    3a1b7c2 -> 8dd55a1
```
`sk doctor` digs deeper (duplicates, missing caches, digest drift) and ties findings to the right follow-up command:
```bash
sk doctor
== frontend-design ==
- Digest mismatch (modified)
- Local edits present and upstream advanced (3a1b7c2 -> 8dd55a1). Run 'sk sync-back frontend-design' to publish or revert changes, then 'sk upgrade frontend-design' to pick up the remote tip.
```
Add `--apply` to rebuild missing installs from the cached commit, drop orphaned lock entries, and prune unused cache clones.

### 5. Push updates for an installed skill (`sk sync-back`)
After editing files under `skills/<name>`:
```bash
sk sync-back frontend-design \
  --repo @your-gh-username/claude-skills \
  --skill-path frontend-design \
  --branch sk/sync/frontend-$(date +%Y%m%d-%H%M) \
  --message "Revise guidance tone"
```
What happens:
1. The installed directory is mirrored into a clean worktree of the cached repo (via `rsync` when available).
2. `sk` commits, pushes `branch` (default `sk/sync/<name>/<timestamp>`), then calls `gh pr list|create|merge`. If required checks pass and the repo has Auto-merge enabled, the PR is armed automatically; conflicts are surfaced with the PR URL.
3. `skills.lock.json` is updated to point at the new commit and digest so teammates pull the new content immediately.

Without `gh`, you’ll see “Skipping PR automation: 'gh' CLI not found” and must open/merge the PR yourself.
> Note: this step requires push access to the skill’s source repo (typically your fork). If you only plan to publish brand-new skills, skip straight to Step 6.

### 6. Publish a brand-new skill back upstream
If a folder exists under `skills/` but isn’t in the lockfile yet (for example, you scaffolded `skills/retro-template` from scratch):
```bash
# ensure SKILL.md front-matter names the skill "retro-template"
sk sync-back retro-template \
  --repo @your-gh-username/claude-skills \
  --skill-path retro-template \
  --branch sk/new/retro-template
```
`--repo` is required for new installs so `sk` knows where to push. `--skill-path` defaults to the installed folder name, but you can target nested paths like `examples/retro-template`. The command pushes the branch, opens/merges a PR (via `gh`), and then *adds* the new entry to `skills.lock.json`.

### 7. Keep caches fresh and roll forward clean installs
```bash
sk update                    # fetch every repo referenced in the lockfile (cache-only)
sk upgrade --dry-run         # show old -> new commits without touching the repo
sk upgrade --all             # apply upgrades for every clean (unmodified) skill
sk remove <name>             # refuses if modified unless you pass --force
```
`sk upgrade --all` skips modified installs and prints the commit span so you can decide whether to `sync-back` or revert.

### 8. Guard CI with `sk precommit`
```bash
sk precommit                 # fails if skills.lock.json references file:// or localhost sources
sk precommit --allow-local   # warn-only (useful for experimentation)
```

> ✅ These Quickstart commands stay honest via `tests/quickstart.rs` (`cargo test quickstart_readme_flow`), which (outside CI) clones the real `anthropics/skills` repo from GitHub and drives the same flow end-to-end (using a fake `gh` binary for PR automation and skipping only the “push upstream” step because CI lacks write access).

## Dependencies
- **Rust (stable channel) + Cargo** — required for `cargo install sk` and to build from source (`rust-toolchain.toml` pins `stable` with `clippy`/`rustfmt`).
- **git (>=2.30)** — cloning, fetching, worktrees, and `git archive` extraction.
- **tar** — used during install/upgrade to unpack archived skill contents.
- **rsync** *(optional but recommended)* — `sk sync-back` mirrors your edited skill tree with `rsync -a --delete`; falls back to a slower copy if missing.
- **GitHub CLI (`gh`)** — `sk sync-back` uses `gh pr list|create|merge` to open and auto-merge PRs. Without `gh`, the push still happens but you must open the PR manually.
- Standard SSH credentials (default protocol) or HTTPS access tokens if you pass `--https`.

## Installation options
### From crates.io (recommended)
```bash
cargo install sk
# upgrade later
cargo install sk --force
```

### From source (for contributors)
```bash
git clone https://github.com/<you>/sk-decisions.git
cd sk-decisions
cargo build --release          # binary at target/release/sk
# optional: cargo install --path .   # installs into ~/.cargo/bin/sk
```

Upgrade dependencies or lint locally:
```bash
cargo fmt --all
cargo clippy --all-targets --all-features
```

## Key concepts & layout
- `skills/` — default install root (override via `sk init --root` or `sk config set default_root`).
- `skills.lock.json` — versioned lockfile tracking each installed skill (`installName`, repo URL, commit, digest, install timestamp).
- Cache clones live under `~/.cache/sk/repos/<host>/<owner>/<repo>` (override with `SK_CACHE_DIR`).
- User config lives in `~/.config/sk/config.json` (override with `SK_CONFIG_DIR`). Keys: `default_root`, `protocol` (`ssh` or `https`), `default_host`, `github_user`.
- Every skill subdirectory must contain `SKILL.md` with YAML front-matter that declares `name` and `description`.

## Command cheat sheet
| Command | Use it when |
| --- | --- |
| `sk init [--root ./skills]` | Bootstrap a repo-local skills directory and lockfile. |
| `sk install <repo> <skill-name> [--path subdir] [--alias name]` | Copy a skill from a git repo into `skills/<alias>` and lock its commit/digest. |
| `sk list` / `sk where <name>` | Inspect installed skill set or find the on-disk path. |
| `sk check [name...] [--json]` | Quick OK/modified/missing status for installs. |
| `sk status [name...] [--json]` | Compare digests plus show upstream tip (`old -> new`). |
| `sk update` | Refresh cached repos (safe to run on CI). |
| `sk upgrade <--all|name> [--dry-run]` | Copy newer commits into the repo and update the lockfile. |
| `sk sync-back <name> [--branch ... --repo ... --skill-path ...]` | Push local edits (or new skills) back to the remote repo, optionally auto-opening a PR with `gh`. |
| `sk doctor [--apply]` | Diagnose duplicates, missing caches, digest drift; with `--apply` rebuild installs, prune caches, drop orphaned lock entries. |
| `sk precommit [--allow-local]` | Enforce no local-only sources in `skills.lock.json` before committing. |
| `sk config get|set <key> [value]` | View or tweak defaults like install root, protocol, host, GitHub username. |

## Troubleshooting & tips
- **Auth & protocols**: Default installs use SSH (`git@github.com:owner/repo.git`). Run `sk config set protocol https` to default to HTTPS, or pass `--https` per command.
- **Cache hygiene**: `sk doctor --apply` prunes caches the project no longer references. Without `--apply`, it only reports.
- **Missing tools**: If `rsync` isn’t installed, `sk sync-back` falls back to a recursive copy (slower, but works). Without `gh`, the command still pushes but prints a warning and skips PR automation.
- **Environment overrides**: `SK_CACHE_DIR=/tmp/sk-cache` stores caches elsewhere; `SK_CONFIG_DIR` relocates user config (useful for CI sandboxes).
- **Commit discipline**: Always commit `skills.lock.json` alongside skill directories so teammates reproduce the same commit + digest.
- **Pre-release validation**: Run `sk update && sk upgrade --dry-run && sk doctor` before tagging a release to confirm caches, lockfile, and local edits are in sync.

That’s it—`sk` keeps your Claude Skills reproducible, reviewable, and easy to upstream. Let us know what other workflows you need!
