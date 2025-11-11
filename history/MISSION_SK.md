# Codex mission: build `sk` — a repo‑scoped Claude Skills manager (Rust)

**Goal**
Create a fast, deterministic CLI named `sk` to *install, check, upgrade, and manage Claude Skills* (exact Anthropic format: a folder with a top‑level `SKILL.md` whose YAML front‑matter includes `name` and `description`) **inside the current Git repo only**. No global installs. A per‑user cache avoids re‑cloning. A repo‑local **JSON lockfile** pins commits. Upgrades are refused if local changes exist. `sk update` touches **only the cache** so `sk upgrade --dry-run` can tell which skills are out of date without modifying the repo.

---

## Non‑goals / constraints

* Do **not** invent new schemas or directories; use the **existing Skills layout** (folders + `SKILL.md`).
* Do **not** write to any global skill directories (no `.claude/…`).
* All effective installs live under the **project root** (default `./skills`).
* Only rely on `git` via `Command`; no network beyond `git`.
* Never rewrite skill contents except when copying on install/upgrade.
* Refuse to upgrade/remove if the installed copy is **modified**.

---

## CLI surface

```
sk init [--root <dir>]
sk install <repo> <skill-name> [--ref <branch|tag|sha>] [--alias <name>] [--path <subdir>] [--root <dir>] [--https]
sk list [--root <dir>] [--json]
sk where <installed-name> [--root <dir>]
sk check [<installed-name> ...] [--root <dir>] [--json]
sk status [<installed-name> ...] [--root <dir>] [--json]
sk update                      # cache-only: fetch all known repos; no project writes
sk upgrade <installed-name|--all> [--ref <...>] [--root <dir>] [--dry-run] [--include-pinned]
sk remove <installed-name> [--root <dir>] [--force]
sk sync-back <installed-name> [--branch <name>] [--message "<msg>"] [--root <dir>]
sk config get|set <key> [value]  # per-user config (~/.config/sk/config.json)
```

**Repo shorthand**

* `@owner/repo` ⇒ `git@github.com:owner/repo.git` (default SSH; `--https` flips to HTTPS).
* Full `ssh://` / `https://` URLs pass through unchanged.

**Skill selection**

* `<skill-name>` matches `front_matter.name` in `SKILL.md`.
* If multiple subdirs in the repo share that `name`, require `--path <subdir>` to disambiguate.

---

## On‑disk layout

**Project (repo)**

```
<project-root>/
  skills/                          # default install root (configurable)
    <installed-name>/...           # copied skill contents; user may edit
  skills.lock.json                 # lockfile (one per repo)
```

**User (cache & config)**

```
~/.cache/sk/repos/<host>/<owner>/<repo>     # normal clone for fetch/checkout
~/.config/sk/config.json                    # {"default_root":"./skills","protocol":"ssh","default_host":"github.com","github_user":""}
```

---

## Lockfile: `skills.lock.json` (repo‑local, JSON)

```json
{
  "version": 1,
  "skills": [
    {
      "installName": "pdf",                    // folder under ./skills
      "source": {
        "url": "git@github.com:anthropics/skills.git",
        "host": "github.com",
        "owner": "anthropics",
        "repo": "skills",
        "skillPath": "document-skills/pdf"     // subdir containing SKILL.md
      },
      "ref": "main",                           // optional user constraint; omit to track default branch
      "commit": "9f0e7c...abc",                // pinned commit actually installed
      "digest": "sha256:...",                  // tree digest of installed copy
      "installedAt": "ISO-8601"
    }
  ],
  "generatedAt": "ISO-8601"
}
```

* **`ref` semantics**

  * *branch name*: track that branch.
  * *tag or exact SHA*: treated as **pinned** (skip in `update`-driven upgrades unless `--include-pinned`).
  * *omitted*: track **remote default branch** (determined via cache).

---

## Command semantics

### `sk init`

* Ensure we’re in a Git working tree.
* Create install root (`--root` or config default `./skills`).
* Create empty `skills.lock.json` if absent: `{\"version\":1,\"skills\":[],\"generatedAt\":\"…\"}`.
* Create user config if missing with defaults.

### `sk install <repo> <skill-name>`

