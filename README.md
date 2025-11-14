# sk — repo-scoped Claude Skills bridge for any agent

`sk` lets Codex, bespoke LLM runners, and every non-Claude agent reuse the same Claude Skills you already trust. It keeps those skills vendored *inside* your Git repository so reviewers, CI, and downstream consumers get the exact same helper set. Behind the scenes `sk` clones remote skill repos into a per-user cache, copies selected skills into `./skills/<name>`, pins them in `skills.lock.json`, and gives you tooling to inspect, upgrade, and publish edits without leaving your repo.

## Why you might want it
- Make Claude Skills available in an agent-agnostic way so Codex and every other automation surface share the same vetted helpers.
- Share, update, and version those skills like code—Git history, reviews, and CI catch drift automatically.

## Quickstart: install → fetch Anthropic skills → publish your own
`sk` is published on crates.io—install it once and then keep everything repo-scoped.

### 0. Install `sk` (one-time)
```bash
cargo install sk
```

### 1. Initialize inside your repo
```bash
cd /path/to/your/git/repo
sk init                      # creates ./skills and skills.lock.json if missing
sk config set default_root ./skills   # optional: persist the root
sk config set default_repo @your-gh-username/claude-skills  # optional: default sync-back repo
```
Commit both `skills/` contents and `skills.lock.json`.

### 2. Pull a few canonical Anthropic skills
`@owner/repo` shorthand targets the default host (`github.com`) over SSH. Grab multiple helpers from the official Anthropic catalog at `github.com/anthropics/skills`:
```bash
sk install @anthropics/skills template-skill --alias template
sk install @anthropics/skills frontend-design
sk install @anthropics/skills artifacts-builder
sk list
```

### 2½. Cache a repo and browse before installing
Use `sk repo add` to clone the catalog into the local cache without copying a skill yet. Then list what’s inside and cherry-pick installs:
```bash
sk repo add @anthropics/skills --alias anthropic-catalog
sk repo list
sk repo catalog anthropic-catalog           # human-friendly table
sk repo search "retro"                      # search across every cached repo
sk repo catalog @anthropics/skills --json   # machine-readable listing
```
`sk repo add` writes to `skills.repos.json` in your project so teammates share the same catalogs.

### 3. Create your own upstream repo with `gh`
Use the GitHub CLI (already required for `sk sync-back`) to host skills you author:
```bash
gh repo create your-gh-username/claude-skills --private --clone
```
Set it as the default publish target once so `sk sync-back` knows where to push brand-new skills:
```bash
sk config set default_repo @your-gh-username/claude-skills
```

### 4. Scaffold a new skill with `sk template`
`sk template create <skill-name> "<description>"` copies the canonical template into `skills/<skill-name>`, rewrites the YAML metadata, and adds stub prompt/test files so every agent sees the same structure.
```bash
sk template create retro-template "Retro two-column recap template"
```
Behavior:
1. The base template comes from `sk config get template_source` (defaults to `@anthropics/skills template-skill`). Change it with `sk config set template_source <repo>/<skill>`.
2. The install root defaults to `./skills` (or your `default_root`). Override once via `sk config set default_root ./some/other/dir`.
3. After the files land in `skills/retro-template`, run `sk doctor retro-template`, add prompts/tests, then publish with `sk sync-back retro-template`.

### 5. Inspect edits with `sk doctor`
`sk doctor` recomputes digests, shows pending upstream updates, and ties findings to the right follow-up command:
```bash
sk doctor
== frontend-design ==
- Digest mismatch (modified)
- Local edits present and upstream advanced (3a1b7c2 -> 8dd55a1). Run 'sk sync-back frontend-design' to publish or revert changes, then 'sk upgrade frontend-design' to pick up the remote tip.
```
Add `--apply` to rebuild missing installs from the cached commit, drop orphaned lock entries, and prune unused cache clones.

### 6. Push updates for an installed skill (`sk sync-back`)
After editing files under `skills/<name>`:
```bash
sk sync-back frontend-design -m "Revise guidance tone"
```
What happens:
1. The installed directory is mirrored into a clean worktree of the cached repo under `~/.cache/sk/repos/...` (using `rsync -a --delete` when available, otherwise falling back to a recursive copy).
2. `sk` commits, pushes to the repo recorded in `skills.lock.json` (typically wherever you originally installed the skill). For brand-new skills (Step 7), it falls back to `sk config get default_repo`. Branch names default to `sk/sync/<name>/<timestamp>`, and `gh pr list|create|merge` wires up the PR. If required checks pass and the repo has Auto-merge enabled, the PR is armed automatically; conflicts are surfaced with the PR URL.
3. `skills.lock.json` is updated to point at the new commit and digest so teammates pull the new content immediately.

