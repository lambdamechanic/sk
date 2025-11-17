# sk Agent Quickstart

You are reading the machine-facing quickstart that Codex (or any MCP-capable agent) can fetch via the `sk://quickstart` resource. Humans should rely on `README.md`; this document is specifically tuned for automated assistants that need deterministic steps.

## Audience & Goals
- Repo-scoped agents working from `/home/mark/lambdalabs/sk`.
- Keep the local `skills/` tree synchronized with upstream catalogs and skill edits.
- Always advertise the available skills to the user by calling the MCP server.
- Leave the repo clean (git + bd) after every change.

## 1. Bootstrap the CLI
```bash
cargo install sk               # installs/updates the CLI on PATH
sk --version                   # confirm the binary works
```

- Run everything from the repo root so `sk` can find `.git`.
- Respect `SK_CACHE_DIR` / `SK_CONFIG_DIR` overrides if the user sets them.

## 2. Initialize the repo copy
```bash
sk init                        # idempotent; sets up ./skills + lockfile
sk repo add @anthropics/skills --alias anthropic
sk repo list
```

- `sk init` refuses to clobber edits; report any dirty tree instead of deleting files.
- Keep `skills.lock.json` under version control at all times.

## 3. Install representative skills
```bash
sk install @anthropics/skills template-skill --alias template
sk install @anthropics/skills landing-the-plane
sk list
```

- Prefer explicit aliases so future instructions can reference stable directory names.
- Capture stdout/stderr (or summarize it) in your response so humans see what changed.

## 4. MCP ritual every session
1. Start in repo root, ensure `sk mcp-server` is registered (see README snippet).
2. Call `skills_list` once to enumerate helpers; mention any relevant skill names in your reasoning.
3. Use `skills_search` when you need targeted guidance (e.g., `{"query":"bd ready"}`).
4. Use `skills_show` to pull the full body before summarizing instructions for the user.

> Never skip this ritual unless the repo explicitly states there are zero vendored skills. MCP calls are read-only—they exist purely to surface instructions. All edits to `skills/` must flow through the `sk` CLI plus normal git/bd tracking.

## 5. Daily workflow loop
1. **Plan** — consult `AGENTS.md` + skills, run `bd ready --json`, and note which issue you are working on.
2. **Modify** — run `sk ...` commands as needed (install, upgrade, sync-back).
3. **Validate** — `cargo fmt`, `cargo test`, or any repo-specific guardrails.
4. **Track** — update the relevant bd issue (`bd update <id> --status in_progress`, later `--status closed`).
5. **Summarize** — describe what changed, reference files + line numbers, and mention any remaining risks.

Keep `git status -sb` clean; never leave throwaway files or stash state behind.

## 6. Publishing your own skills
```bash
sk config set default_repo @your-gh-username/skills
sk template create new-helper "Short description"
# edit files under skills/new-helper/ ...
sk sync-back new-helper -m "Explain the change"
```

- `sk sync-back` shells out to `gh` for PR automation; ensure the CLI is authenticated (`gh auth status`).
- If `rsync` is missing, the command falls back to a slower recursive copy—call that out in your notes so the user can install it later.
- Commit `.beads/issues.jsonl` together with any skill edits so tracker state stays synchronized.

## 7. Keeping caches healthy
```bash
sk update                      # refresh cached repos (safe on CI)
sk doctor --status --json      # detect dirty installs or pending upgrades
sk doctor --summary --json     # structural integrity, digests, cache drift
sk doctor --apply              # rebuild installs/caches when corruption is detected
```

- Prefer `sk upgrade --all` only when `skills/` is clean (no local edits). Otherwise, upgrade specific installs after syncing them back upstream.
- When `sk doctor --status` reports `modified`, either `sk sync-back <skill>` or `sk remove <skill>` (if the user asked you to discard the work).

## 8. Recovery playbook
- **Interrupted upgrade/install** — re-run the command; the lockfile ensures the CLI is idempotent.
- **Git conflict in `skills/`** — describe the conflicting files, ask before overwriting.
- **bd mismatch** — run `bd update <id> --status in_progress` and commit the resulting `.beads/issues.jsonl` change.
- **MCP call failure** — restart `sk mcp-server` from repo root and re-issue `skills_list`.

## 9. Session shutdown checklist
- `cargo test` (or the requested subset) passes locally.
- `sk doctor --status --json` reports all installs clean.
- `git status -sb` is clean; no staged-but-uncommitted files.
- The relevant bd issue is updated/closed, and `.beads/issues.jsonl` is committed.
- Mention the latest `skills.lock.json` diff and any remaining TODOs in your final response.

Stay disciplined about these steps and future agents can pick up the repo without surprises.