1. **Resolve repo URL** from shorthand or explicit URL; ensure cached clone at `~/.cache/sk/repos/...` (clone if missing; otherwise fetch).
2. **Resolve ref**: if `--ref` empty, prepare to use remote default branch; else use the provided ref.
3. **Discover skill**: search for subdirs with `SKILL.md`, parse YAML front‑matter; pick the one whose `name` == `<skill-name>` (or enforced via `--path`).
4. **Checkout commit**: resolve `<ref>` to a commit in the cache (or `origin/HEAD`/default branch head).
5. **Copy** subdir to `<root>/<installName>/` where `installName = --alias` or front‑matter `name`.
6. **Compute digest** (SHA‑256 over sorted rel paths+bytes, ignoring `.git` & editor junk).
7. **Write lock** entry with source, `ref`, `commit`, `digest`.

### `sk list`, `sk where`

* Show installed skills and their source/commit; `--json` returns machine‑readable.

### `sk check`

* For each target (or all): ensure `SKILL.md` exists, `name` & `description` present; recompute digest and report `ok|modified|missing`.

### `sk status`

* Compare current digest vs lock (`modified` vs `clean`).
* If the cache knows a newer commit for the tracked branch (based on the last `sk update`), report `out_of_date` with `old_sha → new_sha` (no changes to files).

### `sk update`  **(cache‑only)**

* For **every repo referenced in the lockfile**, run `git fetch --prune` in its cache clone.
* Update the cache’s notion of **default branch**:

  * Prefer `git symbolic-ref -q refs/remotes/origin/HEAD` in the cached clone.
  * If absent, run `git ls-remote --symref <url> HEAD` and set `origin/HEAD` appropriately in cache.
* **No project files or lockfile are changed.**
* Purpose: keep cache current so `sk upgrade --dry-run` can decide if updates are available.

### `sk upgrade <name|--all>`

* For each target skill:

  * If **modified** (digest mismatch) → **abort** with message:

    > Local edits in `skills/<name>`. Refusing to upgrade. Fork the source and repoint, or use `sk sync-back <name>` if you have push access.
  * Determine target commit:

    * If `ref` is a **branch** → use cache’s `origin/<branch>` tip.
    * If `ref` is **omitted** → use cache’s **default branch** tip.
    * If `ref` is **tag/SHA** → treat as pinned; skip unless `--include-pinned`.
  * If `--dry-run` → *print plan only* (`old → new`) and exit.
  * Otherwise copy the subdir from cache at the new commit into `<root>/<name>`, recompute digest, update the lockfile commit & digest.

### `sk remove <name>`

* Refuse if modified unless `--force`. Remove folder and lock entry.

### `sk sync-back <name>`

* Only for repos the user can push to:

  * In the cached clone, create a branch based at the locked commit.
  * Rsync the installed dir over the skill subdir path, commit with message, push to origin.
  * Print a ready-to-run `gh pr create` suggestion.
* If source is not writable (push denied), suggest `fork` (outside the scope of this CLI).

### `sk doctor`

* **Analyze**:

  * Lockfile parse & schema; duplicate `installName`; missing required fields.
  * On-disk presence of `skills/<name>` and `SKILL.md` w/ required front‑matter.
  * Digest drift (`modified`), missing source repos in cache, missing locked commits (force‑push/prune).
  * Stale cache clones not referenced in lock.
* `--apply` repairs:

  * Rebuild missing `skills/<name>` from **locked commit** (if reachable).
  * Drop orphan lock entries (with confirmation unless `--json`).
  * Prune unreferenced cache clones.
  * Normalize lockfile ordering and timestamps.

### `sk config get|set`

* Keys: `default_root`, `protocol` (`ssh|https`), `default_host` (default `github.com`), `github_user`.

---

## Core data structures (Rust)

```rust
#[derive(Deserialize, Serialize, Clone)]
struct Lockfile {
    version: u32,
    skills: Vec<LockSkill>,
    generatedAt: String,
}

#[derive(Deserialize, Serialize, Clone)]
struct LockSkill {
    installName: String,
    source: Source,
    ref_: Option<String>,  // "main" | "v1.2.3" | "abc123" | None
    commit: String,        // pinned commit installed
    digest: String,        // "sha256:<hex>"
    installedAt: String,
}

#[derive(Deserialize, Serialize, Clone)]
struct Source {
    url: String,
    host: String,
    owner: String,
    repo: String,
    skillPath: String,     // subdir containing SKILL.md
}

#[derive(Deserialize)]
struct SkillMeta {
    name: String,
    description: String,
    // ignore other fields
}
```

---

## Implementation details