Missing `rsync` or `gh` is not fatal—`sk` prints a warning, keeps going, and simply asks you to open the PR manually if `gh` is unavailable.
> Note: this step requires push access to the skill’s source repo (typically your fork). If you only plan to publish brand-new skills, skip straight to Step 7.

### 7. Publish a brand-new skill back upstream
If a folder exists under `skills/` but isn’t in the lockfile yet (for example, you just ran `sk template create retro-template`):
```bash
sk sync-back retro-template
```
For folders that haven’t been synced before `sk` reads the target repo from `default_repo`, infers the on-disk path from the install name, and auto-generates the branch. Pass flags only when overriding defaults (for example, `--repo` if you need to publish to a different org). Once the PR merges, `skills.lock.json` gains the new entry so teammates pick it up on their next pull.

### 8. Keep caches fresh and roll forward clean installs
```bash
sk update                    # fetch every repo referenced in the lockfile (cache-only)
sk upgrade --dry-run         # show old -> new commits without touching the repo
sk upgrade --all             # apply upgrades for every clean (unmodified) skill
sk remove <name>             # refuses if modified unless you pass --force
```
`sk upgrade --all` skips modified installs and prints the commit span so you can decide whether to `sync-back` or revert.

### 9. Guard CI with `sk precommit`
```bash
sk precommit                 # fails if skills.lock.json references file:// or localhost sources
sk precommit --allow-local   # warn-only (useful for experimentation)
```

> ✅ These Quickstart commands stay honest via `tests/quickstart.rs` (`cargo test quickstart_readme_flow`), which runs automatically whenever `CI=1` (e.g., on GitHub Actions). The test clones the real `anthropics/skills` repo, drives the workflow end-to-end with a fake `gh` binary, and skips only the “push upstream” step because CI lacks write access. To run it locally, export `CI=1`.

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
- `skills.repos.json` — optional catalog registry populated by `sk repo add` so teammates know which repos you’ve cached.
- Cache clones live under `~/.cache/sk/repos/<host>/<owner>/<repo>` (override with `SK_CACHE_DIR`).
- User config lives in `~/.config/sk/config.json` (override with `SK_CONFIG_DIR`). Keys: `default_root`, `default_repo`, `template_source`, `protocol` (`ssh` or `https`), `default_host`, `github_user`.
- Every skill subdirectory must contain `SKILL.md` with YAML front-matter that declares `name` and `description`.

## Command cheat sheet
| Command | Use it when |
| --- | --- |
| `sk init [--root ./skills]` | Bootstrap a repo-local skills directory and lockfile. |
| `sk install <repo> <skill-name> [--path subdir] [--alias name]` | Copy a skill from a git repo into `skills/<alias>` and lock its commit/digest. |
| `sk list` / `sk where <name>` | Inspect installed skill set or find the on-disk path. |
| `sk check [name...] [--json]` | Quick OK/modified/missing status for installs. |
| `sk status [name...] [--json]` | Compare digests plus show upstream tip (`old -> new`). |
| `sk repo add <repo> [--alias foo]` | Cache a remote repo (and record it in `skills.repos.json`) without installing a skill yet. |
| `sk repo list [--json]` | Show cached repos + their aliases. |
| `sk repo catalog <alias-or-repo> [--json]` | List every skill exposed by a cached repo before installing. |
| `sk repo search <query> [--repo alias] [--json]` | Search all cached repos (or a single repo via `--repo`) for matching skills. |
| `sk update` | Refresh cached repos (safe to run on CI). |
| `sk upgrade [--all|<name>] [--dry-run]` | Copy newer commits into the repo and update the lockfile. |
| `sk template create <name> "<description>"` | Scaffold a new skill from the configured template into `skills/<name>`. |
| `sk sync-back <name> [-m "..."]` | Push local edits (or brand-new skills) to the configured repo and auto-open a PR with `gh`. |
| `sk doctor [name...] [--apply]` | Diagnose duplicates, missing caches, digest drift; with `--apply` rebuild installs, prune caches, drop orphaned lock entries. |
| `sk precommit [--allow-local]` | Enforce no local-only sources in `skills.lock.json` before committing. |
| `sk config get|set <key> [value]` | View or tweak defaults like install root, protocol, host, GitHub username. |

That’s it—`sk` keeps your Claude Skills reproducible, reviewable, and easy to upstream. Let us know what other workflows you need!
