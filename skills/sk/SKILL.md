---
name: sk
description: "How to use the repo-scoped sk CLI to manage Claude Skills in this codebase."
---

# Using the `sk` CLI

`sk` is the repo-scoped Skills manager that installs, audits, and syncs Claude Skills listed in `skills.lock.json`. Treat it like Cargo for skills: installs live under `./skills`, while the cache stays in `~/.cache/sk/repos`.

## When to reach for this skill
- You need to install or upgrade a skill from the canonical skills repo (`https://github.com/lambdamechanic/skills`).
- You want to verify the working tree is clean (`sk status`, `sk check`).
- You are preparing to sync edits back to the upstream skills repository (`sk sync-back <install>`).

## Core workflow
1. **Refresh the cache** (optional but fast):
   ```bash
   target/debug/sk update
   ```
2. **Install or upgrade a skill** (alias keeps folder names stable):
   ```bash
   target/debug/sk install https://github.com/lambdamechanic/skills testing-patterns --alias testing
   target/debug/sk upgrade testing
   ```
3. **Audit local installs** before landing work:
   ```bash
   target/debug/sk status --json   # detect dirty trees vs lockfile
   target/debug/sk check --json    # shows pending upgrades or cache drift
   ```
4. **Sync edits upstream** after modifying a skill under `./skills/<name>`:
   ```bash
   target/debug/sk sync-back <name> --message "Describe the change"
   ```
   This creates a temp branch in the cached repo, copies your edited skill directory, commits, and pushes. Use the printed `gh pr create --fill --head ...` hint to open a PR on `lambdamechanic/skills`.
5. **Publish a brand-new skill** that doesn’t exist upstream yet:
   ```bash
   target/debug/sk sync-back <name> \
     --repo https://github.com/lambdamechanic/skills \
     --skill-path <subdir> --message "Add <name> skill"
   ```
   Provide the repo (`--repo`) and destination subdirectory (`--skill-path`). `sk` will clone the repo, branch from the default tip, add your local folder, push, and update `skills.lock.json` with the new SHA/digest so status checks stay clean.

## Guardrails
- Always run `bd update` / `bd close` so `.beads/issues.jsonl` matches any skill changes.
- Never edit `skills.lock.json` by hand. Let `sk install`, `sk upgrade`, or `sk remove` update it; commit the lockfile alongside the skill changes.
- If `sk status` reports `dirty`, fix the local tree before running `sk upgrade` or `sk sync-back` to avoid partial syncs.
- When creating a new skill, always specify the upstream repo/path so `sk` can register it and refresh the lockfile automatically.
- Use `sk precommit` (once implemented) to block local-only sources such as `file://` entries.

## Debug tips
- `target/debug/sk doctor --apply` rebuilds missing installs or cache entries.
- `target/debug/sk where <name>` prints the absolute path of an installed skill and its cached repo clone.
- Set `SK_TRACE=1` to surface extra logging when diagnosing cache fetches or rsync failures.
