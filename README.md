# sk — repo-scoped Claude Skills bridge for any agent

`sk` keeps Claude Skills vendored inside your Git repository so agents other than Claude can reuse the same directives. 

## Why sk?
- Vendored helpers travel with your git history, so reviewers and automation see the exact bits you edited.
- Works for any agent.
- Publishing edits is just another PR via `sk sync-back`, so nothing drifts out of band.

## Quickstart: install → cache Anthropic → publish your own

### 0. Install `sk`
```bash
cargo install sk
```

### 1. Initialize inside your repo
```bash
cd /path/to/your/git/repo
sk init
```

### 2. Add the Anthropic catalog
```bash
sk repo add @anthropics/skills --alias anthropic
sk repo list
```
Example `sk repo list` output:
```text
ALIAS       REPO                                SKILLS  INSTALLED
anthropic   github.com/anthropics/skills        120     3
```
`*` next to the SKILLS column means the remote could not be refreshed and the counts are from the last cached fetch.

### 3. Install a few skills
```bash
sk install @anthropics/skills template-skill --alias template
sk install @anthropics/skills brand-guidelines
sk install @anthropics/skills canvas-design
sk list
```

### 4. Create a repo for skills you author
```bash
gh repo create your-gh-username/skills --private --clone
sk config set default_repo @your-gh-username/skills
```

### 5. Scaffold a new skill
```bash
sk template create retro-template "Retro two-column recap template"
```

### 6. Inspect and publish changes
```bash
sk doctor
sk sync-back brand-guidelines -m "Revise guidance tone"
```
`sk sync-back` looks up the push target from `sk config get default_repo`, mirrors `skills/brand-guidelines` into a temporary branch named `sk/sync/brand-guidelines/<timestamp>`, and opens a PR automatically unless you override the repo/path flags yourself.

### 7. Stay up to date
```bash
sk upgrade brand-guidelines
```
Use `sk upgrade --all` when you want every installed skill to follow its upstream tip.

## Need the gory details?
Implementation notes, machine-readable catalog output, cache layouts, building from source, and the full command cheat sheet now live in `GORYDETAILS.md`.

### 8. Keep caches fresh and roll forward clean installs
<!-- QUICKSTART COMMANDS START -->
```bash
sk update                    # fetch every repo referenced in the lockfile (cache-only)
sk doctor --diff             # compare local installs against the cached remote tip
sk doctor --diff brand-guidelines  # limit the diff to just the named installs
sk upgrade --dry-run         # show old -> new commits without touching the repo
sk upgrade --all             # apply upgrades for every clean (unmodified) skill
sk remove <name>             # refuses if modified unless you pass --force
```
<!-- QUICKSTART COMMANDS END -->
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