* **Language & crates**: Rust stable; `clap` v4 (derive), `anyhow`, `thiserror`, `serde`, `serde_json`, `serde_yaml`, `walkdir`, `ignore` (for .gitignore rules), `sha2` (SHA‑256), `rayon` (optional parallel scans), `which`, `directories`, `tempfile`.
* **Git**: shell out with `Command`.

  * Cache clone path: `~/.cache/sk/repos/<host>/<owner>/<repo>`.
  * Ensure clone exists; else `git clone --mirror` or normal clone—use normal clone (easier for worktree/HEAD ops).
  * `sk update`: `git -C <cache> fetch --prune`.
  * Default branch: try `git -C <cache> symbolic-ref -q refs/remotes/origin/HEAD`. If unset, set it using `ls-remote --symref` result.
  * Resolve commit for branch/tag/SHA with `git -C <cache> rev-parse`.
* **Skill discovery**: walk cache repo for `**/SKILL.md` under `<repo>`, ignore `.git` using `ignore` crate; parse front‑matter delimited by a leading `---` block; `serde_yaml` into `SkillMeta`.
* **Copying & digest**:

  * Copy from cache `<commit>`’s tree: use `git -C <cache> checkout <commit>` into a temp worktree or `git show`/`git archive` for the subdir, then extract/copy to project `<root>/<installName>`.
  * Compute SHA‑256 over *sorted* relative paths and file bytes; exclude `.git`, editor temp files.
* **Modified detection**: recompute digest and compare to lock value.
* **Idempotency**: re‑running commands should be safe (no duplicate lock entries; stable sorting).

---

## Module layout

```
src/
  main.rs
  cli.rs
  config.rs          // user config load/save
  paths.rs           // cache + project path helpers
  git.rs             // wrappers for git commands
  skills.rs          // SKILL.md parsing, discovery
  lock.rs            // load/save/update lockfile
  digest.rs          // tree hashing
  install.rs
  update.rs
  upgrade.rs
  doctor.rs
  status.rs
  sync.rs
```

---

## Tests (integration via tempdirs; no network flakiness)

* `init_creates_roots_and_empty_lock`.
* `install_installs_and_locks_commit`: install a known sample skill from a local bare repo fixture; lock contains commit and digest.
* `status_dirty_after_edit`: modify a file under `skills/<name>`; status shows `modified`; `upgrade` refuses.
* `update_is_cache_only`: after advance remote, `sk update` fetches cache; project files untouched.
* `upgrade_dry_run_reports_changes`: after `update`, `sk upgrade --dry-run` reports `old → new` without file writes.
* `upgrade_applies_when_clean`: `sk upgrade` copies new contents, updates lock.
* `remove_refuses_when_modified` then `--force` works.
* `doctor_apply_rebuilds_missing_from_locked_commit`.
* `sync_back_pushes_to_branch_when_writable` (use local file:// remote).

---

## DX & packaging

* `cargo run -- <args>` locally; `cargo build --release` for binaries.
* `cargo fmt`, `clippy`, and GitHub Actions CI (linux/macos/windows).
* Optional `cargo-dist` to package archives.

---

## Error messages (exact, actionable)

* Modified refusal (upgrade):
  `Local edits in 'skills/<name>' detected (digest mismatch). Refusing to upgrade. Fork and repoint, or run 'sk sync-back <name>' if you have push access.`

* Ambiguous skill name:
  `Multiple skills named '<name>' found in <repo>. Re-run with --path one of: <list-of-subdirs>.`

* Missing SKILL.md:
  `'<subdir>/SKILL.md' not found or invalid. A Claude Skill must contain SKILL.md with 'name' and 'description'.`

---

## Beads task tree (seed with `bd`)

* **EPIC**: CLI skeleton & config

  * Clap plumbing, config load/save, path resolution
* **EPIC**: Git cache & default-branch detection

  * Clone/fetch, `origin/HEAD` handling, rev-parse
* **EPIC**: Skill discovery & validation

  * Walk, parse front‑matter, resolve conflicts
* **EPIC**: Install & lockfile

  * Copy + digest, lock read/write, idempotency
* **EPIC**: Status/Check

  * Digest recompute, JSON output
* **EPIC**: Update (cache only)

  * Fetch all repos in lock; never touch project
* **EPIC**: Upgrade/Remove

  * Dry‑run planning; refusal on modified; apply path
* **EPIC**: Sync‑back

  * Branch/worktree in cache; rsync; commit/push; PR hint
* **EPIC**: Doctor

  * Detect & optionally repair; prune cache
* **EPIC**: Tests & CI

  * Fixtures, temp repos, matrix build & release

---

### Quickstart (expected UX)

```bash
sk init
sk install @anthropics/skills template-skill --path examples/template-skill
sk list
sk update                      # cache-only fetch
sk upgrade --dry-run           # see if anything is out of date
sk upgrade --all               # apply upgrades for clean skills
sk check
sk status
```

That’s the full brief.