## Contributing & qlty guardrails
- Run `scripts/install-qlty.sh` once (and whenever `.qlty-version` changes) to install the pinned qlty CLI into `~/.qlty/bin`. The script honors `QLTY_VERSION`/`QLTY_INSTALL` if you need overrides.
- `make precommit` now runs `cargo fmt`, `cargo clippy --all-targets --all-features`, strict `make qlty` (fails on any findings), and blocking `make qlty-smells`. Keep `$HOME/.qlty/bin` on your `PATH` so the make target can find the CLI.
- Use `make qlty-advisory` when you only need warning-level results, or `make qlty-smells-advisory` for a warn-only smells pass. Both standard targets (`make qlty`, `make qlty-smells`) fail the build on issues and respect the flags in `QLTY_FLAGS`/`QLTY_SMELLS_FLAGS`.
- GitHub Actions mirrors the same setup: both `qlty` and `qlty-smells` jobs are required, with artifacts `qlty-results` and `qlty-smells-results` respectively. Check those artifacts whenever CI fails.
- Neither the Makefile nor CI suppress qlty's upgrade check anymore—expect the CLI to verify that your local CLI/plugins match upstream before linting. Keep outbound network enabled or set the `QLTY_UPGRADE_CHECK=0` env only when debugging failures (and restore it before committing).

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
make precommit                 # fmt + clippy + qlty + smells (blocking)
# or run pieces manually:
cargo fmt --all
cargo clippy --all-targets --all-features
make qlty
make qlty-smells               # blocking (use make qlty-smells-advisory for warn-only)
```

## Key concepts & layout
- `skills/` — default install root (override via `sk init --root` or `sk config set default_root`).
- `skills.lock.json` — versioned lockfile tracking each installed skill plus the shared repo registry (aliases, repo specs, commit/digest, timestamps).
- Cache clones live under `~/.cache/sk/repos/<host>/<owner>/<repo>` (override with `SK_CACHE_DIR`).
- User config lives in `~/.config/sk/config.json` (override with `SK_CONFIG_DIR`). Keys: `default_root`, `default_repo`, `template_source`, `protocol` (`ssh` or `https`), `default_host`, `github_user`.
- Every skill subdirectory must contain `SKILL.md` with YAML front-matter that declares `name` and `description`.

## Encourage agents to bootstrap the skills MCP

An MCP server can’t force a model to call it—you have to ask your agents explicitly. We recommend adding a short policy blurb to `AGENTS.md` (or whichever system prompt you use) that makes “run `skills_search`/`skills_list` before you start” part of the default ritual. Mentioning it in README helps teammates keep the policy consistent across repos.

Drop something like this into `AGENTS.md`:

> **Skills bootstrap checklist** — At the top of every session, call the repo-scoped skills MCP once to discover local helpers. Run `skills_search` with a few task keywords (or `skills_list` if you need the catalog) and skim the results before writing a plan. Reference any relevant skills in your response. Skip this step only if there are zero skills installed.

That paragraph solves the “chicken-and-egg” problem: the agent reads the policy first, makes a single MCP call to find out what’s available, and only then starts reasoning about the actual task.

### Wire Codex (or any MCP client) into `sk`

1. Make sure `sk` is on your `$PATH` (`cargo install sk` if needed) and that you run the MCP server from this repository’s root so it can find `.git` and the vendored `skills/` directory.
2. Register the server with Codex (one time per machine) so agents can call `skills_list`, `skills_search`, and `skills_show` via MCP. Add the server to `~/.codex/config.toml`:

   ```toml
   [mcp_servers.sk]
   command = "sk"
   args = ["mcp-server"]
   ```

   Run Codex from this repository’s root (or add `dir = "/path/to/your/checkout"`) so `sk mcp-server` can find `.git` and the vendored `skills/` tree. If you prefer to register the server via CLI instead of editing the config by hand, run the equivalent command once from the repo root:

   ```bash
   codex mcp add -- bash -lc 'cd /home/mark/lambdalabs/sk && sk mcp-server' sk
   ```

   Replace the path with your local checkout if it differs. After either approach, confirm the entry with `codex mcp list`.
3. When you start a Codex (or Claude) session in this repo, remind the agent that the `sk` MCP is available and should be called before planning. The `skills_search` tool is ideal for “what skills apply to <task>?” checks; `skills_list` and `skills_show` return complete metadata/bodies when you already know the name. The MCP server is read-only—it never edits skills or the lockfile; all modifications go through the `sk` CLI.

   Bonus: the MCP server also advertises a `sk://quickstart` resource (via `resources/list`) sourced from `docs/AGENT_QUICKSTART.md`. Agents can `resources/read` that URI to pull the repo-scoped quickstart (install → cache → publish) without scraping the file system.

## Command cheat sheet
| Command | Use it when |
| --- | --- |
| `sk init [--root ./skills]` | Bootstrap a repo-local skills directory and lockfile. |
| `sk install <repo> <skill-name> [--path subdir] [--alias name]` | Copy a skill from a git repo into `skills/<alias>` and lock its commit/digest. |
| `sk list` / `sk where <name>` | Inspect installed skill set or find the on-disk path. |
| `sk doctor [name...] [--summary|--status|--diff] [--json] [--apply]` | Unified health command: `--summary` is the old `sk check`, `--status` shows digests and upgrades, `--diff` compares with the remote tip, and without flags it performs the full repair run (optionally `--apply`). |
| `sk repo add <repo> [--alias foo]` | Cache a remote repo (and record it in `skills.lock.json`’s repo registry) without installing a skill yet. |
| `sk repo list [--json]` | Show cached repos + their aliases. |
| `sk repo remove <alias-or-repo> [--json]` | Drop a cached repo entry (alias or repo spec) when you no longer need it. |
| `sk repo search --repo <alias-or-repo> [--all] [--json]` | List every skill exposed by a cached repo before installing (replacement for `sk repo catalog`). |
| `sk repo search <query> [--repo alias] [--json]` | Search all cached repos (or a single repo via `--repo`) for matching skills. |
| `sk update` | Refresh cached repos (safe to run on CI). |
| `sk upgrade [--all or <name>] [--dry-run]` | Copy newer commits into the repo and update the lockfile. |
| `sk template create <name> "<description>"` | Scaffold a new skill from the configured template into `skills/<name>`. |
| `sk sync-back <name> [-m "..."]` | Push local edits (or brand-new skills) to the configured repo and auto-open a PR with `gh`. |
| `sk precommit [--allow-local]` | Enforce no local-only sources in `skills.lock.json` before committing. |
| `sk config get|set <key> [value]` | View or tweak defaults like install root, protocol, host, GitHub username. |

That’s it—`sk` keeps your Claude Skills reproducible, reviewable, and easy to upstream. Let us know what other workflows you need!
